//! The palette, in one place.
//!
//! ratatui does not degrade truecolor on its own, and `Color::Rgb` renders as
//! glitched blinking text on terminals that cannot do it. So every colour is
//! resolved through [`Palette`], which picks RGB or an indexed approximation
//! once at startup based on `$COLORTERM`.

use ratatui::style::{Color, Modifier, Style};

/// Whether the terminal claimed truecolor support.
fn terminal_has_truecolor() -> bool {
    match std::env::var("COLORTERM") {
        Ok(v) => v.contains("truecolor") || v.contains("24bit"),
        Err(_) => false,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// TIDAL's green, used for the active nav item and the played portion of
    /// the progress bar.
    pub accent: Color,
    /// Primary text.
    pub text: Color,
    /// Secondary text: artists, subtitles, item counts.
    pub dim: Color,
    /// Section headings in the sidebar ("Collection", "Toutes les playlists").
    pub heading: Color,
    /// The panel behind a selected row or a shortcut card.
    pub surface: Color,
    /// Where a cover or avatar would be but is not. Deliberately lighter than
    /// `surface`: a placeholder the same colour as the background reads as a
    /// failure to draw rather than as an item with no artwork, and TIDAL has
    /// no picture for a large share of artists.
    pub placeholder: Color,
    /// Rules and separators.
    pub border: Color,
    /// Text on top of `accent`.
    pub on_accent: Color,
}

impl Palette {
    pub fn detect() -> Self {
        if terminal_has_truecolor() {
            Self {
                accent: Color::Rgb(0, 255, 178),
                text: Color::Rgb(255, 255, 255),
                dim: Color::Rgb(154, 154, 154),
                heading: Color::Rgb(120, 120, 120),
                surface: Color::Rgb(30, 30, 30),
                placeholder: Color::Rgb(58, 58, 58),
                border: Color::Rgb(48, 48, 48),
                on_accent: Color::Rgb(0, 0, 0),
            }
        } else {
            // 256-colour approximations. Terminals below that get the ANSI
            // fallbacks the indexed values themselves degrade to.
            Self {
                accent: Color::Indexed(48),
                text: Color::Indexed(255),
                dim: Color::Indexed(246),
                heading: Color::Indexed(243),
                surface: Color::Indexed(235),
                placeholder: Color::Indexed(240),
                border: Color::Indexed(238),
                on_accent: Color::Indexed(16),
            }
        }
    }

    pub fn title(&self) -> Style {
        Style::default()
            .fg(self.text)
            .add_modifier(Modifier::BOLD)
    }

    pub fn subtitle(&self) -> Style {
        Style::default().fg(self.dim)
    }

    /// A view's own title: "Listes de lecture", "Albums", "Titres". The web
    /// client sets these much larger than anything else on the page; a
    /// terminal has one size, so brightness and weight carry it instead.
    pub fn page_heading(&self) -> Style {
        Style::default()
            .fg(self.heading)
            .add_modifier(Modifier::BOLD)
    }

    /// The primary action button: TIDAL fills it and inverts the text.
    pub fn on_accent_pill(&self) -> Style {
        Style::default()
            .fg(self.on_accent)
            .bg(self.text)
            .add_modifier(Modifier::BOLD)
    }

    /// A secondary button, filled with the surface tint instead.
    pub fn pill(&self) -> Style {
        Style::default().fg(self.text).bg(self.surface)
    }

    pub fn section_heading(&self) -> Style {
        Style::default().fg(self.heading)
    }

    /// The active nav entry: TIDAL fills the row rather than colouring text.
    pub fn nav_selected(&self) -> Style {
        Style::default()
            .fg(self.text)
            .bg(self.surface)
            .add_modifier(Modifier::BOLD)
    }

    pub fn nav_idle(&self) -> Style {
        Style::default().fg(self.dim)
    }

    /// A selected row in a list or carousel, when that pane has focus.
    pub fn row_focused(&self) -> Style {
        Style::default().fg(self.on_accent).bg(self.accent)
    }

    /// The same row when the pane does not have focus — visible, but quiet.
    pub fn row_unfocused(&self) -> Style {
        Style::default().fg(self.text).bg(self.surface)
    }

    pub fn rule(&self) -> Style {
        Style::default().fg(self.border)
    }

    pub fn accent_text(&self) -> Style {
        Style::default().fg(self.accent)
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::detect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_indexed_palette_uses_no_rgb() {
        // A Color::Rgb reaching a terminal without truecolor renders as
        // glitched blinking text, so the fallback must contain none.
        // Read from the source rather than restated here: a copy would agree
        // with itself while the real fallback grew an RGB field unnoticed,
        // which is exactly what a new colour would do.
        let source = include_str!("theme.rs");
        let indexed = source
            .split("// 256-colour approximations")
            .nth(1)
            .expect("the indexed branch is marked by that comment");
        let indexed = &indexed[..indexed.find("\n        }").expect("end of the branch")];

        assert!(
            !indexed.contains("Color::Rgb"),
            "the indexed palette leaked an RGB colour:\n{indexed}"
        );
        // And it must set every field, or one silently keeps a default.
        for field in ["accent", "text", "dim", "heading", "surface", "placeholder",
                      "border", "on_accent"] {
            assert!(
                indexed.contains(&format!("{field}:")),
                "the indexed palette does not set {field}"
            );
        }
    }

    #[test]
    fn focused_and_unfocused_selections_differ() {
        // If these ever collapse, the user cannot tell which pane has focus.
        let p = Palette::detect();
        assert_ne!(p.row_focused(), p.row_unfocused());
    }
}
