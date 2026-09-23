//! Cover art, fetched once and cached.
//!
//! Detection happens once at startup: kitty, iTerm2 and sixel give real
//! images, and everything else falls back to half blocks — each cell carrying
//! two pixels through its foreground and background colour. That fallback is
//! why the layout never assumes an image is available: on Alacritty and the
//! VTE terminals, blocks are all anyone will ever see.

use std::collections::HashMap;

use ratatui::layout::{Rect, Size};
use ratatui::Frame;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::StatefulImage;
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
    decoded: Option<Decoded>,
}

/// A decoded cover: the protocol that draws it whole, and the picture it
/// came from.
///
/// The picture is kept because a cover at the fold cannot be drawn by
/// handing the protocol fewer rows — see `Ready::cut`.
struct Decoded {
    protocol: Box<StatefulProtocol>,
    image: image::DynamicImage,
}

impl std::fmt::Debug for Loaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loaded")
            .field("url", &self.url)
            .field("decoded", &self.decoded.is_some())
            .finish()
    }
}

enum Entry {
    /// A request is in flight; do not start another for the same URL.
    Loading,
    Ready(Ready),
    /// The fetch or the decode failed. Remembered so we stop retrying it every
    /// frame — at ~30fps a failing URL would otherwise hammer the network.
    Failed,
}

/// A cover ready to draw, whole or cut off at the fold.
struct Ready {
    whole: Box<StatefulProtocol>,
    image: image::DynamicImage,
    /// The cut encoding and the area it was made for. A view has one row at
    /// the fold and one card at the right-hand edge, so this is at most one
    /// extra encoding per cover.
    cut: Option<(Size, ratatui_image::protocol::Protocol)>,
}

impl Ready {
    /// The encoding for this cover shown `rows` deep where a whole row would
    /// have given it `whole`.
    ///
    /// Neither of the library's resize modes does this. `Fit` scales the
    /// entire picture into the rows that are left, which is the miniature
    /// the fold row used to show; `Crop` takes a corner of the *source* at
    /// one pixel per pixel, which is a fragment of the artwork rather than
    /// the top of it. What is wanted is the picture at the size a whole row
    /// would have drawn it, with the bottom cut off — so the pixels are
    /// resized and cut here, and the protocol is handed an image that
    /// already fits the area exactly.
    ///
    /// Fitting the area exactly is not only tidiness: iTerm2 and sixel draw
    /// nothing at all when the encoding is larger than the area they are
    /// given, so an encoding that overhangs does not clip, it disappears.
    /// `cols` by `rows` is the area to draw into; `whole` is the square the
    /// cover would have filled had it been given a whole card. Either side
    /// can be short of it: a row at the bottom of a grid is cut in height,
    /// the last card of a carousel is cut in width.
    fn cut(
        &mut self,
        rows: u16,
        cols: u16,
        whole: Size,
        picker: &Picker,
    ) -> Option<&ratatui_image::protocol::Protocol> {
        let want = Size::new(cols, rows);
        if self.cut.as_ref().map(|(size, _)| *size) != Some(want) {
            let cell = picker.font_size();
            // The cells the whole cover fills, as the whole path works them
            // out: a square in a column wider than it is tall stops short of
            // the column's edge. Encoded to the full width instead, the cut
            // stretched the picture over the spare column and the cut row's
            // artwork sat to the right of the rows above it.
            let fitted = cover_resize().size_for(&self.image, cell, whole);
            let keep = Size::new(cols.min(fitted.width), rows.min(fitted.height));
            if keep.width == 0 || keep.height == 0 || cell.width == 0 || cell.height == 0 {
                return None;
            }
            let scaled = cover_resize().resize(&self.image, cell, fitted, None);
            let cut = scaled.crop_imm(
                0,
                0,
                u32::from(keep.width) * u32::from(cell.width),
                u32::from(keep.height) * u32::from(cell.height),
            );
            // `new_protocol` leaves an image that already matches the area
            // alone: the resize it would do is the one just done by hand.
            let encoded = picker
                .new_protocol(cut, keep, ratatui_image::Resize::Fit(None))
                .ok()?;
            self.cut = Some((want, encoded));
        }
        self.cut.as_ref().map(|(_, p)| p)
    }
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
        // An escape hatch for a terminal whose image support is broken.
        // WezTerm 20240203 answers the capability query saying it speaks
        // iTerm2, then draws the first row's covers in the wrong place --
        // the row is left blank and the page reads as half drawn. The same
        // page is correct on a terminal that draws no images at all, so
        // half blocks are the honest fallback rather than a downgrade.
        //
        // The setting is read straight from the file rather than handed in:
        // this runs before raw mode, which is before `run` has loaded the
        // config, and a terminal that draws covers wrong should not need an
        // environment variable remembered at every launch. The variable is
        // kept for a one-off run without editing the file.
        let by_config = crate::config::Config::load()
            .map(|c| c.ui.halfblocks)
            .unwrap_or(false);
        let by_env = std::env::var("RATIDAL_HALFBLOCKS").is_ok_and(|v| v != "0");
        if by_config || by_env {
            tracing::info!(
                "half blocks asked for ({}); covers will not use an image protocol",
                if by_env {
                    "RATIDAL_HALFBLOCKS"
                } else {
                    "ui.halfblocks"
                }
            );
            return Picker::halfblocks();
        }
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

