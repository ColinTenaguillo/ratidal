//! A horizontal row of cover cards, as the web client shows them.
//!
//! Nothing upstream does this: ratatui's own RFC reached no consensus and
//! `tui-scrollview` was archived without horizontal scrolling. The maintainers'
//! guidance is offset windowing at the data layer — compute which cards are
//! visible and render only those, rather than drawing everything into an
//! oversized buffer and cropping.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use super::theme::Palette;

/// Card geometry. A cover is square in pixels, and terminal cells are roughly
/// twice as tall as they are wide, so a 16-column card needs 8 rows of cover
/// to look square.
pub const CARD_WIDTH: u16 = 16;
pub const COVER_HEIGHT: u16 = 8;
/// Cover, then title, then artist.
pub const CARD_HEIGHT: u16 = COVER_HEIGHT + 2;
/// The gutter between cards.
const GAP: u16 = 3;

/// Columns a card's text stops short of its own right edge.
///
/// A cover fills the card's full width and is followed by the gutter, so it
/// never touches its neighbour. Text truncated to the same width does: a
/// title long enough to be cut ran to the last column, putting its ellipsis
/// hard against the next card while the picture above it stopped short.
const TEXT_MARGIN: u16 = 1;

/// What activating a card does. A card without one is a label — a mix or a
/// module the API gave us no identifier for — and enter does nothing on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Playlist(String),
    Album(u64),
    Track(u64),
}

/// One card: a cover plus its lines of text.
#[derive(Debug, Clone, Default)]
pub struct Card {
    pub title: String,
    pub subtitle: String,
    /// A third line under the subtitle, as the playlist grid's track count.
    /// Empty means the card has only two lines.
    pub detail: String,
    pub cover_url: Option<String>,
    /// Artist avatars are round and centred, with no subtitle. Everything
    /// else is a square cover with left-aligned text.
    pub round: bool,
    /// What enter opens or plays. `None` on a card the API gave no id for.
    pub target: Option<Target>,
    /// How long a track card's track runs. Zero for anything that is not a
    /// track, and for a track the API did not say. Carried on the card
    /// because playing from one is all the player knows until the stream
    /// starts — without it the bar had no duration to divide by and drew
    /// itself full over a track that had just begun.
    pub duration: std::time::Duration,
}

impl Card {
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        Self { title: title.into(), subtitle: subtitle.into(), ..Default::default() }
    }
}

/// A card's height for `lines` lines of text under the cover.
pub fn card_height(lines: u16) -> u16 {
    COVER_HEIGHT + lines
}

#[derive(Debug, Default)]
pub struct CarouselState {
    /// Index of the leftmost fully visible card.
    pub offset: usize,
    pub selected: usize,
}

impl CarouselState {
    /// Move the selection right, keeping it visible.
    pub fn next(&mut self, len: usize, visible: usize) {
        if len == 0 {
            return;
        }
        self.selected = (self.selected + 1).min(len - 1);
        self.scroll_into_view(visible);
    }

    pub fn previous(&mut self, visible: usize) {
        self.selected = self.selected.saturating_sub(1);
        self.scroll_into_view(visible);
    }

    /// Pull `offset` just far enough that `selected` is on screen.
    fn scroll_into_view(&mut self, visible: usize) {
        let visible = visible.max(1);
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + visible {
            self.offset = self.selected + 1 - visible;
        }
    }
}

/// How many whole cards fit in `width`.
pub fn visible_cards(width: u16) -> usize {
    if width < CARD_WIDTH {
        return 0;
    }
    // n cards need n*CARD_WIDTH + (n-1)*GAP columns.
    (((width + GAP) / (CARD_WIDTH + GAP)) as usize).max(1)
}

/// Everything one row needs to draw itself, so the call does not take eight
/// loose arguments.
pub struct Row<'a> {
    pub heading: &'a str,
    pub cards: &'a [Card],
    pub state: &'a CarouselState,
    pub focused: bool,
}

