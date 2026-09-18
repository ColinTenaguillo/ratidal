//! What plays next.
//!
//! The engine plays one track and stops; everything about "next" lives here.
//! Kept apart from both the engine and the shell so the rules — what shuffle
//! does to an order, what repeat does at the end of a list — can be tested
//! without a sound card or a network.
//!
//! Shuffle reorders a *view* of the queue rather than the queue itself. The
//! alternative, shuffling the list in place, loses the original order, so
//! turning shuffle off afterwards cannot put it back.

use crate::domain::{Track, TrackId};

/// What happens at the end of the queue.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    /// Stop at the end.
    #[default]
    Off,
    /// Start the queue again.
    All,
    /// Play the same track again.
    One,
}

impl Repeat {
    /// The next mode, as pressing the button cycles them.
    ///
    /// Not `next`: a queue has a `next` track, and a method of the same name
    /// on the mode reads as iteration.
    pub fn cycle(self) -> Self {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Repeat::Off => "off",
            Repeat::All => "all",
            Repeat::One => "one",
        }
    }
}

/// Where a queued track came from, which decides what outlives what.
///
/// Every player worth copying keeps these apart. Apple Music draws "Playing
/// Next" above "AutoPlay"; TIDAL's own client does the same. One flat list
/// cannot: starting a new album has to replace the album you were on
/// without throwing away the track you queued by hand, and it has to drop
/// the radio the last album trailed rather than resurrecting it later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Asked for by name -- "play this next". Survives a change of context,
    /// because the user meant this track rather than this album.
    User,
    /// The album, playlist or mix the current track was started from.
    /// Replaced wholesale when another is started.
    Context,
    /// Filled in by autoplay when everything else ran out. Always last, and
    /// dropped the moment there is anything real to play.
    Autoplay,
}

/// The tracks queued up, and where in them the player is.
///
/// One list, ordered `User` then `Context` then `Autoplay`, rather than
/// three lists to keep in step: shuffle, repeat and the position cursor all
/// work on indices, and three of everything would mean three of those too.
#[derive(Debug, Default, Clone)]
pub struct Queue {
    tracks: Vec<Track>,
    /// Where each track came from, one per entry of `tracks`.
    sources: Vec<Source>,
    /// Position in `order`, not in `tracks` — with shuffle on they differ.
    at: usize,
    /// The order to play in. Indices into `tracks`.
    order: Vec<usize>,
    shuffled: bool,
    pub repeat: Repeat,
    /// What the context is called: the album, playlist or mix the current
    /// tracks came from. The web says "Next up from: Discovery" rather
    /// than a bare heading, which is the difference between knowing what
    /// is coming and knowing why.
    context: Option<String>,
    /// What has been played, newest first.
    ///
    /// Kept rather than dropped so the user can see what a track was --
    /// "what was that?" is a question a player should be able to answer,
    /// and the web keeps a History section for it. Capped: this is a
    /// convenience, not a listening log, and an unbounded one would grow
    /// for the life of the process.
    history: std::collections::VecDeque<Track>,
}

/// How many played tracks to remember.
///
/// Enough to answer "what was the one before last"; not so many that the
/// list becomes something to scroll through rather than glance at.
const HISTORY: usize = 50;

impl Queue {
    /// Queue `tracks` and start at `start`.
    ///
    /// Playing a track from a list queues the whole list, which is what
    /// makes an album play on rather than stopping after one track.
    pub fn new(tracks: Vec<Track>, start: usize) -> Self {
        let mut q = Queue {
            at: start.min(tracks.len().saturating_sub(1)),
            order: (0..tracks.len()).collect(),
            sources: vec![Source::Context; tracks.len()],
            tracks,
            shuffled: false,
            repeat: Repeat::Off,
            context: None,
            history: std::collections::VecDeque::new(),
        };
        q.at = q.position_of_track(q.at);
        q
    }

