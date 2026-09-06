use std::time::Duration;

use base64::Engine as _;
use quick_xml::events::Event;

use crate::domain::Quality;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Manifest {
    Bts {
        url: String,
    },
    Dash {
        init: String,
        segments: Vec<String>,
        segment_durations: Vec<Duration>,
    },
}

#[derive(Debug, Clone)]
pub struct PlaybackInfo {
    pub manifest: Manifest,
    /// What TIDAL actually delivered, which may be lower than requested.
    pub delivered: Quality,
    pub bit_depth: Option<u8>,
    pub sample_rate: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    /// TIDAL refused playback. `sub_status` 4005 means the configured
    /// client_id has been capped and the user must change it — the actionable
    /// wording lives on `TidalError::NotAvailable`, which is what reaches the
    /// user. Surface that rather than this variant directly.
    #[error("track not available for playback (subStatus {sub_status}): {message}")]
    NotAvailable { sub_status: u32, message: String },
    #[error("response was not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("manifest was not valid base64: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("unsupported manifest type: {mime}")]
    UnsupportedManifest { mime: String },
    #[error("malformed MPD: {0}")]
    Mpd(String),
    #[error("unknown audio quality in response: {0}")]
    Quality(String),
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct InfoDto {
    #[serde(rename = "audioQuality")]
    audio_quality: String,
    #[serde(rename = "manifestMimeType")]
    manifest_mime_type: String,
    manifest: String,
    #[serde(rename = "bitDepth")]
    bit_depth: Option<u8>,
    #[serde(rename = "sampleRate")]
    sample_rate: Option<u32>,
    // Error shape, present instead of the above on failure.
    status: Option<u32>,
    #[serde(rename = "subStatus")]
    sub_status: Option<u32>,
    #[serde(rename = "userMessage")]
    user_message: Option<String>,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct BtsDto {
    urls: Vec<String>,
}

pub fn parse_playback_info(json: &str) -> Result<PlaybackInfo, ManifestError> {
    let dto: InfoDto = serde_json::from_str(json)?;

    if let Some(sub_status) = dto.sub_status {
        return Err(ManifestError::NotAvailable {
            sub_status,
            message: dto.user_message.unwrap_or_else(|| "no message".into()),
        });
    }

    let raw = base64::engine::general_purpose::STANDARD.decode(dto.manifest.as_bytes())?;

    let manifest = match dto.manifest_mime_type.as_str() {
        "application/vnd.tidal.bts" => {
            let bts: BtsDto = serde_json::from_slice(&raw)?;
            let url = bts
                .urls
                .into_iter()
                .next()
                .ok_or_else(|| ManifestError::Mpd("BTS manifest had no urls".into()))?;
            Manifest::Bts { url }
        }
        "application/dash+xml" => parse_mpd(&String::from_utf8_lossy(&raw))?,
        other => {
            return Err(ManifestError::UnsupportedManifest { mime: other.to_string() })
        }
    };

    let delivered = dto
        .audio_quality
        .parse::<Quality>()
        .map_err(|_| ManifestError::Quality(dto.audio_quality.clone()))?;

    Ok(PlaybackInfo {
        manifest,
        delivered,
        bit_depth: dto.bit_depth,
        sample_rate: dto.sample_rate,
    })
}

/// Parse TIDAL's MPD subset: one Representation, SegmentTemplate with
/// $Number$, and a SegmentTimeline. Verified against a real hi-res manifest —
/// no ContentProtection, no multiple representations to choose between.
fn parse_mpd(xml: &str) -> Result<Manifest, ManifestError> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut init_template = String::new();
    let mut media_template = String::new();
    let mut timescale: u32 = 1;
    let mut start_number: u64 = 1;
    // (duration_in_timescale_units, repeat_count)
    let mut timeline: Vec<(u64, u64)> = Vec::new();

    loop {
        match reader.read_event() {
            Err(e) => return Err(ManifestError::Mpd(e.to_string())),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = e.name();
                // quick-xml 0.42 hands back &str, not bytes.
                let tag = name.as_ref().to_string();
                match tag.as_str() {
                    "SegmentTemplate" => {
                        for attr in e.attributes().flatten() {
                            let key = attr.key.as_ref().to_string();
                            // Read `value` directly: DASH attribute values carry
                            // no XML entities, and 0.42 deprecated
                            // unescape_value() in favour of a version-taking API.
                            let val = attr.value.as_ref().to_string();
                            match key.as_str() {
                                "initialization" => init_template = val,
                                "media" => media_template = val,
                                "timescale" => timescale = val.parse().unwrap_or(1),
                                "startNumber" => start_number = val.parse().unwrap_or(1),
                                _ => {}
                            }
                        }
                    }
                    "S" => {
                        let mut d = 0u64;
                        let mut r = 0u64;
                        for attr in e.attributes().flatten() {
                            let key = attr.key.as_ref().to_string();
                            let val = attr.value.as_ref().to_string();
                            match key.as_str() {
                                "d" => d = val.parse().unwrap_or(0),
                                "r" => r = val.parse().unwrap_or(0),
                                _ => {}
                            }
                        }
                        timeline.push((d, r));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    if init_template.is_empty() || media_template.is_empty() {
        return Err(ManifestError::Mpd("no SegmentTemplate found".into()));
    }
    if timescale == 0 {
        return Err(ManifestError::Mpd("timescale was zero".into()));
    }

    // A manifest is untrusted input: TIDAL can change shape, and a malformed
    // or hostile response must not be able to take the app down. Two bounds:
    //
    // - A track longer than a day is not a track. Without this cap, `r=` is a
    //   segment count straight from the wire — `r="999999999"` would expand to
    //   a billion entries and try to allocate ~16GB before anything noticed.
    // - `Duration::from_secs_f64` PANICS on a value too large to represent,
    //   and a panic in raw terminal mode wrecks the user's shell. Reject an
    //   absurd `d` instead of letting it reach that constructor.
    const MAX_SEGMENTS: usize = 100_000;
    const MAX_SEGMENT_SECS: f64 = 86_400.0;

    let planned: u64 = timeline.iter().map(|(_, r)| r.saturating_add(1)).sum();
    if planned > MAX_SEGMENTS as u64 {
        return Err(ManifestError::Mpd(format!(
            "SegmentTimeline describes {planned} segments, more than the {MAX_SEGMENTS} cap"
        )));
    }
    // The last segment number must be representable. Otherwise the numbering
    // would silently wrap in a release build and every fetch would 404
    // mid-track, which is far harder to diagnose than a rejection here.
    if start_number.checked_add(planned).is_none() {
        return Err(ManifestError::Mpd(format!(
            "startNumber {start_number} plus {planned} segments overflows"
        )));
    }

    let mut segment_durations = Vec::with_capacity(planned as usize);
    for (d, r) in timeline {
        let secs = d as f64 / timescale as f64;
        if !secs.is_finite() || !(0.0..=MAX_SEGMENT_SECS).contains(&secs) {
            return Err(ManifestError::Mpd(format!(
                "segment duration {secs}s is out of range"
            )));
        }
        for _ in 0..=r {
            segment_durations.push(Duration::from_secs_f64(secs));
        }
    }
    if segment_durations.is_empty() {
        return Err(ManifestError::Mpd("SegmentTimeline was empty".into()));
    }

    let segments = (0..segment_durations.len())
        .map(|i| {
            // Saturating: `startNumber` comes off the wire unchecked, and a
            // value near u64::MAX would panic in debug and wrap silently in
            // release — producing URLs that 404 mid-track instead of failing
            // honestly here.
            media_template.replace(
                "$Number$",
                &start_number.saturating_add(i as u64).to_string(),
            )
        })
        .collect();

    Ok(Manifest::Dash {
        init: init_template,
        segments,
        segment_durations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("tests/fixtures/json/{name}")).unwrap()
    }

    #[test]
    fn parses_a_hires_dash_response() {
        let info = parse_playback_info(&fixture("playbackinfo-hires.json")).unwrap();
        assert_eq!(info.delivered, crate::domain::Quality::HiResLossless);
        assert_eq!(info.bit_depth, Some(24));
        assert_eq!(info.sample_rate, Some(44100));

        let Manifest::Dash { init, segments, segment_durations } = info.manifest else {
            panic!("expected Dash");
        };
        assert!(init.contains("/0.mp4"));
        // SegmentTimeline: <S d="176128" r="31"/> is 32 segments, plus one more.
        assert_eq!(segments.len(), 33);
        assert_eq!(segment_durations.len(), 33);
        // $Number$ starts at startNumber=1 and increments.
        assert!(segments[0].contains("/1.mp4"));
        assert!(segments[32].contains("/33.mp4"));
        assert!(!segments[0].contains("$Number$"), "template must be substituted");
        // 176128 / 44100 timescale ~= 3.994s
        assert!((segment_durations[0].as_secs_f64() - 3.994).abs() < 0.01);
        // The last segment is the short remainder: 147139 / 44100 ~= 3.336s
        assert!((segment_durations[32].as_secs_f64() - 3.336).abs() < 0.01);
    }

    #[test]
    fn parses_a_bts_response() {
        let info = parse_playback_info(&fixture("playbackinfo-bts.json")).unwrap();
        // We asked for HI_RES_LOSSLESS; TIDAL delivered HIGH. The delivered
        // value is what we record — never the requested one.
        assert_eq!(info.delivered, crate::domain::Quality::High);
        let Manifest::Bts { url } = info.manifest else { panic!("expected Bts") };
        assert!(url.starts_with("https://"));
    }

    #[test]
    fn a_4005_error_response_is_a_named_variant() {
        // What every capped client_id returns. Must be diagnosable, since the
        // fix is a config edit, not a retry.
        let body = r#"{"status":401,"subStatus":4005,
                       "userMessage":"Asset is not ready for playback"}"#;
        match parse_playback_info(body) {
            Err(ManifestError::NotAvailable { sub_status, .. }) => {
                assert_eq!(sub_status, 4005);
            }
            other => panic!("expected NotAvailable, got {other:?}"),
        }
    }

    /// Wrap a base64 MPD in the playback-info envelope the parser expects.
    fn dash_response(manifest_b64: &str) -> String {
        format!(
            r#"{{"audioQuality":"HI_RES_LOSSLESS",
                 "manifestMimeType":"application/dash+xml",
                 "bitDepth":24,"sampleRate":44100,
                 "manifest":"{manifest_b64}"}}"#
        )
    }

    #[test]
    fn an_absurd_segment_duration_is_rejected_not_panicked_on() {
        // `d="1000000000000000000"` at timescale 1. Duration::from_secs_f64
        // PANICS on a value it cannot represent, and a panic in raw terminal
        // mode wrecks the user's shell — so this must come back as an error.
        let body = dash_response(concat!(
            "PD94bWwgdmVyc2lvbj0nMS4wJyBlbmNvZGluZz0nVVRGLTgnPz48TVBEPjxQZXJpb2Q+PEFkYXB0",
            "YXRpb25TZXQ+PFJlcHJlc2VudGF0aW9uPjxTZWdtZW50VGVtcGxhdGUgdGltZXNjYWxlPSIxIiBp",
            "bml0aWFsaXphdGlvbj0iaW5pdC5tcDQiIG1lZGlhPSIkTnVtYmVyJC5tNHMiIHN0YXJ0TnVtYmVy",
            "PSIxIj48U2VnbWVudFRpbWVsaW5lPjxTIGQ9IjEwMDAwMDAwMDAwMDAwMDAwMDAiIHI9IjAiLz48",
            "L1NlZ21lbnRUaW1lbGluZT48L1NlZ21lbnRUZW1wbGF0ZT48L1JlcHJlc2VudGF0aW9uPjwvQWRh",
            "cHRhdGlvblNldD48L1BlcmlvZD48L01QRD4="
        ));
        match parse_playback_info(&body) {
            Err(ManifestError::Mpd(m)) => {
                assert!(m.contains("out of range"), "unexpected message: {m}")
            }
            other => panic!("expected an Mpd range error, got {other:?}"),
        }
    }

    #[test]
    fn an_absurd_repeat_count_is_rejected_before_allocating() {
        // `r="999999999"` would expand to a billion segments — roughly 16GB of
        // Duration alone, before any URL strings. The cap must reject it
        // rather than let the allocation be attempted.
        let body = dash_response(concat!(
            "PD94bWwgdmVyc2lvbj0nMS4wJyBlbmNvZGluZz0nVVRGLTgnPz48TVBEPjxQZXJpb2Q+PEFkYXB0",
            "YXRpb25TZXQ+PFJlcHJlc2VudGF0aW9uPjxTZWdtZW50VGVtcGxhdGUgdGltZXNjYWxlPSIxIiBp",
            "bml0aWFsaXphdGlvbj0iaW5pdC5tcDQiIG1lZGlhPSIkTnVtYmVyJC5tNHMiIHN0YXJ0TnVtYmVy",
            "PSIxIj48U2VnbWVudFRpbWVsaW5lPjxTIGQ9IjEwMCIgcj0iOTk5OTk5OTk5Ii8+PC9TZWdtZW50",
            "VGltZWxpbmU+PC9TZWdtZW50VGVtcGxhdGU+PC9SZXByZXNlbnRhdGlvbj48L0FkYXB0YXRpb25T",
            "ZXQ+PC9QZXJpb2Q+PC9NUEQ+"
        ));
        let start = std::time::Instant::now();
        match parse_playback_info(&body) {
            Err(ManifestError::Mpd(m)) => {
                assert!(m.contains("cap"), "unexpected message: {m}")
            }
            other => panic!("expected an Mpd cap error, got {other:?}"),
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(1),
            "rejection must be immediate, not after a huge allocation"
        );
    }

    #[test]
    fn a_start_number_that_would_overflow_is_rejected() {
        // `startNumber="18446744073709551615"` (u64::MAX). Adding the segment
        // index panics in a debug build and wraps silently in release, which
        // would 404 every fetch mid-track instead of failing honestly here.
        let body = dash_response(concat!(
            "PD94bWwgdmVyc2lvbj0nMS4wJz8+PE1QRD48UGVyaW9kPjxBZGFwdGF0aW9uU2V0PjxSZXByZXNl",
            "bnRhdGlvbj48U2VnbWVudFRlbXBsYXRlIHRpbWVzY2FsZT0iMSIgaW5pdGlhbGl6YXRpb249Imlu",
            "aXQubXA0IiBtZWRpYT0iJE51bWJlciQubTRzIiBzdGFydE51bWJlcj0iMTg0NDY3NDQwNzM3MDk1",
            "NTE2MTUiPjxTZWdtZW50VGltZWxpbmU+PFMgZD0iNCIgcj0iMiIvPjwvU2VnbWVudFRpbWVsaW5l",
            "PjwvU2VnbWVudFRlbXBsYXRlPjwvUmVwcmVzZW50YXRpb24+PC9BZGFwdGF0aW9uU2V0PjwvUGVy",
            "aW9kPjwvTVBEPg=="
        ));
        match parse_playback_info(&body) {
            Err(ManifestError::Mpd(m)) => {
                assert!(m.contains("overflows"), "unexpected message: {m}")
            }
            other => panic!("expected an Mpd overflow error, got {other:?}"),
        }
    }

    #[test]
    fn a_realistic_segment_count_still_parses() {
        // The cap must not reject real manifests: the captured hi-res fixture
        // is 33 segments, and a long mix is still far under 100k.
        let info = parse_playback_info(&fixture("playbackinfo-hires.json")).unwrap();
        let Manifest::Dash { segments, .. } = info.manifest else {
            panic!("expected Dash");
        };
        assert_eq!(segments.len(), 33);
    }
}
