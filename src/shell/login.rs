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

pub fn render(frame: &mut Frame, area: Rect, state: &LoginState) {
    let lines: Vec<Line> = match state {
        LoginState::Idle => vec![
            Line::from("Sign in to TIDAL"),
            Line::from(""),
            Line::from("Press Enter to begin."),
        ],
        LoginState::Waiting { code } => vec![
            Line::from("Open this link and confirm:"),
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
        // Flatten the buffer to text so assertions read clearly.
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
