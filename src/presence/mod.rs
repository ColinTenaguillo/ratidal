//! Discord Rich Presence: what is playing, on the user's Discord profile.
//!
//! Off unless the config names a Discord application, since the name shown
//! beside the track is the application's and only the user can register
//! one. The socket blocks on every round trip, so it lives on a thread of
//! its own, fed over a channel; a Discord that is not running is tried
//! again later rather than reported on every track.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

/// What to show, or that there is nothing to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presence {
    Playing {
        title: String,
        artist: String,
        album: String,
        cover: Option<String>,
        /// How far into the track the music is, so the profile's clock
        /// counts from the right place.
        position: Duration,
        duration: Duration,
    },
    Paused {
        title: String,
        artist: String,
    },
    Off,
}

/// How long to leave Discord alone after it could not be reached.
const RETRY: Duration = Duration::from_secs(15);

/// Starts the presence thread for `application_id`. Dropping the sender
/// ends it, clearing what was shown.
pub fn spawn(application_id: String) -> Sender<Presence> {
    let (sender, receiver) = channel();
    if let Err(error) = std::thread::Builder::new()
        .name("sonora-tui-presence".into())
        .spawn(move || run(application_id, receiver))
    {
        tracing::warn!("discord: cannot start the presence thread: {error}");
    }
    sender
}

fn run(application_id: String, receiver: Receiver<Presence>) {
    let mut client = DiscordIpcClient::new(&application_id);
    let mut connected = false;
    let mut failed_at: Option<Instant> = None;
    while let Ok(wanted) = receiver.recv() {
        // Only the latest matters: a burst of changes is one update.
        let wanted = receiver.try_iter().last().unwrap_or(wanted);
        if !connected {
            if failed_at.is_some_and(|at| at.elapsed() < RETRY) {
                continue;
            }
            match client.connect() {
                Ok(()) => {
                    connected = true;
                    failed_at = None;
                    tracing::info!("discord: connected");
                }
                Err(error) => {
                    tracing::info!("discord: not reachable ({error}); trying again later");
                    failed_at = Some(Instant::now());
                    continue;
                }
            }
        }
        let mut scratch = String::new();
        let sent = match &wanted {
            Presence::Off => client.clear_activity(),
            shown => client.set_activity(activity_of(shown, &mut scratch)),
        };
        if let Err(error) = sent {
            tracing::info!("discord: lost ({error}); trying again later");
            connected = false;
            failed_at = Some(Instant::now());
        }
    }
    if connected {
        let _ = client.clear_activity();
        let _ = client.close();
    }
}

/// Seconds since the epoch, as Discord's clock reads them.
fn epoch(at: SystemTime) -> i64 {
    at.duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

/// The activity for what is shown. Listening, titled by the track, with
/// the artist under it and the album on the artwork; a clock that counts
/// down the track while it plays, and nothing to count while it is paused.
/// `scratch` holds the one line built here, for as long as the activity
/// borrows it.
fn activity_of<'a>(shown: &'a Presence, scratch: &'a mut String) -> activity::Activity<'a> {
    match shown {
        Presence::Playing {
            title,
            artist,
            album,
            cover,
            position,
            duration,
        } => {
            let started = epoch(SystemTime::now() - *position);
            let ends = started + i64::try_from(duration.as_secs()).unwrap_or(0);
            let mut assets = activity::Assets::new().large_text(album.as_str());
            if let Some(cover) = cover {
                assets = assets.large_image(cover.as_str());
            }
            activity::Activity::new()
                .activity_type(activity::ActivityType::Listening)
                .details(title.as_str())
                .state(artist.as_str())
                .assets(assets)
                .timestamps(activity::Timestamps::new().start(started).end(ends))
        }
        Presence::Paused { title, artist } => {
            *scratch = format!("{artist} (paused)");
            activity::Activity::new()
                .activity_type(activity::ActivityType::Listening)
                .details(title.as_str())
                .state(scratch.as_str())
        }
        Presence::Off => activity::Activity::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_starts_where_the_track_is_and_ends_where_it_will() {
        let before = epoch(SystemTime::now());
        let shown = Presence::Playing {
            title: "Song".into(),
            artist: "Someone".into(),
            album: "Album".into(),
            cover: None,
            position: Duration::from_secs(30),
            duration: Duration::from_secs(200),
        };
        // Built without a Discord: the activity is data until it is sent.
        let mut scratch = String::new();
        let _ = activity_of(&shown, &mut scratch);
        let paused = Presence::Paused {
            title: "Song".into(),
            artist: "Someone".into(),
        };
        let _ = activity_of(&paused, &mut scratch);
        assert_eq!(scratch, "Someone (paused)");
        let started = epoch(SystemTime::now() - Duration::from_secs(30));
        assert!(started <= before - 29 && started >= before - 31, "{started} vs {before}");
    }
}
