use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

#[derive(Debug, Clone, Default)]
pub enum LoginState {
    #[default]
    Idle,
    /// The code the service showed, to confirm at the link.
    Waiting {
        code: String,
        url: String,
    },
    /// A Subsonic server named in the config, waiting for its password.
    /// Typed here and handed to the provider, which keeps it; it is never
    /// written to the config file.
    Password {
        server: String,
        username: String,
        typed: Secret,
    },
    /// The service wants authorising in the browser, at this link, and
    /// will hear back on its own -- Spotify's callback port.
    Url(String),
    /// The service wants the cookies of a signed-in browser tab, pasted.
    /// No webview in a terminal: the user copies the `Cookie` header out
    /// of the devtools, and the provider takes what it needs from it.
    Secret {
        typed: Secret,
    },
    /// The cookies carried several accounts; the service wants one named.
    Accounts {
        choices: Vec<music::AccountChoice>,
        selected: usize,
    },
    /// The service is working on it: nothing to type, nothing to press.
    Busy(String),
    Failed(String),
}

/// A password in flight. Printed as dots: `Action` is logged at debug
/// level, and this travels in one.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(pub String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(\u{2022}\u{2022}\u{2022})")
    }
}

/// Centre a box of the given size inside `area`, clamped so it always fits.
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

/// Hand the verification link to the system browser.
///
/// Best effort by design: the link stays on screen either way, so a failure
/// here costs nothing — the user copies it as before. Anything that spawns a
/// browser can fail (a headless box, a locked-down desktop, no handler
/// registered), and none of that should interrupt the login.
///
/// `Stdio::null()` on all three streams matters: a helper that writes to the
/// terminal would scribble over the TUI, which is in raw mode on the
/// alternate screen.
pub fn open_in_browser(url: &str) {
    use std::process::{Command, Stdio};

    // Refuse anything that is not plainly an https URL. The value comes from a
    // network response, and handing an arbitrary string to a shell-adjacent
    // launcher is not something to do on trust.
    if !url.starts_with("https://") || url.contains(char::is_whitespace) {
        tracing::warn!("refusing to open a verification link that is not a plain https URL");
        return;
    }

    let mut command = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        c
    } else if cfg!(target_os = "windows") {
        // `start` is a cmd builtin, and its first quoted argument is taken as
        // a window title — hence the empty one.
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };

    match command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => tracing::info!("opened the verification link in the browser"),
        Err(e) => tracing::info!("could not open a browser ({e}); the link is on screen"),
    }
}

