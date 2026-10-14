//! The shell's side of playback: commands in, events out.
//!
//! The engine itself is sonora's (`music::engine`): its threads, its output
//! device, its queue ahead of the decoder. What lives here is the
//! translation between the shell's vocabulary and the engine's, and the
//! `Fetch` that teaches the engine TIDAL is in `fetch.rs`.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use crate::domain::TrackId;
use crate::playback::fetch::{Meta, TidalFetch};

/// What the engine's events leave out and the badge wants: the bit depth,
/// the rate and the tier of the stream that just started, and why a load
/// failed. TIDAL's fetch records them; another service has none to give.
pub trait StreamMeta: Send + Sync {
    fn take_meta(&self, _id: &str) -> Option<Meta> {
        None
    }
    fn take_failure(&self, _id: &str) -> Option<String> {
        None
    }
    /// What to ask for from now on. A service with one quality ignores it.
    fn set_quality(&self, _wanted: crate::domain::Quality) {}
}

/// A service whose streams say nothing about themselves.
pub struct NoMeta;

impl StreamMeta for NoMeta {}

impl StreamMeta for TidalFetch {
    fn set_quality(&self, wanted: crate::domain::Quality) {
        TidalFetch::set_quality(self, wanted);
    }
    fn take_meta(&self, id: &str) -> Option<Meta> {
        TidalFetch::take_meta(self, id)
    }
    fn take_failure(&self, id: &str) -> Option<String> {
        TidalFetch::take_failure(self, id)
    }
}

/// What an engine is started with, from the user's settings. The
/// equaliser is a shared handle: the engine keeps reading it, so a preset
/// chosen later reaches the output without a restart. The two flags are
/// read once, at the start, so changing them restarts the engine.
#[derive(Clone, Debug)]
pub struct Options {
    pub normalisation: bool,
    pub gapless: bool,
    pub equalizer: music::Equalizer,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            normalisation: false,
            gapless: true,
            equalizer: music::Equalizer::default(),
        }
    }
}

