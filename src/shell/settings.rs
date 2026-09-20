//! The Settings section: what the app can be told, in the app.
//!
//! Everything here is written straight back to `config.toml`, so a setting
//! changed in a session is still set in the next one. The file stays the
//! authority — this view edits it rather than shadowing it.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::theme::Palette;
use crate::domain::Quality;

/// A row of the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Quality,
    Volume,
    Autoplay,
    Explicit,
    Ai,
    NerdFont,
    Halfblocks,
}

impl Setting {
    pub const ALL: [Setting; 7] = [
        Setting::Quality,
        Setting::Volume,
        Setting::Autoplay,
        Setting::Explicit,
        Setting::Ai,
        Setting::NerdFont,
        Setting::Halfblocks,
    ];

    fn label(&self) -> &'static str {
        match self {
            Setting::Quality => "Audio quality",
            Setting::Volume => "Volume",
            Setting::Autoplay => "Autoplay",
            Setting::Explicit => "Allow explicit content",
            Setting::Ai => "Allow AI-generated content",
            Setting::NerdFont => "Nerd font icons",
            Setting::Halfblocks => "Draw covers as half blocks",
        }
    }

    /// What the setting does, in the words a person would use.
    fn explain(&self) -> &'static str {
        match self {
            Setting::Quality => "What is asked for. The bar shows what arrived.",
            Setting::Volume => "Applied to the output, so full is untouched audio.",
            Setting::Autoplay => "Keep playing something similar when the queue runs out.",
            Setting::Explicit => "Off, tracks marked E will not play.",
            Setting::Ai => "Off, tracks marked AI will not play.",
            Setting::NerdFont => "Needs a nerd font. Empty boxes mean you have none.",
            // Named for the terminal it was added for: the setting is only
            // worth finding if the row says what it is for.
            Setting::Halfblocks => {
                "On if your terminal draws covers in the wrong place. Takes effect at the next start."
            }
        }
    }
}

/// The qualities in the order the view steps through them, best first.
///
/// TIDAL's own order, and the one that matters: a user moving down this
/// list is giving something up, and it should read that way.
pub const QUALITIES: [Quality; 4] = [
    Quality::HiResLossless,
    Quality::Lossless,
    Quality::High,
    Quality::Low,
];

/// How each quality reads to someone who is not reading the API docs.
pub fn describe(q: Quality) -> &'static str {
    match q {
        Quality::HiResLossless => "Max — FLAC up to 24-bit",
        Quality::Lossless => "High — FLAC, CD quality",
        Quality::High => "Low — AAC 320k",
        Quality::Low => "Lowest — AAC 96k",
    }
}

#[derive(Debug, Default)]
pub struct SettingsState {
    pub selected: usize,
}

impl SettingsState {
    pub fn next(&mut self) {
        self.selected = (self.selected + 1).min(Setting::ALL.len().saturating_sub(1));
    }

    pub fn previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn current(&self) -> Setting {
        Setting::ALL
            .get(self.selected)
            .copied()
            .unwrap_or(Setting::Quality)
    }
}

/// Step the current setting's value along by one, and say what it became.
///
/// Wrapping rather than stopping at the ends: there are four qualities and
/// a list that stops needs two keys to walk, which is more chrome than the
/// setting is worth.
pub fn cycle(config: &mut crate::config::Config, setting: Setting, forward: bool) {
    match setting {
        Setting::Quality => {
            let current = config.audio.quality();
            let at = QUALITIES.iter().position(|q| *q == current).unwrap_or(0);
            let n = QUALITIES.len();
            let next = if forward { at + 1 } else { at + n - 1 };
            config.audio.set_quality(QUALITIES[next % n]);
        }
        // A step rather than a wrap: turning the volume up past full and
        // round to silence is not what anyone means by the key.
        Setting::Volume => {
            let step = if forward { VOLUME_STEP } else { -VOLUME_STEP };
            config.audio.volume = (config.audio.volume + step).clamp(0.0, 1.0);
        }
        // Toggles: either direction flips them, since there are two states
        // and a key that only turned one on would be half a key.
        Setting::Autoplay => config.playback.autoplay = !config.playback.autoplay,
        Setting::Explicit => config.playback.explicit = !config.playback.explicit,
        Setting::Ai => config.playback.ai = !config.playback.ai,
        Setting::NerdFont => {
            config.ui.nerd_font = !config.ui.nerd_font;
            // Applied at once rather than at the next start: the icons are
            // on screen while the setting is being changed, and seeing
            // whether the font has them is the whole point of the row.
            super::icons::set_nerd_font(config.ui.nerd_font);
        }
        // Not applied at once, unlike the icons above: the image protocol
        // is probed before the terminal is put into raw mode, so the picker
        // this would change was built before the app drew anything.
        Setting::Halfblocks => config.ui.halfblocks = !config.ui.halfblocks,
    }
}

