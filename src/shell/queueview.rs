//! What is playing next, and where each of it came from.
//!
//! A queue the user cannot see is a queue they cannot trust: the reported
//! complaint was that changing track sometimes brought an old album back,
//! which is exactly the kind of thing that is obvious the moment the list
//! is on screen and baffling while it is not.

use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::carousel::truncate;
use super::theme::Palette;
use crate::playback::{Queue, Source};

/// Rows of chrome: the border, the heading under it, and the closing hint
/// with its blank line.
const CHROME: u16 = 5;

/// The widest the modal grows, whatever the terminal has.
///
/// A queue is a list of titles rather than a document; past this the eye
/// has to travel a long way from the number to the name.
const MAX_WIDTH: u16 = 72;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    queue: &Queue,
    selected: usize,
) {
    let entries = queue.entries();
    let width = MAX_WIDTH.min(area.width.saturating_sub(4));
    // As tall as it needs, up to what the pane has.
    let wanted = entries.len() as u16 + CHROME;
    let height = wanted.min(area.height.saturating_sub(2));
    let modal = centred(area, width, height);
    if modal.width < 8 || modal.height < CHROME {
        return;
    }

    // The rows the list itself gets, once the chrome has taken its share.
    let room = modal.height.saturating_sub(CHROME) as usize;
    let playing = queue.position();
    // Scrolled to the cursor rather than to the player: the view opens with
    // the two together, and after that it is the cursor the user is moving.
    // A queue of two hundred opened at the top shows nothing of either.
    let first = selected.saturating_sub(room / 2).min(
        entries.len().saturating_sub(room),
    );

    let mut lines: Vec<Line> = Vec::new();
    let mut last_source: Option<Source> = None;

    for (i, (track, source)) in entries.iter().enumerate().skip(first).take(room) {
        // A heading each time the source changes, which is what tells
        // "queued by hand" from "the rest of the album" at a glance.
        if last_source != Some(*source) && i >= first {
            if let Some(heading) = heading_for(*source) {
                lines.push(Line::styled(heading, palette.section_heading()));
            }
            last_source = Some(*source);
        }

        let mark = if i == playing {
            super::icons::playing()
        } else {
            " "
        };
        let style = if i == selected {
            // The row under the cursor, whether or not it is the one
            // playing: this is the thing the next key acts on.
            palette.title().bg(palette.selection)
        } else if i == playing {
            palette.title()
        } else {
            palette.subtitle()
        };
        // The number column is the room the mark and a space take.
        let room_for_text = modal.width.saturating_sub(6);
        lines.push(Line::from(vec![
            Span::styled(format!(" {mark} "), palette.accent_text()),
            Span::styled(
                truncate(&format!("{} — {}", track.title, track.artist), room_for_text),
                style,
            ),
        ]));
    }

    if entries.is_empty() {
        lines.push(Line::styled("  nothing queued", palette.subtitle()));
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "  j/k move · x removes · enter plays · any other key closes",
        palette.subtitle(),
    ));

    frame.render_widget(Clear, modal);
    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Left).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(palette.rule())
                .title(" Queue ")
                .title_style(palette.page_heading()),
        ),
        modal,
    );
}

/// What to call each run of tracks.
///
/// The context has no heading: it is the album or playlist the user
/// started, and naming it "Context" would be the app talking about itself.
/// The other two are worth marking, because they are the ones that surprise
/// people when they play.
fn heading_for(source: Source) -> Option<&'static str> {
    match source {
        Source::User => Some("  Queued"),
        Source::Context => None,
        Source::Autoplay => Some("  Autoplay"),
    }
}

/// A box of this size in the middle of `area`.
fn centred(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Track;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::time::Duration;

    fn track(title: &str) -> Track {
        Track::sample(title, "An Artist", Duration::from_secs(100))
    }

    fn drawn(queue: &Queue, w: u16, h: u16) -> String {
        at(queue, w, h, queue.position())
    }

    /// Drawn with the cursor somewhere of the caller's choosing.
    fn at(queue: &Queue, w: u16, h: u16, selected: usize) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        let palette = Palette::detect();
        terminal
            .draw(|f| render(f, f.area(), &palette, queue, selected))
            .unwrap();
        super::super::geometry::text(&terminal.backend().buffer().clone())
    }

    #[test]
    fn the_queue_says_where_each_track_came_from() {
        // The point of the view: "why is this playing" is answered by which
        // heading the track sits under.
        let mut queue = Queue::new(vec![track("From the album")], 0);
        queue.play_next(track("Asked for"));
        queue.set_autoplay(vec![track("Suggested")]);

        let text = drawn(&queue, 80, 20);
        assert!(text.contains("Asked for"), "the hand-queued track:\n{text}");
        assert!(text.contains("Queued"), "under its own heading:\n{text}");
        assert!(text.contains("Autoplay"), "and the radio under its:\n{text}");
        assert!(
            !text.contains("Context"),
            "but the album is not labelled with a word from the code:\n{text}"
        );
    }

    #[test]
    fn the_track_that_is_playing_is_marked() {
        let mut queue = Queue::new(vec![track("First"), track("Second")], 0);
        queue.next();
        let text = drawn(&queue, 80, 20);
        let marked = text
            .lines()
            .find(|l| l.contains(super::super::icons::playing()))
            .unwrap_or("");
        assert!(
            marked.contains("Second"),
            "the mark is on what is playing, not the top of the list:\n{text}"
        );
    }

    #[test]
    fn a_long_queue_opens_where_the_player_is() {
        // Opening at the top of two hundred tracks shows everything except
        // the part the user opened it to see.
        let tracks: Vec<Track> = (0..200).map(|i| track(&format!("Track {i}"))).collect();
        let mut queue = Queue::new(tracks, 0);
        for _ in 0..150 {
            queue.next();
        }
        let text = drawn(&queue, 80, 20);
        assert!(
            text.contains("Track 150"),
            "the playing track is on screen:\n{text}"
        );
    }

    #[test]
    fn an_empty_queue_says_so_rather_than_drawing_a_blank_box() {
        let text = drawn(&Queue::default(), 80, 20);
        assert!(text.contains("nothing queued"), "{text}");
    }

    #[test]
    fn a_pane_too_small_for_the_modal_does_not_panic() {
        for (w, h) in [(0, 0), (4, 2), (10, 4), (20, 6)] {
            let queue = Queue::new(vec![track("One")], 0);
            let _ = drawn(&queue, w.max(1), h.max(1));
        }
    }
}
