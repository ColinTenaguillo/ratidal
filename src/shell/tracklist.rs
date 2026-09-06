//! The Titres view: every track the user has favourited.
//!
//! Drawn row by row rather than with `Table`, because each row carries a
//! thumbnail. `Table` paints its own cells, leaving nowhere for an image
//! protocol to write, so the columns are laid out here instead.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use super::carousel::truncate;
use super::theme::Palette;
use crate::domain::Track;

/// A thumbnail is two rows tall, which is about square given a terminal
/// cell's aspect, so a row is three high with a blank line under it.
const ROW_HEIGHT: u16 = 3;
const THUMB_WIDTH: u16 = 4;
/// Rows above the list for the plain Tracks view: the heading, a blank, the
/// buttons, a blank, the filter, a blank, then the column headers and a blank.
const HEADER_ROWS: u16 = 8;

/// Rows above the list when the caller has drawn its own heading and box:
/// just the column headers and a blank line. Search reuses this view under
/// its own tabs, and a second "Tracks" heading with a second filter box
/// under the search box would be the same furniture twice.
const BARE_HEADER_ROWS: u16 = 2;

/// An opened album or playlist puts a cover and its details above all that.
/// The cover is 8 rows, which is square at a terminal cell's aspect for the
/// 16 columns it spans, plus a blank line under it.
const BANNER_ROWS: u16 = 9;

#[derive(Debug, Default)]
pub struct TrackListState {
    pub selected: usize,
    /// First visible track.
    pub offset: usize,
    pub filter: String,
}

impl TrackListState {
    /// Move down, clamped to the last row. `len` is passed in so the state
    /// need not own the data.
    pub fn next(&mut self, len: usize) {
        if len == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected + 1).min(len - 1);
    }

    pub fn previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Pull `offset` just far enough that `selected` is on screen.
    pub fn scroll_into_view(&mut self, visible: usize) {
        let visible = visible.max(1);
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + visible {
            self.offset = self.selected + 1 - visible;
        }
    }

    /// Keep the selection inside a list that may have shrunk under it.
    pub fn clamp(&mut self, len: usize) {
        if len == 0 {
            self.selected = 0;
            self.offset = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }
}

/// How many track rows fit under the header.
pub fn visible_rows(height: u16) -> usize {
    visible_rows_with(height, false)
}

pub fn visible_rows_with(height: u16, has_banner: bool) -> usize {
    visible_rows_chrome(height, has_banner, Chrome::Full)
}

pub fn visible_rows_chrome(height: u16, has_banner: bool, chrome: Chrome) -> usize {
    (height.saturating_sub(header_rows_with(has_banner, chrome)) / ROW_HEIGHT) as usize
}

/// Case-insensitive match on title, artist or album, as the web client's
/// filter box does.
pub fn filter<'a>(tracks: &'a [Track], needle: &str) -> Vec<&'a Track> {
    if needle.is_empty() {
        return tracks.iter().collect();
    }
    let needle = needle.to_lowercase();
    tracks
        .iter()
        .filter(|t| {
            t.title.to_lowercase().contains(&needle)
                || t.artist.to_lowercase().contains(&needle)
                || t.album.to_lowercase().contains(&needle)
        })
        .collect()
}

/// Column widths for a given pane width.
///
/// Title, artist and album share what is left after the fixed columns, in
/// the same proportions the web client uses. Returned rather than computed
/// inline so the header and the rows cannot drift apart.
struct Columns {
    number: u16,
    thumb: u16,
    title: u16,
    artist: u16,
    album: u16,
    added: u16,
    duration: u16,
}

fn columns(width: u16, in_collection: bool) -> Columns {
    let number = 4;
    let duration = 6;
    // Inside an album every row has the same album and no added date, so
    // both columns are a wall of repetition. Their space goes to the title.
    let added = if width > 90 && !in_collection { 12 } else { 0 };
    let fixed = number + THUMB_WIDTH + added + duration;
    let flexible = width.saturating_sub(fixed);

    // Below this there is no room for three text columns; drop the album
    // first, then the artist, rather than squeezing all three to nothing.
    let (title, artist, album) = if in_collection {
        // No album column: it is the same album on every row.
        let title = flexible * 60 / 100;
        (title, flexible - title, 0)
    } else if flexible < 30 {
        (flexible, 0, 0)
    } else if flexible < 50 {
        (flexible / 2, flexible - flexible / 2, 0)
    } else {
        let title = flexible * 40 / 100;
        let artist = flexible * 30 / 100;
        (title, artist, flexible - title - artist)
    };

    Columns { number, thumb: THUMB_WIDTH, title, artist, album, added, duration }
}