/// A toggle's value column.
fn on_off(on: bool) -> String {
    if on { "On" } else { "Off" }.to_string()
}

/// How much one press moves the volume. A twentieth: ten steps end to end
/// is a lot of pressing, and a hundred is a key that does nothing.
const VOLUME_STEP: f32 = 0.05;

/// The value column, as drawn.
fn value_of(config: &crate::config::Config, setting: Setting) -> String {
    match setting {
        Setting::Quality => describe(config.audio.quality()).to_string(),
        Setting::Volume => volume_bar(config.audio.volume),
        Setting::Autoplay => on_off(config.playback.autoplay),
        Setting::Explicit => on_off(config.playback.explicit),
        Setting::Ai => on_off(config.playback.ai),
        // Drawn with the icons themselves: a terminal cannot be asked what
        // font it has, so the row shows the glyphs and lets the user see.
        Setting::NerdFont => {
            if config.ui.nerd_font {
                format!(
                    "On   {} {} {}",
                    super::icons::music(),
                    super::icons::albums(),
                    super::icons::favourite()
                )
            } else {
                "Off".to_string()
            }
        }
        Setting::Halfblocks => on_off(config.ui.halfblocks),
    }
}

/// The volume as a bar and a percentage.
///
/// A number alone says what it is; the bar says where it sits between the
/// ends, which is what the key is moving.
fn volume_bar(v: f32) -> String {
    const CELLS: usize = 10;
    let v = v.clamp(0.0, 1.0);
    let full = (v * CELLS as f32).round() as usize;
    format!(
        "{}{}  {:>3}%",
        "█".repeat(full),
        "░".repeat(CELLS - full),
        (v * 100.0).round() as u32
    )
}

