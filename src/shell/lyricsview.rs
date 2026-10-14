//! The lyrics pane: what the track says, over the main pane, on `L`.
//!
//! Synced lyrics follow the position, the line being sung in the middle
//! and lit; plain ones are a page to scroll. Found through sonora's lyrics
//! providers, ranked by its own rules, and kept per track for the session.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use super::theme::Palette;
use crate::domain::{Track, TrackId};

/// What the search turned up for a track.
#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    Searching,
    Nothing,
    Instrumental,
    Sheet(music::Lyrics),
}

/// The pane's state: open or not, and what it holds for each track asked.
#[derive(Debug, Default)]
pub struct Pane {
    pub open: bool,
    /// By track, for the session: a track played twice is not searched
    /// twice, and skipping back lands on lyrics already in hand.
    pub found: HashMap<TrackId, Found>,
    /// Lines scrolled by hand. Synced lyrics follow the music until the
    /// user scrolls; a new track starts following again.
    pub scroll: usize,
    pub following: bool,
}

impl Pane {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.scroll = 0;
        self.following = true;
    }

    /// Whether this track still needs a search: nothing held, or a search
    /// held that never answered.
    pub fn wants(&self, id: &TrackId) -> bool {
        !matches!(
            self.found.get(id),
            Some(Found::Nothing | Found::Instrumental | Found::Sheet(_))
        )
    }

    pub fn scroll_by(&mut self, down: bool) {
        self.following = false;
        if down {
            self.scroll += 1;
        } else {
            self.scroll = self.scroll.saturating_sub(1);
        }
    }

    /// Back to the top, following the music, for the track that starts.
    pub fn restart(&mut self) {
        self.scroll = 0;
        self.following = true;
    }
}

/// The query sonora's providers take, from what the shell knows of a
/// track. `provider` is the service's slug, which lets a provider that is
/// the service itself look the track up by id.
pub fn query(track: &Track, provider: &'static str) -> music::LyricsQuery {
    music::LyricsQuery {
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: (!track.album.is_empty()).then(|| track.album.clone()),
        duration: track.duration,
        track: Some(music::TrackKey {
            provider,
            id: track.id.0.clone(),
        }),
    }
}

/// The providers asked, in sonora's own order less the ones that need a
/// signed-in service of their own.
pub fn providers() -> Vec<Arc<dyn music::LyricsProvider>> {
    vec![
        Arc::new(music::binimum::Binimum::new()),
        Arc::new(music::musixmatch::Musixmatch::new()),
        Arc::new(music::lrclib::LrcLib::new()),
        Arc::new(music::kugou::Kugou::new()),
        Arc::new(music::netease::NetEase::new()),
    ]
}

