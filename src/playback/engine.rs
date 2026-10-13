//! The shell's side of playback: commands in, events out.
//!
//! The engine itself is sonora's (`music::engine`): its threads, its output
//! device, its queue ahead of the decoder. What lives here is the
//! translation between the shell's vocabulary and the engine's, and the
//! `Fetch` that teaches the engine TIDAL is in `fetch.rs`.

use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::auth::StoredToken;
use crate::domain::{Quality, TrackId};
use crate::playback::fetch::{Session, TidalFetch};

/// Not `Clone`: a command is consumed by the audio thread exactly once.
#[derive(Debug)]
pub enum Cmd {
    /// Fetch and play a track. The token and the quality travel with it so
    /// a refreshed session or a changed setting reaches the next request.
    Play {
        id: TrackId,
        token: StoredToken,
        /// What the user asked for. The response says what was delivered,
        /// and the badge reads that.
        wanted: Quality,
    },
    Pause,
    Resume,
    Seek(Duration),
    Volume(f32),
    Stop,
}

#[derive(Debug, Clone)]
pub enum PlaybackEvent {
    /// Emitted once a stream is decoding, carrying what was actually
    /// delivered — never the requested quality.
    Started {
        bit_depth: Option<u8>,
        sample_rate: u32,
        delivered: crate::domain::Quality,
    },
    Position(Duration),
    Finished,
    /// A track that could not be fetched or opened. The shell takes back
    /// the bar it filled when the track was chosen.
    Failed {
        id: TrackId,
        message: String,
    },
    /// TIDAL rejected the session: the stored token is no good any more.
    SessionExpired,
    Error(String),
}

/// Index of the segment containing `at`, clamped to the last segment.
pub(crate) fn segment_for(durations: &[Duration], at: Duration) -> usize {
    let mut acc = Duration::ZERO;
    for (i, d) in durations.iter().enumerate() {
        acc += *d;
        if at < acc {
            return i;
        }
    }
    durations.len().saturating_sub(1)
}

/// Playback offset at which segment `index` begins.
pub(crate) fn segment_start(durations: &[Duration], index: usize) -> Duration {
    durations.iter().take(index).copied().sum()
}

/// Where a seek into a DASH stream restarts: the segment holding `at`,
/// and how much of it to skip to land on `at` itself.
pub(crate) fn restart_point(durations: &[Duration], at: Duration) -> (usize, Duration) {
    let index = segment_for(durations, at);
    (index, at.saturating_sub(segment_start(durations, index)))
}

/// Start the engine. Commands in on a std channel (the shell sends from
/// wherever it is); events out on a tokio channel (the shell awaits it).
pub fn spawn() -> (
    Sender<Cmd>,
    tokio::sync::mpsc::UnboundedReceiver<PlaybackEvent>,
) {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Cmd>();
    let (evt_tx, evt_rx) = tokio::sync::mpsc::unbounded_channel::<PlaybackEvent>();

    let fetch = match TidalFetch::new() {
        Ok(fetch) => fetch,
        Err(error) => {
            // Falling back to a client without timeouts would defeat the
            // point, so report and stop instead.
            let _ = evt_tx.send(PlaybackEvent::Error(format!(
                "could not build the audio HTTP client: {error}"
            )));
            return (cmd_tx, evt_rx);
        }
    };
    let config = music::PlaybackConfig {
        normalisation: false,
        // ponytail: off until the queue preloads the next track; with
        // nothing lined up there is nothing to join.
        gapless: false,
        // ~4Hz progress updates, as before.
        position_interval: Duration::from_millis(250),
        gain: 1.0,
        equalizer: music::Equalizer::default(),
    };
    let (player, events) = music::engine::start(fetch.clone(), config);

    let commanded = fetch.clone();
    std::thread::Builder::new()
        .name("ratidal-commands".into())
        .spawn(move || forward_commands(cmd_rx, player, commanded))
        .expect("spawning the command thread");
    std::thread::Builder::new()
        .name("ratidal-events".into())
        .spawn(move || forward_events(events, evt_tx, fetch))
        .expect("spawning the event thread");

    (cmd_tx, evt_rx)
}

/// The engine's `Player` is fire-and-forget, so each command is one call.
fn forward_commands(cmds: Receiver<Cmd>, player: Box<dyn music::Player>, fetch: TidalFetch) {
    while let Ok(cmd) = cmds.recv() {
        match cmd {
            Cmd::Play { id, token, wanted } => {
                fetch.set_session(Session { token, wanted });
                if let Err(error) = player.load(&id.0.to_string(), Duration::ZERO, false) {
                    tracing::error!("could not ask for track {id}: {error}");
                }
            }
            Cmd::Pause => player.pause(),
            Cmd::Resume => player.play(),
            Cmd::Seek(to) => player.seek(to),
            Cmd::Volume(v) => player.set_gain(v.clamp(0.0, 1.0)),
            // ponytail: the engine has no stop. Paused, the track holds
            // silent until the next load replaces it.
            Cmd::Stop => player.pause(),
        }
    }
}

