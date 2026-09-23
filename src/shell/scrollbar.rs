//! A scrollbar for the views that scroll.
//!
//! The home page, the card grids and the track list all show a window onto a
//! longer list, and none of them said so: a view that ended because it ran
//! out of room looked exactly like one that had reached its end.
//!
//! The column is reserved whether or not the bar is drawn. Taking it only
//! when the content overflows would shift the whole layout the moment one
//! more item arrived.

use ratatui::layout::Rect;
use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;

use super::theme::Palette;

/// The column the bar sits in.
pub const WIDTH: u16 = 1;

/// The area left for content once the bar has its column.
pub fn reserve(area: Rect) -> Rect {
    Rect {
        width: area.width.saturating_sub(WIDTH),
        ..area
    }
}

/// Draw the bar beside `area`, which must already have been through
/// [`reserve`] — it is drawn in the column that reserved.
///
/// Nothing is drawn when everything fits: a full-length bar against a list
/// with no more to show says there is somewhere else to go.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    total: usize,
    position: usize,
    visible: usize,
) {
    if total <= visible || visible == 0 || area.height == 0 {
        return;
    }
    // The widget spreads `position` over `0..content_length - 1`, but an
    // offset only ever reaches `total - visible`: the last screenful is
    // still a screenful. Handing it the item count left the thumb short of
    // the end — 34 of 39 on a forty-item list — with the list already at
    // its bottom. What it wants is the number of scroll positions.
    let positions = total - visible + 1;
    let mut state = ScrollbarState::new(positions)
        .position(position.min(positions - 1))
        .viewport_content_length(visible);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            // The thumb is the bright part: in the border colour it was
            // invisible against the pane.
            .thumb_style(palette.title())
            .track_style(palette.rule())
            .begin_symbol(None)
            .end_symbol(None),
        Rect {
            x: area.x + area.width,
            y: area.y,
            width: WIDTH,
            height: area.height,
        },
        &mut state,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::geometry;

    fn drawn(total: usize, position: usize, visible: usize) -> String {
        let buf = geometry::draw(20, 10, move |f, area, palette| {
            let area = reserve(area);
            render(f, area, palette, total, position, visible);
        });
        geometry::text(&buf)
    }

    /// Which rows of the bar carry the thumb rather than the track.
    fn thumb(total: usize, position: usize, visible: usize, height: u16) -> Vec<u16> {
        let buf = geometry::draw(20, height, move |f, area, palette| {
            let area = reserve(area);
            render(f, area, palette, total, position, visible);
        });
        // The thumb is a full block; the track is a lighter rule.
        (0..height)
            .filter(|y| buf[(19, *y)].symbol() == "\u{2588}")
            .collect()
    }

    #[test]
    fn the_thumb_tracks_the_position_across_the_whole_list() {
        // Reaching the bottom was checked, but not the way there: an
        // off-by-one in the position count moves the thumb everywhere
        // except at the two ends, where clamping hides it.
        // What this holds is the shape — the thumb only ever moves down,
        // and reaches the end when the list does. It does not pin the exact
        // count: one position too many shifts the thumb by 0.07 of a row at
        // mid-track, which no cell boundary can show.
        let (total, visible, height) = (400usize, 6usize, 60u16);
        let mut last = 0;
        for offset in 0..=(total - visible) {
            let rows = thumb(total, offset, visible, height);
            let top = *rows.first().expect("the thumb is drawn");
            assert!(
                top >= last,
                "the thumb went backwards at offset {offset}: {top} after {last}"
            );
            last = top;
        }
        // The last offset is the end of the list, so the thumb's top has
        // to be at the end of the track. One position too many leaves it
        // short — invisible on a short bar, plain on a long one.
        let rows = thumb(total, total - visible, visible, height);
        assert_eq!(
            *rows.last().unwrap(),
            height - 1,
            "the thumb reaches the last row of a tall track"
        );
        assert!(last > 0, "and travelled to get there");
    }

    #[test]
    fn the_thumb_reaches_the_bottom_at_the_end_of_the_list() {
        // The widget spreads `position` over the content length, but an
        // offset only reaches `total - visible`: handed the item count, the
        // thumb stopped short of the end while the list was already at its
        // bottom.
        let (total, visible, height) = (40usize, 6usize, 12u16);
        let rows = thumb(total, total - visible, visible, height);
        assert!(!rows.is_empty(), "the thumb is drawn");
        assert_eq!(
            *rows.last().unwrap(),
            height - 1,
            "it reaches the last row when the list does"
        );
    }

    #[test]
    fn the_thumb_starts_at_the_top_at_the_start_of_the_list() {
        let rows = thumb(40, 0, 6, 12);
        assert_eq!(*rows.first().unwrap(), 0, "it starts at the first row");
    }

    #[test]
    fn an_offset_past_the_end_does_not_panic() {
        // Nothing should pass one, but the clamp is what keeps a stale
        // offset from indexing past the track.
        let _ = thumb(10, 99, 3, 10);
    }

    #[test]
    fn a_list_that_fits_shows_no_bar() {
        // A full-length bar against a list with nothing more to show says
        // there is somewhere else to go.
        let text = drawn(3, 0, 5);
        assert!(
            text.chars().all(|c| c == ' ' || c == '\n'),
            "nothing is drawn:\n{text}"
        );
    }

    #[test]
    fn a_longer_list_shows_one() {
        let text = drawn(20, 0, 5);
        assert!(
            text.chars().any(|c| c != ' ' && c != '\n'),
            "the bar is drawn:\n{text}"
        );
    }

    #[test]
    fn the_thumb_follows_the_position() {
        // Top and bottom have to look different, or the bar says nothing
        // beyond "there is more".
        assert_ne!(drawn(20, 0, 5), drawn(20, 15, 5));
    }

    #[test]
    fn the_column_is_reserved_whether_or_not_a_bar_is_drawn() {
        // Taking it only on overflow would shift the layout the moment one
        // more item arrived.
        let area = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 10,
        };
        assert_eq!(reserve(area).width, 39);
    }

    #[test]
    fn a_pane_too_narrow_for_the_bar_does_not_underflow() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 10,
        };
        assert_eq!(reserve(area).width, 0);
    }
}
