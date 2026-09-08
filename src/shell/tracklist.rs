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

/// Three rows so the text has a row exactly at the thumbnail's middle — two
/// rows have no middle row, which left every row's text aligned with the top
/// of its cover instead. Nothing is added for separation: a trailing blank
/// line cost a quarter of the pane, and the rows are already told apart by
/// their artwork, the selected one by its band.
pub(super) const ROW_HEIGHT: u16 = 3;

/// How much of a row has to fit for it to be worth cutting at the fold.
///
/// Two of its three rows: the first carries the top of the artwork and the
/// second the title beside it, so two rows is a row you can read. One is a
/// sliver of cover with nothing to say what it is.
const MIN_VISIBLE_ROW: u16 = 2;
/// Three rows of cover, whose width `square_width` derives from them: at a
/// terminal cell's aspect (roughly 7x14px) that comes out about square, and
/// four rows would leave it tall and narrow.
const THUMB_ROWS: u16 = 3;
/// Columns between the thumbnail and the title.
const THUMB_GAP: u16 = 1;

/// The column a row's thumbnail takes, gutter included.
///
/// The artwork itself gets `square_width`, which is what makes it square at
/// this terminal's cell; the gutter is added rather than taken out of it, or
/// the image is fitted to a narrower box and stops short of filling its
/// rows — which showed as a selection band standing well above the cover it
/// was meant to be behind.
fn thumb_width() -> u16 {
    super::carousel::square_width(THUMB_ROWS) + THUMB_GAP
}
/// Rows above the list for the plain Tracks view: the heading, a blank, the
/// filter box's three rows, a blank, then the column headers and a blank.
const HEADER_ROWS: u16 = 8;

/// Rows above the list when the caller has drawn its own heading and box:
/// just the column headers and a blank line. Search reuses this view under
/// its own tabs, and a second "Tracks" heading with a second filter box
/// under the search box would be the same furniture twice.
const BARE_HEADER_ROWS: u16 = 2;

/// The column each side of a row that the selection ring is drawn in.
const RING_WIDTH: u16 = 1;

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
    duration: u16,
}

/// Whether every track here belongs to the same record.
///
/// True for an opened album, where the column would repeat one title down
/// the whole list; false for a playlist or a curated row, where it is what
/// tells the tracks apart. An empty list has no column to justify either.
///
/// Tracks with no album at all do not count against it: the API leaves the
/// field empty on some rows, and one blank should not cost the column for
/// every other track.
fn one_album(tracks: &[&Track]) -> bool {
    let mut named = tracks.iter().map(|t| t.album.as_str()).filter(|a| !a.is_empty());
    match named.next() {
        None => true,
        Some(first) => named.all(|a| a == first),
    }
}

fn columns(width: u16, in_collection: bool) -> Columns {
    let width = width.saturating_sub(RING_WIDTH * 2);
    let number = 4;
    let duration = 6;
    let fixed = number + thumb_width() + duration;
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

    Columns { number, thumb: thumb_width(), title, artist, album, duration }
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
    /// Whether the filter box has the keyboard, so it can show a caret.
    /// Without one there is nothing to say a keystroke goes into the box
    /// rather than driving the list.
    pub filtering: bool,
    /// The ids of the user's favourites, for the mark at the end of a row.
    /// A track reached through an album, a playlist or a search carries no
    /// `added` date of its own, so membership is the only thing that says
    /// whether it is one.
    pub favourites: &'a std::collections::HashSet<crate::domain::TrackId>,
}

/// The header an opened album or playlist gets: its cover and what it is.
pub struct Banner<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    pub detail: &'a str,
    pub cover: Option<&'a str>,
    /// Whether the cover is an avatar rather than a record sleeve. An
    /// artist opens into a moment of this view before their own page
    /// arrives, and their photo drawn square here flashed from round to
    /// square and back again.
    pub round: bool,
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

/// The rows a banner takes: its cover's height, or just its lines of text
/// when it has no cover.
///
/// A row opened with "see all" is not a record and has no artwork, so it is
/// a heading over its contents rather than a picture with text beside it --
/// reserving the cover's rows for it left a band of empty pane.
pub fn banner_rows(has_cover: bool) -> u16 {
    if has_cover {
        BANNER_ROWS
    } else {
        // The title and the detail under it, then the blank the cover's own
        // height would have carried.
        3
    }
}

/// Rows above the track list, which depends on its chrome and its banner.
pub fn header_rows(has_banner: bool) -> u16 {
    header_rows_with(has_banner, Chrome::Full)
}

pub fn header_rows_with(has_banner: bool, chrome: Chrome) -> u16 {
    header_rows_of(has_banner, true, chrome)
}

