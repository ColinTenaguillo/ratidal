//! A horizontal row of cover cards, as the web client shows them.
//!
//! Nothing upstream does this: ratatui's own RFC reached no consensus and
//! `tui-scrollview` was archived without horizontal scrolling. The maintainers'
//! guidance is offset windowing at the data layer — compute which cards are
//! visible and render only those, rather than drawing everything into an
//! oversized buffer and cropping.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
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
    /// The day this happened, counted from 1970-01-01. Only the feed sets
    /// it, which is the one view that groups by when rather than by what.
    pub day: Option<i64>,
}

impl Card {
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: subtitle.into(),
            ..Default::default()
        }
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

/// Whether these cards are page links rather than things with artwork.
///
/// Explore's genres, moods and decades carry no image of any kind, so drawn
/// as covers they are a grid of empty grey squares. Read off the target
/// rather than off a missing cover: an album whose artwork failed to load
/// is still an album, and gets its placeholder.
pub fn are_links(cards: &[&Card]) -> bool {
    !cards.is_empty()
        && cards
            .iter()
            .all(|c| matches!(c.target, Some(Target::Page(_))))
}

/// A pill's height: the title's own line and nothing else.
pub const PILL_HEIGHT: u16 = 1;

/// Columns between one pill and the next.
const PILL_GAP: u16 = 2;

/// The columns a pill takes: its title, plus a space either side inside
/// the rounded ends.
fn pill_width(title: &str) -> u16 {
    title.chars().count() as u16 + 4
}

/// Draw pills across as many lines as the area allows, wrapping at its
/// width -- what "see all" on a row of page links opens.
///
/// The row itself draws one line of them and cuts the rest; this is the
/// whole set, so it wraps instead.
pub fn render_pills_wrapped(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    cards: &[&Card],
    selected: Option<usize>,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    const LINE_GAP: u16 = 1;
    let (mut x, mut y) = (area.x, area.y);
    for (i, card) in cards.iter().enumerate() {
        let w = pill_width(&card.title);
        // Wrap when this one would run past the edge. A pill wider than the
        // pane is drawn anyway, clipped, rather than dropped in silence.
        if x > area.x && x + w > area.x + area.width {
            x = area.x;
            y += PILL_HEIGHT + LINE_GAP;
        }
        if y >= area.y + area.height {
            break;
        }
        render_pill(
            frame,
            Rect {
                x,
                y,
                width: w.min(area.x + area.width - x),
                height: 1,
            },
            palette,
            &card.title,
            selected == Some(i),
        );
        x += w + PILL_GAP;
    }
}

/// One pill: a rounded shape holding a title.
fn render_pill(frame: &mut Frame, area: Rect, palette: &Palette, title: &str, selected: bool) {
    let (bg, fg) = if selected {
        (palette.selection, palette.title())
    } else {
        (palette.surface, palette.subtitle())
    };
    frame.render_widget(
        Block::default().style(Style::default().bg(bg)),
        Rect {
            x: area.x + 1,
            width: area.width.saturating_sub(2),
            ..area
        },
    );
    // The ends, drawn as half blocks inked in the pill's own colour so it
    // reads as one rounded shape. Half blocks rather than the powerline
    // arrows a pill is usually built from: those need a patched font, and a
    // terminal without one draws a blank box.
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("\u{258c}", Style::default().fg(bg)),
            Span::styled(format!(" {title} "), fg.bg(bg)),
            Span::styled("\u{2590}", Style::default().fg(bg)),
        ])),
        area,
    );
}

/// How many pills fit in `width`, counting the gaps between them.
pub fn visible_pills(cards: &[Card], width: u16) -> usize {
    let mut used = 0u16;
    let mut n = 0;
    for card in cards {
        let w = pill_width(&card.title);
        let next = if n == 0 { w } else { used + PILL_GAP + w };
        if next > width {
            break;
        }
        used = next;
        n += 1;
    }
    n.max(1)
}

