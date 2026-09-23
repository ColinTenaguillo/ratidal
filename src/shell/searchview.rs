//! The search box and its results.
//!
//! Tabs across the top, one list under them — the web client's shape. What
//! is under them is not a fifth kind of list: Tracks is the app's own track
//! list, and Albums, Artists and Playlists are its own card grid. A search
//! result should look like the same thing it looks like everywhere else, and
//! a separate renderer here meant a fifth set of the same row arithmetic to
//! keep in step with the other four.
//!
//! The reused views draw their own heading and filter box normally; here
//! they are asked for the bare form, since this view has already drawn a
//! heading and a search box of its own.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::carousel::Card;
use super::theme::Palette;
use super::{artistview, carousel, grid, tracklist};
use crate::domain::Track;
use crate::search::Results;

/// The heading, a blank, the box's three rows, a blank, the tabs, a blank.
pub const HEADER_ROWS: u16 = 8;

/// The tabs, in the order they are drawn.
///
/// The web client has Top results, Profiles, Albums, Tracks, Playlists,
/// Videos and Uploads. Videos and Uploads are left out: nothing plays a
/// video, and uploads are not on this API at all — a tab that can only ever
/// be empty is worse than no tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Top,
    Tracks,
    Albums,
    Artists,
    Playlists,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Top,
        Tab::Tracks,
        Tab::Albums,
        Tab::Artists,
        Tab::Playlists,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Tab::Top => "Top results",
            Tab::Tracks => "Tracks",
            Tab::Albums => "Albums",
            Tab::Artists => "Artists",
            Tab::Playlists => "Playlists",
        }
    }

    pub fn from_index(i: usize) -> Self {
        Self::ALL[i.min(Self::ALL.len() - 1)]
    }

    /// Whether this tab's results are tracks, and so drawn as a track list
    /// rather than a grid of cards. Top results is neither: it stacks a
    /// section of each kind.
    pub fn is_tracks(&self) -> bool {
        matches!(self, Tab::Tracks)
    }
}

/// Which stacked section of Top results has the selection.
///
/// The tab draws an artist row, an album row and a track list at once, so a
/// single index cannot say where the cursor is; moving down off the end of
/// one section steps into the next.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TopSection {
    Artists,
    Albums,
    #[default]
    Tracks,
}

impl TopSection {
    /// The sections in the order they are drawn, skipping the ones this
    /// result set has nothing for.
    pub fn present(results: &Results) -> Vec<TopSection> {
        let mut out = Vec::new();
        if !results.artists.is_empty() {
            out.push(TopSection::Artists);
        }
        if !results.albums.is_empty() {
            out.push(TopSection::Albums);
        }
        if !results.tracks.is_empty() {
            out.push(TopSection::Tracks);
        }
        out
    }
}

/// The tracks a tab shows.
///
/// Top results carries tracks as well as cards: its track section is the
/// part the selection runs through, so Enter there plays a result rather
/// than needing a second kind of selection.
pub fn track_rows(results: &Results, tab: Tab) -> Vec<Track> {
    match tab {
        Tab::Top | Tab::Tracks => results.tracks.clone(),
        _ => Vec::new(),
    }
}

/// The cards a tab shows, in the same shape the collection grids use.
pub fn cards(results: &Results, tab: Tab) -> Vec<Card> {
    match tab {
        Tab::Albums => results
            .albums
            .iter()
            .map(super::carousel::album_card)
            .collect(),
        Tab::Artists => results
            .artists
            .iter()
            .map(super::carousel::artist_card)
            .collect(),
        Tab::Playlists => results
            .playlists
            .iter()
            .map(super::carousel::playlist_card)
            .collect(),
        _ => Vec::new(),
    }
}

/// Lines of text under each card, matching what the collection grids use for
/// the same kind: albums an artist and a year, playlists a creator and a
/// count, artists just a name.
fn card_lines(tab: Tab) -> u16 {
    match tab {
        Tab::Playlists => 3,
        Tab::Artists => 1,
        _ => 2,
    }
}