/// Ask every provider at once and keep the best answer, by sonora's
/// ranking: synced over plain, a close match over a loose one.
pub async fn search(query: music::LyricsQuery) -> Found {
    let asked = providers();
    let answers = futures::future::join_all(asked.iter().map(|p| p.search(&query))).await;
    let mut hits = Vec::new();
    for (provider, answer) in asked.iter().zip(answers) {
        match answer {
            Ok(found) => hits.extend(found),
            Err(error) => tracing::info!("lyrics: {} answered with {error:#}", provider.name()),
        }
    }
    if music::lyrics::instrumental(&query, &hits) {
        return Found::Instrumental;
    }
    let mut ranked = music::lyrics::rank(&query, hits);
    music::lyrics::reshape(&mut ranked);
    match ranked.into_iter().next() {
        Some(hit) if !hit.lyrics.is_empty() => Found::Sheet(hit.lyrics),
        _ => Found::Nothing,
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

/// The lines a synced sheet draws, the active one marked, with the
/// romanisation under a line that has one.
fn synced_lines<'a>(
    lines: &'a [music::LyricsLine],
    at: Duration,
    palette: &Palette,
) -> (Vec<Line<'a>>, Option<usize>) {
    let active = music::lyrics::active(lines, at);
    let mut out = Vec::with_capacity(lines.len() * 2);
    let mut active_row = None;
    for (i, line) in lines.iter().enumerate() {
        let style = match active {
            Some(a) if a == i => palette.title(),
            Some(a) if i < a => palette.subtitle(),
            _ => palette.row_unfocused(),
        };
        if active == Some(i) {
            active_row = Some(out.len());
        }
        let text = if line.text.trim().is_empty() {
            "♪"
        } else {
            line.text.as_str()
        };
        out.push(Line::from(Span::styled(text, style)));
        if let Some(romanized) = &line.romanized {
            out.push(Line::from(Span::styled(
                romanized.text.as_str(),
                palette.subtitle(),
            )));
        }
    }
    (out, active_row)
}

/// Draw the pane over `area`. `at` is the position in the track, which a
/// synced sheet follows.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    pane: &Pane,
    track: Option<&Track>,
    at: Duration,
) {
    let modal = centred(area, 72, area.height.saturating_sub(2));
    if modal.width < 4 || modal.height < 3 {
        return;
    }
    frame.render_widget(Clear, modal);
    let title = match track {
        Some(t) => format!(" {} — {} ", t.title, t.artist),
        None => " Lyrics ".to_string(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(palette.rule());
    let inner = block.inner(modal);
    frame.render_widget(block, modal);

    let found = track.and_then(|t| pane.found.get(&t.id));
    let (lines, active_row): (Vec<Line>, Option<usize>) = match found {
        None if track.is_none() => (vec![Line::from("Nothing is playing.")], None),
        None | Some(Found::Searching) => (vec![Line::from("Looking for the words…")], None),
        Some(Found::Nothing) => (vec![Line::from("No lyrics found for this track.")], None),
        Some(Found::Instrumental) => (vec![Line::from("♪ Instrumental ♪")], None),
        Some(Found::Sheet(music::Lyrics::Plain { text, .. })) => {
            (text.lines().map(|l| Line::from(l.to_string())).collect(), None)
        }
        Some(Found::Sheet(music::Lyrics::Synced { lines })) => synced_lines(lines, at, palette),
    };

    // Following: the active line sits in the middle of the pane. Scrolled
    // by hand: wherever the user put it.
    let height = inner.height as usize;
    let scroll = match (pane.following, active_row) {
        (true, Some(row)) => row.saturating_sub(height / 2),
        _ => pane.scroll.min(lines.len().saturating_sub(1)),
    };
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .scroll((scroll as u16, 0)),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet() -> music::Lyrics {
        let line = |start: u64, text: &str| music::LyricsLine {
            start: Duration::from_secs(start),
            end: None,
            text: text.into(),
            romanized: None,
            words: None,
            secondary: Vec::new(),
            voice: music::Voice::default(),
        };
        music::Lyrics::Synced {
            lines: Arc::from(vec![line(0, "First"), line(10, "Second"), line(20, "Third")]),
        }
    }

    fn drawn(pane: &Pane, track: Option<&Track>, at: Duration) -> String {
        let buf = crate::shell::geometry::draw(80, 12, |f, area, p| {
            render(f, area, p, pane, track, at)
        });
        crate::shell::geometry::text(&buf)
    }

    #[test]
    fn a_synced_sheet_marks_the_line_being_sung() {
        let sheet = sheet();
        let music::Lyrics::Synced { lines } = &sheet else {
            unreachable!()
        };
        let (lines, active) = synced_lines(lines, Duration::from_secs(12), &Palette::detect());
        assert_eq!(lines.len(), 3);
        assert_eq!(active, Some(1), "ten seconds in: the second line");
    }

    #[test]
    fn the_pane_says_what_it_is_doing_and_then_what_it_found() {
        let track = Track::sample("Song", "Someone", Duration::from_secs(100));
        let mut pane = Pane::default();
        assert!(pane.wants(&track.id), "nothing held: ask");
        assert!(drawn(&pane, Some(&track), Duration::ZERO).contains("Looking"));

        pane.found.insert(track.id.clone(), Found::Instrumental);
        assert!(!pane.wants(&track.id));
        assert!(drawn(&pane, Some(&track), Duration::ZERO).contains("Instrumental"));

        pane.found.insert(track.id.clone(), Found::Sheet(sheet()));
        let text = drawn(&pane, Some(&track), Duration::from_secs(12));
        assert!(text.contains("Second"), "{text}");
        assert!(text.contains("Song — Someone"), "headed by the track:\n{text}");

        assert!(drawn(&pane, None, Duration::ZERO).contains("Nothing is playing"));
    }

    #[test]
    fn scrolling_by_hand_stops_following_until_the_next_track() {
        let mut pane = Pane::default();
        pane.toggle();
        assert!(pane.open && pane.following);
        pane.scroll_by(true);
        pane.scroll_by(true);
        pane.scroll_by(false);
        assert_eq!((pane.scroll, pane.following), (1, false));
        pane.restart();
        assert!(pane.following);
        assert_eq!(pane.scroll, 0);
    }

    #[test]
    fn the_query_carries_the_service_and_the_id_so_a_provider_can_look_it_up() {
        let mut track = Track::sample("Song", "Someone", Duration::from_secs(100));
        track.album = String::new();
        let q = query(&track, "tidal");
        assert_eq!(q.title, "Song");
        assert!(q.album.is_none(), "an empty album is no album");
        assert_eq!(q.track.as_ref().map(|k| k.provider), Some("tidal"));
        assert_eq!(q.id_for("tidal"), Some(track.id.as_str()));
    }
}
