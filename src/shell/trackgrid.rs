//! A row of tracks laid out as a grid, the way the web client does it.
//!
//! A TRACK_LIST module is not a carousel. The web client draws it as three
//! columns of three, each cell a thumbnail with the title over the artist —
//! and drawing those two rows of the home page as strips of covers made them
//! look like albums, which they are not.
//!
//! Measured from the running client: cells are 483px apart with a 56px
//! thumbnail and 12px before the text. Against a 7x14 cell that is a
//! four-column thumbnail two rows tall, two columns of gutter, and the rest
//! for the text.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use super::carousel::{truncate, Card};
use super::theme::Palette;

/// Columns of cells, as the web client's three.
pub const COLUMNS: usize = 3;
/// Rows of cells, as the web client's three. Counted off the running
/// client: a track row there holds nine, three across and three down.
pub const ROWS: usize = 3;
/// A thumbnail is square at a cell's aspect: six columns to three rows is
/// 42x42px against roughly 7x14 per cell. Four by two was half that and
/// looked like a mistake next to a carousel's covers.
/// A thumbnail is three rows tall; how many columns that is depends on the
/// shape of a terminal cell, the same as a card's own cover.
const THUMB_H: u16 = 3;
fn thumb_w() -> u16 {
    super::carousel::square_width(THUMB_H)
}
/// Between the thumbnail and its text.
const TEXT_GAP: u16 = 2;
/// Between one cell and the next, across and down.
const CELL_GAP_X: u16 = 2;
const CELL_GAP_Y: u16 = 1;
/// A cell is its thumbnail plus a blank line under it.
const CELL_HEIGHT: u16 = THUMB_H + CELL_GAP_Y;

/// How tall a full grid is, so the caller can lay out what follows.
///
/// The blank under a cell is the space between one line of cells and the
/// next, so the last line does not carry one — counting it left two blank
/// lines below a track row where a carousel has one.
pub fn height() -> u16 {
    ROWS as u16 * CELL_HEIGHT - CELL_GAP_Y
}

/// How many cards this grid draws of a row, at `columns` across.
///
/// The grid takes the first `columns * ROWS` and no more — it does not
/// scroll, so anything past those is simply not on screen. Movement has to
/// stop there or the selection lands on a card nobody can see, which it
/// did twice: stepping down through a row, and entering one from below.
pub fn drawn(cards: usize, columns: usize) -> usize {
    cards.min(columns.max(1) * ROWS)
}

/// How many cells fit in `width`, capped at the web client's three.
pub fn columns(width: u16) -> usize {
    // Below this a cell has no room for a title worth reading.
    const MIN_CELL: u16 = 18;
    let fits = (width / (MIN_CELL + CELL_GAP_X)).max(1) as usize;
    fits.min(COLUMNS)
}

/// Draw up to `ROWS * COLUMNS` tracks as a grid.
///
/// `draw_cover` matches the carousel's, so a terminal with no image protocol
/// gets the same layout with placeholders.
pub fn render<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    cards: &[Card],
    selected: Option<usize>,
    marks: Marks<'_>,
    mut draw_cover: F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    if area.width == 0 || area.height == 0 {
        return;
    }
    let cols = columns(area.width);
    if cols == 0 {
        return;
    }
    let cell_w = (area.width - (cols as u16 - 1) * CELL_GAP_X) / cols as u16;

    for (i, card) in cards.iter().take(drawn(cards.len(), cols)).enumerate() {
        let (col, row) = (i % cols, i / cols);
        let x = area.x + col as u16 * (cell_w + CELL_GAP_X);
        let y = area.y + row as u16 * CELL_HEIGHT;
        if y + THUMB_H > area.y + area.height {
            break;
        }
        render_cell(
            frame,
            Rect { x, y, width: cell_w, height: THUMB_H },
            palette,
            card,
            selected == Some(i),
            marks,
            &mut draw_cover,
        );
    }
}

/// What the track list marks a row with, for the same marks here.
///
/// The home page's track rows showed neither what was playing nor what was
/// a favourite, so the same track read differently depending on which view
/// it was in.
#[derive(Debug, Clone, Copy)]
pub struct Marks<'a> {
    pub favourites: &'a std::collections::HashSet<crate::domain::TrackId>,
    pub playing: Option<crate::domain::TrackId>,
    pub tier: super::nowplaying::Tier,
}