pub struct View<'a> {
    pub query: &'a str,
    /// The heart on a favourited result.
    pub liked: carousel::Liked<'a>,
    pub typing: bool,
    pub results: &'a Results,
    pub tab: usize,
    /// The state of whichever view this tab reuses.
    pub tracks: &'a tracklist::TrackListState,
    pub grid: &'a grid::GridState,
    /// Top results draws three sections at once, so it needs the state of
    /// the card rows as well and which section holds the selection.
    ///
    /// Carousels, not grids, as the artist page's sections are: one row of
    /// covers that scrolls sideways and is cut at the pane's edge. As a
    /// grid, each row drew a scrollbar beside its single row and dropped
    /// the card that did not fit across rather than cutting it.
    pub artists: &'a carousel::CarouselState,
    pub albums: &'a carousel::CarouselState,
    pub top: TopSection,
    /// The first section drawn. The page scrolls by section, as the artist
    /// page does, so the selection is kept on screen rather than running
    /// under the now-playing bar.
    pub scroll: usize,
    /// The user's favourites, for the mark at the end of a track row.
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

    frame.render_widget(
        Paragraph::new(Line::styled("Search", palette.page_heading())),
        Rect { height: 1, ..area },
    );

    // The same box every filter uses, so a field looks like a field
    // wherever it is.
    //
    // Neither this nor the tabs below check they fit first: a `Rect` past
    // the end of the buffer is clipped to nothing, so a pane too short for
    // either already draws neither. The body below is the one that has to
    // ask, because it works out its own height by subtraction.
    super::inputbox::render(
        frame,
        Rect {
            y: area.y + 2,
            ..area
        },
        palette,
        "Type to search",
        view.query,
        view.typing,
    );

    render_tabs(
        frame,
        Rect {
            y: area.y + 6,
            height: 1,
            ..area
        },
        palette,
        view.tab,
    );

    let body_y = area.y + HEADER_ROWS;
    if body_y >= bottom {
        return;
    }
    let body = Rect {
        x: area.x,
        y: body_y,
        width: area.width,
        height: bottom - body_y,
    };

    let tab = Tab::from_index(view.tab);
    if tab == Tab::Top {
        render_top(frame, body, palette, &view, &mut draw_cover);
        return;
    }
    if tab.is_tracks() {
        let tracks = track_rows(view.results, tab);
        if tracks.is_empty() {
            render_empty(frame, body, palette, view.results);
            return;
        }
        // The app's own track list, without the heading and filter box it
        // draws for the Tracks section — this view has its own.
        let refs: Vec<&Track> = tracks.iter().collect();
        tracklist::render(
            frame,
            body,
            palette,
            tracklist::TrackList {
                // Search has its own box above; this list never has the
                // keyboard, so it never shows a caret.
                filtering: false,
                favourites: view.favourites,
                tracks: &refs,
                state: view.tracks,
                focused: true,
                playing: view.playing,
                tier: view.tier,
                banner: None,
                chrome: tracklist::Chrome::Bare,
            },
            &mut draw_cover,
        );
    } else {
        let all = cards(view.results, tab);
        if all.is_empty() {
            render_empty(frame, body, palette, view.results);
            return;
        }
        // And the app's own card grid, likewise bare.
        let refs: Vec<&Card> = all.iter().collect();
        grid::render(
            frame,
            body,
            palette,
            grid::Grid {
                filtering: false,
                heading: tab.label(),
                filter_hint: "",
                cards: &refs,
                state: view.grid,
                focused: true,
                lines: card_lines(tab),
                chrome: grid::Chrome::Bare,
                tabs: (&[], 0),
                liked: view.liked,
            },
            &mut draw_cover,
        );
    }
}

/// Rows a card section takes: the heading, the blank under it, the cards,
/// and the clear line before the next section.
fn card_section_rows(section: TopSection) -> u16 {
    let lines = match section {
        TopSection::Artists => card_lines(Tab::Artists),
        _ => card_lines(Tab::Albums),
    };
    artistview::HEADER_ROWS + carousel::card_height(lines) + SECTION_GAP
}

/// The clear line between one section and the next.
const SECTION_GAP: u16 = 1;

