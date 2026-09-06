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
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use super::carousel::Card;
use super::theme::Palette;
use super::{grid, tracklist};
use crate::domain::Track;
use crate::search::Results;

/// The heading, a blank, the box, a blank, the tabs, a blank.
pub const HEADER_ROWS: u16 = 6;

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
    /// rather than a grid of cards.
    pub fn is_tracks(&self) -> bool {
        matches!(self, Tab::Top | Tab::Tracks)
    }
}

/// The tracks a tab shows.
///
/// Top results is tracks too: the web client leads with the artist and mixes
/// the kinds, but a terminal tab that mixed them would need a renderer that
/// draws all four — which is the thing this rewrite removed. Top gives the
/// tracks, which is what someone searching for a song wants first.
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

    // The box. A caret marks it while it has the keyboard, so it is clear
    // whether a keystroke goes into the query or drives the results.
    if area.y + 2 < bottom {
        let (text, style) = if view.query.is_empty() && !view.typing {
            ("  Type to search".to_string(), palette.subtitle())
        } else {
            let caret = if view.typing { "▌" } else { "" };
            (format!("  {}{caret}", view.query), palette.title())
        };
        frame.render_widget(
            Paragraph::new(Line::styled(text, style))
                .block(Block::default().style(Style::default().bg(palette.surface))),
            Rect { y: area.y + 2, height: 1, ..area },
        );
    }

    if area.y + 4 < bottom {
        render_tabs(frame, Rect { y: area.y + 4, height: 1, ..area }, palette, view.tab);
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
                })
                .collect(),
            artists: vec![crate::library::Artist {
                id: 1,
                name: "Daft Punk".into(),
                picture: None,
            }],
            playlists: vec![crate::library::Playlist::sample("Essentials", 30)],
        }
    }

    fn draw(tab: usize) -> ratatui::buffer::Buffer {
        let r = results();
        let tracks = tracklist::TrackListState::default();
        let g = grid::GridState::default();
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
        assert!(artists.contains("Daft Punk"), "the artist grid:\n{artists}");

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
    fn a_pane_too_short_for_a_card_says_so_rather_than_going_blank() {
        // The grid draws nothing when not even one row of covers fits.
        // Silence there is indistinguishable from an empty result set.
        let r = results();
        let tracks = tracklist::TrackListState::default();
        let g = grid::GridState::default();
        let buf = geometry::draw(100, 10, move |f, area, p| {
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
        let buf = geometry::draw(100, 30, move |f, area, p| {
            render(
                f, area, p,
                View { query: "zzz", typing: false, results: &empty, tab: 0,
                       tracks: &tracks, grid: &g, playing: None,
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
        let buf = geometry::draw(100, 30, move |f, area, p| {
            render(
                f, area, p,
                View { query: "", typing: true, results: &Results::default(), tab: 0,
                       tracks: &tracks, grid: &g, playing: None,
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
            let _ = geometry::draw(w, h, move |f, area, p| {
                render(
                    f, area, p,
                    View { query: "q", typing: true, results: &r, tab: 0,
                           tracks: &tracks, grid: &g, playing: None,
                           tier: super::super::nowplaying::Tier::Low },
                    |_, _, _, _| false,
                )
            });
        }
    }
}