/// `service` names what is being signed in to, and `guest` whether it can
/// be used without an account -- YouTube Music can.
pub fn render(frame: &mut Frame, area: Rect, state: &LoginState, service: &str, guest: bool) {
    let lines: Vec<Line> = match state {
        LoginState::Idle => vec![
            Line::from(format!("Sign in to {service}")),
            Line::from(""),
            Line::from(if guest {
                "Press Enter to begin, or g to continue as a guest."
            } else {
                "Press Enter to begin."
            }),
        ],
        LoginState::Url(url) => vec![
            Line::from("Authorise in your browser — opening it now."),
            Line::from(""),
            Line::from("If it did not open, go to:"),
            Line::from(""),
            Line::from(Span::styled(
                url.clone(),
                Style::default().add_modifier(Modifier::UNDERLINED),
            )),
            Line::from(""),
            Line::from("Waiting for the browser…"),
        ],
        LoginState::Secret { typed } => vec![
            Line::from(format!("Paste the cookies of a tab signed in to {service}")),
            Line::from(""),
            Line::from("In the browser: devtools, Network, any request, Request Headers,"),
            Line::from("copy the Cookie line."),
            Line::from(""),
            Line::from(vec![
                Span::raw("cookies: "),
                Span::styled(
                    format!("{} characters", typed.0.chars().count()),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from("Enter to send, Esc to quit."),
        ],
        LoginState::Accounts { choices, selected } => {
            let mut lines = vec![Line::from("Which account?"), Line::from("")];
            for (i, choice) in choices.iter().enumerate() {
                let mark = if i == *selected { "> " } else { "  " };
                let detail = choice
                    .detail
                    .as_deref()
                    .map(|d| format!(" — {d}"))
                    .unwrap_or_default();
                let style = if i == *selected {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                lines.push(Line::from(Span::styled(
                    format!("{mark}{}{detail}", choice.name),
                    style,
                )));
            }
            lines.push(Line::from(""));
            lines.push(Line::from("j/k to choose, Enter to pick, Esc to quit."));
            lines
        }
        LoginState::Busy(what) => vec![
            Line::from(what.clone()),
            Line::from(""),
            Line::from("Esc to quit."),
        ],
        LoginState::Waiting { code, url } => vec![
            // The browser is opened for them, but the link stays visible: the
            // open is best effort, and on a headless or locked-down machine
            // this is the only way through.
            Line::from("Confirm in your browser — opening it now."),
            Line::from(""),
            Line::from("If it did not open, go to:"),
            Line::from(""),
            Line::from(Span::styled(
                url.clone(),
                Style::default().add_modifier(Modifier::UNDERLINED),
            )),
            Line::from(""),
            Line::from(vec![
                Span::raw("code: "),
                Span::styled(code.clone(), Style::default().add_modifier(Modifier::BOLD)),
            ]),
            Line::from(""),
            Line::from("Waiting for confirmation…"),
        ],
        LoginState::Password {
            server,
            username,
            typed,
        } => vec![
            Line::from(format!("Connect to {server}")),
            Line::from(""),
            Line::from(format!("as {username}")),
            Line::from(""),
            Line::from(vec![
                Span::raw("password: "),
                Span::styled(
                    "\u{2022}".repeat(typed.0.chars().count()),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from("Enter to connect, Esc to quit."),
        ],
        LoginState::Failed(reason) => vec![
            Line::from("Sign-in failed"),
            Line::from(""),
            Line::from(reason.clone()),
            Line::from(""),
            Line::from("Press Enter to retry, q to quit."),
        ],
    };

    let box_area = centred(area, 60, lines.len() as u16 + 2);
    frame.render_widget(Clear, box_area);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title(" ratidal ")),
        box_area,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;

    fn rendered(state: &LoginState) -> String {
        let mut terminal = Terminal::new(TestBackend::new(70, 12)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), state, "TIDAL", false))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn waiting_shows_the_code_and_the_link() {
        // Both are needed: the link to open, the code to confirm.
        let text = rendered(&LoginState::Waiting {
            code: "VFOXP".into(),
            url: "https://link.tidal.com/VFOXP".into(),
        });
        assert!(text.contains("VFOXP"), "user code must be visible:\n{text}");
        assert!(
            text.contains("link.tidal.com"),
            "link must be visible:\n{text}"
        );
    }

    #[test]
    fn idle_prompts_the_user_to_begin() {
        let text = rendered(&LoginState::Idle);
        assert!(
            text.to_lowercase().contains("enter"),
            "idle must tell the user how to start:\n{text}"
        );
    }

    #[test]
    fn failure_shows_the_reason() {
        let text = rendered(&LoginState::Failed("network unreachable".into()));
        assert!(
            text.contains("network unreachable"),
            "reason must be shown:\n{text}"
        );
    }

    #[test]
    fn rendering_a_long_message_in_a_narrow_frame_does_not_panic() {
        let mut terminal = Terminal::new(TestBackend::new(12, 5)).unwrap();
        let state = LoginState::Failed("x".repeat(500));
        terminal
            .draw(|frame| render(frame, frame.area(), &state, "TIDAL", false))
            .unwrap();
    }

    #[test]
    fn a_typed_password_is_shown_as_dots_and_never_printed() {
        let state = LoginState::Password {
            server: "https://music.example.org".into(),
            username: "colin".into(),
            typed: Secret("hunter2".into()),
        };
        let text = rendered(&state);
        assert!(text.contains("music.example.org"), "{text}");
        assert!(
            text.contains(&"\u{2022}".repeat(7)),
            "one dot a letter:\n{text}"
        );
        assert!(
            !text.contains("hunter2"),
            "the password itself is not drawn"
        );
        assert!(
            !format!("{state:?}").contains("hunter2"),
            "nor does it reach the log through Debug"
        );
    }

    #[test]
    fn the_cookie_prompt_counts_what_was_pasted_rather_than_showing_it() {
        // A cookie header is a credential: the screen says how much is in
        // the box, never what.
        let text = rendered(&LoginState::Secret {
            typed: Secret("arl=abc123".into()),
        });
        assert!(text.contains("10 characters"), "{text}");
        assert!(!text.contains("abc123"), "the secret is not drawn:\n{text}");
        assert!(text.contains("Cookie"), "says what to copy:\n{text}");
    }

    #[test]
    fn the_account_prompt_marks_the_one_selected() {
        let choice = |id: &str, name: &str| music::AccountChoice {
            id: id.into(),
            name: name.into(),
            detail: Some("brand".into()),
        };
        let text = rendered(&LoginState::Accounts {
            choices: vec![choice("1", "Colin"), choice("2", "Label")],
            selected: 1,
        });
        assert!(text.contains("> Label"), "{text}");
        assert!(text.contains("  Colin"), "{text}");
    }

    #[test]
    fn a_url_prompt_shows_the_link_to_open() {
        let text = rendered(&LoginState::Url(
            "https://accounts.spotify.com/authorize?x=1".into(),
        ));
        assert!(text.contains("accounts.spotify.com"), "{text}");
    }
}
