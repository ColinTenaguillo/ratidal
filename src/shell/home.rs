//! The main pane: tabs, the shortcut grid, and the carousel rows.
//!
//! This mirrors the web client's "Music" page — the tab strip across the top,
//! a 3×2 grid of wide shortcut cards, then titled rows of covers.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use super::carousel::{self, Card, CarouselState, CARD_HEIGHT};
use super::theme::Palette;
use super::trackgrid;

/// The web client shows a third, Uploads, whose rows come from a service
/// this API does not expose — every plausible `/pages/*` id for it is a 404,
/// and the endpoint the web client uses is restricted to its own client. A
/// tab that can only ever be empty is worse than no tab.
pub const TABS: [&str; 2] = ["For you", "Staff Picks"];

/// A wide shortcut card: a small cover beside two lines of text.
#[derive(Debug, Clone)]
pub struct Shortcut {
    pub title: String,
    pub subtitle: String,
    pub cover_url: Option<String>,
}

/// One titled row of the home page.
#[derive(Debug)]
pub struct Row {
    pub heading: String,
    /// Covers in a strip, or tracks in a grid — the API says which.
    pub kind: crate::browse::RowKind,
    pub cards: Vec<Card>,
    pub state: CarouselState,
    /// Where the rest of this row's items live, when there are more than
    /// the page handed back. What "See all" opens.
    pub more: Option<String>,
}

#[derive(Debug, Default)]
pub struct HomeState {
    pub tab: usize,
    pub shortcuts: Vec<Shortcut>,
    pub rows: Vec<Row>,
    /// Which row has the selection.
    pub row: usize,
    /// Rows scrolled past the top of the pane.
    pub scroll: usize,
}

impl HomeState {
    pub fn next_tab(&mut self) {
        self.tab = (self.tab + 1) % TABS.len();
    }

    /// Step down within a track grid, or on to the next row when there is
    /// no grid row left below.
    ///
    /// A TRACK_LIST is drawn as a grid, so `j` has somewhere to go inside
    /// the row. Treating every row as one line left the grid's lower half
    /// unreachable: the cards were drawn but nothing could select them.
    pub fn down(&mut self, visible: usize, columns: usize) {
        if self.step_within(true, columns) {
            return;
        }
        self.row_down(visible);
        // Coming into a grid from above lands on its top row, which is
        // where the eye already is.
        if let Some(row) = self.current_row_mut() {
            if row.kind == crate::browse::RowKind::Tracks {
                row.state.selected %= columns.max(1);
            }
        }
    }

    /// As [`down`], upwards.
    pub fn up(&mut self, visible: usize, columns: usize) {
        if self.step_within(false, columns) {
            return;
        }
        self.row_up(visible);
        // And entering a grid from below lands on its bottom row.
        let columns = columns.max(1);
        if let Some(row) = self.current_row_mut() {
            if row.kind == crate::browse::RowKind::Tracks {
                let last_row = row.cards.len().saturating_sub(1) / columns;
                let col = row.state.selected % columns;
                row.state.selected = (last_row * columns + col).min(
                    row.cards.len().saturating_sub(1),
                );
            }
        }
    }

    /// Move a row inside a track grid. Returns whether it moved — false
    /// means the selection is already at that edge of the grid, and the
    /// caller should leave the row instead.
    fn step_within(&mut self, down: bool, columns: usize) -> bool {
        let columns = columns.max(1);
        let Some(row) = self.current_row_mut() else { return false };
        if row.kind != crate::browse::RowKind::Tracks {
            return false;
        }
        let len = row.cards.len();
        if len == 0 {
            return false;
        }
        let at = row.state.selected;
        if down {
            let next = at + columns;
            if next < len {
                row.state.selected = next;
                return true;
            }
        } else if at >= columns {
            row.state.selected = at - columns;
            return true;
        }
        false
    }

    pub fn row_down(&mut self, visible: usize) {
        if !self.rows.is_empty() {
            self.row = (self.row + 1).min(self.rows.len() - 1);
        }
        self.scroll_into_view(visible);
    }

    pub fn row_up(&mut self, visible: usize) {
        self.row = self.row.saturating_sub(1);
        self.scroll_into_view(visible);
    }

