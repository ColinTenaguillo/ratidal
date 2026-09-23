//! Asking a rendered frame where things are, rather than whether they exist.
//!
//! Every test of the player bar so far asserted on content — "the text
//! contains 1:51" — which passes whichever column that lands in. So a run of
//! alignment faults each had to be found by eye, one at a time: the bar
//! centred on the wrong region, the cover pressed into the corner, the
//! elapsed time flush against the bar while the total had air after it.
//! Each fix was correct and none of them was checked by anything.
//!
//! These helpers report positions, so a test can say where something belongs
//! and fail when it moves.

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::Terminal;

use super::theme::Palette;

/// Render into a buffer of the given size, with the palette `Palette::detect`
/// reads off the real terminal environment rather than one passed in — so a
/// test sees the colours the user does, and CI and a truecolor terminal can
/// disagree about them. Panics on terminal or draw failure; in a test that is
/// the report.
pub fn draw<F>(width: u16, height: u16, f: F) -> Buffer
where
    F: FnOnce(&mut ratatui::Frame, Rect, &Palette),
{
    let palette = Palette::detect();
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|frame| f(frame, frame.area(), &palette))
        .expect("draw");
    terminal.backend().buffer().clone()
}

/// One row of the buffer as a string, trailing blanks kept — a caller
/// measuring columns needs them.
pub fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

/// Every row, for a failure message that shows the whole frame.
pub fn text(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|y| row(buf, y))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Where a run of `needle` sits: the columns of its first and last cell.
///
/// Returned as a half-open span so `width()` and `centre()` are the obvious
/// arithmetic rather than something with an off-by-one in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u16,
    /// One past the last column, as ranges elsewhere.
    pub end: u16,
    pub row: u16,
}

impl Span {
    pub fn width(&self) -> u16 {
        self.end.saturating_sub(self.start)
    }

    /// The midpoint, doubled, so a span of even width has an exact centre
    /// without a float. Compare two of these directly.
    pub fn centre_x2(&self) -> u16 {
        self.start + self.end.saturating_sub(1)
    }

    /// Midpoint as a percentage of `width`, for comparing against a
    /// measurement taken from somewhere else.
    pub fn centre_pct(&self, width: u16) -> f64 {
        self.centre_x2() as f64 / 2.0 / width as f64 * 100.0
    }

    pub fn width_pct(&self, width: u16) -> f64 {
        self.width() as f64 / width as f64 * 100.0
    }
}

/// The span of the longest unbroken run of any of `chars` in the buffer.
///
/// Takes a set rather than one character because a progress bar is drawn in
/// two glyphs — played and remaining — and a helper that matched only one
/// would report half the bar and call it the whole thing.
pub fn longest_run_of(buf: &Buffer, chars: &[char]) -> Option<Span> {
    let mut best: Option<Span> = None;
    for y in 0..buf.area.height {
        let cells: Vec<char> = row(buf, y).chars().collect();
        let mut x = 0usize;
        while x < cells.len() {
            if !chars.contains(&cells[x]) {
                x += 1;
                continue;
            }
            let start = x;
            while x < cells.len() && chars.contains(&cells[x]) {
                x += 1;
            }
            let span = Span {
                start: start as u16,
                end: x as u16,
                row: y,
            };
            if best.is_none_or(|b| span.width() > b.width()) {
                best = Some(span);
            }
        }
    }
    best
}

/// The longest unbroken run of cells whose foreground is one of `colours`.
///
/// Shape alone cannot always identify a thing: the progress bar and the
/// frame's own border are drawn with the same rule character, and telling
/// them apart by colour is what the eye does too.
pub fn longest_run_coloured(buf: &Buffer, colours: &[ratatui::style::Color]) -> Option<Span> {
    let mut best: Option<Span> = None;
    for y in 0..buf.area.height {
        let mut x = 0u16;
        while x < buf.area.width {
            if !colours.contains(&buf[(x, y)].fg) {
                x += 1;
                continue;
            }
            let start = x;
            while x < buf.area.width && colours.contains(&buf[(x, y)].fg) {
                x += 1;
            }
            let span = Span {
                start,
                end: x,
                row: y,
            };
            if best.is_none_or(|b| span.width() > b.width()) {
                best = Some(span);
            }
        }
    }
    best
}

/// The span of the longest unbroken run of `ch` anywhere in the buffer.
pub fn longest_run(buf: &Buffer, ch: char) -> Option<Span> {
    let mut best: Option<Span> = None;
    for y in 0..buf.area.height {
        let line = row(buf, y);
        let cells: Vec<char> = line.chars().collect();
        let mut x = 0usize;
        while x < cells.len() {
            if cells[x] != ch {
                x += 1;
                continue;
            }
            let start = x;
            while x < cells.len() && cells[x] == ch {
                x += 1;
            }
            let span = Span {
                start: start as u16,
                end: x as u16,
                row: y,
            };
            if best.is_none_or(|b| span.width() > b.width()) {
                best = Some(span);
            }
        }
    }
    best
}

/// The first place `needle` shows up: topmost row, then leftmost column in
/// that row. Callers compare the rows of several finds to assert which
/// section is drawn above which, so the top-down scan is the guarantee.
///
/// A row is matched on its own, so a needle wrapped across a line break is
/// never found.
pub fn find(buf: &Buffer, needle: &str) -> Option<Span> {
    for y in 0..buf.area.height {
        let line = row(buf, y);
        if let Some(byte) = line.find(needle) {
            // Byte offset to column: the buffer is one char per cell, but a
            // multi-byte char would otherwise shift everything after it.
            let start = line[..byte].chars().count() as u16;
            let width = needle.chars().count() as u16;
            return Some(Span {
                start,
                end: start + width,
                row: y,
            });
        }
    }
    None
}