/// Turns the engine's events into the shell's. The engine says a track is
/// playing at every start, resume and seek; the shell wants `Started` once.
fn forward_events(
    mut events: Box<dyn music::PlaybackEvents>,
    out: tokio::sync::mpsc::UnboundedSender<PlaybackEvent>,
    fetch: TidalFetch,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = out.send(PlaybackEvent::Error(format!(
                "could not build the event runtime: {error}"
            )));
            return;
        }
    };
    // The track whose `Started` has gone out, so a resume or a seek on it
    // does not reset the badge and the position.
    let mut announced: Option<String> = None;
    runtime.block_on(async {
        while let Some(event) = events.next().await {
            let Some(event) = translate(event, &mut announced, &fetch) else {
                continue;
            };
            if out.send(event).is_err() {
                return;
            }
        }
    });
}

fn translate(
    event: music::PlaybackEvent,
    announced: &mut Option<String>,
    fetch: &TidalFetch,
) -> Option<PlaybackEvent> {
    use music::PlaybackEvent as Engine;
    match event {
        Engine::Playing { id: Some(id), .. } if announced.as_deref() != Some(id.as_str()) => {
            let meta = fetch.take_meta(&id)?;
            *announced = Some(id);
            Some(PlaybackEvent::Started {
                bit_depth: meta.bit_depth,
                sample_rate: meta.sample_rate,
                delivered: meta.delivered,
            })
        }
        Engine::Position { at, .. } => Some(PlaybackEvent::Position(at)),
        Engine::Ended { .. } => {
            *announced = None;
            Some(PlaybackEvent::Finished)
        }
        Engine::Unavailable { id: Some(id) } => {
            let message = fetch
                .take_failure(&id)
                .unwrap_or_else(|| "track not available for playback".into());
            Some(failed(&id, message))
        }
        Engine::Throttled { id: Some(id) } => Some(failed(
            &id,
            "TIDAL is rate limiting playback; try again shortly".into(),
        )),
        Engine::Gated => Some(PlaybackEvent::SessionExpired),
        Engine::Refused => Some(PlaybackEvent::Error(
            "the audio output refused the stream".into(),
        )),
        // The engine reopens the output itself; a resume, a seek, a
        // pause and a length are things the shell already knows.
        Engine::Playing { .. }
        | Engine::Loading { .. }
        | Engine::Paused { .. }
        | Engine::Seeked { .. }
        | Engine::Length { .. }
        | Engine::Downloaded { .. }
        | Engine::Unavailable { id: None }
        | Engine::Throttled { id: None }
        | Engine::OutputChanged => None,
    }
}