/// Draw a row of page links as the web client does: rounded pills holding
/// a title, with no artwork.
///
/// Explore's genres, moods and decades carry no image of any kind -- their
/// `imageId` is a name like "hiphop" rather than a uuid, and nothing is
/// served for it. Drawn as covers they were a row of empty grey squares.
pub fn render_pills(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    cards: &[Card],
    selected: Option<usize>,
    offset: usize,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let mut x = area.x;
    for (i, card) in cards.iter().enumerate().skip(offset) {
        let w = pill_width(&card.title);
        if x + w > area.x + area.width {
            break;
        }
        let is_selected = selected == Some(i);
        let (bg, fg) = if is_selected {
            (palette.selection, palette.title())
        } else {
            (palette.surface, palette.subtitle())
        };
        frame.render_widget(
            Block::default().style(Style::default().bg(bg)),
            Rect {
                x: x + 1,
                y: area.y,
                width: w.saturating_sub(2),
                height: 1,
            },
        );
        // The ends, drawn as half blocks inked in the pill's own colour so
        // it reads as one rounded shape. Half blocks rather than the
        // powerline arrows a pill is usually built from: those need a
        // patched font, and a terminal without one draws a blank box.
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("\u{258c}", Style::default().fg(bg)),
                Span::styled(format!(" {} ", card.title), fg.bg(bg)),
                Span::styled("\u{2590}", Style::default().fg(bg)),
            ])),
            Rect {
                x,
                y: area.y,
                width: w,
                height: 1,
            },
        );
        x += w + PILL_GAP;
    }
}

pub fn visible_cards(width: u16) -> usize {
    if width == 0 {
        return 0;
    }
    // n cards need n*card_width() + (n-1)*GAP columns.
    let whole = ((width + GAP) / (card_width() + GAP)) as usize;
    // The card at the edge is cut rather than dropped, so it is drawn and
    // the keys must be able to reach it. Any column of it counts: the
    // row's edge cuts the way the pane's bottom does, with no floor.
    let used = whole as u16 * (card_width() + GAP);
    let left = width.saturating_sub(used);
    (whole + usize::from(left > 0)).max(1)
}

/// Everything one row needs to draw itself, so the call does not take eight
/// loose arguments.
/// Whether the account has what a card opens: the heart on a favourited
/// album, playlist, mix or artist. Asked per card at draw time, so the
/// mark follows the account rather than the moment the card was built.
pub type Liked<'a> = &'a dyn Fn(&Target) -> bool;

/// A `Liked` that likes nothing, for views without an account behind them.
pub fn nobody(_: &Target) -> bool {
    false
}

pub struct Row<'a> {
    pub heading: &'a str,
    pub liked: Liked<'a>,
    pub cards: &'a [Card],
    pub state: &'a CarouselState,
    pub focused: bool,
    /// Whether to offer "see all" whatever fits. A home row offers it only
    /// when cards run past the edge — the key would otherwise show the same
    /// cards again — but an artist's sections are the top few of a longer
    /// list, so there is always more behind them.
    pub always_more: bool,
}