/// The rows the track list has once the sections above it are drawn.
///
/// The list starts wherever the covers end, not at the top of the body, so
/// the keys have to scroll it against this rather than the whole pane —
/// against the pane, the selection ran under the now-playing bar.
pub fn tracks_height(results: &Results, scroll: usize, body: u16) -> u16 {
    let above: u16 = TopSection::present(results)
        .into_iter()
        .skip(scroll)
        .take_while(|s| *s != TopSection::Tracks)
        .map(card_section_rows)
        .sum();
    // And the section's own heading.
    body.saturating_sub(above + 1)
}

/// Top results: a section of each kind, stacked.
///
/// The web client leads with the best artist, then a row of albums, then the
/// tracks. Drawn as the artist page draws its sections: each a carousel or
/// the track list, the one at the foot cut by the pane's edge, and the page
/// scrolling by section so the selection stays on screen. Each section used
/// to be budgeted into one screen instead, which dropped whole rows of
/// covers and let the selection run off the bottom of the tracks.
fn render_top<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    view: &View<'_>,
    draw_cover: &mut F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    let results = view.results;
    if results.is_empty() {
        render_empty(frame, area, palette, results);
        return;
    }

    let bottom = area.y + area.height;
    let mut y = area.y;

    for section in TopSection::present(results).into_iter().skip(view.scroll) {
        if y >= bottom {
            break;
        }
        let focused = view.top == section;
        let (label, cards) = match section {
            TopSection::Artists => ("Artists", cards(results, Tab::Artists)),
            TopSection::Albums => ("Albums", cards(results, Tab::Albums)),
            TopSection::Tracks => {
                // A TRACKS heading with nothing under it is worse than no
                // section: it reads as a list that failed to load.
                let height = bottom - y - 1;
                debug_assert_eq!(
                    height,
                    tracks_height(results, view.scroll, area.height),
                    "the keys and the renderer disagree on where the tracks start"
                );
                if tracklist::visible_rows_chrome(height, false, tracklist::Chrome::Bare) == 0 {
                    break;
                }
                // The same heading a home row draws, so a section reads the
                // same wherever it is.
                carousel::render_heading(
                    frame,
                    Rect {
                        x: area.x,
                        y,
                        width: area.width,
                        height: 1,
                    },
                    palette,
                    "Tracks",
                    focused,
                    false,
                    false,
                );
                let tracks = track_rows(results, Tab::Top);
                let refs: Vec<&Track> = tracks.iter().collect();
                tracklist::render(
                    frame,
                    Rect {
                        x: area.x,
                        y: y + 1,
                        width: area.width,
                        height,
                    },
                    palette,
                    tracklist::TrackList {
                        filtering: false,
                        favourites: view.favourites,
                        tracks: &refs,
                        state: view.tracks,
                        focused,
                        playing: view.playing,
                        tier: view.tier,
                        banner: None,
                        chrome: tracklist::Chrome::Bare,
                    },
                    &mut *draw_cover,
                );
                y = bottom;
                break;
            }
        };
        let needed = card_section_rows(section) - SECTION_GAP;
        // The section at the bottom shows as much of itself as fits and is
        // cut by the pane's edge, the way a row of the home page is. Below
        // a heading and a row of artwork there is nothing to see.
        let drawn = needed.min(bottom - y);
        if drawn < artistview::HEADER_ROWS + 1 {
            break;
        }
        let state = match section {
            TopSection::Artists => view.artists,
            _ => view.albums,
        };
        carousel::render(
            frame,
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: drawn,
            },
            palette,
            carousel::Row {
                heading: label,
                cards: &cards,
                state,
                focused,
                // "See all" only when cards run past the edge: it opens the
                // section's own tab, which is the whole of what was found.
                always_more: false,
                liked: view.liked,
            },
            &mut *draw_cover,
        );
        y += needed + SECTION_GAP;
    }

    // Nothing drawn at all, in a pane too short for any section, looks
    // like a search that found nothing rather than one with no room.
    if y == area.y {
        frame.render_widget(
            Paragraph::new(Line::styled("Pane too short", palette.subtitle())),
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            },
        );
    }
}

/// Say nothing before a search has run, and say so after one that found
/// nothing — "Nothing found" over an empty box would be a lie.
fn render_empty(frame: &mut Frame, area: Rect, palette: &Palette, results: &Results) {
    if results.query.is_empty() {
        return;
    }
    frame.render_widget(
        Paragraph::new(Line::styled("Nothing found", palette.subtitle())),
        Rect { height: 1, ..area },
    );
}

