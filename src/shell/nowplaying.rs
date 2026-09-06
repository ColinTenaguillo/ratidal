use std::time::Duration;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, LineGauge, Paragraph};
use ratatui::Frame;

use crate::domain::Track;

#[derive(Debug, Default, Clone)]
pub struct NowPlaying {
    pub track: Option<Track>,
    pub position: Duration,
    pub playing: bool,
    /// The DELIVERED quality, e.g. "24-bit 44.1kHz". Never the requested one.
    pub quality: Option<String>,
}

/// m:ss, with minutes uncapped (a 90-minute mix reads "90:00").
pub fn format_time(d: Duration) -> String {
    let total = d.as_secs();
    format!("{}:{:02}", total / 60, total % 60)
}

/// Fraction played, clamped to 0.0..=1.0. LineGauge panics outside that range,
/// and a reported position can exceed the container's stated duration.
pub fn progress(position: Duration, total: Duration) -> f64 {
    if total.is_zero() {
        return 0.0;
    }
    (position.as_secs_f64() / total.as_secs_f64()).clamp(0.0, 1.0)
}

pub fn render(frame: &mut Frame, area: Rect, state: &NowPlaying) {
    let block = Block::default().borders(Borders::TOP);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let Some(track) = &state.track else {
        frame.render_widget(Paragraph::new("Nothing playing"), inner);
        return;
    };

    // Left: title/artist. Centre: transport + progress. Right: quality badge.
    let columns = Layout::horizontal([
        Constraint::Percentage(30),
        Constraint::Min(20),
        Constraint::Length(16),
    ])
    .split(inner);

    frame.render_widget(
        Paragraph::new(vec![
            ratatui::text::Line::from(track.title.clone()),
            ratatui::text::Line::from(track.artist.clone()),
        ]),
        columns[0],
    );

    let transport = if state.playing { "⏮  ▶  ⏭" } else { "⏮  ⏸  ⏭" };
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)])
        .split(columns[1]);

    frame.render_widget(
        Paragraph::new(transport).alignment(ratatui::layout::Alignment::Center),
        rows[0],
    );

    if rows.len() > 1 && rows[1].height > 0 {
        frame.render_widget(
            LineGauge::default()
                .ratio(progress(state.position, track.duration))
                .label(format!(
                    "{} / {}",
                    format_time(state.position),
                    format_time(track.duration)
                )),
            rows[1],
        );
    }

    if let Some(quality) = &state.quality {
        frame.render_widget(
            Paragraph::new(quality.clone())
                .style(Style::default().add_modifier(Modifier::DIM))
                .alignment(ratatui::layout::Alignment::Right),
            columns[2],
        );
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;

    fn track() -> crate::domain::Track {
        crate::domain::Track {
            id: crate::domain::TrackId(1),
            title: "Money Trees".into(),
            artist: "Kendrick Lamar, Jay Rock".into(),
            duration: Duration::from_secs(387),
            cover: None,
            tags: vec!["LOSSLESS".into(), "HIRES_LOSSLESS".into()],
        }
    }

    fn rendered(state: &NowPlaying, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 4)).unwrap();
        terminal.draw(|frame| render(frame, frame.area(), state)).unwrap();
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
    fn shows_title_artist_and_both_times() {
        let state = NowPlaying {
            track: Some(track()),
            position: Duration::from_secs(108),
            playing: true,
            quality: Some("24-bit 44.1kHz".into()),
        };
        let text = rendered(&state, 100);
        assert!(text.contains("Money Trees"), "title:\n{text}");
        assert!(text.contains("Kendrick Lamar"), "artist:\n{text}");
        assert!(text.contains("1:48"), "elapsed:\n{text}");
        assert!(text.contains("6:27"), "total:\n{text}");
        assert!(text.contains("24-bit"), "quality badge:\n{text}");
    }

    #[test]
    fn formats_times_as_minutes_and_seconds() {
        assert_eq!(format_time(Duration::from_secs(0)), "0:00");
        assert_eq!(format_time(Duration::from_secs(9)), "0:09");
        assert_eq!(format_time(Duration::from_secs(108)), "1:48");
        assert_eq!(format_time(Duration::from_secs(387)), "6:27");
        assert_eq!(format_time(Duration::from_secs(3600)), "60:00");
    }

    #[test]
    fn progress_ratio_is_clamped_to_one() {
        // A position past the reported duration must not panic LineGauge,
        // which requires 0.0..=1.0.
        assert_eq!(progress(Duration::from_secs(10), Duration::from_secs(5)), 1.0);
        assert_eq!(progress(Duration::ZERO, Duration::from_secs(10)), 0.0);
        assert!((progress(Duration::from_secs(5), Duration::from_secs(10)) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_zero_duration_track_does_not_divide_by_zero() {
        assert_eq!(progress(Duration::from_secs(3), Duration::ZERO), 0.0);
    }

    #[test]
    fn renders_with_no_track_without_panicking() {
        let text = rendered(&NowPlaying::default(), 60);
        assert!(!text.is_empty());
    }

    #[test]
    fn renders_in_a_very_narrow_terminal_without_panicking() {
        let state = NowPlaying {
            track: Some(track()),
            position: Duration::from_secs(1),
            playing: true,
            quality: None,
        };
        let _ = rendered(&state, 8);
    }
}
