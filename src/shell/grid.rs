//! A wrapping grid of cover cards: the Playlists, Albums and Profils views.
//!
//! All three are the same picture — a heading, a filter box, then rows of
//! cards that wrap at the pane's width. What differs is the card: playlists
//! carry a track count, albums a year, artists a round avatar and no
//! subtitle. So the grid takes cards and draws them; it does not know what
//! it is showing.
//!
//! Cards are drawn by `carousel::render_card`, the same routine the home
//! rows use, so a change to how a card looks lands everywhere at once.

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::carousel::{card_height, render_card, Card, CARD_WIDTH};
use super::theme::Palette;

/// The gutter between columns, matching the carousel's.
const GAP: u16 = 3;
/// Blank rows between one row of cards and the next.
const ROW_GAP: u16 = 1;

#[derive(Debug, Default, Clone)]
pub struct GridState {
    pub selected: usize,
    /// First visible row of cards, in card-rows not terminal rows.
    pub offset: usize,
    /// What the user has typed into the filter box.
    pub filter: String,
}

/// How many cards fit across `width`.
pub fn columns(width: u16) -> usize {
    if width < CARD_WIDTH {
        return 0;
    }
    // n cards need n*CARD_WIDTH + (n-1)*GAP columns.
    (((width + GAP) / (CARD_WIDTH + GAP)) as usize).max(1)
}

/// How many rows of cards fit in `height`, given cards `lines` tall.
pub fn rows(height: u16, lines: u16) -> usize {
    let step = card_height(lines) + ROW_GAP;
    if height < card_height(lines) {
        return 0;
    }
    // The last row needs no trailing gap.
    (((height + ROW_GAP) / step) as usize).max(1)
}

impl GridState {
    pub fn next(&mut self, len: usize, cols: usize, visible_rows: usize) {
        if len == 0 {
            return;
        }
        self.selected = (self.selected + 1).min(len - 1);
        self.scroll_into_view(cols, visible_rows);
    }

    pub fn previous(&mut self, cols: usize, visible_rows: usize) {
        self.selected = self.selected.saturating_sub(1);
        self.scroll_into_view(cols, visible_rows);
    }

    /// Down a whole row, which is what `j` does in a grid — moving one card
    /// at a time down a 6-wide grid would take six presses per line.
    pub fn next_row(&mut self, len: usize, cols: usize, visible_rows: usize) {
        if len == 0 {
            return;
        }
        self.selected = (self.selected + cols.max(1)).min(len - 1);
        self.scroll_into_view(cols, visible_rows);
    }

    pub fn previous_row(&mut self, cols: usize, visible_rows: usize) {
        self.selected = self.selected.saturating_sub(cols.max(1));
        self.scroll_into_view(cols, visible_rows);
    }

    fn scroll_into_view(&mut self, cols: usize, visible_rows: usize) {
        let cols = cols.max(1);
        let visible_rows = visible_rows.max(1);
        let row = self.selected / cols;
        if row < self.offset {
            self.offset = row;
        } else if row >= self.offset + visible_rows {
            self.offset = row + 1 - visible_rows;
        }
    }

    /// Keep the selection inside a list that may have shrunk under it — the
    /// filter box does exactly that on every keystroke.
    pub fn clamp(&mut self, len: usize) {
        if len == 0 {
            self.selected = 0;
            self.offset = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }
}

/// Case-insensitive substring match on title and subtitle, which is what the
/// web client's filter box does.
pub fn filter<'a>(cards: &'a [Card], needle: &str) -> Vec<&'a Card> {
    if needle.is_empty() {
        return cards.iter().collect();
    }
    let needle = needle.to_lowercase();
    cards
        .iter()
        .filter(|c| {
            c.title.to_lowercase().contains(&needle)
                || c.subtitle.to_lowercase().contains(&needle)
        })
        .collect()
}

/// The positions in `cards` that a filter keeps, in order. Callers that need
/// to map a selection back to the item behind it use this rather than
/// searching the filtered list.
pub fn filter_indices(cards: &[Card], needle: &str) -> Vec<usize> {
    if needle.is_empty() {
        return (0..cards.len()).collect();
    }
    let needle = needle.to_lowercase();
    cards
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            c.title.to_lowercase().contains(&needle)
                || c.subtitle.to_lowercase().contains(&needle)
        })
        .map(|(i, _)| i)
        .collect()
}

