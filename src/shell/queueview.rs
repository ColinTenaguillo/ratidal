//! What is playing next, and what has been played, as the track list
//! everywhere else draws.
//!
//! A queue the user cannot see is a queue they cannot trust: the reported
//! complaint was that changing track sometimes brought an old album back,
//! which is exactly the kind of thing that is obvious the moment the list
//! is on screen and baffling while it is not.
//!
//! Both were modals with a list of their own -- a shorter row, headings by
//! source, and keys that closed them. They draw the same list the Tracks
//! section draws now, so a row here is a row like any other: the keys act
//! on it, it is marked the same way, and it looks like what it is.

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::theme::Palette;
use super::trackgrid::Marks;
use super::tracklist::{self, Chrome, TrackList, TrackListState};

/// The heading and the blank under it, above the list's own column headers.
pub const HEADING_ROWS: u16 = 2;

/// A list over the pane: its name, then the rows.
///
/// `tracks` is whichever list is up, in the order it is shown -- the queue
/// in play order, the history newest first -- and `state` is where in it
/// the user is.
pub fn render<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    heading: &str,
    tracks: &[&crate::domain::Track],
    state: &TrackListState,
    marks: Marks<'_>,
    draw_cover: F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Line::styled(heading, palette.page_heading())),
        Rect { height: 1, ..area },
    );
    let list = Rect {
        y: area.y + HEADING_ROWS,
        height: area.height.saturating_sub(HEADING_ROWS),
        ..area
    };
    tracklist::render(
        frame,
        list,
        palette,
        TrackList {
            tracks,
            state,
            focused: true,
            playing: marks.playing,
            tier: marks.tier,
            // No banner: neither list is a record, and the name is the
            // heading above.
            banner: None,
            chrome: Chrome::Bare,
            favourites: marks.favourites,
            filtering: false,
        },
        draw_cover,
    );
}

/// How many rows the list under the heading shows in `height`.
///
/// Counted the way the renderer lays it out, so the keys scroll where the
/// rows are drawn -- the same reason every other list has one of these.
pub fn visible_rows(height: u16) -> usize {
    tracklist::visible_rows_chrome(height.saturating_sub(HEADING_ROWS), false, Chrome::Bare)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Track, TrackId};
    use std::time::Duration;

    #[test]
    fn the_list_is_the_track_list_under_a_heading() {
        // The point of the rewrite: what the queue draws is what the Tracks
        // section draws, so it needs no rows, marks or keys of its own.
        let a = Track { id: TrackId(1), ..Track::sample("First", "A", Duration::from_secs(1)) };
        let b = Track { id: TrackId(2), ..Track::sample("Second", "B", Duration::from_secs(1)) };
        let tracks = vec![&a, &b];
        let state = TrackListState::default();
        let favourites = std::collections::HashSet::new();
        let buf = super::super::geometry::draw(80, 12, |f, area, p| {
            render(
                f,
                area,
                p,
                "Queue",
                &tracks,
                &state,
                Marks { favourites: &favourites, playing: Some(TrackId(2)), tier: Default::default() },
                |_, _, _, _| false,
            );
        });
        let text: String = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf.cell((x, y)).map(|c| c.symbol().to_string()).unwrap_or_default())
                    .collect::<String>()
                    + "\n"
            })
            .collect();
        assert!(text.contains("Queue"), "headed by its name");
        assert!(text.contains("First") && text.contains("Second"), "the rows are the tracks");
        // Under the heading, past the blank and the column headers: the
        // first track is not on the first three lines.
        let first_row = text.lines().position(|l| l.contains("First")).unwrap();
        assert!(first_row >= 3, "the list sits under its heading, not over it");
    }

    #[test]
    fn a_short_pane_shows_fewer_rows_and_the_keys_know_it() {
        assert_eq!(visible_rows(HEADING_ROWS), 0, "no room under the heading");
        assert!(visible_rows(30) > visible_rows(12), "more room, more rows");
    }
}
