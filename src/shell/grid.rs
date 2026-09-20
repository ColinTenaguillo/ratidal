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

use super::carousel::{card_height, card_width, render_card, Card};
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
    if width < card_width() {
        return 0;
    }
    // n cards need n*card_width() + (n-1)*GAP columns.
    (((width + GAP) / (card_width() + GAP)) as usize).max(1)
}

/// Rows above the cards: the heading, a blank, the filter box, a blank.
///
/// Shared with the shell, which has to agree with the renderer about how
/// many cards are on screen — when they disagreed the keys reached cards
/// that were never drawn.
pub fn header_rows(chrome: Chrome) -> u16 {
    header_rows_with(chrome, &[])
}

/// Rows above the cards, counting a tab strip when the view has one.
pub fn header_rows_with(chrome: Chrome, tabs: &[&str]) -> u16 {
    match chrome {
        Chrome::Bare => 0,
        // Heading, the tabs, blank, the box's three rows, blank.
        Chrome::Full => 2 + tab_rows(tabs) + super::inputbox::HEIGHT + 1,
    }
}

/// The rows a tab strip takes: itself, or nothing when there is none.
fn tab_rows(tabs: &[&str]) -> u16 {
    u16::from(!tabs.is_empty())
}

/// The card grid a section draws, and how many of its cards are visible.
pub fn geometry(width: u16, height: u16, lines: u16, chrome: Chrome) -> (usize, usize) {
    geometry_with(width, height, lines, chrome, &[])
}

/// As [`geometry`], for a view that also draws a tab strip.
pub fn geometry_with(
    width: u16,
    height: u16,
    lines: u16,
    chrome: Chrome,
    tabs: &[&str],
) -> (usize, usize) {
    let body = height.saturating_sub(header_rows_with(chrome, tabs));
    // Beside the scrollbar's column, which the renderer holds back before
    // it counts: measured against the whole pane the keys counted one
    // column more at the widths where one more just fit, and each press
    // of j moved the selection down a row and along by one.
    let width = width.saturating_sub(super::scrollbar::WIDTH);
    (columns(width), rows(body, lines))
}

/// How many rows of cards fit in `height`, given cards `lines` tall.
pub fn rows(height: u16, lines: u16) -> usize {
    let step = card_height(lines) + ROW_GAP;
    let full = ((height + ROW_GAP) / step) as usize;

    // A row at the fold shows as much of itself as fits, cut off by the
    // pane's edge rather than dropped — which is what the web client does,
    // and what makes it obvious the grid continues. Selecting it scrolls it
    // into view whole.
    let used = full as u16 * step;
    let left = height.saturating_sub(used);
    // A pane too short for a whole row is all remainder, so the partial
    // count covers it: there was a `.max` here for that case and it never
    // changed an answer, at any height for any card.
    let partial = usize::from(left >= min_partial_row(lines));

    full + partial
}

/// Rows a part-drawn row needs before it is worth showing at all.
///
/// One is a line, not a picture; two reads as artwork running past the
/// edge of the pane. The name is not the floor's business: a cut card
/// gives its last line to the label rather than to more cover, so even
/// two rows draw something that says what it is.
fn min_partial_row(_lines: u16) -> u16 {
    2
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
        // The last of `visible_rows` may be the one cut off at the fold, so
        // scrolling against the full count would leave the selection half
        // drawn. Landing on it scrolls it up into the whole part instead,
        // which is what the web client does.
        let whole_rows = visible_rows.saturating_sub(1).max(1);
        let row = self.selected / cols;
        if row < self.offset {
            self.offset = row;
        } else if row >= self.offset + whole_rows {
            self.offset = row + 1 - whole_rows;
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

/// The cards the filter box keeps: title or subtitle, matched as
/// [`super::fuzzy::matches`] matches -- without case or accents, a word at
/// a time.
pub fn filter<'a>(cards: &'a [Card], needle: &str) -> Vec<&'a Card> {
    super::fuzzy::ranked(cards.iter().enumerate(), needle, |c| vec![&c.title, &c.subtitle])
        .into_iter()
        .map(|(_, c)| c)
        .collect()
}