/// How much of its own chrome the grid draws above the cards.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Chrome {
    /// Heading and filter box.
    #[default]
    Full,
    /// Neither: the caller has drawn its own. Search reuses this grid under
    /// its tabs, and a second heading with a second filter box under the
    /// search box would be the same furniture twice.
    Bare,
}

pub struct Grid<'a> {
    pub heading: &'a str,
    /// Placeholder text for the filter box, e.g. "Filtrer playlists".
    pub filter_hint: &'a str,
    pub cards: &'a [&'a Card],
    pub state: &'a GridState,
    pub focused: bool,
    /// Lines of text under each cover: 2 for albums, 3 for playlists, 1 for
    /// profiles. Fixed per view so rows line up.
    pub lines: u16,
    pub chrome: Chrome,
}

/// Render heading, filter box and the visible page of cards.
///
/// `draw_cover` matches the carousel's: it is handed each cover's area and
/// returns whether it drew a real image, so a terminal that cannot show
/// pictures gets the same layout with placeholders.
pub fn render<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    grid: Grid<'_>,
    mut draw_cover: F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    let Grid { heading, filter_hint, cards, state, focused, lines, chrome } = grid;
    if area.width == 0 || area.height == 0 {
        return;
    }

    let body_y = match chrome {
        Chrome::Bare => area.y,
        Chrome::Full => {
            frame.render_widget(
                Paragraph::new(Line::styled(heading, palette.page_heading())),
                Rect { height: 1, ..area },
            );
            // Heading, a blank line, the filter box, a blank line, then the
            // cards.
            let filter_y = area.y + 2;
            if filter_y < area.y + area.height {
                render_filter(
                    frame,
                    Rect { y: filter_y, height: 1, ..area },
                    palette,
                    filter_hint,
                    state,
                );
            }
            filter_y + 2
        }
    };
    if body_y >= area.y + area.height {
        return;
    }
    let body = Rect {
        x: area.x,
        y: body_y,
        width: area.width,
        height: area.y + area.height - body_y,
    };

    let cols = columns(body.width);
    let visible_rows = rows(body.height, lines);
    if cols == 0 || visible_rows == 0 {
        return;
    }

    let step_y = card_height(lines) + ROW_GAP;
    let first = state.offset * cols;

    for (i, card) in cards.iter().enumerate().skip(first) {
        let row = i / cols - state.offset;
        if row >= visible_rows {
            break;
        }
        let col = i % cols;
        let x = body.x + col as u16 * (CARD_WIDTH + GAP);
        let y = body.y + row as u16 * step_y;
        // A card that does not fit across is not drawn: clipped to what was
        // left it painted a sliver of cover under a truncated title, which
        // reads as a fault rather than as a grid that continues. `columns`
        // already says how many fit, so this only catches the rounding.
        if x + CARD_WIDTH > body.x + body.width || y >= body.y + body.height {
            continue;
        }
        // Height still clips: a part-drawn bottom row is what scrolling
        // through a long grid looks like, and Rect is u16 either way.
        let card_area = Rect {
            x,
            y,
            width: CARD_WIDTH,
            height: card_height(lines).min(body.y + body.height - y),
        };
        render_card(
            frame,
            card_area,
            palette,
            card,
            focused && i == state.selected,
            &mut draw_cover,
        );
    }
}