fn render_tabs(frame: &mut Frame, area: Rect, palette: &Palette, active: usize) {
    let mut spans: Vec<Span> = Vec::new();
    for (i, tab) in Tab::ALL.iter().enumerate() {
        // A pill, as the web client draws them: the active one filled.
        let style = if i == active {
            palette.on_accent_pill()
        } else {
            palette.pill()
        };
        spans.push(Span::styled(format!(" {} ", tab.label()), style));
        spans.push(Span::raw("  "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::geometry;
    use std::time::Duration;

    fn results() -> Results {
        Results {
            query: "daft punk".into(),
            tracks: (0..8)
                .map(|i| {
                    crate::domain::Track::sample(
                        &format!("Track {i}"),
                        "Daft Punk",
                        Duration::from_secs(200),
                    )
                })
                .collect(),
            albums: (0..2)
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
            artists: (0..6)
                .map(|i| crate::library::Artist {
                    id: i,
                    name: format!("Artist {i}"),
                    picture: None,
                })
                .collect(),
            playlists: vec![crate::library::Playlist::sample("Essentials", 30)],
        }
    }

    fn draw(tab: usize) -> ratatui::buffer::Buffer {
        let r = results();
        let tracks = tracklist::TrackListState::default();
        let g = grid::GridState::default();
        let c = carousel::CarouselState::default();
        let favourites: std::collections::HashSet<crate::domain::TrackId> =
            std::collections::HashSet::new();
        geometry::draw(100, 30, move |f, area, p| {
            render(
                f,
                area,
                p,
                View {
                    query: "daft punk",
                    typing: false,
                    results: &r,
                    tab,
                    tracks: &tracks,
                    grid: &g,
                    liked: &carousel::nobody,
                    artists: &c,
                    albums: &c,
                    top: TopSection::default(),
                    scroll: 0,
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                },
                |_, _, _, _| false,
            )
        })
    }

    #[test]
    fn the_tracks_tab_is_the_apps_own_track_list() {
        // Not a fifth kind of list: the same columns the Tracks section
        // draws, so a result looks like what it is everywhere else.
        let buf = draw(1);
        let text = geometry::text(&buf);
        assert!(
            text.contains("TITLE"),
            "the track list's own columns:\n{text}"
        );
        assert!(text.contains("ARTIST"));
        assert!(text.contains("Track 0"));
    }

    #[test]
    fn the_album_tab_is_the_apps_own_grid() {
        let buf = draw(2);
        let text = geometry::text(&buf);
        assert!(text.contains("Album 0"), "{text}");
        assert!(
            text.contains("Daft Punk"),
            "with its artist under it:\n{text}"
        );
    }

    #[test]
    fn a_reused_view_does_not_draw_a_second_heading() {
        // Both views draw their own heading and filter box normally; under
        // the search box that would be the same furniture twice.
        let buf = draw(1);
        let text = geometry::text(&buf);
        assert_eq!(
            text.matches("Search").count(),
            1,
            "one heading, not two:\n{text}"
        );
        assert!(
            !text.contains("Filter tracks"),
            "and no second filter box under the search box:\n{text}"
        );
    }

    #[test]
    fn the_artist_and_playlist_tabs_draw_their_own_grids_too() {
        // Albums was the only grid tab with a rendering test, and the three
        // differ: a playlist card is three lines to an artist's one.
        let artists = geometry::text(&draw(3));
        assert!(artists.contains("Artist 0"), "the artist grid:\n{artists}");

        let playlists = geometry::text(&draw(4));
        assert!(
            playlists.contains("Essentials"),
            "the playlist grid:\n{playlists}"
        );
    }

    #[test]
    fn every_tab_draws_inside_the_pane() {
        // A card taller than the space left under the tabs used to run off
        // the bottom, which a terminal shows as nothing at all.
        for tab in 0..Tab::ALL.len() {
            for height in [10, 14, 30] {
                let r = results();
                let tracks = tracklist::TrackListState::default();
                let g = grid::GridState::default();
                let c = carousel::CarouselState::default();
                let favourites: std::collections::HashSet<crate::domain::TrackId> =
                    std::collections::HashSet::new();
                let buf = geometry::draw(100, height, move |f, area, p| {
                    render(
                        f,
                        area,
                        p,
                        View {
                            query: "daft punk",
                            typing: false,
                            results: &r,
                            tab,
                            tracks: &tracks,
                            grid: &g,
                            liked: &carousel::nobody,
                            artists: &c,
                            albums: &c,
                            top: TopSection::default(),
                            scroll: 0,
                            favourites: &favourites,
                            playing: None,
                            tier: super::super::nowplaying::Tier::Low,
                        },
                        |_, _, _, _| false,
                    )
                });
                // The heading always paints, so checking the frame is not
                // blank proves nothing. What matters is that the body under
                // the tabs holds the results rather than running off the
                // bottom, which a terminal shows as an empty pane.
                let body = (HEADER_ROWS..height)
                    .filter(|y| geometry::occupied(&buf, *y).is_some())
                    .count();
                assert!(
                    body > 0,
                    "tab {tab} at height {height} drew nothing under its tabs:\n{}",
                    geometry::text(&buf)
                );
            }
        }
    }

    fn draw_at(tab: usize, height: u16) -> ratatui::buffer::Buffer {
        let r = results();
        let tracks = tracklist::TrackListState::default();
        let g = grid::GridState::default();
        let c = carousel::CarouselState::default();
        let favourites: std::collections::HashSet<crate::domain::TrackId> =
            std::collections::HashSet::new();
        geometry::draw(100, height, move |f, area, p| {
            render(
                f,
                area,
                p,
                View {
                    query: "daft punk",
                    typing: false,
                    results: &r,
                    tab,
                    tracks: &tracks,
                    grid: &g,
                    liked: &carousel::nobody,
                    artists: &c,
                    albums: &c,
                    top: TopSection::default(),
                    scroll: 0,
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                },
                |_, _, _, _| false,
            )
        })
    }

    #[test]
    fn top_results_stacks_a_section_of_each_kind() {
        // It used to be the track list twice over: the same rows under two
        // tabs, so one of the five did nothing.
        let buf = draw_at(0, 40);
        let text = geometry::text(&buf);
        assert!(
            section_at(&buf, "Artists").is_some(),
            "leads with artists:\n{text}"
        );
        assert!(section_at(&buf, "Albums").is_some(), "then albums:\n{text}");
        assert!(section_at(&buf, "Tracks").is_some(), "then tracks:\n{text}");

        let artist = section_at(&draw_at(0, 40), "Artists").unwrap();
        let albums = section_at(&draw_at(0, 40), "Albums").unwrap();
        let tracks = section_at(&draw_at(0, 40), "Tracks").unwrap();
        assert!(artist.row < albums.row, "in that order");
        assert!(albums.row < tracks.row);
    }

    #[test]
    fn top_results_is_no_longer_the_tracks_tab_over_again() {
        let top = geometry::text(&draw_at(0, 40));
        let tracks = geometry::text(&draw_at(1, 40));
        assert_ne!(top, tracks, "the two tabs show different things");
    }

    /// Top results scrolled to its `scroll`th section.
    fn draw_scrolled(height: u16, scroll: usize) -> ratatui::buffer::Buffer {
        draw_top(100, height, scroll)
    }

    fn draw_top(width: u16, height: u16, scroll: usize) -> ratatui::buffer::Buffer {
        let r = results();
        let tracks = tracklist::TrackListState::default();
        let g = grid::GridState::default();
        let c = carousel::CarouselState::default();
        let favourites: std::collections::HashSet<crate::domain::TrackId> =
            std::collections::HashSet::new();
        geometry::draw(width, height, move |f, area, p| {
            render(
                f,
                area,
                p,
                View {
                    query: "daft punk",
                    typing: false,
                    results: &r,
                    tab: 0,
                    tracks: &tracks,
                    grid: &g,
                    liked: &carousel::nobody,
                    artists: &c,
                    albums: &c,
                    top: TopSection::default(),
                    scroll,
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                },
                |_, _, _, _| false,
            )
        })
    }

    #[test]
    fn a_short_pane_cuts_the_section_at_the_fold_and_scrolls_to_the_rest() {
        // As the artist page and the home rows do: the section at the foot
        // shows as much of itself as fits, and the sections past it come
        // up when the selection moves into them. The covers used to be
        // dropped whole to make room for the tracks, so a short pane showed
        // a tab with no artists on it at all.
        let buf = draw_at(0, 16);
        let text = geometry::text(&buf);
        assert!(
            section_at(&buf, "Artists").is_some(),
            "the artists lead:\n{text}"
        );
        // A cut card keeps its cover and loses its name, as the home rows
        // do: a label over a squeezed circle read as text cutting the art.
        assert!(
            geometry::find(&buf, "Artist 0").is_none(),
            "the name is below the fold:\n{text}"
        );
        assert!(
            section_at(&buf, "Tracks").is_none(),
            "the tracks are below the fold:\n{text}"
        );

        let buf = draw_scrolled(16, 2);
        let text = geometry::text(&buf);
        assert!(
            section_at(&buf, "Tracks").is_some(),
            "scrolled to, the tracks show:\n{text}"
        );
        assert!(
            text.contains("Track 0"),
            "with a row under the heading:\n{text}"
        );
        assert!(
            section_at(&buf, "Artists").is_none(),
            "and the artists have scrolled off:\n{text}"
        );
    }

    #[test]
    fn the_card_rows_have_no_scrollbar_and_cut_the_card_at_the_edge() {
        // Reported: a scrollbar beside each single row of covers, and the
        // card that did not fit across dropped rather than cut the way
        // every other row of covers is. Both came from drawing the rows as
        // grids; they are carousels now, like the artist page's.
        // At 95 wide five cards fit with three columns over: too few for a
        // sixth, so nothing but a scrollbar could reach the last column.
        let buf = draw_top(95, 40, 0);
        let text = geometry::text(&buf);
        let heading = section_at(&buf, "Artists").expect("the artist row");
        let label = geometry::find(&buf, "Artist 0").expect("its first card");
        let last = buf.area.width - 1;
        // Under the heading, whose "See all" hint reaches the edge.
        for y in heading.row + 1..=label.row {
            assert_eq!(
                buf[(last, y)].symbol(),
                " ",
                "no scrollbar in the last column on row {y}:\n{text}"
            );
        }
        // At 100 wide the eight columns over are enough of a sixth card to
        // be worth cutting rather than dropping: its name starts where the
        // five whole cards end.
        let buf = draw_top(100, 40, 0);
        let text = geometry::text(&buf);
        let label = geometry::find(&buf, "Artist 0").expect("the first card");
        let five = 5 * usize::from(carousel::card_width() + 3);
        assert!(
            geometry::row(&buf, label.row)
                .chars()
                .skip(five)
                .any(|c| c != ' '),
            "the sixth card is cut at the edge, not dropped:\n{text}"
        );
    }

    #[test]
    fn the_keys_and_the_renderer_agree_on_where_the_tracks_start() {
        // The list starts wherever the covers end, and the keys scroll it
        // against that height. Read off the drawing rather than trusting
        // the sum: at scroll 0 the two card sections are above it, at
        // scroll 1 one is, at scroll 2 none.
        for scroll in 0..3 {
            let buf = draw_scrolled(60, scroll);
            let heading = section_at(&buf, "Tracks").expect("the tracks");
            let body = 60 - HEADER_ROWS;
            assert_eq!(
                buf.area.height - heading.row - 1,
                tracks_height(&results(), scroll, body),
                "at scroll {scroll}:\n{}",
                geometry::text(&buf)
            );
        }
    }

    #[test]
    fn no_section_heading_is_ever_left_without_its_content() {
        // A heading with nothing under it reads as a list that failed to
        // load, which is worse than the section not being there.
        for height in 8..40u16 {
            let text = geometry::text(&draw_at(0, height));
            if let Some(h) = section_at(&draw_at(0, height), "Tracks") {
                let rows_under = (h.row + 1..height)
                    .filter(|y| geometry::occupied(&draw_at(0, height), *y).is_some())
                    .count();
                assert!(
                    rows_under >= 2,
                    "at height {height} TRACKS has only {rows_under} rows under \
                     it:\n{text}"
                );
            }
        }
    }

    #[test]
    fn top_results_never_draws_past_its_pane() {
        // The pane ends where the now-playing bar begins; anything drawn
        // below is hidden under it.
        for height in 8..40u16 {
            let buf = draw_at(0, height);
            assert_eq!(buf.area.height, height, "at height {height}");
        }
    }

    /// Where a section heading sits, ignoring the tab strip above it,
    /// which now carries the same words in the same case.
    fn section_at(buf: &ratatui::buffer::Buffer, name: &str) -> Option<geometry::Span> {
        (HEADER_ROWS..buf.area.height).find_map(|y| {
            let line = geometry::row(buf, y);
            line.trim_start().starts_with(name).then(|| geometry::Span {
                start: line.len() as u16 - line.trim_start().len() as u16,
                end: 0,
                row: y,
            })
        })
    }

    /// The search view with the box focused or not.
    fn with_box(typing: bool) -> ratatui::buffer::Buffer {
        let r = results();
        let tracks = tracklist::TrackListState::default();
        let g = grid::GridState::default();
        let c = carousel::CarouselState::default();
        let favourites: std::collections::HashSet<crate::domain::TrackId> =
            std::collections::HashSet::new();
        geometry::draw(64, 20, move |f, area, p| {
            render(
                f,
                area,
                p,
                View {
                    query: "daft punk",
                    typing,
                    results: &r,
                    tab: 0,
                    tracks: &tracks,
                    grid: &g,
                    liked: &carousel::nobody,
                    artists: &c,
                    albums: &c,
                    top: TopSection::default(),
                    scroll: 0,
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                },
                |_, _, _, _| false,
            )
        })
    }

    #[test]
    fn the_query_sits_inside_a_rounded_box() {
        let buf = with_box(true);
        let text = geometry::text(&buf);
        for corner in ['╭', '╮', '╰', '╯'] {
            assert!(text.contains(corner), "the box has its {corner}:\n{text}");
        }

        // The query is inside it, not above or below.
        let query = geometry::find(&buf, "daft punk").expect("the query");
        let top = geometry::find(&buf, "╭").expect("the top edge");
        let bottom = geometry::find(&buf, "╰").expect("the bottom edge");
        assert!(
            top.row < query.row && query.row < bottom.row,
            "the query is between the edges:\n{text}"
        );
    }

    #[test]
    fn the_tabs_clear_the_box_rather_than_overwriting_it() {
        // The box is three rows where the field was one; drawn at the old
        // offset the tabs landed on its bottom edge.
        let buf = with_box(false);
        let bottom = geometry::find(&buf, "╰").expect("the bottom edge");
        let tabs = geometry::find(&buf, "Top results").expect("the tabs");
        assert!(
            tabs.row > bottom.row,
            "the tabs sit below the box:\n{}",
            geometry::text(&buf)
        );
    }

    #[test]
    fn artists_are_round_and_albums_are_not() {
        let r = results();
        assert!(cards(&r, Tab::Artists)[0].round);
        assert!(!cards(&r, Tab::Albums)[0].round);
    }

    #[test]
    fn a_card_carries_what_it_opens() {
        // Enter on a search result has to reach the same album the
        // collection grid would open.
        let r = results();
        assert!(matches!(
            cards(&r, Tab::Albums)[0].target,
            Some(super::super::carousel::Target::Album(0))
        ));
        assert!(matches!(
            cards(&r, Tab::Playlists)[0].target,
            Some(super::super::carousel::Target::Playlist(_))
        ));
    }

    #[test]
    fn top_results_leads_with_tracks() {
        // The web client mixes the kinds there. A terminal tab that mixed
        // them would need the fifth renderer this rewrite removed, so Top
        // gives the tracks — what someone searching a song wants first.
        let r = results();
        assert_eq!(track_rows(&r, Tab::Top).len(), 8);
        assert!(cards(&r, Tab::Top).is_empty());
    }

    #[test]
    fn the_active_tab_is_marked() {
        let buf = draw(1);
        let tracks = geometry::find(&buf, "Tracks").expect("the Tracks tab");
        let albums = geometry::find(&buf, "Albums").expect("the Albums tab");
        assert_ne!(
            buf[(tracks.start, tracks.row)].bg,
            buf[(albums.start, albums.row)].bg,
            "the chosen tab is filled and the others are not"
        );
    }

    #[test]
    fn a_query_that_found_nothing_says_so() {
        let empty = Results {
            query: "zzz".into(),
            ..Default::default()
        };
        let tracks = tracklist::TrackListState::default();
        let g = grid::GridState::default();
        let c = carousel::CarouselState::default();
        let favourites: std::collections::HashSet<crate::domain::TrackId> =
            std::collections::HashSet::new();
        let buf = geometry::draw(100, 30, move |f, area, p| {
            render(
                f,
                area,
                p,
                View {
                    query: "zzz",
                    liked: &carousel::nobody,
                    typing: false,
                    results: &empty,
                    tab: 0,
                    tracks: &tracks,
                    grid: &g,
                    artists: &c,
                    albums: &c,
                    top: TopSection::default(),
                    scroll: 0,
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                },
                |_, _, _, _| false,
            )
        });
        assert!(geometry::text(&buf).contains("Nothing found"));
    }

    #[test]
    fn before_any_search_the_pane_is_quiet() {
        let tracks = tracklist::TrackListState::default();
        let g = grid::GridState::default();
        let c = carousel::CarouselState::default();
        let favourites: std::collections::HashSet<crate::domain::TrackId> =
            std::collections::HashSet::new();
        let buf = geometry::draw(100, 30, move |f, area, p| {
            render(
                f,
                area,
                p,
                View {
                    query: "",
                    liked: &carousel::nobody,
                    typing: true,
                    results: &Results::default(),
                    tab: 0,
                    tracks: &tracks,
                    grid: &g,
                    artists: &c,
                    albums: &c,
                    top: TopSection::default(),
                    scroll: 0,
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                },
                |_, _, _, _| false,
            )
        });
        let text = geometry::text(&buf);
        assert!(
            !text.contains("Nothing found"),
            "nothing was searched for yet:\n{text}"
        );
    }

    #[test]
    fn rendering_into_any_size_does_not_panic() {
        for (w, h) in [(1u16, 1u16), (20, 5), (100, 30), (200, 60)] {
            let r = results();
            let tracks = tracklist::TrackListState::default();
            let g = grid::GridState::default();
            let c = carousel::CarouselState::default();
            let favourites: std::collections::HashSet<crate::domain::TrackId> =
                std::collections::HashSet::new();
            let _ = geometry::draw(w, h, move |f, area, p| {
                render(
                    f,
                    area,
                    p,
                    View {
                        query: "q",
                        liked: &carousel::nobody,
                        typing: true,
                        results: &r,
                        tab: 0,
                        tracks: &tracks,
                        grid: &g,
                        artists: &c,
                        albums: &c,
                        top: TopSection::default(),
                        scroll: 0,
                        favourites: &favourites,
                        playing: None,
                        tier: super::super::nowplaying::Tier::Low,
                    },
                    |_, _, _, _| false,
                )
            });
        }
    }

    #[test]
    fn each_part_of_the_header_waits_until_the_pane_can_hold_it() {
        // A short pane drops the header from the bottom up: the search box
        // at three rows, the tabs at seven, the body at nine. Nothing said
        // where those steps fall, so a part could start appearing a row
        // earlier or later and only a squashed terminal would show it.
        let seen = |height: u16| geometry::text(&draw_at(0, height));

        assert!(
            !seen(2).contains('\u{256d}'),
            "the box needs three rows, not two"
        );
        assert!(seen(3).contains('\u{256d}'), "at three rows the box starts");

        assert!(
            !seen(6).contains("Top results"),
            "the tabs need seven rows, not six"
        );
        assert!(
            seen(7).contains("Top results"),
            "at seven rows the tabs fit"
        );

        // The body sits under HEADER_ROWS, so it needs one row more again.
        assert!(
            !seen(8).contains("Pane too short"),
            "no body above HEADER_ROWS"
        );
        assert!(
            seen(9).contains("Pane too short"),
            "at nine rows the body starts"
        );
    }
}