/// Whether a section has anything behind what it draws, and so whether the
/// heading offers "See all".
///
/// One question, asked the same way everywhere a heading is drawn. It used
/// to be worked out at each call site, and the answers drifted apart from
/// what the key actually does: a home row of nine tracks fits in its grid,
/// so the hint stayed hidden, while `o` fetched the rest from the path the
/// API handed back. The hint said one thing and the key did another.
///
/// `behind` is that path, present when the API says there is more to fetch.
/// `drawn` is how many of `count` the view can show at this size.
pub fn has_more(count: usize, drawn: usize, behind: bool) -> bool {
    behind || count > drawn
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
    let Row {
        heading,
        cards,
        state,
        focused,
        always_more,
        liked,
    } = row_spec;
    if area.height == 0 || area.width == 0 {
        return;
    }

    // "See all" only when there is something behind the edge: a row whose
    // cards all fit points at a key that would show the same cards again.
    // The key stays bound either way — it costs nothing and a row can grow
    // between one draw and the next.
    let overflows = has_more(cards.len(), visible_cards(area.width), always_more);
    render_heading(
        frame,
        Rect { height: 1, ..area },
        palette,
        heading,
        focused,
        overflows,
        true,
    );

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
        // there, the way the web client leaves one half-scrolled.
        if x >= right {
            break;
        }
        let width = card_width().min(right - x);
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
            card.target.as_ref().is_some_and(liked),
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
    // Whether the row scrolls sideways, which is what the arrows mean. A
    // grid does not: its rest is behind "See all" alone, and arrows over
    // it pointed at keys that step through cells.
    scrolls: bool,
) {
    let hint = match (more, scrolls) {
        (false, _) => "",
        (true, true) => "‹ ›  See all",
        (true, false) => "See all",
    };
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
    liked: bool,
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

    // A row at the fold shows as much of itself as fits, the way the web
    // client leaves one half-scrolled -- and selecting it scrolls it into
    // view whole. The cover takes the room and the label is what falls off:
    // a name drawn over a squeezed circle read as the text cutting into the
    // artwork, which is what the Profiles page showed along its bottom row.
    let cover_height = COVER_HEIGHT.min(area.height);
    let cover = Rect {
        height: cover_height,
        ..area
    };

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

    // The heart a favourite track carries, after the title: what `F` did
    // has to show somewhere, and the title line is the one every card has.
    let mark = if liked {
        format!(" {}", super::icons::favourite())
    } else {
        String::new()
    };
    let title_line = |width: u16| {
        let room = width.saturating_sub(mark.chars().count() as u16);
        Line::from(vec![
            Span::styled(truncate(&card.title, room), text_style),
            Span::styled(mark.clone(), palette.mark()),
        ])
    };

    // A round card centres its single label under the avatar; a square one
    // left-aligns a title and its subtitles.
    if card.round {
        if y < bottom {
            frame.render_widget(
                Paragraph::new(title_line(area.width))
                    .alignment(ratatui::layout::Alignment::Center),
                Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: 1,
                },
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
            Paragraph::new(title_line(text_w)),
            Rect {
                x: area.x,
                y,
                width: text_w,
                height: 1,
            },
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
            Rect {
                x: area.x,
                y,
                width: text_w,
                height: 1,
            },
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
/// Drawn with the stencil the half-block covers are cut with, so the disc
/// and the photo that replaces it have exactly the same outline.
/// A 5x7 bitmap of the capitals and the digits: one byte per row, top to
/// bottom, the low five bits its pixels left to right.
const FONT: [(char, [u8; 7]); 36] = [
    (
        'A',
        [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'B',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
    ),
    (
        'C',
        [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
    ),
    (
        'D',
        [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
    ),
    (
        'E',
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
    ),
    (
        'F',
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
    ),
    (
        'G',
        [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
    ),
    (
        'H',
        [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'I',
        [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
    ),
    (
        'J',
        [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
    ),
    (
        'K',
        [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
    ),
    (
        'L',
        [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
    ),
    (
        'M',
        [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'N',
        [
            0b10001, 0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001,
        ],
    ),
    (
        'O',
        [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        'P',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
    ),
    (
        'Q',
        [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
    ),
    (
        'R',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
    ),
    (
        'S',
        [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
    ),
    (
        'T',
        [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        'U',
        [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        'V',
        [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
    ),
    (
        'W',
        [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
    ),
    (
        'X',
        [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
    ),
    (
        'Y',
        [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        'Z',
        [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
    ),
    (
        '0',
        [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
    ),
    (
        '1',
        [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
    ),
    (
        '2',
        [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
    ),
    (
        '3',
        [
            0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110,
        ],
    ),
    (
        '4',
        [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
    ),
    (
        '5',
        [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
    ),
    (
        '6',
        [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        '7',
        [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
    ),
    (
        '8',
        [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        '9',
        [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
    ),
];

/// The block glyph whose inked quarters are `[top-left, top-right,
/// bottom-left, bottom-right]`.
fn quadrant(q: [bool; 4]) -> &'static str {
    match q {
        [false, false, false, false] => " ",
        [true, false, false, false] => "▘",
        [false, true, false, false] => "▝",
        [true, true, false, false] => "▀",
        [false, false, true, false] => "▖",
        [true, false, true, false] => "▌",
        [false, true, true, false] => "▞",
        [true, true, true, false] => "▛",
        [false, false, false, true] => "▗",
        [true, false, false, true] => "▚",
        [false, true, false, true] => "▐",
        [true, true, false, true] => "▜",
        [false, false, true, true] => "▄",
        [true, false, true, true] => "▙",
        [false, true, true, true] => "▟",
        [true, true, true, true] => "█",
    }
}

/// The first and last of `n` frame positions the glyph inks, by column
/// (`lit(row, column)`) or by row (`lit(row, _)`).
fn ink_span(rows: &[u8; 7], lit: impl Fn(&u8, u16) -> bool, n: u16) -> (u16, u16) {
    let mut lo = n;
    let mut hi = 0;
    for r in rows {
        for i in 0..n {
            if lit(r, i) {
                lo = lo.min(i);
                hi = hi.max(i);
            }
        }
    }
    (lo, hi)
}

/// Where the frame starts so that the ink between frame positions `lo` and
/// `hi`, scaled by `k`, sits centred in `extent` pixels -- and half a
/// pixel to the right or below when it cannot sit dead centre.
fn ink_origin(extent: u16, k: u16, (lo, hi): (u16, u16)) -> u16 {
    // Ink spans `k * (hi - lo + 1)` pixels from `k * lo` past the origin.
    let ink = k * (hi - lo + 1);
    let free = extent.saturating_sub(ink);
    (free.div_ceil(2)).saturating_sub(k * lo)
}

/// The bitmap of a capital or digit; accented capitals fall back to the
/// terminal's glyph.
fn glyph(c: char) -> Option<&'static [u8; 7]> {
    FONT.iter().find(|(g, _)| *g == c).map(|(_, rows)| rows)
}

pub(super) fn render_disc(frame: &mut Frame, area: Rect, palette: &Palette, initial: Option<char>) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    // Half blocks, the same stencil as a half-block photo: a cell is two
    // pixels, and each is in or out of the circle on its own. The initial
    // is drawn in the same pixels, from a bitmap: a terminal glyph on a
    // block of colour was a letter in a box, and never quite centred.
    let square = ratatui::layout::Size::new(area.width, area.height);
    let (w, h2) = (area.width, area.height * 2);
    let letter = initial.and_then(glyph).and_then(|rows| {
        // About half the disc across, and never more than fits: a letter
        // that touched the edge would erase the shape that says "artist".
        let k = (h2 * 55 / 100 / 7).min(w * 55 / 100 / 5);
        if k < 1 {
            return None;
        }
        // Centred on its ink, not on its 5x7 frame: a J fills four of the
        // frame's columns and sat half a column off from an A that fills
        // five. Across, the origin is in half columns: a disc eighteen
        // wide leaves thirteen free around five columns of A, and no whole
        // column splits that -- half a one does, and the quarter blocks
        // below can draw it.
        // Down, the disc is always an even number of half rows and the
        // glyph seven: at an odd scale the ink sits a quarter row off,
        // and stays so. Doubling a row of the glyph to even it out was
        // tried twice, in the middle and at the bottom, and both read as
        // a misdrawn letter; a quarter row is not seen.
        let (x0, y0) = (
            ink_origin(
                2 * w,
                2 * k,
                ink_span(rows, |r, c| r & (1 << (4 - c)) != 0, 5),
            ),
            ink_origin(h2, k, ink_span(rows, |r, _| *r != 0, 7)),
        );
        Some((rows, k, x0, y0))
    });
    #[derive(Clone, Copy, PartialEq)]
    enum Ink {
        Out,
        Disc,
    }
    let ink = |col: u16, half: u16| -> Ink {
        let (up, lo) = super::artwork::round_stencil(square, col, half / 2);
        if !(if half.is_multiple_of(2) { up } else { lo }) {
            return Ink::Out;
        }
        Ink::Disc
    };
    // Whether the letter inks this quarter of a cell: `hcol` in half
    // columns, `half` in half rows.
    let lettered = |hcol: u16, half: u16| -> bool {
        let Some((rows, k, x0, y0)) = letter else {
            return false;
        };
        if hcol < x0 || half < y0 {
            return false;
        }
        let (px, py) = ((hcol - x0) / (2 * k), (half - y0) / k);
        px < 5 && py < 7 && rows[usize::from(py)] & (1 << (4 - px)) != 0
    };
    let colour = |_: Ink| palette.placeholder;
    let buf = frame.buffer_mut();
    for row in 0..area.height {
        for col in 0..area.width {
            let (upper, lower) = (ink(col, 2 * row), ink(col, 2 * row + 1));
            let Some(cell) = buf.cell_mut((area.x + col, area.y + row)) else {
                continue;
            };
            // The letter first: it lies inside the disc, so a cell it
            // touches is disc under it, and the two colours a cell has are
            // enough for any pattern of its four quarters.
            let quarters = [
                lettered(2 * col, 2 * row),
                lettered(2 * col + 1, 2 * row),
                lettered(2 * col, 2 * row + 1),
                lettered(2 * col + 1, 2 * row + 1),
            ];
            if quarters.iter().any(|q| *q) {
                cell.set_symbol(quadrant(quarters))
                    .set_fg(palette.text)
                    .set_bg(palette.placeholder);
                continue;
            }
            // A pixel outside the circle leaves the cell's background as
            // it was: the band under a selected card shows through.
            match (upper, lower) {
                (Ink::Out, Ink::Out) => {}
                (Ink::Out, lower) => {
                    cell.set_symbol("▄").set_fg(colour(lower));
                }
                (upper, Ink::Out) => {
                    cell.set_symbol("▀").set_fg(colour(upper));
                }
                (upper, lower) if upper == lower => {
                    cell.set_symbol("█").set_fg(colour(upper));
                }
                (upper, lower) => {
                    cell.set_symbol("▀")
                        .set_fg(colour(upper))
                        .set_bg(colour(lower));
                }
            }
        }
    }

    // A letter the font does not have, or a disc too small for one in
    // pixels: the terminal's own glyph, centred. Skipped on a disc too
    // small to hold a character without covering the shape.
    if let Some(c) = initial.filter(|_| letter.is_none()) {
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
    fn a_liked_card_carries_the_heart_after_its_title() {
        let mut liked = Card::new("Discovery", "Daft Punk");
        liked.target = Some(Target::Album(9));
        let mut other = Card::new("Homework", "Daft Punk");
        other.target = Some(Target::Album(10));
        let cards = vec![liked, other];
        let state = CarouselState::default();
        let is_nine = |t: &Target| matches!(t, Target::Album(9));
        let buf = crate::shell::geometry::draw(60, 14, |f, area, p| {
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
                    liked: &is_nine,
                },
                |_, _, _, _| false,
            )
        });
        let text = crate::shell::geometry::text(&buf);
        let heart = crate::shell::icons::favourite();
        let line = text
            .lines()
            .find(|l| l.contains("Discovery"))
            .expect("the title");
        assert!(
            line.contains(&format!("Discovery {heart}")),
            "the heart after the title:\n{text}"
        );
        assert!(
            !line.contains(&format!("Homework {heart}")),
            "and not on the other:\n{text}"
        );
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
            .filter(|x| crate::shell::geometry::is_disc(&buf, *x, 5, palette.placeholder))
            .count();
        assert!(
            widest >= 18,
            "the middle row spans the disc, got {widest} of 20"
        );
        let tallest = (0..10u16)
            .filter(|y| crate::shell::geometry::is_disc(&buf, 10, *y, palette.placeholder))
            .count();
        assert!(tallest >= 6, "and the middle column, got {tallest} of 10");
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
                .filter(|x| crate::shell::geometry::is_disc(&buf, *x, y, palette.placeholder))
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
    fn every_letter_sits_centred_on_its_ink() {
        // Measured on what is drawn: the ink's bounding box against the
        // disc's, in half-block pixels. An A fills its frame and a J does
        // not, and centring the frame put them half a column apart.
        let palette = Palette::detect();
        for (w, h) in [(27u16, 13u16), (26, 13), (13, 8)] {
            for (c, _) in FONT.iter() {
                let buf = disc(w, h, Some(*c));
                // The letter's quarters, in half columns and half rows.
                let (mut x0, mut x1, mut y0, mut y1) = (u16::MAX, 0u16, u16::MAX, 0u16);
                for y in 0..h {
                    for x in 0..w {
                        let cell = &buf[(x, y)];
                        if cell.fg != palette.text {
                            continue;
                        }
                        let q = (0..16u8)
                            .map(|b| [b & 1 != 0, b & 2 != 0, b & 4 != 0, b & 8 != 0])
                            .find(|q| quadrant(*q) == cell.symbol())
                            .unwrap_or_else(|| {
                                panic!("{c}: {:?} is not a quarter glyph", cell.symbol())
                            });
                        for (i, lit) in q.iter().enumerate() {
                            if !lit {
                                continue;
                            }
                            let (hx, hy) = (2 * x + (i as u16 % 2), 2 * y + (i as u16 / 2));
                            x0 = x0.min(hx);
                            x1 = x1.max(hx);
                            y0 = y0.min(hy);
                            y1 = y1.max(hy);
                        }
                    }
                }
                assert!(x1 >= x0, "{c} on {w}x{h}: no ink");
                // Centres, doubled to stay in integers: the ink's against
                // the disc's, which is the whole area.
                let dx = (x0 + x1) as i32 - (2 * w as i32 - 1);
                let dy = (y0 + y1) as i32 - (2 * h as i32 - 1);
                assert_eq!(
                    dx, 0,
                    "{c} on {w}x{h}: {dx}/2 half columns off centre ({x0}..{x1})"
                );
                // Down, a quarter row is allowed at an odd scale, and then
                // always below: the glyph is not redrawn to even it out.
                assert!(
                    dy == 0 || dy == 1,
                    "{c} on {w}x{h}: {dy}/2 half rows off centre ({y0}..{y1})"
                );
            }
        }
    }

    #[test]
    fn a_disc_too_small_for_a_letter_does_not_get_one() {
        // A character on a three-cell disc covers the shape that says
        // "artist", which is the whole point of drawing it.
        // With room, the initial is pixels of the text colour rather than
        // a glyph: a letter in a box on the disc, and never quite centred.
        let palette = Palette::detect();
        let big = disc(13, 8, Some('K'));
        let lit: Vec<(u16, u16)> = (0..8u16)
            .flat_map(|y| (0..13u16).map(move |x| (x, y)))
            .filter(|(x, y)| big[(*x, *y)].fg == palette.text || big[(*x, *y)].bg == palette.text)
            .collect();
        assert!(lit.len() >= 8, "the letter is drawn in pixels: {lit:?}");
        assert!(
            !crate::shell::geometry::text(&big).contains('K'),
            "and not as a glyph"
        );
        // Centred: its pixels sit around the middle column and row.
        let (xs, ys): (Vec<u16>, Vec<u16>) = lit.iter().copied().unzip();
        let (x0, x1) = (*xs.iter().min().unwrap(), *xs.iter().max().unwrap());
        let (y0, y1) = (*ys.iter().min().unwrap(), *ys.iter().max().unwrap());
        assert!(
            (x0 + x1) / 2 == 6 || (x0 + x1).div_ceil(2) == 6,
            "across: {x0}..{x1}"
        );
        assert!(
            (y0 + y1) / 2 == 3 || (y0 + y1).div_ceil(2) == 4,
            "down: {y0}..{y1}"
        );

        // Either side of the line, since the line itself is the rule: at
        // three the letter fits, at two it covers the shape that says
        // "artist".
        let at_the_line = crate::shell::geometry::text(&disc(3, 3, Some('K')));
        assert!(
            at_the_line.contains('K'),
            "three cells is room enough:\n{at_the_line}"
        );
        let small = crate::shell::geometry::text(&disc(2, 2, Some('K')));
        assert!(!small.contains('K'), "and two is not:\n{small}");
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
        assert_eq!(
            state.selected, 0,
            "the first card is as far left as it goes"
        );
        assert_eq!(state.offset, 0);

        for _ in 0..20 {
            state.next(6, 4);
        }
        assert_eq!(state.selected, 5, "and the last is as far right");

        // And from a selection out past the end, which a shrinking row
        // leaves behind.
        let mut state = CarouselState {
            offset: usize::MAX - 1,
            selected: usize::MAX,
        };
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
        assert!(
            card.detail.contains('5'),
            "and the count: {:?}",
            card.detail
        );

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
        term.draw(|f| render_disc(f, f.area(), &palette, None))
            .unwrap();
        let buf = term.backend().buffer();
        // A cell is part of the disc whether it is filled outright or drawn
        // as a half block -- the half is inked in the disc's colour, so it
        // is the foreground there rather than the background.
        (0..h)
            .map(|y| {
                (0..w)
                    .filter(|&x| {
                        let cell = &buf[(x, y)];
                        cell.bg == palette.placeholder || cell.fg == palette.placeholder
                    })
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
        assert_eq!(
            collection_detail(1, Some(Duration::from_secs(90))),
            "1 track  (1:30)"
        );
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
        assert!(
            columns_for_cell(1, 200) <= 40,
            "and an extreme ratio is capped"
        );
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
                    liked: &nobody,
                },
                |_f, a, _url, _shape| {
                    seen.borrow_mut().push(a.width);
                    false
                },
            )
        });
        let text = crate::shell::geometry::text(&buf);

        assert!(
            text.contains("Card 0"),
            "the whole cards are drawn:\n{text}"
        );
        assert!(text.contains("Card 1"));

        let widths = seen.borrow();
        assert_eq!(widths.len(), 3, "three cards drew a cover: {widths:?}");
        assert!(
            widths[2] < card_width() && widths[2] >= 1,
            "the third is cut to what is left: {widths:?}"
        );
    }

    #[test]
    fn a_sliver_of_a_card_is_still_drawn() {
        // The row's edge cuts the way the pane's bottom does: whatever is
        // left of the next card is drawn, one column of it or all but one.
        let all: Vec<Card> = (0..10)
            .map(|i| {
                let mut c = Card::new(format!("Card {i}"), "An Artist");
                c.cover_url = Some("http://x".into());
                c
            })
            .collect();
        let state = CarouselState::default();

        let width = card_width() * 2 + GAP * 2 + 1;
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
                    liked: &nobody,
                },
                |_f, a, _url, _shape| {
                    seen.borrow_mut().push(a.width);
                    false
                },
            )
        });
        let seen = seen.borrow();
        assert_eq!(seen.len(), 3, "the sliver is drawn: {seen:?}");
        assert_eq!(seen[2], 1, "at the one column left for it");
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
            let buf = crate::shell::geometry::draw(width, CARD_HEIGHT + 2, move |f, area, p| {
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
                        liked: &nobody,
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
    fn the_disc_draws_its_edge_in_half_blocks() {
        // Whole cells give the disc twice the vertical step of the
        // horizontal one, and it reads as a staircase next to the round
        // photos it stands in for. A cell holds two half blocks, the same
        // two pixels a half-block photo has, so the edge steps by half a
        // cell -- and matches the photo's edge exactly.
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let palette = Palette::detect();
        let mut term = Terminal::new(TestBackend::new(16, 8)).expect("terminal");
        term.draw(|f| render_disc(f, f.area(), &palette, None))
            .expect("draw");
        let buf = term.backend().buffer();

        // A partial glyph is neither blank nor a full block: those are the
        // cells the curve passes through.
        let partial = (0..8u16)
            .flat_map(|y| (0..16u16).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let s = buf[(*x, *y)].symbol();
                s != " " && s != "\u{2588}"
            })
            .count();
        assert!(
            partial >= 8,
            "the edge is drawn in whole cells, so it is a staircase:\n{}",
            crate::shell::geometry::text(buf)
        );

        // And the body is solid, or the disc has holes in it.
        let solid = (0..16u16)
            .filter(|x| buf[(*x, 4)].symbol() == "\u{2588}")
            .count();
        assert!(
            solid >= 12,
            "the middle of the disc is not solid, got {solid} of 16:\n{}",
            crate::shell::geometry::text(buf)
        );
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
        // width keeps growing towards the middle. It touches the sides, as
        // the half-block photo it matches does, so the middle rows are
        // full width -- but never more than half of them.
        assert!(
            widths[0] < widths[1] && widths[1] < widths[2],
            "the width keeps growing: {widths:?}"
        );
        let full = widths.iter().filter(|&&n| n == 16).count();
        assert!(
            full <= widths.len() / 2,
            "a circle flattens out for at most half the rows, got {full}"
        );
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
        // than dropped, so it counts as soon as a column of it shows.
        assert_eq!(visible_cards(16), 1);
        assert_eq!(visible_cards(19), 1, "the gutter alone is not a card");
        assert_eq!(visible_cards(20), 2, "one column of the next is");
        assert_eq!(visible_cards(35), 2);
        assert_eq!(visible_cards(54), 3);
    }

    #[test]
    fn a_pane_too_narrow_for_any_of_a_card_shows_none() {
        // A cut card is still a card, down to one column of it.
        assert_eq!(visible_cards(1), 1);
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
        let mut s = CarouselState {
            offset: 4,
            selected: 6,
        };
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
        let state = CarouselState {
            offset: 0,
            selected: 1,
        };
        let buf = crate::shell::geometry::draw(48, 14, move |f, area, p| {
            render(
                f,
                area,
                p,
                Row {
                    heading: "Row",
                    cards: &cards,
                    state: &state,
                    focused: true,
                    always_more: false,
                    liked: &nobody,
                },
                |_, _, _, _| false,
            )
        });

        let second = crate::shell::geometry::find(&buf, "Second").expect("the card");
        let first = crate::shell::geometry::find(&buf, "First").expect("the other");

        // The title's row and the subtitle under it. Not the cover's own
        // rows: a card with no artwork paints its placeholder over them,
        // and a real cover would paint pixels there -- either way the shade
        // is covered. It showed through in truecolor only because the two
        // greys round to the same 24-bit value; in 256 colours they are one
        // index apart and the difference is visible.
        for y in [second.row, second.row + 1] {
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
                    Row {
                        heading: "Row",
                        cards: &cards,
                        state: &state,
                        focused: true,
                        always_more: false,
                        liked: &nobody,
                    },
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
                    Row {
                        heading: "Row",
                        cards: &cards,
                        state: &state,
                        focused: true,
                        always_more: false,
                        liked: &nobody,
                    },
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
                        liked: &nobody,
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
                    Row {
                        heading: "H",
                        cards: &cards,
                        state: &state,
                        focused: false,
                        always_more: false,
                        liked: &nobody,
                    },
                    |_, _, _, _| false,
                );
            })
            .unwrap();
    }
}