/// The columns of a row that show something, as a span from first to last.
/// A row with nothing on it gives `None`.
///
/// A cell counts as painted if it has a glyph OR a background colour. A
/// cover thumbnail is a coloured block with no character in it, so testing
/// the symbol alone reports an empty row and an assertion about where the
/// cover sits passes without looking at the cover — which is how the flush
/// left edge went unnoticed.
pub fn occupied(buf: &Buffer, y: u16) -> Option<Span> {
    let painted = |x: u16| {
        let cell = &buf[(x, y)];
        cell.symbol() != " " || cell.bg != ratatui::style::Color::Reset
    };
    let first = (0..buf.area.width).find(|x| painted(*x))?;
    let last = (0..buf.area.width).rev().find(|x| painted(*x))?;
    Some(Span {
        start: first,
        end: last + 1,
        row: y,
    })
}

/// Assert two things are centred on each other, within a cell.
///
/// A character grid cannot always put a midpoint on a boundary: an odd run
/// in an even field is half a cell off whatever you do. So this allows one
/// cell of slack and fails on anything more, which is the difference
/// between rounding and a bug.
#[track_caller]
pub fn assert_centred_on(what: Span, on: Span, buf: &Buffer) {
    let diff = what.centre_x2().abs_diff(on.centre_x2());
    assert!(
        diff <= 1,
        "expected centres within half a cell, but {:?} centres at {} and {:?} at {} \
         (doubled), a gap of {}\n{}",
        what,
        what.centre_x2(),
        on,
        on.centre_x2(),
        diff,
        text(buf)
    );
}

/// Assert something is centred in the frame.
#[track_caller]
pub fn assert_centred(what: Span, buf: &Buffer) {
    let field = Span {
        start: 0,
        end: buf.area.width,
        row: what.row,
    };
    assert_centred_on(what, field, buf);
}

/// Assert the gap between two spans on the same row, in columns.
#[track_caller]
pub fn assert_gap(left: Span, right: Span, columns: u16, buf: &Buffer) {
    let actual = right.start.saturating_sub(left.end);
    assert_eq!(
        actual,
        columns,
        "expected {columns} columns between {left:?} and {right:?}, found {actual}\n{}",
        text(buf)
    );
}

/// Whether this cell is part of a placeholder disc.
///
/// The disc is drawn in sextants, which are inked in the placeholder colour
/// rather than filling the cell's background -- so a test that asks only
/// about the background misses every cell on the curve, which is most of
/// the ones that matter.
pub fn is_disc(buf: &Buffer, x: u16, y: u16, placeholder: ratatui::style::Color) -> bool {
    let cell = &buf[(x, y)];
    cell.bg == placeholder || cell.fg == placeholder
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(width: u16, from: u16, to: u16) -> Buffer {
        draw(width, 1, |frame, area, _| {
            let run: String = "▁".repeat((to - from) as usize);
            frame.render_widget(
                ratatui::widgets::Paragraph::new(run),
                Rect {
                    x: from,
                    width: to - from,
                    ..area
                },
            );
        })
    }

    #[test]
    fn a_run_reports_where_it_sits() {
        let buf = bar(20, 5, 15);
        let span = longest_run(&buf, '▁').expect("a run");
        assert_eq!((span.start, span.end), (5, 15));
        assert_eq!(span.width(), 10);
    }

    #[test]
    fn the_longest_run_wins_over_a_shorter_one() {
        let buf = draw(20, 1, |frame, area, _| {
            frame.render_widget(ratatui::widgets::Paragraph::new("▁▁  ▁▁▁▁▁"), area);
        });
        let span = longest_run(&buf, '▁').expect("a run");
        assert_eq!(span.width(), 5, "the five, not the two");
        assert_eq!(span.start, 4);
    }

    #[test]
    fn centring_allows_the_grid_its_half_cell_and_no_more() {
        // An even run in an even field lands exactly.
        let buf = bar(20, 5, 15);
        assert_centred(longest_run(&buf, '▁').unwrap(), &buf);

        // One column off is the rounding a grid cannot avoid.
        let buf = bar(20, 5, 14);
        assert_centred(longest_run(&buf, '▁').unwrap(), &buf);
    }

    #[test]
    #[should_panic(expected = "within half a cell")]
    fn centring_fails_on_a_real_lean() {
        // Two columns off is not rounding.
        let buf = bar(20, 3, 13);
        assert_centred(longest_run(&buf, '▁').unwrap(), &buf);
    }

    #[test]
    fn a_gap_is_measured_between_the_two_edges() {
        let buf = draw(20, 1, |frame, area, _| {
            frame.render_widget(ratatui::widgets::Paragraph::new("ab  cd"), area);
        });
        let a = find(&buf, "ab").unwrap();
        let b = find(&buf, "cd").unwrap();
        assert_gap(a, b, 2, &buf);
    }

    #[test]
    #[should_panic(expected = "expected 3 columns")]
    fn a_wrong_gap_fails_and_says_what_it_found() {
        let buf = draw(20, 1, |frame, area, _| {
            frame.render_widget(ratatui::widgets::Paragraph::new("ab  cd"), area);
        });
        assert_gap(
            find(&buf, "ab").unwrap(),
            find(&buf, "cd").unwrap(),
            3,
            &buf,
        );
    }

    #[test]
    fn an_empty_row_is_reported_as_empty() {
        let buf = draw(10, 2, |frame, area, _| {
            frame.render_widget(ratatui::widgets::Paragraph::new("x"), area);
        });
        assert!(occupied(&buf, 0).is_some());
        assert!(
            occupied(&buf, 1).is_none(),
            "the second row has nothing on it"
        );
    }
}
