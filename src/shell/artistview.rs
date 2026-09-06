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
use super::{carousel, grid, tracklist};
use crate::domain::Track;
use crate::library::ArtistPage;

/// The heading and the blank line under it.
pub const HEADER_ROWS: u16 = 2;

/// Which section holds the selection.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    #[default]
    Tracks,
    Albums,
    Similar,
}

impl Section {
    /// The sections in the order they are drawn, skipping the ones this
    /// artist has nothing for.
    pub fn present(page: &ArtistPage) -> Vec<Section> {
        let mut out = Vec::new();
        if !page.top_tracks.is_empty() {
            out.push(Section::Tracks);
        }
        if !page.albums.is_empty() {
            out.push(Section::Albums);
        }
        if !page.similar.is_empty() {
            out.push(Section::Similar);
        }
        out
    }

    pub fn from_index(i: usize) -> Self {
        match i {
            1 => Section::Albums,
            2 => Section::Similar,
            _ => Section::Tracks,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Section::Tracks => 0,
            Section::Albums => 1,
            Section::Similar => 2,
        }
    }
}

/// The cards a section shows.
pub fn cards(page: &ArtistPage, section: Section) -> Vec<Card> {
    match section {
        Section::Albums => page
            .albums
            .iter()
            .map(carousel::album_card)
            .collect(),
        Section::Similar => page
            .similar
            .iter()
            .map(carousel::artist_card)
            .collect(),
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
    pub albums: &'a grid::GridState,
    pub similar: &'a grid::GridState,
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
        Paragraph::new(Line::styled(view.page.name.clone(), palette.page_heading())),
        Rect { height: 1, ..area },
    );

    let mut y = area.y + HEADER_ROWS;
    if y >= bottom {
        return;
    }

    // What the card sections below will take, so the tracks can have the
    // rest. Reserving only the minimum for the tracks left one row showing
    // out of ten with blank pane under the covers: on an artist's page the
    // tracks are the section, not a footnote to the artwork.
    let cards_cost: u16 = [Section::Albums, Section::Similar]
        .iter()
        .filter(|s| !cards(view.page, **s).is_empty())
        .map(|s| 1 + carousel::card_height(card_lines(*s)) + 1)
        .sum();

    let tracks_cost = if view.page.top_tracks.is_empty() {
        0
    } else {
        // The room left over, but never less than one row and never more
        // than the tracks can fill.
        let least = (1..=area.height)
            .find(|h| tracklist::visible_rows_chrome(*h, false, tracklist::Chrome::Bare) > 0)
            .map_or(area.height, |h| h + 1);
        let most = tracklist::header_rows_with(false, tracklist::Chrome::Bare)
            + view.page.top_tracks.len() as u16 * tracklist::ROW_HEIGHT
            + 1;
        bottom
            .saturating_sub(y)
            .saturating_sub(cards_cost)
            .clamp(least, most.max(least))
    };

    if !view.page.top_tracks.is_empty() {
        let left = bottom.saturating_sub(y + 1);
        if tracklist::visible_rows_chrome(left, false, tracklist::Chrome::Bare) > 0 {
            carousel::render_heading(
                frame,
                Rect { x: area.x, y, width: area.width, height: 1 },
                palette,
                "Tracks",
                false,
                false,
            );
            let refs: Vec<&Track> = view.page.top_tracks.iter().collect();
            // Only as many rows as this section is given, so the sections
            // below it are not drawn over.
            let height = (tracks_cost.saturating_sub(1)).min(bottom - y - 1);
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

    for section in [Section::Albums, Section::Similar] {
        let all = cards(view.page, section);
        if all.is_empty() {
            continue;
        }
        let lines = card_lines(section);
        let needed = 1 + carousel::card_height(lines);
        if y + needed + 1 > bottom {
            break;
        }
        carousel::render_heading(
            frame,
            Rect { x: area.x, y, width: area.width, height: 1 },
            palette,
            match section {
                Section::Albums => "Albums",
                _ => "Similar artists",
            },
            false,
            false,
        );
        let refs: Vec<&Card> = all.iter().collect();
        grid::render(
            frame,
            Rect { x: area.x, y: y + 1, width: area.width, height: needed - 1 },
            palette,
            grid::Grid {
                filtering: false,
                heading: "",
                filter_hint: "",
                cards: &refs,
                state: match section {
                    Section::Albums => view.albums,
                    _ => view.similar,
                },
                focused: view.section == section,
                lines,
                chrome: grid::Chrome::Bare,
                            tabs: (&[], 0),
            },
            &mut draw_cover,
        );
        y += needed + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::geometry;
    use std::time::Duration;

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
        }
    }

    fn draw(height: u16) -> ratatui::buffer::Buffer {
        let p = page();
        let tracks = tracklist::TrackListState::default();
        let g = grid::GridState::default();
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
                    albums: &g,
                    similar: &g,
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

        // And the sections below still fit.
        assert!(text.contains("Albums"), "the albums are still drawn:\n{text}");
        assert!(
            text.contains("Similar artists"),
            "and so are the similar artists:\n{text}"
        );
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
