//! An artist's own page: their top tracks, albums, and similar artists.
//!
//! Three stacked sections, like Top results — and drawn the same way, by the
//! app's own track list and card grid rather than by a fourth renderer. The
//! sections are budgeted rather than simply stacked: a terminal has far less
//! room than a page, and the tracks are what someone opening an artist is
//! usually after, so they are reserved first and the covers take what is
//! left.

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::carousel::Card;
use super::theme::Palette;
use super::{carousel, tracklist};
use crate::domain::Track;
use crate::library::{ArtistPage, PageKind};

/// The heading and the blank line under it.
pub const HEADER_ROWS: u16 = carousel::HEADING_ROWS;

/// The clear line between one section and the next.
const SECTION_GAP: u16 = 1;

/// Which section holds the selection.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    #[default]
    Tracks,
    Albums,
    /// EPs and singles, which the web client draws under the albums.
    Singles,
    Similar,
    /// Records the artist appears on rather than made.
    AppearsOn,
}

impl Section {
    /// Every section, in the order the web client draws them.
    ///
    /// Fans Also Like sits between the compilations and them on the web,
    /// with Videos and Credits in between — neither of which this shows —
    /// so the two that remain keep their relative order.
    pub const ALL: [Section; 5] = [
        Section::Tracks,
        Section::Albums,
        Section::Singles,
        Section::Similar,
        Section::AppearsOn,
    ];

    /// The sections in the order they are drawn, skipping the ones this
    /// artist has nothing for.
    pub fn present(page: &ArtistPage) -> Vec<Section> {
        Self::ALL
            .into_iter()
            .filter(|s| !s.is_empty(page))
            .collect()
    }

    fn is_empty(self, page: &ArtistPage) -> bool {
        match self {
            Section::Tracks => page.top_tracks.is_empty(),
            Section::Albums => page.albums.is_empty(),
            Section::Singles => page.singles.is_empty(),
            Section::Similar => page.similar.is_empty(),
            Section::AppearsOn => page.appears_on.is_empty(),
        }
    }

    /// What the web client heads this section with, on this kind of page:
    /// an album's rows sit in the artist's slots and are named for what
    /// they hold.
    pub fn heading(self, kind: PageKind) -> &'static str {
        match (kind, self) {
            (PageKind::Artist, Section::Tracks) => "Top Tracks",
            (PageKind::Artist, Section::Albums) => "Albums",
            (PageKind::Artist, Section::Singles) => "EP & Singles",
            (PageKind::Artist, Section::Similar) => "Fans Also Like",
            (PageKind::Artist, Section::AppearsOn) => "Appears On",
            (PageKind::Album, Section::Tracks) => "Tracks",
            (PageKind::Album, Section::Albums) => "More by the artist",
            (PageKind::Album, Section::Singles) => "Other versions",
            (PageKind::Album, Section::Similar) => "Related artists",
            (PageKind::Album, Section::AppearsOn) => "Related albums",
        }
    }

    /// Where the rest of this section lives, when the page cut it.
    pub fn more(self, page: &ArtistPage) -> Option<&str> {
        match self {
            Section::Tracks => page.top_tracks_path.as_deref(),
            Section::Albums => page.albums_more.as_deref(),
            Section::Singles => page.singles_more.as_deref(),
            Section::Similar => page.similar_more.as_deref(),
            Section::AppearsOn => page.appears_on_more.as_deref(),
        }
    }

    pub fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or(Section::Tracks)
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

/// The artist's picture, name and blurb, as the web client stacks them.
///
/// Returns the rows it used, so the sections below start under it. A
/// portrait is round, as the artist cards are; the blurb is wrapped beside
/// it and cut where the picture ends, since prose that pushes the tracks
/// off the pane is worse than prose the reader can open elsewhere.
/// The artist's name, and beside it the key that plays their radio.
///
/// One line for every way the header is drawn: an artist with no
/// photograph has a radio like any other, and drawing their name alone
/// hid the key on every page TIDAL has no picture for.
///
/// `S`, not `R`: `R` plays the radio of whatever track is selected, from
/// wherever the user is, and this page's own radio moved aside for it.
fn heading_line<'a>(page: &'a ArtistPage, palette: &Palette) -> Line<'a> {
    let mut spans = vec![ratatui::text::Span::styled(
        page.name.clone(),
        palette.artist_name(),
    )];
    // "23.8K fans" under the name on the web; beside it here, where the
    // one line is. Followed artists say so, which is what `F` toggles.
    if let Some(fans) = page.fans {
        spans.push(ratatui::text::Span::styled(
            format!("   {} fans", compact(fans)),
            palette.subtitle(),
        ));
    }
    match (page.kind, page.following) {
        (PageKind::Artist, true) => spans.push(ratatui::text::Span::styled(
            "   following",
            palette.subtitle(),
        )),
        // An album in the favourites carries the heart every other
        // favourite does.
        (PageKind::Album, true) => spans.push(ratatui::text::Span::styled(
            format!(" {}", super::icons::favourite()),
            palette.mark(),
        )),
        (_, false) => {}
    }
    if page.radio.is_some() {
        spans.push(ratatui::text::Span::styled(
            "   S for radio",
            palette.subtitle(),
        ));
    }
    Line::from(spans)
}

