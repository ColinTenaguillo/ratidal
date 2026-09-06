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

/// Rows of cover on a card.
pub const COVER_HEIGHT: u16 = 8;

/// Columns a card is wide, for a square cover.
///
/// Not a constant: it depends on the shape of a terminal cell, which varies
/// by terminal and font. Sixteen columns assumes a cell exactly twice as
/// tall as it is wide; on cells of 19x30 the cover only fills twelve of
/// them and the other four are empty, always on the same side, which is
/// what made a selected card's shading look lopsided.
///
/// Set once at startup from the size the image picker measured, and read
/// from everywhere that lays cards out.
static CARD_COLUMNS: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(16);

/// The width a card is drawn at.
pub fn card_width() -> u16 {
    CARD_COLUMNS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Work out the card width from a terminal cell's pixel size.
///
/// A cover is square, so it needs as many columns as `COVER_HEIGHT` rows of
/// pixels covers. Clamped: a terminal reporting something absurd should give
/// a card that is merely wrong rather than one that is zero or fills the
/// pane.
pub fn set_cell_size(cell_w: u16, cell_h: u16) {
    CARD_COLUMNS.store(
        columns_for_cell(cell_w, cell_h),
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// How many columns a square thumbnail of `rows` needs.
///
/// The same rule a card follows, for the smaller artwork in a track row and
/// a track grid: those had their own fixed pairs, which assumed the same
/// 1:2 cell and were wrong in the same way.
pub fn square_width(rows: u16) -> u16 {
    square_width_of(rows, card_width())
}

/// The proportion itself, with the card width passed in.
///
/// Separate so it can be checked at widths other than the default — at
/// sixteen columns for eight rows the ratio is exactly two, which any
/// fixed two-columns-per-row rule also satisfies, so a test that only ever
/// sees that width cannot tell the two apart.
fn square_width_of(rows: u16, card: u16) -> u16 {
    (rows * card).div_ceil(COVER_HEIGHT).max(1)
}

/// The card width a cell of this size calls for.
///
/// Separate from the store so it can be tested without writing the global
/// that every other test reads — the suite runs in parallel, and a test that
/// reached in to change it would make the rest flaky.
fn columns_for_cell(cell_w: u16, cell_h: u16) -> u16 {
    if cell_w == 0 {
        return 16;
    }
    // Clamped: a terminal reporting something absurd should give a card that
    // is merely wrong rather than one that is zero or fills the pane.
    (COVER_HEIGHT * cell_h).div_ceil(cell_w).clamp(8, 40)
}
/// Cover, then title, then artist.
pub const CARD_HEIGHT: u16 = COVER_HEIGHT + 2;
/// The gutter between cards.
const GAP: u16 = 3;

/// The least of a card that is worth drawing at the row's edge.
///
/// Two columns of cover is a stripe rather than a picture, and the title
/// under it would be one letter and an ellipsis.
const MIN_PARTIAL: u16 = 4;

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
    /// An artist's own page. A profile card opened nothing before; it is
    /// the one kind of card the app drew with nowhere to go.
    Artist(u64),
    /// A mix, whose id is a string rather than a number.
    Mix(String),
    /// Another page of rows, which is what an Explore link opens: a genre,
    /// a mood, a decade. They carry a path rather than an id.
    Page(String),
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

/// The card for an album, wherever it is shown.
///
/// Written out three times — the collection grid, the search results and an
/// artist's page — which is three chances for the same record to read
/// differently depending on where it is drawn. It already had: a playlist
/// showed its running time in one place and only its track count in
/// another.
pub fn album_card(a: &crate::library::Album) -> Card {
    Card {
        title: a.title.clone(),
        subtitle: a.artist.clone(),
        // The year on the card, since a grid of covers has one line for
        // it; the count and running time go on the banner when the album
        // is opened.
        detail: a.year.clone().unwrap_or_default(),
        cover_url: a.cover.clone(),
        round: false,
        target: Some(Target::Album(a.id)),
        ..Default::default()
    }
}

/// The card for an artist: a round avatar with no subtitle.
pub fn artist_card(a: &crate::library::Artist) -> Card {
    Card {
        title: a.name.clone(),
        cover_url: a.picture.clone(),
        round: true,
        target: Some(Target::Artist(a.id)),
        ..Default::default()
    }
}

/// The card for a playlist, with its count and running time.
pub fn playlist_card(p: &crate::library::Playlist) -> Card {
    Card {
        title: p.title.clone(),
        subtitle: p.creator.clone(),
        detail: collection_detail(p.track_count, p.duration),
        cover_url: p.cover.clone(),
        round: false,
        target: Some(Target::Playlist(p.uuid.clone())),
        ..Default::default()
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
        self.selected = self.selected.saturating_add(1).min(len - 1);
        self.scroll_into_view(visible);
    }

    pub fn previous(&mut self, visible: usize) {
        // Saturating: wrapping left `selected` at the top of a usize, and
        // the next move right added to it and panicked. The first card is
        // as far left as this goes.
        self.selected = self.selected.saturating_sub(1);
        self.scroll_into_view(visible);
    }

    /// Pull `offset` just far enough that `selected` is on screen.
    fn scroll_into_view(&mut self, visible: usize) {
        let visible = visible.max(1);
        if self.selected < self.offset {
            self.offset = self.selected;
        // Saturating, because both of these are indices the caller hands
        // in: a row that shrank under a selection near the end of it made
        // this add past the top of a usize and panic.
        } else if self.selected >= self.offset.saturating_add(visible) {
            self.offset = self.selected.saturating_add(1).saturating_sub(visible);
        }
    }
}

/// How many whole cards fit in `width`.
pub fn visible_cards(width: u16) -> usize {
    if width < MIN_PARTIAL {
        return 0;
    }
    // n cards need n*card_width() + (n-1)*GAP columns.
    let whole = ((width + GAP) / (card_width() + GAP)) as usize;
    // The card at the edge is cut rather than dropped, so it is drawn and
    // the keys must be able to reach it. Below `MIN_PARTIAL` the renderer
    // stops, so this stops too.
    let used = whole as u16 * (card_width() + GAP);
    let left = width.saturating_sub(used);
    (whole + usize::from(left >= MIN_PARTIAL)).max(1)
}

/// Everything one row needs to draw itself, so the call does not take eight
/// loose arguments.
pub struct Row<'a> {
    pub heading: &'a str,
    pub cards: &'a [Card],
    pub state: &'a CarouselState,
    pub focused: bool,
    /// Whether to offer "see all" whatever fits. A home row offers it only
    /// when cards run past the edge — the key would otherwise show the same
    /// cards again — but an artist's sections are the top few of a longer
    /// list, so there is always more behind them.
    pub always_more: bool,
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
    let Row { heading, cards, state, focused, always_more } = row_spec;
    if area.height < 2 || area.width == 0 {
        return;
    }

    // "See all" only when there is something behind the edge: a row whose
    // cards all fit points at a key that would show the same cards again.
    // The key stays bound either way — it costs nothing and a row can grow
    // between one draw and the next.
    let overflows = always_more || cards.len() > visible_cards(area.width);
    render_heading(frame, Rect { height: 1, ..area }, palette, heading, focused, overflows);

    // Two rows under the heading rather than one: the blank between them is
    // where a selected card's shade reaches, so it can mark the top of the
    // cover without covering the heading.
    let row = Rect {
        x: area.x,
        y: area.y + 2,
        width: area.width,
        height: area.height.saturating_sub(2),
    };
    if row.height == 0 {
        return;
    }

    let mut x = row.x;
    let right = row.x + row.width;
    for (i, card) in cards.iter().enumerate().skip(state.offset) {
        // The card at the edge shows as much of itself as fits and is cut
        // there, the way the web client leaves one half-scrolled. Below
        // `MIN_PARTIAL` there is nothing to see: a column or two of cover
        // is a stripe, not a picture.
        if x >= right {
            break;
        }
        let width = card_width().min(right - x);
        if width < MIN_PARTIAL {
            break;
        }
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
        x = x.saturating_add(card_width() + GAP);
    }
}

/// A collection's own line: how many tracks and how long they run.
///
/// The web client writes "5 TITRES  (41:01)" over an album and the same
/// over a playlist. The running time is what says whether a record is an
/// EP or a double album, and a count on its own does not.
pub fn collection_detail(tracks: u32, duration: Option<std::time::Duration>) -> String {
    let count = if tracks == 1 {
        "1 track".to_string()
    } else {
        format!("{tracks} tracks")
    };
    match duration {
        Some(d) if !d.is_zero() => format!("{count}  ({})", running_time(d)),
        _ => count,
    }
}

/// A running time: `41:01`, or `1:08:30` once it passes an hour.
fn running_time(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// A row's heading, with its "See all" pushed to the right.
///
/// Shared with the track grids, which draw their own heading rather than
/// going through a carousel: without this they were the one kind of row
/// with no way to see the rest of it, and the key works on them too.
pub(crate) fn render_heading(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    heading: &str,
    focused: bool,
    // Whether this section has more behind it. Search's own sections do
    // not: they are the whole of what was found, and offering to show the
    // rest would point at a key that does nothing there.
    more: bool,
) {
    let hint = if more { "‹ ›  See all" } else { "" };
    let hint_width = hint.chars().count() as u16;
    let style = if focused {
        palette.accent_text()
    } else {
        palette.title()
    };
    frame.render_widget(Paragraph::new(Line::styled(heading, style)), area);
    if !hint.is_empty() && area.width > hint_width + 2 {
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
}

/// A strip of tab names, the active one underlined as on the web.
///
/// Shared by the home page and any grid that has tabs, so a tab strip
/// reads the same wherever it is.
pub fn render_tabs(frame: &mut Frame, area: Rect, palette: &Palette, tabs: &[&str], active: usize) {
    use ratatui::style::Modifier;
    use ratatui::text::Span;

    if area.width == 0 || area.height == 0 {
        return;
    }
    let mut spans: Vec<Span> = Vec::new();
    for (i, tab) in tabs.iter().enumerate() {
        let style = if i == active {
            palette.title().add_modifier(Modifier::UNDERLINED)
        } else {
            palette.subtitle()
        };
        spans.push(Span::styled(*tab, style));
        spans.push(Span::raw("   "));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect { height: 1, ..area },
    );
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
    // A selected card is shaded behind, the way a hovered tile is on the
    // web: the whole card is what enter opens, so the whole card is what is
    // marked. Drawn first, so the cover and its text sit on top.
    //
    // A column either side and nothing above or below. A terminal cell is
    // about 19x30 pixels, so a row of margin is half again as thick as a
    // column — and the grid has no half rows, so the choice is a row of 30
    // against a column of 19, or none at all. None is the closer match.
    //
    // The column fits the three-column gutter between cards, so the artwork
    // does not shrink when it is selected.
    if selected {
        let shade = Rect {
            x: area.x.saturating_sub(1),
            y: area.y,
            width: area.width + 2,
            height: area.height,
        };
        frame.render_widget(
            Block::default().style(Style::default().bg(palette.selection)),
            shade,
        );
    }

    // The cover keeps its full height and whatever runs past the pane's
    // edge is simply cut, text included. A row at the fold shows as much of
    // itself as fits, the way the web client leaves one half-scrolled —
    // and selecting it scrolls it into view whole.
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

    // The title keeps its own colour: the shade behind it is the mark, and
    // tinting the text as well is two marks for one selection.
    let text_style = palette.title();

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
        // Clamped, though the radius is bounded so it never has to be:
        // `cx + 0.5` is the largest it can get, which lands exactly on the
        // last column. Checked across every size from 1x1 to 120x60 — the
        // clamp is there so a change to the radius cannot paint over the
        // card beside this one.
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

    /// The disc drawn on its own, for a test that cares about its shape.
    fn disc(width: u16, height: u16, initial: Option<char>) -> ratatui::buffer::Buffer {
        crate::shell::geometry::draw(width, height, move |f, area, p| {
            render_disc(f, area, p, initial)
        })
    }

    #[test]
    fn the_disc_stays_inside_the_area_it_is_given() {
        // Nothing exercised this at all, and it stands in for every artist
        // without a photo — which on this account is most of them.
        for (w, h) in [(4u16, 3u16), (13, 8), (20, 10), (40, 20), (3, 9)] {
            // A pane wider than the disc, so anything painted past its
            // right-hand edge shows up.
            let buf = crate::shell::geometry::draw(w + 6, h, move |f, area, p| {
                render_disc(f, Rect { width: w, ..area }, p, None)
            });
            for y in 0..h {
                for x in w..(w + 6) {
                    assert_eq!(
                        buf[(x, y)].bg,
                        ratatui::style::Color::Reset,
                        "at {w}x{h}, the disc painted column {x} outside its area"
                    );
                }
            }
        }
    }

    #[test]
    fn the_disc_reaches_the_edges_of_its_area() {
        // The other half of staying inside: a disc that stopped short would
        // pass the bounds check and look like a dot. The middle row spans
        // nearly the whole width, and the middle column nearly the height.
        let palette = Palette::detect();
        let buf = disc(20, 10, None);
        let widest = (0..20u16)
            .filter(|x| buf[(*x, 5)].bg == palette.placeholder)
            .count();
        assert!(
            widest >= 18,
            "the middle row spans the disc, got {widest} of 20"
        );
        let tallest = (0..10u16)
            .filter(|y| buf[(10, *y)].bg == palette.placeholder)
            .count();
        assert!(
            tallest >= 6,
            "and the middle column, got {tallest} of 10"
        );
    }

    #[test]
    fn the_disc_is_round_rather_than_a_rectangle() {
        // The row offset is doubled before the radius test, since a cell is
        // about twice as tall as it is wide; without it the shape comes out
        // a tall ellipse, and without the test at all a filled rectangle
        // would pass just as well.
        let palette = Palette::detect();
        let buf = disc(20, 10, None);
        let painted = |y: u16| {
            (0..20u16)
                .filter(|x| buf[(*x, y)].bg == palette.placeholder)
                .count()
        };
        let middle = painted(5);
        let top = painted(0);
        assert!(middle > 0, "the middle of the disc is painted");
        assert!(
            top < middle,
            "and the top is narrower than the middle: {top} against {middle}"
        );
    }

    #[test]
    fn a_disc_too_small_for_a_letter_does_not_get_one() {
        // A character on a three-cell disc covers the shape that says
        // "artist", which is the whole point of drawing it.
        let big = crate::shell::geometry::text(&disc(13, 8, Some('K')));
        assert!(big.contains('K'), "a disc with room shows the initial:\n{big}");

        // Either side of the line, since the line itself is the rule: at
        // three the letter fits, at two it covers the shape that says
        // "artist".
        let at_the_line = crate::shell::geometry::text(&disc(3, 3, Some('K')));
        assert!(
            at_the_line.contains('K'),
            "three cells is room enough:\n{at_the_line}"
        );
        let small = crate::shell::geometry::text(&disc(2, 2, Some('K')));
        assert!(
            !small.contains('K'),
            "and two is not:\n{small}"
        );
    }

    #[test]
    fn walking_off_either_end_does_not_overflow() {
        // `previous` wrapped, so at the first card `selected` became the
        // top of a usize — and the next move right added to it and
        // panicked. A row is walked to both ends more often than not.
        let mut state = CarouselState::default();
        for _ in 0..5 {
            state.previous(4);
        }
        assert_eq!(state.selected, 0, "the first card is as far left as it goes");
        assert_eq!(state.offset, 0);

        for _ in 0..20 {
            state.next(6, 4);
        }
        assert_eq!(state.selected, 5, "and the last is as far right");

        // And from a selection out past the end, which a shrinking row
        // leaves behind.
        let mut state = CarouselState { offset: usize::MAX - 1, selected: usize::MAX };
        state.next(6, 4);
        state.previous(4);
        assert!(state.selected <= 6, "back inside the row it is in");
    }

    #[test]
    fn a_record_reads_the_same_in_every_view() {
        // The album, artist and playlist cards were written out three times
        // — the collection grid, search, and an artist's page — and had
        // already drifted: a playlist showed its running time in one place
        // and only its track count in another.
        let playlist = crate::library::Playlist {
            uuid: "u".into(),
            title: "A Playlist".into(),
            track_count: 5,
            duration: Some(std::time::Duration::from_secs(41 * 60 + 1)),
            creator: "Someone".into(),
            cover: None,
        };
        let card = playlist_card(&playlist);
        assert!(
            card.detail.contains("41:01"),
            "a playlist carries its running time, not just a count: {:?}",
            card.detail
        );
        assert!(card.detail.contains('5'), "and the count: {:?}", card.detail);

        // An artist is round and unsubtitled; an album is neither.
        let artist = crate::library::Artist {
            id: 1,
            name: "An Artist".into(),
            picture: None,
        };
        assert!(artist_card(&artist).round, "an artist is a round avatar");
        assert!(artist_card(&artist).subtitle.is_empty());

        let album = crate::library::Album {
            id: 2,
            title: "An Album".into(),
            artist: "An Artist".into(),
            year: Some("2001".into()),
            cover: None,
            track_count: 10,
            duration: None,
        };
        let card = album_card(&album);
        assert!(!card.round, "an album is a square cover");
        assert_eq!(card.subtitle, "An Artist");
        assert_eq!(card.detail, "2001", "the year, which is what fits a grid");
    }

    #[test]
    fn every_view_builds_its_cards_from_the_one_place() {
        // The guard on the above: a fourth copy would pass every test here
        // and drift on its own. Nothing outside this module may name the
        // targets these builders set.
        for module in [
            include_str!("mod.rs"),
            include_str!("searchview.rs"),
            include_str!("artistview.rs"),
        ] {
            for target in ["Target::Album(a.id)", "Target::Artist(a.id)"] {
                assert!(
                    !module.contains(target),
                    "a card is built outside carousel: {target}"
                );
            }
        }
    }
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
    fn a_collections_line_carries_its_count_and_running_time() {
        // "5 tracks  (41:01)", as the web client writes it. A count alone
        // does not say whether a record is an EP or a double album.
        use std::time::Duration;
        assert_eq!(
            collection_detail(5, Some(Duration::from_secs(2461))),
            "5 tracks  (41:01)"
        );
        assert_eq!(collection_detail(1, Some(Duration::from_secs(90))), "1 track  (1:30)");
        assert_eq!(
            collection_detail(200, Some(Duration::from_secs(4110))),
            "200 tracks  (1:08:30)",
            "past an hour it grows an hours field"
        );
    }

    #[test]
    fn a_collection_with_no_running_time_says_only_its_count() {
        // Some responses carry no duration; "(0:00)" would be a lie.
        assert_eq!(collection_detail(12, None), "12 tracks");
        assert_eq!(
            collection_detail(12, Some(std::time::Duration::ZERO)),
            "12 tracks"
        );
    }

    #[test]
    fn every_artwork_scales_from_the_same_measurement() {
        // A card's cover, a track row's thumbnail and a track grid's cell
        // all had their own fixed pair, each assuming the same 1:2 cell.
        // They are one rule now, so a terminal that is not 1:2 does not
        // leave three different kinds of gap.
        let card = card_width();
        assert_eq!(square_width(COVER_HEIGHT), card, "a full cover is the card");
        assert!(square_width(3) < card, "a three-row thumbnail is smaller");
        assert!(square_width(1) >= 1, "and never zero columns");

        // At other card widths, where a fixed two-columns-per-row no
        // longer coincides with the proportion.
        assert_eq!(square_width_of(3, 13), 5, "a 13-column card");
        assert_eq!(square_width_of(3, 20), 8, "a 20-column one");
        assert_eq!(square_width_of(8, 13), 13, "a full cover is the card");
        assert_eq!(square_width_of(1, 8), 1, "and never zero");
    }

    #[test]
    fn a_card_is_as_wide_as_its_cover_is_tall() {
        // A cover is square in pixels. Sixteen columns is right only when a
        // cell is exactly twice as tall as it is wide; anywhere else the
        // cover fills part of its card and the rest is empty, always on the
        // same side.
        assert_eq!(columns_for_cell(7, 14), 16, "the classic 1:2 cell");
        assert_eq!(columns_for_cell(10, 20), 16, "and any other 1:2");

        // Ratios that are not 1:2 give a different card.
        assert_eq!(columns_for_cell(19, 30), 13, "8 rows of 30px is 240px wide");
        assert_eq!(columns_for_cell(8, 20), 20, "a tall narrow cell needs more");
        assert_eq!(columns_for_cell(12, 18), 12, "a squat one needs fewer");
    }

    #[test]
    fn an_absurd_cell_size_gives_a_wrong_card_rather_than_a_broken_one() {
        // A terminal that reports nonsense should not produce a card of
        // zero columns or one that fills the pane.
        assert_eq!(columns_for_cell(0, 20), 16, "no width at all falls back");
        assert!(columns_for_cell(1, 200) <= 40, "and an extreme ratio is capped");
        assert!(columns_for_cell(200, 1) >= 8, "as is the other extreme");
    }

    #[test]
    fn the_card_at_the_edge_is_cut_rather_than_dropped() {
        // It used to be left out entirely, which ended the row in a band of
        // empty pane. The cut is what says the row carries on — the web
        // client leaves one half-scrolled.
        let all: Vec<Card> = (0..10)
            .map(|i| {
                let mut c = Card::new(format!("Card {i}"), "An Artist");
                // A cover, or `draw_cover` is never asked and the widths
                // this checks are never recorded.
                c.cover_url = Some("http://x".into());
                c
            })
            .collect();
        let state = CarouselState::default();

        // Room for two whole cards and half of a third.
        let width = card_width() * 2 + GAP * 2 + card_width() / 2;
        let cards = all.clone();
        let seen = std::cell::RefCell::new(Vec::<u16>::new());
        let buf = crate::shell::geometry::draw(width, CARD_HEIGHT + 2, |f, area, p| {
            render(
                f,
                area,
                p,
                Row {
                    heading: "Row",
                    cards: &cards,
                    state: &state,
                    focused: false,
                    always_more: false,
                },
                |_f, a, _url, _shape| {
                    seen.borrow_mut().push(a.width);
                    false
                },
            )
        });
        let text = crate::shell::geometry::text(&buf);

        assert!(text.contains("Card 0"), "the whole cards are drawn:\n{text}");
        assert!(text.contains("Card 1"));

        let widths = seen.borrow();
        assert_eq!(widths.len(), 3, "three cards drew a cover: {widths:?}");
        assert!(
            widths[2] < card_width() && widths[2] >= MIN_PARTIAL,
            "the third is cut to what is left: {widths:?}"
        );
    }

    #[test]
    fn a_sliver_of_a_card_is_not_drawn_at_all() {
        // The cut is only worth it while there is a picture to see. Two
        // columns of cover is a stripe, and the title under it would be one
        // letter and an ellipsis.
        let all: Vec<Card> = (0..10)
            .map(|i| {
                let mut c = Card::new(format!("Card {i}"), "An Artist");
                c.cover_url = Some("http://x".into());
                c
            })
            .collect();
        let state = CarouselState::default();

        let width = card_width() * 2 + GAP * 2 + (MIN_PARTIAL - 1);
        let cards = all.clone();
        let seen = std::cell::RefCell::new(Vec::<u16>::new());
        let _ = crate::shell::geometry::draw(width, CARD_HEIGHT + 2, |f, area, p| {
            render(
                f,
                area,
                p,
                Row {
                    heading: "Row",
                    cards: &cards,
                    state: &state,
                    focused: false,
                    always_more: false,
                },
                |_f, a, _url, _shape| {
                    seen.borrow_mut().push(a.width);
                    false
                },
            )
        });
        assert_eq!(seen.borrow().len(), 2, "the sliver is left out entirely");
    }

    #[test]
    fn the_number_drawn_matches_what_visible_cards_promises() {
        // The renderer and the movement keys read the same count; if they
        // disagree the selection walks onto a card that was never drawn.
        for width in [40u16, 60, 80, 100, 120, 137, 200] {
            let n = visible_cards(width);
            let all: Vec<Card> = (0..20)
                .map(|i| Card::new(format!("Card {i}"), "An Artist"))
                .collect();
            let state = CarouselState::default();
            let cards = all.clone();
            let buf =
                crate::shell::geometry::draw(width, CARD_HEIGHT + 2, move |f, area, p| {
                    render(
                        f,
                        area,
                        p,
                        Row {
                            heading: "Row",
                            cards: &cards,
                            state: &state,
                            focused: false,
                            always_more: false,
                        },
                        |_, _, _, _| false,
                    )
                });
            let text = crate::shell::geometry::text(&buf);
            let drawn = (0..20)
                .filter(|i| text.contains(&format!("Card {i}")))
                .count();
            // The card at the edge is cut, so its title is the first thing
            // to go. What must hold is that no card the count promises is
            // missing altogether — the selection would walk onto one that
            // was never drawn.
            let whole = n.saturating_sub(1);
            assert!(
                drawn >= whole && drawn <= n,
                "at width {width}: drew {drawn}, visible_cards said {n}\n{text}"
            );
        }
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
        // three need 54. Past each of those the next card is cut rather
        // than dropped, so it counts as soon as `MIN_PARTIAL` of it fits.
        assert_eq!(visible_cards(16), 1);
        assert_eq!(visible_cards(18), 1, "two columns of the next is a stripe");
        assert_eq!(visible_cards(23), 2, "four of it is a picture");
        assert_eq!(visible_cards(35), 2);
        assert_eq!(visible_cards(54), 3);
    }

    #[test]
    fn a_pane_too_narrow_for_any_of_a_card_shows_none() {
        // A cut card is still a card, so the floor is `MIN_PARTIAL` rather
        // than a whole one.
        assert_eq!(visible_cards(MIN_PARTIAL), 1);
        assert_eq!(visible_cards(MIN_PARTIAL - 1), 0);
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
    fn the_selected_card_is_shaded_behind_all_of_it() {
        // The whole card is what enter opens, so the whole card is marked —
        // cover, title and subtitle, the way a hovered tile is on the web.
        // Marking the title alone said the title was picked.
        let palette = Palette::detect();
        let cards = vec![Card::new("First", "A"), Card::new("Second", "B")];
        let state = CarouselState { offset: 0, selected: 1 };
        let buf = crate::shell::geometry::draw(48, 14, move |f, area, p| {
            render(
                f,
                area,
                p,
                Row { heading: "Row", cards: &cards, state: &state, focused: true, always_more: false },
                |_, _, _, _| false,
            )
        });

        let second = crate::shell::geometry::find(&buf, "Second").expect("the card");
        let first = crate::shell::geometry::find(&buf, "First").expect("the other");

        // Its title's row, its cover's rows, and the row under it.
        for y in [second.row - 4, second.row, second.row + 1] {
            assert_eq!(
                buf[(second.start, y)].bg,
                palette.selection,
                "row {y} of the selected card is shaded\n{}",
                crate::shell::geometry::text(&buf)
            );
        }

        // And the card beside it is not.
        assert_ne!(
            buf[(first.start, first.row)].bg,
            palette.selection,
            "the unselected card is left alone"
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
                    Row { heading: "Row", cards: &cards, state: &state, focused: true, always_more: false },
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
            ellipsis < card_width() as usize,
            "the ellipsis is inside the first card"
        );
        assert_eq!(
            cells[card_width() as usize - 1],
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
                    Row { heading: "Row", cards: &cards, state: &state, focused: true, always_more: false },
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
    fn a_title_that_exactly_fits_is_left_alone() {
        // The line between whole and cut, which nothing walked: a title the
        // width of its column would otherwise lose its last character to an
        // ellipsis that says nothing was lost.
        assert_eq!(truncate("abcde", 5), "abcde", "exactly the width");
        assert_eq!(truncate("abcdef", 5), "abcd…", "one over it");
        assert_eq!(truncate("abcd", 5), "abcd", "and one under");
    }

    #[test]
    fn truncation_counts_characters_rather_than_bytes() {
        // Album titles carry accents and worse; counting bytes would cut
        // "Café" at three and leave half a character behind.
        assert_eq!(truncate("Café", 4), "Café", "four characters, eight bytes");
        assert_eq!(truncate("Café Society", 5), "Café…");
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
                        always_more: false,
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
                    Row { heading: "H", cards: &cards, state: &state, focused: false, always_more: false },
                    |_, _, _, _| false,
                );
            })
            .unwrap();
    }
}