/// Width of the label column, so the values line up in one column.
///
/// Measured from the labels rather than written down: a fixed width was one
/// short of the longest label, so two rows had their value pressed against
/// the text with no gap at all. Adding a setting cannot break the alignment
/// now.
fn label_width() -> usize {
    const GAP: usize = 2;
    Setting::ALL
        .iter()
        .map(|s| s.label().chars().count())
        .max()
        .unwrap_or(0)
        + GAP
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    config: &crate::config::Config,
    state: &SettingsState,
    focused: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(
        Paragraph::new(Line::styled("Settings", palette.page_heading())),
        Rect { height: 1, ..area },
    );

    let mut y = area.y + 2;
    let bottom = area.y + area.height;
    for (i, setting) in Setting::ALL.iter().enumerate() {
        if y >= bottom {
            break;
        }
        let selected = focused && i == state.selected;
        // The same grey band a selected row gets everywhere else, so a
        // selection here reads as the selections do in the rest of the app.
        if selected {
            frame.render_widget(
                ratatui::widgets::Block::default()
                    .style(ratatui::style::Style::default().bg(palette.selection)),
                Rect { x: area.x, y, width: area.width, height: 1 },
            );
        }
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!("  {:width$}", setting.label(), width = label_width()),
                    palette.title(),
                ),
                Span::styled(value_of(config, *setting), palette.accent_text()),
            ])),
            Rect { x: area.x, y, width: area.width, height: 1 },
        );
        y += 1;

        if y < bottom {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    format!("  {:width$}{}", "", setting.explain(), width = label_width()),
                    palette.subtitle(),
                )),
                Rect { x: area.x, y, width: area.width, height: 1 },
            );
            y += 2;
        }
    }

    if y < bottom {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "  h l  change      saved to config.toml",
                palette.subtitle(),
            )),
            Rect { x: area.x, y, width: area.width, height: 1 },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn the_three_content_settings_are_toggles_with_tidals_own_defaults() {
        let mut config = crate::config::Config::default();

        // TIDAL's own: explicit and AI allowed, autoplay on. Turning
        // something off is a choice the user makes.
        assert!(config.playback.explicit, "explicit starts allowed");
        assert!(config.playback.ai, "so does AI");
        assert!(config.playback.autoplay, "the music carries on by default");

        for (setting, read) in [
            (Setting::Autoplay, (|c: &crate::config::Config| c.playback.autoplay)
                as fn(&crate::config::Config) -> bool),
            (Setting::Explicit, |c| c.playback.explicit),
            (Setting::Ai, |c| c.playback.ai),
        ] {
            let before = read(&config);
            cycle(&mut config, setting, true);
            assert_ne!(read(&config), before, "{setting:?} did not flip");
            // Either direction: two states, so a key that only turned it on
            // would be half a key.
            cycle(&mut config, setting, false);
            assert_eq!(read(&config), before, "{setting:?} did not flip back");
        }
    }

    #[test]
    fn a_toggle_reads_on_or_off_rather_than_true_or_false() {
        let mut config = crate::config::Config::default();
        config.playback.autoplay = false;
        assert_eq!(value_of(&config, Setting::Autoplay), "Off");
        config.playback.autoplay = true;
        assert_eq!(value_of(&config, Setting::Autoplay), "On");
    }

    #[test]
    fn every_value_starts_on_the_same_column() {
        // Reported as the value pressed against the label with no gap. The
        // label column was a fixed 16, one short of "Allow AI-generated
        // content" at 26 -- so two rows ran into their own value. Measured
        // from the labels now, so adding a setting cannot break it.
        let fixed = crate::shell::icons::Fixed::at(false);
        let _ = &fixed;
        let config = crate::config::Config::default();
        let state = SettingsState::default();
        let buf = crate::shell::geometry::draw(120, 24, |f, area, palette| {
            render(f, area, palette, &config, &state, false);
        });
        let text = crate::shell::geometry::text(&buf);

        let mut columns = Vec::new();
        for setting in Setting::ALL {
            let label = setting.label();
            let row = text
                .lines()
                .find(|l| l.contains(label))
                .unwrap_or_else(|| panic!("{label} is drawn"));
            let at = row.find(label).expect("the label");
            // Byte offsets added to a char count: they agree only while
            // nothing left of the value is multi-byte, which is why the
            // labels are all ASCII.
            let value_at = row[at + label.len()..]
                .find(|c: char| !c.is_whitespace())
                .map(|off| at + label.chars().count() + off);
            columns.push((label, value_at));
        }

        let first = columns[0].1.expect("the first row has a value");
        for (label, at) in &columns {
            let at = at.unwrap_or_else(|| panic!("{label} has no value beside it"));
            assert_eq!(
                at, first,
                "{label:?} starts its value at {at}, the first row at {first}:\n{text}"
            );
        }

        // And there is a gap: a value against the text is what was reported.
        let longest = Setting::ALL
            .iter()
            .map(|s| s.label().chars().count())
            .max()
            .unwrap_or(0);
        assert!(
            first > longest,
            "the value column starts at {first}, inside the longest label at {longest}"
        );
    }

    #[test]
    fn the_halfblocks_row_toggles_and_is_off_by_default() {
        // Added for WezTerm 20240203, which says it speaks iTerm2 and then
        // draws the first row's covers somewhere other than where it was
        // told -- that row is left blank while every row below it is fine.
        // A terminal that draws images correctly should keep using them, so
        // this is off until it is asked for.
        let mut config = crate::config::Config::default();
        assert!(!config.ui.halfblocks, "a working terminal draws images");

        cycle(&mut config, Setting::Halfblocks, true);
        assert!(config.ui.halfblocks, "the setting turned on");
        assert_eq!(value_of(&config, Setting::Halfblocks), "On");

        // Either direction flips it, as the other toggles do.
        cycle(&mut config, Setting::Halfblocks, false);
        assert!(!config.ui.halfblocks, "and back again");
        assert_eq!(value_of(&config, Setting::Halfblocks), "Off");
    }

    #[test]
    fn the_nerd_font_row_toggles_the_icons_as_it_is_changed() {
        // The point of the row is finding out whether the font is
        // installed, so the icons change while it is being chosen rather
        // than at the next start. There is no detecting it: a terminal does
        // not say what font it has, and a nerd font advances one cell just
        // as the replacement box does.
        let fixed = crate::shell::icons::Fixed::at(false);
        let _ = &fixed;
        let mut config = crate::config::Config::default();
        assert!(!config.ui.nerd_font, "off by default, as lazygit has it");

        let plain = crate::shell::icons::music();
        cycle(&mut config, Setting::NerdFont, true);
        assert!(config.ui.nerd_font, "the setting turned on");
        assert_ne!(
            crate::shell::icons::music(),
            plain,
            "and the icons changed with it"
        );

        // Either direction flips it: two states, so a key that only turned
        // it on would be half a key.
        cycle(&mut config, Setting::NerdFont, false);
        assert!(!config.ui.nerd_font, "and back again");
        assert_eq!(crate::shell::icons::music(), plain);
    }

    #[test]
    fn the_row_shows_the_icons_it_is_offering() {
        // "On" alone says nothing about whether they will draw. The row
        // carries a few of the glyphs so the answer is on screen -- which
        // is the whole reason there is no auto-detection here.
        let fixed = crate::shell::icons::Fixed::at(false);
        let mut config = crate::config::Config::default();

        config.ui.nerd_font = false;
        assert_eq!(value_of(&config, Setting::NerdFont), "Off");

        config.ui.nerd_font = true;
        fixed.set(true);
        let on = value_of(&config, Setting::NerdFont);
        assert!(on.starts_with("On"), "it says so: {on:?}");
        assert!(
            on.chars().any(|c| (0xE000..=0xF8FF).contains(&(c as u32))),
            "and shows the glyphs themselves: {on:?}"
        );
    }
}