/// A count the way the web writes it: 3701 as "3.7K", 1.2M past a million,
/// and under a thousand as it is.
fn compact(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}K", n as f64 / 1_000.0),
        _ => format!("{:.1}M", n as f64 / 1_000_000.0),
    }
}

/// Where the header's parts go and how tall it is.
///
/// Worked out once, without drawing, so the keys can count the tracks
/// under the header the way the renderer draws them: the two used to do
/// this arithmetic apart and disagreed on where the list started.
struct Header {
    /// The portrait's rows and columns, when there is a picture and room.
    portrait: Option<(u16, u16)>,
    /// Where the name and blurb start, as an offset from the left, and the
    /// columns they have.
    text: Option<(u16, u16)>,
    /// Lines of blurb drawn, and whether the key to open the rest is offered.
    bio: Option<(u16, bool)>,
    height: u16,
}

fn header_layout(page: &ArtistPage, bio_open: bool, width: u16, height: u16) -> Header {
    // No picture: the name alone, and the hint beside it -- the radio works
    // whether or not TIDAL has a photograph.
    let bare = Header {
        portrait: None,
        text: Some((0, width)),
        bio: None,
        height: HEADER_ROWS,
    };
    if page.picture.is_none() {
        return bare;
    }
    let rows = PORTRAIT_ROWS.min(height);
    let cols = carousel::square_width(rows).min(width);
    if rows < 2 || cols == 0 {
        return bare;
    }
    let portrait = Some((rows, cols));
    let text_x = cols + 2;
    if text_x >= width {
        return Header {
            portrait,
            text: None,
            bio: None,
            height: rows + 1,
        };
    }
    let text_w = width - text_x;
    let text = Some((text_x, text_w));
    // Beside the portrait, or down the pane when it is open. The whole of
    // one of these is a page of prose; the header is not where a page of
    // prose belongs unless it was asked for.
    let lines = if bio_open {
        height.saturating_sub(3)
    } else {
        rows.saturating_sub(2)
    };
    let Some(bio) = page.bio.as_deref().filter(|_| lines > 0) else {
        return Header {
            portrait,
            text,
            bio: None,
            height: rows + 1,
        };
    };
    // Only worth offering when there is more than what is drawn: a short
    // blurb that already fits points at a key that would change nothing.
    let hint = bio.chars().count() > usize::from(lines) * usize::from(text_w);
    let used = lines + if hint { 3 } else { 2 };
    let height = if bio_open {
        used.max(rows) + 1
    } else {
        rows + 1
    };
    Header {
        portrait,
        text,
        bio: Some((lines, hint)),
        height,
    }
}

/// The rows the header takes at this size.
pub fn header_height(page: &ArtistPage, bio_open: bool, width: u16, height: u16) -> u16 {
    header_layout(page, bio_open, width, height).height
}

fn render_header<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    page: &ArtistPage,
    bio_open: bool,
    draw_cover: &mut F,
) -> u16
where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    let header = header_layout(page, bio_open, area.width, area.height);
    if let Some((rows, cols)) = header.portrait {
        // A face is round, a record sleeve square -- and each stands in
        // with the shape it will have.
        carousel::cover_or_stand_in(
            frame,
            Rect {
                width: cols,
                height: rows,
                ..area
            },
            palette,
            page.picture.as_deref(),
            page.kind == PageKind::Artist,
            Some(&page.name),
            draw_cover,
        );
    }
    let Some((x, text_w)) = header.text else {
        return header.height;
    };
    let text_x = area.x + x;
    frame.render_widget(
        Paragraph::new(heading_line(page, palette)),
        Rect {
            x: text_x,
            y: area.y,
            width: text_w,
            height: 1,
        },
    );
    if let (Some((lines, hint)), Some(bio)) = (header.bio, page.bio.as_deref()) {
        frame.render_widget(
            Paragraph::new(bio)
                .style(palette.subtitle())
                .wrap(ratatui::widgets::Wrap { trim: true }),
            Rect {
                x: text_x,
                y: area.y + 2,
                width: text_w,
                height: lines,
            },
        );
        if hint {
            let hint = if bio_open {
                "  b to close"
            } else {
                "  b for more"
            };
            frame.render_widget(
                Paragraph::new(Line::styled(hint, palette.accent_text())),
                Rect {
                    x: text_x,
                    y: area.y + 2 + lines,
                    width: text_w,
                    height: 1,
                },
            );
        }
    }
    header.height
}

/// How tall the artist's portrait is.
///
/// The same height a card's cover gets: an artist's page leads with the
/// picture on the web, and anything shorter reads as a thumbnail rather
/// than a portrait.
const PORTRAIT_ROWS: u16 = carousel::COVER_HEIGHT;

/// The cards a section shows.
///
/// Empty for `Tracks`, which is a list of tracks rather than a row of
/// covers: the callers that draw or open a section skip it before asking,
/// and the sideways move reads the empty length as nothing to scroll.
pub fn cards(page: &ArtistPage, section: Section) -> Vec<Card> {
    match section {
        Section::Albums => page.albums.iter().map(carousel::album_card).collect(),
        Section::Singles => page.singles.iter().map(carousel::album_card).collect(),
        Section::AppearsOn => page.appears_on.iter().map(carousel::album_card).collect(),
        Section::Similar => page.similar.iter().map(carousel::artist_card).collect(),
        Section::Tracks => Vec::new(),
    }
}

