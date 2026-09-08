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
    /// The selected row's background. Lighter than `surface`, which is the
    /// tint a filter box or a placeholder uses — a selection the same colour
    /// as the furniture around it is not a selection.
    pub selection: Color,
    /// The hi-res badge. TIDAL marks its best tier in amber rather than the
    /// usual green, so it reads as a fact about the stream and not as
    /// another piece of chrome.
    pub quality: Color,
    /// The unplayed part of the progress bar. The web client draws it as
    /// white at 15% over black, which lands just under the frame's own
    /// border colour — dark enough to read as a groove rather than a rule,
    /// and distinct from the frame it sits inside.
    pub track: Color,
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
                selection: Color::Rgb(58, 58, 58),
                quality: Color::Rgb(255, 212, 50),
                placeholder: Color::Rgb(58, 58, 58),
                track: Color::Rgb(38, 38, 38),
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
                selection: Color::Indexed(239),
                quality: Color::Indexed(221),
                placeholder: Color::Indexed(240),
                track: Color::Indexed(236),
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

    /// The name at the top of an artist's page.
    ///
    /// White rather than the grey a section heading gets: it is the subject
    /// of the page, not a label for part of it.
    pub fn artist_name(&self) -> Style {
        Style::default().fg(self.text).add_modifier(Modifier::BOLD)
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

    /// The playing row: the web client tints its title in the quality's own
    /// colour rather than filling the row, so the mark reads as "this is
    /// what is playing, at this quality" in one glance.
    pub fn playing_row(&self, tier: super::nowplaying::Tier) -> Style {
        use super::nowplaying::Tier;
        let fg = match tier {
            Tier::Max => self.quality,
            Tier::High => self.accent,
            Tier::Low => self.text,
        };
        Style::default().fg(fg).add_modifier(Modifier::BOLD)
    }

    /// The delivered-quality badge, coloured by how good the stream is:
    /// amber for hi-res, green for lossless, plain for anything less. The
    /// point of the badge is telling those apart at a glance.
    pub fn quality_badge(&self, tier: super::nowplaying::Tier) -> Style {
        use super::nowplaying::Tier;
        let fg = match tier {
            Tier::Max => self.quality,
            Tier::High => self.accent,
            Tier::Low => self.text,
        };
        // No background: a filled chip drew the eye harder than the track
        // name beside it, and the colour already says which tier this is.
        Style::default().fg(fg).add_modifier(Modifier::BOLD)
    }

    pub fn section_heading(&self) -> Style {
        Style::default().fg(self.heading)
    }

    /// The active nav entry: TIDAL fills the row rather than colouring text.
    pub fn nav_selected(&self) -> Style {
        // The same band a selected row gets, so "where I am" looks the same
        // in both panes.
        Style::default()
            .fg(self.text)
            .bg(self.selection)
            .add_modifier(Modifier::BOLD)
    }

    pub fn nav_idle(&self) -> Style {
        Style::default().fg(self.dim)
    }

    /// A selected row in a list or carousel, when that pane has focus.
    pub fn row_focused(&self) -> Style {
        // A grey band rather than the accent: a full row of the accent
        // colour shouts over the artwork it sits beside, and the accent is
        // what marks the things that are actually on — a favourite, a mode,
        // the heading of the row with the keys.
        Style::default().fg(self.text).bg(self.selection)
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

    /// A passing message in the top right: a mode that has just changed, or
    /// an error. On its own background, since it sits over whatever the view
    /// had drawn there.
    pub fn notice(&self) -> Style {
        Style::default().fg(self.on_accent).bg(self.accent)
    }

    /// The play/pause button: the brightest thing in the transport row.
    ///
    /// The web client draws it about half again the size of the controls
    /// either side of it. A terminal has one glyph size, so the emphasis is
    /// carried by weight and by being the only white in the row.
    pub fn play_button(&self) -> Style {
        Style::default()
            .fg(self.text)
            .add_modifier(Modifier::BOLD)
    }

    /// The small marks that trail a track's title — favourite, explicit.
    /// Dimmed, so they read as annotations on the title rather than as
    /// competing with it.
    pub fn mark(&self) -> Style {
        Style::default()
            .fg(self.dim)
            .add_modifier(Modifier::DIM)
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::detect()
    }
}

/// The palette a theme name stands for.
///
/// `default` is TIDAL's own colours, detected against what the terminal can
/// show. Anything else is a named theme, drawn in truecolor: a theme is a
/// set of exact colours, and approximating one to 256 would be neither the
/// theme nor the default.
pub fn named(name: &str) -> Option<Palette> {
    match name.trim().to_lowercase().as_str() {
        "" | "default" => Some(Palette::detect()),
        "catppuccin" | "catppuccin-mocha" | "mocha" => Some(catppuccin_mocha()),
        "gruvbox" | "gruvbox-dark" => Some(gruvbox_dark()),
        "tokyonight" | "tokyo-night" | "tokyo_night" => Some(tokyo_night()),
        "nord" => Some(nord()),
        "dracula" => Some(dracula()),
        "solarized" | "solarized-dark" => Some(solarized_dark()),
        _ => None,
    }
}

/// The names a theme may be given, for the message when one is misspelt.
pub const THEMES: [&str; 7] = [
    "default",
    "catppuccin",
    "gruvbox",
    "tokyonight",
    "nord",
    "dracula",
    "solarized",
];

/// Catppuccin Mocha, from the palette the project publishes.
///
/// Mapped by the role each colour plays here rather than by name: mauve is
/// the accent because it is the flavour's own highlight, and the three
/// surface steps line up with the three greys the layout already uses.
fn catppuccin_mocha() -> Palette {
    Palette {
        accent: Color::Rgb(0xcb, 0xa6, 0xf7),
        text: Color::Rgb(0xcd, 0xd6, 0xf4),
        dim: Color::Rgb(0xa6, 0xad, 0xc8),
        heading: Color::Rgb(0x7f, 0x84, 0x9c),
        surface: Color::Rgb(0x31, 0x32, 0x44),
        selection: Color::Rgb(0x45, 0x47, 0x5a),
        quality: Color::Rgb(0xf9, 0xe2, 0xaf),
        placeholder: Color::Rgb(0x58, 0x5b, 0x70),
        track: Color::Rgb(0x18, 0x18, 0x25),
        border: Color::Rgb(0x45, 0x47, 0x5a),
        on_accent: Color::Rgb(0x1e, 0x1e, 0x2e),
    }
}

/// Gruvbox dark, from the values the scheme itself defines.
///
/// Orange is the accent: gruvbox's own highlight, and the one colour in the
/// palette that carries at a single cell. The four `dark` steps give the
/// layout its greys without inventing any.
fn gruvbox_dark() -> Palette {
    Palette {
        accent: Color::Rgb(0xfe, 0x80, 0x19),
        text: Color::Rgb(0xeb, 0xdb, 0xb2),
        dim: Color::Rgb(0xa8, 0x99, 0x84),
        heading: Color::Rgb(0x92, 0x83, 0x74),
        surface: Color::Rgb(0x3c, 0x38, 0x36),
        selection: Color::Rgb(0x50, 0x49, 0x45),
        quality: Color::Rgb(0xfa, 0xbd, 0x2f),
        placeholder: Color::Rgb(0x66, 0x5c, 0x54),
        track: Color::Rgb(0x1d, 0x20, 0x21),
        border: Color::Rgb(0x50, 0x49, 0x45),
        on_accent: Color::Rgb(0x28, 0x28, 0x28),
    }
}

/// Tokyo Night, the Storm variant.
///
/// Storm rather than Night: its background is a step lighter, which leaves
/// room for the two darker tones the progress bar and the panels need.
fn tokyo_night() -> Palette {
    Palette {
        accent: Color::Rgb(0x7a, 0xa2, 0xf7),
        text: Color::Rgb(0xc0, 0xca, 0xf5),
        dim: Color::Rgb(0xa9, 0xb1, 0xd6),
        heading: Color::Rgb(0x56, 0x5f, 0x89),
        surface: Color::Rgb(0x29, 0x2e, 0x42),
        selection: Color::Rgb(0x3b, 0x42, 0x61),
        quality: Color::Rgb(0xe0, 0xaf, 0x68),
        placeholder: Color::Rgb(0x41, 0x48, 0x68),
        track: Color::Rgb(0x1f, 0x23, 0x35),
        border: Color::Rgb(0x3b, 0x42, 0x61),
        on_accent: Color::Rgb(0x1a, 0x1b, 0x26),
    }
}

/// Nord, from nord0 through nord15.
///
/// The frost blue is the accent. Nord publishes only four dark steps, so
/// `placeholder` takes nord3 and `border` sits with `selection` -- the
/// palette has no fifth tone to separate them, and inventing one would
/// make this something other than Nord.
fn nord() -> Palette {
    Palette {
        accent: Color::Rgb(0x88, 0xc0, 0xd0),
        text: Color::Rgb(0xec, 0xef, 0xf4),
        dim: Color::Rgb(0xd8, 0xde, 0xe9),
        heading: Color::Rgb(0x7b, 0x88, 0xa1),
        surface: Color::Rgb(0x3b, 0x42, 0x52),
        selection: Color::Rgb(0x43, 0x4c, 0x5e),
        quality: Color::Rgb(0xeb, 0xcb, 0x8b),
        placeholder: Color::Rgb(0x4c, 0x56, 0x6a),
        track: Color::Rgb(0x2e, 0x34, 0x40),
        border: Color::Rgb(0x43, 0x4c, 0x5e),
        on_accent: Color::Rgb(0x2e, 0x34, 0x40),
    }
}

/// Dracula, from the palette the theme publishes.
fn dracula() -> Palette {
    Palette {
        accent: Color::Rgb(0xbd, 0x93, 0xf9),
        text: Color::Rgb(0xf8, 0xf8, 0xf2),
        dim: Color::Rgb(0xb8, 0xbd, 0xd9),
        heading: Color::Rgb(0x62, 0x72, 0xa4),
        surface: Color::Rgb(0x34, 0x37, 0x46),
        selection: Color::Rgb(0x44, 0x47, 0x5a),
        quality: Color::Rgb(0xf1, 0xfa, 0x8c),
        placeholder: Color::Rgb(0x62, 0x72, 0xa4),
        track: Color::Rgb(0x21, 0x22, 0x2c),
        border: Color::Rgb(0x44, 0x47, 0x5a),
        on_accent: Color::Rgb(0x28, 0x2a, 0x36),
    }
}

/// Solarized dark, from the canonical sRGB values.
///
/// The one theme here that was built for syntax rather than for chrome: it
/// publishes two backgrounds and no greys between them, so `surface` and
/// `selection` are base02 and base01 -- the closest the palette has to the
/// two steps this layout needs, and further apart than they are elsewhere.
fn solarized_dark() -> Palette {
    Palette {
        accent: Color::Rgb(0x2a, 0xa1, 0x98),
        text: Color::Rgb(0x93, 0xa1, 0xa1),
        dim: Color::Rgb(0x83, 0x94, 0x96),
        heading: Color::Rgb(0x65, 0x7b, 0x83),
        surface: Color::Rgb(0x07, 0x36, 0x42),
        selection: Color::Rgb(0x0d, 0x4b, 0x5a),
        quality: Color::Rgb(0xb5, 0x89, 0x00),
        placeholder: Color::Rgb(0x58, 0x6e, 0x75),
        track: Color::Rgb(0x00, 0x2b, 0x36),
        border: Color::Rgb(0x0d, 0x4b, 0x5a),
        on_accent: Color::Rgb(0x00, 0x2b, 0x36),
    }
}

/// A `#rrggbb` colour, or `None` if it is not one.
///
/// The leading `#` is optional, since a bare six digits is what a config
/// file tends to grow. Nothing shorter is accepted: `#abc` would have to
/// guess at what the writer meant by it.
pub fn parse_color(text: &str) -> Option<Color> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    Some(Color::Rgb(byte(0)?, byte(2)?, byte(4)?))
}