    /// Pull `scroll` just far enough that the selected row is drawn.
    ///
    /// Nothing ever wrote `scroll` before: the renderer read it and the
    /// movement keys did not, so the last rows of the page could not be
    /// reached at all on a short terminal — they were simply never drawn.
    fn scroll_into_view(&mut self, visible: usize) {
        let visible = visible.max(1);
        if self.row < self.scroll {
            self.scroll = self.row;
        } else if self.row >= self.scroll + visible {
            self.scroll = self.row + 1 - visible;
        }
    }

    /// The carousel the user is currently moving through.
    pub fn current_row_mut(&mut self) -> Option<&mut Row> {
        self.rows.get_mut(self.row)
    }

    pub fn current_row(&self) -> Option<&Row> {
        self.rows.get(self.row)
    }
}

/// Height of the shortcut block: two rows of cards, each two lines tall.
const SHORTCUT_ROWS: u16 = 2;
const SHORTCUT_HEIGHT: u16 = SHORTCUT_ROWS * 3;
const SHORTCUT_COLUMNS: usize = 3;

/// A carousel row: its cards, its heading, and the blank line under it.
/// A carousel row: its heading, a blank line, the card, and a blank line
/// under it. The blank above is what lets a selected card's shade reach
/// past the top of its cover without landing on the heading.
const ROW_HEIGHT: u16 = carousel::CARD_HEIGHT + 3;

/// Rows above the carousels: the tabs and their blank line, plus the
/// shortcut block when there is one.
fn header_height(has_shortcuts: bool) -> u16 {
    let tabs = 2;
    if has_shortcuts {
        tabs + SHORTCUT_HEIGHT + 1
    } else {
        tabs
    }
}

/// How many carousel rows fit under the header.
///
/// The renderer stops when the next row will not fit; the scrolling has to
/// stop at the same place, or the selection walks off the bottom into rows
/// that are never drawn — which is what hid the last row of the page.
pub fn visible_rows(height: u16, has_shortcuts: bool) -> usize {
    // Without the rows themselves, all this can do is assume they are all
    // carousels — which is what it did, and why the count disagreed with
    // the renderer on a page with track grids in it.
    visible_rows_of(height, has_shortcuts, &[])
}

/// How many of `rows` the renderer will draw in `height`.
///
/// Counted the same way the renderer lays them out — a track grid is taller
/// than a carousel, so dividing by one row height put the selection on rows
/// that were never drawn.
pub fn visible_rows_of(height: u16, has_shortcuts: bool, rows: &[Row]) -> usize {
    let body = height.saturating_sub(header_height(has_shortcuts));
    if rows.is_empty() {
        if body < ROW_HEIGHT {
            return 0;
        }
        return ((body / ROW_HEIGHT).max(1)) as usize;
    }
    // Walked the way the renderer walks them, blank line between rows and
    // all: counting heights alone said two rows fitted where one did.
    let mut used = 0u16;
    let mut count = 0usize;
    for row in rows {
        let wanted = row_height(row.kind);
        if used + wanted > body {
            break;
        }
        used += wanted + 1;
        count += 1;
    }
    count
}