    /// Draw the cover for `url`, masked to `shape`.
    ///
    /// The same URL can be wanted both ways — an album cover stands in for a
    /// missing artist portrait — so the cache is keyed by shape as well, or
    /// the first view to ask would decide how it looked everywhere.
    ///
    /// Returns false when the caller should draw its own placeholder.
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
        // A cover is square, so whichever side of the area is the larger in
        // pixels is the one that was not cut: a grid's bottom row keeps its
        // width and loses height, a carousel's last card keeps its height
        // and loses width. The whole cover is the square that side implies,
        // and an area that is already square is not cut at all.
        let cell = self.picker.font_size();
        let by_width = Size::new(area.width, square_rows(area.width, cell));
        let by_height = Size::new(square_cols(area.height, cell), area.height);
        let whole = if by_width.height >= by_height.height {
            by_width
        } else {
            by_height
        };
        self.render_cut(frame, area, url, shape, whole)
    }

    /// Draw the cover for `url` into `area`, showing only as much of it as
    /// fits when `area` is smaller than `whole`.
    ///
    /// `whole` is the area this cover would have had were it not at the edge
    /// of the pane: a grid's bottom row is short in height, a carousel's
    /// last card is short in width. What is drawn is the top-left of the
    /// cover at the size it would have been, cut — not the whole picture
    /// squeezed into what is left.
    pub fn render_cut(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        url: &str,
        shape: Shape,
        whole: Size,
    ) -> bool {
        if area.width == 0 || area.height == 0 {
            return false;
        }
        self.request_shaped(url, shape);
        let picker = self.picker.clone();
        let stencil = shape == Shape::Round
            && picker.protocol_type() == ratatui_image::picker::ProtocolType::Halfblocks;
        // What is under the cover before it is drawn: a selected card's
        // band, or nothing. The pixels the stencil takes out show it again,
        // so the band goes round the circle rather than stopping at the
        // square it would have been.
        let under: Vec<ratatui::style::Color> = if stencil {
            let buf = frame.buffer_mut();
            (0..area.height)
                .flat_map(|r| (0..area.width).map(move |c| (c, r)))
                .map(|(c, r)| {
                    buf.cell((area.x + c, area.y + r))
                        .map_or(ratatui::style::Color::Reset, |cell| cell.bg)
                })
                .collect()
        } else {
            Vec::new()
        };
        match self.cache.get_mut(&(url.to_string(), shape)) {
            Some(Entry::Ready(ready)) => {
                // What the cover fills, for the mask below: the library's
                // own answer, since `Fit` never scales a picture up and a
                // small one fills less than the area.
                let mut drawn = None;
                if area.height < whole.height || area.width < whole.width {
                    if let Some(p) = ready.cut(area.height, area.width, whole, &picker) {
                        frame.render_widget(ratatui_image::Image::new(p), area);
                        drawn = Some(whole);
                    }
                }
                let drawn = drawn.unwrap_or_else(|| {
                    let size = ready.whole.size_for(cover_resize(), area.into());
                    frame.render_stateful_widget(
                        StatefulImage::default().resize(cover_resize()),
                        area,
                        ready.whole.as_mut(),
                    );
                    size
                });
                // Half blocks carry no alpha, so the circle is cut out of
                // the cells rather than the pixels: a fixed stencil, the
                // same for every cover of this size.
                if stencil {
                    clear_round_corners(frame.buffer_mut(), area, drawn, &under);
                }
                true
            }
            _ => false,
        }
    }
}