/// Lines of text under a section's covers.
fn card_lines(section: Section) -> u16 {
    match section {
        Section::Similar => 1,
        _ => 2,
    }
}

pub struct View<'a> {
    /// The heart on a favourited record among the sections.
    pub liked: carousel::Liked<'a>,
    pub page: &'a ArtistPage,
    pub section: Section,
    pub tracks: &'a tracklist::TrackListState,
    /// One per card section, indexed by `Section::index`.
    ///
    /// A carousel, not a grid: these are one row of covers that scrolls
    /// sideways, and a grid scrolls by whole rows — so a section of one row
    /// could never scroll at all, and the cards past the pane's edge were
    /// unreachable.
    pub rows: &'a [carousel::CarouselState; Section::ALL.len()],
    /// The first section drawn. Five sections and a portrait come to more
    /// than a terminal holds, so the page scrolls rather than budgeting
    /// them all into one screen — which left the tracks showing one row of
    /// five under a full set of covers.
    pub scroll: usize,
    /// Whether the blurb is open. Beside the portrait it gets six lines,
    /// which is a paragraph of the several TIDAL writes — 2Pac's runs to a
    /// page — so `b` opens the rest rather than the page carrying it all.
    pub bio_open: bool,
    pub favourites: &'a std::collections::HashSet<crate::domain::TrackId>,
    pub playing: Option<crate::domain::TrackId>,
    pub tier: super::nowplaying::Tier,
}

pub fn render<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    view: View<'_>,
    mut draw_cover: F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    if area.width == 0 || area.height == 0 {
        return;
    }
    // A column on the right for the scrollbar, so a row's last card does not
    // run under it. Taken whether or not the bar is drawn, or the layout
    // would shift as soon as the page grew past one screen. `layout` takes
    // the same column off, so the keys count beside it too.
    let laid = layout(
        view.page,
        view.scroll,
        view.bio_open,
        area.width,
        area.height,
    );
    let full = area;
    let area = super::scrollbar::reserve(area);
    let bottom = area.y + area.height;

    // The header stays put while the sections scroll under it, as the
    // banner over an opened radio does: the cover and the name are what
    // says which page this is, and they went with the first section.
    // Where the sections start under it is `layout`'s to say.
    render_header(
        frame,
        area,
        palette,
        view.page,
        view.bio_open,
        &mut draw_cover,
    );
    for (section, offset, wants) in laid {
        let y = area.y + offset;
        // The section at the bottom shows as much of itself as fits and is
        // cut by the pane's edge, the way a row of the home page is.
        let height = wants.min(bottom - y);
        if section == Section::Tracks {
            carousel::render_heading(
                frame,
                Rect {
                    y,
                    height: 1,
                    ..area
                },
                palette,
                Section::Tracks.heading(view.page.kind),
                view.section == Section::Tracks,
                // The web offers it here too: these are an artist's top few,
                // and there are always more behind them.
                true,
                false,
            );
            if height <= 1 {
                continue;
            }
            let refs: Vec<&Track> = view.page.top_tracks.iter().collect();
            tracklist::render(
                frame,
                Rect {
                    y: y + 1,
                    height: height - 1,
                    ..area
                },
                palette,
                tracklist::TrackList {
                    filtering: false,
                    favourites: view.favourites,
                    tracks: &refs,
                    state: view.tracks,
                    focused: view.section == Section::Tracks,
                    playing: view.playing,
                    tier: view.tier,
                    banner: None,
                    chrome: tracklist::Chrome::Bare,
                },
                &mut draw_cover,
            );
        } else {
            carousel::render(
                frame,
                Rect { y, height, ..area },
                palette,
                carousel::Row {
                    heading: section.heading(view.page.kind),
                    cards: &cards(view.page, section),
                    state: &view.rows[section.index()],
                    focused: view.section == section,
                    always_more: true,
                    liked: view.liked,
                },
                &mut draw_cover,
            );
        }
    }

    // The page scrolls by section, so the bar measures sections: how many
    // there are, how many are scrolled past, how many are drawn whole.
    super::scrollbar::render(
        frame,
        area,
        palette,
        Section::present(view.page).len(),
        view.scroll,
        sections_in_view(
            view.page,
            view.scroll,
            view.bio_open,
            full.width,
            full.height,
        ),
    );
}

/// Rows a section wants: a card section's heading, the blank under it and
/// its cards; the track list's heading, column headers and every row.
fn section_rows(page: &ArtistPage, section: Section) -> u16 {
    match section {
        Section::Tracks => {
            1 + tracklist::header_rows_of(None, tracklist::Chrome::Bare)
                + page.top_tracks.len() as u16 * tracklist::ROW_HEIGHT
        }
        s => HEADER_ROWS + carousel::card_height(card_lines(s)),
    }
}