pub fn render<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    state: &HomeState,
    focused: bool,
    mut draw_cover: F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    if area.width == 0 || area.height == 0 {
        return;
    }

    // A column on the right for the scrollbar, so a row's last card does not
    // run under it. Taken whether or not the bar is drawn, or the layout
    // would shift as soon as the page grew past one screen.
    let area = super::scrollbar::reserve(area);

    let mut y = area.y;

    // Tab strip. The active tab is underlined, as on the web.
    if y < area.y + area.height {
        let mut spans: Vec<Span> = Vec::new();
        for (i, tab) in TABS.iter().enumerate() {
            let style = if i == state.tab {
                palette.title().add_modifier(Modifier::UNDERLINED)
            } else {
                palette.subtitle()
            };
            spans.push(Span::styled(*tab, style));
            spans.push(Span::raw("   "));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect { x: area.x, y, width: area.width, height: 1 },
        );
        y += 2;
    }

    // Shortcut grid.
    if !state.shortcuts.is_empty() && y + SHORTCUT_HEIGHT <= area.y + area.height {
        render_shortcuts(
            frame,
            Rect { x: area.x, y, width: area.width, height: SHORTCUT_HEIGHT },
            palette,
            &state.shortcuts,
            &mut draw_cover,
        );
        y += SHORTCUT_HEIGHT + 1;
    }

    // Each row is a heading plus its items, laid out as the module asked.
    //
    // A row that does not fit whole is not drawn: it used to be clipped to
    // whatever was left, which painted a heading over half a strip of
    // covers. The page's height decides how many rows appear, and a
    // terminal a few lines shorter than another showed a broken one rather
    // than one fewer.
    for (i, row) in state.rows.iter().enumerate().skip(state.scroll) {
        let height = row_height(row.kind);
        if y + height > area.y + area.height {
            break;
        }
        let is_focused = focused && i == state.row;

        match row.kind {
            crate::browse::RowKind::Tracks => {
                // The same heading a carousel draws, "See all" and all: the
                // key reaches these rows too, and without the hint they
                // were the one kind with no sign of it.
                carousel::render_heading(
                    frame,
                    Rect { x: area.x, y, width: area.width, height: 1 },
                    palette,
                    &row.heading,
                    is_focused,
                    true,
                );
                if height > 2 {
                    trackgrid::render(
                        frame,
                        Rect {
                            x: area.x,
                            y: y + 2,
                            width: area.width,
                            height: height - 2,
                        },
                        palette,
                        &row.cards,
                        is_focused.then_some(row.state.selected),
                        &mut draw_cover,
                    );
                }
            }
            crate::browse::RowKind::Carousel => {
                carousel::render(
                    frame,
                    Rect { x: area.x, y, width: area.width, height },
                    palette,
                    carousel::Row {
                        heading: &row.heading,
                        cards: &row.cards,
                        state: &row.state,
                        focused: is_focused,
                    },
                    &mut draw_cover,
                );
            }
        }
        y += height + 1;
    }

    // Beside the rows, below the tabs and the shortcut block: those do not
    // scroll, so a bar spanning them measures the wrong thing.
    let header = header_height(!state.shortcuts.is_empty());
    super::scrollbar::render(
        frame,
        Rect {
            x: area.x,
            y: area.y + header,
            width: area.width,
            height: area.height.saturating_sub(header),
        },
        palette,
        state.rows.len(),
        state.scroll,
        visible_rows_of(area.height, !state.shortcuts.is_empty(), &state.rows),
    );
}

/// How tall a row of each kind wants to be.
///
/// A grid of three rows of tracks is taller than a strip of covers, so a
/// single height would either crop the grid or leave a gap under every
/// carousel.
fn row_height(kind: crate::browse::RowKind) -> u16 {
    match kind {
        // A heading plus the grid itself.
        // The heading, a blank line, then the grid — the blank is where a
        // selected cell's shade reaches, so it can mark the top of the
        // thumbnail without covering the heading.
        crate::browse::RowKind::Tracks => trackgrid::height() + 2,
        crate::browse::RowKind::Carousel => CARD_HEIGHT + 3,
    }
}

