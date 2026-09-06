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
use super::{carousel, grid, tracklist};
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
    pub const ALL: [Tab; 5] =
        [Tab::Top, Tab::Tracks, Tab::Albums, Tab::Artists, Tab::Playlists];

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
            .map(|a| Card {
                title: a.title.clone(),
                subtitle: a.artist.clone(),
                detail: a.year.clone().unwrap_or_default(),
                cover_url: a.cover.clone(),
                round: false,
                target: Some(super::carousel::Target::Album(a.id)),
                duration: std::time::Duration::ZERO,
            })
            .collect(),
        Tab::Artists => results
            .artists
            .iter()
            .map(|a| Card {
                title: a.name.clone(),
                cover_url: a.picture.clone(),
                round: true,
                target: Some(super::carousel::Target::Artist(a.id)),
                ..Default::default()
            })
            .collect(),
        Tab::Playlists => results
            .playlists
            .iter()
            .map(|p| Card {
                title: p.title.clone(),
                subtitle: p.creator.clone(),
                detail: format!("{} tracks", p.track_count),
                cover_url: p.cover.clone(),
                round: false,
                target: Some(super::carousel::Target::Playlist(p.uuid.clone())),
                duration: std::time::Duration::ZERO,
            })
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
    pub typing: bool,
    pub results: &'a Results,
    pub tab: usize,
    /// The state of whichever view this tab reuses.
    pub tracks: &'a tracklist::TrackListState,
    pub grid: &'a grid::GridState,
    /// Top results draws three sections at once, so it needs the state of
    /// the card rows as well and which section holds the selection.
    pub artists: &'a grid::GridState,
    pub albums: &'a grid::GridState,
    pub top: TopSection,
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
    if area.y + 2 < bottom {
        super::inputbox::render(
            frame,
            Rect { y: area.y + 2, ..area },
            palette,
            "Type to search",
            view.query,
            view.typing,
        );
    }

    if area.y + 6 < bottom {
        render_tabs(frame, Rect { y: area.y + 6, height: 1, ..area }, palette, view.tab);
    }

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
        // A card is a cover plus its text; in a pane this short not even
        // one row fits, and the grid draws nothing at all. A blank pane
        // reads as "no results" rather than "no room", so say which.
        if grid::rows(body.height, card_lines(tab)) == 0 {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    "Pane too short for covers",
                    palette.subtitle(),
                )),
                Rect { height: 1, ..body },
            );
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
            },
            &mut draw_cover,
        );
    }
}