    /// Start a new context, keeping what the user queued by hand.
    ///
    /// This is what playing a track from a list does. The album that was
    /// playing goes, and so does any autoplay trailing it -- that radio was
    /// chosen to follow a track nobody is listening to any more. What the
    /// user asked for by name stays, ahead of the new context, because they
    /// asked for the track rather than for the album it came from.
    pub fn start_context(
        &mut self,
        tracks: Vec<Track>,
        start: usize,
        named: Option<String>,
    ) {
        let played = self.at;
        let mut kept: Vec<Track> = Vec::new();
        for (i, track) in self.tracks.drain(..).enumerate() {
            // Only what is still ahead: a hand-queued track already played
            // is as done as any other.
            let ahead = self.order.iter().position(|o| *o == i).is_some_and(|p| p > played);
            if self.sources.get(i) == Some(&Source::User) && ahead {
                kept.push(track);
            }
        }

        let kept_len = kept.len();
        let mut all = kept;
        all.extend(tracks);
        // The new context's own starting track, shifted past what was kept.
        let at = kept_len + start;

        self.sources = std::iter::repeat_n(Source::User, kept_len)
            .chain(std::iter::repeat_n(Source::Context, all.len() - kept_len))
            .collect();
        self.order = (0..all.len()).collect();
        self.tracks = all;
        self.shuffled = false;
        self.at = at.min(self.tracks.len().saturating_sub(1));
        self.context = named;
        tracing::debug!(
            tracks = self.tracks.len(),
            kept = kept_len,
            at = self.at,
            context = ?self.context,
            "queue: context started"
        );
    }

    /// Put a track next, ahead of everything but what is playing.
    ///
    /// Several in a row keep the order they were asked for, which is what
    /// "play next" means everywhere else.
    pub fn play_next(&mut self, track: Track) {
        let after = self.insert_point();
        self.tracks.insert(after, track);
        self.sources.insert(after, Source::User);
        self.reindex(after);
    }

    /// Put a track at the end of what the user asked for, before the
    /// context carries on. "Play last" in every other player.
    pub fn play_last(&mut self, track: Track) {
        let at = self.tracks.len();
        self.tracks.push(track);
        self.sources.push(Source::User);
        self.reindex(at);
    }

    /// Where a "play next" track goes: after the current one, and after any
    /// hand-queued tracks already waiting behind it.
    fn insert_point(&self) -> usize {
        let playing = self.order.get(self.at).copied().unwrap_or(0);
        let mut at = playing + 1;
        while self.sources.get(at) == Some(&Source::User) {
            at += 1;
        }
        at.min(self.tracks.len())
    }

    /// Rebuild `order` after an insert at `at`, keeping the cursor on the
    /// track that is playing.
    ///
    /// Shuffle is dropped on insert: a shuffled order is a permutation of
    /// the old length, and there is no honest place to put a new index in
    /// one. The alternative is silently playing the wrong track.
    fn reindex(&mut self, at: usize) {
        let playing = self.order.get(self.at).copied();
        self.order = (0..self.tracks.len()).collect();
        self.shuffled = false;
        if let Some(track) = playing {
            // Everything at or past the insert shifted up by one.
            let moved = if track >= at { track + 1 } else { track };
            self.at = self.position_of_track(moved);
        }
    }

    /// Add tracks autoplay found, at the very end.
    ///
    /// Replaces any autoplay already waiting: it was chosen to follow a
    /// different track, and two radios queued back to back is not what
    /// following one means.
    pub fn set_autoplay(&mut self, tracks: Vec<Track>) {
        let playing = self.order.get(self.at).copied();
        let keep: Vec<usize> = (0..self.tracks.len())
            .filter(|i| self.sources.get(*i) != Some(&Source::Autoplay))
            .collect();
        let dropped_before = |i: usize| keep.iter().take_while(|k| **k < i).count();

        self.tracks = keep.iter().map(|i| self.tracks[*i].clone()).collect();
        self.sources = keep.iter().map(|i| self.sources[*i]).collect();
        let at_track = playing.map(dropped_before);

        self.sources
            .extend(std::iter::repeat_n(Source::Autoplay, tracks.len()));
        self.tracks.extend(tracks);
        self.order = (0..self.tracks.len()).collect();
        self.shuffled = false;
        self.at = at_track.unwrap_or(0).min(self.tracks.len().saturating_sub(1));
    }

    /// What the context is called, for the heading over it.
    pub fn context_name(&self) -> Option<&str> {
        self.context.as_deref()
    }

    /// What has been played, newest first.
    pub fn history(&self) -> impl Iterator<Item = &Track> {
        self.history.iter()
    }