fn failed(id: &str, message: String) -> PlaybackEvent {
    PlaybackEvent::Failed {
        id: TrackId::from(id),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetch() -> TidalFetch {
        TidalFetch::new().unwrap()
    }

    #[test]
    fn a_track_is_announced_once_however_often_the_engine_says_playing() {
        // `Started` resets the position and the badge. The engine says
        // `Playing` on every resume and after every seek, and a resume
        // that drew the bar back to zero would be a bug the user sees.
        let fetch = fetch();
        let mut announced = None;
        let playing = |at| music::PlaybackEvent::Playing {
            id: Some("1".into()),
            at,
        };
        // Nothing opened yet: no meta, so nothing to announce.
        assert!(translate(playing(Duration::ZERO), &mut announced, &fetch).is_none());
        assert_eq!(announced, None, "an unannounced track stays unannounced");

        // Stand in for `open`, which is what records the meta.
        fetch.record_meta(
            "1",
            crate::playback::fetch::Meta {
                bit_depth: Some(24),
                delivered: Quality::HiResLossless,
                sample_rate: 96_000,
            },
        );
        match translate(playing(Duration::ZERO), &mut announced, &fetch) {
            Some(PlaybackEvent::Started {
                bit_depth,
                sample_rate,
                delivered,
            }) => {
                assert_eq!((bit_depth, sample_rate, delivered), (Some(24), 96_000, Quality::HiResLossless));
            }
            other => panic!("expected Started, got {other:?}"),
        }
        assert_eq!(announced.as_deref(), Some("1"));

        // A resume, or the engine confirming a seek: same track, no badge reset.
        assert!(translate(playing(Duration::from_secs(30)), &mut announced, &fetch).is_none());
        assert!(translate(playing(Duration::ZERO), &mut announced, &fetch).is_none());
    }

    #[test]
    fn a_failed_load_names_the_track_and_the_reason() {
        let fetch = fetch();
        let mut announced = None;
        let event = translate(
            music::PlaybackEvent::Unavailable {
                id: Some("655911".into()),
            },
            &mut announced,
            &fetch,
        );
        match event {
            Some(PlaybackEvent::Failed { id, message }) => {
                assert_eq!(id, TrackId::from(655911));
                assert_eq!(message, "track not available for playback");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn a_gated_load_expires_the_session() {
        let fetch = fetch();
        let mut announced = Some("1".to_owned());
        assert!(matches!(
            translate(music::PlaybackEvent::Gated, &mut announced, &fetch),
            Some(PlaybackEvent::SessionExpired)
        ));
    }

    #[test]
    fn the_end_of_a_track_clears_the_announcement() {
        // So the same track played twice in a row is announced twice.
        let fetch = fetch();
        let mut announced = Some("1".to_owned());
        assert!(matches!(
            translate(
                music::PlaybackEvent::Ended {
                    id: Some("1".into())
                },
                &mut announced,
                &fetch
            ),
            Some(PlaybackEvent::Finished)
        ));
        assert_eq!(announced, None);
    }

    #[test]
    fn what_the_shell_already_knows_is_not_repeated() {
        let fetch = fetch();
        let mut announced = Some("1".to_owned());
        for event in [
            music::PlaybackEvent::Paused {
                id: Some("1".into()),
                at: Duration::ZERO,
            },
            music::PlaybackEvent::Seeked {
                id: Some("1".into()),
                at: Duration::from_secs(3),
            },
            music::PlaybackEvent::Playing {
                id: Some("1".into()),
                at: Duration::from_secs(3),
            },
            music::PlaybackEvent::OutputChanged,
        ] {
            assert!(
                translate(event.clone(), &mut announced, &fetch).is_none(),
                "{event:?} reached the shell"
            );
        }
    }

    #[test]
    fn segment_index_for_a_timestamp_is_found_from_durations() {
        // 4s segments: t=0 -> 0, t=5 -> 1, t=9 -> 2.
        let durs = vec![
            Duration::from_secs(4),
            Duration::from_secs(4),
            Duration::from_secs(4),
        ];
        assert_eq!(segment_for(&durs, Duration::from_secs(0)), 0);
        assert_eq!(segment_for(&durs, Duration::from_secs(5)), 1);
        assert_eq!(segment_for(&durs, Duration::from_secs(9)), 2);

        // The edges themselves. A segment covers [start, start + d): four
        // seconds in is the first instant of the second segment, not the
        // last of the first. Seeking to a segment boundary is what the
        // player does every time it crosses one, so the off-by-one here
        // would land a seek a whole segment early.
        assert_eq!(
            segment_for(&durs, Duration::from_secs(4)),
            1,
            "4s starts segment 1"
        );
        assert_eq!(
            segment_for(&durs, Duration::from_secs(8)),
            2,
            "8s starts segment 2"
        );
    }

    #[test]
    fn a_timestamp_past_the_end_clamps_to_the_last_segment() {
        let durs = vec![Duration::from_secs(4), Duration::from_secs(4)];
        assert_eq!(segment_for(&durs, Duration::from_secs(999)), 1);
    }

    #[test]
    fn an_empty_timeline_yields_the_first_segment() {
        assert_eq!(segment_for(&[], Duration::from_secs(5)), 0);
    }

    #[test]
    fn a_seek_restarts_at_its_segment_and_skips_the_rest_of_it() {
        let durs = vec![Duration::from_secs(4); 3];
        assert_eq!(restart_point(&durs, Duration::ZERO), (0, Duration::ZERO));
        assert_eq!(
            restart_point(&durs, Duration::from_millis(5500)),
            (1, Duration::from_millis(1500)),
            "the second segment, a second and a half in"
        );
        assert_eq!(
            restart_point(&durs, Duration::from_secs(8)),
            (2, Duration::ZERO),
            "a boundary is the start of the segment after it"
        );
        assert_eq!(
            restart_point(&durs, Duration::from_secs(40)),
            (2, Duration::from_secs(32)),
            "past the end: the last segment, and rodio skips to its end"
        );
    }

    #[test]
    fn segment_start_offset_accumulates_preceding_durations() {
        let durs = vec![
            Duration::from_secs(4),
            Duration::from_secs(4),
            Duration::from_secs(4),
        ];
        assert_eq!(segment_start(&durs, 0), Duration::ZERO);
        assert_eq!(segment_start(&durs, 2), Duration::from_secs(8));
    }
}
