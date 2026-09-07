//! MPRIS, the D-Bus interface every Linux desktop uses to find media players.
//!
//! Registering here is what makes the keyboard's play/pause and next keys
//! reach this app, and what puts the track in the desktop's own player
//! widget. The commands become the same [`Action`] the keyboard sends, so
//! there is one path through the app however a command arrives.

use mpris_server::zbus::fdo;
use mpris_server::{
    LoopStatus, Metadata, PlaybackRate, PlaybackStatus, PlayerInterface, Property, RootInterface,
    Server, Time, TrackId, Volume,
};

use super::mediakeys::{State, StateReceiver};
use super::Action;

/// The name this appears under on D-Bus. The prefix is fixed by the spec.
const BUS_NAME: &str = "ratidal";

struct Player {
    actions: tokio::sync::mpsc::UnboundedSender<Action>,
    state: StateReceiver,
}

impl Player {
    fn now(&self) -> State {
        self.state.borrow().clone()
    }

    /// Hand a command to the app the way a keystroke would.
    ///
    /// A closed channel means the app is shutting down, which is not worth
    /// reporting to D-Bus as a failure.
    fn send(&self, action: Action) -> fdo::Result<()> {
        let _ = self.actions.send(action);
        Ok(())
    }
}

impl RootInterface for Player {
    async fn identity(&self) -> fdo::Result<String> {
        Ok("ratidal".into())
    }
    async fn desktop_entry(&self) -> fdo::Result<String> {
        Ok(String::new())
    }
    async fn supported_uri_schemes(&self) -> fdo::Result<Vec<String>> {
        Ok(Vec::new())
    }
    async fn supported_mime_types(&self) -> fdo::Result<Vec<String>> {
        Ok(Vec::new())
    }
    /// No window to raise: this is a terminal application.
    async fn can_raise(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn raise(&self) -> fdo::Result<()> {
        Ok(())
    }
    /// The desktop must not be able to close the app: `q` does that, and a
    /// stray quit from a player widget would drop the user's session.
    async fn can_quit(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn quit(&self) -> fdo::Result<()> {
        Ok(())
    }
    async fn fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn set_fullscreen(&self, _: bool) -> mpris_server::zbus::Result<()> {
        Ok(())
    }
    async fn can_set_fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn has_track_list(&self) -> fdo::Result<bool> {
        Ok(false)
    }
}

impl PlayerInterface for Player {
    async fn next(&self) -> fdo::Result<()> {
        self.send(Action::QueueNext)
    }
    async fn previous(&self) -> fdo::Result<()> {
        self.send(Action::QueuePrevious)
    }
    async fn play_pause(&self) -> fdo::Result<()> {
        self.send(Action::TogglePause)
    }

    /// Play and pause are one key on a keyboard, but the desktop sends them
    /// separately -- a widget's play button when already playing must not
    /// pause. Both are toggles here, guarded by the state so they only fire
    /// when they would change something.
    async fn play(&self) -> fdo::Result<()> {
        if self.now().playing {
            return Ok(());
        }
        self.send(Action::TogglePause)
    }
    async fn pause(&self) -> fdo::Result<()> {
        if !self.now().playing {
            return Ok(());
        }
        self.send(Action::TogglePause)
    }
    async fn stop(&self) -> fdo::Result<()> {
        // Nothing in the app stops outright; pausing is the nearest thing
        // that leaves the user where they were.
        self.pause().await
    }

    async fn seek(&self, _: Time) -> fdo::Result<()> {
        Ok(())
    }
    async fn set_position(&self, _: TrackId, _: Time) -> fdo::Result<()> {
        Ok(())
    }
    async fn open_uri(&self, _: String) -> fdo::Result<()> {
        Ok(())
    }

    async fn playback_status(&self) -> fdo::Result<PlaybackStatus> {
        let state = self.now();
        Ok(match (state.track.is_some(), state.playing) {
            (false, _) => PlaybackStatus::Stopped,
            (true, true) => PlaybackStatus::Playing,
            (true, false) => PlaybackStatus::Paused,
        })
    }

    async fn metadata(&self) -> fdo::Result<Metadata> {
        let state = self.now();
        let Some(track) = state.track else {
            return Ok(Metadata::new());
        };
        let mut metadata = Metadata::new();
        metadata.set_title(Some(track.title));
        metadata.set_artist(Some(vec![track.artist]));
        metadata.set_album(Some(track.album));
        metadata.set_length(Time::from_micros(track.duration.as_micros() as i64).into());
        if let Some(cover) = track.cover {
            metadata.set_art_url(Some(cover));
        }
        // The spec wants an object path; the id is what makes it unique.
        if let Ok(id) = TrackId::try_from(format!("/dev/ratidal/track/{}", track.id.0)) {
            metadata.set_trackid(Some(id));
        }
        Ok(metadata)
    }

    async fn can_go_next(&self) -> fdo::Result<bool> {
        Ok(self.now().can_next)
    }
    async fn can_go_previous(&self) -> fdo::Result<bool> {
        Ok(self.now().can_previous)
    }
    async fn can_play(&self) -> fdo::Result<bool> {
        Ok(self.now().track.is_some())
    }
    async fn can_pause(&self) -> fdo::Result<bool> {
        Ok(self.now().track.is_some())
    }
    async fn can_seek(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn can_control(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn position(&self) -> fdo::Result<Time> {
        Ok(Time::from_micros(self.now().position.as_micros() as i64))
    }
    async fn rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }
    async fn set_rate(&self, _: PlaybackRate) -> mpris_server::zbus::Result<()> {
        Ok(())
    }
    async fn minimum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }
    async fn maximum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }
    async fn volume(&self) -> fdo::Result<Volume> {
        Ok(1.0)
    }
    async fn set_volume(&self, _: Volume) -> mpris_server::zbus::Result<()> {
        Ok(())
    }
    async fn shuffle(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn set_shuffle(&self, _: bool) -> mpris_server::zbus::Result<()> {
        Ok(())
    }
    async fn loop_status(&self) -> fdo::Result<LoopStatus> {
        Ok(LoopStatus::None)
    }
    async fn set_loop_status(&self, _: LoopStatus) -> mpris_server::zbus::Result<()> {
        Ok(())
    }
}

/// Register with the desktop, and keep it told what is playing.
///
/// Failure is not fatal: a machine with no session bus -- a container, a
/// headless box -- simply has no media keys, and the app is still usable.
pub fn spawn(
    actions: tokio::sync::mpsc::UnboundedSender<Action>,
    state: StateReceiver,
) {
    tokio::spawn(async move {
        let mut watch = state.clone();
        let server = match Server::new(BUS_NAME, Player { actions, state }).await {
            Ok(server) => server,
            Err(e) => {
                tracing::info!("no media-key integration: {e}");
                return;
            }
        };
        tracing::info!("registered with the desktop as {BUS_NAME}");

        let mut last = watch.borrow().clone();
        while watch.changed().await.is_ok() {
            let now = watch.borrow().clone();
            if !last.differs_from(&now) {
                continue;
            }
            last = now.clone();
            // Every property carries its value on this bus, so they are
            // built from the state that just arrived rather than read back.
            let player = server.imp();
            let Ok(status) = player.playback_status().await else { continue };
            let Ok(metadata) = player.metadata().await else { continue };
            let _ = server
                .properties_changed([
                    Property::PlaybackStatus(status),
                    Property::Metadata(metadata),
                    Property::CanGoNext(now.can_next),
                    Property::CanGoPrevious(now.can_previous),
                    Property::CanPlay(now.track.is_some()),
                    Property::CanPause(now.track.is_some()),
                ])
                .await;
        }
    });
}