    /// Remember a track as played.
    ///
    /// Called as each one starts rather than as it ends: a track skipped
    /// half way through is still one the user heard and may want to name.
    /// The same track twice in a row is recorded once -- repeat-one would
    /// otherwise fill the history with a single title.
    pub fn remember(&mut self, track: &Track) {
        if self.history.front().map(|t| t.id) == Some(track.id) {
            return;
        }
        self.history.push_front(track.clone());
        self.history.truncate(HISTORY);
    }

    /// Move the cursor to a position in play order and return what is
    /// there. What the queue view's enter does.
    pub fn jump_to(&mut self, at: usize) -> Option<&Track> {
        if at >= self.order.len() {
            return None;
        }
        self.at = at;
        self.current()
    }

    /// How many tracks are still to play after this one.
    ///
    /// What autoplay watches: waiting for the queue to empty means the
    /// fetch starts at the silence, and the user hears the gap. The web
    /// client fills its "up next" while the current track is still going.
    pub fn remaining(&self) -> usize {
        self.order.len().saturating_sub(self.at + 1)
    }

    /// Whether anything in the queue was put there by autoplay.
    ///
    /// Asked before fetching more: a radio already queued is the radio
    /// that is about to play, and fetching another would replace it every
    /// time a track ended.
    pub fn has_autoplay(&self) -> bool {
        self.sources.contains(&Source::Autoplay)
    }

    /// Take a track out of the queue, by its place in play order.
    ///
    /// Removing what is playing is the caller's business rather than
    /// forbidden here: the queue says what is left and moves the cursor to
    /// whatever now sits in that place, and the shell decides whether that
    /// means starting it or stopping.
    ///
    /// Returns whether the track that was playing is now gone, which is the
    /// one thing the caller cannot work out afterwards.
    pub fn remove(&mut self, at: usize) -> bool {
        let Some(track) = self.order.get(at).copied() else {
            return false;
        };
        let was_playing = at == self.at;

        self.tracks.remove(track);
        self.sources.remove(track);
        // `order` holds indices into `tracks`, so everything past the hole
        // shifts down with it.
        self.order.remove(at);
        for i in self.order.iter_mut() {
            if *i > track {
                *i -= 1;
            }
        }
        // A track taken from behind the cursor shifts everything after it
        // down, the cursor included, or it would land one place further on
        // and play something nobody chose. One from ahead leaves it where
        // it is, and one at the cursor gives its place to whatever
        // followed -- except at the end, where it steps back onto the last.
        if at < self.at {
            self.at -= 1;
        }
        self.at = self.at.min(self.order.len().saturating_sub(1));
        was_playing
    }

    /// What each queued track is, in play order. For the queue view.
    pub fn entries(&self) -> Vec<(&Track, Source)> {
        self.order
            .iter()
            .filter_map(|i| Some((self.tracks.get(*i)?, *self.sources.get(*i)?)))
            .collect()
    }

    /// How far into the queue the player is, in play order.
    pub fn position(&self) -> usize {
        self.at
    }

    /// Where a track index sits in the current order.
    fn position_of_track(&self, track: usize) -> usize {
        self.order.iter().position(|i| *i == track).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn shuffled(&self) -> bool {
        self.shuffled
    }

    /// The track playing now.
    pub fn current(&self) -> Option<&Track> {
        self.tracks.get(*self.order.get(self.at)?)
    }

    /// Step forward, honouring repeat. `None` means the queue is finished.
    ///
    /// Named for the button rather than for iteration: it borrows `self`
    /// mutably and returns a reference, which no `Iterator` does.
    ///
    /// `Repeat::One` is deliberately not applied here: it belongs to a track
    /// ending on its own, not to someone pressing next — pressing next on a
    /// repeating track should still move.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&Track> {
        if self.tracks.is_empty() {
            return None;
        }
        if self.at + 1 < self.order.len() {
            self.at += 1;
        } else if self.repeat == Repeat::All {
            self.at = 0;
        } else {
            return None;
        }
        self.current()
    }