/// Not `Clone`: a command is consumed by the audio thread exactly once.
pub enum Cmd {
    /// Start an engine on this service's factory, replacing whatever was
    /// playing. Sent once a session is in hand; nothing plays before it.
    Attach {
        factory: Arc<dyn music::PlaybackFactory>,
        meta: Arc<dyn StreamMeta>,
        options: Options,
    },
    /// Fetch and play a track. What to ask for, and with which token, the
    /// service's fetch already knows.
    Play(TrackId),
    /// The same, from a position into it: what a restarted engine picks
    /// the track back up with.
    PlayAt(TrackId, Duration),
    /// Fetch the track that plays next, so it starts at once and a gapless
    /// engine lines it up behind the one playing.
    Preload(TrackId),
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
    /// The whole of this track has arrived, so the next can be fetched
    /// without taking bandwidth from it.
    Downloaded(TrackId),
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

/// Start the command thread. Commands in on a std channel (the shell
/// sends from wherever it is); events out on a tokio channel (the shell
/// awaits it). The engine itself starts on `Cmd::Attach`.
pub fn spawn() -> (
    Sender<Cmd>,
    tokio::sync::mpsc::UnboundedReceiver<PlaybackEvent>,
) {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Cmd>();
    let (evt_tx, evt_rx) = tokio::sync::mpsc::unbounded_channel::<PlaybackEvent>();
    std::thread::Builder::new()
        .name("ratidal-commands".into())
        .spawn(move || forward_commands(cmd_rx, evt_tx))
        .expect("spawning the command thread");
    (cmd_tx, evt_rx)
}

fn config(options: &Options) -> music::PlaybackConfig {
    music::PlaybackConfig {
        normalisation: options.normalisation,
        gapless: options.gapless,
        // ~4Hz progress updates, as before.
        position_interval: Duration::from_millis(250),
        gain: 1.0,
        equalizer: options.equalizer.clone(),
    }
}

/// The engine's `Player` is fire-and-forget, so each command is one call.
/// Dropping a player ends its engine, so an `Attach` replaces the last.
fn forward_commands(
    cmds: Receiver<Cmd>,
    events: tokio::sync::mpsc::UnboundedSender<PlaybackEvent>,
) {
    let mut player: Option<Box<dyn music::Player>> = None;
    // Kept across an attach: the level is set once at startup, before any
    // session, and a fresh engine starts at full.
    let mut volume = 1.0f32;
    // The track preloaded as the next in the queue. Playing it is a segue:
    // a gapless engine keeps what it lined up, so the join has no gap.
    let mut preloaded: Option<TrackId> = None;
    while let Ok(cmd) = cmds.recv() {
        match cmd {
            Cmd::Attach { factory, meta, options } => {
                preloaded = None;
                let (fresh, stream) = factory.start(config(&options));
                fresh.set_gain(volume);
                let out = events.clone();
                if let Err(error) = std::thread::Builder::new()
                    .name("ratidal-events".into())
                    .spawn(move || forward_events(stream, out, meta))
                {
                    let _ = events.send(PlaybackEvent::Error(format!(
                        "could not start the playback event thread: {error}"
                    )));
                }
                player = Some(fresh);
            }
            Cmd::Play(id) => match &player {
                Some(player) => {
                    let seamless = preloaded.take().is_some_and(|next| next == id);
                    if let Err(error) = player.load(id.as_str(), Duration::ZERO, seamless) {
                        tracing::error!("could not ask for track {id}: {error}");
                    }
                }
                None => {
                    let _ = events.send(PlaybackEvent::Failed {
                        id,
                        message: "no playback engine: sign in first".into(),
                    });
                }
            },
            Cmd::PlayAt(id, at) => {
                preloaded = None;
                if let Some(player) = &player {
                    if let Err(error) = player.load(id.as_str(), at, false) {
                        tracing::error!("could not ask for track {id}: {error}");
                    }
                }
            }
            Cmd::Preload(id) => {
                if let Some(player) = &player {
                    match player.preload(id.as_str(), true) {
                        Ok(()) => preloaded = Some(id),
                        Err(error) => tracing::warn!("could not preload track {id}: {error}"),
                    }
                }
            }
            Cmd::Pause => {
                if let Some(player) = &player {
                    player.pause();
                }
            }
            Cmd::Resume => {
                if let Some(player) = &player {
                    player.play();
                }
            }
            Cmd::Seek(to) => {
                if let Some(player) = &player {
                    player.seek(to);
                }
            }
            Cmd::Volume(v) => {
                volume = v.clamp(0.0, 1.0);
                if let Some(player) = &player {
                    player.set_gain(volume);
                }
            }
            // ponytail: the engine has no stop. Paused, the track holds
            // silent until the next load replaces it.
            Cmd::Stop => {
                if let Some(player) = &player {
                    player.pause();
                }
            }
        }
    }
}

/// Turns the engine's events into the shell's. The engine says a track is
/// playing at every start, resume and seek; the shell wants `Started` once.
fn forward_events(
    mut events: Box<dyn music::PlaybackEvents>,
    out: tokio::sync::mpsc::UnboundedSender<PlaybackEvent>,
    meta: Arc<dyn StreamMeta>,
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
            let Some(event) = translate(event, &mut announced, meta.as_ref()) else {
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
    meta: &dyn StreamMeta,
) -> Option<PlaybackEvent> {
    use music::PlaybackEvent as Engine;
    match event {
        Engine::Playing { id: Some(id), .. } if announced.as_deref() != Some(id.as_str()) => {
            // A service whose fetch says nothing about the stream still
            // started one: the badge is left blank rather than the track
            // left looking stopped.
            let found = meta.take_meta(&id).unwrap_or(Meta {
                bit_depth: None,
                delivered: crate::domain::Quality::Lossless,
                sample_rate: 0,
            });
            *announced = Some(id);
            Some(PlaybackEvent::Started {
                bit_depth: found.bit_depth,
                sample_rate: found.sample_rate,
                delivered: found.delivered,
            })
        }
        Engine::Position { at, .. } => Some(PlaybackEvent::Position(at)),
        Engine::Downloaded { id: Some(id) } => Some(PlaybackEvent::Downloaded(TrackId::from(id))),
        Engine::Ended { .. } => {
            *announced = None;
            Some(PlaybackEvent::Finished)
        }
        Engine::Unavailable { id: Some(id) } => {
            let message = meta
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
        | Engine::Downloaded { id: None }
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
    use crate::domain::Quality;

    fn fetch() -> TidalFetch {
        TidalFetch::new(crate::domain::Quality::Lossless).unwrap()
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
    fn a_stream_that_says_nothing_about_itself_is_still_announced() {
        // Another service's fetch records no meta. The track started all
        // the same, and a shell that never hears so draws it stopped.
        let mut announced = None;
        let event = translate(
            music::PlaybackEvent::Playing {
                id: Some("sub-1".into()),
                at: Duration::ZERO,
            },
            &mut announced,
            &NoMeta,
        );
        assert!(
            matches!(event, Some(PlaybackEvent::Started { sample_rate: 0, .. })),
            "{event:?}"
        );
        assert_eq!(announced.as_deref(), Some("sub-1"));
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