/// Where each section from the scroll lands in a pane this size: the
/// section, its top as an offset from the pane's, and the rows it wants.
/// `width` and `height` are the whole pane's; the scrollbar's column is
/// taken off here.
///
/// The page is laid out as the web lays it: the header, the whole list,
/// then the card sections one under the other, and the pane's edge cuts
/// whatever it reaches — a list longer than the pane scrolls within it,
/// and the sections past the fold come up when the selection moves into
/// them. The renderer draws from this and the keys count from it, so a
/// section the keys land on is one the renderer drew. Capping the list at
/// half the pane, or pinning the sections to its foot, left the one thing
/// the web never shows: blank pane between a list and what follows it.
pub fn layout(
    page: &ArtistPage,
    scroll: usize,
    bio_open: bool,
    width: u16,
    height: u16,
) -> Vec<(Section, u16, u16)> {
    // Beside the scrollbar's column, which the renderer holds back.
    let width = super::scrollbar::content_width(width);
    // Under the header, which stays put while the sections scroll.
    let mut y = header_height(page, bio_open, width, height);
    let mut out = Vec::new();
    for (k, section) in Section::present(page).into_iter().enumerate().skip(scroll) {
        if k > scroll {
            y += SECTION_GAP;
        }
        if y >= height {
            break;
        }
        let wants = section_rows(page, section);
        out.push((section, y, wants));
        y += wants;
    }
    out
}

/// The rows the track list is drawn in, under its own heading, or zero
/// when it is scrolled off the page. What the keys scroll the list against.
pub fn tracks_height(
    page: &ArtistPage,
    scroll: usize,
    bio_open: bool,
    width: u16,
    height: u16,
) -> u16 {
    layout(page, scroll, bio_open, width, height)
        .into_iter()
        .find(|(s, _, _)| *s == Section::Tracks)
        .map_or(0, |(_, y, wants)| {
            (wants - 1).min(height.saturating_sub(y + 1))
        })
}

