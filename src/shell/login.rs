use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::auth::DeviceCode;

#[derive(Debug, Clone, Default)]
pub enum LoginState {
    #[default]
    Idle,
    Waiting { code: DeviceCode },
    Failed(String),
}

/// What a poll attempt reported, flattened so it can travel in an `Action`
/// (which must stay `Clone`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollResult {
    Pending,
    SlowDown,
    Expired,
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

pub fn render(frame: &mut Frame, area: Rect, state: &LoginState) {
    let lines: Vec<Line> = match state {
        LoginState::Idle => vec![
            Line::from("Sign in to TIDAL"),
            Line::from(""),
            Line::from("Press Enter to begin."),
        ],
        LoginState::Waiting { code } => vec![
            // The browser is opened for them, but the link stays visible: the
            // open is best effort, and on a headless or locked-down machine
            // this is the only way through.
            Line::from("Confirm in your browser — opening it now."),
            Line::from(""),
            Line::from("If it did not open, go to:"),
            Line::from(""),
            Line::from(Span::styled(
                code.verification_uri.clone(),
                Style::default().add_modifier(Modifier::UNDERLINED),
            )),
            Line::from(""),
            Line::from(vec![
                Span::raw("code: "),
                Span::styled(
                    code.user_code.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from("Waiting for confirmation…"),
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

    fn code() -> crate::auth::DeviceCode {
        crate::auth::DeviceCode {
            device_code: "dc".into(),
            user_code: "VFOXP".into(),
            verification_uri: "https://link.tidal.com/VFOXP".into(),
            interval_secs: 2,
            expires_in_secs: 300,
        }
    }

    fn rendered(state: &LoginState) -> String {
        let mut terminal = Terminal::new(TestBackend::new(70, 12)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), state))
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
        let text = rendered(&LoginState::Waiting { code: code() });
        assert!(text.contains("VFOXP"), "user code must be visible:\n{text}");
        assert!(text.contains("link.tidal.com"), "link must be visible:\n{text}");
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
        assert!(text.contains("network unreachable"), "reason must be shown:\n{text}");
    }

    #[test]
    fn rendering_a_long_message_in_a_narrow_frame_does_not_panic() {
        let mut terminal = Terminal::new(TestBackend::new(12, 5)).unwrap();
        let state = LoginState::Failed("x".repeat(500));
        terminal.draw(|frame| render(frame, frame.area(), &state)).unwrap();
    }
}
