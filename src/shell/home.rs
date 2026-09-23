//! The main pane: tabs and the carousel rows.
//!
//! This mirrors the web client's "Music" page — the tab strip across the top,
//! titled rows of covers, the first of them the web's shortcuts.

use ratatui::layout::Rect;
use ratatui::Frame;

use super::carousel::{self, Card, CarouselState, CARD_HEIGHT};
use super::theme::Palette;
use super::trackgrid;

/// The tab strip's labels, read from the tabs themselves so the strip and
/// the pages behind it cannot drift apart.
pub fn tab_labels() -> Vec<&'static str> {
    crate::browse::Tab::ALL.iter().map(|t| t.label()).collect()
}

pub fn tab_count() -> usize {
    crate::browse::Tab::ALL.len()
}

/// One titled row of the home page.
#[derive(Debug)]
pub struct Row {
    pub heading: String,
    /// Covers in a strip, or a grid of wide cells — the API says which.
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
    /// Whether the tab strip is drawn over this page. The home page has
    /// one; Explore and the genre pages it opens are the same shape
    /// without, and the strip appeared over them naming pages they have
    /// nothing to do with.
    pub has_tabs: bool,
    /// What this page is called, when it is one opened from Explore rather
    /// than a section of its own. Explore's own list has none: it is named
    /// by the nav entry that is highlighted.
    pub heading: Option<String>,
    pub rows: Vec<Row>,
    /// Which row has the selection.
    pub row: usize,
    /// Rows scrolled past the top of the pane.
    pub scroll: usize,
    /// What the filter box holds. Only the Feed uses it -- the home page
    /// and Explore have no filter box -- and it lives here so the rows can
    /// be rebuilt from it when it changes.
    pub filter: String,
}

