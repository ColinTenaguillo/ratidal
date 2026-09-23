//! The OS media keys: play/pause, next, previous.
//!
//! A terminal application never sees these. The keys are captured by the
//! desktop before any terminal gets them, and delivered only to whatever has
//! registered itself as a media player with the system. So this registers.
//!
//! There is no one way to do that. Linux has MPRIS, a D-Bus interface, and
//! macOS has `MPRemoteCommandCenter`, which needs a Core Foundation run loop
//! on the main thread. The two share nothing but the idea, so they are two
//! implementations behind one function.
//!
//! Both ends are already in the app: the commands become the same [`Action`]
//! the keyboard sends, and the state comes over a watch channel rather than
//! from `App`, which is not shared across threads.

use crate::domain::Track;

/// What the OS shows about the track, and what it needs to decide whether
/// "next" is even offered.
///
/// Sent over a watch channel: the media player lives in its own task, and
/// `App` is owned by the render loop.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub track: Option<Track>,
    pub playing: bool,
    pub position: std::time::Duration,
    /// Whether the queue has anything either side of the current track.
    pub can_next: bool,
    pub can_previous: bool,
}

impl State {
    /// Whether the OS needs telling. Compared by track id rather than by the
    /// whole track: the position moves every tick, and re-announcing the
    /// metadata that often makes the desktop redraw its player for nothing.
    pub fn differs_from(&self, other: &State) -> bool {
        self.playing != other.playing
            || self.can_next != other.can_next
            || self.can_previous != other.can_previous
            || self.track.as_ref().map(|t| t.id) != other.track.as_ref().map(|t| t.id)
    }
}

pub type StateSender = tokio::sync::watch::Sender<State>;
pub type StateReceiver = tokio::sync::watch::Receiver<State>;

/// A channel for the state the OS shows.
pub fn channel() -> (StateSender, StateReceiver) {
    tokio::sync::watch::channel(State::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_track() -> Track {
        Track {
            id: crate::domain::TrackId(1),
            title: "A Track".into(),
            artist: "Someone".into(),
            album: "An Album".into(),
            duration: std::time::Duration::from_secs(200),
            cover: None,
            tags: Vec::new(),
            added: None,
            explicit: false,
            ai: false,
            radio: None,
            album_id: None,
            artist_id: None,
        }
    }

    #[test]
    fn the_state_starts_empty_so_nothing_is_announced_before_a_track() {
        // The player registers with the OS at startup, before anything is
        // playing. Announcing a blank track there would put an empty entry
        // in the Control Center.
        let (_tx, rx) = channel();
        let state = rx.borrow();
        assert!(state.track.is_none(), "nothing to show yet");
        assert!(!state.playing);
        assert!(!state.can_next, "an empty queue offers no next");
    }

    #[test]
    fn a_sent_state_reaches_the_receiver() {
        let (tx, rx) = channel();
        tx.send(State {
            track: Some(a_track()),
            playing: true,
            position: std::time::Duration::from_secs(5),
            can_next: true,
            can_previous: false,
        })
        .expect("the receiver is alive");

        let state = rx.borrow();
        assert_eq!(
            state.track.as_ref().map(|t| t.title.as_str()),
            Some("A Track")
        );
        assert!(state.playing);
        assert!(state.can_next);
        assert!(!state.can_previous);
    }

    #[test]
    fn only_a_real_change_is_worth_announcing() {
        // The position moves on every tick. Announcing that as a metadata
        // change makes the desktop redraw its player thirty times a second,
        // and souvlaki's own D-Bus handler is documented as coping with one
        // event a second.
        let base = State {
            track: Some(a_track()),
            playing: true,
            position: std::time::Duration::from_secs(5),
            can_next: true,
            can_previous: true,
        };

        let moved_on = State {
            position: std::time::Duration::from_secs(6),
            ..base.clone()
        };
        assert!(
            !base.differs_from(&moved_on),
            "the position alone is not a change worth sending"
        );

        let paused = State {
            playing: false,
            ..base.clone()
        };
        assert!(base.differs_from(&paused), "play to pause is");

        let mut other = a_track();
        other.id = crate::domain::TrackId(2);
        let next_track = State {
            track: Some(other),
            ..base.clone()
        };
        assert!(
            base.differs_from(&next_track),
            "and so is a different track"
        );

        let same_track_retitled = State {
            track: Some(Track {
                title: "Renamed".into(),
                ..a_track()
            }),
            ..base.clone()
        };
        assert!(
            !base.differs_from(&same_track_retitled),
            "the same id is the same track, whatever the fields say"
        );

        let ran_out = State {
            can_next: false,
            ..base.clone()
        };
        assert!(
            base.differs_from(&ran_out),
            "the end of the queue is a change"
        );
    }
}
