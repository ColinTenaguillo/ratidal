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
/// Rows above the list: the heading, a blank, the buttons, a blank, the
/// filter, a blank, then the column headers and a blank.
const HEADER_ROWS: u16 = 8;

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
    (height.saturating_sub(HEADER_ROWS) / ROW_HEIGHT) as usize
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

fn columns(width: u16) -> Columns {
    let number = 4;
    let duration = 6;
    // Wide enough for an ISO date; dropped entirely below that.
    let added = if width > 90 { 12 } else { 0 };
    let fixed = number + THUMB_WIDTH + added + duration;
    let flexible = width.saturating_sub(fixed);

    // Below this there is no room for three text columns; drop the album
    // first, then the artist, rather than squeezing all three to nothing.
    let (title, artist, album) = if flexible < 30 {
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
    /// The track currently playing, marked with a ▶ in place of its number.
    pub playing: Option<crate::domain::TrackId>,
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
    let TrackList { tracks, state, focused, playing } = list;
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(
        Paragraph::new(Line::styled("Tracks", palette.page_heading())),
        Rect { height: 1, ..area },
    );

    if area.y + 2 < area.y + area.height {
        render_buttons(frame, Rect { y: area.y + 2, height: 1, ..area }, palette);
    }
    if area.y + 4 < area.y + area.height {
        render_filter(frame, Rect { y: area.y + 4, height: 1, ..area }, palette, state);
    }

    let cols = columns(area.width);
    if area.y + 6 < area.y + area.height {
        render_header(frame, Rect { y: area.y + 6, height: 1, ..area }, palette, &cols);
    }

    let body_y = area.y + HEADER_ROWS;
    if body_y >= area.y + area.height {
        return;
    }
    let visible = visible_rows(area.height);

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
            &mut draw_cover,
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

fn render_filter(frame: &mut Frame, area: Rect, palette: &Palette, state: &TrackListState) {
    let (text, style) = if state.filter.is_empty() {
        ("  Filter tracks".to_string(), palette.subtitle())
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

    // A playing track shows ▶ where its number would be, as the web client
    // does — the number is the less useful of the two.
    if cols.number > 0 && x < area.x + area.width {
        let (text, style) = if playing {
            ("▶".to_string(), palette.accent_text())
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
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(title, palette.title()),
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
        let wide = columns(140);
        assert!(wide.album > 0 && wide.added > 0, "a wide pane shows everything");

        let medium = columns(80);
        assert_eq!(medium.added, 0, "the date goes first");
        assert!(medium.artist > 0);

        let narrow = columns(50);
        assert_eq!(narrow.album, 0, "then the album");

        let tiny = columns(40);
        assert_eq!(tiny.artist, 0, "then the artist, leaving the title");
        assert!(tiny.title > 0, "the title always survives");
    }

    #[test]
    fn column_widths_never_exceed_the_pane() {
        for w in [20u16, 40, 60, 80, 100, 140, 200] {
            let c = columns(w);
            let total = c.number + c.thumb + c.title + c.artist + c.album + c.added + c.duration;
            assert!(total <= w, "columns for {w} sum to {total}");
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
                        tracks: &refs,
                        state: &state,
                        focused: true,
                        playing: None,
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
                    tracks: &refs,
                    state: &state,
                    focused: true,
                    playing: Some(all[1].id),
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