impl HomeState {
    pub fn next_tab(&mut self) {
        self.tab = (self.tab + 1) % tab_count();
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
            if row.kind.grid_rows().is_some() {
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
        // And entering a grid from below lands on its bottom row — the
        // bottom row of what is *drawn*. Measured against everything the
        // row holds, this put the selection on a card the grid never draws:
        // the page said it was on the row and nothing was highlighted.
        let columns = columns.max(1);
        if let Some(row) = self.current_row_mut() {
            if let Some(deep) = row.kind.grid_rows() {
                let last = trackgrid::drawn(row.cards.len(), columns, deep).saturating_sub(1);
                let last_row = last / columns;
                let col = row.state.selected % columns;
                row.state.selected = (last_row * columns + col).min(last);
            }
        }
    }

    /// Move a row inside a track grid. Returns whether it moved — false
    /// means the selection is already at that edge of the grid, and the
    /// caller should leave the row instead.
    fn step_within(&mut self, down: bool, columns: usize) -> bool {
        let columns = columns.max(1);
        let Some(row) = self.current_row_mut() else { return false };
        let Some(deep) = row.kind.grid_rows() else { return false };
        // What the grid draws, not what the row holds: it takes the first
        // `columns * rows` cards, so stepping past those moved the
        // selection onto cards that were never on screen — and the page
        // looked stuck instead of moving to the next row.
        let len = trackgrid::drawn(row.cards.len(), columns, deep);
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

/// A carousel row: its heading, a blank line, the card, and a blank line
/// under it. The blank above is what lets a selected card's shade reach
/// past the top of its cover without landing on the heading.
const ROW_HEIGHT: u16 = carousel::CARD_HEIGHT + 3;

/// Rows above the carousels: the tabs and their blank line.
fn header_height() -> u16 {
    header_height_with(true)
}

/// As [`header_height`], for a page drawn without the tab strip.
fn header_height_with(has_tabs: bool) -> u16 {
    if has_tabs {
        2
    } else {
        0
    }
}

/// How many carousel rows fit under the header.
///
/// The renderer stops when the next row will not fit; the scrolling has to
/// stop at the same place, or the selection walks off the bottom into rows
/// that are never drawn — which is what hid the last row of the page.
pub fn visible_rows(height: u16) -> usize {
    // Without the rows themselves, all this can do is assume they are all
    // carousels — which is what it did, and why the count disagreed with
    // the renderer on a page with track grids in it.
    visible_rows_of(height, &[])
}

/// How many of `rows` the renderer will draw in `height`.
///
/// Counted the same way the renderer lays them out — a track grid is taller
/// than a carousel, so dividing by one row height put the selection on rows
/// that were never drawn.
pub fn visible_rows_of(height: u16, rows: &[Row]) -> usize {
    let body = height.saturating_sub(header_height());
    if rows.is_empty() {
        if body < ROW_HEIGHT {
            return 0;
        }
        return ((body / ROW_HEIGHT).max(1)) as usize;
    }
    // Walked the way the renderer walks them, blank line between rows and
    // all: counting heights alone said two rows fitted where one did.
    //
    // The last row may be cut off by the pane's edge and still counts —
    // it is drawn, so the keys must be able to reach it.
    visible_and_cut(body, rows).0
}

/// How many rows fit in `body`, and whether the last of them is cut off by
/// the pane's edge.
///
/// The two are worked out together because they are the same walk. The
/// second is what stops the selection landing on a half-drawn row: a whole
/// last row needs no scrolling, a cut one does.
fn visible_and_cut(body: u16, rows: &[Row]) -> (usize, bool) {
    let mut used = 0u16;
    let mut count = 0usize;
    let mut cut = false;
    for row in rows {
        if used >= body {
            break;
        }
        let wanted = row_height(row.kind);
        let drawn = wanted.min(body - used);
        cut = drawn < wanted;
        used += wanted + 1;
        count += 1;
    }
    (count, cut)
}

/// Whether the last visible row is cut off, so the keys know to scroll
/// rather than leave the selection half drawn.
pub fn last_row_is_cut(height: u16, rows: &[Row]) -> bool {
    visible_and_cut(height.saturating_sub(header_height()), rows).1
}

pub fn render<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    state: &HomeState,
    focused: bool,
    marks: super::trackgrid::Marks<'_>,
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

    // A page opened from Explore is headed by its own name -- the genre
    // the user pressed enter on. Without it the rows arrived under
    // whatever the nav still highlighted, and a genre page read as though
    // Explore itself had changed.
    if let Some(heading) = state.heading.as_deref() {
        frame.render_widget(
            ratatui::widgets::Paragraph::new(ratatui::text::Line::styled(
                heading.to_string(),
                palette.title(),
            )),
            Rect { x: area.x, y, width: area.width, height: 1 },
        );
        y += 2;
    }

    // Tab strip. The active tab is underlined, as on the web.
    if state.has_tabs && y < area.y + area.height {
        carousel::render_tabs(
            frame,
            Rect { x: area.x, y, width: area.width, height: 1 },
            palette,
            &tab_labels(),
            state.tab,
        );
        y += 2;
    }

    // Each row is a heading plus its items, laid out as the module asked.
    //
    // A row at the bottom of the page shows as much of itself as fits and
    // is cut off by the pane's edge, the way the web client leaves one
    // half-scrolled. Dropping it instead left a band of empty pane, and it
    // is the cut that tells the reader the page carries on. Whatever fits
    // is drawn, down to the heading alone: every view cuts at the edge the
    // same way, and a floor here left a band of pane the others did not.
    let bottom = area.y + area.height;
    for (i, row) in state.rows.iter().enumerate().skip(state.scroll) {
        let full = row_height(row.kind);
        if y >= bottom {
            break;
        }
        let height = full.min(bottom - y);
        let is_focused = focused && i == state.row;

        match row.kind {
            crate::browse::RowKind::Compact | crate::browse::RowKind::Shortcuts => {
                // A grid of wide cells: three deep for tracks, two for the
                // web's shortcuts. The same heading a carousel draws, "See
                // all" and all: the key reaches these rows too, and
                // without the hint they were the one kind with no sign of
                // it. The hint is shown only when the grid cannot hold the
                // row, for the same reason a carousel's is.
                let deep = row.kind.grid_rows().unwrap_or(trackgrid::ROWS);
                let shown =
                    trackgrid::drawn(row.cards.len(), trackgrid::columns(area.width), deep);
                carousel::render_heading(
                    frame,
                    Rect { x: area.x, y, width: area.width, height: 1 },
                    palette,
                    &row.heading,
                    is_focused,
                    carousel::has_more(row.cards.len(), shown, row.more.is_some()),
                    false,
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
                        deep,
                        is_focused.then_some(row.state.selected),
                        marks,
                        &mut draw_cover,
                    );
                }
            }
            // Page links: a line of pills, headed like any other row. They
            // carry no artwork, so a carousel drew them as empty grey
            // squares -- eight rows of nothing per row of links.
            crate::browse::RowKind::Links => {
                carousel::render_heading(
                    frame,
                    Rect { x: area.x, y, width: area.width, height: 1 },
                    palette,
                    &row.heading,
                    is_focused,
                    carousel::has_more(
                        row.cards.len(),
                        carousel::visible_pills(&row.cards, area.width),
                        row.more.is_some(),
                    ),
                    true,
                );
                if height > 2 {
                    carousel::render_pills(
                        frame,
                        Rect { x: area.x, y: y + 2, width: area.width, height: 1 },
                        palette,
                        &row.cards,
                        is_focused.then_some(row.state.selected),
                        row.state.offset,
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
                        // What `o` opens: the path the API handed back for
                        // the rest of this row. Without it a row that fits
                        // hid the hint while the key still fetched more.
                        always_more: row.more.is_some(),
                        liked: marks.liked,
                    },
                    &mut draw_cover,
                );
            }
        }
        y += full + 1;
    }

    // Beside the rows, below the tabs: those do not scroll, so a bar
    // spanning them measures the wrong thing.
    let header = header_height_with(state.has_tabs);
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
        visible_rows_of(area.height, &state.rows),
    );
}