impl Palette {
    /// Set one colour by the name the config calls it.
    ///
    /// Returns false for a name that is not one of the eleven, so the
    /// caller can say which line of the file did nothing.
    pub fn set(&mut self, name: &str, color: Color) -> bool {
        match name.trim().to_lowercase().as_str() {
            "accent" => self.accent = color,
            "text" => self.text = color,
            "dim" => self.dim = color,
            "heading" => self.heading = color,
            "surface" => self.surface = color,
            "selection" => self.selection = color,
            "quality" => self.quality = color,
            "track" => self.track = color,
            "placeholder" => self.placeholder = color,
            "border" => self.border = color,
            "on_accent" => self.on_accent = color,
            _ => return false,
        }
        true
    }

    /// The palette a config asks for, and what could not be read.
    ///
    /// Best-effort by design: a line that cannot be read is skipped and
    /// reported, leaving that colour as the theme had it. The app is still
    /// usable with one wrong grey; it is not usable if it will not open.
    pub fn from_config(ui: &crate::config::UiConfig) -> (Self, Vec<String>) {
        let mut problems = Vec::new();
        let mut palette = match named(&ui.theme) {
            Some(p) => p,
            None => {
                problems.push(format!(
                    "no theme called {:?}; the ones there are: {}",
                    ui.theme,
                    THEMES.join(", ")
                ));
                Palette::detect()
            }
        };
        // Sorted, so the same file always reports its problems in the same
        // order -- a HashMap walks differently every run.
        let mut named_colors: Vec<(&String, &String)> = ui.colors.iter().collect();
        named_colors.sort();
        for (name, value) in named_colors {
            match parse_color(value) {
                Some(color) => {
                    if !palette.set(name, color) {
                        problems.push(format!("no colour called {name:?}"));
                    }
                }
                None => problems.push(format!(
                    "{name:?} is not a colour: {value:?} is not #rrggbb"
                )),
            }
        }
        (palette, problems)
    }
}

