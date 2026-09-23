//! The left navigation column, following the web client's structure:
//! three top entries, a Collection group, then the user's playlists with
//! their item counts.

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::theme::Palette;

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
    Profiles,
    Settings,
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
        Section::Profiles,
        Section::Settings,
    ];

    /// The key that reaches this entry: 1 for Music, 9 for Settings.
    ///
    /// There are exactly nine, so every one of them has a number and no
    /// number is spare.
    pub fn number(&self) -> usize {
        Self::ALL.iter().position(|s| s == self).unwrap_or(0) + 1
    }

    pub fn label(&self) -> &'static str {
        match self {
            Section::Music => "Music",
            Section::Explore => "Explore",
            Section::Feed => "Feed",
            Section::MixesAndRadio => "Mixes & Radio",
            Section::Playlists => "Playlists",
            Section::Albums => "Albums",
            Section::Tracks => "Tracks",
            Section::Profiles => "Profiles",
            Section::Settings => "Settings",
        }
    }

    /// The icon beside the name, from whichever set is in use.
    pub fn icon(&self) -> &'static str {
        use super::icons;
        match self {
            Section::Music => icons::music(),
            Section::Explore => icons::explore(),
            Section::Feed => icons::feed(),
            Section::MixesAndRadio => icons::mixes(),
            Section::Playlists => icons::playlists(),
            Section::Albums => icons::albums(),
            Section::Tracks => icons::tracks(),
            Section::Profiles => icons::profiles(),
            Section::Settings => icons::settings(),
        }
    }

    /// True for the entries under the "Collection" heading.
    ///
    /// Settings sits below the collection rather than in it: it is not a
    /// shelf of the user's music, it is where the app is configured.
    fn in_collection(&self) -> bool {
        !matches!(
            self,
            Section::Music | Section::Explore | Section::Feed | Section::Settings
        )
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
        // Padded to the pane's width so the selected row's background runs
        // the whole way across. A styled line only paints the cells its
        // text occupies, which left the highlight stopping mid-row.
        //
        // The number that reaches this entry is written at the end of it:
        // the key is no use to anyone who cannot see which is which.
        let number = section.number();
        lines.push(Line::styled(
            format!(
                "  {} {:width$}{number} ",
                section.icon(),
                section.label(),
                width = (area.width as usize).saturating_sub(6),
            ),
            style,
        ));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selected_entry_is_shaded_across_the_whole_pane() {
        // A styled line paints only the cells its text occupies, so the
        // highlight stopped where the label did.
        let state = SidebarState::default();
        let buf = crate::shell::geometry::draw(26, 14, move |f, area, p| {
            render(f, area, p, &state, true);
        });

        let row = crate::shell::geometry::find(&buf, "Music").expect("the entry");
        let shaded = (0..26)
            .filter(|x| buf[(*x, row.row)].bg != ratatui::style::Color::Reset)
            .count();
        assert_eq!(
            shaded,
            26,
            "every column of the row is shaded, found {shaded}\n{}",
            crate::shell::geometry::text(&buf)
        );
    }

    #[test]
    fn an_unselected_entry_is_not_shaded() {
        let state = SidebarState::default();
        let buf = crate::shell::geometry::draw(26, 14, move |f, area, p| {
            render(f, area, p, &state, true);
        });
        let row = crate::shell::geometry::find(&buf, "Explore").expect("the entry");
        let shaded = (0..26)
            .filter(|x| buf[(*x, row.row)].bg != ratatui::style::Color::Reset)
            .count();
        assert_eq!(shaded, 0, "only the selected row is shaded");
    }

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
        assert_eq!(
            s.section(),
            *Section::ALL.last().expect("the nav is not empty"),
            "must stop at the last entry"
        );

        for _ in 0..50 {
            s.previous();
        }
        assert_eq!(s.section(), Section::Music, "must stop at the first entry");
    }

    #[test]
    fn renders_in_a_pane_too_short_for_the_nav_without_panicking() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let palette = Palette::detect();
        let state = SidebarState::default();

        // Fewer rows than there are entries.
        let mut terminal = Terminal::new(TestBackend::new(26, 4)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &palette, &state, true))
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
            .draw(|f| render(f, f.area(), &palette, &state, false))
            .unwrap();
    }
}
