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
    let sink = match open_sink(None) {
        Ok(s) => s,
        Err(e) => {
            let _ = events.send(PlaybackEvent::Error(format!("no audio device: {e}")));
            return;
        }
    };
    let mut player = rodio::Player::connect_new(sink.mixer());
    // An Option so a reopen can close the old device before opening the
    // new one; see start_stream.
    let mut sink = Some(sink);

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
    let mut reported_finished = true;
    // What the player counts from: zero for a track started at its head,
    // the seek target once a DASH stream has been rebuilt there.
    let mut base = Duration::ZERO;

    loop {
        // Poll for commands, but wake regularly to report position and
        // end-of-track. 250ms gives ~4Hz progress updates.
        let cmd = cmds.recv_timeout(Duration::from_millis(250));

        match cmd {
            Ok(Cmd::Play {
                manifest,
                bit_depth,
                delivered,
            }) => {
                match start_stream(&mut sink, &mut player, &http, &manifest, Duration::ZERO) {
                    Ok((info, from)) => {
                        base = from;
                        current = Some(manifest);
                        reported_finished = false;
                        // start_stream reads the sample rate off the decoder;
                        // the bit depth only exists in the manifest response.
                        let info = match info {
                            PlaybackEvent::Started { sample_rate, .. } => PlaybackEvent::Started {
                                bit_depth,
                                sample_rate,
                                delivered,
                            },
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
            Ok(Cmd::Seek(to)) => match &current {
                // In memory and declared seekable, so rodio's own seek is
                // real: it lands within a frame or two, in microseconds.
                Some(Manifest::Bts { .. }) => {
                    if let Err(e) = player.try_seek(to) {
                        let _ = events.send(PlaybackEvent::Error(format!("could not seek: {e}")));
                    }
                }
                // symphonia cannot seek a stream whose length it does not
                // know, and rodio's try_seek on one is worse than refused:
                // it *reports* the new position while the audio carries on
                // from where it was, and a second seek ends the stream, so
                // the queue moved on to the next track. The stream is
                // rebuilt from the segment holding `to` instead, and the
                // rest of that segment skipped; one fetch, and the samples
                // come out identical to playing straight through.
                Some(manifest @ Manifest::Dash { .. }) => {
                    // A rebuilt stream starts playing; one seeked while
                    // paused should stay paused, as rodio's own seek would.
                    let paused = player.is_paused();
                    // No Started for the shell here: same stream, same
                    // badge, and it reads one as "playing from the top".
                    match start_stream(&mut sink, &mut player, &http, manifest, to) {
                        Ok((_, from)) => {
                            base = from;
                            if paused {
                                player.pause();
                            }
                        }
                        Err(e) => {
                            let _ = events.send(PlaybackEvent::Error(e));
                        }
                    }
                }
                None => {}
            },
            Ok(Cmd::Stop) => {
                player.pause();
                player.clear();
                current = None;
                // Cleared with the manifest so no later path can report a
                // previous track's depth.
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }

        if current.is_some() {
            let _ = events.send(PlaybackEvent::Position(base + player.get_pos()));
            if player.empty() && !reported_finished {
                reported_finished = true;
                current = None;
                let _ = events.send(PlaybackEvent::Finished);
            }
        }
    }
}

/// Where a seek into a DASH stream restarts: the segment holding `at`,
/// and how much of it to skip to land on `at` itself.
pub(crate) fn restart_point(durations: &[Duration], at: Duration) -> (usize, Duration) {
    let index = segment_for(durations, at);
    (index, at.saturating_sub(segment_start(durations, index)))
}

/// Build a decoder and hand it to a player at the stream's own rate.
///
/// Returns what the player now counts its position from: `from` for a
/// DASH stream rebuilt there, zero otherwise.
///
/// The sink and the player are rebuilt when the stream needs a rate the
/// device is not already open at: rodio resamples anything that does not
/// match its configuration, which on a hi-res stream discards the reason
/// for asking for hi-res. A decoder has to exist before its rate is known,
/// so the order is decode, then reopen, then append.
fn start_stream(
    sink: &mut Option<rodio::MixerDeviceSink>,
    player: &mut rodio::Player,
    http: &reqwest::blocking::Client,
    manifest: &Manifest,
    from: Duration,
) -> Result<(PlaybackEvent, Duration), String> {
    player.clear();

    let (decoder, skip, base) = match manifest {
        // One file, fetched whole before the first sample either way: the
        // reader used to block on the same download. Held in memory and
        // declared seekable, with its length, so that symphonia will seek
        // it rather than refuse -- the FLAC demuxer needs both. `from` is
        // not applied here: a seek in a BTS stream goes through the player.
        Manifest::Bts { url } => {
            let bytes = super::segments::fetch(http, url).map_err(|e| e.to_string())?;
            let len = bytes.len() as u64;
            let decoder = rodio::decoder::DecoderBuilder::new()
                .with_data(SegmentReader::from_slices(vec![bytes]))
                .with_byte_len(len)
                .with_seekable(true)
                .build()
                .map_err(|e| format!("could not decode the stream: {e}"))?;
            (decoder, Duration::ZERO, Duration::ZERO)
        }
        Manifest::Dash {
            init,
            segments,
            segment_durations,
        } => {
            let (start, skip) = restart_point(segment_durations, from);
            if from > Duration::ZERO {
                tracing::debug!("seek to {from:?}: segment {start}, skipping {skip:?}");
            }
            let reader = SegmentReader::new(http.clone(), init.clone(), segments[start..].to_vec());
            let decoder = rodio::Decoder::new(reader)
                .map_err(|e| format!("could not decode the stream: {e}"))?;
            (decoder, skip, from)
        }
    };

    use rodio::Source as _;
    let rate = decoder.sample_rate();
    // rodio's SampleRate is NonZero<u32>; PlaybackEvent carries a plain u32.
    let sample_rate = rate.get();

    // Reopen only on a change: tearing the device down between every track
    // costs a gap, and most of a library is one rate.
    let open_rate = sink.as_ref().map(|s| s.config().sample_rate());
    if open_rate != Some(rate) {
        tracing::info!(
            "reopening the output at {} Hz (was {:?})",
            rate.get(),
            open_rate.map(NonZero::get)
        );
        // Carried across: a fresh player starts at full volume, so without
        // this the setting was undone by the first track at a different
        // rate -- silently, and only sometimes.
        let volume = player.volume();
        // Closed before the new one opens. PipeWire switches the device's
        // rate only while no stream holds it, and the old stream held it at
        // the old rate: opening first left every hi-res track resampled to
        // whatever the device was already running at.
        *sink = None;
        let fresh = open_sink(Some(rate)).or_else(|e| {
            // Reopen at the old rate rather than fall silent: a resampled
            // track is worse than the source, and better than no track.
            tracing::warn!("could not reopen the output at {} Hz: {e}", rate.get());
            open_sink(open_rate)
        })?;
        *player = rodio::Player::connect_new(fresh.mixer());
        player.set_volume(volume);
        *sink = Some(fresh);
    }

    // The skip decodes and discards up to `from` within the segment, so
    // the player counts from `from` exactly.
    player.append(decoder.skip_duration(skip));
    player.play();

    Ok((
        PlaybackEvent::Started {
            bit_depth: None,
            sample_rate,
            // The decoder knows neither the bit depth nor what TIDAL called
            // the stream; the caller replaces both from the playback-info
            // response.
            delivered: crate::domain::Quality::Low,
        },
        base,
    ))
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
            .split("*player = rodio::Player::connect_new(fresh.mixer());")
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
