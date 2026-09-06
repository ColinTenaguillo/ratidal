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
use crate::library::ArtistPage;

/// The heading and the blank line under it.
pub const HEADER_ROWS: u16 = 2;

/// The clear line between one section and the next.
const SECTION_GAP: u16 = 1;

/// The least of a section worth drawing at the foot of the page.
///
/// The heading, its blank line, and a row of artwork — the same floor the
/// home page's rows have, and for the same reason.
const MIN_SECTION: u16 = HEADER_ROWS + 1;

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

    /// Whether this artist has nothing for this section.
    fn is_empty(self, page: &ArtistPage) -> bool {
        match self {
            Section::Tracks => page.top_tracks.is_empty(),
            Section::Albums => page.albums.is_empty(),
            Section::Singles => page.singles.is_empty(),
            Section::Similar => page.similar.is_empty(),
            Section::AppearsOn => page.appears_on.is_empty(),
        }
    }

    /// What the web client heads this section with.
    pub fn heading(self) -> &'static str {
        match self {
            Section::Tracks => "Top Tracks",
            Section::Albums => "Albums",
            Section::Singles => "EP & Singles",
            Section::Similar => "Fans Also Like",
            Section::AppearsOn => "Appears On",
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
    let Some(url) = page.picture.as_ref() else {
        // No picture: the name alone, as before.
        frame.render_widget(
            Paragraph::new(Line::styled(page.name.clone(), palette.artist_name())),
            Rect { height: 1, ..area },
        );
        return HEADER_ROWS;
    };

    let rows = PORTRAIT_ROWS.min(area.height);
    let width = carousel::square_width(rows).min(area.width);
    if rows < 2 || width == 0 {
        frame.render_widget(
            Paragraph::new(Line::styled(page.name.clone(), palette.artist_name())),
            Rect { height: 1, ..area },
        );
        return HEADER_ROWS;
    }

    let portrait = Rect { width, height: rows, ..area };
    if !draw_cover(frame, portrait, url, super::artwork::Shape::Round) {
        render_initial(frame, portrait, palette, &page.name);
    }

    let text_x = area.x + width + 2;
    if text_x >= area.x + area.width {
        return rows + 1;
    }
    let text_w = area.x + area.width - text_x;

    // The name, and beside it the radio the web offers as a button. A key
    // nobody can see is a key nobody uses.
    let mut heading = vec![ratatui::text::Span::styled(
        page.name.clone(),
        palette.artist_name(),
    )];
    if page.radio.is_some() {
        heading.push(ratatui::text::Span::styled(
            "   R for radio",
            palette.subtitle(),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(heading)),
        Rect { x: text_x, y: area.y, width: text_w, height: 1 },
    );
    let Some(bio) = page.bio.as_deref() else {
        return rows + 1;
    };

    // Beside the portrait, or down the pane when it is open. The whole of
    // one of these is a page of prose; the header is not where a page of
    // prose belongs unless it was asked for.
    let (lines, hint) = if bio_open {
        (area.height.saturating_sub(3), "  b to close")
    } else {
        (rows.saturating_sub(2), "  b for more")
    };
    if lines == 0 {
        return rows + 1;
    }
    frame.render_widget(
        Paragraph::new(bio)
            .style(palette.subtitle())
            .wrap(ratatui::widgets::Wrap { trim: true }),
        Rect { x: text_x, y: area.y + 2, width: text_w, height: lines },
    );

    // Only worth offering when there is more than what is drawn: a short
    // blurb that already fits points at a key that would change nothing.
    let shown = usize::from(lines) * usize::from(text_w);
    let used = if bio.chars().count() > shown {
        frame.render_widget(
            Paragraph::new(Line::styled(hint, palette.accent_text())),
            Rect { x: text_x, y: area.y + 2 + lines, width: text_w, height: 1 },
        );
        lines + 3
    } else {
        lines + 2
    };
    if bio_open {
        used.max(rows) + 1
    } else {
        rows + 1
    }
}

/// A round placeholder with the artist's initial, for a terminal that
/// cannot draw the picture.
/// The portrait's stand-in while it loads, and for an artist with no photo.
///
/// A disc rather than a filled rectangle: the photo it stands in for is
/// masked to a circle, so a square placeholder showed as a grey block that
/// turned round the moment the download landed -- a visible flash on every
/// artist page. The carousel's own avatars have always been drawn this way;
/// this is the same disc, not a second one.
fn render_initial(frame: &mut Frame, area: Rect, palette: &Palette, name: &str) {
    let initial = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().next().unwrap_or(c));
    super::carousel::render_disc(frame, area, palette, initial);
}