pub struct TrackList<'a> {
    pub tracks: &'a [&'a Track],
    pub state: &'a TrackListState,
    pub focused: bool,
    /// The track currently playing, marked in place of its number.
    pub playing: Option<crate::domain::TrackId>,
    /// What quality that track is playing at, which is the colour the web
    /// client tints the row with.
    pub tier: super::nowplaying::Tier,
    /// Set when the user opened a playlist or album, rather than looking at
    /// their favourites. Without it an opened album was indistinguishable
    /// from the Tracks view: same heading, same everything.
    pub banner: Option<Banner<'a>>,
    /// Whether this list draws its own heading and filter box, or the
    /// caller has already drawn them.
    pub chrome: Chrome,
}

/// The header an opened album or playlist gets: its cover and what it is.
pub struct Banner<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    pub detail: &'a str,
    pub cover: Option<&'a str>,
}

/// How much of its own chrome the list draws above the rows.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Chrome {
    /// Heading, transport buttons, filter box, column headers.
    #[default]
    Full,
    /// Column headers only: the caller has drawn the rest.
    Bare,
}

/// Rows above the track list, which depends on its chrome and its banner.
pub fn header_rows(has_banner: bool) -> u16 {
    header_rows_with(has_banner, Chrome::Full)
}

pub fn header_rows_with(has_banner: bool, chrome: Chrome) -> u16 {
    match chrome {
        Chrome::Bare => BARE_HEADER_ROWS,
        Chrome::Full if has_banner => HEADER_ROWS + BANNER_ROWS,
        Chrome::Full => HEADER_ROWS,
    }
}

pub fn render<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    list: TrackList<'_>,
    mut draw_cover: F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    let TrackList { tracks, state, focused, playing, banner, tier, chrome } = list;
    if area.width == 0 || area.height == 0 {
        return;
    }

    let bottom = area.y + area.height;

    // An opened album leads with its cover and details; the plain Tracks view
    // just names itself.
    let cols = columns(area.width, banner.is_some());
    match chrome {
        Chrome::Bare => {
            // Only the column headers; the caller drew the rest.
            frame.render_widget(
                Paragraph::new(Line::raw("")),
                Rect { y: area.y, height: 1, ..area },
            );
            render_header(frame, Rect { y: area.y, height: 1, ..area }, palette, &cols);
        }
        Chrome::Full => {
            let mut y = area.y;
            match &banner {
                Some(b) => {
                    render_banner(frame, Rect { y, ..area }, palette, b, &mut draw_cover);
                    y += BANNER_ROWS;
                }
                None => {
                    frame.render_widget(
                        Paragraph::new(Line::styled("Tracks", palette.page_heading())),
                        Rect { y, height: 1, ..area },
                    );
                }
            }
            if y + 2 < bottom {
                render_buttons(frame, Rect { y: y + 2, height: 1, ..area }, palette);
            }
            if y + 4 < bottom {
                render_filter(
                    frame,
                    Rect { y: y + 4, height: 1, ..area },
                    palette,
                    state,
                    banner.is_some(),
                );
            }
            if y + 6 < bottom {
                render_header(frame, Rect { y: y + 6, height: 1, ..area }, palette, &cols);
            }
        }
    }

    let body_y = area.y + header_rows_with(banner.is_some(), chrome);
    if body_y >= bottom {
        return;
    }
    let visible = visible_rows_chrome(area.height, banner.is_some(), chrome);

    for (i, track) in tracks.iter().enumerate().skip(state.offset).take(visible) {
        let y = body_y + (i - state.offset) as u16 * ROW_HEIGHT;
        if y >= area.y + area.height {
            break;
        }
        render_row(
            frame,
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: ROW_HEIGHT.min(area.y + area.height - y),
            },
            palette,
            &cols,
            track,
            i + 1,
            focused && i == state.selected,
            playing == Some(track.id),
            tier,
            &mut draw_cover,
        );
    }
}

