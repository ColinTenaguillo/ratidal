use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use std::num::NonZero;

use crate::playback::{Manifest, SegmentReader};

/// Not `Clone`: a command is consumed by the audio thread exactly once.
#[derive(Debug)]
pub enum Cmd {
    /// `bit_depth` and `delivered` both come from the playback-info
    /// response, not the decoder: rodio exposes neither, so without
    /// threading them through here the quality badge could only ever show a
    /// sample rate.
    Play {
        manifest: Manifest,
        bit_depth: Option<u8>,
        /// What TIDAL called the stream. The authority for the badge: a
        /// LOSSLESS stream comes back with no bit depth at all, so depth
        /// alone cannot tell lossless from lossy.
        delivered: crate::domain::Quality,
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

/// Open the output device, at `rate` when one is asked for.
///
/// rodio otherwise opens at the device's *default* configuration and
/// resamples anything that does not match — silently, and on a hi-res
/// stream that is the whole point of asking for hi-res. qobine does the
/// same thing for the same reason.
fn open_sink(rate: Option<NonZero<u32>>) -> Result<rodio::MixerDeviceSink, String> {
    let build = rodio::DeviceSinkBuilder::from_default_device()
        .map_err(|e| format!("no audio device: {e}"))?;
    let build = match rate {
        Some(r) => build.with_sample_rate(r),
        None => build,
    };
    // `open_sink_or_fallback` tries the device's other configurations when
    // the asked-for one is refused, so an unusual rate degrades to a
    // resampled stream rather than to no sound at all.
    let mut sink = build
        .open_sink_or_fallback()
        .map_err(|e| format!("could not open the audio device: {e}"))?;
    // Takes &mut self and returns (), so it cannot be chained onto the
    // constructor. Silences the "Dropping DeviceSink" warning on shutdown.
    sink.log_on_drop(false);
    Ok(sink)
}

fn run(cmds: Receiver<Cmd>, events: tokio::sync::mpsc::UnboundedSender<PlaybackEvent>) {
    // Held for the whole thread: dropping this stops all audio.
    let mut sink = match open_sink(None) {
        Ok(s) => s,
        Err(e) => {
            let _ = events.send(PlaybackEvent::Error(format!("no audio device: {e}")));
            return;
        }
    };
    let mut player = rodio::Player::connect_new(sink.mixer());

    // A timeout is not optional here. Segment fetches happen synchronously on
    // this thread, so a stalled CDN response blocks the command loop: Pause,
    // Stop and the next Play all stop being drained, and no position event is
    // sent. The transport silently goes dead for as long as the socket hangs.
    let http = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            // Falling back to a client without timeouts would defeat the
            // point, so report and stop instead.
            let _ = events.send(PlaybackEvent::Error(format!(
                "could not build the audio HTTP client: {e}"
            )));
            return;
        }
    };
    // Kept so a Seek can rebuild the stream from a segment boundary.
    let mut current: Option<Manifest> = None;
    // Kept so a rebuilt stream (the Seek fallback) can re-report the same
    // quality rather than dropping the bit depth from the badge.
    let mut current_bit_depth: Option<u8> = None;
    let mut current_quality = crate::domain::Quality::Low;
    let mut reported_finished = true;

