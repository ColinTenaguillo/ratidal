//! The left navigation column, following the web client's structure:
//! three top entries, a Collection group, then the user's playlists with
//! their item counts.

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::theme::Palette;
use crate::library::Playlist;

/// Every navigable entry, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Music,
    Explore,
    Feed,
    MixesAndRadio,
    Playlists,
    Albums,
    Tracks,
    Videos,
    Profiles,
}

impl Section {
    pub const ALL: [Section; 9] = [
        Section::Music,
        Section::Explore,
        Section::Feed,
        Section::MixesAndRadio,
        Section::Playlists,
        Section::Albums,
        Section::Tracks,
        Section::Videos,
        Section::Profiles,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Section::Music => "Music",
            Section::Explore => "Explore",
            Section::Feed => "Feed",
            Section::MixesAndRadio => "Mixes & Radio",
            Section::Playlists => "Playlists",
            Section::Albums => "Albums",
            Section::Tracks => "Tracks",
            Section::Videos => "Videos",
            Section::Profiles => "Profiles",
        }
    }

    /// A nerd-font-free glyph, so the sidebar reads on any terminal.
    pub fn icon(&self) -> &'static str {
        match self {
            Section::Music => "♫",
            Section::Explore => "⊕",
            Section::Feed => "◔",
            Section::MixesAndRadio => "◉",
            Section::Playlists => "≣",
            Section::Albums => "◎",
            Section::Tracks => "♪",
            Section::Videos => "▷",
            Section::Profiles => "☺",
        }
    }

    /// True for the entries under the "Collection" heading.
    fn in_collection(&self) -> bool {
        !matches!(self, Section::Music | Section::Explore | Section::Feed)
    }
}

#[derive(Debug, Default)]
pub struct SidebarState {
    index: usize,
    /// First playlist row drawn, so a long list can scroll.
    playlist_offset: usize,
}

impl SidebarState {
    pub fn section(&self) -> Section {
        Section::ALL[self.index.min(Section::ALL.len() - 1)]
    }

    /// Clamps rather than wrapping: running off the end of a nav list and
    /// reappearing at the top is disorienting.
    pub fn next(&mut self) {
        self.index = (self.index + 1).min(Section::ALL.len() - 1);
    }

    pub fn previous(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    /// Jump straight to a section, as opening a playlist does.
    pub fn select(&mut self, section: Section) {
        if let Some(i) = Section::ALL.iter().position(|s| *s == section) {
            self.index = i;
        }
    }
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    state: &SidebarState,
    playlists: &[Playlist],
    focused: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    let selected = state.section();

    let mut collection_heading_drawn = false;
    for section in Section::ALL {
        // The "Collection" heading goes above the first entry that belongs to
        // it, so adding a section later does not need this to be re-counted.
        if section.in_collection() && !collection_heading_drawn {
            lines.push(Line::from(""));
            lines.push(Line::styled("  Collection", palette.section_heading()));
            collection_heading_drawn = true;
        }

        let style = if section == selected && focused {
            palette.nav_selected()
        } else if section == selected {
            palette.row_unfocused()
        } else {
            palette.nav_idle()
        };
        lines.push(Line::styled(
            format!("  {} {}", section.icon(), section.label()),
            style,
        ));
    }

    if !playlists.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::styled("  All playlists", palette.section_heading()));

        // Two lines per playlist: name, then the item count beneath it.
        let room = (area.height as usize).saturating_sub(lines.len());
        let rows = room / 2;
        for p in playlists.iter().skip(state.playlist_offset).take(rows) {
            lines.push(Line::styled(
                format!("  {}", clip(&p.title, area.width.saturating_sub(2))),
                palette.title(),
            ));
            lines.push(Line::styled(
                format!("  {} items", p.track_count),
                palette.subtitle(),
            ));
        }
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn clip(s: &str, width: u16) -> String {
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

    #[test]
    fn every_section_has_a_label_and_an_icon() {
        for s in Section::ALL {
            assert!(!s.label().is_empty());
            assert!(!s.icon().is_empty());
        }
    }

    #[test]
    fn the_collection_group_starts_at_mixes() {
        // The heading is drawn before the first collection entry, so the split
        // must land where the web client puts it.
        assert!(!Section::Music.in_collection());
        assert!(!Section::Explore.in_collection());
        assert!(!Section::Feed.in_collection());
        assert!(Section::MixesAndRadio.in_collection());
        assert!(Section::Profiles.in_collection());
    }

    #[test]
    fn navigation_clamps_at_both_ends() {
        let mut s = SidebarState::default();
        assert_eq!(s.section(), Section::Music);

        for _ in 0..50 {
            s.next();
        }
        assert_eq!(s.section(), Section::Profiles, "must stop at the last entry");

        for _ in 0..50 {
            s.previous();
        }
        assert_eq!(s.section(), Section::Music, "must stop at the first entry");
    }

    #[test]
    fn renders_with_playlists_in_a_short_pane_without_panicking() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let playlists: Vec<Playlist> = (0..40)
            .map(|i| Playlist::sample(&format!("A playlist with a rather long name {i}"), i * 7))
            .collect();
        let palette = Palette::detect();
        let state = SidebarState::default();

        // Far too short for the nav plus 40 playlists.
        let mut terminal = Terminal::new(TestBackend::new(26, 8)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &palette, &state, &playlists, true))
            .unwrap();
    }

    #[test]
    fn renders_in_a_one_cell_pane_without_panicking() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let palette = Palette::detect();
        let state = SidebarState::default();
        let mut terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &palette, &state, &[], false))
            .unwrap();
    }
}
