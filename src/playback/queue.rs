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

/// The tracks queued up, and where in them the player is.
#[derive(Debug, Default, Clone)]
pub struct Queue {
    tracks: Vec<Track>,
    /// Position in `order`, not in `tracks` — with shuffle on they differ.
    at: usize,
    /// The order to play in. Indices into `tracks`.
    order: Vec<usize>,
    shuffled: bool,
    pub repeat: Repeat,
}

impl Queue {
    /// Queue `tracks` and start at `start`.
    ///
    /// Playing a track from a list queues the whole list, which is what
    /// makes an album play on rather than stopping after one track.
    pub fn new(tracks: Vec<Track>, start: usize) -> Self {
        let mut q = Queue {
            at: start.min(tracks.len().saturating_sub(1)),
            order: (0..tracks.len()).collect(),
            tracks,
            shuffled: false,
            repeat: Repeat::Off,
        };
        q.at = q.position_of_track(q.at);
        q
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

    /// Where a given track sits, for marking the playing row.
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