    loop {
        // Poll for commands, but wake regularly to report position and
        // end-of-track. 250ms gives ~4Hz progress updates.
        let cmd = cmds.recv_timeout(Duration::from_millis(250));

        match cmd {
            Ok(Cmd::Play { manifest, bit_depth, delivered }) => {
                match start_stream(&mut sink, &mut player, &http, &manifest, Duration::ZERO) {
                    Ok(info) => {
                        current = Some(manifest);
                        current_bit_depth = bit_depth;
                        current_quality = delivered;
                        reported_finished = false;
                        // start_stream reads the sample rate off the decoder;
                        // the bit depth only exists in the manifest response.
                        let info = match info {
                            PlaybackEvent::Started { sample_rate, .. } => {
                                PlaybackEvent::Started { bit_depth, sample_rate, delivered }
                            }
                            other => other,
                        };
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
                        let restarted =
                            start_stream(&mut sink, &mut player, &http, manifest, to);
                        if let Ok(PlaybackEvent::Started { sample_rate, .. }) = restarted {
                            // Re-report the STORED bit depth: rebuilding the
                            // stream re-reads the decoder, which does not know
                            // it, so otherwise the badge would blank mid-track.
                            let _ = events.send(PlaybackEvent::Started {
                                bit_depth: current_bit_depth,
                                sample_rate,
                                delivered: current_quality,
                            });
                        }
                        if let Err(e) = restarted {
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
                // Cleared with the manifest so no later path can report a
                // previous track's depth.
                current_bit_depth = None;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }

        if current.is_some() {
            let _ = events.send(PlaybackEvent::Position(player.get_pos()));
            if player.empty() && !reported_finished {
                reported_finished = true;
                current = None;
                current_bit_depth = None;
                let _ = events.send(PlaybackEvent::Finished);
            }
        }
    }
}

/// Build a decoder for `manifest` and hand it to the player. `from` restarts
/// at the segment boundary containing that offset.
/// Build a decoder and hand it to a player at the stream's own rate.
///
/// The sink and the player are rebuilt when the stream needs a rate the
/// device is not already open at: rodio resamples anything that does not
/// match its configuration, which on a hi-res stream discards the reason
/// for asking for hi-res. A decoder has to exist before its rate is known,
/// so the order is decode, then reopen, then append.
fn start_stream(
    sink: &mut rodio::MixerDeviceSink,
    player: &mut rodio::Player,
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
    let rate = decoder.sample_rate();
    // rodio's SampleRate is NonZero<u32>; PlaybackEvent carries a plain u32.
    let sample_rate = rate.get();

    // Reopen only on a change: tearing the device down between every track
    // costs a gap, and most of a library is one rate.
    if sink.config().sample_rate() != rate {
        tracing::info!(
            "reopening the output at {} Hz (was {})",
            rate.get(),
            sink.config().sample_rate().get()
        );
        match open_sink(Some(rate)) {
            Ok(fresh) => {
                // Carried across: a fresh player starts at full volume, so
                // without this the setting was undone by the first track at
                // a different rate — silently, and only sometimes.
                let volume = player.volume();
                *sink = fresh;
                *player = rodio::Player::connect_new(sink.mixer());
                player.set_volume(volume);
            }
            // Keep playing through the old sink rather than falling silent:
            // a resampled track is worse than the source, and better than
            // no track.
            Err(e) => tracing::warn!("could not reopen the output: {e}"),
        }
    }

    player.append(decoder);
    player.play();

    Ok(PlaybackEvent::Started {
        bit_depth: None,
        sample_rate,
        // The decoder knows neither the bit depth nor what TIDAL called the
        // stream; the caller replaces both from the playback-info response.
        delivered: crate::domain::Quality::Low,
    })
}

#[cfg(test)]
mod tests {

    #[test]
    fn reopening_the_output_carries_the_volume_across() {
        // A fresh player starts at full. Reopening happens on a change of
        // sample rate, so without carrying the level the setting was undone
        // by the first track at a different rate — silently, and only
        // sometimes, which is the hardest kind of bug to be told about.
        //
        // Opening a device needs hardware, so this reads the source: the
        // level must be taken before the player is replaced and put back
        // after.
        let source = include_str!("engine.rs");
        let reopen = source
            .split("*player = rodio::Player::connect_new")
            .next()
            .expect("the reopen path");
        assert!(
            reopen.contains("let volume = player.volume();"),
            "the level is read before the player is replaced"
        );
        let after = source
            .split("*player = rodio::Player::connect_new(sink.mixer());")
            .nth(1)
            .expect("what follows the replacement");
        assert!(
            after.trim_start().starts_with("player.set_volume(volume);"),
            "and put back on the new one"
        );
    }
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
