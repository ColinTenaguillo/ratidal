//! The key bindings, on `?`.
//!
//! The bar at the bottom of the screen only has room for the handful of keys
//! that matter in the current view, and it had been quietly wrong about which
//! pane j and k drove. This is the full list, in one place, so the bar can
//! stay short without the rest being undiscoverable.

use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::theme::Palette;

/// One binding, or a heading when `keys` is empty.
struct Binding {
    keys: &'static str,
    what: &'static str,
}

const BINDINGS: &[Binding] = &[
    Binding { keys: "", what: "Moving around" },
    Binding { keys: "j k", what: "down / up in the list or grid" },
    Binding { keys: "h l", what: "left / right along a row" },
    Binding { keys: "J K", what: "move through the sidebar" },
    Binding { keys: "1 - 9", what: "straight to a nav entry, 1 is Music" },
    Binding { keys: "enter", what: "open a playlist or album, or play a track" },
    Binding { keys: "esc [", what: "back one view, or out of the filter" },
    Binding { keys: "]", what: "forward again, through views and sections" },
    Binding { keys: "", what: "" },
    Binding { keys: "", what: "Finding things" },
    Binding { keys: "/", what: "filter the current view" },
    Binding { keys: "s", what: "search the catalogue" },
    Binding { keys: "o", what: "see all of a home row" },
    Binding { keys: "b", what: "open an artist's biography" },
    Binding { keys: "a", what: "open the artist of the selected track" },
    Binding { keys: "d", what: "open its album" },
    Binding { keys: "R", what: "radio from the selected track" },
    Binding { keys: "S", what: "radio from the artist whose page is open" },

    Binding { keys: "t", what: "next tab, on the home page or in search" },
    Binding { keys: "", what: "" },
    Binding { keys: "", what: "Playing" },
    Binding { keys: "space", what: "pause or resume" },
    Binding { keys: "A", what: "favourite the track, or take it out again" },
    Binding { keys: "n p", what: "next or previous in the queue" },
    Binding { keys: "z", what: "shuffle the queue" },
    Binding { keys: "r", what: "repeat: off, all, one" },
    Binding { keys: "", what: "" },
    Binding { keys: "?", what: "this list" },
    Binding { keys: "q", what: "quit" },
];

/// Width of the key column, so the descriptions line up.
const KEY_WIDTH: usize = 7;

/// Centre a box of the given size inside `area`, clamped so it always fits.
fn centred(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

pub fn render(frame: &mut Frame, area: Rect, palette: &Palette) {
    // Two for the border, one for the closing hint and its blank line.
    let height = BINDINGS.len() as u16 + 4;
    let modal = centred(area, 56, height);
    if modal.width == 0 || modal.height == 0 {
        return;
    }

    let mut lines: Vec<Line> = BINDINGS
        .iter()
        .map(|b| {
            if b.keys.is_empty() {
                // A heading, or a blank spacer.
                return Line::styled(b.what, palette.section_heading());
            }
            Line::from(vec![
                Span::styled(format!("{:>KEY_WIDTH$}  ", b.keys), palette.accent_text()),
                Span::styled(b.what, palette.title()),
            ])
        })
        .collect();

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "any key to close",
        palette.subtitle(),
    ));

    frame.render_widget(Clear, modal);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(palette.rule())
                    .title(" Keys ")
                    .title_style(palette.page_heading()),
            ),
        modal,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_binding_has_a_description() {
        for b in BINDINGS {
            if !b.keys.is_empty() {
                assert!(!b.what.is_empty(), "{:?} has no description", b.keys);
            }
        }
    }

    #[test]
    fn no_key_overflows_its_column() {
        // The descriptions line up only if every key string fits.
        for b in BINDINGS {
            assert!(
                b.keys.chars().count() <= KEY_WIDTH,
                "{:?} is wider than the key column",
                b.keys
            );
        }
    }

    #[test]
    fn rendering_into_a_pane_smaller_than_the_modal_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let palette = Palette::detect();
        for (w, h) in [(1, 1), (10, 4), (56, 24), (200, 60)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| render(f, f.area(), &palette)).unwrap();
        }
    }

    #[test]
    fn every_key_the_app_binds_is_listed() {
        // The guard below checks that what is documented exists. This is
        // the other direction: a key nobody can discover is a key nobody
        // uses, and four have been added since the list was written.
        let source = include_str!("mod.rs");
        let listed: std::collections::HashSet<&str> = BINDINGS
            .iter()
            .flat_map(|b| b.keys.split_whitespace())
            .collect();

        let mut missing = Vec::new();
        for line in source.lines() {
            // `KeyCode::Char('x')` in the key handler, not in a test.
            let Some(rest) = line.split("KeyCode::Char('").nth(1) else {
                continue;
            };
            let Some(key) = rest.chars().next() else { continue };
            if key == ' ' {
                continue; // listed as "space"
            }
            if !listed.contains(key.to_string().as_str()) {
                missing.push(key);
            }
        }
        missing.sort_unstable();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "these keys are bound but not in the help: {missing:?}"
        );
    }

    #[test]
    fn the_listed_keys_are_the_ones_the_app_binds() {
        // A help screen that drifts from the bindings is worse than none:
        // this is the list people will trust. Every key here must appear in
        // `on_key`, and the ones that move around must be listed here.
        let source = include_str!("mod.rs");
        for keys in BINDINGS.iter().filter(|b| !b.keys.is_empty()) {
            for key in keys.keys.split_whitespace() {
                let bound = match key {
                    "tab" => source.contains("KeyCode::Tab"),
                    "enter" => source.contains("KeyCode::Enter"),
                    "esc" => source.contains("KeyCode::Esc"),
                    "space" => source.contains("KeyCode::Char(' ')"),
                    // A run of keys bound as one pattern: the nav numbers
                    // are `'1'..='9'`, not nine separate arms.
                    "-" => true,
                    k => {
                        source.contains(&format!("KeyCode::Char('{k}')"))
                            || source.contains(&format!("'{k}'..="))
                            || source.contains(&format!("..='{k}'"))
                    }
                };
                assert!(bound, "{key:?} is documented but not bound in on_key");
            }
        }
    }
}