/// The grey band that marks a selected row, with its ends softened.
///
/// A background fills whole cells, so a band alone is a hard-edged
/// rectangle; a half block is inked over half its cell, which softens each
/// end into something nearer a rounded edge than a wall.
///
/// Shared so a selection reads the same wherever it is: the track list drew
/// this and the home page's track grid drew a plain rectangle, which made
/// the same selection look like two different marks.
///
/// `RING` is the column each end takes. A caller leaves it free whether or
/// not the row is selected, or the row would jump sideways as the cursor
/// passed over it.
pub const RING: u16 = 1;

pub fn selection_band(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    palette: &Palette,
) {
    use ratatui::layout::Rect;
    use ratatui::text::Line;
    use ratatui::widgets::{Block, Paragraph};

    if area.width <= RING * 2 || area.height == 0 {
        return;
    }
    let band = Rect {
        x: area.x + RING,
        width: area.width - RING * 2,
        ..area
    };
    frame.render_widget(Block::default().style(palette.row_focused()), band);

    let cap = Style::default().fg(palette.selection);
    for y in area.y..area.y + area.height {
        frame.render_widget(
            Paragraph::new(Line::styled("▐", cap)),
            Rect { x: area.x, y, width: RING, height: 1 },
        );
        frame.render_widget(
            Paragraph::new(Line::styled("▌", cap)),
            Rect { x: band.x + band.width, y, width: RING, height: 1 },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A config naming a theme and nothing else.
    fn ui(theme: &str) -> crate::config::UiConfig {
        crate::config::UiConfig {
            theme: theme.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn a_named_theme_is_the_one_that_is_drawn() {
        let (catppuccin, problems) = Palette::from_config(&ui("catppuccin"));
        assert!(problems.is_empty(), "a name that exists: {problems:?}");
        assert_eq!(
            catppuccin.accent,
            Color::Rgb(0xcb, 0xa6, 0xf7),
            "the flavour's own mauve, not TIDAL's green"
        );
        assert_ne!(
            catppuccin.accent,
            Palette::detect().accent,
            "and not the default palette under another name"
        );
    }

    #[test]
    fn every_theme_the_message_offers_is_one_that_resolves() {
        // The list is what a misspelt name is answered with, so a theme
        // named there and not handled would be advice that does not work.
        // It was already wrong once: solarized sat in a comment.
        for name in THEMES {
            assert!(
                named(name).is_some(),
                "{name} is offered by the error message"
            );
        }
    }

    #[test]
    fn no_two_themes_are_the_same_colours() {
        // A theme that resolves to another one's palette is a name that
        // does nothing, and nothing else would say so.
        let themes: Vec<(&str, Palette)> = THEMES
            .iter()
            .filter(|n| **n != "default")
            .map(|n| (*n, named(n).expect("a theme")))
            .collect();
        for (i, (name, a)) in themes.iter().enumerate() {
            for (other, b) in themes.iter().skip(i + 1) {
                assert_ne!(
                    format!("{a:?}"),
                    format!("{b:?}"),
                    "{name} and {other} are the same palette"
                );
            }
        }
    }

    #[test]
    fn a_selected_row_is_visible_against_the_panel_behind_it() {
        // The one rule the layout puts on a palette: `selection` marks the
        // highlighted row and `surface` is the furniture it sits on, so a
        // theme whose two are equal has no visible selection at all.
        // `placeholder` likewise stands for missing artwork, and matching
        // the background makes that read as a failure to draw.
        for name in THEMES {
            let p = named(name).expect("a theme");
            assert_ne!(
                format!("{:?}", p.selection),
                format!("{:?}", p.surface),
                "{name}: a selected row is the same colour as the panel"
            );
            assert_ne!(
                format!("{:?}", p.placeholder),
                format!("{:?}", p.surface),
                "{name}: a missing cover is the same colour as the panel"
            );
            assert_ne!(
                format!("{:?}", p.text),
                format!("{:?}", p.surface),
                "{name}: text is the same colour as what is behind it"
            );
        }
    }

    #[test]
    fn a_theme_nobody_has_heard_of_says_so_and_draws_the_default() {
        let (palette, problems) = Palette::from_config(&ui("monokai"));
        assert_eq!(problems.len(), 1, "said once: {problems:?}");
        assert!(
            problems[0].contains("monokai") && problems[0].contains("catppuccin"),
            "naming what was asked for and what there is: {problems:?}"
        );
        assert_eq!(
            palette.accent,
            Palette::detect().accent,
            "and the app still opens, in its own colours"
        );
    }

    #[test]
    fn a_colour_set_by_hand_wins_over_the_theme() {
        let mut config = ui("catppuccin");
        config
            .colors
            .insert("accent".into(), "#ff0000".into());
        let (palette, problems) = Palette::from_config(&config);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(palette.accent, Color::Rgb(255, 0, 0), "the one that was set");
        assert_eq!(
            palette.text,
            Color::Rgb(0xcd, 0xd6, 0xf4),
            "and the rest of the theme is left alone"
        );
    }

    #[test]
    fn one_bad_line_does_not_take_the_rest_of_the_palette_with_it() {
        // The whole point of reporting rather than failing: a misspelt
        // colour costs that colour and nothing else, and never stops the
        // app opening.
        let mut config = ui("catppuccin");
        config.colors.insert("accent".into(), "not a colour".into());
        config.colors.insert("nonsense".into(), "#ffffff".into());
        let (palette, problems) = Palette::from_config(&config);
        assert_eq!(problems.len(), 2, "both said: {problems:?}");
        assert_eq!(
            palette.accent,
            Color::Rgb(0xcb, 0xa6, 0xf7),
            "the theme's own accent survives the line that failed"
        );
    }

    #[test]
    fn a_colour_is_six_hex_digits_with_or_without_the_hash() {
        assert_eq!(parse_color("#1e1e2e"), Some(Color::Rgb(30, 30, 46)));
        assert_eq!(parse_color("1e1e2e"), Some(Color::Rgb(30, 30, 46)));
        assert_eq!(parse_color("  #1E1E2E  "), Some(Color::Rgb(30, 30, 46)));
        // Nothing shorter: what a three-digit form means is a guess.
        assert_eq!(parse_color("#abc"), None);
        assert_eq!(parse_color("#zzzzzz"), None);
        assert_eq!(parse_color(""), None);
    }

    #[test]
    fn every_colour_the_palette_has_can_be_set_by_name() {
        // A field added without a name here would be one the config could
        // not reach, and nothing else would notice.
        let mut palette = Palette::detect();
        for name in [
            "accent", "text", "dim", "heading", "surface", "selection",
            "quality", "track", "placeholder", "border", "on_accent",
        ] {
            assert!(
                palette.set(name, Color::Rgb(1, 2, 3)),
                "{name} is a colour the config can set"
            );
        }
        // And every field is now the one that was set, which is what says
        // the list above is the whole of the struct.
        let text = format!("{palette:?}");
        assert!(
            !text.contains("Indexed") && !text.contains("Rgb(0,"),
            "every field was reachable by name: {text}"
        );
    }

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
