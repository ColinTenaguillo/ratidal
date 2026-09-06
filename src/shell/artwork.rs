//! Cover art, fetched once and cached.
//!
//! Detection happens once at startup: kitty, iTerm2 and sixel give real
//! images, and everything else falls back to half blocks — each cell carrying
//! two pixels through its foreground and background colour. That fallback is
//! why the layout never assumes an image is available: on Alacritty and the
//! VTE terminals, blocks are all anyone will ever see.

use std::collections::HashMap;

use ratatui::layout::Rect;
use ratatui::Frame;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::ResizeEncodeRender;
use tokio::sync::mpsc::UnboundedSender;

/// How a cover is drawn. An artist avatar is a circle in the web client, and
/// a square photo in a row of circles is the thing people notice first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Shape {
    #[default]
    Square,
    Round,
}

/// A cover that finished downloading and decoding.
pub struct Loaded {
    pub url: String,
    pub shape: Shape,
    protocol: Option<Box<StatefulProtocol>>,
}

impl std::fmt::Debug for Loaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loaded")
            .field("url", &self.url)
            .field("decoded", &self.protocol.is_some())
            .finish()
    }
}

enum Entry {
    /// A request is in flight; do not start another for the same URL.
    Loading,
    Ready(Box<StatefulProtocol>),
    /// The fetch or the decode failed. Remembered so we stop retrying it every
    /// frame — at ~30fps a failing URL would otherwise hammer the network.
    Failed,
}

impl std::fmt::Debug for Artwork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Artwork")
            .field("cached", &self.cache.len())
            .field("protocol", &self.picker.protocol_type())
            .finish()
    }
}

pub struct Artwork {
    cache: HashMap<(String, Shape), Entry>,
    picker: Picker,
    http: reqwest::Client,
    tx: UnboundedSender<Loaded>,
}

impl Artwork {
    /// Probe the terminal for an image protocol.
    ///
    /// Must run BEFORE raw mode and the alternate screen: the query writes an
    /// escape to stdout and reads the reply from stdin, which the alternate
    /// screen would swallow. `main` calls this and hands the result to `run`.
    pub fn probe() -> Picker {
        match Picker::from_query_stdio() {
            Ok(p) => {
                tracing::info!("terminal image protocol: {:?}", p.protocol_type());
                p
            }
            Err(e) => {
                // Not a failure. Most terminals answer nothing — Alacritty
                // among them — and half blocks are the normal case rather
                // than a degraded one: each cell carries two pixels through
                // its foreground and background colour, which is coarse but
                // is a picture rather than a grey rectangle.
                tracing::info!("no image protocol ({e}); covers will render as half blocks");
                Picker::halfblocks()
            }
        }
    }

    pub fn with_picker(picker: Picker, tx: UnboundedSender<Loaded>) -> Self {
        Self {
            cache: HashMap::new(),
            picker,
            http: reqwest::Client::new(),
            tx,
        }
    }

    /// Draw the cover for `url` into `area`, if it is decoded and ready.
    /// Returns false when the caller should draw its own placeholder.
    /// Draw the cover for `url`, masked to `shape`.
    ///
    /// The same URL can be wanted both ways — an album cover stands in for a
    /// missing artist portrait — so the cache is keyed by shape as well, or
    /// the first view to ask would decide how it looked everywhere.
    pub fn render_shaped(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        url: &str,
        shape: Shape,
    ) -> bool {
        if area.width == 0 || area.height == 0 {
            return false;
        }
        self.request_shaped(url, shape);
        let full = Rect {
            height: square_rows(area.width, self.picker.font_size()).max(area.height),
            ..area
        };
        match self.cache.get_mut(&(url.to_string(), shape)) {
            Some(Entry::Ready(protocol)) => {
                // Encoded for the height the cover wants, then drawn into the
                // height it was given. A cover at the fold is cut off rather
                // than shrunk: every protocol here stops at `area.height`
                // (half blocks skip the cells, kitty stops emitting rows), so
                // the visible part stays the same size as a whole row's.
                if let Some(size) = protocol.needs_resize(&cover_resize(), full.into()) {
                    protocol.resize_encode(&cover_resize(), size);
                }
                protocol.render(area, frame.buffer_mut());
                true
            }
            _ => false,
        }
    }

}