fn render_filter(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    hint: &str,
    state: &GridState,
) {
    let (text, style) = if state.filter.is_empty() {
        (format!("  {hint}"), palette.subtitle())
    } else {
        (format!("  {}", state.filter), palette.title())
    };
    frame.render_widget(
        Paragraph::new(Line::styled(text, style))
            .block(ratatui::widgets::Block::default().style(
                ratatui::style::Style::default().bg(palette.surface),
            )),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards(n: usize) -> Vec<Card> {
        (0..n).map(|i| Card::new(format!("Item {i}"), "Coco")).collect()
    }

    #[test]
    fn columns_account_for_the_gutters() {
        // Same geometry as the carousel: 16-wide cards, 3-column gutter.
        assert_eq!(columns(16), 1);
        assert_eq!(columns(34), 1);
        assert_eq!(columns(35), 2);
        assert_eq!(columns(114), 6);
        assert_eq!(columns(15), 0);
    }

    #[test]
    fn rows_account_for_the_row_gap() {
        // A 3-line card is 8 + 3 = 11 tall, plus 1 blank between rows.
        assert_eq!(rows(10, 3), 0, "less than one card does not fit");
        assert_eq!(rows(11, 3), 1);
        assert_eq!(rows(22, 3), 1, "a second row needs the gap too");
        assert_eq!(rows(23, 3), 2);
    }

    #[test]
    fn moving_down_advances_a_whole_row() {
        // One card at a time down a 6-wide grid would be six presses a line.
        let mut s = GridState::default();
        s.next_row(30, 6, 3);
        assert_eq!(s.selected, 6);
        s.next_row(30, 6, 3);
        assert_eq!(s.selected, 12);
        s.previous_row(6, 3);
        assert_eq!(s.selected, 6);
    }

    #[test]
    fn moving_down_stops_on_the_last_card_not_past_it() {
        // A short final row must not let the selection leave the list.
        let mut s = GridState::default();
        for _ in 0..10 {
            s.next_row(14, 6, 3);
        }
        assert_eq!(s.selected, 13);
    }

    #[test]
    fn scrolling_follows_the_selection_down_and_back() {
        let mut s = GridState::default();
        // 6 columns, 2 visible rows: reaching row 2 must pull the window.
        for _ in 0..3 {
            s.next_row(60, 6, 2);
        }
        assert_eq!(s.selected, 18, "row 3");
        assert_eq!(s.offset, 2, "the window shows rows 2..4");

        for _ in 0..3 {
            s.previous_row(6, 2);
        }
        assert_eq!(s.selected, 0);
        assert_eq!(s.offset, 0, "the window follows back to the top");
    }

    #[test]
    fn the_filter_matches_title_and_subtitle_ignoring_case() {
        let mut c = cards(3);
        c[0].title = "Coco summer".into();
        c[1].title = "Jazzy".into();
        c[1].subtitle = "TIDAL".into();
        c[2].title = "Chill".into();

        // "Coco" is c[0]'s title and c[2]'s subtitle, so it matches both.
        assert_eq!(filter(&c, "coco").len(), 2);
        assert_eq!(filter(&c, "SUMMER").len(), 1, "matching must ignore case");
        assert_eq!(filter(&c, "tidal").len(), 1, "the subtitle counts too");
        assert_eq!(filter(&c, "").len(), 3, "an empty filter keeps everything");
        assert_eq!(filter(&c, "zzzz").len(), 0);
    }

    #[test]
    fn filtering_pulls_the_selection_back_into_the_list() {
        // Typing in the filter box shrinks the list under the selection; a
        // stale index would point past the end and select nothing.
        let mut s = GridState { selected: 40, offset: 6, filter: String::new() };
        s.clamp(3);
        assert_eq!(s.selected, 2);
        s.clamp(0);
        assert_eq!(s.selected, 0);
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn rendering_into_a_pane_too_small_for_a_card_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let palette = Palette::detect();
        let all = cards(20);
        let refs: Vec<&Card> = all.iter().collect();
        let state = GridState::default();

        for (w, h) in [(1, 1), (10, 30), (120, 3), (16, 12), (2, 2)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| {
                render(
                    f,
                    f.area(),
                    &palette,
                    Grid {
                        chrome: Chrome::Full,
                        heading: "Playlists",
                        filter_hint: "Filtrer playlists",
                        cards: &refs,
                        state: &state,
                        focused: true,
                        lines: 3,
                    },
                    |_, _, _, _| false,
                )
            })
            .unwrap();
        }
    }

    #[test]
    fn the_grid_wraps_onto_several_rows() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let palette = Palette::detect();
        let all = cards(12);
        let refs: Vec<&Card> = all.iter().collect();
        let state = GridState::default();

        // Wide enough for 2 columns, tall enough for 2 rows of cards.
        let mut term = Terminal::new(TestBackend::new(35, 30)).unwrap();
        term.draw(|f| {
            render(
                f,
                f.area(),
                &palette,
                Grid {
                    chrome: Chrome::Full,
                    heading: "Albums",
                    filter_hint: "Filtrer Albums",
                    cards: &refs,
                    state: &state,
                    focused: true,
                    lines: 2,
                },
                |_, _, _, _| false,
            )
        })
        .unwrap();

        let text = buffer_text(term.backend().buffer());
        assert!(text.contains("Albums"), "the heading must be drawn");
        assert!(text.contains("Item 0"));
        // Item 1 sits in the second column, Item 2 wraps to the next row.
        assert!(text.contains("Item 1"));
        assert!(text.contains("Item 2"), "the grid must wrap, not run off the edge");
    }

    fn buffer_text(buf: &ratatui::buffer::Buffer) -> String {
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
