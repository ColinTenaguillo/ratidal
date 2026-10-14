//! The Widevine module, as the screen talks about it.
//!
//! Apple Music's streams are encrypted, and only Google's module decrypts
//! them. Nothing ships it: Google publishes nothing anyone may redistribute.
//! So, once an Apple Music account is in, the machine is searched -- a
//! browser's copy will do -- and with none found the user is asked whether
//! to download it from Google, shown Google's terms out of the archive, and
//! asked again before it goes into the store. Declined, Apple Music keeps
//! its catalogue and refuses to play.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use super::theme::Palette;

/// Where the module stands this run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Cdm {
    /// Not looked for: the service does not need it.
    #[default]
    Unneeded,
    /// Searching the machine.
    Looking,
    /// One is here.
    Ready(music::drm::Origin),
    /// None is here: asking whether to download.
    Wanted,
    /// Downloading, to show the terms.
    Offering,
    /// Downloaded; the terms are up, waiting for a yes.
    Offered {
        version: String,
        license: String,
        scroll: u16,
    },
    /// Accepted; going into the store.
    Installing,
    /// Said no this run.
    Declined,
    /// Nothing here, nothing asked for, or the download failed.
    Missing,
}

impl Cdm {
    /// Whether a question is up, taking the keys.
    pub fn asking(&self) -> bool {
        matches!(self, Cdm::Wanted | Cdm::Offered { .. })
    }

    /// Whether the screen shows this at all.
    pub fn shown(&self) -> bool {
        matches!(
            self,
            Cdm::Wanted | Cdm::Offering | Cdm::Offered { .. } | Cdm::Installing
        )
    }

    pub fn scroll_by(&mut self, down: bool) {
        if let Cdm::Offered { scroll, .. } = self {
            *scroll = match down {
                true => scroll.saturating_add(1),
                false => scroll.saturating_sub(1),
            };
        }
    }
}

/// What the status line says once the module is settled.
pub fn describe(origin: music::drm::Origin) -> &'static str {
    match origin {
        music::drm::Origin::Configured => "using the Widevine module the environment names",
        music::drm::Origin::Installed => "using a browser's Widevine module",
        music::drm::Origin::Fetched => "using the Widevine module from Google",
    }
}

fn centred(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

pub fn render(frame: &mut Frame, area: Rect, palette: &Palette, cdm: &Cdm) {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let (lines, tall): (Vec<Line>, bool) = match cdm {
        Cdm::Wanted => (
            vec![
                Line::from("Apple Music needs Google's Widevine module to play."),
                Line::from(""),
                Line::from("No browser on this machine has one that can be used."),
                Line::from("It can be downloaded from Google's update service, as"),
                Line::from("Chrome and Kodi get theirs; Google's terms come first."),
                Line::from(""),
                Line::from(vec![
                    Span::styled("y", bold),
                    Span::raw(" download   "),
                    Span::styled("n", bold),
                    Span::raw(" not now"),
                ]),
            ],
            false,
        ),
        Cdm::Offering => (vec![Line::from("Downloading the Widevine module…")], false),
        Cdm::Offered {
            version, license, ..
        } => {
            let mut lines = vec![
                Line::from(format!("Widevine {version} is downloaded. Google's terms:")),
                Line::from(""),
            ];
            lines.extend(license.lines().map(|l| Line::from(l.to_string())));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("y", bold),
                Span::raw(" accept and install   "),
                Span::styled("n", bold),
                Span::raw(" decline   "),
                Span::styled("j k", bold),
                Span::raw(" scroll"),
            ]));
            (lines, true)
        }
        Cdm::Installing => (vec![Line::from("Installing the Widevine module…")], false),
        _ => return,
    };
    let height = match tall {
        true => area.height.saturating_sub(2),
        false => lines.len() as u16 + 2,
    };
    let modal = centred(area, 70, height);
    if modal.width < 4 || modal.height < 3 {
        return;
    }
    frame.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Widevine ")
        .border_style(palette.rule());
    let inner = block.inner(modal);
    frame.render_widget(block, modal);
    let scroll = match cdm {
        Cdm::Offered { scroll, .. } => *scroll,
        _ => 0,
    };
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(if tall { Alignment::Left } else { Alignment::Center })
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(cdm: &Cdm) -> String {
        let buf = crate::shell::geometry::draw(90, 20, |f, area, p| render(f, area, p, cdm));
        crate::shell::geometry::text(&buf)
    }

    #[test]
    fn the_question_and_the_terms_are_drawn_and_nothing_else_is() {
        assert!(drawn(&Cdm::Wanted).contains("download"));
        let offered = Cdm::Offered {
            version: "4.10.2891.0".into(),
            license: "TERMS OF SERVICE\nBy using this you agree.".into(),
            scroll: 0,
        };
        let text = drawn(&offered);
        assert!(text.contains("4.10.2891.0"), "{text}");
        assert!(text.contains("TERMS OF SERVICE"), "{text}");
        assert!(text.contains("accept"), "{text}");
        assert!(drawn(&Cdm::Ready(music::drm::Origin::Installed)).trim().is_empty());
        assert!(drawn(&Cdm::Unneeded).trim().is_empty());
    }

    #[test]
    fn only_the_two_questions_take_the_keys() {
        assert!(Cdm::Wanted.asking());
        let mut offered = Cdm::Offered {
            version: String::new(),
            license: String::new(),
            scroll: 0,
        };
        assert!(offered.asking());
        offered.scroll_by(true);
        offered.scroll_by(false);
        offered.scroll_by(false);
        assert!(matches!(offered, Cdm::Offered { scroll: 0, .. }), "never below the top");
        assert!(!Cdm::Offering.asking() && !Cdm::Missing.asking() && !Cdm::Unneeded.asking());
        assert!(Cdm::Offering.shown() && !Cdm::Declined.shown());
    }
}