/// How a cover is fitted to its cells.
///
/// Always `Fit`, which keeps the image's proportions. Cropping was tried to
/// square up a card's shading and it discards real artwork: the cell area is
/// only square when a cell is exactly twice as tall as it is wide, and on a
/// terminal whose cells are 19x30 it takes 22% off the height of every
/// cover. A margin that is a column out is worth less than that.
fn cover_resize() -> ratatui_image::Resize {
    ratatui_image::Resize::Fit(None)
}

/// How many rows a square cover `cols` wide needs, at this cell size.
///
/// A cover is square in pixels, not in cells: a cell here is 19x30, so eight
/// rows of it are as tall as thirteen columns are wide. The rows a cover
/// wants are what it is encoded for, whatever number of them it is then
/// given to draw into.
fn square_rows(cols: u16, cell: ratatui_image::FontSize) -> u16 {
    if cell.height == 0 {
        return cols;
    }
    ((cols * cell.width) / cell.height).max(1)
}

impl Artwork {
    /// Start a fetch if this URL has never been seen in this shape.
    fn request_shaped(&mut self, url: &str, shape: Shape) {
        let key = (url.to_string(), shape);
        if self.cache.contains_key(&key) {
            return;
        }
        self.cache.insert(key, Entry::Loading);
        let picker = self.picker.clone();
        let (url, http, tx) = (url.to_string(), self.http.clone(), self.tx.clone());

        tokio::spawn(async move {
            let protocol = fetch_and_decode(&http, &picker, &url, shape).await;
            let _ = tx.send(Loaded { url, shape, protocol });
        });
    }

    /// Take a finished download into the cache.
    pub fn insert(&mut self, loaded: Loaded) {
        let entry = match loaded.protocol {
            Some(p) => Entry::Ready(p),
            None => Entry::Failed,
        };
        self.cache.insert((loaded.url, loaded.shape), entry);
    }
}

async fn fetch_and_decode(
    http: &reqwest::Client,
    picker: &Picker,
    url: &str,
    shape: Shape,
) -> Option<Box<StatefulProtocol>> {
    let bytes = http.get(url).send().await.ok()?.bytes().await.ok()?;
    let picker = picker.clone();

    // Decoding is CPU-bound and would stall the runtime's worker otherwise.
    tokio::task::spawn_blocking(move || {
        let image = image::load_from_memory(&bytes).ok()?;
        let image = match shape {
            Shape::Square => image,
            Shape::Round => round_off(image),
        };
        Some(Box::new(picker.new_resize_protocol(image)))
    })
    .await
    .ok()?
}

