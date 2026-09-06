//! The pane while an opened view waits on the request that fills it.
//!
//! What comes back decides which view this becomes -- a genre is a page of
//! rows, an album a list of tracks -- so until it does, neither shape can
//! be drawn. Drawing the empty one meanwhile showed a filter box and a set
//! of column headings over nothing, which reads as a view that is broken
//! rather than one still loading.

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::theme::Palette;

/// Draw the heading of what is opening, and a line saying it is on its way.
pub fn render(frame: &mut Frame, area: Rect, palette: &Palette, title: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    // The heading first, so the pane names what the user just opened rather
    // than going blank under them.
    frame.render_widget(
        Paragraph::new(Line::styled(title.to_string(), palette.title())),
        Rect { height: 1, ..area },
    );
    if area.height > 2 {
        frame.render_widget(
            Paragraph::new(Line::styled("Loading…", palette.subtitle())),
            Rect { y: area.y + 2, height: 1, ..area },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::geometry;

    #[test]
    fn it_names_what_is_opening_rather_than_going_blank() {
        // The pane must not empty under the user: they pressed enter on a
        // title, and that title is what heads the wait.
        let buf = geometry::draw(60, 10, |f, area, palette| {
            render(f, area, palette, "Hip-Hop");
        });
        let text = geometry::text(&buf);
        assert!(text.contains("Hip-Hop"), "the name of what is opening:\n{text}");
        assert!(text.contains("Loading"), "and that it is on its way:\n{text}");
    }

    #[test]
    fn a_pane_too_short_for_the_line_still_draws_the_heading() {
        let buf = geometry::draw(60, 1, |f, area, palette| {
            render(f, area, palette, "Hip-Hop");
        });
        let text = geometry::text(&buf);
        assert!(text.contains("Hip-Hop"), "the heading fits in one row:\n{text}");
    }

    #[test]
    fn a_pane_with_no_room_draws_nothing_rather_than_panicking() {
        let buf = geometry::draw(60, 10, |f, area, palette| {
            render(f, Rect { width: 0, height: 0, ..area }, palette, "Hip-Hop");
        });
        assert!(geometry::text(&buf).trim().is_empty());
    }
}
