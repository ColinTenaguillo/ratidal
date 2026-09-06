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
/// Rows of cells.
pub const ROWS: usize = 3;
/// A thumbnail is square: four columns wide is two rows tall at a cell's
/// aspect.
const THUMB_W: u16 = 4;
const THUMB_H: u16 = 2;
/// Between the thumbnail and its text.
const TEXT_GAP: u16 = 2;
/// Between one cell and the next, across and down.
const CELL_GAP_X: u16 = 2;
const CELL_GAP_Y: u16 = 1;
/// A cell is its thumbnail plus a blank line under it.
const CELL_HEIGHT: u16 = THUMB_H + CELL_GAP_Y;

/// How tall a full grid is, so the caller can lay out what follows.
pub fn height() -> u16 {
    ROWS as u16 * CELL_HEIGHT
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

    for (i, card) in cards.iter().take(cols * ROWS).enumerate() {
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
            &mut draw_cover,
        );
    }
}

fn render_cell<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    card: &Card,
    selected: bool,
    draw_cover: &mut F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    let thumb_w = THUMB_W.min(area.width);
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
    let title_style = if selected {
        palette.row_focused()
    } else {
        palette.title()
    };
    frame.render_widget(
        Paragraph::new(ratatui::text::Line::styled(
            truncate(&card.title, text_w),
            title_style,
        )),
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
            render(f, area, p, &cards, None, |_, _, _, _| false)
        })
    }

    #[test]
    fn a_full_grid_is_three_by_three() {
        let all = cards(12);
        let buf = draw(114, 12, &all);
        let text = geometry::text(&buf);
        for i in 0..9 {
            assert!(text.contains(&format!("Track {i}")), "Track {i} is drawn:\n{text}");
        }
        assert!(
            !text.contains("Track 9"),
            "a tenth track does not fit a three-by-three grid:\n{text}"
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
            title.start >= THUMB_W + TEXT_GAP,
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