/// The cover and details of an opened album or playlist.
fn render_banner<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    banner: &Banner<'_>,
    draw_cover: &mut F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    const COVER_WIDTH: u16 = 16;
    const COVER_HEIGHT: u16 = 8;

    let cover_w = COVER_WIDTH.min(area.width);
    let cover_h = COVER_HEIGHT.min(area.height);
    let cover = Rect { x: area.x, y: area.y, width: cover_w, height: cover_h };

    let drew = match banner.cover {
        Some(url) if cover.width > 0 && cover.height > 0 => {
            draw_cover(frame, cover, url, super::artwork::Shape::Square)
        }
        _ => false,
    };
    if !drew && cover.width > 0 && cover.height > 0 {
        frame.render_widget(
            Block::default().style(Style::default().bg(palette.placeholder)),
            cover,
        );
    }

    // The text sits beside the cover, bottom-aligned with it the way the web
    // client stacks it.
    let text_x = area.x + cover_w + 2;
    if text_x >= area.x + area.width {
        return;
    }
    let text_w = area.x + area.width - text_x;

    let lines: [(&str, ratatui::style::Style); 3] = [
        (banner.title, palette.page_heading()),
        (banner.subtitle, palette.title()),
        (banner.detail, palette.subtitle()),
    ];
    // Bottom-aligned: the title sits three rows up from the cover's base.
    let first_y = area.y + cover_h.saturating_sub(3);
    for (i, (text, style)) in lines.iter().enumerate() {
        let y = first_y + i as u16;
        if text.is_empty() || y >= area.y + area.height {
            continue;
        }
        frame.render_widget(
            Paragraph::new(Line::styled(truncate(text, text_w), *style)),
            Rect { x: text_x, y, width: text_w, height: 1 },
        );
    }
}

fn render_buttons(frame: &mut Frame, area: Rect, palette: &Palette) {
    // The web client's two pill buttons. They are labels here, not controls:
    // the keys do the work, and a fake button nobody can click would only
    // mislead.
    let line = Line::from(vec![
        Span::styled("  ▶ Play  ", palette.on_accent_pill()),
        Span::raw("  "),
        Span::styled("  ⤨ Shuffle  ", palette.pill()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_filter(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    state: &TrackListState,
    in_collection: bool,
) {
    let (text, style) = if state.filter.is_empty() {
        let hint = if in_collection {
            "  Filter this list"
        } else {
            "  Filter tracks"
        };
        (hint.to_string(), palette.subtitle())
    } else {
        (format!("  {}", state.filter), palette.title())
    };
    frame.render_widget(
        Paragraph::new(Line::styled(text, style))
            .block(Block::default().style(Style::default().bg(palette.surface))),
        area,
    );
}

fn render_header(frame: &mut Frame, area: Rect, palette: &Palette, cols: &Columns) {
    let style = palette.subtitle();
    let mut x = area.x;
    let mut put = |frame: &mut Frame, text: &str, width: u16| {
        if width > 0 && x < area.x + area.width {
            let width = width.min(area.x + area.width - x);
            frame.render_widget(
                Paragraph::new(Line::styled(truncate(text, width), style)),
                Rect { x, y: area.y, width, height: 1 },
            );
        }
        x += width;
    };

    put(frame, "#", cols.number);
    put(frame, "", cols.thumb);
    put(frame, "TITLE", cols.title);
    put(frame, "ARTIST", cols.artist);
    put(frame, "ALBUM", cols.album);
    put(frame, "ADDED", cols.added);
    put(frame, "TIME", cols.duration);
}

#[allow(clippy::too_many_arguments)]
fn render_row<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    cols: &Columns,
    track: &Track,
    number: usize,
    selected: bool,
    playing: bool,
    tier: super::nowplaying::Tier,
    draw_cover: &mut F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    if selected {
        frame.render_widget(
            Block::default().style(palette.row_focused()),
            Rect { height: area.height.min(ROW_HEIGHT), ..area },
        );
    }

    // The thumbnail spans the row's two content rows; the text sits on the
    // first of them so it lines up with the middle of the image.
    let text_y = area.y;
    let mut x = area.x;

    // A playing track shows a speaker where its number would be, as the web
    // client does — the number is the less useful of the two.
    if cols.number > 0 && x < area.x + area.width {
        let (text, style) = if playing {
            // A single-cell glyph: an emoji speaker is two cells wide and
            // shunts the title out of line with every other row.
            ("♪".to_string(), palette.playing_row(tier))
        } else {
            (number.to_string(), palette.subtitle())
        };
        frame.render_widget(
            Paragraph::new(Line::styled(text, style)),
            Rect { x, y: text_y, width: cols.number.min(area.width), height: 1 },
        );
    }
    x += cols.number;

    if cols.thumb > 0 && x + cols.thumb <= area.x + area.width {
        let thumb = Rect {
            x,
            y: area.y,
            width: cols.thumb.saturating_sub(1),
            height: area.height.min(2),
        };
        let drew = match &track.cover {
            Some(url) if thumb.height > 0 => draw_cover(frame, thumb, url, super::artwork::Shape::Square),
            _ => false,
        };
        if !drew && thumb.height > 0 {
            frame.render_widget(
                Block::default().style(Style::default().bg(palette.placeholder)),
                thumb,
            );
        }
    }
    x += cols.thumb;

    // The title carries the explicit badge, so it gets built from spans
    // rather than being one truncated string.
    if cols.title > 0 && x < area.x + area.width {
        let width = cols.title.min(area.x + area.width - x);
        let badge = if track.explicit { " E" } else { "" };
        let title = truncate(&track.title, width.saturating_sub(badge.len() as u16 + 1));
        let title_style = if playing {
            palette.playing_row(tier)
        } else {
            palette.title()
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(title, title_style),
                Span::styled(badge, palette.subtitle()),
            ])),
            Rect { x, y: text_y, width, height: 1 },
        );
    }
    x += cols.title;

    let mut put = |frame: &mut Frame, text: &str, width: u16, style: Style| {
        if width > 0 && x < area.x + area.width {
            let width = width.min(area.x + area.width - x);
            frame.render_widget(
                Paragraph::new(Line::styled(truncate(text, width), style)),
                Rect { x, y: text_y, width, height: 1 },
            );
        }
        x += width;
    };

    put(frame, &track.artist, cols.artist, palette.subtitle());
    put(frame, &track.album, cols.album, palette.subtitle());
    put(frame, &added_label(track.added.as_deref()), cols.added, palette.subtitle());

    // The duration is right-aligned against the pane's edge, as a column of
    // times reads better that way.
    if cols.duration > 0 && x < area.x + area.width {
        let width = cols.duration.min(area.x + area.width - x);
        frame.render_widget(
            Paragraph::new(Line::styled(
                super::nowplaying::format_time(track.duration),
                palette.subtitle(),
            ))
            .alignment(ratatui::layout::Alignment::Right),
            Rect { x, y: text_y, width, height: 1 },
        );
    }
}