fn render_shortcuts<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    shortcuts: &[Shortcut],
    draw_cover: &mut F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    let gap = 2u16;
    let columns = SHORTCUT_COLUMNS as u16;
    // Integer division leaves a remainder; the last column absorbs it rather
    // than leaving a ragged edge.
    let cell_width = area.width.saturating_sub(gap * (columns - 1)) / columns;
    if cell_width < 6 {
        return;
    }

    for (i, shortcut) in shortcuts.iter().take(SHORTCUT_COLUMNS * 2).enumerate() {
        let col = (i % SHORTCUT_COLUMNS) as u16;
        let row = (i / SHORTCUT_COLUMNS) as u16;
        let y = area.y + row * 3;
        if y + 1 > area.y + area.height {
            break;
        }

        let cell = Rect {
            x: area.x + col * (cell_width + gap),
            y,
            width: cell_width,
            height: 2.min(area.y + area.height - y),
        };

        // The whole cell gets a panel, as on the web: without it the entries
        // read as loose text rather than cards.
        frame.render_widget(
            Block::default().style(Style::default().bg(palette.surface)),
            cell,
        );

        // A small square cover on the left, text filling the rest.
        let cover_width = 4u16.min(cell.width);
        let cover = Rect { width: cover_width, ..cell };
        let drew = match &shortcut.cover_url {
            Some(url) => draw_cover(frame, cover, url, super::artwork::Shape::Square),
            None => false,
        };
        if !drew {
            frame.render_widget(
                Block::default().style(Style::default().bg(palette.border)),
                cover,
            );
        }

        let text_x = cell.x + cover_width + 1;
        if text_x >= cell.x + cell.width {
            continue;
        }
        let text_width = cell.x + cell.width - text_x;
        frame.render_widget(
            Paragraph::new(Line::styled(
                clip(&shortcut.title, text_width),
                palette.title(),
            )),
            Rect { x: text_x, y: cell.y, width: text_width, height: 1 },
        );
        if cell.height > 1 {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    clip(&shortcut.subtitle, text_width),
                    palette.subtitle(),
                )),
                Rect { x: text_x, y: cell.y + 1, width: text_width, height: 1 },
            );
        }
    }
}