/// How many sections from the scroll are drawn whole, so a move into one
/// that is cut off, or below the fold, scrolls the page up to it.
pub fn sections_in_view(
    page: &ArtistPage,
    scroll: usize,
    bio_open: bool,
    width: u16,
    height: u16,
) -> usize {
    layout(page, scroll, bio_open, width, height)
        .into_iter()
        .filter(|(_, y, wants)| y + wants <= height)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::geometry;
    use std::time::Duration;

    /// A page with something in every section, as a real one has.
    fn full_page() -> ArtistPage {
        let album = |i: u64, title: &str| crate::library::Album {
            id: i,
            title: title.into(),
            artist: "Daft Punk".into(),
            year: Some("2001".into()),
            cover: None,
            track_count: 10,
            duration: None,
        };
        ArtistPage {
            id: 0,
            fans: None,
            following: false,
            kind: PageKind::Artist,
            albums_more: None,
            singles_more: None,
            appears_on_more: None,
            similar_more: None,
            name: "Daft Punk".into(),
            top_tracks_path: Some("pages/data/top-tracks".into()),
            picture: Some("http://x".into()),
            bio: Some("A duo from Paris.".into()),
            top_tracks: (0..4)
                .map(|i| {
                    Track::sample(&format!("Track {i}"), "Daft Punk", Duration::from_secs(200))
                })
                .collect(),
            albums: (0..3).map(|i| album(i, &format!("Album {i}"))).collect(),
            singles: (0..3)
                .map(|i| album(10 + i, &format!("Single {i}")))
                .collect(),
            appears_on: (0..3)
                .map(|i| album(20 + i, &format!("Compilation {i}")))
                .collect(),
            similar: (0..3)
                .map(|i| crate::library::Artist {
                    id: i,
                    name: format!("Artist {i}"),
                    picture: None,
                })
                .collect(),
            radio: Some("mix-1".into()),
        }
    }

    #[test]
    fn every_section_the_web_shows_is_drawn() {
        // The page had three sections where the web has five: the EPs and
        // the compilations were fetched and never shown.
        let p = full_page();
        let tracks = tracklist::TrackListState::default();
        let g: [carousel::CarouselState; Section::ALL.len()] = Default::default();
        let favourites = std::collections::HashSet::new();
        let buf = geometry::draw(120, 80, move |f, area, palette| {
            render(
                f,
                area,
                palette,
                View {
                    page: &p,
                    section: Section::Tracks,
                    tracks: &tracks,
                    rows: &g,
                    scroll: 0,
                    bio_open: false,
                    liked: &carousel::nobody,
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                },
                |_, _, _, _| false,
            )
        });
        let text = geometry::text(&buf);
        for section in Section::ALL {
            assert!(
                text.contains(section.heading(PageKind::Artist)),
                "{:?} is drawn:\n{text}",
                section.heading(PageKind::Artist)
            );
        }
    }

    #[test]
    fn the_headings_are_the_ones_the_web_uses() {
        // Read off the running client: "Top Tracks", not "Tracks"; "Fans
        // Also Like", not "Similar artists".
        assert_eq!(Section::Tracks.heading(PageKind::Artist), "Top Tracks");
        assert_eq!(Section::Albums.heading(PageKind::Artist), "Albums");
        assert_eq!(Section::Singles.heading(PageKind::Artist), "EP & Singles");
        assert_eq!(Section::Similar.heading(PageKind::Artist), "Fans Also Like");
        assert_eq!(Section::AppearsOn.heading(PageKind::Artist), "Appears On");
    }

    #[test]
    fn the_header_leads_with_the_picture_and_the_blurb() {
        // The web page leads with a portrait, the name beside it and the
        // artist's own blurb under that.
        let p = full_page();
        let tracks = tracklist::TrackListState::default();
        let g: [carousel::CarouselState; Section::ALL.len()] = Default::default();
        let favourites = std::collections::HashSet::new();
        let asked = std::cell::RefCell::new(Vec::<(u16, super::super::artwork::Shape)>::new());
        let buf = geometry::draw(120, 60, |f, area, palette| {
            render(
                f,
                area,
                palette,
                View {
                    page: &p,
                    section: Section::Tracks,
                    tracks: &tracks,
                    rows: &g,
                    scroll: 0,
                    bio_open: false,
                    liked: &carousel::nobody,
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                },
                |_f, a, _url, shape| {
                    asked.borrow_mut().push((a.height, shape));
                    false
                },
            )
        });
        let text = geometry::text(&buf);
        assert!(text.contains("Daft Punk"), "the name:\n{text}");
        assert!(text.contains("A duo from Paris"), "and the blurb:\n{text}");

        let first = asked
            .borrow()
            .first()
            .copied()
            .expect("a cover was asked for");
        assert_eq!(
            first.1,
            super::super::artwork::Shape::Round,
            "the portrait is round, as an artist's cards are"
        );
        assert!(first.0 > 1, "and a portrait rather than a thumbnail");
    }

    #[test]
    fn the_portrait_stands_in_with_a_disc_rather_than_a_square() {
        // Reported as a flash on opening an artist: the placeholder was a
        // filled rectangle, so the pane showed a grey square that turned
        // round the moment the photo arrived. The photo is masked to a
        // circle, so what stands in for it has to be one too.
        let p = full_page();
        let tracks = super::tracklist::TrackListState::default();
        let g: [carousel::CarouselState; Section::ALL.len()] = Default::default();
        let favourites = std::collections::HashSet::new();
        let buf = geometry::draw(80, 24, |f, area, palette| {
            render(
                f,
                area,
                palette,
                View {
                    page: &p,
                    section: Section::Tracks,
                    tracks: &tracks,
                    rows: &g,
                    scroll: 0,
                    bio_open: false,
                    liked: &carousel::nobody,
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                },
                // Nothing drawn, which is the pane while the download is
                // still in flight.
                |_f, _a, _url, _shape| false,
            )
        });

        // The corners of the portrait's own box: a disc leaves them clear,
        // a rectangle fills them.
        let filled = |x: u16, y: u16| {
            crate::shell::geometry::is_disc(&buf, x, y, Palette::detect().placeholder)
        };
        let rows = PORTRAIT_ROWS;
        let width = carousel::square_width(rows);
        assert!(
            width >= 4 && rows >= 4,
            "the portrait is big enough to test"
        );

        assert!(
            !filled(0, 0),
            "the top-left corner is painted, so this is a square:\n{}",
            geometry::text(&buf)
        );
        assert!(
            !filled(width - 1, 0),
            "the top-right corner is painted:\n{}",
            geometry::text(&buf)
        );
        // And the body of it is filled, or nothing stood in at all. Read
        // near the left edge rather than the centre: the initial sits in
        // the middle, and its own colour is the text's rather than the
        // disc's.
        assert!(
            filled(1, rows / 2),
            "the body of the disc is not painted, so nothing stood in:\n{}",
            geometry::text(&buf)
        );
    }

    #[test]
    fn a_section_scrolls_sideways_through_its_cards() {
        // These were drawn as grids, which scroll by whole rows — so a
        // section one row tall could never scroll at all and the cards past
        // the pane's edge were unreachable.
        let mut p = full_page();
        p.albums = (0..30)
            .map(|i| crate::library::Album {
                id: i,
                title: format!("Album {i}"),
                artist: "Daft Punk".into(),
                year: None,
                cover: None,
                track_count: 1,
                duration: None,
            })
            .collect();

        let mut rows: [carousel::CarouselState; Section::ALL.len()] = Default::default();
        // Past what fits across a narrow pane.
        let visible = carousel::visible_cards(60);
        for _ in 0..visible + 2 {
            rows[Section::Albums.index()].next(30, visible);
        }
        assert!(
            rows[Section::Albums.index()].offset > 0,
            "the row scrolled rather than running off the edge"
        );

        let tracks = tracklist::TrackListState::default();
        let favourites = std::collections::HashSet::new();
        let buf = geometry::draw(60, 60, move |f, area, palette| {
            render(
                f,
                area,
                palette,
                View {
                    page: &p,
                    section: Section::Albums,
                    tracks: &tracks,
                    rows: &rows,
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                    scroll: 0,
                    bio_open: false,
                    liked: &carousel::nobody,
                },
                |_, _, _, _| false,
            )
        });
        let text = geometry::text(&buf);
        assert!(
            !text.contains("Album 0 "),
            "the row has scrolled past its first card:\n{text}"
        );
    }

    #[test]
    fn every_section_offers_to_show_the_rest_of_itself() {
        // The web has "View all" on each of them, Top Tracks included:
        // these are an artist's top few and there are always more behind.
        let p = full_page();
        let tracks = tracklist::TrackListState::default();
        let g: [carousel::CarouselState; Section::ALL.len()] = Default::default();
        let favourites = std::collections::HashSet::new();
        let buf = geometry::draw(120, 80, move |f, area, palette| {
            render(
                f,
                area,
                palette,
                View {
                    page: &p,
                    section: Section::Tracks,
                    tracks: &tracks,
                    rows: &g,
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                    scroll: 0,
                    bio_open: false,
                    liked: &carousel::nobody,
                },
                |_, _, _, _| false,
            )
        });
        let text = geometry::text(&buf);
        assert!(
            text.matches("See all").count() >= 2,
            "the tracks and the card rows both offer it:\n{text}"
        );
    }

    #[test]
    fn a_clear_line_sits_above_every_section() {
        // The last track sat against the Albums heading: the track budget
        // was capped at half the pane and the cap took the gap with it,
        // and the list was then drawn into the whole of what was left.
        // Every height: the cap on the track budget is what took the gap,
        // and it only bites at some of them.
        for height in 30..60u16 {
            let p = full_page();
            let tracks = tracklist::TrackListState::default();
            let g: [carousel::CarouselState; Section::ALL.len()] = Default::default();
            let favourites = std::collections::HashSet::new();
            let buf = geometry::draw(120, height, move |f, area, palette| {
                render(
                    f,
                    area,
                    palette,
                    View {
                        page: &p,
                        section: Section::Tracks,
                        tracks: &tracks,
                        rows: &g,
                        favourites: &favourites,
                        playing: None,
                        tier: super::super::nowplaying::Tier::Low,
                        scroll: 0,
                        bio_open: false,
                        liked: &carousel::nobody,
                    },
                    |_, _, _, _| false,
                )
            });
            // Every heading, not just the albums: the same gap belongs
            // above each of them.
            for section in Section::ALL.into_iter().skip(1) {
                let Some(at) = geometry::find(&buf, section.heading(PageKind::Artist)) else {
                    continue;
                };
                let above = at.row.saturating_sub(1);
                // Beside the scrollbar's column, which is the bar's to fill.
                let content = super::super::scrollbar::content_width(120);
                let clear = (0..content).all(|x| buf[(x, above)].symbol().trim().is_empty());
                assert!(
                    clear,
                    "at height {height}, row {above} above {:?} is not clear:\n{}",
                    section.heading(PageKind::Artist),
                    geometry::text(&buf)
                );
            }
        }
    }

    #[test]
    fn the_artists_name_is_white_not_grey() {
        // It is the subject of the page, not a label for part of it — the
        // grey a section heading gets made it read as furniture.
        let palette = Palette::detect();
        let p = full_page();
        let tracks = tracklist::TrackListState::default();
        let g: [carousel::CarouselState; Section::ALL.len()] = Default::default();
        let favourites = std::collections::HashSet::new();
        let buf = geometry::draw(120, 44, move |f, area, palette| {
            render(
                f,
                area,
                palette,
                View {
                    page: &p,
                    section: Section::Tracks,
                    tracks: &tracks,
                    rows: &g,
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                    scroll: 0,
                    bio_open: false,
                    liked: &carousel::nobody,
                },
                |_, _, _, _| false,
            )
        });
        let at = geometry::find(&buf, "Daft Punk").expect("the name");
        assert_eq!(
            buf[(at.start, at.row)].fg,
            palette.text,
            "the name is drawn in the primary text colour"
        );
        assert_ne!(
            buf[(at.start, at.row)].fg,
            palette.heading,
            "and not the grey a section heading gets"
        );
    }

    #[test]
    fn a_long_blurb_is_cut_until_it_is_opened() {
        // TIDAL's run to a page of prose — 2Pac's fills the header and
        // pushes the tracks off the pane. The header shows what fits beside
        // the portrait and offers the rest.
        let long = "word ".repeat(400);
        let mut p = full_page();
        p.bio = Some(long.clone());
        let tracks = tracklist::TrackListState::default();
        let g: [carousel::CarouselState; Section::ALL.len()] = Default::default();
        let favourites = std::collections::HashSet::new();

        let shown = |open: bool| {
            let p = &p;
            let tracks = &tracks;
            let g = &g;
            let favourites = &favourites;
            let buf = geometry::draw(120, 44, move |f, area, palette| {
                render(
                    f,
                    area,
                    palette,
                    View {
                        page: p,
                        section: Section::Tracks,
                        tracks,
                        rows: g,
                        favourites,
                        playing: None,
                        tier: super::super::nowplaying::Tier::Low,
                        scroll: 0,
                        bio_open: open,
                        liked: &carousel::nobody,
                    },
                    |_, _, _, _| false,
                )
            });
            geometry::text(&buf).matches("word").count()
        };

        let closed = shown(false);
        let opened = shown(true);
        assert!(closed > 0, "some of the blurb is drawn closed");
        assert!(
            opened > closed,
            "and more of it when opened: {closed} then {opened}"
        );
    }

    #[test]
    fn a_short_blurb_offers_nothing_to_open() {
        // The hint points at a key; a blurb that already fits would have it
        // change nothing.
        let mut p = full_page();
        p.bio = Some("Two words.".into());
        let tracks = tracklist::TrackListState::default();
        let g: [carousel::CarouselState; Section::ALL.len()] = Default::default();
        let favourites = std::collections::HashSet::new();
        let buf = geometry::draw(120, 44, move |f, area, palette| {
            render(
                f,
                area,
                palette,
                View {
                    page: &p,
                    section: Section::Tracks,
                    tracks: &tracks,
                    rows: &g,
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                    scroll: 0,
                    bio_open: false,
                    liked: &carousel::nobody,
                },
                |_, _, _, _| false,
            )
        });
        let text = geometry::text(&buf);
        assert!(text.contains("Two words"), "the blurb is drawn:\n{text}");
        assert!(
            !text.contains("b for more"),
            "with nothing behind it:\n{text}"
        );
    }

    #[test]
    fn an_artist_with_no_blurb_still_gets_a_header() {
        // Kaaris has no blurb; the name and picture must still be drawn.
        let mut p = full_page();
        p.bio = None;
        let tracks = tracklist::TrackListState::default();
        let g: [carousel::CarouselState; Section::ALL.len()] = Default::default();
        let favourites = std::collections::HashSet::new();
        let buf = geometry::draw(120, 60, move |f, area, palette| {
            render(
                f,
                area,
                palette,
                View {
                    page: &p,
                    section: Section::Tracks,
                    tracks: &tracks,
                    rows: &g,
                    scroll: 0,
                    bio_open: false,
                    liked: &carousel::nobody,
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                },
                |_, _, _, _| false,
            )
        });
        let text = geometry::text(&buf);
        assert!(text.contains("Daft Punk"), "the name is drawn:\n{text}");
        assert!(
            text.contains("Top Tracks"),
            "and the sections under it:\n{text}"
        );
    }

    fn page() -> ArtistPage {
        ArtistPage {
            id: 0,
            fans: None,
            following: false,
            kind: PageKind::Artist,
            albums_more: None,
            singles_more: None,
            appears_on_more: None,
            similar_more: None,
            name: "Daft Punk".into(),
            picture: None,
            top_tracks: (0..6)
                .map(|i| {
                    Track::sample(&format!("Track {i}"), "Daft Punk", Duration::from_secs(200))
                })
                .collect(),
            albums: (0..4)
                .map(|i| crate::library::Album {
                    id: i,
                    title: format!("Album {i}"),
                    artist: "Daft Punk".into(),
                    year: Some("2001".into()),
                    cover: None,
                    track_count: 10,
                    duration: None,
                })
                .collect(),
            similar: (0..3)
                .map(|i| crate::library::Artist {
                    id: i,
                    name: format!("Artist {i}"),
                    picture: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    fn draw(height: u16) -> ratatui::buffer::Buffer {
        draw_page(&page(), height)
    }

    fn draw_page(p: &ArtistPage, height: u16) -> ratatui::buffer::Buffer {
        let tracks = tracklist::TrackListState::default();
        let g: [carousel::CarouselState; Section::ALL.len()] = Default::default();
        let favourites = std::collections::HashSet::new();
        geometry::draw(100, height, move |f, area, palette| {
            render(
                f,
                area,
                palette,
                View {
                    page: p,
                    section: Section::Tracks,
                    tracks: &tracks,
                    rows: &g,
                    scroll: 0,
                    bio_open: false,
                    liked: &carousel::nobody,
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                },
                |_, _, _, _| false,
            )
        })
    }

    #[test]
    fn the_tracks_get_the_room_the_covers_do_not_need() {
        // Reserving only the minimum for them showed one track of ten with
        // blank pane under the artwork: on an artist's page the tracks are
        // the section, not a footnote to the covers.
        let buf = draw(44);
        let text = geometry::text(&buf);
        let drawn = (0..10)
            .filter(|i| text.contains(&format!("Track {i}")))
            .count();
        assert!(drawn > 1, "more than one track fits in 44 rows:\n{text}");

        // And a card section still starts under them. The rest are reached
        // by scrolling — five sections and a portrait are taller than any
        // terminal, so the page does not try to fit them all at once.
        assert!(
            text.contains("Albums"),
            "the albums are still drawn:\n{text}"
        );
    }

    #[test]
    fn a_page_taller_than_the_pane_shows_a_scrollbar_and_one_that_fits_does_not() {
        // The page scrolls by section, and nothing said so: a page that
        // ended because it ran out of pane looked like one that had ended.
        let bar_column = |buf: &ratatui::buffer::Buffer| {
            (0..buf.area.height).any(|y| buf[(buf.area.width - 1, y)].symbol() != " ")
        };
        let tall = draw_page(&full_page(), 44);
        assert!(
            bar_column(&tall),
            "five sections in 44 rows scroll:\n{}",
            geometry::text(&tall)
        );
        let short = draw_page(&page(), 60);
        assert!(
            !bar_column(&short),
            "a page that fits has no bar:\n{}",
            geometry::text(&short)
        );
    }

    #[test]
    fn the_header_stays_while_the_sections_scroll_under_it() {
        // As the banner over an opened radio does: the cover and the name
        // say which page this is, and they used to go with the first
        // section.
        let p = full_page();
        let tracks = tracklist::TrackListState::default();
        let g: [carousel::CarouselState; Section::ALL.len()] = Default::default();
        let favourites = std::collections::HashSet::new();
        let buf = geometry::draw(100, 44, move |f, area, palette| {
            render(
                f,
                area,
                palette,
                View {
                    page: &p,
                    section: Section::Albums,
                    tracks: &tracks,
                    rows: &g,
                    scroll: 2,
                    bio_open: false,
                    liked: &carousel::nobody,
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                },
                |_, _, _, _| false,
            )
        });
        let text = geometry::text(&buf);
        assert!(text.contains("Daft Punk"), "the name is still up:\n{text}");
        assert!(
            !text.contains("Track 0"),
            "and the tracks have scrolled off:\n{text}"
        );
    }

    #[test]
    fn a_long_list_is_drawn_whole_with_the_sections_right_under_it() {
        // An album of fifteen tracks showed twelve, capped at half the pane,
        // with its card sections under the cap and the rest of the list
        // scrolling behind them; pinned to the pane's foot instead, the
        // sections left a band of blank pane after the last track. The
        // page is the web's: the whole list, then the sections, and the
        // fold cuts whatever it reaches.
        let mut p = page();
        p.kind = PageKind::Album;
        p.top_tracks = (0..15)
            .map(|i| Track::sample(&format!("Track {i}"), "Daft Punk", Duration::from_secs(200)))
            .collect();
        let buf = draw_page(&p, 70);
        let text = geometry::text(&buf);
        let last = geometry::find(&buf, "Track 14").expect("every track is drawn");
        let albums = geometry::find(&buf, Section::Albums.heading(PageKind::Album))
            .expect("the albums follow");
        // The track's text is on the middle of its three rows, then the
        // clear line, then the heading.
        assert_eq!(
            albums.row,
            last.row + 3,
            "no blank band between them:\n{text}"
        );
        assert!(
            geometry::find(&buf, "Artist 0").is_none(),
            "the similar artists are past the fold, reached by scrolling:\n{text}"
        );
    }

    #[test]
    fn scrolling_reaches_the_sections_below_the_fold() {
        // Five sections do not fit at once, so the page scrolls by section
        // — otherwise Fans Also Like and Appears On could never be seen.
        let p = full_page();
        let tracks = tracklist::TrackListState::default();
        let g: [carousel::CarouselState; Section::ALL.len()] = Default::default();
        let favourites = std::collections::HashSet::new();

        let mut seen = std::collections::HashSet::new();
        for scroll in 0..Section::ALL.len() {
            let p = &p;
            let tracks = &tracks;
            let g = &g;
            let favourites = &favourites;
            let buf = geometry::draw(120, 44, move |f, area, palette| {
                render(
                    f,
                    area,
                    palette,
                    View {
                        page: p,
                        section: Section::Tracks,
                        tracks,
                        rows: g,
                        favourites,
                        playing: None,
                        tier: super::super::nowplaying::Tier::Low,
                        scroll,
                        bio_open: false,
                        liked: &carousel::nobody,
                    },
                    |_, _, _, _| false,
                )
            });
            let text = geometry::text(&buf);
            for section in Section::ALL {
                if text.contains(section.heading(PageKind::Artist)) {
                    seen.insert(section);
                }
            }
        }
        for section in Section::ALL {
            assert!(
                seen.contains(&section),
                "{:?} is reachable by scrolling",
                section.heading(PageKind::Artist)
            );
        }
    }

    #[test]
    fn the_page_leads_with_the_artists_name() {
        let text = geometry::text(&draw(40));
        assert!(text.contains("Daft Punk"), "the heading:\n{text}");
    }

    #[test]
    fn the_heading_carries_the_fans_and_whether_they_are_followed() {
        let palette = Palette::detect();
        let mut p = page();
        assert!(
            !heading_line(&p, &palette).to_string().contains("fans"),
            "no count, no claim"
        );
        p.fans = Some(23_812);
        p.following = true;
        let line = heading_line(&p, &palette).to_string();
        assert!(line.contains("23.8K fans"), "{line}");
        assert!(line.contains("following"), "{line}");
        assert_eq!(compact(999), "999");
        assert_eq!(compact(3_701), "3.7K");
        assert_eq!(compact(1_260_000), "1.3M");
    }

    #[test]
    fn it_stacks_tracks_then_albums_then_similar_artists() {
        let buf = draw(50);
        let text = geometry::text(&buf);
        let tracks = geometry::find(&buf, "Track 0").expect("the tracks");
        let albums = geometry::find(&buf, "Album 0").expect("the albums");
        let similar = geometry::find(&buf, "Artist 0").expect("the similar artists");
        assert!(tracks.row < albums.row, "tracks first:\n{text}");
        assert!(
            albums.row < similar.row,
            "then albums, then similar:\n{text}"
        );
    }

    #[test]
    fn a_short_pane_keeps_the_tracks_and_drops_the_covers() {
        // The covers are the expensive part; a page of artwork with no
        // tracks under it is not what someone opening an artist came for.
        let text = geometry::text(&draw(16));
        assert!(text.contains("Track 0"), "the tracks survive:\n{text}");
        assert!(!text.contains("Album 0"), "the albums give way:\n{text}");
    }

    #[test]
    fn a_section_the_artist_has_nothing_for_is_left_out() {
        let mut p = page();
        p.albums.clear();
        let present = Section::present(&p);
        assert_eq!(present, vec![Section::Tracks, Section::Similar]);
    }

    #[test]
    fn similar_artists_are_round_and_albums_are_not() {
        let p = page();
        assert!(cards(&p, Section::Similar)[0].round);
        assert!(!cards(&p, Section::Albums)[0].round);
    }

    #[test]
    fn a_card_carries_what_it_opens() {
        let p = page();
        assert!(matches!(
            cards(&p, Section::Albums)[0].target,
            Some(carousel::Target::Album(0))
        ));
        assert!(matches!(
            cards(&p, Section::Similar)[0].target,
            Some(carousel::Target::Artist(0))
        ));
    }

    #[test]
    fn rendering_into_any_size_does_not_panic() {
        for h in [1u16, 3, 8, 16, 30, 60] {
            let _ = draw(h);
        }
    }
}