/// The positions in `cards` that a filter keeps, in order. Callers that need
/// to map a selection back to the item behind it use this rather than
/// searching the filtered list.
pub fn filter_indices(cards: &[Card], needle: &str) -> Vec<usize> {
    super::fuzzy::ranked(cards.iter().enumerate(), needle, |c| vec![&c.title, &c.subtitle])
        .into_iter()
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
    /// Whether the filter box has the keyboard, so it can show a caret.
    pub filtering: bool,
    /// A tab strip under the heading, and which of them is showing. Empty
    /// for a view with only one thing to show.
    pub tabs: (&'a [&'a str], usize),
    pub liked: super::carousel::Liked<'a>,
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
    let Grid {
        heading, filter_hint, cards, state, focused, lines, chrome, filtering, tabs, liked,
    } = grid;
    if area.width == 0 || area.height == 0 {
        return;
    }
    // A column for the scrollbar, held back whether or not it is drawn.
    let full = area;
    let area = super::scrollbar::reserve(area);

    let body_y = match chrome {
        Chrome::Bare => area.y,
        Chrome::Full => {
            frame.render_widget(
                Paragraph::new(Line::styled(heading, palette.page_heading())),
                Rect { height: 1, ..area },
            );
            // Heading, the tab strip if there is one, a blank line, the
            // filter box's three rows, a blank line, then the cards.
            if !tabs.0.is_empty() {
                super::carousel::render_tabs(
                    frame,
                    Rect { y: area.y + 1, height: 1, ..area },
                    palette,
                    tabs.0,
                    tabs.1,
                );
            }
            let filter_y = area.y + 2 + tab_rows(tabs.0);
            // No check that it fits: a `Rect` past the end of the buffer is
            // clipped to nothing, so a pane too short for the box already
            // draws none of it. The body below is the one that has to ask,
            // because it works out its own height by subtraction.
            render_filter(
                frame,
                Rect { y: filter_y, height: super::inputbox::HEIGHT, ..area },
                palette,
                filter_hint,
                state,
                filtering,
            );
            filter_y + super::inputbox::HEIGHT + 1
        }
    };
    debug_assert_eq!(
        body_y - area.y,
        header_rows_with(chrome, tabs.0),
        "the shared header count has drifted from what is drawn"
    );
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

    // In rows of cards, which is what `offset` counts.
    let total_rows = cards.len().div_ceil(cols.max(1));
    // Beside the cards, not the whole pane: the heading and filter box do
    // not scroll, so a bar spanning them measures the wrong thing.
    super::scrollbar::render(
        frame,
        Rect {
            x: full.x,
            y: body.y,
            width: full.width.saturating_sub(super::scrollbar::WIDTH),
            height: body.height,
        },
        palette,
        total_rows,
        state.offset,
        visible_rows,
    );

    // Page links have no artwork at all -- Explore's genres and the like --
    // so they are drawn as the pills their own row uses rather than as a
    // grid of empty grey squares.
    if super::carousel::are_links(cards) {
        super::carousel::render_pills_wrapped(
            frame,
            body,
            palette,
            cards,
            focused.then_some(state.selected),
        );
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
        let x = body.x + col as u16 * (card_width() + GAP);
        let y = body.y + row as u16 * step_y;
        // A card that does not fit across is not drawn: clipped to what was
        // left it painted a sliver of cover under a truncated title, which
        // reads as a fault rather than as a grid that continues. `columns`
        // already says how many fit, so this only catches the rounding.
        if x + card_width() > body.x + body.width || y >= body.y + body.height {
            continue;
        }
        // Height clips: the bottom row shows as much of itself as fits,
        // cut off by the pane's edge. Anything past it is not drawn at all,
        // which the `y >=` guard above catches.
        let card_area = Rect {
            x,
            y,
            width: card_width(),
            height: card_height(lines).min(body.y + body.height - y),
        };
        render_card(
            frame,
            card_area,
            palette,
            card,
            focused && i == state.selected,
            card.target.as_ref().is_some_and(liked),
            &mut draw_cover,
        );
    }
}