/// How tall the artist's portrait is.
///
/// The same height a card's cover gets: an artist's page leads with the
/// picture on the web, and anything shorter reads as a thumbnail rather
/// than a portrait.
const PORTRAIT_ROWS: u16 = carousel::COVER_HEIGHT;

/// The cards a section shows.
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
    let bottom = area.y + area.height;

    // The header scrolls away with the first section, as it does on the
    // web: it is the top of the page, not a fixed bar.
    let mut y = area.y;
    if view.scroll == 0 {
        y += render_header(
            frame,
            area,
            palette,
            view.page,
            view.bio_open,
            &mut draw_cover,
        );
        if y >= bottom {
            return;
        }
    }
    let present = Section::present(view.page);
    let showing: Vec<Section> = present.into_iter().skip(view.scroll).collect();

    // What the card sections below will take, so the tracks can have the
    // rest. Reserving only the minimum for the tracks left one row showing
    // out of ten with blank pane under the covers: on an artist's page the
    // tracks are the section, not a footnote to the artwork.
    let tracks_cost = if showing.first() != Some(&Section::Tracks) {
        0
    } else {
        // The tracks come first, not last. They used to take what the card
        // sections left, and with five sections on the page that was
        // nothing — an artist showed one of their five top tracks with the
        // covers filling the rest.
        //
        // What they want is a heading and their rows; what they get is that
        // or half the pane, whichever is less, so the sections below still
        // have somewhere to start.
        // The heading, the rows, and a clear line under them — the same
        // gap a card section leaves, or the last track sits against the
        // next heading.
        // The section's own heading, the list's column row, and the rows
        // themselves. Leaving the section heading out of this cost a track:
        // it is drawn above the list and taken off the height the list is
        // given, so the budget has to carry it.
        let wants = 1
            + tracklist::header_rows_with(false, tracklist::Chrome::Bare)
            + view.page.top_tracks.len() as u16 * tracklist::ROW_HEIGHT;
        let half = bottom.saturating_sub(y) / 2;
        let least = (1..=area.height)
            .find(|h| tracklist::visible_rows_chrome(*h, false, tracklist::Chrome::Bare) > 0)
            .map_or(area.height, |h| h + 1);
        // The gap is not the cap's to take: capped to half the pane, the
        // last row landed against the next heading. Whatever the cap works
        // out to, the clear line under the section is added after it.
        wants.min(half.max(least)) + SECTION_GAP
    };

    if showing.first() == Some(&Section::Tracks) {
        let left = bottom.saturating_sub(y + 1);
        if tracklist::visible_rows_chrome(left, false, tracklist::Chrome::Bare) > 0 {
            carousel::render_heading(
                frame,
                Rect { x: area.x, y, width: area.width, height: 1 },
                palette,
                Section::Tracks.heading(),
                view.section == Section::Tracks,
                // The web offers it here too: these are an artist's top
                // few, and there are always more behind them.
                true,
            );
            let refs: Vec<&Track> = view.page.top_tracks.iter().collect();
            // Only as many rows as this section is given, less the heading
            // above it and the clear line below — the sections under it are
            // not to be drawn over, and the last row is not to sit against
            // the next heading.
            let height = tracks_cost
                .saturating_sub(1 + SECTION_GAP)
                .min(bottom - y - 1);
            tracklist::render(
                frame,
                Rect { x: area.x, y: y + 1, width: area.width, height },
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
            y += tracks_cost;
        }
    }

    // Every card section from the scroll onwards, in the order the web
    // draws them — the tracks have had their turn above.
    for section in showing.into_iter().filter(|s| *s != Section::Tracks) {
        let all = cards(view.page, section);
        if all.is_empty() {
            continue;
        }
        let lines = card_lines(section);
        // The heading, the blank under it, and the cards: a carousel draws
        // its own heading and leaves a line below it, where the grid this
        // used to be needed only one.
        let needed = HEADER_ROWS + carousel::card_height(lines);
        if y >= bottom {
            break;
        }
        // The section at the bottom shows as much of itself as fits and is
        // cut by the pane's edge, the way a row of the home page is. Below
        // `MIN_SECTION` there is nothing to see: a heading over one stripe
        // of cover reads as a fault rather than a page that carries on.
        let drawn = needed.min(bottom - y);
        if drawn < MIN_SECTION {
            break;
        }
        carousel::render(
            frame,
            Rect { x: area.x, y, width: area.width, height: drawn },
            palette,
            carousel::Row {
                heading: section.heading(),
                cards: &all,
                state: &view.rows[section.index()],
                focused: view.section == section,
                always_more: true,
            },
            &mut draw_cover,
        );
        y += needed + SECTION_GAP;
    }
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
            singles: (0..3).map(|i| album(10 + i, &format!("Single {i}"))).collect(),
            appears_on: (0..3).map(|i| album(20 + i, &format!("Compilation {i}"))).collect(),
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
                text.contains(section.heading()),
                "{:?} is drawn:\n{text}",
                section.heading()
            );
        }
    }

    #[test]
    fn the_headings_are_the_ones_the_web_uses() {
        // Read off the running client: "Top Tracks", not "Tracks"; "Fans
        // Also Like", not "Similar artists".
        assert_eq!(Section::Tracks.heading(), "Top Tracks");
        assert_eq!(Section::Albums.heading(), "Albums");
        assert_eq!(Section::Singles.heading(), "EP & Singles");
        assert_eq!(Section::Similar.heading(), "Fans Also Like");
        assert_eq!(Section::AppearsOn.heading(), "Appears On");
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

        let first = asked.borrow().first().copied().expect("a cover was asked for");
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
        let filled = |x: u16, y: u16| buf[(x, y)].bg == Palette::detect().placeholder;
        let rows = PORTRAIT_ROWS;
        let width = carousel::square_width(rows);
        assert!(width >= 4 && rows >= 4, "the portrait is big enough to test");

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
        // And the middle of it is filled, or there is no placeholder at all.
        assert!(
            filled(width / 2, rows / 2),
            "the middle is not painted, so nothing stood in:\n{}",
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
                    },
                    |_, _, _, _| false,
                )
            });
            // Every heading, not just the albums: the same gap belongs
            // above each of them.
            for section in Section::ALL.into_iter().skip(1) {
                let Some(at) = geometry::find(&buf, section.heading()) else {
                    continue;
                };
                let above = at.row.saturating_sub(1);
                let clear = (0..120u16).all(|x| buf[(x, above)].symbol().trim().is_empty());
                assert!(
                    clear,
                    "at height {height}, row {above} above {:?} is not clear:\n{}",
                    section.heading(),
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
                },
                |_, _, _, _| false,
            )
        });
        let text = geometry::text(&buf);
        assert!(text.contains("Two words"), "the blurb is drawn:\n{text}");
        assert!(!text.contains("b for more"), "with nothing behind it:\n{text}");
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
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                },
                |_, _, _, _| false,
            )
        });
        let text = geometry::text(&buf);
        assert!(text.contains("Daft Punk"), "the name is drawn:\n{text}");
        assert!(text.contains("Top Tracks"), "and the sections under it:\n{text}");
    }

    fn page() -> ArtistPage {
        ArtistPage {
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
        let p = page();
        let tracks = tracklist::TrackListState::default();
        let g: [carousel::CarouselState; Section::ALL.len()] = Default::default();
        let favourites = std::collections::HashSet::new();
        geometry::draw(100, height, move |f, area, palette| {
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
        assert!(text.contains("Albums"), "the albums are still drawn:\n{text}");
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
                    },
                    |_, _, _, _| false,
                )
            });
            let text = geometry::text(&buf);
            for section in Section::ALL {
                if text.contains(section.heading()) {
                    seen.insert(section);
                }
            }
        }
        for section in Section::ALL {
            assert!(
                seen.contains(&section),
                "{:?} is reachable by scrolling",
                section.heading()
            );
        }
    }

    #[test]
    fn the_page_leads_with_the_artists_name() {
        let text = geometry::text(&draw(40));
        assert!(text.contains("Daft Punk"), "the heading:\n{text}");
    }

    #[test]
    fn it_stacks_tracks_then_albums_then_similar_artists() {
        let buf = draw(50);
        let text = geometry::text(&buf);
        let tracks = geometry::find(&buf, "Track 0").expect("the tracks");
        let albums = geometry::find(&buf, "Album 0").expect("the albums");
        let similar = geometry::find(&buf, "Artist 0").expect("the similar artists");
        assert!(tracks.row < albums.row, "tracks first:\n{text}");
        assert!(albums.row < similar.row, "then albums, then similar:\n{text}");
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