/// Render a titled row of cards.
///
/// `draw_cover` is handed each card's area and returns true if it drew a real
/// image; when it returns false a coloured block stands in, so the row looks
/// the same shape whether or not the terminal can show pictures.
pub fn render<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    row_spec: Row<'_>,
    mut draw_cover: F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    let Row { heading, cards, state, focused } = row_spec;
    if area.height < 2 || area.width == 0 {
        return;
    }

    // Heading, then the scroll hint pushed to the right.
    let hint = "‹ ›  See all";
    let hint_width = hint.chars().count() as u16;
    let heading_style = if focused {
        palette.accent_text()
    } else {
        palette.title()
    };
    frame.render_widget(
        Paragraph::new(Line::styled(heading, heading_style)),
        Rect { height: 1, ..area },
    );
    if area.width > hint_width + 2 {
        frame.render_widget(
            Paragraph::new(Line::styled(hint, palette.subtitle())),
            Rect {
                x: area.x + area.width - hint_width,
                y: area.y,
                width: hint_width,
                height: 1,
            },
        );
    }

    let row = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: area.height.saturating_sub(1),
    };
    if row.height == 0 {
        return;
    }

    let mut x = row.x;
    for (i, card) in cards.iter().enumerate().skip(state.offset) {
        if x >= row.x + row.width {
            break;
        }
        // Rect is u16, so a card running past the right edge cannot be given a
        // negative width — clip it to what is left instead.
        let width = CARD_WIDTH.min(row.x + row.width - x);
        let card_area = Rect {
            x,
            y: row.y,
            width,
            height: row.height.min(CARD_HEIGHT),
        };
        render_card(
            frame,
            card_area,
            palette,
            card,
            focused && i == state.selected,
            &mut draw_cover,
        );
        x = x.saturating_add(CARD_WIDTH + GAP);
    }
}

pub(crate) fn render_card<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    card: &Card,
    selected: bool,
    draw_cover: &mut F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    let cover_height = COVER_HEIGHT.min(area.height);
    let cover = Rect { height: cover_height, ..area };

    let drew = match &card.cover_url {
        Some(url) if cover.height > 0 => {
            let shape = if card.round {
                super::artwork::Shape::Round
            } else {
                super::artwork::Shape::Square
            };
            draw_cover(frame, cover, url, shape)
        }
        _ => false,
    };
    if !drew && cover.height > 0 {
        // The placeholder is not a fallback for a missing image so much as the
        // normal case: most terminals cannot draw one at all.
        if card.round {
            // Uppercased: TIDAL's own avatars use a capital, and a lowercase
            // initial next to a capital one looks like a mistake.
            let initial = card
                .title
                .chars()
                .find(|c| c.is_alphanumeric())
                .map(|c| c.to_uppercase().next().unwrap_or(c));
            render_disc(frame, cover, palette, initial);
        } else {
            frame.render_widget(
                Block::default().style(Style::default().bg(palette.placeholder)),
                cover,
            );
        }
    }

    // A selected card is marked on its title, not its cover: painting over
    // the cover would hide the artwork that is the point of the card.
    //
    // The title's whole line is inverted rather than prefixed with a caret.
    // A caret pushed the text a column right of the artwork it belongs to;
    // an inversion marks it in place, and unlike a foreground tint it stays
    // visible on a terminal with no truecolor — which is what the caret was
    // there to guarantee.
    let text_style = if selected {
        palette.row_focused()
    } else {
        palette.title()
    };

    let mut y = area.y + cover_height;
    let bottom = area.y + area.height;

    // A round card centres its single label under the avatar; a square one
    // left-aligns a title and its subtitles.
    if card.round {
        if y < bottom {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    truncate(&card.title, area.width),
                    text_style,
                ))
                .alignment(ratatui::layout::Alignment::Center),
                Rect { x: area.x, y, width: area.width, height: 1 },
            );
        }
        return;
    }

    if y < bottom {
        // Flush with the cover's left edge. A caret in front of the title
        // pushed it two columns right of the artwork it belongs to, and the
        // subtitle with it — every card's text hanging off its own picture.
        // Selection is carried by colour alone, which is what the web client
        // does and what the rest of this UI already does everywhere else.
        let text_w = area.width.saturating_sub(TEXT_MARGIN);
        frame.render_widget(
            Paragraph::new(Line::styled(truncate(&card.title, text_w), text_style)),
            Rect { x: area.x, y, width: text_w, height: 1 },
        );
        y += 1;
    }
    for text in [&card.subtitle, &card.detail] {
        if text.is_empty() || y >= bottom {
            continue;
        }
        let text_w = area.width.saturating_sub(TEXT_MARGIN);
        frame.render_widget(
            Paragraph::new(Line::styled(truncate(text, text_w), palette.subtitle())),
            Rect { x: area.x, y, width: text_w, height: 1 },
        );
        y += 1;
    }
}