/// Whether the two half-block pixels of cell `(c, r)` are inside the
/// circle inscribed in a square `drawn` cells big, drawn from the origin:
/// the upper, then the lower. A pixel is in when its centre is.
///
/// One stencil for everything round in half blocks -- the covers and the
/// disc that stands in for a missing one -- so the two are the same shape
/// and a card does not change outline when its picture lands.
pub(super) fn round_stencil(drawn: Size, c: u16, r: u16) -> (bool, bool) {
    let (w, h) = (
        f32::from(drawn.width.max(1)),
        f32::from(drawn.height.max(1)),
    );
    let inside = |u: f32, v: f32| (u - 0.5).powi(2) + (v - 0.5).powi(2) <= 0.25;
    let u = (f32::from(c) + 0.5) / w;
    (
        inside(u, (f32::from(r) + 0.25) / h),
        inside(u, (f32::from(r) + 0.75) / h),
    )
}

/// Cut a circle out of a half-block cover: the stencil `round_off` is for
/// the pixel protocols.
///
/// A half block is one cell holding two pixels, the upper in its foreground
/// and the lower in its background. The circle is centred and touches the
/// sides of the square the cover fills, which is `drawn` cells from the
/// top-left of `area`; a pixel is in when its centre is. A cell with both
/// pixels out goes back to what was under it -- `under`, one background
/// per cell of `area`, row by row -- and one with a single pixel out keeps
/// the other over that background. Decided by geometry alone, so every
/// cover of a size gets the same circle.
fn clear_round_corners(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    drawn: Size,
    under: &[ratatui::style::Color],
) {
    if drawn.width == 0 || drawn.height == 0 {
        return;
    }
    for r in 0..area.height {
        for c in 0..area.width {
            let (upper_in, lower_in) = round_stencil(drawn, c, r);
            let (up, lo) = (!upper_in, !lower_in);
            if !up && !lo {
                continue;
            }
            let Some(cell) = buf.cell_mut((area.x + c, area.y + r)) else {
                continue;
            };
            let (upper, lower) = match cell.symbol() {
                "▀" => (cell.fg, cell.bg),
                "▄" => (cell.bg, cell.fg),
                // Two pixels of one colour: the encoder writes a blank on
                // that background.
                " " if cell.bg != ratatui::style::Color::Reset => (cell.bg, cell.bg),
                // Nothing of the cover is here.
                _ => continue,
            };
            let was = under
                .get(usize::from(r) * usize::from(area.width) + usize::from(c))
                .copied()
                .unwrap_or(ratatui::style::Color::Reset);
            cell.reset();
            cell.set_bg(was);
            if up && !lo {
                cell.set_symbol("▄").set_fg(lower);
            } else if lo && !up {
                cell.set_symbol("▀").set_fg(upper);
            }
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

/// How many rows a square cover `cols` wide fills, at this cell size.
///
/// A cover is square in pixels, not in cells: on a 19x30 cell, eight rows
/// are as tall as thirteen columns are wide. This is what says whether a
/// given area is a whole row or the cut-off one at the fold.
fn square_rows(cols: u16, cell: ratatui_image::FontSize) -> u16 {
    if cell.height == 0 {
        return cols;
    }
    ((cols * cell.width) / cell.height).max(1)
}

/// How many columns a square cover `rows` tall fills — `square_rows` the
/// other way about, for a cover cut at the right-hand edge of a carousel
/// rather than at the bottom of a grid.
fn square_cols(rows: u16, cell: ratatui_image::FontSize) -> u16 {
    if cell.width == 0 {
        return rows;
    }
    ((rows * cell.height) / cell.width).max(1)
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
            let decoded = fetch_and_decode(&http, &picker, &url, shape).await;
            let _ = tx.send(Loaded {
                url,
                shape,
                decoded,
            });
        });
    }

    /// Take a finished download into the cache.
    pub fn insert(&mut self, loaded: Loaded) {
        let entry = match loaded.decoded {
            Some(d) => Entry::Ready(Ready {
                whole: d.protocol,
                image: d.image,
                cut: None,
            }),
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
) -> Option<Decoded> {
    let bytes = http.get(url).send().await.ok()?.bytes().await.ok()?;
    let picker = picker.clone();

    // Decoding is CPU-bound and would stall the runtime's worker otherwise.
    tokio::task::spawn_blocking(move || {
        let image = image::load_from_memory(&bytes).ok()?;
        let image = match shape {
            Shape::Square => image,
            // Half blocks carry no alpha, and the encoder blends the cleared
            // corners into the edge as black: the circle came out ragged
            // and different for every picture. There the picture stays
            // square and `clear_round_corners` cuts the circle out of the
            // cells afterwards, the same stencil every time.
            Shape::Round
                if picker.protocol_type() == ratatui_image::picker::ProtocolType::Halfblocks =>
            {
                image
            }
            Shape::Round => round_off(image),
        };
        Some(Decoded {
            protocol: Box::new(picker.new_resize_protocol(image.clone())),
            image,
        })
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
        // The fold row drew miniatures: handing the widget the short area
        // scales the whole picture into it. What the web client shows is
        // the top of the cover at full size with the bottom cut off, so the
        // visible rows must match a whole row's, pixel for pixel.
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        // Larger than the area, or `Fit` leaves it alone and both renders
        // come out the same however the size was worked out.
        let mut img = image::RgbImage::new(640, 640);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = image::Rgb([(x / 3) as u8, (y / 3) as u8, 128]);
        }
        let image = image::DynamicImage::ImageRgb8(img);

        // Half blocks are the only protocol whose output lands in the
        // buffer where a test can read it back.
        let render = |rows: u16| {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            let mut art = Artwork {
                cache: HashMap::new(),
                picker: Picker::halfblocks(),
                http: reqwest::Client::new(),
                tx,
            };
            art.cache.insert(
                ("u".to_string(), Shape::Square),
                Entry::Ready(Ready {
                    whole: Box::new(art.picker.new_resize_protocol(image.clone())),
                    image: image.clone(),
                    cut: None,
                }),
            );
            let mut term = Terminal::new(TestBackend::new(16, 8)).unwrap();
            term.draw(|f| {
                assert!(art.render_shaped(f, Rect::new(0, 0, 16, rows), "u", Shape::Square));
            })
            .unwrap();
            term.backend().buffer().clone()
        };

        let whole = render(8);
        let cut = render(5);

        // Within a shade: cutting resamples once more than drawing whole
        // does, so a channel can land a step either side. What must not
        // happen is the picture being scaled into the short area, which
        // moves these by tens.
        for y in 0..5 {
            for x in 0..16 {
                let (a, b) = (rgb(cut[(x, y)].fg), rgb(whole[(x, y)].fg));
                let off = a
                    .iter()
                    .zip(b.iter())
                    .map(|(p, q)| p.abs_diff(*q) as u32)
                    .max()
                    .unwrap_or(0);
                assert!(off <= 2, "row {y} column {x}: {a:?} against {b:?} whole");
            }
        }
    }

    #[test]
    fn a_cut_cover_encodes_no_larger_than_its_area() {
        // Why the cut is done on the pixels rather than by handing the
        // protocol a short area: iTerm2 and sixel draw nothing at all when
        // the encoding is larger than the area, so an encoding that
        // overhangs does not clip, the cover vanishes. This machine picked
        // iTerm2, and two attempts died on exactly that.
        let mut img = image::RgbImage::new(640, 640);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = image::Rgb([(x / 3) as u8, (y / 3) as u8, 128]);
        }
        let image = image::DynamicImage::ImageRgb8(img);
        let picker = Picker::halfblocks();

        for rows in 1..8u16 {
            let mut ready = Ready {
                whole: Box::new(picker.new_resize_protocol(image.clone())),
                image: image.clone(),
                cut: None,
            };
            let p = ready
                .cut(rows, 16, Size::new(16, 8), &picker)
                .expect("encodes");

            // Drawn into an area exactly that tall. iTerm2 and sixel bail
            // out and paint nothing when the encoding is bigger than the
            // area, so a cover that comes out blank here is one that would
            // have vanished on those terminals.
            use ratatui::backend::TestBackend;
            use ratatui::Terminal;
            let mut term = Terminal::new(TestBackend::new(16, 8)).unwrap();
            term.draw(|f| {
                f.render_widget(ratatui_image::Image::new(p), Rect::new(0, 0, 16, rows));
            })
            .unwrap();
            let buf = term.backend().buffer();
            let painted = (0..16)
                .flat_map(|x| (0..rows).map(move |y| (x, y)))
                .filter(|&(x, y)| buf[(x, y)].fg != ratatui::style::Color::Reset)
                .count();
            assert!(
                painted > 0,
                "a cover cut to {rows} rows painted nothing; its encoding overhangs the area"
            );
        }
    }

    /// A cell's colour as three channels, for comparing two renders.
    fn rgb(c: ratatui::style::Color) -> [u8; 3] {
        match c {
            ratatui::style::Color::Rgb(r, g, b) => [r, g, b],
            _ => [0, 0, 0],
        }
    }

    #[test]
    fn a_square_cover_is_as_tall_as_it_is_wide() {
        use ratatui_image::FontSize;
        // On a 19x30 cell, thirteen columns of cover are eight rows of it.
        // Getting this wrong is what decides a whole row is the fold one.
        assert_eq!(
            square_rows(
                13,
                FontSize {
                    width: 19,
                    height: 30
                }
            ),
            8
        );
        assert_eq!(
            square_rows(
                16,
                FontSize {
                    width: 10,
                    height: 20
                }
            ),
            8
        );
        assert_eq!(
            square_rows(
                1,
                FontSize {
                    width: 10,
                    height: 20
                }
            ),
            1
        );
        assert_eq!(
            square_rows(
                9,
                FontSize {
                    width: 8,
                    height: 0
                }
            ),
            9
        );
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
        let colours: std::collections::HashSet<_> = (0..8)
            .flat_map(|y| (0..16).map(move |x| (x, y)))
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

        assert_eq!(
            out.get_pixel(0, 0).0[3],
            0,
            "the top-left corner must be cut away"
        );
        assert_eq!(out.get_pixel(63, 0).0[3], 0, "and the top-right");
        assert_eq!(out.get_pixel(0, 63).0[3], 0);
        assert_eq!(out.get_pixel(63, 63).0[3], 0);

        assert_eq!(
            out.get_pixel(32, 32).0[3],
            255,
            "the middle must be untouched"
        );
        assert_eq!(
            out.get_pixel(32, 2).0[3],
            255,
            "and so must the top edge's centre"
        );
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

        assert_eq!(
            back.get_pixel(0, 0).0[3],
            0,
            "the corner must still be transparent"
        );
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
    fn half_blocks_leave_the_corners_of_a_round_cover_empty() {
        // Half blocks carry no alpha, so the corners `round_off` cleared
        // came out black: a face in a black square, in a row of circles.
        use ratatui::backend::TestBackend;
        use ratatui::style::Color;
        use ratatui::Terminal;

        // As large as a real cover: `Fit` never scales up, so a small
        // picture would fill a corner of the area rather than the area.
        let red = image::RgbaImage::from_pixel(320, 320, image::Rgba([255, 0, 0, 255]));
        let picker = Picker::halfblocks();
        let draw = |shape: Shape| {
            // Both shapes from the square picture: that is what the
            // half-block decode keeps, the circle being cut afterwards.
            let image = image::DynamicImage::ImageRgba8(red.clone());
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            let mut art = Artwork {
                cache: HashMap::new(),
                picker: picker.clone(),
                http: reqwest::Client::new(),
                tx,
            };
            art.cache.insert(
                ("u".into(), shape),
                Entry::Ready(Ready {
                    whole: Box::new(picker.new_resize_protocol(image.clone())),
                    image,
                    cut: None,
                }),
            );
            let mut term = Terminal::new(TestBackend::new(16, 8)).unwrap();
            term.draw(|f| {
                // A selection band under the card, as a selected card has.
                f.render_widget(
                    ratatui::widgets::Block::default()
                        .style(ratatui::style::Style::default().bg(Color::Blue)),
                    f.area(),
                );
                assert!(art.render_shaped(f, f.area(), "u", shape));
            })
            .unwrap();
            term.backend().buffer().clone()
        };

        let round = draw(Shape::Round);
        let corner = &round[(0, 0)];
        assert_eq!(corner.symbol(), " ", "the corner is empty, not black");
        assert_eq!(
            corner.bg,
            Color::Blue,
            "and shows the band that was under the card"
        );
        let centre = &round[(8, 4)];
        assert_eq!(centre.bg, Color::Rgb(255, 0, 0), "the face is still there");
        // The stencil is geometry: the four corners come out the same, and
        // the edge keeps the picture's own colour rather than a blend.
        let empty =
            |x: u16, y: u16| round[(x, y)].symbol() == " " && round[(x, y)].bg == Color::Blue;
        assert!(empty(15, 0) && empty(0, 7) && empty(15, 7));
        let edge = &round[(3, 0)];
        assert!(
            edge.fg == Color::Rgb(255, 0, 0) || edge.bg == Color::Rgb(255, 0, 0),
            "an edge cell holds pure red, not red mixed with black: {edge:?}"
        );

        let square = draw(Shape::Square);
        assert_eq!(
            square[(0, 0)].bg,
            Color::Rgb(255, 0, 0),
            "a square cover keeps its corners"
        );
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
            matches!(
                art.cache
                    .get(&("https://example.invalid/a.jpg".to_string(), Shape::Square)),
                Some(Entry::Loading)
            ),
            "a cover must be fetched, not written off"
        );
    }
}

#[cfg(test)]
mod cut_alignment {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::Terminal;
    use std::collections::HashMap;

    /// The columns a cover paints in the top row, `R` for its red left half
    /// and `B` for its blue right half, drawn `rows` deep into `w` columns.
    fn painted(w: u16, rows: u16) -> String {
        let mut img = image::RgbImage::new(640, 640);
        for (x, _y, px) in img.enumerate_pixels_mut() {
            *px = if x < 320 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 0, 255])
            };
        }
        let image = image::DynamicImage::ImageRgb8(img);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut art = Artwork {
            cache: HashMap::new(),
            picker: Picker::halfblocks(),
            http: reqwest::Client::new(),
            tx,
        };
        art.cache.insert(
            ("u".to_string(), Shape::Square),
            Entry::Ready(Ready {
                whole: Box::new(art.picker.new_resize_protocol(image.clone())),
                image,
                cut: None,
            }),
        );
        let mut term = Terminal::new(TestBackend::new(w, 3)).unwrap();
        term.draw(|f| {
            art.render_shaped(f, Rect::new(0, 0, w, rows), "u", Shape::Square);
        })
        .unwrap();
        let buf = term.backend().buffer();
        (0..w)
            .map(|x| match buf[(x, 0)].fg {
                ratatui::style::Color::Rgb(r, _, b) if r > b => 'R',
                ratatui::style::Color::Rgb(..) => 'B',
                _ => '.',
            })
            .collect()
    }

    #[test]
    fn a_cut_cover_paints_the_same_columns_as_the_whole_one() {
        // A square cover in a column wider than it is tall fills only as
        // many columns as its height allows, and the whole and the cut have
        // to agree on which: stretched to the full width, the cut row's
        // artwork sat a column to the right of the rows above it.
        for w in [6u16, 7, 8, 9] {
            assert_eq!(painted(w, 1), painted(w, 3), "at {w} columns");
        }
    }
}