/// The track a card stands for, when it is a track at all.
fn track_id(card: &Card) -> Option<crate::domain::TrackId> {
    match card.target {
        Some(super::carousel::Target::Track(id)) => Some(crate::domain::TrackId(id)),
        _ => None,
    }
}

fn render_cell<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    card: &Card,
    selected: bool,
    marks: Marks<'_>,
    draw_cover: &mut F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    // The whole cell is shaded, thumbnail and text alike — the same mark a
    // carousel card gets, since the same key opens both. Marking the title
    // alone said the title was picked rather than the track.
    //
    // The same band the track list draws, softened ends and all: this drew
    // a hard-edged rectangle, so one selection looked like two different
    // marks depending on which view it was in.
    if selected {
        super::theme::selection_band(frame, area, palette);
    }

    // Inside the band's end column, which every cell leaves free whether or
    // not it is selected — taken only on selection, the cell's contents
    // would jump sideways as the cursor passed over it.
    let area = Rect {
        x: area.x + super::theme::RING,
        width: area.width.saturating_sub(super::theme::RING * 2),
        ..area
    };
    if area.width == 0 {
        return;
    }

    let thumb_w = thumb_w().min(area.width);
    if thumb_w > 0 {
        let thumb = Rect { width: thumb_w, height: area.height, ..area };
        let drew = match &card.cover_url {
            Some(url) => draw_cover(frame, thumb, url, super::artwork::Shape::Square),
            None => false,
        };
        if !drew {
            frame.render_widget(
                Block::default().style(Style::default().bg(palette.placeholder)),
                thumb,
            );
        }
    }

    let text_x = area.x + thumb_w + TEXT_GAP;
    if text_x >= area.x + area.width {
        return;
    }
    let text_w = area.x + area.width - text_x;

    // Title over artist, as the web client stacks them: white over a dimmer
    // grey.
    // The shade behind it is the mark; tinting the text as well is two
    // marks for one selection.
    let id = track_id(card);
    let playing = id.is_some() && id == marks.playing;
    let favourite = id.is_some_and(|i| marks.favourites.contains(&i));

    // The same two marks a row of the track list carries: a note for what
    // is playing, a heart for a favourite. The note replaces nothing here —
    // a grid cell has no number column to give up — so it leads the title.
    let mut spans = Vec::new();
    if playing {
        spans.push(ratatui::text::Span::styled(
            "♪ ",
            palette.playing_row(marks.tier),
        ));
    }
    let heart = if favourite { " ♥" } else { "" };
    let room = text_w
        .saturating_sub(spans.len() as u16 * 2)
        .saturating_sub(heart.chars().count() as u16);
    spans.push(ratatui::text::Span::styled(
        truncate(&card.title, room),
        palette.title(),
    ));
    if favourite {
        spans.push(ratatui::text::Span::styled(heart, palette.accent_text()));
    }
    frame.render_widget(
        Paragraph::new(ratatui::text::Line::from(spans)),
        Rect { x: text_x, y: area.y, width: text_w, height: 1 },
    );
    if area.height > 1 {
        frame.render_widget(
            Paragraph::new(ratatui::text::Line::styled(
                truncate(&card.subtitle, text_w),
                palette.subtitle(),
            )),
            Rect { x: text_x, y: area.y + 1, width: text_w, height: 1 },
        );
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_selected_cell_carries_the_same_band_as_a_track_row() {
        // The list drew a band with softened ends and the grid drew a hard
        // rectangle, so one selection looked like two different marks
        // depending on which view the track was in.
        let cards = cards(3);
        let buf = geometry::draw(60, 8, move |f, area, p| {
            render(f, area, p, &cards, Some(0), no_marks(), |_, _, _, _| false)
        });
        let text = geometry::text(&buf);
        assert!(text.contains('▐'), "a cap on the left:\n{text}");
        assert!(text.contains('▌'), "and one on the right:\n{text}");
    }

    #[test]
    fn a_playing_track_and_a_favourite_are_marked_here_too() {
        // The home page showed neither, so the same track read differently
        // depending on which view it was in.
        let id = crate::domain::TrackId(7);
        let mut card = Card::new("A Track", "An Artist");
        card.target = Some(crate::shell::carousel::Target::Track(7));
        let cards = vec![card];

        let favourites: std::collections::HashSet<_> = [id].into_iter().collect();
        let buf = geometry::draw(60, 8, move |f, area, p| {
            render(
                f,
                area,
                p,
                &cards,
                None,
                Marks {
                    favourites: &favourites,
                    playing: Some(id),
                    tier: crate::shell::nowplaying::Tier::default(),
                },
                |_, _, _, _| false,
            )
        });
        let text = geometry::text(&buf);
        assert!(text.contains('♪'), "the playing track is marked:\n{text}");
        assert!(text.contains('♥'), "and the favourite:\n{text}");
    }

    #[test]
    fn a_track_that_is_neither_gets_no_marks() {
        // The marks have to mean something: drawn on every cell they would
        // say nothing at all.
        let mut card = Card::new("A Track", "An Artist");
        card.target = Some(crate::shell::carousel::Target::Track(7));
        let cards = vec![card];
        let buf = geometry::draw(60, 8, move |f, area, p| {
            render(f, area, p, &cards, None, no_marks(), |_, _, _, _| false)
        });
        let text = geometry::text(&buf);
        assert!(!text.contains('♪'), "nothing is playing:\n{text}");
        assert!(!text.contains('♥'), "and it is not a favourite:\n{text}");
    }

    /// No favourites, nothing playing — what most of these tests want.
    fn no_marks() -> Marks<'static> {
        use std::sync::OnceLock;
        static EMPTY: OnceLock<std::collections::HashSet<crate::domain::TrackId>> =
            OnceLock::new();
        Marks {
            favourites: EMPTY.get_or_init(Default::default),
            playing: None,
            tier: super::super::nowplaying::Tier::default(),
        }
    }
    use super::*;
    use crate::shell::geometry;

    fn cards(n: usize) -> Vec<Card> {
        (0..n)
            .map(|i| Card::new(format!("Track {i}"), format!("Artist {i}")))
            .collect()
    }

    fn draw(width: u16, height: u16, cards: &[Card]) -> ratatui::buffer::Buffer {
        let cards = cards.to_vec();
        geometry::draw(width, height, move |f, area, p| {
            render(f, area, p, &cards, None, no_marks(), |_, _, _, _| false)
        })
    }

    #[test]
    fn what_the_grid_draws_is_what_movement_may_reach() {
        // The count lived in three places — the renderer's `take`, stepping
        // down through a row, and entering one from below — and two of them
        // were wrong in different ways. One answer now, and this checks it
        // against what actually lands in the buffer.
        for width in [40u16, 60, 80, 114, 200] {
            for held in [0usize, 1, 5, 9, 30] {
                let cols = columns(width);
                let all = cards(held);
                let buf = draw(width, height() + 2, &all);
                let text = geometry::text(&buf);
                let painted = (0..held)
                    .filter(|i| text.contains(&format!("Track {i}")))
                    .count();
                assert_eq!(
                    painted,
                    drawn(held, cols),
                    "at width {width} with {held} cards: drew {painted}, \
                     movement may reach {}",
                    drawn(held, cols)
                );
            }
        }
    }

    #[test]
    fn a_full_grid_is_three_across_and_three_down() {
        // The shape the web client draws these in: nine tracks, counted off
        // the running client.
        let n = COLUMNS * ROWS;
        let all = cards(n + 4);
        let buf = draw(114, height(), &all);
        let text = geometry::text(&buf);
        for i in 0..n {
            assert!(text.contains(&format!("Track {i}")), "Track {i} is drawn:\n{text}");
        }
        assert!(
            !text.contains(&format!("Track {n}")),
            "and the one past the grid is not:\n{text}"
        );
    }

    #[test]
    fn a_thumbnail_is_square_at_the_terminals_own_cell() {
        // It used to be a fixed six by three, which is square only when a
        // cell is exactly twice as tall as it is wide. It is scaled from
        // the card's cover now, so one measurement drives both.
        assert_eq!(THUMB_H, 3);
        let card = crate::shell::carousel::card_width();
        assert_eq!(
            thumb_w(),
            (THUMB_H * card).div_ceil(crate::shell::carousel::COVER_HEIGHT),
            "the same proportion as a card's cover"
        );
    }

    #[test]
    fn the_selected_cell_is_shaded_thumbnail_and_all() {
        // The same mark a carousel card gets, since the same key opens
        // both. Marking the title alone said the title was picked.
        let palette = crate::shell::theme::Palette::detect();
        let all = cards(4);
        let buf = geometry::draw(114, 12, move |f, area, p| {
            render(f, area, p, &all, Some(1), no_marks(), |_, _, _, _| false)
        });

        let second = geometry::find(&buf, "Track 1").expect("the selected cell");
        let first = geometry::find(&buf, "Track 0").expect("the other");
        assert_eq!(
            buf[(second.start, second.row)].bg,
            palette.selection,
            "the selected cell is shaded\n{}",
            geometry::text(&buf)
        );
        assert_ne!(
            buf[(first.start, first.row)].bg,
            palette.selection,
            "and its neighbour is not"
        );

        // The gutter between the thumbnail and the title, which is the
        // cell's own background rather than the artwork's. The thumbnail's
        // columns are not checked: a track with no cover paints its
        // placeholder there, and a real one paints pixels -- either way the
        // shade is covered, which showed in 256 colours where the two greys
        // are one index apart rather than rounding together.
        assert_eq!(
            buf[(second.start - 2, second.row)].bg,
            palette.selection,
            "the shade reaches past the title, up to the thumbnail\n{}",
            geometry::text(&buf)
        );
    }

    #[test]
    fn the_artist_sits_under_the_title_not_beside_it() {
        let all = cards(1);
        let buf = draw(114, 12, &all);
        let title = geometry::find(&buf, "Track 0").expect("the title");
        let artist = geometry::find(&buf, "Artist 0").expect("the artist");
        assert_eq!(artist.row, title.row + 1, "the artist is on the next line");
        assert_eq!(artist.start, title.start, "and starts in the same column");
    }

    #[test]
    fn the_text_clears_the_thumbnail() {
        let all = cards(1);
        let buf = draw(114, 12, &all);
        let title = geometry::find(&buf, "Track 0").expect("the title");
        assert!(
            title.start >= thumb_w() + TEXT_GAP,
            "the title starts after the thumbnail and its gutter, at {}",
            title.start
        );
    }

    #[test]
    fn the_columns_are_evenly_spaced() {
        let all = cards(3);
        let buf = draw(114, 12, &all);
        let first = geometry::find(&buf, "Track 0").expect("first");
        let second = geometry::find(&buf, "Track 1").expect("second");
        let third = geometry::find(&buf, "Track 2").expect("third");
        assert_eq!(first.row, second.row, "the first three share a row");
        assert_eq!(second.row, third.row);
        assert_eq!(
            second.start - first.start,
            third.start - second.start,
            "the pitch between columns is the same"
        );
    }

    #[test]
    fn a_narrow_pane_uses_fewer_columns_rather_than_squeezing_three() {
        assert_eq!(columns(114), 3);
        assert_eq!(columns(60), 3);
        assert_eq!(columns(40), 2);
        assert_eq!(columns(20), 1);
        assert_eq!(columns(4), 1, "never zero, so a card is always attempted");
    }

    #[test]
    fn rendering_into_any_size_does_not_panic() {
        let all = cards(9);
        for (w, h) in [(1, 1), (10, 3), (40, 6), (114, 12), (200, 40)] {
            let _ = draw(w, h, &all);
        }
    }

    #[test]
    fn a_short_pane_draws_what_fits_and_stops() {
        // Two rows of cells in six lines, not three.
        let all = cards(9);
        let buf = draw(114, 6, &all);
        let text = geometry::text(&buf);
        assert!(text.contains("Track 0"));
        assert!(
            !text.contains("Track 6"),
            "the third row of cells does not fit:\n{text}"
        );
    }
}