    /// Step back. Stops at the first track rather than wrapping, unless the
    /// whole queue repeats.
    pub fn previous(&mut self) -> Option<&Track> {
        if self.tracks.is_empty() {
            return None;
        }
        if self.at > 0 {
            self.at -= 1;
        } else if self.repeat == Repeat::All {
            self.at = self.order.len().saturating_sub(1);
        } else {
            return None;
        }
        self.current()
    }

    /// What follows the track that just ended on its own.
    ///
    /// Unlike `next`, this honours `Repeat::One`, which is the difference
    /// between the mode and the button.
    pub fn advance(&mut self) -> Option<&Track> {
        if self.repeat == Repeat::One {
            return self.current();
        }
        self.next()
    }

    /// Turn shuffle on or off, keeping the current track under the cursor.
    ///
    /// Reordering a view rather than the list itself: turning shuffle off
    /// has to restore the order it was queued in, which shuffling in place
    /// would have thrown away.
    pub fn set_shuffled(&mut self, on: bool, rng: &mut impl FnMut(usize) -> usize) {
        if on == self.shuffled {
            return;
        }
        let playing = self.order.get(self.at).copied();
        self.shuffled = on;
        self.order = (0..self.tracks.len()).collect();
        if on {
            // Fisher-Yates, with the caller's source of randomness so the
            // shuffle can be made deterministic in a test.
            for i in (1..self.order.len()).rev() {
                self.order.swap(i, rng(i + 1));
            }
        }
        if let Some(track) = playing {
            self.at = self.position_of_track(track);
        }
    }

    pub fn contains(&self, id: TrackId) -> bool {
        self.tracks.iter().any(|t| t.id == id)
    }
}

