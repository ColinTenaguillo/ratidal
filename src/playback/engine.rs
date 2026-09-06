use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::playback::{Manifest, SegmentReader};

/// Not `Clone`: a command is consumed by the audio thread exactly once.
#[derive(Debug)]
pub enum Cmd {
    Play(Manifest),
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
    Started { bit_depth: Option<u8>, sample_rate: u32 },
    Position(Duration),
    Finished,
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

/// Spawn the audio thread. Commands in on a std channel (the thread blocks on
/// it); events out on a tokio channel (the UI awaits it).
pub fn spawn() -> (
    Sender<Cmd>,
    tokio::sync::mpsc::UnboundedReceiver<PlaybackEvent>,
) {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Cmd>();
    let (evt_tx, evt_rx) = tokio::sync::mpsc::unbounded_channel::<PlaybackEvent>();

    std::thread::Builder::new()
        .name("ratidal-audio".into())
        .spawn(move || run(cmd_rx, evt_tx))
        .expect("spawning the audio thread");

    (cmd_tx, evt_rx)
}

fn run(cmds: Receiver<Cmd>, events: tokio::sync::mpsc::UnboundedSender<PlaybackEvent>) {
    // Held for the whole thread: dropping this stops all audio.
    let mut sink = match rodio::DeviceSinkBuilder::open_default_sink() {
        Ok(s) => s,
        Err(e) => {
            let _ = events.send(PlaybackEvent::Error(format!("no audio device: {e}")));
            return;
        }
    };
    // Takes &mut self and returns (), so it cannot be chained onto the
    // constructor. Silences the "Dropping DeviceSink" warning on shutdown.
    sink.log_on_drop(false);
    let player = rodio::Player::connect_new(sink.mixer());

    let http = reqwest::blocking::Client::new();
    // Kept so a Seek can rebuild the stream from a segment boundary.
    let mut current: Option<Manifest> = None;
    let mut reported_finished = true;

    loop {
        // Poll for commands, but wake regularly to report position and
        // end-of-track. 250ms gives ~4Hz progress updates.
        let cmd = cmds.recv_timeout(Duration::from_millis(250));

        match cmd {
            Ok(Cmd::Play(manifest)) => {
                match start_stream(&player, &http, &manifest, Duration::ZERO) {
                    Ok(info) => {
                        current = Some(manifest);
                        reported_finished = false;
                        let _ = events.send(info);
                    }
                    Err(e) => {
                        let _ = events.send(PlaybackEvent::Error(e));
                    }
                }
            }
            Ok(Cmd::Pause) => player.pause(),
            Ok(Cmd::Resume) => player.play(),
            Ok(Cmd::Volume(v)) => player.set_volume(v.clamp(0.0, 1.0)),
            Ok(Cmd::Seek(to)) => {
                // rodio can seek within the decoder it already holds; that is
                // cheaper and more accurate than rebuilding the stream.
                if player.try_seek(to).is_err() {
                    // Fall back to restarting from the containing segment.
                    if let Some(manifest) = &current {
                        if let Err(e) = start_stream(&player, &http, manifest, to) {
                            let _ = events.send(PlaybackEvent::Error(e));
                        } else if let Manifest::Dash { segment_durations, .. } = manifest {
                            // The stream restarts at a segment boundary, which
                            // is earlier than the requested instant. Report
                            // where playback actually resumed so the progress
                            // bar does not lie.
                            let index = segment_for(segment_durations, to);
                            let resumed = segment_start(segment_durations, index);
                            let _ = events.send(PlaybackEvent::Position(resumed));
                        }
                    }
                }
            }
            Ok(Cmd::Stop) => {
                player.pause();
                player.clear();
                current = None;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }

        if current.is_some() {
            let _ = events.send(PlaybackEvent::Position(player.get_pos()));
            if player.empty() && !reported_finished {
                reported_finished = true;
                current = None;
                let _ = events.send(PlaybackEvent::Finished);
            }
        }
    }
}

/// Build a decoder for `manifest` and hand it to the player. `from` restarts
/// at the segment boundary containing that offset.
fn start_stream(
    player: &rodio::Player,
    http: &reqwest::blocking::Client,
    manifest: &Manifest,
    from: Duration,
) -> Result<PlaybackEvent, String> {
    player.clear();

    let reader = match manifest {
        // BTS is a single file with no segment timeline, so `from` cannot be
        // honoured here: a seek that falls back to rebuilding the stream
        // restarts a BTS track from 0. In practice rodio's own try_seek
        // handles the common case and this path is only the fallback.
        Manifest::Bts { url } => {
            SegmentReader::new(http.clone(), url.clone(), Vec::new())
        }
        Manifest::Dash { init, segments, segment_durations } => {
            let start = segment_for(segment_durations, from);
            SegmentReader::new(
                http.clone(),
                init.clone(),
                segments[start..].to_vec(),
            )
        }
    };

    let decoder = rodio::Decoder::new(reader)
        .map_err(|e| format!("could not decode the stream: {e}"))?;

    use rodio::Source as _;
    // rodio's SampleRate is NonZero<u32>; PlaybackEvent carries a plain u32.
    let sample_rate = decoder.sample_rate().get();

    player.append(decoder);
    player.play();

    Ok(PlaybackEvent::Started { bit_depth: None, sample_rate })
}

#[cfg(test)]
mod tests {
    use super::*;

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