/// How tall a row of each kind wants to be.
///
/// A grid of three rows of tracks is taller than a strip of covers, so a
/// single height would either crop the grid or leave a gap under every
/// carousel.
fn row_height(kind: crate::browse::RowKind) -> u16 {
    // Both are a heading, the blank under it, and the content. The blank is
    // where a selected card's shade reaches, so it can mark the top of the
    // artwork without covering the heading.
    //
    // A carousel used to take one row more than that, which put two blank
    // lines between two carousels and one between a carousel and a track
    // grid — and cost a section on a short terminal. The loop's own gap is
    // the space between rows; the row itself does not carry a second one.
    const HEADING: u16 = 2;
    match kind {
        crate::browse::RowKind::Compact | crate::browse::RowKind::Shortcuts => {
            trackgrid::height(kind.grid_rows().unwrap_or(trackgrid::ROWS)) + HEADING
        }
        crate::browse::RowKind::Carousel => CARD_HEIGHT + HEADING,
        // A single line of pills, as the web draws them. They carry no
        // artwork, so the eight rows a cover needs would be eight rows of
        // empty grey.
        crate::browse::RowKind::Links => carousel::PILL_HEIGHT + HEADING,
    }
}

#[cfg(test)]
mod tests {

    /// No favourites, nothing playing — what most of these tests want.
    fn no_marks() -> super::super::trackgrid::Marks<'static> {
        use std::sync::OnceLock;
        static EMPTY: OnceLock<std::collections::HashSet<crate::domain::TrackId>> =
            OnceLock::new();
        super::super::trackgrid::Marks {
            favourites: EMPTY.get_or_init(Default::default),
            playing: None,
            tier: super::super::nowplaying::Tier::default(),
            liked: &crate::shell::carousel::nobody,
        }
    }

    #[test]
    fn the_last_row_is_cut_at_the_pane_rather_than_dropped() {
        // The page used to drop a row that did not fit whole, which left a
        // band of empty pane below the last one drawn. The cut is what says
        // the page carries on — and now that a cover can be cut rather than
        // shrunk, the row at the bottom shows as much of itself as fits.
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::cell::RefCell;

        let cards: Vec<Card> = (0..6)
            .map(|i| {
                let mut c = Card::new(format!("T{i}"), "artist");
                c.cover_url = Some("http://x".into());
                c
            })
            .collect();
        let mut state = HomeState { has_tabs: true, ..Default::default() };
        state.rows = (0..8)
            .map(|i| Row {
                heading: format!("Row {i}"),
                kind: crate::browse::RowKind::Carousel,
                cards: cards.clone(),
                state: CarouselState::default(),
                more: None,
            })
            .collect();

        // A height whose last row lands part-way through its covers.
        let heights = RefCell::new(Vec::<u16>::new());
        let mut term = Terminal::new(TestBackend::new(100, 60)).unwrap();
        term.draw(|f| {
            render(
                f,
                Rect::new(0, 0, 100, 50),
                &Palette::detect(),
                &state,
                true,
                no_marks(),
                |_f, area, _url, _shape| {
                    heights.borrow_mut().push(area.height);
                    true
                },
            );
        })
        .unwrap();

        let hs = heights.borrow();
        assert!(
            hs.iter().any(|&h| h > 0 && h < carousel::COVER_HEIGHT),
            "no row was cut at the pane's edge; heights were {hs:?}"
        );
        assert!(
            hs.iter().all(|&h| h >= 2),
            "a row was drawn too short to read as artwork: {hs:?}"
        );
    }
    use super::*;

    fn home_with_rows(n: usize) -> HomeState {
        HomeState {
            // The home page, which is the one these tests measure.
            has_tabs: true,
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
                    kind: crate::browse::RowKind::Compact,
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
    fn page_links_are_pills_on_one_line_rather_than_empty_covers() {
        // Explore's genres carry no artwork: their `imageId` is a name like
        // "hiphop", not a uuid, and nothing is served for it. Drawn as
        // covers they were a row of empty grey squares eight rows tall.
        // The web draws them as pills on a single line, and so does this.
        let mut home = home_with_rows(1);
        home.rows[0].kind = crate::browse::RowKind::Links;
        home.rows[0].heading = "Genres".into();
        home.rows[0].cards = ["Hip-Hop", "Pop", "Jazz"]
            .iter()
            .map(|t| carousel::Card::new(*t, ""))
            .collect();

        let buf = crate::shell::geometry::draw(100, 12, move |f, area, p| {
            render(f, area, p, &home, true, no_marks(), |_, _, _, _| false)
        });
        let text = crate::shell::geometry::text(&buf);

        assert!(text.contains("Hip-Hop"), "the titles are drawn:\n{text}");
        assert!(text.contains("Jazz"), "all of them:\n{text}");

        // All on one line, which is what a pill row is.
        let with_titles: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("Hip-Hop") || l.contains("Jazz"))
            .collect();
        assert_eq!(
            with_titles.len(),
            1,
            "the links share a line rather than standing in a column:\n{text}"
        );

        // And the row is short: a heading, a blank, and the pills. A
        // carousel would take eight rows of cover under the same heading.
        assert_eq!(
            row_height(crate::browse::RowKind::Links),
            carousel::PILL_HEIGHT + 2,
            "a row of links is its heading and one line"
        );
        assert!(
            row_height(crate::browse::RowKind::Links)
                < row_height(crate::browse::RowKind::Carousel),
            "and shorter than a row of covers"
        );
    }

    #[test]
    fn a_pill_row_counts_what_fits_by_the_width_of_the_titles() {
        // Pills are as wide as their titles, so how many fit depends on the
        // titles rather than on a fixed card width.
        let short: Vec<carousel::Card> = ["A", "B", "C", "D"]
            .iter()
            .map(|t| carousel::Card::new(*t, ""))
            .collect();
        let long: Vec<carousel::Card> = ["Reggae / Dancehall", "Dance & Electronic"]
            .iter()
            .map(|t| carousel::Card::new(*t, ""))
            .collect();

        assert_eq!(carousel::visible_pills(&short, 40), 4, "four short ones fit");
        assert_eq!(carousel::visible_pills(&long, 40), 1, "one long one does");
        assert_eq!(
            carousel::visible_pills(&short, 1),
            1,
            "a pane too narrow for any still counts one, so the keys work"
        );
    }

    #[test]
    fn a_row_that_fits_still_says_so_when_the_api_has_more() {
        // "New Tracks" on For You: nine tracks, and a grid that holds all
        // nine. The hint asked whether the cards ran past the edge -- they
        // do not -- while `o` asked whether the API handed back a path for
        // the rest, which it did. So the row showed nothing and the key
        // opened a whole view of more tracks.
        for kind in [crate::browse::RowKind::Compact, crate::browse::RowKind::Carousel] {
            let mut home = home_with_rows(1);
            home.rows[0].kind = kind;
            home.rows[0].heading = "New Tracks".into();
            home.rows[0].cards = (0..9)
                .map(|i| carousel::Card::new(format!("Track {i}"), "Artist"))
                .collect();
            home.rows[0].more = Some("pages/data/new-tracks".into());

            let buf = crate::shell::geometry::draw(80, 40, move |f, area, p| {
                render(f, area, p, &home, false, no_marks(), |_, _, _, _| false)
            });
            let text = crate::shell::geometry::text(&buf);
            assert_eq!(
                text.matches("See all").count(),
                1,
                "{kind:?}: nine fit, but there are more behind them:\n{text}"
            );
        }
    }

    #[test]
    fn a_row_offers_to_show_the_rest_only_when_there_is_a_rest() {
        // The track grids drew their own heading and so had no "See all",
        // though o opens them the same as any other row. And a row whose
        // cards all fit points at a key that would show the same cards
        // again, so the hint appears only when something is off the edge.
        let mut home = home_with_rows(2);
        home.rows[0].kind = crate::browse::RowKind::Compact;
        // More than a grid of six holds, and more than a carousel's width.
        home.rows[0].cards = (0..20)
            .map(|i| carousel::Card::new(format!("Track {i}"), "Artist"))
            .collect();
        home.rows[1].cards = (0..20)
            .map(|i| carousel::Card::new(format!("Card {i}"), "Artist"))
            .collect();
        let buf = crate::shell::geometry::draw(80, 40, move |f, area, p| {
            render(f, area, p, &home, false, no_marks(), |_, _, _, _| false)
        });
        let text = crate::shell::geometry::text(&buf);
        assert_eq!(
            text.matches("See all").count(),
            2,
            "both rows overflow, the grid included:\n{text}"
        );

        // And with rows that fit, neither offers it.
        let mut home = home_with_rows(2);
        home.rows[0].kind = crate::browse::RowKind::Compact;
        home.rows[0].cards = vec![carousel::Card::new("Track", "Artist")];
        home.rows[1].cards = vec![carousel::Card::new("Card", "Artist")];
        let buf = crate::shell::geometry::draw(80, 40, move |f, area, p| {
            render(f, area, p, &home, false, no_marks(), |_, _, _, _| false)
        });
        let text = crate::shell::geometry::text(&buf);
        assert_eq!(
            text.matches("See all").count(),
            0,
            "nothing is behind the edge, so nothing offers to show it:\n{text}"
        );
    }

    #[test]
    fn the_bottom_of_a_track_grid_steps_out_to_the_next_row() {
        // Reported: from the third line of Recently played, down did not
        // reach the next section. A row holds more cards than the grid
        // draws — it takes the first `columns * ROWS` — so stepping past
        // those moved the selection onto cards that were never on screen.
        let mut home = home_with_rows(2);
        home.rows[0].kind = crate::browse::RowKind::Compact;
        home.rows[0].cards = (0..30)
            .map(|i| Card::new(format!("Track {i}"), "artist"))
            .collect();
        home.row = 0;

        let columns = trackgrid::COLUMNS;
        let drawn = columns * trackgrid::ROWS;

        // Down through every line the grid draws.
        for line in 1..trackgrid::ROWS {
            home.down(10, columns);
            assert_eq!(home.row, 0, "still inside the grid on line {line}");
            assert_eq!(home.current_row().unwrap().state.selected, line * columns);
        }
        assert!(
            home.current_row().unwrap().state.selected < drawn,
            "the selection never leaves what is drawn"
        );

        // And once more leaves it, rather than walking onto cards that are
        // not on screen.
        home.down(10, columns);
        assert_eq!(home.row, 1, "down from the last drawn line leaves the row");
    }

    #[test]
    fn coming_up_into_a_track_grid_lands_on_a_card_that_is_drawn() {
        // Reported: moving up into Recently played said the selection was
        // on that row and nothing was highlighted. A row holds more cards
        // than the grid draws, and entering from below aimed at the last
        // line of everything it holds rather than of what is on screen.
        let mut home = home_with_rows(2);
        home.rows[0].kind = crate::browse::RowKind::Compact;
        home.rows[0].cards = (0..30)
            .map(|i| Card::new(format!("Track {i}"), "artist"))
            .collect();
        home.row = 1;

        let columns = trackgrid::COLUMNS;
        let drawn = columns * trackgrid::ROWS;

        home.up(10, columns);
        assert_eq!(home.row, 0, "up leaves the second row for the grid");
        let at = home.current_row().unwrap().state.selected;
        assert!(
            at < drawn,
            "the selection is on a card the grid draws: {at} of {drawn}"
        );
        assert!(
            at >= drawn - columns,
            "and on its bottom line, as coming up should: {at}"
        );
    }

    #[test]
    fn one_blank_line_between_every_pair_of_rows() {
        // A track row left two blank lines below it where a carousel left
        // one: the grid counted a gap under its last line of cells, which
        // is the space *between* lines and not below them.
        for kinds in [
            [crate::browse::RowKind::Carousel, crate::browse::RowKind::Carousel],
            [crate::browse::RowKind::Carousel, crate::browse::RowKind::Compact],
            [crate::browse::RowKind::Compact, crate::browse::RowKind::Carousel],
            [crate::browse::RowKind::Compact, crate::browse::RowKind::Compact],
        ] {
            let mut state = HomeState::default();
            for (i, kind) in kinds.iter().enumerate() {
                state.rows.push(Row {
                    heading: format!("Row {i}"),
                    kind: *kind,
                    cards: (0..9)
                        .map(|c| {
                            let mut card = Card::new(format!("R{i}C{c}"), "artist");
                            // A cover, so the artwork rows are painted and
                            // "blank" means blank rather than "no picture".
                            card.cover_url = Some("http://x".into());
                            card
                        })
                        .collect(),
                    state: CarouselState::default(),
                    more: None,
                });
            }
            let buf = crate::shell::geometry::draw(120, 60, move |f, area, p| {
                // `false`, so the placeholder is painted: an image
                // protocol writes nothing a test buffer can see, and every
                // artwork row would read as blank.
                render(f, area, p, &state, false, no_marks(), |_, _, _, _| false)
            });
            let a = crate::shell::geometry::find(&buf, "Row 0").expect("first").row;
            let b = crate::shell::geometry::find(&buf, "Row 1").expect("second").row;

            // Counting up from the second heading: exactly one clear line
            // before the first row's content starts again.
            let clear = |y: u16| {
                (0..120u16).all(|x| {
                    let c = &buf[(x, y)];
                    c.symbol().trim().is_empty()
                        && c.bg == ratatui::style::Color::Reset
                })
            };
            assert!(clear(b - 1), "{kinds:?}: a clear line above the next heading");
            assert!(
                !clear(b - 2),
                "{kinds:?}: and only one — row {} is blank too",
                b - 2
            );
            let _ = a;
        }
    }

    #[test]
    fn a_selected_cards_shade_never_reaches_the_next_rows_heading() {
        // The rows sit one line closer together than they did. What that
        // must not cost is the clear line under a selected card: its shade
        // touching the heading below would read as one band across two
        // rows.
        let palette = crate::shell::theme::Palette::detect();
        let mut home = home_with_rows(3);
        home.row = 0;
        let buf = crate::shell::geometry::draw(60, 60, move |f, area, p| {
            render(f, area, p, &home, true, no_marks(), |_, _, _, _| false)
        });

        // The heading of the row under the selected one.
        let next = crate::shell::geometry::find(&buf, "Row 1").expect("the next heading");
        let shaded_on_that_row = (0..60u16)
            .filter(|x| buf[(*x, next.row)].bg == palette.selection)
            .count();
        assert_eq!(
            shaded_on_that_row, 0,
            "the shade stops short of the next row's heading"
        );
        // And the line above it is clear too, or the band would touch.
        let above = next.row.saturating_sub(1);
        let shaded_above = (0..60u16)
            .filter(|x| buf[(*x, above)].bg == palette.selection)
            .count();
        assert_eq!(shaded_above, 0, "with a clear line between them");
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
            render(f, area, p, &home, true, no_marks(), |_, _, _, _| false)
        });

        // Only the selected row's band: a cover placeholder is the same
        // colour, so the rows either side would be counted too. The window
        // is the card itself and a row of margin, no wider — with the rows
        // now one line closer together, a wider one reaches the next row.
        let title = crate::shell::geometry::find(&buf, "Card A").expect("the card");
        let shaded: Vec<(u16, u16)> = (title.row.saturating_sub(carousel::CARD_HEIGHT)
            ..(title.row + 2))
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
            render(f, area, p, &home, true, no_marks(), |_, _, _, _| false)
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
        // The card's own title row, which is the shade's colour rather than
        // the artwork's: the cover's rows carry a placeholder, or real
        // pixels, over the shade. That showed once the greys stopped
        // rounding together -- in 256 colours they are one index apart.
        assert_eq!(
            buf[(title.start, title.row)].bg,
            palette.selection,
            "and the card itself is shaded\n{}",
            crate::shell::geometry::text(&buf)
        );
    }

    #[test]
    fn the_last_row_drawn_shows_its_heading() {
        // From the report: on a 61-line terminal the home page's last row
        // showed nothing at all, not even its heading, because the floor
        // for a part-drawn row was too high. Whichever row the height puts
        // at the bottom, it names itself.
        // Rebuilt per height, since a Row is not Clone.
        fn the_page() -> HomeState {
            let mut state = HomeState::default();
            for (heading, kind) in [
                ("The Hits", crate::browse::RowKind::Carousel),
                ("New Tracks", crate::browse::RowKind::Compact),
                ("New Albums", crate::browse::RowKind::Carousel),
                ("Spotlighted Uploads", crate::browse::RowKind::Compact),
                ("From our editors", crate::browse::RowKind::Carousel),
            ] {
                state.rows.push(Row {
                    heading: heading.into(),
                    kind,
                    cards: (0..9)
                        .map(|i| Card::new(format!("{heading} {i}"), "artist"))
                        .collect(),
                    state: CarouselState::default(),
                    more: None,
                });
            }
            state
        }

        // Every height that draws more than one row: the last of them must
        // carry its heading, whichever it turns out to be.
        for height in 30..60u16 {
            let state = the_page();
            let n = visible_rows_of(height, &state.rows);
            if n < 2 {
                continue;
            }
            let last = state.rows[n - 1].heading.clone();
            let buf = crate::shell::geometry::draw(199, height, move |f, area, p| {
                render(f, area, p, &state, false, no_marks(), |_, _, _, _| false)
            });
            let text = crate::shell::geometry::text(&buf);
            assert!(
                text.contains(&last),
                "at height {height}: the last of {n} rows, {last:?}, drew no heading:\n{text}"
            );
        }
    }

    #[test]
    fn a_row_with_only_its_heading_left_still_shows_it() {
        // The pane's edge cuts the page the way a browser's does: whatever
        // is left is drawn, down to a heading alone. A floor here ended the
        // page in a band of empty pane that no other view had.
        let home = home_with_rows(6);
        let full = row_height(crate::browse::RowKind::Carousel);

        // Room for one whole row and one line of the next: its heading.
        let height = header_height() + full + 1 + 1;
        let buf = crate::shell::geometry::draw(60, height, move |f, area, p| {
            render(f, area, p, &home, false, no_marks(), |_, _, _, _| false)
        });
        let text = crate::shell::geometry::text(&buf);

        assert!(text.contains("Row 0"), "the row that fits is drawn:\n{text}");
        assert!(text.contains("Row 1"), "and the next shows its heading at the fold:\n{text}");
    }

    #[test]
    fn the_count_matches_what_is_drawn_for_rows_of_different_heights() {
        // A track grid is taller than a carousel, so dividing the pane by
        // one row height put the selection on rows that were never drawn.
        let mut home = home_with_rows(4);
        home.rows[1].kind = crate::browse::RowKind::Compact;

        for height in 12..50u16 {
            let n = visible_rows_of(height, &home.rows);
            let mut state = home_with_rows(4);
            state.rows[1].kind = crate::browse::RowKind::Compact;
            let buf = crate::shell::geometry::draw(80, height, move |f, area, p| {
                render(f, area, p, &state, false, no_marks(), |_, _, _, _| false)
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
    fn an_empty_track_row_is_stepped_over_rather_than_panicked_on() {
        // A row the API returned with no tracks in it still counts as a
        // row, and the cursor still passes through it. Working out its
        // bottom card means subtracting one from what it draws, which is
        // nothing -- so the subtraction has to hold at zero rather than
        // wrap into a selection no grid could ever satisfy.
        let mut home = home_with_grid(0);
        home.row = 2;

        home.up(10, 3);

        assert_eq!(home.row, 1, "landed on the empty row");
        assert_eq!(
            home.current_row().unwrap().state.selected,
            0,
            "nothing to select, so the first cell"
        );
    }

    #[test]
    fn the_left_hand_card_of_a_grids_second_row_still_steps_up_inside_it() {
        // The boundary between "there is a row above me here" and "this is
        // the top of the grid". The first card of the second row sits
        // exactly one row in, so an off-by-one here reads it as the top and
        // leaves the grid altogether -- pressing up from the left-hand card
        // would skip the whole first row and jump to the section above.
        let mut home = home_with_grid(6);
        home.row = 1;
        home.current_row_mut().unwrap().state.selected = 3;

        home.up(10, 3);

        assert_eq!(home.row, 1, "up from the second row stays inside the grid");
        assert_eq!(
            home.current_row().unwrap().state.selected,
            0,
            "straight up, onto the first card"
        );
    }

    #[test]
    fn dropping_into_a_grid_keeps_the_column_it_was_left_on() {
        // The top row, but not its first card: a grid remembers the column
        // the cursor was in when it was last left, and coming back into it
        // from above lands on the top row of that same column. Folding to
        // the first card instead would drag the eye to the left edge of the
        // page every time the cursor passed through.
        for (left_on, expected) in [(0usize, 0usize), (2, 2), (4, 1), (5, 2)] {
            let mut home = home_with_grid(6);
            home.row = 1;
            home.current_row_mut().unwrap().state.selected = left_on;
            home.row = 0;
            home.down(10, 3);

            assert_eq!(home.row, 1);
            assert_eq!(
                home.current_row().unwrap().state.selected,
                expected,
                "left on card {left_on} of a three-wide grid, so column {expected}"
            );
        }
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
        let visible = visible_rows(30);
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
        let visible = visible_rows(30);
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
        let visible = visible_rows(60);
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
            has_tabs: true,
            heading: None,
            filter: String::new(),
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
    fn the_page_shows_tabs_and_row_headings() {
        let palette = Palette::detect();
        let state = sample();
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &palette, &state, true, no_marks(), |_, _, _, _| false))
            .unwrap();

        let text = flatten(&terminal);
        assert!(text.contains("For you"), "tab strip missing:\n{text}");
        assert!(text.contains("Uploads"), "the web's third tab:\n{text}");
        assert!(text.contains("New albums for you"), "first row heading missing:\n{text}");
        assert!(text.contains("Album 0"), "carousel cards missing:\n{text}");
    }

    #[test]
    fn only_a_row_that_scrolls_sideways_gets_the_arrows() {
        // A grid's rest is behind "See all" alone: arrows over it pointed
        // at keys that step through its cells.
        let mut home = home_with_rows(2);
        home.rows[0].kind = crate::browse::RowKind::Compact;
        home.rows[0].more = Some("home/pages/X/view-all".into());
        home.rows[0].cards = (0..20)
            .map(|i| carousel::Card::new(format!("Cell {i}"), "Someone"))
            .collect();
        home.rows[1].more = Some("home/pages/Y/view-all".into());
        home.rows[1].cards = (0..20)
            .map(|i| carousel::Card::new(format!("Card {i}"), "Someone"))
            .collect();
        let buf = crate::shell::geometry::draw(80, 40, move |f, area, p| {
            render(f, area, p, &home, false, no_marks(), |_, _, _, _| false)
        });
        let text = crate::shell::geometry::text(&buf);
        let grid_line = text.lines().find(|l| l.contains("Row 0")).expect("the grid's heading");
        let strip_line = text.lines().find(|l| l.contains("Row 1")).expect("the strip's heading");
        assert!(grid_line.contains("See all") && !grid_line.contains("‹"), "{grid_line}");
        assert!(strip_line.contains("‹ ›  See all"), "{strip_line}");
    }

    #[test]
    fn a_wide_row_is_a_two_deep_grid_that_the_keys_step_through() {
        // The web's shortcut grid: the track grid's cells, two lines of
        // them. Six cards fill it; a seventh is not drawn, and stepping
        // down from the second line leaves the row.
        let mut home = home_with_rows(2);
        home.rows[0].kind = crate::browse::RowKind::Shortcuts;
        home.rows[0].cards = (0..7)
            .map(|i| carousel::Card::new(format!("Shortcut {i}"), "Someone"))
            .collect();
        assert!(
            row_height(crate::browse::RowKind::Shortcuts) < row_height(crate::browse::RowKind::Compact),
            "two lines of cells, not three"
        );
        let buf = crate::shell::geometry::draw(100, 40, move |f, area, p| {
            render(f, area, p, &home, true, no_marks(), |_, _, _, _| false)
        });
        let text = crate::shell::geometry::text(&buf);
        assert!(text.contains("Shortcut 5"), "the sixth cell is drawn:\n{text}");
        assert!(!text.contains("Shortcut 6"), "the seventh is not:\n{text}");

        let mut home = home_with_rows(2);
        home.rows[0].kind = crate::browse::RowKind::Shortcuts;
        home.rows[0].cards = (0..7)
            .map(|i| carousel::Card::new(format!("Shortcut {i}"), "Someone"))
            .collect();
        home.down(10, 3);
        assert_eq!((home.row, home.rows[0].state.selected), (0, 3), "down the first column");
        home.down(10, 3);
        assert_eq!(home.row, 1, "and out of the grid: nothing is drawn below its second line");
    }

    #[test]
    fn tabs_cycle() {
        // However many tabs there are: stepping through all of them comes
        // back to the first.
        let mut s = HomeState::default();
        assert_eq!(s.tab, 0);
        for i in 1..tab_count() {
            s.next_tab();
            assert_eq!(s.tab, i);
        }
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
            .draw(|f| render(f, f.area(), &palette, &state, true, no_marks(), |_, _, _, _| false))
            .unwrap();
    }

    #[test]
    fn a_one_cell_pane_does_not_panic() {
        let palette = Palette::detect();
        let state = sample();
        let mut terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &palette, &state, false, no_marks(), |_, _, _, _| false))
            .unwrap();
    }
}