/// Make the corners of a square image transparent, leaving a circle.
///
/// Masking has to happen here rather than over the drawn cells: the image
/// protocols write pixels straight to the terminal, so nothing painted
/// afterwards can cut a shape out of them.
fn round_off(image: image::DynamicImage) -> image::DynamicImage {
    use image::GenericImageView as _;

    let (w, h) = image.dimensions();
    if w == 0 || h == 0 {
        return image;
    }

    let mut rgba = image.to_rgba8();
    let (cx, cy) = ((w as f32 - 1.0) / 2.0, (h as f32 - 1.0) / 2.0);
    let radius = cx.min(cy);

    for (x, y, px) in rgba.enumerate_pixels_mut() {
        let (dx, dy) = (x as f32 - cx, y as f32 - cy);
        let d = (dx * dx + dy * dy).sqrt();
        if d > radius {
            px.0[3] = 0;
        } else if d > radius - 1.0 {
            // One pixel of feathering, so the edge is not a staircase.
            px.0[3] = ((radius - d).clamp(0.0, 1.0) * px.0[3] as f32) as u8;
        }
    }

    image::DynamicImage::ImageRgba8(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cover_is_never_cropped() {
        // Cropping squares up a card's shading by throwing away artwork.
        // The cell area is only square when a cell is exactly twice as tall
        // as it is wide; on a 19x30 cell it costs 22% of every cover's
        // height, and the round ones came out flattened along the bottom.
        assert!(
            matches!(cover_resize(), ratatui_image::Resize::Fit(_)),
            "covers keep their proportions"
        );
    }

    #[test]
    fn a_cover_at_the_fold_is_cut_not_shrunk() {
        // The report this came from: the grid's bottom row drew smaller
        // covers rather than covers cut off at the pane's edge. Handing the
        // widget the short area resizes the whole image down to it. So the
        // rows a cover is encoded for and the rows it is drawn into are
        // decided separately, and the visible rows must come out the same as
        // a whole row's — pixel for pixel, not merely similar.
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        // Bigger than the area it is drawn into, or `Fit` leaves it alone
        // and both renders come out the same however the size is worked out.
        let mut img = image::RgbImage::new(640, 640);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = image::Rgb([(x / 3) as u8, (y / 3) as u8, 128]);
        }
        let image = image::DynamicImage::ImageRgb8(img);

        // Half blocks are the fallback every terminal has, and the only
        // protocol whose output lands in the buffer where a test can read it.
        let render = |rows: u16| {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            let mut art = Artwork {
                cache: HashMap::new(),
                picker: Picker::halfblocks(),
                http: reqwest::Client::new(),
                tx,
            };
            let protocol = art
                .picker
                .new_resize_protocol(image.clone());
            art.cache.insert(
                ("u".to_string(), Shape::Square),
                Entry::Ready(Box::new(protocol)),
            );

            let mut term = Terminal::new(TestBackend::new(16, 8)).unwrap();
            term.draw(|f| {
                let area = Rect { x: 0, y: 0, width: 16, height: rows };
                assert!(art.render_shaped(f, area, "u", Shape::Square));
            })
            .unwrap();
            let buf = term.backend().buffer().clone();
            buf
        };

        let whole = render(8);
        let cut = render(5);

        for y in 0..5 {
            for x in 0..16 {
                assert_eq!(
                    (cut[(x, y)].fg, cut[(x, y)].bg),
                    (whole[(x, y)].fg, whole[(x, y)].bg),
                    "row {y} column {x} must be the same ink as in a whole row"
                );
            }
        }
    }

    #[test]
    fn a_square_cover_is_as_tall_as_it_is_wide() {
        use ratatui_image::FontSize;

        // On this machine a cell is 19x30, so thirteen columns of cover are
        // eight rows of it. Getting this wrong is what shrinks the artwork:
        // encode for too few rows and the image is scaled down to them.
        assert_eq!(square_rows(13, FontSize { width: 19, height: 30 }), 8);
        // A cell exactly twice as tall as it is wide is the textbook case.
        assert_eq!(square_rows(16, FontSize { width: 10, height: 20 }), 8);
        // Never zero, whatever the numbers say.
        assert_eq!(square_rows(1, FontSize { width: 10, height: 20 }), 1);
        assert_eq!(square_rows(9, FontSize { width: 8, height: 0 }), 9);
    }

    #[test]
    fn a_failed_url_is_not_retried() {
        // Without this the render loop would re-request a broken cover on
        // every frame, ~30 times a second.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut art = Artwork {
            cache: HashMap::new(),
            picker: Picker::halfblocks(),
            http: reqwest::Client::new(),
            tx,
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            art.request_shaped("https://example.invalid/cover.jpg", Shape::Square);
            art.request_shaped("https://example.invalid/cover.jpg", Shape::Square);
        });

        assert_eq!(art.cache.len(), 1, "the same URL must be recorded once");
    }

    #[test]
    fn a_terminal_with_no_protocol_still_gets_half_blocks() {
        // This is the whole point of the fallback: covers used to become
        // plain grey rectangles on every terminal that answered no protocol,
        // which is most of them, Alacritty included. Half blocks carry two
        // pixels per cell — coarse, but a picture.
        use ratatui_image::picker::ProtocolType;
        assert_eq!(
            Picker::halfblocks().protocol_type(),
            ProtocolType::Halfblocks
        );
    }

    #[test]
    fn half_blocks_actually_draw_the_picture() {
        // The failure this came from: with no protocol, every cover became
        // one flat grey rectangle. A fallback that paints a solid block is
        // not a fallback. This decodes a real gradient and checks the cells
        // differ from each other.
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use ratatui_image::{Image, Resize};

        let mut img = image::RgbImage::new(64, 64);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = image::Rgb([(x * 4) as u8, (y * 4) as u8, 128]);
        }
        let image = image::DynamicImage::ImageRgb8(img);

        let picker = Picker::halfblocks();
        let protocol = picker
            .new_protocol(image, ratatui::layout::Size::new(16, 8), Resize::Fit(None))
            .expect("halfblocks must encode any image");

        let mut term = Terminal::new(TestBackend::new(16, 8)).unwrap();
        term.draw(|f| f.render_widget(Image::new(&protocol), f.area()))
            .unwrap();

        let buf = term.backend().buffer();
        let colours: std::collections::HashSet<_> =
            (0..8).flat_map(|y| (0..16).map(move |x| (x, y)))
                .map(|(x, y)| format!("{:?}/{:?}", buf[(x, y)].fg, buf[(x, y)].bg))
                .collect();
        assert!(
            colours.len() > 8,
            "a gradient must produce many distinct cells, got {}",
            colours.len()
        );
    }

    #[test]
    fn rounding_clears_the_corners_and_keeps_the_middle() {
        // A square photo in a row of circles is the thing people notice
        // first, and masking cannot happen after the fact: the image
        // protocols write pixels straight to the terminal.
        use image::GenericImageView as _;

        let mut img = image::RgbaImage::new(64, 64);
        for px in img.pixels_mut() {
            *px = image::Rgba([200, 100, 50, 255]);
        }
        let out = round_off(image::DynamicImage::ImageRgba8(img));

        assert_eq!(out.get_pixel(0, 0).0[3], 0, "the top-left corner must be cut away");
        assert_eq!(out.get_pixel(63, 0).0[3], 0, "and the top-right");
        assert_eq!(out.get_pixel(0, 63).0[3], 0);
        assert_eq!(out.get_pixel(63, 63).0[3], 0);

        assert_eq!(out.get_pixel(32, 32).0[3], 255, "the middle must be untouched");
        assert_eq!(out.get_pixel(32, 2).0[3], 255, "and so must the top edge's centre");
        assert_eq!(out.get_pixel(2, 32).0[3], 255);

        // The colour must survive; only the alpha changes.
        assert_eq!(&out.get_pixel(32, 32).0[..3], &[200, 100, 50]);
    }

    #[test]
    fn the_transparency_survives_encoding() {
        // Masking is pointless if the pipeline flattens alpha: the corners
        // would come back as black squares, which is worse than leaving the
        // photo square. iTerm2 and kitty carry PNG, which keeps it.
        use image::GenericImageView as _;

        let mut img = image::RgbaImage::new(64, 64);
        for px in img.pixels_mut() {
            *px = image::Rgba([200, 100, 50, 255]);
        }
        let rounded = round_off(image::DynamicImage::ImageRgba8(img));

        let mut png: Vec<u8> = Vec::new();
        rounded
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode");
        let back = image::load_from_memory(&png).expect("decode");

        assert_eq!(back.get_pixel(0, 0).0[3], 0, "the corner must still be transparent");
        assert_eq!(back.get_pixel(32, 32).0[3], 255);
    }

    #[test]
    fn rounding_a_degenerate_image_does_not_panic() {
        for (w, h) in [(0u32, 0u32), (1, 1), (1, 64), (64, 1), (3, 2)] {
            let img = image::RgbaImage::new(w, h);
            let _ = round_off(image::DynamicImage::ImageRgba8(img));
        }
    }

    #[test]
    fn the_same_cover_can_be_held_both_square_and_round() {
        // An album cover stands in for a missing artist portrait, so one URL
        // is wanted both ways. A cache keyed by URL alone would let whichever
        // view asked first decide how it looked everywhere.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut art = Artwork {
            cache: HashMap::new(),
            picker: Picker::halfblocks(),
            http: reqwest::Client::new(),
            tx,
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            art.request_shaped("https://example.invalid/a.jpg", Shape::Square);
            art.request_shaped("https://example.invalid/a.jpg", Shape::Round);
        });
        assert_eq!(art.cache.len(), 2, "one URL, two shapes, two entries");
    }

    #[test]
    fn a_cover_is_requested_once_and_then_awaited() {
        // Before the fallback existed, a missing protocol marked every URL
        // Failed without fetching. Now the request goes out.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut art = Artwork {
            cache: HashMap::new(),
            picker: Picker::halfblocks(),
            http: reqwest::Client::new(),
            tx,
        };
        // No runtime here, so the spawn would panic; drive `request` inside
        // one and only check what it recorded.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            art.request_shaped("https://example.invalid/a.jpg", Shape::Square);
        });
        assert!(
            matches!(art.cache.get(&("https://example.invalid/a.jpg".to_string(), Shape::Square)), Some(Entry::Loading)),
            "a cover must be fetched, not written off"
        );
    }
}
