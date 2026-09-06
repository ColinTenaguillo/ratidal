use std::time::Duration;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, LineGauge, Paragraph};
use ratatui::Frame;

use super::theme::Palette;
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

pub fn render(frame: &mut Frame, area: Rect, palette: &Palette, state: &NowPlaying) {
    render_with_cover(frame, area, palette, state, |_, _, _, _| false)
}

/// As [`render`], but able to draw the track's cover as a thumbnail.
///
/// `draw_cover` returns false when it could not draw one, and a coloured
/// block stands in — so the bar keeps its shape on terminals with no image
/// protocol, which is most of them.
pub fn render_with_cover<F>(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    state: &NowPlaying,
    mut draw_cover: F,
) where
    F: FnMut(&mut Frame, Rect, &str, super::artwork::Shape) -> bool,
{
    let block = Block::default().borders(Borders::TOP);
    let full = block.inner(area);
    frame.render_widget(block, area);
    // Reserve the last row for the key hints the shell draws.
    let inner = Rect { height: full.height.saturating_sub(1), ..full };

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let Some(track) = &state.track else {
        frame.render_widget(
            Paragraph::new(ratatui::text::Line::styled(
                "Nothing playing",
                palette.subtitle(),
            )),
            inner,
        );
        return;
    };

    // Cover thumb, then title/artist. Centre: transport + progress. Right:
    // the quality badge.
    // The left column holds a thumb plus two lines of text, so it needs real
    // width; at 30% the artist ran straight into the progress bar.
    let columns = Layout::horizontal([
        Constraint::Percentage(38),
        Constraint::Min(20),
        Constraint::Length(17),
    ])
    .split(inner);

    // A square-ish thumb: cells are about twice as tall as they are wide, so
    // the width is double the height.
    // Capped at 6: derived from the bar's height alone it grew with the bar
    // and started eating the artist name. A cover thumb is an accent, not the
    // point of the column.
    let thumb_width = (inner.height * 2).min(columns[0].width / 4).min(6);
    let text_x = columns[0].x + thumb_width + if thumb_width > 0 { 1 } else { 0 };

    if thumb_width > 0 {
        let thumb = Rect { width: thumb_width, ..columns[0] };
        let drew = match &track.cover {
            Some(url) => draw_cover(frame, thumb, url, super::artwork::Shape::Square),
            None => false,
        };
        if !drew {
            frame.render_widget(
                Block::default().style(Style::default().bg(palette.surface)),
                thumb,
            );
        }
    }

    if text_x < columns[0].x + columns[0].width {
        frame.render_widget(
            Paragraph::new(vec![
                ratatui::text::Line::styled(track.title.clone(), palette.title()),
                ratatui::text::Line::styled(track.artist.clone(), palette.subtitle()),
            ]),
            Rect {
                x: text_x,
                y: columns[0].y,
                // Leave a gutter: without it a long artist name runs straight
                // into the progress bar in the next column.
                width: (columns[0].x + columns[0].width - text_x).saturating_sub(2),
                height: columns[0].height,
            },
        );
    }

    let transport = if state.playing { "⏮  ▶  ⏭" } else { "⏮  ⏸  ⏭" };
    // Inset the centre column so the gauge cannot bleed into the title beside
    // it — LineGauge fills its whole rect, label included.
    let centre = Rect {
        x: columns[1].x + 1,
        width: columns[1].width.saturating_sub(2),
        ..columns[1]
    };
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)])
        .split(centre);

    frame.render_widget(
        Paragraph::new(ratatui::text::Line::styled(transport, palette.title()))
            .alignment(ratatui::layout::Alignment::Center),
        rows[0],
    );

    if rows.len() > 1 && rows[1].height > 0 {
        frame.render_widget(
            LineGauge::default()
                .filled_style(palette.accent_text())
                .unfilled_style(palette.rule())
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
            Paragraph::new(ratatui::text::Line::styled(
                quality.clone(),
                palette.accent_text(),
            ))
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
            tags: vec!["LOSSLESS".into(), "HIRES_LOSSLESS".into()],
            ..crate::domain::Track::sample(
                "Money Trees",
                "Kendrick Lamar, Jay Rock",
                Duration::from_secs(387),
            )
        }
    }

    fn rendered(state: &NowPlaying, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 4)).unwrap();
        let palette = Palette::detect();
        terminal.draw(|frame| render(frame, frame.area(), &palette, state)).unwrap();
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
    fn the_thumbnail_does_not_crowd_out_the_title() {
        // The cover thumb eats into the left column, so the title and artist
        // have to survive the squeeze.
        let state = NowPlaying {
            track: Some(track()),
            position: Duration::from_secs(108),
            playing: true,
            quality: Some("24-bit 44.1kHz".into()),
        };
        let text = rendered(&state, 100);
        assert!(text.contains("Money Trees"), "title lost to the thumb:\n{text}");
        assert!(text.contains("Kendrick"), "artist lost to the thumb:\n{text}");
        assert!(text.contains("1:48"), "elapsed lost:\n{text}");
        assert!(text.contains("24-bit"), "quality badge lost:\n{text}");
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