/// A filled circle carrying the artist's initial, for an avatar with no
/// picture to show.
///
/// TIDAL has no photo for a large share of artists — 42% of one real
/// library — so this is a common sight, not an edge case. An unadorned disc
/// in the surface colour is nearly the background colour, which read as a
/// missing image rather than an artist without one.
///
/// Cells are about twice as tall as wide, so the row offset is doubled before
/// the radius test; without that the "circle" comes out as a tall ellipse.
fn render_disc(frame: &mut Frame, area: Rect, palette: &Palette, initial: Option<char>) {
    let style = Style::default().bg(palette.placeholder);
    let cx = (area.width as f32 - 1.0) / 2.0;
    let cy = (area.height as f32 - 1.0) / 2.0;
    // Whichever axis is smaller bounds the circle, pulled in slightly: at a
    // radius that exactly reaches the edge, the middle rows all round out to
    // the full width and the "circle" comes out an octagon.
    let radius = (cx + 0.5).min((cy + 0.5) * 2.0) * 0.95;

    for row in 0..area.height {
        let dy = (row as f32 - cy) * 2.0;
        // Half-width of the circle at this row: r² = dx² + dy².
        let half = radius * radius - dy * dy;
        if half <= 0.0 {
            continue;
        }
        let half = half.sqrt();
        let x0 = (cx - half).round().max(0.0) as u16;
        let x1 = (cx + half).round().min(area.width as f32 - 1.0) as u16;
        if x1 < x0 {
            continue;
        }
        frame.render_widget(
            Block::default().style(style),
            Rect { x: area.x + x0, y: area.y + row, width: x1 - x0 + 1, height: 1 },
        );
    }

    // The initial, centred. Skipped on a disc too small to hold a character
    // without covering the shape that says "artist".
    if let Some(c) = initial {
        if area.width >= 3 && area.height >= 3 {
            frame.render_widget(
                Paragraph::new(Line::styled(c.to_string(), palette.subtitle()))
                    .alignment(ratatui::layout::Alignment::Center),
                Rect {
                    x: area.x,
                    y: area.y + area.height / 2,
                    width: area.width,
                    height: 1,
                },
            );
        }
    }
}

