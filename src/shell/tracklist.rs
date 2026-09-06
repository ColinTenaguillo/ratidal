use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Row, Table, TableState};
use ratatui::Frame;

use crate::domain::Track;

#[derive(Debug, Default)]
pub struct TrackListState {
    pub selected: usize,
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
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    tracks: &[Track],
    state: &TrackListState,
    focused: bool,
) {
    let rows: Vec<Row> = tracks
        .iter()
        .map(|t| {
            Row::new(vec![
                if t.is_hires() { "HI" } else { "" }.to_string(),
                t.title.clone(),
                t.artist.clone(),
                super::nowplaying::format_time(t.duration),
            ])
        })
        .collect();

    let mut table_state = TableState::default();
    table_state.select(Some(state.selected.min(tracks.len().saturating_sub(1))));

    let highlight = if focused {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };

    frame.render_stateful_widget(
        Table::new(
            rows,
            [
                Constraint::Length(3),
                Constraint::Percentage(50),
                Constraint::Percentage(35),
                Constraint::Length(6),
            ],
        )
        .block(Block::default().borders(Borders::NONE))
        .row_highlight_style(highlight),
        area,
        &mut table_state,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