/// As [`header_rows_with`], saying whether the banner carries a cover.
pub fn header_rows_of(has_banner: bool, banner_has_cover: bool, chrome: Chrome) -> u16 {
    match chrome {
        Chrome::Bare => BARE_HEADER_ROWS,
        // The banner stands in for the heading rather than sitting above
        // it, so it costs one row less than its own height. Counting both
        // put the body a row below where the columns were drawn, and with a
        // banner up an album showed its cover and not one track.
        Chrome::Full if has_banner => HEADER_ROWS + banner_rows(banner_has_cover) - 1,
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
    let TrackList {
        tracks, state, focused, playing, banner, tier, chrome, favourites, filtering,
    } = list;
    if area.width == 0 || area.height == 0 {
        return;
    }

    // A column for the scrollbar, held back whether or not it is drawn.
    let full = area;
    let area = super::scrollbar::reserve(area);
    let bottom = area.y + area.height;

    // An opened album leads with its cover and details; the plain Tracks view
    // just names itself.
    //
    // Whether the album column is worth its width is asked of the tracks,
    // not of the banner: a row like "TIDAL's Top Hits" opens with a banner
    // and every track on it comes from a different record, so keying on the
    // banner dropped the one column that told them apart.
    let cols = columns(area.width, one_album(tracks));
    match chrome {
        Chrome::Bare => {
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
                    y += banner_rows(b.cover.is_some() || b.round);
                }
                None => {
                    frame.render_widget(
                        Paragraph::new(Line::styled("Tracks", palette.page_heading())),
                        Rect { y, height: 1, ..area },
                    );
                }
            }
            if y + 2 < bottom {
                render_filter(
                    frame,
                    Rect { y: y + 2, height: super::inputbox::HEIGHT, ..area },
                    palette,
                    state,
                    banner.is_some(),
                    filtering,
                );
            }
            if y + 6 < bottom {
                render_header(frame, Rect { y: y + 6, height: 1, ..area }, palette, &cols);
            }
        }
    }

    let body_y = area.y
        + header_rows_of(
            banner.is_some(),
            banner.as_ref().is_some_and(|b| b.cover.is_some() || b.round),
            chrome,
        );
    if body_y >= bottom {
        return;
    }
    // Measured from where the body actually starts, not from a header of an
    // assumed shape: a banner with no cover is shorter than one with, and
    // reserving the cover's rows anyway left the list stopping short of the
    // player with a band of empty pane under it.
    let left = bottom - body_y;
    let whole = (left / ROW_HEIGHT) as usize;
    // A row at the fold shows as much of itself as fits, cut off by the
    // pane's edge the way the web client leaves one half-scrolled -- and the
    // way the home page's own rows do. Counting whole rows alone ended the
    // list on a hard edge with a band of pane under it, which reads as the
    // list having stopped rather than carrying on.
    //
    // Below MIN_VISIBLE_ROW there is nothing to see: a sliver of artwork
    // with no room for the title beside it is noise rather than a row.
    let part = left % ROW_HEIGHT;
    let visible = whole + usize::from(part >= MIN_VISIBLE_ROW);

    // Beside the rows themselves, not the whole pane: the bar marks how far
    // down the list you are, and a bar that started at the heading and
    // stopped short of the player was measuring something nobody scrolls.
    super::scrollbar::render(
        frame,
        Rect {
            x: full.x,
            y: body_y,
            width: full.width.saturating_sub(super::scrollbar::WIDTH),
            height: bottom.saturating_sub(body_y),
        },
        palette,
        tracks.len(),
        state.offset,
        visible,
    );

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
                // Clamped, though `visible_rows` divides the pane by
                // ROW_HEIGHT and so only ever counts whole rows: nothing
                // reaches here with less than a row left. The clamp is what
                // keeps that true if the count and the layout ever drift.
                height: ROW_HEIGHT.min(area.y + area.height - y),
            },
            palette,
            &cols,
            track,
            i + 1,
            focused && i == state.selected,
            playing == Some(track.id),
            favourites.contains(&track.id),
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

    // A row opened with "see all" has no artwork of its own -- a row is not
    // a record -- so it gets no cover box at all rather than a grey square
    // standing in for a picture that was never coming. The web does the
    // same: "Custom mixes" is a heading over its contents, with nothing
    // beside it.
    let has_cover = banner.cover.is_some() || banner.round;
    let cover_w = if has_cover { COVER_WIDTH.min(area.width) } else { 0 };
    let cover_h = if has_cover { COVER_HEIGHT.min(area.height) } else { 0 };
    let cover = Rect { x: area.x, y: area.y, width: cover_w, height: cover_h };

    let shape = if banner.round {
        super::artwork::Shape::Round
    } else {
        super::artwork::Shape::Square
    };
    let drew = match banner.cover {
        Some(url) if cover.width > 0 && cover.height > 0 => {
            draw_cover(frame, cover, url, shape)
        }
        _ => false,
    };
    if !drew && cover.width > 0 && cover.height > 0 && banner.round {
        // The same disc the avatars use, so the stand-in is the shape of
        // what it stands in for.
        let initial = banner
            .title
            .chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_uppercase().next().unwrap_or(c));
        super::carousel::render_disc(frame, cover, palette, initial);
    } else if !drew && cover.width > 0 && cover.height > 0 {
        frame.render_widget(
            Block::default().style(Style::default().bg(palette.placeholder)),
            cover,
        );
    }

    // The text sits beside the cover, bottom-aligned with it the way the web
    // client stacks it.
    let text_x = if has_cover { area.x + cover_w + 2 } else { area.x };
    if text_x >= area.x + area.width {
        return;
    }
    let text_w = area.x + area.width - text_x;

    let lines: [(&str, ratatui::style::Style); 3] = [
        (banner.title, palette.page_heading()),
        (banner.subtitle, palette.title()),
        (banner.detail, palette.subtitle()),
    ];
    // Bottom-aligned against the cover, when there is one: the title sits
    // three rows up from its base. With no cover the heading starts at the
    // top, where a heading belongs.
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