/// A shuffle source that needs no dependency.
///
/// A xorshift seeded from the clock. Nothing here needs cryptographic
/// randomness or an even distribution to three decimal places — it needs a
/// different order each time the button is pressed, which this gives without
/// pulling in a crate for it.
pub fn clock_rng() -> impl FnMut(usize) -> usize {
    let mut state = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0x2545_F491_4F6C_DD1D, |d| d.as_nanos() as u64)
        | 1;
    move |bound| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        if bound == 0 {
            0
        } else {
            (state % bound as u64) as usize
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A track that is not from `tracks(n)`, so it is told apart by title.
    fn named(title: &str, id: u64) -> Track {
        let mut t = Track::sample(title, "Someone", Duration::from_secs(100));
        t.id = TrackId(id);
        t
    }

    #[test]
    fn starting_an_album_drops_the_one_before_it() {
        // The reported fault: changing track mid-album, then hearing the
        // old album resume when the new one ended. A queue is what is
        // playing now, not everything ever played.
        let mut q = Queue::new(tracks(3), 0);
        q.start_context(vec![named("New 0", 10), named("New 1", 11)], 0, None);

        let titles: Vec<&str> = q.entries().iter().map(|(t, _)| t.title.as_str()).collect();
        assert_eq!(titles, ["New 0", "New 1"], "only the album just started");
        assert_eq!(q.current().map(|t| t.title.as_str()), Some("New 0"));
    }

    #[test]
    fn what_the_user_queued_by_hand_outlives_the_album_it_was_queued_over() {
        // The whole reason the three sources exist. "Play this next" is
        // about the track, so changing album must not throw it away --
        // Apple Music and TIDAL both keep it, above the new context.
        let mut q = Queue::new(tracks(3), 0);
        q.play_next(named("By hand", 99));
        q.start_context(tracks(2), 0, None);

        let entries = q.entries();
        assert_eq!(
            entries[0].0.title, "By hand",
            "still first: {:?}",
            entries.iter().map(|(t, _)| &t.title).collect::<Vec<_>>()
        );
        assert_eq!(entries[0].1, Source::User);
        assert_eq!(entries[1].1, Source::Context, "then the new album");
    }

    #[test]
    fn a_hand_queued_track_that_already_played_is_not_kept() {
        // It is as done as any other played track; keeping it would replay
        // it every time the user started something new.
        let mut q = Queue::new(tracks(2), 0);
        q.play_next(named("By hand", 99));
        q.next();
        assert_eq!(q.current().map(|t| t.title.as_str()), Some("By hand"));

        q.start_context(tracks(2), 0, None);
        let titles: Vec<&str> = q.entries().iter().map(|(t, _)| t.title.as_str()).collect();
        assert!(!titles.contains(&"By hand"), "played, so gone: {titles:?}");
    }

    #[test]
    fn play_next_goes_after_the_current_track_and_keeps_its_order() {
        let mut q = Queue::new(tracks(3), 0);
        q.play_next(named("First asked", 90));
        q.play_next(named("Second asked", 91));

        let titles: Vec<&str> = q.entries().iter().map(|(t, _)| t.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Track 0", "First asked", "Second asked", "Track 1", "Track 2"],
            "the order they were asked for, not reversed"
        );
        assert_eq!(q.current().map(|t| t.title.as_str()), Some("Track 0"));
    }

    #[test]
    fn autoplay_sits_at_the_end_and_replaces_the_radio_before_it() {
        // Two radios back to back is not what following one means: the
        // second was chosen to follow a track the first one displaced.
        let mut q = Queue::new(tracks(2), 0);
        q.set_autoplay(vec![named("Radio A", 50)]);
        q.set_autoplay(vec![named("Radio B", 51)]);

        let entries = q.entries();
        let titles: Vec<&str> = entries.iter().map(|(t, _)| t.title.as_str()).collect();
        assert_eq!(titles, ["Track 0", "Track 1", "Radio B"], "one radio, the latest");
        assert_eq!(entries[2].1, Source::Autoplay);
        assert_eq!(q.current().map(|t| t.title.as_str()), Some("Track 0"),
            "and what is playing did not move");
    }

    #[test]
    fn probe_queue_shape() {
        let mut q = Queue::new(tracks(4), 0);
        q.next(); // playing Track 1
        eprintln!("--- playing Track 1, then queue two by hand ---");
        q.play_next(named("Hand A", 90));
        q.play_next(named("Hand B", 91));
        for (i, (t, s)) in q.entries().iter().enumerate() {
            let mark = if i == q.position() { ">" } else { " " };
            eprintln!("{mark} {:?}  {}", s, t.title);
        }
        eprintln!("--- then play_last ---");
        q.play_last(named("Hand C", 92));
        for (i, (t, s)) in q.entries().iter().enumerate() {
            let mark = if i == q.position() { ">" } else { " " };
            eprintln!("{mark} {:?}  {}", s, t.title);
        }
    }

    #[test]
    fn the_context_carries_the_name_it_was_started_from() {
        // "Next up from Discovery" rather than a bare heading: the web says
        // which album is coming, and that is the difference between knowing
        // what plays next and knowing why.
        let mut q = Queue::new(tracks(2), 0);
        assert_eq!(q.context_name(), None, "nothing was named yet");

        q.start_context(tracks(3), 0, Some("Discovery".into()));
        assert_eq!(q.context_name(), Some("Discovery"));
    }

    #[test]
    fn what_has_been_played_is_remembered_newest_first() {
        let mut q = Queue::new(tracks(3), 0);
        for i in 0..3 {
            let t = q.entries()[i].0.clone();
            q.remember(&t);
        }
        let played: Vec<&str> = q.history().map(|t| t.title.as_str()).collect();
        assert_eq!(played, ["Track 2", "Track 1", "Track 0"], "newest first");
    }

    #[test]
    fn the_same_track_twice_running_is_remembered_once() {
        // Repeat-one would otherwise fill the history with one title.
        let mut q = Queue::new(tracks(1), 0);
        let t = q.entries()[0].0.clone();
        q.remember(&t);
        q.remember(&t);
        assert_eq!(q.history().count(), 1);
    }

    #[test]
    fn the_history_stops_growing_at_its_cap() {
        // A convenience, not a listening log: unbounded it would grow for
        // the life of the process.
        let mut q = Queue::new(tracks(1), 0);
        for i in 0..HISTORY + 20 {
            let mut t = q.entries()[0].0.clone();
            t.id = TrackId(i as u64 + 1000);
            t.title = format!("Played {i}");
            q.remember(&t);
        }
        assert_eq!(q.history().count(), HISTORY);
        assert_eq!(
            q.history().next().map(|t| t.title.as_str()),
            Some(format!("Played {}", HISTORY + 19).as_str()),
            "and the newest survived rather than the oldest"
        );
    }

    #[test]
    fn removing_a_track_ahead_leaves_the_rest_in_order() {
        let mut q = Queue::new(tracks(4), 0);
        assert!(!q.remove(2), "what is playing was not the one removed");

        let titles: Vec<&str> = q.entries().iter().map(|(t, _)| t.title.as_str()).collect();
        assert_eq!(titles, ["Track 0", "Track 1", "Track 3"]);
        assert_eq!(
            q.current().map(|t| t.title.as_str()),
            Some("Track 0"),
            "and the cursor did not move"
        );
    }

    #[test]
    fn removing_a_track_behind_keeps_the_cursor_on_what_is_playing() {
        // `at` is a position in the order, so dropping something before it
        // shifts everything down -- forgetting that plays the wrong track.
        let mut q = Queue::new(tracks(4), 0);
        q.next();
        q.next();
        assert_eq!(q.current().map(|t| t.title.as_str()), Some("Track 2"));

        assert!(!q.remove(0), "Track 0 was not playing");
        assert_eq!(
            q.current().map(|t| t.title.as_str()),
            Some("Track 2"),
            "still the same track, one place earlier"
        );
    }

    #[test]
    fn removing_what_is_playing_says_so_and_moves_on() {
        let mut q = Queue::new(tracks(3), 0);
        q.next();
        assert!(q.remove(1), "the playing track went");
        assert_eq!(
            q.current().map(|t| t.title.as_str()),
            Some("Track 2"),
            "whatever followed takes its place"
        );
    }

    #[test]
    fn removing_the_last_track_steps_back_rather_than_off_the_end() {
        let mut q = Queue::new(tracks(2), 0);
        q.next();
        assert!(q.remove(1));
        assert_eq!(q.current().map(|t| t.title.as_str()), Some("Track 0"));
    }

    #[test]
    fn removing_the_only_track_empties_the_queue() {
        let mut q = Queue::new(tracks(1), 0);
        assert!(q.remove(0));
        assert!(q.is_empty(), "nothing left");
        assert!(q.current().is_none(), "and nothing playing");
    }

    #[test]
    fn removing_out_of_range_changes_nothing() {
        let mut q = Queue::new(tracks(2), 0);
        assert!(!q.remove(9));
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn removing_from_a_shuffled_queue_takes_the_track_that_was_shown() {
        // `order` is a permutation, so the position the view drew is not
        // the index into `tracks`. Removing the wrong one is invisible
        // until the queue plays something nobody asked for.
        let mut q = Queue::new(tracks(4), 0);
        q.set_shuffled(true, &mut |n| n - 1);
        let shown: Vec<String> = q
            .entries()
            .iter()
            .map(|(t, _)| t.title.clone())
            .collect();

        q.remove(2);
        let after: Vec<String> = q
            .entries()
            .iter()
            .map(|(t, _)| t.title.clone())
            .collect();
        let mut expected = shown.clone();
        expected.remove(2);
        assert_eq!(after, expected, "the one at that position, not at that index");
    }

    #[test]
    fn autoplay_added_to_a_finished_queue_is_what_plays_next() {
        // The case autoplay actually fires in: the queue ran out, and what
        // it appends has to be reachable. Leaving the cursor on the last
        // played track and appending behind it would sit silent.
        let mut q = Queue::new(tracks(2), 0);
        q.next();
        assert!(q.next().is_none(), "the queue is finished");

        q.set_autoplay(vec![named("Radio", 50)]);
        assert_eq!(
            q.next().map(|t| t.title.as_str()),
            Some("Radio"),
            "the radio is the next thing to play"
        );
    }

    #[test]
    fn starting_an_album_drops_the_radio_that_trailed_the_last_one() {
        // That radio was picked to follow a track nobody is listening to.
        let mut q = Queue::new(tracks(2), 0);
        q.set_autoplay(vec![named("Radio", 50)]);
        q.start_context(tracks(2), 0, None);

        let titles: Vec<&str> = q.entries().iter().map(|(t, _)| t.title.as_str()).collect();
        assert!(!titles.contains(&"Radio"), "the old radio went: {titles:?}");
    }

    fn tracks(n: usize) -> Vec<Track> {
        (0..n)
            .map(|i| {
                let mut t = Track::sample(
                    &format!("Track {i}"),
                    "An Artist",
                    Duration::from_secs(100),
                );
                t.id = TrackId(i as u64);
                t
            })
            .collect()
    }

    /// A deterministic stand-in for a random number source.
    fn fixed(values: Vec<usize>) -> impl FnMut(usize) -> usize {
        let mut it = values.into_iter();
        move |bound| it.next().unwrap_or(0).min(bound.saturating_sub(1))
    }

    #[test]
    fn playing_a_track_queues_the_list_it_came_from() {
        // The point of the queue: an album played from its third track goes
        // on to the fourth rather than stopping.
        let q = Queue::new(tracks(5), 2);
        assert_eq!(q.current().unwrap().title, "Track 2");
        assert_eq!(q.len(), 5, "the whole list is queued, not just one track");
    }

    #[test]
    fn next_runs_to_the_end_and_then_stops() {
        let mut q = Queue::new(tracks(3), 0);
        assert_eq!(q.next().unwrap().title, "Track 1");
        assert_eq!(q.next().unwrap().title, "Track 2");
        assert!(q.next().is_none(), "the queue is finished");
    }

    #[test]
    fn previous_stops_at_the_first_track() {
        let mut q = Queue::new(tracks(3), 1);
        assert_eq!(q.previous().unwrap().title, "Track 0");
        assert!(q.previous().is_none(), "there is nothing before the first");
    }

    #[test]
    fn repeat_all_wraps_at_both_ends() {
        let mut q = Queue::new(tracks(3), 2);
        q.repeat = Repeat::All;
        assert_eq!(q.next().unwrap().title, "Track 0", "past the end is the start");
        assert_eq!(
            q.previous().unwrap().title,
            "Track 2",
            "and before the start is the end"
        );
    }

    #[test]
    fn repeat_one_holds_a_track_that_ends_but_not_the_next_button() {
        // The mode is about a track finishing on its own. Pressing next on a
        // repeating track that would not move is a button that does nothing.
        let mut q = Queue::new(tracks(3), 0);
        q.repeat = Repeat::One;
        assert_eq!(
            q.advance().unwrap().title,
            "Track 0",
            "a track that ends plays again"
        );
        assert_eq!(
            q.next().unwrap().title,
            "Track 1",
            "but next still moves on"
        );
    }

    #[test]
    fn a_finished_queue_advances_to_nothing() {
        let mut q = Queue::new(tracks(1), 0);
        assert!(q.advance().is_none(), "one track, no repeat, nothing after");
    }

    #[test]
    fn shuffle_keeps_the_track_that_is_playing() {
        // Turning shuffle on mid-track must not jump to something else.
        let mut q = Queue::new(tracks(5), 3);
        let before = q.current().unwrap().id;
        q.set_shuffled(true, &mut fixed(vec![0, 1, 0, 1]));
        assert_eq!(q.current().unwrap().id, before, "still on the same track");
        assert!(q.shuffled());
    }

    #[test]
    fn turning_shuffle_off_restores_the_order_it_was_queued_in() {
        // Shuffling the list in place would lose the original order, so
        // there would be nothing to go back to.
        let mut q = Queue::new(tracks(5), 0);
        q.set_shuffled(true, &mut fixed(vec![3, 2, 1, 0]));
        q.set_shuffled(false, &mut fixed(vec![]));

        let titles: Vec<String> = (0..5)
            .map(|_| {
                let t = q.current().unwrap().title.clone();
                q.next();
                t
            })
            .collect();
        assert_eq!(
            titles,
            vec!["Track 0", "Track 1", "Track 2", "Track 3", "Track 4"],
            "back in the order it was queued"
        );
    }

    #[test]
    fn shuffle_still_reaches_every_track_exactly_once() {
        // A shuffle that drops or repeats a track is worse than none.
        let mut q = Queue::new(tracks(6), 0);
        q.set_shuffled(true, &mut fixed(vec![4, 0, 2, 1, 0]));

        // From the top of the shuffled order, not from wherever the current
        // track landed in it — walking forward from there only ever sees the
        // tail.
        q.at = 0;
        let mut seen: Vec<u64> = vec![q.current().unwrap().id.0];
        while let Some(t) = q.next() {
            seen.push(t.id.0);
        }
        seen.sort_unstable();
        assert_eq!(seen, vec![0, 1, 2, 3, 4, 5], "every track, once each");
    }

    #[test]
    fn an_empty_queue_does_nothing_rather_than_panicking() {
        let mut q = Queue::new(Vec::new(), 0);
        assert!(q.is_empty());
        assert!(q.current().is_none());
        assert!(q.next().is_none());
        assert!(q.previous().is_none());
        assert!(q.advance().is_none());
    }

    #[test]
    fn an_empty_queue_survives_every_mode() {
        // Repeat-all wraps `previous` to the last track, which on an empty
        // queue is one before the first. Nothing walked that path.
        for repeat in [Repeat::Off, Repeat::All, Repeat::One] {
            let mut q = Queue::new(Vec::new(), 0);
            q.repeat = repeat;
            assert!(q.previous().is_none(), "previous with {repeat:?}");
            assert!(q.next().is_none(), "next with {repeat:?}");
            assert!(q.advance().is_none(), "advance with {repeat:?}");
            // And the position stayed sane: wrapping past zero leaves an
            // index no later track could ever be at.
            assert_eq!(q.at, 0, "the position after {repeat:?}");
        }
    }

    #[test]
    fn starting_past_the_end_of_an_empty_queue_is_the_start_of_nothing() {
        // `start.min(len - 1)` on an empty queue subtracts below zero;
        // wrapping there gives an enormous index and `current` reads past
        // the end of the order.
        let q = Queue::new(Vec::new(), 5);
        assert!(q.current().is_none(), "there is nothing to be at");
        // `current` reads with `get`, so a wrapped index is `None` all the
        // same — the position itself is what must not be enormous, or the
        // first track queued after this lands unreachable.
        assert_eq!(q.at, 0, "the position is the start, not a wrapped one");
    }

    #[test]
    fn a_shuffle_can_reach_every_position() {
        // Fisher-Yates swaps each element with one at or below it, so the
        // range passed to the source of randomness is `i + 1` — one short
        // and the first element never moves; one long and it indexes past
        // the end.
        let tracks: Vec<Track> = (0..8)
            .map(|i| Track::sample(&format!("T{i}"), "An Artist", Duration::from_secs(1)))
            .collect();
        let mut q = Queue::new(tracks, 0);

        // A source that always picks the top of the range: every element
        // swaps with itself, so the order is unchanged and nothing is out
        // of bounds.
        q.set_shuffled(true, &mut |n| n - 1);
        assert_eq!(q.order, (0..8).collect::<Vec<_>>(), "each swapped with itself");

        // And one that always picks the bottom: every element swaps with
        // the first, which reaches position zero.
        q.set_shuffled(false, &mut |_| 0);
        q.set_shuffled(true, &mut |_| 0);
        assert_ne!(q.order, (0..8).collect::<Vec<_>>(), "the order moved");
        assert_eq!(q.order.len(), 8, "and kept every track");
    }

    #[test]
    fn a_start_past_the_end_lands_on_the_last_track() {
        let q = Queue::new(tracks(3), 99);
        assert_eq!(q.current().unwrap().title, "Track 2");
    }

    #[test]
    fn the_clock_shuffle_stays_inside_its_bound() {
        // An index past the end would panic on the swap.
        let mut rng = clock_rng();
        for bound in 1..50usize {
            let v = rng(bound);
            assert!(v < bound, "{v} is not below {bound}");
        }
        assert_eq!(rng(0), 0, "a zero bound is not a division by zero");
    }

    #[test]
    fn the_clock_shuffle_actually_mixes() {
        // A source that returns the same value every time would leave the
        // order untouched and shuffle would do nothing.
        let mut rng = clock_rng();
        let values: Vec<usize> = (0..20).map(|_| rng(100)).collect();
        assert!(
            values.iter().any(|v| *v != values[0]),
            "every draw came back the same: {values:?}"
        );
    }

    #[test]
    fn the_repeat_button_cycles_the_three_modes() {
        assert_eq!(Repeat::Off.cycle(), Repeat::All);
        assert_eq!(Repeat::All.cycle(), Repeat::One);
        assert_eq!(Repeat::One.cycle(), Repeat::Off, "and back round");
    }
}