fn clip(s: &str, width: u16) -> String {
    let width = width as usize;
    if width == 0 {
        return String::new();
    }
    if s.chars().count() <= width {
        return s.to_string();
    }
    if width == 1 {
        return "…".into();
    }
    let mut out: String = s.chars().take(width - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home_with_rows(n: usize) -> HomeState {
        HomeState {
            rows: (0..n)
                .map(|i| Row {
                    kind: crate::browse::RowKind::Carousel,
                    heading: format!("Row {i}"),
                    cards: vec![carousel::Card::new("Card", "Artist")],
                    state: carousel::CarouselState::default(),
                    more: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    /// A page whose middle row is a track grid of `n` cards.
    fn home_with_grid(n: usize) -> HomeState {
        HomeState {
            rows: vec![
                Row {
                    kind: crate::browse::RowKind::Carousel,
                    heading: "Above".into(),
                    cards: vec![carousel::Card::new("Card", "Artist")],
                    state: carousel::CarouselState::default(),
                    more: None,
                },
                Row {
                    kind: crate::browse::RowKind::Tracks,
                    heading: "New Tracks".into(),
                    cards: (0..n)
                        .map(|i| carousel::Card::new(format!("Track {i}"), "Artist"))
                        .collect(),
                    state: carousel::CarouselState::default(),
                    more: None,
                },
                Row {
                    kind: crate::browse::RowKind::Carousel,
                    heading: "Below".into(),
                    cards: vec![carousel::Card::new("Card", "Artist")],
                    state: carousel::CarouselState::default(),
                    more: None,
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn every_row_offers_to_show_the_rest_of_itself() {
        // The track grids drew their own heading and so had no "See all",
        // though o opens them the same as any other row.
        let mut home = home_with_rows(2);
        home.rows[0].kind = crate::browse::RowKind::Tracks;
        home.rows[0].cards = vec![carousel::Card::new("Track", "Artist")];
        let buf = crate::shell::geometry::draw(80, 40, move |f, area, p| {
            render(f, area, p, &home, false, |_, _, _, _| false)
        });
        let text = crate::shell::geometry::text(&buf);

        assert_eq!(
            text.matches("See all").count(),
            2,
            "both rows offer it, the grid included:\n{text}"
        );
    }

    #[test]
    fn a_selected_cards_shade_is_even_on_all_four_sides() {
        // A row of margin above and below, a column either side. It reached
        // up but not down, so the band sat heavier at the bottom.
        let palette = crate::shell::theme::Palette::detect();
        let mut home = home_with_rows(2);
        home.row = 1;
        home.rows[1].cards = vec![carousel::Card::new("Card A", "Artist")];
        let buf = crate::shell::geometry::draw(60, 40, move |f, area, p| {
            render(f, area, p, &home, true, |_, _, _, _| false)
        });

        // Only the selected row's band: a cover placeholder is the same
        // colour, so the unselected row above would be counted too.
        let title = crate::shell::geometry::find(&buf, "Card A").expect("the card");
        let shaded: Vec<(u16, u16)> = (title.row.saturating_sub(14)..(title.row + 6))
            .flat_map(|y| (0..60).map(move |x| (x, y)))
            .filter(|(x, y)| buf[(*x, *y)].bg == palette.selection)
            .collect();
        assert!(!shaded.is_empty(), "the card is shaded");

        let top = shaded.iter().map(|(_, y)| *y).min().unwrap();
        let bottom = shaded.iter().map(|(_, y)| *y).max().unwrap();
        let left = shaded.iter().map(|(x, _)| *x).min().unwrap();
        let right = shaded.iter().map(|(x, _)| *x).max().unwrap();

        // The card itself: its cover is CARD_HEIGHT tall and card_width()
        // wide, and the band is a row and a column past it either way.
        // No margin above or below: a terminal cell is about 19x30 pixels,
        // so a row of margin is far thicker than the column beside it, and
        // the grid has no half rows to split the difference with.
        assert_eq!(
            bottom - top + 1,
            carousel::CARD_HEIGHT,
            "the band is the card's own height"
        );
        assert_eq!(
            right - left + 1,
            carousel::card_width() + 2,
            "and a column either side, which is the closer match to none \
             above than a whole row would be"
        );
    }

    #[test]
    fn a_selected_cards_shade_clears_its_rows_heading() {
        // The shade reaches a row above the cover so the card is marked all
        // round; the row leaves a blank line under its heading for exactly
        // that, and without it the shade landed on the heading.
        let palette = crate::shell::theme::Palette::detect();
        let mut home = home_with_rows(2);
        home.row = 1;
        home.rows[1].cards = vec![carousel::Card::new("Card A", "Artist")];
        let buf = crate::shell::geometry::draw(40, 32, move |f, area, p| {
            render(f, area, p, &home, true, |_, _, _, _| false)
        });

        let heading = crate::shell::geometry::find(&buf, "Row 1").expect("the heading");
        let title = crate::shell::geometry::find(&buf, "Card A").expect("the card");
        assert_ne!(
            buf[(heading.start, heading.row)].bg,
            palette.selection,
            "the heading is left alone\n{}",
            crate::shell::geometry::text(&buf)
        );
        assert_ne!(
            buf[(title.start, heading.row + 1)].bg,
            palette.selection,
            "the blank line under the heading stays clear too"
        );
        assert_eq!(
            buf[(title.start, heading.row + 2)].bg,
            palette.selection,
            "and the shade starts with the card itself"
        );
    }

    #[test]
    fn a_row_that_does_not_fit_whole_is_not_drawn() {
        // It used to be clipped to whatever was left, so a heading appeared
        // over half a strip of covers — and a terminal a few lines shorter
        // than another showed a broken row rather than one fewer. It is why
        // this looked right in Alacritty and wrong in WezTerm.
        let home = home_with_rows(6);
        let full = row_height(crate::browse::RowKind::Carousel);

        // A height with room for one whole row and most of a second.
        let height = header_height(false) + full + full - 2;
        let buf = crate::shell::geometry::draw(60, height, move |f, area, p| {
            render(f, area, p, &home, false, |_, _, _, _| false)
        });
        let text = crate::shell::geometry::text(&buf);

        assert!(text.contains("Row 0"), "the row that fits is drawn:\n{text}");
        assert!(
            !text.contains("Row 1"),
            "and the one that does not is left out entirely:\n{text}"
        );
    }

    #[test]
    fn the_count_matches_what_is_drawn_for_rows_of_different_heights() {
        // A track grid is taller than a carousel, so dividing the pane by
        // one row height put the selection on rows that were never drawn.
        let mut home = home_with_rows(4);
        home.rows[1].kind = crate::browse::RowKind::Tracks;

        for height in 12..50u16 {
            let n = visible_rows_of(height, false, &home.rows);
            let mut state = home_with_rows(4);
            state.rows[1].kind = crate::browse::RowKind::Tracks;
            let buf = crate::shell::geometry::draw(80, height, move |f, area, p| {
                render(f, area, p, &state, false, |_, _, _, _| false)
            });
            let text = crate::shell::geometry::text(&buf);
            let drawn = (0..4)
                .filter(|i| text.contains(&format!("Row {i}")))
                .count();
            assert_eq!(
                drawn, n,
                "at height {height}: drew {drawn}, the count said {n}\n{text}"
            );
        }
    }

    #[test]
    fn down_steps_through_a_track_grid_before_leaving_it() {
        // The grid is drawn three across and two down, but every row was
        // treated as one line — so the lower half was drawn and could not be
        // selected.
        let mut home = home_with_grid(6);
        home.row = 1; // the grid
        assert_eq!(home.current_row().unwrap().state.selected, 0);

        home.down(10, 3);
        assert_eq!(home.row, 1, "still in the grid");
        assert_eq!(
            home.current_row().unwrap().state.selected,
            3,
            "and a row further down it"
        );

        home.down(10, 3);
        assert_eq!(home.row, 2, "off the bottom of the grid, on to the next row");
    }

    #[test]
    fn up_steps_back_through_the_grid() {
        let mut home = home_with_grid(6);
        home.row = 1;
        home.current_row_mut().unwrap().state.selected = 4; // second row

        home.up(10, 3);
        assert_eq!(home.row, 1, "still in the grid");
        assert_eq!(home.current_row().unwrap().state.selected, 1, "top row");

        home.up(10, 3);
        assert_eq!(home.row, 0, "and then out of it");
    }

    #[test]
    fn entering_a_grid_lands_on_the_edge_it_was_entered_from() {
        // Coming down into a grid onto its bottom row would skip the top
        // one entirely.
        let mut home = home_with_grid(6);
        home.row = 0;
        home.current_row_mut().unwrap().state.selected = 0;
        home.down(10, 3);
        assert_eq!(home.row, 1);
        assert!(
            home.current_row().unwrap().state.selected < 3,
            "entered from above, so on the top row"
        );

        let mut home = home_with_grid(6);
        home.row = 2;
        home.up(10, 3);
        assert_eq!(home.row, 1);
        assert!(
            home.current_row().unwrap().state.selected >= 3,
            "entered from below, so on the bottom row"
        );
    }

    #[test]
    fn entering_a_partly_filled_grid_from_below_lands_on_a_card() {
        // Coming up into a grid puts the cursor on its bottom row, at the
        // column it was already in. With four cards in a three-wide grid
        // that row holds one card, so two of the three columns are not
        // there — and the clamp is what keeps the selection off them.
        let mut home = home_with_grid(4);
        // The column is carried from the grid itself: start inside it, on
        // the third card of the top row, then step down and back up.
        home.row = 1;
        home.current_row_mut().unwrap().state.selected = 2;
        home.down(10, 3); // out of the grid, since its second row is short
        home.up(10, 3); // and back into it

        assert_eq!(home.row, 1, "into the grid");
        let selected = home.current_row().unwrap().state.selected;
        assert!(
            selected < 4,
            "selected {selected} of four cards — the bottom row of a \
             three-wide grid holds only the fourth"
        );
    }

    #[test]
    fn a_partly_filled_grid_does_not_select_past_its_cards() {
        // Four cards in a three-wide grid is a full row and a stub; stepping
        // down from the third card must not land on a cell that is not there.
        let mut home = home_with_grid(4);
        home.row = 1;
        home.current_row_mut().unwrap().state.selected = 2;

        home.down(10, 3);
        let selected = home.current_row().unwrap().state.selected;
        assert!(
            home.row != 1 || selected < 4,
            "selected {selected} of 4 cards"
        );
    }

    #[test]
    fn a_carousel_row_still_moves_a_whole_row_at_a_time() {
        // The grid rules must not leak into the rows that are carousels.
        let mut home = home_with_rows(3);
        home.down(10, 3);
        assert_eq!(home.row, 1, "one row, not one card");
    }

    #[test]
    fn the_last_row_can_be_reached_on_a_short_pane() {
        // `scroll` was read by the renderer and written by nothing, so rows
        // past the fold were never drawn and never reachable — the bottom
        // row of the page was invisible on any terminal too short for it.
        let mut home = home_with_rows(5);
        let visible = visible_rows(30, false);
        assert!(visible < 5, "the pane is too short for every row: {visible}");

        for _ in 0..4 {
            home.row_down(visible);
        }
        assert_eq!(home.row, 4, "the selection reached the last row");
        assert!(
            home.row < home.scroll + visible,
            "row {} is past the last drawn row (scroll {}, {visible} visible)",
            home.row,
            home.scroll,
        );
    }

    #[test]
    fn scrolling_back_up_brings_the_first_row_with_it() {
        let mut home = home_with_rows(6);
        let visible = visible_rows(30, false);
        for _ in 0..5 {
            home.row_down(visible);
        }
        assert!(home.scroll > 0, "the page scrolled");

        for _ in 0..5 {
            home.row_up(visible);
        }
        assert_eq!(home.row, 0);
        assert_eq!(home.scroll, 0, "and the page came back to the top");
    }

    #[test]
    fn a_page_that_fits_never_scrolls() {
        let mut home = home_with_rows(2);
        let visible = visible_rows(60, false);
        assert!(visible >= 2);
        for _ in 0..2 {
            home.row_down(visible);
        }
        assert_eq!(home.scroll, 0, "nothing to scroll past");
    }
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn sample() -> HomeState {
        HomeState {
            tab: 0,
            shortcuts: (0..6)
                .map(|i| Shortcut {
                    title: format!("Shortcut {i}"),
                    subtitle: "Created by me".into(),
                    cover_url: None,
                })
                .collect(),
            rows: vec![
                Row {
                    kind: crate::browse::RowKind::Carousel,
                    heading: "New albums for you".into(),
                    cards: (0..8)
                        .map(|i| Card::new(format!("Album {i}"), "Artist"))
                        .collect(),
                    state: CarouselState::default(),
                    more: None,
                },
                Row {
                    kind: crate::browse::RowKind::Carousel,
                    heading: "Mixes made for you".into(),
                    cards: (0..6)
                        .map(|i| Card::new(format!("My Mix {i}"), "Various"))
                        .collect(),
                    state: CarouselState::default(),
                    more: None,
                },
            ],
            row: 0,
            scroll: 0,
        }
    }

    fn flatten(t: &Terminal<TestBackend>) -> String {
        let b = t.backend().buffer().clone();
        (0..b.area.height)
            .map(|y| {
                (0..b.area.width)
                    .map(|x| b[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_page_shows_tabs_shortcuts_and_row_headings() {
        let palette = Palette::detect();
        let state = sample();
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &palette, &state, true, |_, _, _, _| false))
            .unwrap();

        let text = flatten(&terminal);
        assert!(text.contains("For you"), "tab strip missing:\n{text}");
        assert!(text.contains("Shortcut 0"), "shortcut grid missing:\n{text}");
        assert!(text.contains("New albums for you"), "first row heading missing:\n{text}");
        assert!(text.contains("Album 0"), "carousel cards missing:\n{text}");
    }

    #[test]
    fn tabs_cycle() {
        let mut s = HomeState::default();
        assert_eq!(s.tab, 0);
        s.next_tab();
        assert_eq!(s.tab, 1);
        s.next_tab();
        assert_eq!(s.tab, 0, "cycles back round");
    }

    #[test]
    fn row_selection_clamps() {
        let mut s = sample();
        // A pane tall enough for both rows, so this tests the clamp and not
        // the scrolling.
        let visible = 4;
        for _ in 0..10 {
            s.row_down(visible);
        }
        assert_eq!(s.row, 1, "two rows, so index stops at 1");
        for _ in 0..10 {
            s.row_up(visible);
        }
        assert_eq!(s.row, 0);
    }

    #[test]
    fn a_short_pane_drops_what_does_not_fit_rather_than_panicking() {
        let palette = Palette::detect();
        let state = sample();
        let mut terminal = Terminal::new(TestBackend::new(60, 6)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &palette, &state, true, |_, _, _, _| false))
            .unwrap();
    }

    #[test]
    fn a_one_cell_pane_does_not_panic() {
        let palette = Palette::detect();
        let state = sample();
        let mut terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &palette, &state, false, |_, _, _, _| false))
            .unwrap();
    }
}
