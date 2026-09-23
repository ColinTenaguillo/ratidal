//! The one box text is typed into.
//!
//! The search field and every view's filter are the same thing — a line you
//! type into that says whether it has the keyboard — so they are drawn by
//! one routine. They had drifted: the search box was a bordered box and the
//! filters were shaded lines, which read as two different kinds of control.

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use super::theme::Palette;

/// The rows a box takes: its border, the line inside it, and the border
/// again.
pub const HEIGHT: u16 = 3;

/// How wide a box is drawn.
///
/// Not the whole pane: run the width of a wide terminal it reads as a panel
/// rather than a field, and the web client's own is a fraction of its
/// window.
pub const WIDTH: u16 = 60;

/// Draw a box at `area`'s top-left, `HEIGHT` rows tall.
///
/// `hint` shows when there is nothing typed and the box does not have the
/// keyboard — a hint under a caret reads as text somebody already entered.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    hint: &str,
    text: &str,
    focused: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    // A full block rather than a half one: a selected row is capped with
    // `▌`, and two marks that look alike a few rows apart read as one.
    let caret = if focused { "█" } else { "" };
    let (line, style) = if text.is_empty() && !focused {
        (format!(" {hint}"), palette.subtitle())
    } else {
        (format!(" {text}{caret}"), palette.title())
    };

    frame.render_widget(
        Paragraph::new(Line::styled(line, style)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                // Brighter while it has the keyboard, but never the accent:
                // a lit green box shouts louder than the results under it,
                // and the accent marks what is on rather than what is
                // focused.
                .border_style(if focused {
                    palette.subtitle()
                } else {
                    palette.rule()
                }),
        ),
        Rect {
            width: area.width.min(WIDTH),
            height: area.height.min(HEIGHT),
            ..area
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::geometry;

    fn drawn(hint: &str, text: &str, focused: bool) -> ratatui::buffer::Buffer {
        let (hint, text) = (hint.to_string(), text.to_string());
        geometry::draw(70, 5, move |f, area, palette| {
            render(f, area, palette, &hint, &text, focused);
        })
    }

    #[test]
    fn the_text_sits_inside_a_rounded_box() {
        let buf = drawn("Filter", "daft", true);
        let text = geometry::text(&buf);
        for corner in ['╭', '╮', '╰', '╯'] {
            assert!(text.contains(corner), "the box has its {corner}:\n{text}");
        }
        let typed = geometry::find(&buf, "daft").expect("the text");
        let top = geometry::find(&buf, "╭").expect("the top");
        let bottom = geometry::find(&buf, "╰").expect("the bottom");
        assert!(top.row < typed.row && typed.row < bottom.row, "{text}");
    }

    #[test]
    fn the_hint_gives_way_to_a_caret() {
        // A hint under a caret reads as text somebody already entered.
        let idle = geometry::text(&drawn("Filter tracks", "", false));
        assert!(idle.contains("Filter tracks"), "the hint shows:\n{idle}");

        let typing = geometry::text(&drawn("Filter tracks", "", true));
        assert!(
            !typing.contains("Filter tracks"),
            "and gives way:\n{typing}"
        );
        assert!(typing.contains('█'), "to a caret:\n{typing}");
    }

    #[test]
    fn the_border_brightens_with_focus_but_is_never_the_accent() {
        // The accent marks what is on — a favourite, a mode — not what has
        // the keyboard, and a lit green box shouts over its own results.
        let palette = Palette::detect();
        let focused = drawn("Filter", "", true);
        let idle = drawn("Filter", "", false);
        let at = geometry::find(&focused, "╭").expect("the corner");

        assert_ne!(
            focused[(at.start, at.row)].fg,
            idle[(at.start, at.row)].fg,
            "the two states differ"
        );
        assert_ne!(
            focused[(at.start, at.row)].fg,
            palette.accent,
            "and neither is the accent"
        );
    }

    #[test]
    fn a_pane_too_small_for_a_box_does_not_panic() {
        for (w, h) in [(0, 0), (1, 1), (3, 2), (70, 1)] {
            let buf = geometry::draw(w.max(1), h.max(1), |f, area, palette| {
                render(f, area, palette, "Filter", "text", true);
            });
            let _ = geometry::text(&buf);
        }
    }
}