pub(super) fn render_filter(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    hint: &str,
    state: &GridState,
    filtering: bool,
) {
    super::inputbox::render(frame, area, palette, hint, &state.filter, filtering);
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_sliver_at_the_fold_is_not_a_row() {
        // `min_partial_row(lines)` is the line between "the grid carries on" and
        // a stripe of cover that reads as a fault. Both sides of it, and
        // the step between them, since nothing exercised either.
        let lines = 2;
        let step = card_height(lines) + ROW_GAP;

        // Exactly two whole rows and nothing over.
        let whole = step * 2 - ROW_GAP;
        assert_eq!(rows(whole, lines), 2, "two rows and no remainder");

        // One row past them is below the floor: not drawn.
        assert_eq!(
            rows(whole + ROW_GAP + min_partial_row(lines) - 1, lines),
            2,
            "a sliver is not a row"
        );

        // And at the floor it counts.
        assert_eq!(
            rows(whole + ROW_GAP + min_partial_row(lines), lines),
            3,
            "at the floor the cut row is drawn"
        );
    }

    #[test]
    fn a_pane_too_short_for_anything_shows_nothing() {
        let lines = 2;
        assert_eq!(rows(0, lines), 0, "no pane, no rows");
        assert_eq!(
            rows(min_partial_row(lines) - 1, lines),
            0,
            "less than the floor is not a row either"
        );
        assert_eq!(
            rows(min_partial_row(lines), lines),
            1,
            "and the floor itself is one"
        );
    }
    use super::*;

    fn cards(n: usize) -> Vec<Card> {
        (0..n).map(|i| Card::new(format!("Item {i}"), "Coco")).collect()
    }

    #[test]
    fn a_selected_cards_shade_is_even_on_both_sides() {
        // A column either side of the card, taken from the three-column
        // gutter between them. Uneven, the card looks nudged out of its own
        // highlight.
        let palette = crate::shell::theme::Palette::detect();
        let cards: Vec<Card> = (0..3)
            .map(|i| Card::new(format!("Card {i}"), "TIDAL"))
            .collect();
        let refs: Vec<&Card> = cards.iter().collect();
        let state = GridState { selected: 1, ..Default::default() };
        let buf = crate::shell::geometry::draw(80, 24, move |f, area, palette| {
            render(
                f,
                area,
                palette,
                Grid {
                    filtering: false,
                    heading: "Albums",
                    filter_hint: "Filter",
                    cards: &refs,
                    state: &state,
                    focused: true,
                    lines: 2,
                    chrome: Chrome::Full,
                                    tabs: (&[], 0),
                                    liked: &crate::shell::carousel::nobody,
                },
                |_, _, _, _| false,
            )
        });

        // On the title's row, where only the shade paints a background —
        // a cover row would also carry the neighbours' placeholders.
        let title = crate::shell::geometry::find(&buf, "Card 1").expect("the card");
        let shaded: Vec<u16> = (0..80)
            .filter(|x| buf[(*x, title.row)].bg == palette.selection)
            .collect();

        let first = *shaded.first().expect("the shade");
        let last = *shaded.last().expect("the shade");
        assert_eq!(
            title.start - first,
            1,
            "a column to the left of the card, found {}",
            title.start - first
        );
        assert_eq!(
            last - (title.start + card_width() - 1),
            1,
            "and one to its right, found {}",
            last - (title.start + card_width() - 1)
        );
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
    fn geometry_counts_columns_beside_the_scrollbar() {
        // The renderer holds a column back for the scrollbar before it
        // counts cards, so the keys have to count the same way. At 92 wide,
        // 95 / 19 says five columns fit; beside the bar only four are drawn.
        // Counting five moved the selection diagonally down a wide screen.
        let (cols, _) = geometry(92, 24, 1, Chrome::Full);
        let beside_bar = super::super::scrollbar::reserve(Rect::new(0, 0, 92, 24)).width;
        assert_eq!(cols, columns(beside_bar));
        assert_eq!(cols, 4);
    }

    #[test]
    fn rows_account_for_the_row_gap() {
        // A 3-line card is 8 + 3 = 11 tall, plus 1 blank between rows.
        assert_eq!(rows(11, 3), 1, "one whole card");
        assert_eq!(rows(23, 3), 2, "two, with the gap between them");
    }

    #[test]
    fn a_row_at_the_fold_shows_as_much_of_itself_as_fits() {
        // Dropping it left a band of empty pane under the grid. The web
        // client cuts the row off at the edge instead, which is what makes
        // it obvious the grid continues — selecting it scrolls it in whole.
        assert_eq!(rows(2, 3), 1, "two rows of cover is a row beginning");
        assert_eq!(rows(1, 3), 0, "one is a line, not a picture");
        assert_eq!(rows(16, 3), 2, "a whole row and the top of the next");
        assert_eq!(rows(23, 3), 2, "two whole rows, nothing left over");
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
        // 6 columns, 3 visible rows — the last of which is the one cut off
        // at the fold, so two are drawn whole.
        for _ in 0..3 {
            s.next_row(60, 6, 3);
        }
        assert_eq!(s.selected, 18, "row 3");
        assert_eq!(s.offset, 2, "pulled down so the selected row is whole");

        for _ in 0..3 {
            s.previous_row(6, 3);
        }
        assert_eq!(s.selected, 0);
        assert_eq!(s.offset, 0, "the window follows back to the top");
    }

    #[test]
    fn selecting_the_row_at_the_fold_scrolls_it_into_view_whole() {
        // The bottom row is drawn cut off. Landing on it has to pull the
        // window, or the selected card is the one card you cannot see.
        let mut s = GridState::default();
        // Three rows counted, so two are whole and the third is the sliver.
        s.next_row(60, 6, 3); // row 1 — inside the whole part
        assert_eq!(s.offset, 0, "no need to scroll yet");

        s.next_row(60, 6, 3); // row 2 — the sliver
        assert_eq!(
            s.offset, 1,
            "the window moved so the selected row is drawn whole"
        );
        let row = s.selected / 6;
        assert!(
            row < s.offset + 2,
            "row {row} is inside the two whole rows at offset {}",
            s.offset
        );
    }

    #[test]
    fn a_grid_with_tabs_stacks_its_header_in_order() {
        // Every other test here draws a grid with no tab strip, so the row
        // the strip sits on was never checked -- it could be drawn over the
        // heading or down into the filter box and nothing would say so. The
        // order is the web client's: heading, tabs, blank, the box, blank,
        // then the cards.
        let all = cards(6);
        let refs: Vec<&Card> = all.iter().collect();
        let state = GridState::default();
        let buf = crate::shell::geometry::draw(60, 20, |f, area, palette| {
            render(
                f,
                area,
                palette,
                Grid {
                    filtering: false,
                    chrome: Chrome::Full,
                    heading: "Playlists",
                    filter_hint: "Filtrer",
                    cards: &refs,
                    state: &state,
                    focused: true,
                    lines: 3,
                    tabs: (&["Alpha", "Beta"], 0),
                    liked: &crate::shell::carousel::nobody,
                },
                |_, _, _, _| false,
            )
        });
        let row = |y: u16| crate::shell::geometry::row(&buf, y).trim_end().to_string();

        assert!(row(0).contains("Playlists"), "the heading is the first row");
        assert!(
            row(1).contains("Alpha") && row(1).contains("Beta"),
            "the tabs sit directly under it, on row 1, not {:?}",
            row(1)
        );
        assert_eq!(row(2), "", "a blank line before the box");
        assert!(row(3).starts_with('\u{256d}'), "the box opens on row 3");
        assert!(row(4).contains("Filtrer"), "its text on row 4");
        assert!(row(5).starts_with('\u{2570}'), "and it closes on row 5");
        assert_eq!(row(6), "", "a blank line before the cards");
    }

    #[test]
    fn going_up_from_the_top_row_stays_on_it() {
        // `previous_row` subtracts a whole row. From the first row there is
        // no row to subtract, and the selection has to hold at the top
        // rather than wrap to the end of the list.
        let mut s = GridState::default();
        s.previous_row(6, 3);
        assert_eq!(s.selected, 0, "already at the top");
        assert_eq!(s.offset, 0);

        // From part-way along the first row, the same: up leaves the grid,
        // it does not walk backwards through it.
        let mut s = GridState { selected: 3, ..Default::default() };
        s.previous_row(6, 3);
        assert_eq!(s.selected, 0, "the row above the first is the first");
    }

    #[test]
    fn scrolling_back_up_pulls_the_window_with_the_selection() {
        // Coming back up to a row above the window has to move the window
        // too, or the selected card sits off the top of the pane. The test
        // that walks back to the very top passes whether or not this fires,
        // because the offset ends at zero either way -- this stops part-way
        // instead, where the window has to have moved but not to the top.
        let mut s = GridState::default();
        for _ in 0..4 {
            s.next_row(60, 6, 3);
        }
        assert_eq!((s.selected, s.offset), (24, 3), "row 4, window pulled down");

        s.previous_row(6, 3);
        assert_eq!((s.selected, s.offset), (18, 3), "row 3 is still drawn whole");

        s.previous_row(6, 3);
        assert_eq!(
            (s.selected, s.offset),
            (12, 2),
            "row 2 is above the window, so the window came up with it"
        );
    }

    #[test]
    fn a_selection_level_with_the_length_is_still_out_of_bounds() {
        // `clamp` takes a length, not a last index. A selection equal to it
        // is one past the end -- the boundary a filter lands on when it
        // trims the list to exactly the cards above the cursor.
        let mut s = GridState { selected: 10, ..Default::default() };
        s.clamp(10);
        assert_eq!(s.selected, 9, "ten cards, so the last is nine");

        let mut s = GridState { selected: 9, ..Default::default() };
        s.clamp(10);
        assert_eq!(s.selected, 9, "already inside, so left alone");
    }

    #[test]
    fn a_pane_with_no_room_counted_still_tracks_the_selection() {
        // `rows` returns zero for a pane too short to draw anything, and
        // the keyboard still works while the window is that small. One less
        // than no rows must hold at zero rather than wrap into a count so
        // large that the window never follows the selection again -- which
        // is what you would see on the next resize: a grid scrolled to the
        // top with the cursor somewhere far below it.
        let mut s = GridState::default();
        s.next_row(60, 6, 0);
        assert_eq!(s.selected, 6, "moved a row");
        assert_eq!(s.offset, 1, "and the window followed");
    }

    #[test]
    fn a_pane_with_only_a_sliver_still_scrolls() {
        // One row counted is one row cut off at the fold, so there is no
        // whole row to hold the selection. It still has to scroll rather
        // than divide by nothing.
        let mut s = GridState::default();
        s.next_row(60, 6, 1);
        assert_eq!(s.selected, 6, "moved a row");
        assert_eq!(s.offset, 1, "and the window came with it");
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
                        filtering: false,
                        chrome: Chrome::Full,
                        heading: "Playlists",
                        filter_hint: "Filtrer playlists",
                        cards: &refs,
                        state: &state,
                        focused: true,
                        lines: 3,
                                            tabs: (&[], 0),
                                            liked: &crate::shell::carousel::nobody,
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
        // Two cards wide plus the column the scrollbar holds back.
        let mut term = Terminal::new(TestBackend::new(36, 30)).unwrap();
        term.draw(|f| {
            render(
                f,
                f.area(),
                &palette,
                Grid {
                    filtering: false,
                    chrome: Chrome::Full,
                    heading: "Albums",
                    filter_hint: "Filtrer Albums",
                    cards: &refs,
                    state: &state,
                    focused: true,
                    lines: 2,
                                    tabs: (&[], 0),
                                    liked: &crate::shell::carousel::nobody,
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
