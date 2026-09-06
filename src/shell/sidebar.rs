use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Playlists,
    Albums,
    Tracks,
}

impl Section {
    pub const ALL: [Section; 3] = [Section::Playlists, Section::Albums, Section::Tracks];

    pub fn label(&self) -> &'static str {
        match self {
            Section::Playlists => "Playlists",
            Section::Albums => "Albums",
            Section::Tracks => "Tracks",
        }
    }
}

#[derive(Debug, Default)]
pub struct SidebarState {
    index: usize,
}

impl SidebarState {
    pub fn section(&self) -> Section {
        Section::ALL[self.index.min(Section::ALL.len() - 1)]
    }

    /// Clamps at the last entry rather than wrapping.
    pub fn next(&mut self) {
        self.index = (self.index + 1).min(Section::ALL.len() - 1);
    }

    pub fn previous(&mut self) {
        self.index = self.index.saturating_sub(1);
    }
}

pub fn render(frame: &mut Frame, area: Rect, state: &SidebarState, focused: bool) {
    let items: Vec<ListItem> = Section::ALL
        .iter()
        .map(|s| ListItem::new(s.label()))
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(state.index));

    let highlight = if focused {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };

    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::RIGHT).title("Collection"))
            .highlight_style(highlight),
        area,
        &mut list_state,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_through_sections_and_stops_at_the_ends() {
        let mut s = SidebarState::default();
        assert_eq!(s.section(), Section::Playlists);

        s.next();
        assert_eq!(s.section(), Section::Albums);
        s.next();
        assert_eq!(s.section(), Section::Tracks);

        // Clamps rather than wrapping: wrapping past the end surprises users.
        s.next();
        assert_eq!(s.section(), Section::Tracks);

        s.previous();
        assert_eq!(s.section(), Section::Albums);
        s.previous();
        s.previous();
        assert_eq!(s.section(), Section::Playlists);
    }
}