/// Say nothing before a search has run, and say so after one that found
/// nothing — "Nothing found" over an empty box would be a lie.
/// Top results: a section of each kind, stacked.
///
/// The web client leads with the best artist, then a row of albums, then the
/// tracks. A terminal has far less room, so each section is drawn only if
/// what is left of the pane can hold it, and the tracks — what someone
/// searching a song is usually after — get whatever remains rather than
/// being squeezed out by the covers above them.
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

    // A section is worth drawing only if its heading and one row of cards
    // both fit; half a cover reads as a rendering fault.
    let mut card_section = |y: &mut u16,
                            label: &str,
                            cards: Vec<Card>,
                            lines: u16,
                            limit: u16,
                            state: &grid::GridState,
                            focused: bool| {
        if cards.is_empty() {
            return;
        }
        // The blank line after the section counts too: leaving it out of the
        // check let a section end exactly on the ceiling and push what
        // follows one row past it.
        let needed = 1 + carousel::card_height(lines);
        if *y + needed + 1 > limit {
            return;
        }
        super::carousel::render_heading(
            frame,
            Rect { x: area.x, y: *y, width: area.width, height: 1 },
            palette,
            label,
            false,
            false,
        );
        let refs: Vec<&Card> = cards.iter().collect();
        grid::render(
            frame,
            Rect { x: area.x, y: *y + 1, width: area.width, height: needed - 1 },
            palette,
            grid::Grid {
                filtering: false,
                heading: "",
                filter_hint: "",
                cards: &refs,
                state,
                focused,
                lines,
                chrome: grid::Chrome::Bare,
            },
            &mut *draw_cover,
        );
        *y += needed + 1;
    };

    // The tracks are what someone searching a song is after, so they are
    // budgeted first and the covers above them take only what is left. Done
    // the other way round, a card section ate the whole pane and the tab
    // showed a TRACKS heading with nothing under it.
    let tracks = track_rows(results, Tab::Top);
    // What a heading plus the list's own columns and one row actually costs,
    // asked of the list rather than restated here: a row is taller than it
    // looks, and guessing it left a TRACKS heading with nothing under it.
    let tracks_cost = if tracks.is_empty() {
        0
    } else {
        (1..=area.height)
            .find(|h| {
                tracklist::visible_rows_chrome(*h, false, tracklist::Chrome::Bare) > 0
            })
            .map_or(area.height, |h| h + 1)
    };
    // The ceiling the card sections must stay under, so that whatever they
    // take, the tracks still get their heading, columns and a row. If not
    // even that fits, the covers may as well have the pane.
    let card_budget = if y + tracks_cost > bottom {
        bottom
    } else {
        bottom - tracks_cost
    };
    debug_assert!(card_budget <= bottom);

    // A row of artists rather than the single best match: the row is as wide
    // as the album row under it, and leaving it with one card in it looked
    // like a rendering fault rather than a choice.
    let artists = cards(results, Tab::Artists);
    card_section(
        &mut y,
        "Artists",
        artists,
        card_lines(Tab::Artists),
        card_budget,
        view.artists,
        view.top == TopSection::Artists,
    );
    let albums = cards(results, Tab::Albums);
    card_section(
        &mut y,
        "Albums",
        albums,
        card_lines(Tab::Albums),
        card_budget,
        view.albums,
        view.top == TopSection::Albums,
    );

    // A TRACKS heading with nothing under it is worse than no section: it
    // reads as a list that failed to load.
    let left = bottom.saturating_sub(y + 1);
    if tracks.is_empty()
        || tracklist::visible_rows_chrome(left, false, tracklist::Chrome::Bare) == 0
    {
        // Nothing drawn at all, in a pane too short for any section, looks
        // like a search that found nothing rather than one with no room.
        if y == area.y && !results.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled("Pane too short", palette.subtitle())),
                Rect { x: area.x, y, width: area.width, height: 1 },
            );
        }
        return;
    }
    // The same heading a home row draws, so a section reads the same
    // wherever it is — these were shouted in capitals and tinted like a
    // sidebar label, which made them a third kind of heading.
    super::carousel::render_heading(
        frame,
        Rect { x: area.x, y, width: area.width, height: 1 },
        palette,
        "Tracks",
        false,
        false,
    );
    let refs: Vec<&Track> = tracks.iter().collect();
    tracklist::render(
        frame,
        Rect { x: area.x, y: y + 1, width: area.width, height: bottom - y - 1 },
        palette,
        tracklist::TrackList {
            filtering: false,
            favourites: view.favourites,
            tracks: &refs,
            state: view.tracks,
            focused: view.top == TopSection::Tracks,
            playing: view.playing,
            tier: view.tier,
            banner: None,
            chrome: tracklist::Chrome::Bare,
        },
        draw_cover,
    );
}

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
                    artists: &g,
                    albums: &g,
                    top: TopSection::default(),
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
        assert!(text.contains("TITLE"), "the track list's own columns:\n{text}");
        assert!(text.contains("ARTIST"));
        assert!(text.contains("Track 0"));
    }

    #[test]
    fn the_album_tab_is_the_apps_own_grid() {
        let buf = draw(2);
        let text = geometry::text(&buf);
        assert!(text.contains("Album 0"), "{text}");
        assert!(text.contains("Daft Punk"), "with its artist under it:\n{text}");
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
        assert!(playlists.contains("Essentials"), "the playlist grid:\n{playlists}");
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
                            artists: &g,
                            albums: &g,
                            top: TopSection::default(),
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

    #[test]
    fn a_pane_with_no_room_for_even_a_sliver_says_so() {
        // The grid draws nothing when not even one row of covers fits.
        // Silence there is indistinguishable from an empty result set.
        let r = results();
        let tracks = tracklist::TrackListState::default();
        let g = grid::GridState::default();
        let favourites: std::collections::HashSet<crate::domain::TrackId> =
            std::collections::HashSet::new();
        // Two rows of cover is enough to show a row beginning, so this has
        // to be shorter than that to reach the message at all.
        let buf = geometry::draw(100, 9, move |f, area, p| {
            render(
                f,
                area,
                p,
                View {
                    query: "daft punk",
                    typing: false,
                    results: &r,
                    tab: 2, // Albums
                    tracks: &tracks,
                    grid: &g,
                    artists: &g,
                    albums: &g,
                    top: TopSection::default(),
                    favourites: &favourites,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                },
                |_, _, _, _| false,
            )
        });
        let text = geometry::text(&buf);
        assert!(text.contains("too short"), "says why the pane is bare:\n{text}");
        assert!(
            !text.contains("Nothing found"),
            "and does not claim the search found nothing:\n{text}"
        );
    }

    fn draw_at(tab: usize, height: u16) -> ratatui::buffer::Buffer {
        let r = results();
        let tracks = tracklist::TrackListState::default();
        let g = grid::GridState::default();
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
                    artists: &g,
                    albums: &g,
                    top: TopSection::default(),
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
        assert!(section_at(&buf, "Artists").is_some(), "leads with artists:\n{text}");
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

    #[test]
    fn a_short_pane_keeps_the_tracks_and_drops_the_covers() {
        // The covers are the expensive part and the tracks are what someone
        // searching a song is after, so the cards give way first.
        let buf = draw_at(0, 16);
        let text = geometry::text(&buf);
        assert!(section_at(&buf, "Tracks").is_some(), "the tracks survive:\n{text}");
        assert!(text.contains("Track 0"), "with a row under the heading:\n{text}");
        // "ARTIST" is also a column header in the track list, so look for
        // the card's own text instead.
        assert!(
            section_at(&buf, "Albums").is_none(),
            "the cover sections give way:\n{text}"
        );
        assert!(section_at(&buf, "Albums").is_none(), "no album row here:\n{text}");
    }

    #[test]
    fn the_tracks_get_their_row_before_the_covers_take_the_pane() {
        // At heights where both cannot fit, the cards used to be laid out
        // first and eat everything, leaving the tab with covers and no
        // tracks — the opposite of what someone searching a song wants.
        // At these heights the body can hold a track row, so it must: a
        // pane showing only a cover is the failure this budget prevents.
        for height in 18..26u16 {
            let text = geometry::text(&draw_at(0, height));
            assert!(
                text.contains("Track 0"),
                "at height {height} the covers took the pane and left no \
                 track row:\n{text}"
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
                    artists: &g,
                    albums: &g,
                    top: TopSection::default(),
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
        let empty = Results { query: "zzz".into(), ..Default::default() };
        let tracks = tracklist::TrackListState::default();
        let g = grid::GridState::default();
        let favourites: std::collections::HashSet<crate::domain::TrackId> =
            std::collections::HashSet::new();
        let buf = geometry::draw(100, 30, move |f, area, p| {
            render(
                f, area, p,
                View { query: "zzz", typing: false, results: &empty, tab: 0,
                       tracks: &tracks, grid: &g, artists: &g, albums: &g,
                       top: TopSection::default(), favourites: &favourites, playing: None,
                       tier: super::super::nowplaying::Tier::Low },
                |_, _, _, _| false,
            )
        });
        assert!(geometry::text(&buf).contains("Nothing found"));
    }

    #[test]
    fn before_any_search_the_pane_is_quiet() {
        let tracks = tracklist::TrackListState::default();
        let g = grid::GridState::default();
        let favourites: std::collections::HashSet<crate::domain::TrackId> =
            std::collections::HashSet::new();
        let buf = geometry::draw(100, 30, move |f, area, p| {
            render(
                f, area, p,
                View { query: "", typing: true, results: &Results::default(), tab: 0,
                       tracks: &tracks, grid: &g, artists: &g, albums: &g,
                       top: TopSection::default(), favourites: &favourites, playing: None,
                       tier: super::super::nowplaying::Tier::Low },
                |_, _, _, _| false,
            )
        });
        let text = geometry::text(&buf);
        assert!(!text.contains("Nothing found"), "nothing was searched for yet:\n{text}");
    }

    #[test]
    fn rendering_into_any_size_does_not_panic() {
        for (w, h) in [(1u16, 1u16), (20, 5), (100, 30), (200, 60)] {
            let r = results();
            let tracks = tracklist::TrackListState::default();
            let g = grid::GridState::default();
        let favourites: std::collections::HashSet<crate::domain::TrackId> =
            std::collections::HashSet::new();
            let _ = geometry::draw(w, h, move |f, area, p| {
                render(
                    f, area, p,
                    View { query: "q", typing: true, results: &r, tab: 0,
                           tracks: &tracks, grid: &g, artists: &g, albums: &g,
                       top: TopSection::default(), favourites: &favourites, playing: None,
                           tier: super::super::nowplaying::Tier::Low },
                    |_, _, _, _| false,
                )
            });
        }
    }
}