/// Cut to `width` columns, with an ellipsis when something was removed.
pub(crate) fn truncate(s: &str, width: u16) -> String {
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

    /// Count the filled cells per row of a rendered disc.
    fn disc_widths(w: u16, h: u16) -> Vec<usize> {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let palette = Palette::detect();
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render_disc(f, f.area(), &palette, None)).unwrap();
        let buf = term.backend().buffer();
        (0..h)
            .map(|y| {
                (0..w)
                    .filter(|&x| buf[(x, y)].bg == palette.placeholder)
                    .count()
            })
            .collect()
    }

    #[test]
    fn the_avatar_placeholder_is_round_not_square() {
        // The whole point of the profiles grid is that its cards are circles.
        // A square placeholder would make it indistinguishable from albums.
        let widths = disc_widths(16, 8);
        let middle = widths[widths.len() / 2];
        assert!(middle > widths[0], "the disc must be widest in the middle");
        assert!(middle > *widths.last().unwrap());
        assert!(widths[0] < 16, "the top row must not span the full width");

        // An octagon passes every check above: it is widest in the middle and
        // narrow at the top. What separates a circle from one is that the
        // width keeps changing between the two — at a radius that reaches the
        // edge, four middle rows all come out full-width.
        let full = widths.iter().filter(|&&n| n == 16).count();
        assert!(full <= 2, "a circle flattens out for at most two rows, got {full}");
    }

    #[test]
    fn a_disc_in_a_tiny_area_does_not_panic() {
        for (w, h) in [(1, 1), (2, 1), (1, 2), (0, 0), (3, 2)] {
            let _ = disc_widths(w.max(1), h.max(1));
        }
    }

    #[test]
    fn card_count_accounts_for_the_gaps() {
        // 16-wide cards with a 3-column gutter: one needs 16, two need 35,
        // three need 54.
        assert_eq!(visible_cards(16), 1);
        assert_eq!(visible_cards(34), 1);
        assert_eq!(visible_cards(35), 2);
        assert_eq!(visible_cards(54), 3);
    }

    #[test]
    fn a_pane_too_narrow_for_one_card_shows_none() {
        assert_eq!(visible_cards(15), 0);
        assert_eq!(visible_cards(0), 0);
    }

    #[test]
    fn scrolling_right_pulls_the_window_along() {
        let mut s = CarouselState::default();
        for _ in 0..5 {
            s.next(10, 3);
        }
        assert_eq!(s.selected, 5);
        // With three visible, selecting index 5 puts the window at 3..6.
        assert_eq!(s.offset, 3);
    }

    #[test]
    fn scrolling_back_left_pulls_the_window_back() {
        let mut s = CarouselState { offset: 4, selected: 6 };
        s.previous(3);
        s.previous(3);
        s.previous(3);
        assert_eq!(s.selected, 3);
        assert_eq!(s.offset, 3, "the window follows the selection back");
    }

    #[test]
    fn the_selection_stops_at_the_last_card() {
        let mut s = CarouselState::default();
        for _ in 0..20 {
            s.next(4, 3);
        }
        assert_eq!(s.selected, 3);
    }

    #[test]
    fn an_empty_row_does_not_move() {
        let mut s = CarouselState::default();
        s.next(0, 3);
        assert_eq!(s.selected, 0);
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn the_selected_card_is_marked_without_relying_on_a_foreground_tint() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        // The mark used to be a caret, because a foreground tint alone was
        // invisible enough that moving the selection looked like nothing had
        // happened. But the caret pushed the title a column right of its
        // cover, so the text on every card hung off its own artwork.
        //
        // An inverted line marks it in place and survives a terminal with no
        // truecolor, which is what the caret was guaranteeing.
        let cards = vec![Card::new("First", "A"), Card::new("Second", "B")];
        let palette = Palette::detect();
        let state = CarouselState { offset: 0, selected: 1 };

        let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    f.area(),
                    &palette,
                    Row { heading: "Row", cards: &cards, state: &state, focused: true },
                    |_, _, _, _| false,
                );
            })
            .unwrap();

        let b = terminal.backend().buffer().clone();
        let cell_at = |needle: &str| {
            for y in 0..b.area.height {
                let line: String = (0..b.area.width)
                    .map(|x| b[(x, y)].symbol().to_string())
                    .collect();
                if let Some(byte) = line.find(needle) {
                    let x = line[..byte].chars().count() as u16;
                    return Some(b[(x, y)].clone());
                }
            }
            None
        };

        let selected = cell_at("Second").expect("the selected title is drawn");
        let other = cell_at("First").expect("the unselected title is drawn");

        assert_ne!(
            selected.bg, other.bg,
            "the selected card is marked by a background, which survives a \
             terminal that ignores foreground colour"
        );
        assert_eq!(
            selected.bg, palette.accent,
            "and it is the accent that marks it"
        );
    }

    #[test]
    fn a_truncated_title_does_not_touch_the_next_card() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        // A cover fills the card and the gutter keeps it off its neighbour.
        // Text truncated to the same width had no such gutter: a title long
        // enough to be cut ran to the last column and put its ellipsis hard
        // against the next card.
        let cards = vec![
            Card::new("A title far too long to fit", "Artist"),
            Card::new("Second", "Artist"),
        ];
        let palette = Palette::detect();
        let state = CarouselState::default();

        let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    f.area(),
                    &palette,
                    Row { heading: "Row", cards: &cards, state: &state, focused: true },
                    |_, _, _, _| false,
                );
            })
            .unwrap();

        let b = terminal.backend().buffer().clone();
        let title_row = (0..b.area.height)
            .map(|y| {
                (0..b.area.width)
                    .map(|x| b[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .find(|l| l.contains('…'))
            .expect("a truncated title");

        let cells: Vec<char> = title_row.chars().collect();
        let ellipsis = cells.iter().position(|c| *c == '…').expect("the ellipsis");
        assert!(
            ellipsis < CARD_WIDTH as usize,
            "the ellipsis is inside the first card"
        );
        assert_eq!(
            cells[CARD_WIDTH as usize - 1],
            ' ',
            "the card's last column is clear, so the text does not run into \
             the gutter:\n{title_row}"
        );
    }

    #[test]
    fn a_card_title_starts_at_the_covers_left_edge() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        // The caret indented the title by two columns while the cover began
        // at zero, so the text hung off the picture it belonged to.
        let cards = vec![Card::new("First", "Artist")];
        let palette = Palette::detect();
        let state = CarouselState::default();

        let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    f.area(),
                    &palette,
                    Row { heading: "Row", cards: &cards, state: &state, focused: true },
                    |_, _, _, _| false,
                );
            })
            .unwrap();

        let b = terminal.backend().buffer().clone();
        let column_of = |needle: &str| {
            for y in 0..b.area.height {
                let line: String = (0..b.area.width)
                    .map(|x| b[(x, y)].symbol().to_string())
                    .collect();
                if let Some(byte) = line.find(needle) {
                    return Some(line[..byte].chars().count() as u16);
                }
            }
            None
        };

        let title = column_of("First").expect("the title is drawn");
        let subtitle = column_of("Artist").expect("the subtitle is drawn");
        assert_eq!(title, 0, "the title starts where the cover does");
        assert_eq!(subtitle, title, "and the subtitle lines up under it");
    }

    #[test]
    fn truncation_marks_what_it_cut() {
        assert_eq!(truncate("Short", 10), "Short");
        assert_eq!(truncate("A very long album title", 10), "A very lo…");
        assert_eq!(truncate("abc", 1), "…");
        assert_eq!(truncate("abc", 0), "");
    }

    #[test]
    fn rendering_a_row_wider_than_the_pane_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let cards: Vec<Card> = (0..20)
            .map(|i| Card::new(format!("Album {i}"), "Artist"))
            .collect();
        let palette = Palette::detect();
        let state = CarouselState::default();

        // A pane that cuts a card in half at the right edge: Rect is u16, so
        // the clip has to be computed rather than allowed to go negative.
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    f.area(),
                    &palette,
                    Row {
                        heading: "Nouveaux albums",
                        cards: &cards,
                        state: &state,
                        focused: true,
                    },
                    |_, _, _, _| false,
                );
            })
            .unwrap();
    }

    #[test]
    fn rendering_into_a_one_cell_pane_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let palette = Palette::detect();
        let state = CarouselState::default();
        let cards = vec![Card::new("T", "S")];
        let mut terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    f.area(),
                    &palette,
                    Row { heading: "H", cards: &cards, state: &state, focused: false },
                    |_, _, _, _| false,
                );
            })
            .unwrap();
    }
}
