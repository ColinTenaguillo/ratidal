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
}

impl Setting {
    pub const ALL: [Setting; 2] = [Setting::Quality, Setting::Volume];

    fn label(&self) -> &'static str {
        match self {
            Setting::Quality => "Audio quality",
            Setting::Volume => "Volume",
        }
    }

    /// What the setting does, in the words a person would use.
    fn explain(&self) -> &'static str {
        match self {
            Setting::Quality => "What is asked for. The bar shows what arrived.",
            Setting::Volume => "Applied to the output, so full is untouched audio.",
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
    }
}

/// How much one press moves the volume. A twentieth: ten steps end to end
/// is a lot of pressing, and a hundred is a key that does nothing.
const VOLUME_STEP: f32 = 0.05;

/// The value column, as drawn.
fn value_of(config: &crate::config::Config, setting: Setting) -> String {
    match setting {
        Setting::Quality => describe(config.audio.quality()).to_string(),
        Setting::Volume => volume_bar(config.audio.volume),
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

/// Width of the label column, so the values line up.
const LABEL_WIDTH: usize = 16;

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
                    format!("  {:LABEL_WIDTH$}", setting.label()),
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
                    format!("  {:LABEL_WIDTH$}{}", "", setting.explain()),
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