/// The date column. TIDAL sends a full timestamp; only the day is shown, and
/// anything unparseable shows nothing rather than a wrong date.
///
/// The web client says "This week" and "Last month" here. Those need a
/// calendar — month lengths, leap years, the user's timezone — for a column
/// nobody sorts by, and a plain date says the same thing without any of it.
fn added_label(added: Option<&str>) -> String {
    let Some(added) = added else { return String::new() };
    // "2026-07-17T09:12:44.000+0000" → "2026-07-17".
    added.split('T').next().unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn tracks(n: usize) -> Vec<Track> {
        (0..n)
            .map(|i| Track::sample(&format!("Track {i}"), "An Artist", Duration::from_secs(200)))
            .collect()
    }

    #[test]
    fn selection_moves_and_clamps_to_the_list() {
        let mut s = TrackListState::default();
        s.next(3);
        assert_eq!(s.selected, 1);
        s.next(3);
        s.next(3);
        assert_eq!(s.selected, 2, "must not run past the last row");
        s.previous();
        assert_eq!(s.selected, 1);
        s.previous();
        s.previous();
        assert_eq!(s.selected, 0, "must not go below zero");
    }

    #[test]
    fn selection_on_an_empty_list_stays_at_zero() {
        let mut s = TrackListState::default();
        s.next(0);
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn scrolling_follows_the_selection_and_comes_back() {
        let mut s = TrackListState::default();
        for _ in 0..10 {
            s.next(100);
            s.scroll_into_view(5);
        }
        assert_eq!(s.selected, 10);
        assert_eq!(s.offset, 6, "the window shows 6..11");

        for _ in 0..10 {
            s.previous();
            s.scroll_into_view(5);
        }
        assert_eq!(s.selected, 0);
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn filtering_pulls_the_selection_back_into_the_list() {
        let mut s = TrackListState { selected: 90, offset: 80, filter: String::new() };
        s.clamp(4);
        assert_eq!(s.selected, 3);
        s.clamp(0);
        assert_eq!(s.selected, 0);
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn the_filter_matches_title_artist_and_album() {
        let mut t = tracks(3);
        t[0].title = "Dear Mama".into();
        t[1].artist = "2Pac".into();
        t[2].album = "All Eyez On Me".into();

        assert_eq!(filter(&t, "dear").len(), 1);
        assert_eq!(filter(&t, "2PAC").len(), 1, "matching must ignore case");
        assert_eq!(filter(&t, "eyez").len(), 1, "the album counts too");
        assert_eq!(filter(&t, "").len(), 3);
    }

    #[test]
    fn a_date_shows_only_its_day_and_junk_shows_nothing() {
        assert_eq!(added_label(Some("2026-07-17T09:12:44.000+0000")), "2026-07-17");
        assert_eq!(added_label(None), "");
    }

    #[test]
    fn narrow_panes_drop_columns_rather_than_squeezing_them_all() {
        // Three text columns in 40 cells would leave each unreadable.
        let wide = columns(140, false);
        assert!(wide.album > 0 && wide.added > 0, "a wide pane shows everything");

        let medium = columns(80, false);
        assert_eq!(medium.added, 0, "the date goes first");
        assert!(medium.artist > 0);

        let narrow = columns(50, false);
        assert_eq!(narrow.album, 0, "then the album");

        let tiny = columns(40, false);
        assert_eq!(tiny.artist, 0, "then the artist, leaving the title");
        assert!(tiny.title > 0, "the title always survives");
    }

    #[test]
    fn an_album_drops_the_columns_that_repeat_on_every_row() {
        // Inside an album every row carries the same album name and no added
        // date. Two columns of identical text push the title into an
        // ellipsis for nothing.
        let inside = columns(140, true);
        assert_eq!(inside.album, 0, "the album column repeats itself");
        assert_eq!(inside.added, 0, "album tracks have no added date");
        assert!(inside.artist > 0, "the artist still varies, on compilations");

        let outside = columns(140, false);
        assert!(
            inside.title > outside.title,
            "the reclaimed space goes to the title: {} vs {}",
            inside.title,
            outside.title
        );
    }

    #[test]
    fn column_widths_never_exceed_the_pane() {
        for w in [20u16, 40, 60, 80, 100, 140, 200] {
            let c = columns(w, false);
            let total = c.number + c.thumb + c.title + c.artist + c.album + c.added + c.duration;
            assert!(total <= w, "columns for {w} sum to {total}");

            let c = columns(w, true);
            let total = c.number + c.thumb + c.title + c.artist + c.album + c.added + c.duration;
            assert!(total <= w, "collection columns for {w} sum to {total}");
        }
    }

    #[test]
    fn rendering_does_not_panic_at_any_size() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let palette = Palette::detect();
        let all = tracks(30);
        let refs: Vec<&Track> = all.iter().collect();
        let state = TrackListState::default();

        for (w, h) in [(1, 1), (20, 5), (80, 24), (200, 60), (40, 9)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| {
                render(
                    f,
                    f.area(),
                    &palette,
                    TrackList {
                        chrome: Chrome::Full,
                        tracks: &refs,
                        state: &state,
                        focused: true,
                        playing: None,
                        tier: super::super::nowplaying::Tier::Max,
                        banner: None,
                    },
                    |_, _, _, _| false,
                )
            })
            .unwrap();
        }
    }

    #[test]
    fn the_playing_track_is_marked_in_place_of_its_number() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let palette = Palette::detect();
        let all = tracks(3);
        let refs: Vec<&Track> = all.iter().collect();
        let state = TrackListState::default();

        let mut term = Terminal::new(TestBackend::new(100, 20)).unwrap();
        term.draw(|f| {
            render(
                f,
                f.area(),
                &palette,
                TrackList {
                    chrome: Chrome::Full,
                    tracks: &refs,
                    state: &state,
                    focused: true,
                    playing: Some(all[1].id),
                    tier: super::super::nowplaying::Tier::Max,
                    banner: None,
                },
                |_, _, _, _| false,
            )
        })
        .unwrap();

        let buf = term.backend().buffer();
        let text: String = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains('▶'), "the playing row must be marked");
        assert!(text.contains("Track 0"));
    }
}
