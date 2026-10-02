//! Does a seek land on the right audio? Decodes a reference straight
//! through, seeks the way the engine does, and compares the samples.
//!
//! Live, like `live_api.rs`: needs the network and a signed-in session.
//! It exists because rodio's `try_seek` on a stream it was not told the
//! length of *reports* the new position while the audio carries on from
//! where it was -- a bug no offline test can see, since the position
//! looked right throughout.
//!
//!     cargo test --test live_playback -- --ignored --nocapture

use rodio::Source;
use std::time::Duration;

/// A track known to be served as DASH at hi-res and as a single file at
/// lossless: Miles Davis, "Freddie Freeloader".
const TRACK: u64 = 655911;

fn session() -> Option<ratidal::auth::StoredToken> {
    match ratidal::auth::store::load() {
        Ok(Some(token)) => Some(token),
        _ => {
            eprintln!("skipped: not signed in");
            None
        }
    }
}

fn playback_info(quality: ratidal::domain::Quality) -> Option<ratidal::playback::PlaybackInfo> {
    let token = session()?;
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let info = rt.block_on(async {
        let client = ratidal::tidal::Client::new(token);
        client
            .playback_info(ratidal::domain::TrackId(TRACK), quality)
            .await
    });
    Some(info.expect("playback info"))
}

fn skip_n(src: &mut dyn Iterator<Item = f32>, n: usize) {
    assert!((0..n).all(|_| src.next().is_some()), "the stream ended early");
}

fn take_n(src: &mut dyn Iterator<Item = f32>, n: usize) -> Vec<f32> {
    src.take(n).collect()
}

/// A second of reference audio at `a` and at `b`, decoded straight through.
fn reference(
    mut decoder: impl Iterator<Item = f32>,
    rate: usize,
    ch: usize,
    a: Duration,
    b: Duration,
) -> (Vec<f32>, Vec<f32>) {
    let frames = |t: Duration| (t.as_secs_f64() * rate as f64).round() as usize * ch;
    skip_n(&mut decoder, frames(a));
    let at_a = take_n(&mut decoder, rate * ch);
    skip_n(&mut decoder, frames(b) - frames(a) - rate * ch);
    let at_b = take_n(&mut decoder, rate * ch);
    (at_a, at_b)
}

/// How many frames at the head differ before the two streams agree, or
/// None when they never do.
fn head_mismatch(reference: &[f32], test: &[f32], ch: usize) -> Option<usize> {
    assert_eq!(test.len(), reference.len(), "the test stream came up short");
    let last_bad = reference
        .iter()
        .zip(test)
        .rposition(|(x, y)| x != y)
        .map_or(0, |i| i / ch + 1);
    let agreed = reference.len() / ch - last_bad;
    (agreed > reference.len() / ch / 2).then_some(last_bad)
}

#[test]
#[ignore = "needs the network and a signed-in session"]
fn a_dash_stream_rebuilt_at_a_segment_plays_the_same_samples() {
    let Some(info) = playback_info(ratidal::domain::Quality::HiResLossless) else {
        return;
    };
    let ratidal::playback::Manifest::Dash {
        init,
        segments,
        segment_durations,
    } = info.manifest
    else {
        panic!("expected DASH at hi-res")
    };
    let http = reqwest::blocking::Client::new();
    let (a, b) = (Duration::from_millis(16770), Duration::from_millis(26770));
    let full = rodio::Decoder::new(ratidal::playback::SegmentReader::new(
        http.clone(),
        init.clone(),
        segments.clone(),
    ))
    .expect("decoder");
    let (rate, ch) = (full.sample_rate().get() as usize, full.channels().get() as usize);
    let (ref_a, ref_b) = reference(full, rate, ch, a, b);

    // As the engine seeks: back to b, then further back to a.
    for (target, expected) in [(b, &ref_b), (a, &ref_a)] {
        let (start, skip) = restart_point(&segment_durations, target);
        let decoder = rodio::Decoder::new(ratidal::playback::SegmentReader::new(
            http.clone(),
            init.clone(),
            segments[start..].to_vec(),
        ))
        .expect("decoder");
        let got = take_n(&mut decoder.skip_duration(skip), rate * ch);
        assert_eq!(
            head_mismatch(expected, &got, ch),
            Some(0),
            "restarting at segment {start} and skipping {skip:?} must land on {target:?} exactly"
        );
    }
}

#[test]
#[ignore = "needs the network and a signed-in session"]
fn a_bts_file_held_in_memory_seeks_for_real() {
    let Some(info) = playback_info(ratidal::domain::Quality::Lossless) else {
        return;
    };
    let ratidal::playback::Manifest::Bts { url } = info.manifest else {
        panic!("expected a single file at lossless")
    };
    let bytes = reqwest::blocking::get(&url)
        .and_then(|r| r.bytes())
        .expect("the file")
        .to_vec();
    let (a, b) = (Duration::from_millis(16770), Duration::from_millis(26770));
    let full = rodio::Decoder::new(ratidal::playback::SegmentReader::from_slices(vec![
        bytes.clone()
    ]))
    .expect("decoder");
    let (rate, ch) = (full.sample_rate().get() as usize, full.channels().get() as usize);
    let (ref_a, ref_b) = reference(full, rate, ch, a, b);

    // As the engine builds it: seekable, with its length.
    let len = bytes.len() as u64;
    let mut src = rodio::decoder::DecoderBuilder::new()
        .with_data(ratidal::playback::SegmentReader::from_slices(vec![bytes]))
        .with_byte_len(len)
        .with_seekable(true)
        .build()
        .expect("decoder")
        .track_position();
    skip_n(&mut src, 37 * rate * ch);
    for (target, expected) in [(b, &ref_b), (a, &ref_a)] {
        src.try_seek(target).expect("a seekable stream seeks");
        let got = take_n(&mut src, rate * ch);
        // rodio lands within a few frames; what follows must be the track.
        let head = head_mismatch(expected, &got, ch).unwrap_or_else(|| {
            panic!("after seeking to {target:?} the audio is not the track's")
        });
        assert!(
            head < rate / 20,
            "seeking to {target:?} landed {head} frames off, more than 50ms"
        );
    }
}

/// The engine's own arithmetic, repeated here because it is crate-private
/// there and this is what the test is about.
fn restart_point(durations: &[Duration], at: Duration) -> (usize, Duration) {
    let mut acc = Duration::ZERO;
    for (i, d) in durations.iter().enumerate() {
        if at < acc + *d {
            return (i, at - acc);
        }
        acc += *d;
    }
    let last = durations.len().saturating_sub(1);
    (last, at.saturating_sub(acc - durations.last().copied().unwrap_or_default()))
}