fn render_filter(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    state: &TrackListState,
    in_collection: bool,
    filtering: bool,
) {
    let hint = if in_collection {
        "Filter this list"
    } else {
        "Filter tracks"
    };
    super::inputbox::render(frame, area, palette, hint, &state.filter, filtering);
}

fn render_header(frame: &mut Frame, area: Rect, palette: &Palette, cols: &Columns) {
    let style = palette.subtitle();
    // Over the rows' own columns, which sit inside the selection ring.
    let mut x = area.x + RING_WIDTH;
    let mut put = |frame: &mut Frame,
                   text: &str,
                   width: u16,
                   align: ratatui::layout::Alignment| {
        if width > 0 && x < area.x + area.width {
            let width = width.min(area.x + area.width - x);
            frame.render_widget(
                Paragraph::new(Line::styled(truncate(text, width), style)).alignment(align),
                Rect { x, y: area.y, width, height: 1 },
            );
        }
        x += width;
    };

    use ratatui::layout::Alignment;
    let (left, centre) = (Alignment::Left, Alignment::Center);
    put(frame, "#", cols.number, left);
    put(frame, "", cols.thumb, left);
    put(frame, "TITLE", cols.title, left);
    put(frame, "ARTIST", cols.artist, left);
    put(frame, "ALBUM", cols.album, left);
    // Centred, so the header sits over the times rather than off to one side.
    put(frame, "TIME", cols.duration, centre);
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
    favourite: bool,
    tier: super::nowplaying::Tier,
    draw_cover: &mut F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    if selected {
        // The row's content, not its whole pitch: the last of the four rows
        // is the blank line between one row and the next.
        super::theme::selection_band(
            frame,
            Rect { height: area.height.min(THUMB_ROWS), ..area },
            palette,
        );
    }

    // The thumbnail spans the row's three content rows and the text sits on
    // the middle one, which is what puts every cell of the row on the
    // cover's centre line rather than its top edge.
    let text_y = area.y + (area.height.min(THUMB_ROWS)) / 2;
    // Inside the ring's column, which every row leaves free whether or not
    // it is the selected one — taken only on selection, the whole row would
    // jump a column sideways as the cursor passed over it.
    let mut x = area.x + RING_WIDTH;

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
            width: cols.thumb.saturating_sub(THUMB_GAP),
            height: area.height.min(THUMB_ROWS),
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

    // The title carries its marks, so it gets built from spans rather than
    // being one truncated string.
    if cols.title > 0 && x < area.x + area.width {
        let width = cols.title.min(area.x + area.width - x);
        let marks = marks(track, favourite);
        let title = truncate(
            &track.title,
            width.saturating_sub(marks.chars().count() as u16 + 1),
        );
        let title_style = if playing {
            palette.playing_row(tier)
        } else {
            palette.title()
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(title, title_style),
                Span::styled(marks, palette.mark()),
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

    // Centred in its column: right-aligned, the times sat hard against the
    // pane's edge with the TIME header floating away from them.
    if cols.duration > 0 && x < area.x + area.width {
        let width = cols.duration.min(area.x + area.width - x);
        frame.render_widget(
            Paragraph::new(Line::styled(
                super::nowplaying::format_time(track.duration),
                palette.subtitle(),
            ))
            .alignment(ratatui::layout::Alignment::Center),
            Rect { x, y: text_y, width, height: 1 },
        );
    }
}

/// The marks that follow a title: explicit, then favourite.
///
/// One place, so a change of icon set lands everywhere at once. These are
/// plain Unicode rather than nerd-font glyphs, which show as empty boxes for
/// anyone without the font — the same rule the sidebar's icons follow. A
/// favourite marks itself; nothing is drawn when it is not one, so a list
/// with no favourites in it carries no column of empty circles.
fn marks(track: &Track, favourite: bool) -> String {
    let mut out = String::new();
    if track.explicit {
        out.push_str(" E");
    }
    if favourite {
        out.push(' ');
        out.push_str(super::icons::favourite());
    }
    out
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
    fn a_rows_text_sits_on_the_middle_of_its_cover() {
        // The cover used to be two rows with the text on the first, so every
        // row read as top-aligned against its artwork with a blank line
        // hanging under it. Two rows have no middle row; three do.
        let mut all = tracks(2);
        let mut covers: Vec<Rect> = Vec::new();
        for (i, t) in all.iter_mut().enumerate() {
            t.id = crate::domain::TrackId(i as u64);
            t.cover = Some("x".into());
        }
        let favourites = std::collections::HashSet::new();
        let state = TrackListState::default();
        let buf = crate::shell::geometry::draw(60, 12, |f, area, palette| {
            let refs: Vec<&Track> = all.iter().collect();
            render(
                f,
                area,
                palette,
                TrackList {
                    filtering: false,
                    favourites: &favourites,
                    tracks: &refs,
                    state: &state,
                    focused: false,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                    banner: None,
                    chrome: Chrome::Bare,
                },
                |_, r, _, _| {
                    covers.push(r);
                    true
                },
            )
        });

        let cover = covers.first().copied().expect("a cover was drawn");
        // Stated, not read back off the cover: deriving the middle from
        // whatever height was drawn holds for a two-row cover too, which is
        // the geometry this replaced.
        assert_eq!(cover.height, 3, "the cover is three rows tall");

        let title = crate::shell::geometry::find(&buf, "Track 0").expect("its title");
        let middle = cover.y + cover.height / 2;
        assert_eq!(
            title.row,
            middle,
            "the title sits on the cover's middle row ({}..{}), not at {}\n{}",
            cover.y,
            cover.y + cover.height,
            title.row,
            crate::shell::geometry::text(&buf)
        );
    }

    #[test]
    fn every_cell_of_a_row_shares_one_line() {
        // The number, title, artist and time all have to sit on the same row
        // as each other, or the row reads as several.
        let mut all = tracks(1);
        all[0].cover = Some("x".into());
        let favourites = std::collections::HashSet::new();
        let state = TrackListState::default();
        let buf = crate::shell::geometry::draw(80, 12, |f, area, palette| {
            let refs: Vec<&Track> = all.iter().collect();
            render(
                f,
                area,
                palette,
                TrackList {
                    filtering: false,
                    favourites: &favourites,
                    tracks: &refs,
                    state: &state,
                    focused: false,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                    banner: None,
                    chrome: Chrome::Bare,
                },
                |_, _, _, _| true,
            )
        });

        let title = crate::shell::geometry::find(&buf, "Track 0").expect("the title");
        let artist = crate::shell::geometry::find(&buf, "An Artist").expect("the artist");
        let time = crate::shell::geometry::find(&buf, "3:20").expect("the time");
        assert_eq!(artist.row, title.row, "the artist shares the title's row");
        assert_eq!(time.row, title.row, "and so does the time");
    }

    #[test]
    fn the_selected_band_is_capped_at_both_ends() {
        // A background fills whole cells, so a band on its own is a
        // hard-edged rectangle. A half block is inked over half its cell,
        // which softens each end into something nearer a rounded edge.
        let all = tracks(3);
        let favourites = std::collections::HashSet::new();
        let state = TrackListState { selected: 1, ..Default::default() };
        let refs: Vec<&Track> = all.iter().collect();
        let buf = crate::shell::geometry::draw(56, 16, move |f, area, palette| {
            render(
                f,
                area,
                palette,
                TrackList {
                    filtering: false,
                    favourites: &favourites,
                    tracks: &refs,
                    state: &state,
                    focused: true,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                    banner: None,
                    chrome: Chrome::Bare,
                },
                |_, _, _, _| false,
            )
        });
        let text = crate::shell::geometry::text(&buf);

        assert!(text.contains('▐'), "a cap on the left:\n{text}");
        assert!(text.contains('▌'), "and one on the right:\n{text}");

        // The band between them is filled, not outlined.
        let row = crate::shell::geometry::find(&buf, "Track 1").expect("the row");
        assert_ne!(
            buf[(30, row.row)].bg,
            ratatui::style::Color::Reset,
            "the band is filled:\n{text}"
        );
    }

    #[test]
    fn a_row_is_exactly_its_own_content() {
        // It used to carry a blank line after it, which cost a quarter of
        // the pane for separation the rows do not need — and left the
        // selection band standing above the artwork it marks.
        assert_eq!(ROW_HEIGHT, THUMB_ROWS, "no spare row in a row's pitch");
    }

    #[test]
    fn the_thumbnail_gets_its_full_width_and_the_gutter_besides() {
        // The gutter used to be taken out of the artwork's own columns, so
        // the image was fitted to a narrower box and stopped short of
        // filling its rows.
        let square = crate::shell::carousel::square_width(THUMB_ROWS);
        assert_eq!(
            thumb_width(),
            square + THUMB_GAP,
            "the column is the artwork plus its gutter, not the artwork \
             with a column taken out"
        );
    }

    #[test]
    fn the_selection_stops_at_the_row_and_not_the_gap_below_it() {
        // A row is four rows tall and the last is the blank line between one
        // row and the next; shading it made the highlight look like it had
        // spilled past the track it marks.
        let all = tracks(3);
        let favourites = std::collections::HashSet::new();
        let state = TrackListState { selected: 1, ..Default::default() };
        let refs: Vec<&Track> = all.iter().collect();
        let buf = crate::shell::geometry::draw(60, 20, move |f, area, palette| {
            render(
                f,
                area,
                palette,
                TrackList {
                    filtering: false,
                    favourites: &favourites,
                    tracks: &refs,
                    state: &state,
                    focused: true,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                    banner: None,
                    chrome: Chrome::Bare,
                },
                |_, _, _, _| false,
            )
        });

        // The shaded rows, read off a column past the thumbnail so a cover
        // placeholder is not mistaken for the highlight.
        let x = 30;
        let shaded: Vec<u16> = (0..20)
            .filter(|y| buf[(x, *y)].bg != ratatui::style::Color::Reset)
            .collect();
        assert_eq!(
            shaded.len(),
            THUMB_ROWS as usize,
            "the highlight covers the row's content and nothing else: {shaded:?}\n{}",
            crate::shell::geometry::text(&buf)
        );
        assert!(
            shaded.windows(2).all(|w| w[1] == w[0] + 1),
            "and they are the row's own, unbroken: {shaded:?}"
        );
    }

    #[test]
    fn the_scrollbar_spans_the_rows_and_nothing_else() {
        // It marks how far down the list you are, so it belongs beside the
        // rows: drawn against the whole pane it started at the heading and
        // measured something nobody scrolls.
        let all = tracks(30);
        let favourites = std::collections::HashSet::new();
        let state = TrackListState::default();
        let refs: Vec<&Track> = all.iter().collect();
        let buf = crate::shell::geometry::draw(70, 20, move |f, area, palette| {
            render(
                f,
                area,
                palette,
                TrackList {
                    filtering: false,
                    favourites: &favourites,
                    tracks: &refs,
                    state: &state,
                    focused: true,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                    banner: None,
                    chrome: Chrome::Full,
                },
                |_, _, _, _| false,
            )
        });

        let x = 70 - crate::shell::scrollbar::WIDTH;
        let painted: Vec<u16> = (0..20)
            .filter(|y| buf[(x, *y)].symbol() != " ")
            .collect();
        assert!(!painted.is_empty(), "the bar is drawn");

        let first = *painted.first().unwrap();
        let last = *painted.last().unwrap();
        assert_eq!(
            first,
            HEADER_ROWS,
            "it starts at the first row, not at the heading\n{}",
            crate::shell::geometry::text(&buf)
        );
        assert_eq!(last, 19, "and runs to the bottom of the pane");
    }

    #[test]
    fn an_opened_album_shows_its_tracks_under_the_banner() {
        // The banner stands in for the heading rather than sitting above it,
        // so counting both put the body a row below where the columns were
        // drawn — an opened album showed its cover and not one track.
        let all = tracks(3);
        let favourites = std::collections::HashSet::new();
        let state = TrackListState::default();
        let refs: Vec<&Track> = all.iter().collect();
        // Tight: room for the banner, the filter box, the columns and one
        // row. A row of slack here and the off-by-one does not show.
        let buf = crate::shell::geometry::draw(90, 20, move |f, area, palette| {
            render(
                f,
                area,
                palette,
                TrackList {
                    filtering: false,
                    favourites: &favourites,
                    tracks: &refs,
                    state: &state,
                    focused: true,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                    banner: Some(Banner {
                        title: "An Album",
                        subtitle: "An Artist",
                        detail: "2026",
                        cover: None,
                        round: false,
                    }),
                    chrome: Chrome::Full,
                },
                |_, _, _, _| false,
            )
        });
        let text = crate::shell::geometry::text(&buf);

        assert!(text.contains("An Album"), "the banner is drawn:\n{text}");
        assert!(
            text.contains("Track 0"),
            "and the tracks under it, not just the cover:\n{text}"
        );
    }

    #[test]
    fn an_opened_row_shows_which_album_each_track_is_from() {
        // Straight from the report: opening "TIDAL's Top Hits" listed the
        // tracks with no album. A row opens with a banner, and the column
        // was dropped whenever there was one — but the whole point of that
        // row is that every track comes from somewhere different.
        let mut all = tracks(3);
        for (i, t) in all.iter_mut().enumerate() {
            t.album = format!("Record {i}");
        }
        let favourites = std::collections::HashSet::new();
        let state = TrackListState::default();
        let refs: Vec<&Track> = all.iter().collect();
        // Tall enough for all three rows: the column is drawn per row, so
        // a pane that fits one would pass on the first alone.
        let buf = crate::shell::geometry::draw(140, 32, move |f, area, palette| {
            render(
                f,
                area,
                palette,
                TrackList {
                    filtering: false,
                    favourites: &favourites,
                    tracks: &refs,
                    state: &state,
                    focused: true,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                    banner: Some(Banner {
                        title: "TIDAL's Top Hits",
                        subtitle: "",
                        detail: "",
                        cover: None,
                        round: false,
                    }),
                    chrome: Chrome::Full,
                },
                |_, _, _, _| false,
            )
        });
        let text = crate::shell::geometry::text(&buf);

        for i in 0..3 {
            assert!(
                text.contains(&format!("Record {i}")),
                "every track says which record it is from:\n{text}"
            );
        }
    }

    #[test]
    fn the_filter_box_shows_a_caret_while_it_has_the_keyboard() {
        // Nothing said whether a keystroke went into the box or drove the
        // list; the search box has had a caret for exactly that reason.
        let all = tracks(2);
        let favourites = std::collections::HashSet::new();
        let state = TrackListState::default();
        let draw = |filtering: bool| {
            let refs: Vec<&Track> = all.iter().collect();
            let state = &state;
            let favourites = &favourites;
            crate::shell::geometry::draw(80, 20, move |f, area, palette| {
                render(
                    f,
                    area,
                    palette,
                    TrackList {
                        filtering,
                        favourites,
                        tracks: &refs,
                        state,
                        focused: true,
                        playing: None,
                        tier: super::super::nowplaying::Tier::Low,
                        banner: None,
                        chrome: Chrome::Full,
                    },
                    |_, _, _, _| false,
                )
            })
        };

        // On the filter's own line, since a frame holds other marks too.
        let buf = draw(true);
        let caret = crate::shell::geometry::find(&buf, "█").expect("a caret while typing");
        // Inside the box, which starts two rows under the heading.
        assert_eq!(caret.row, 3, "on the filter box's own line");

        let idle = crate::shell::geometry::text(&draw(false));
        let idle_line = idle.lines().nth(3).unwrap_or_default();
        assert!(
            !idle_line.contains('█'),
            "and none when it does not: {idle_line:?}"
        );
        assert!(idle.contains("Filter tracks"), "the hint is back:\n{idle}");
    }

    #[test]
    fn the_list_reaches_the_bottom_of_the_pane_with_or_without_a_cover() {
        // Reported as a band of empty pane between the last track and the
        // player. How many rows fit was worked out from a header of an
        // assumed shape -- one with a cover -- while the body started higher
        // when there was none, so six rows of pane went unused.
        let favourites = std::collections::HashSet::new();
        let state = TrackListState::default();
        let all = tracks(40);

        let bottom_row = |banner: Banner<'_>| {
            let buf = crate::shell::geometry::draw(100, 40, |f, area, palette| {
                let refs: Vec<&Track> = all.iter().collect();
                render(
                    f,
                    area,
                    palette,
                    TrackList {
                        tracks: &refs,
                        state: &state,
                        focused: false,
                        playing: None,
                        tier: super::super::nowplaying::Tier::Low,
                        banner: Some(banner),
                        chrome: Chrome::Full,
                        filtering: false,
                        favourites: &favourites,
                    },
                    |_, _, _, _| false,
                )
            });
            crate::shell::geometry::text(&buf)
                .lines()
                .enumerate()
                .filter(|(_, l)| l.contains("Track "))
                .map(|(i, _)| i)
                .last()
                .expect("some tracks are drawn")
        };

        let without = bottom_row(Banner {
            title: "New Tracks",
            subtitle: "",
            detail: "150 tracks",
            cover: None,
            round: false,
        });
        let with = bottom_row(Banner {
            title: "An Album",
            subtitle: "Someone",
            detail: "12 tracks",
            cover: Some("x"),
            round: false,
        });

        // Both fill the pane: the last row a track can occupy is the same
        // whichever banner is above it, since the rows below the banner are
        // what is left of the pane either way.
        assert_eq!(
            without, with,
            "a list with no cover stops short of the one with"
        );
        assert!(
            without >= 36,
            "the list stops at row {without} of 40, leaving the pane empty \
             above the player"
        );
    }

    #[test]
    fn a_banner_with_no_cover_paints_no_placeholder() {
        // Reported as a big grey square at the top of "see all". A row is
        // not a record and has no artwork, so the placeholder stood in for
        // a picture that was never coming -- and the box reserved eight
        // rows of pane for it. The web heads these pages with their name
        // and nothing else.
        let favourites = std::collections::HashSet::new();
        let state = TrackListState::default();
        let all = tracks(3);
        let buf = crate::shell::geometry::draw(80, 20, |f, area, palette| {
            let refs: Vec<&Track> = all.iter().collect();
            render(
                f,
                area,
                palette,
                TrackList {
                    tracks: &refs,
                    state: &state,
                    focused: false,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                    banner: Some(Banner {
                        title: "New Tracks",
                        subtitle: "",
                        detail: "150 tracks",
                        cover: None,
                        round: false,
                    }),
                    chrome: Chrome::Full,
                    filtering: false,
                    favourites: &favourites,
                },
                |_, _, _, _| false,
            )
        });

        // The placeholder is a background, not a character, so this reads
        // the cells rather than the text.
        let placeholder = Palette::detect().placeholder;
        let painted = (0..8u16)
            .flat_map(|y| (0..16u16).map(move |x| (x, y)))
            .filter(|(x, y)| buf[(*x, *y)].bg == placeholder)
            .count();
        assert_eq!(
            painted,
            0,
            "a cover's worth of placeholder is painted for artwork that does \
             not exist:\n{}",
            crate::shell::geometry::text(&buf)
        );

        // And the heading is still drawn, at the top of the pane.
        let text = crate::shell::geometry::text(&buf);
        assert!(text.contains("New Tracks"), "headed by its name:\n{text}");
        assert_eq!(
            text.lines().position(|l| l.contains("New Tracks")),
            Some(0),
            "on the first row, not pushed down by a cover box:\n{text}"
        );
    }

    #[test]
    fn a_favourite_is_marked_beside_its_title() {
        // Beside the title rather than in a column of its own: a column
        // reserves two cells on every row to say "not a favourite", which is
        // most of them.
        let mut all = tracks(3);
        for (i, t) in all.iter_mut().enumerate() {
            t.id = crate::domain::TrackId(i as u64);
        }
        all[2].explicit = true;
        all[1].explicit = true;
        let mut favourites = std::collections::HashSet::new();
        favourites.insert(all[1].id);
        let state = TrackListState::default();
        let buf = crate::shell::geometry::draw(100, 16, |f, area, palette| {
            let refs: Vec<&Track> = all.iter().collect();
            render(
                f,
                area,
                palette,
                TrackList {
                    filtering: false,
                    favourites: &favourites,
                    tracks: &refs,
                    state: &state,
                    focused: true,
                    playing: None,
                    tier: super::super::nowplaying::Tier::Low,
                    banner: None,
                    chrome: Chrome::Bare,
                },
                |_, _, _, _| false,
            )
        });
        let text = crate::shell::geometry::text(&buf);

        assert_eq!(
            text.matches('\u{2665}').count(),
            1,
            "only the favourite is marked:\n{text}"
        );
        assert!(!text.contains("ADDED"), "the added column is gone:\n{text}");

        // It follows the title it belongs to, well before the duration.
        let mark = crate::shell::geometry::find(&buf, "\u{2665}").expect("the mark");
        let title = crate::shell::geometry::find(&buf, "Track 1").expect("its title");
        let time = crate::shell::geometry::find(&buf, "3:20").expect("a time");
        assert_eq!(mark.row, title.row, "on the same row as its title");
        assert!(mark.start > title.start, "after the title");
        assert!(mark.start < time.start, "and before the duration:\n{text}");
    }

    #[test]
    fn the_marks_follow_a_title_in_a_fixed_order() {
        // Explicit then favourite, so a track carrying both does not shuffle
        // its marks between rows.
        let mut t = Track::sample("A Title", "An Artist", Duration::from_secs(200));
        assert_eq!(marks(&t, false), "", "an ordinary track carries none");
        assert_eq!(marks(&t, true), " \u{2665}");

        t.explicit = true;
        assert_eq!(marks(&t, false), " E");
        assert_eq!(marks(&t, true), " E \u{2665}", "explicit first, then favourite");
    }

    #[test]
    fn narrow_panes_drop_columns_rather_than_squeezing_them_all() {
        // Three text columns in 40 cells would leave each unreadable.
        let wide = columns(140, false);
        assert!(wide.album > 0, "a wide pane shows everything");

        let medium = columns(80, false);
        assert!(medium.artist > 0);

        let narrow = columns(50, false);
        assert_eq!(narrow.album, 0, "then the album");

        let tiny = columns(40, false);
        assert_eq!(tiny.artist, 0, "then the artist, leaving the title");
        assert!(tiny.title > 0, "the title always survives");
    }

    #[test]
    fn a_row_of_different_albums_keeps_the_album_column() {
        // Opening "TIDAL's Top Hits" showed no album, because the column
        // was dropped whenever the view had a banner — and a row opens with
        // one. Every track there is from a different record, which is
        // exactly when the column earns its width.
        let mut mixed = tracks(3);
        for (i, t) in mixed.iter_mut().enumerate() {
            t.album = format!("Album {i}");
        }
        let refs: Vec<&Track> = mixed.iter().collect();
        assert!(!one_album(&refs), "three records are not one album");

        // And the opposite: an opened album repeats one name down the list.
        let mut same = tracks(3);
        for t in same.iter_mut() {
            t.album = "One Record".into();
        }
        let refs: Vec<&Track> = same.iter().collect();
        assert!(one_album(&refs), "one record on every row");
    }

    #[test]
    fn a_missing_album_does_not_cost_the_column() {
        // The API leaves the field empty on some rows. One blank among
        // several records must not read as "they are all the same".
        let mut mixed = tracks(3);
        mixed[0].album = String::new();
        mixed[1].album = "Album A".into();
        mixed[2].album = "Album B".into();
        let refs: Vec<&Track> = mixed.iter().collect();
        assert!(!one_album(&refs), "two records are still two records");

        // Nothing named at all: there is no column to justify.
        let mut blank = tracks(2);
        for t in blank.iter_mut() {
            t.album = String::new();
        }
        let refs: Vec<&Track> = blank.iter().collect();
        assert!(one_album(&refs), "no album is named anywhere");
    }


    #[test]
    fn an_album_drops_the_columns_that_repeat_on_every_row() {
        // Inside an album every row carries the same album name and no added
        // date. Two columns of identical text push the title into an
        // ellipsis for nothing.
        let inside = columns(140, true);
        assert_eq!(inside.album, 0, "the album column repeats itself");
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
            let total =
                c.number + c.thumb + c.title + c.artist + c.album + c.duration;
            assert!(total <= w, "columns for {w} sum to {total}");

            let c = columns(w, true);
            let total =
                c.number + c.thumb + c.title + c.artist + c.album + c.duration;
            assert!(total <= w, "collection columns for {w} sum to {total}");
        }
    }

    #[test]
    fn rendering_does_not_panic_at_any_size() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let palette = Palette::detect();
        let favourites: std::collections::HashSet<crate::domain::TrackId> =
            std::collections::HashSet::new();
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
                        filtering: false,
                        favourites: &favourites,
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
        let favourites: std::collections::HashSet<crate::domain::TrackId> =
            std::collections::HashSet::new();
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
                    filtering: false,
                    favourites: &favourites,
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
        // The row's own mark, not the Play pill that used to sit in the
        // header: this passed on that instead for as long as it was there.
        assert!(text.contains('♪'), "the playing row must be marked");
        assert!(text.contains("Track 0"));
    }

    #[test]
    fn a_cover_is_never_handed_a_box_that_runs_past_the_pane() {
        // The artwork is drawn by the caller, straight to the terminal, so
        // it is the one thing here that ratatui's clipping does not catch:
        // handed a box wider than the pane it paints over whatever is
        // beside it, and on the protocols that refuse to scale it paints
        // nothing at all. A narrow pane has to drop the column instead.
        let favourites = std::collections::HashSet::new();
        for (width, height) in (1..46u16).flat_map(|w| (4..14u16).map(move |h| (w, h))) {
            let mut all = tracks(4);
            for t in all.iter_mut() {
                t.cover = Some("x".into());
            }
            let state = TrackListState::default();
            let mut covers: Vec<Rect> = Vec::new();
            crate::shell::geometry::draw(width, height, |f, area, palette| {
                let refs: Vec<&Track> = all.iter().collect();
                render(
                    f,
                    area,
                    palette,
                    TrackList {
                        filtering: false,
                        favourites: &favourites,
                        tracks: &refs,
                        state: &state,
                        focused: false,
                        playing: None,
                        tier: super::super::nowplaying::Tier::Low,
                        banner: None,
                        chrome: Chrome::Bare,
                    },
                    |_, r, _, _| {
                        covers.push(r);
                        true
                    },
                )
            });
            for c in &covers {
                assert!(
                    c.x + c.width <= width,
                    "at {width}x{height} a cover was given {c:?}, which ends past the pane"
                );
                assert!(
                    c.y + c.height <= height,
                    "at {width}x{height} a cover was given {c:?}, which ends below the pane"
                );
                assert!(
                    c.width > 0 && c.height > 0,
                    "at {width}x{height} an empty cover box"
                );
            }
        }
    }

    #[test]
    fn probe_fold() {
        let favourites = std::collections::HashSet::new();
        let state = TrackListState::default();
        let all = tracks(20);
        let buf = crate::shell::geometry::draw(90, 43, |f, area, palette| {
            let refs: Vec<&Track> = all.iter().collect();
            render(f, area, palette, TrackList {
                tracks: &refs, state: &state, focused: false, playing: None,
                tier: super::super::nowplaying::Tier::Low, banner: None,
                chrome: Chrome::Full, filtering: false, favourites: &favourites,
            }, |_, _, _, _| false)
        });
        for (i, l) in crate::shell::geometry::text(&buf).lines().enumerate().skip(36) {
            println!("PROBE {i:2} |{}|", l.trim_end());
        }
    }
}
