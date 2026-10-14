//! What sonora's playback engine needs from TIDAL: `music::engine::Fetch`.
//!
//! The engine owns the threads, the output device, the queue ahead of the
//! decoder and the gapless join. All TIDAL adds is how a track id becomes a
//! decoder: the playback-info request, the DASH manifest behind it, and a
//! `SegmentReader` over the segments.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use music::engine::Fetch;
use rodio::Source as _;

use crate::auth::StoredToken;
use crate::domain::{Quality, TrackId};
use crate::playback::engine::restart_point;
use crate::playback::{Manifest, PlaybackInfo, SegmentReader};
use crate::tidal::{Client, TidalError};

/// Who is asking, and for what. The shell sets it before each play, so a
/// refreshed token or a changed quality setting reaches the next request
/// without the engine knowing either exists.
#[derive(Clone)]
pub struct Session {
    pub token: StoredToken,
    pub wanted: Quality,
}

/// What a stream turned out to be: the response's word on depth and tier,
/// and the decoder's on rate. Read once the engine says the track is
/// playing, to fill the quality badge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    pub bit_depth: Option<u8>,
    pub delivered: Quality,
    pub sample_rate: u32,
}

/// A track the engine asked for, as a handle: cloned to line it up behind
/// the one playing, and kept so a seek can open a second decoder over it.
#[derive(Clone, Debug)]
pub struct Loaded {
    info: Arc<PlaybackInfo>,
    /// A single-file (BTS) stream, fetched whole in `load`. Segments are
    /// not fetched ahead: `SegmentReader` does that itself when opened.
    bytes: Option<Arc<Vec<u8>>>,
}

struct Shared {
    session: Mutex<Option<Session>>,
    /// What the next session is asked at. Kept apart from the session so a
    /// quality chosen while signed out holds for the sign-in that follows.
    wanted: Mutex<Quality>,
    /// The engine's own runtime drives `load`, so this is the async client.
    http: reqwest::Client,
    /// The audio thread's client. A timeout is not optional: a stalled CDN
    /// response there would hold the decoder, and with it the output.
    blocking: reqwest::blocking::Client,
    /// What `open` learned, by track id, until the shell reads it.
    meta: Mutex<HashMap<String, Meta>>,
    /// Why a load failed, by track id. The engine reports only that the
    /// track is unavailable; the shell wants to say why.
    failure: Mutex<HashMap<String, String>>,
}

/// Cheap to clone: every clone is the same fetcher. The engine takes one by
/// value and the shell keeps another to set the session and read results.
#[derive(Clone)]
pub struct TidalFetch {
    shared: Arc<Shared>,
}

/// A poisoned lock holds nothing worse than a stale map, so it is taken
/// anyway rather than panicking in raw mode.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl TidalFetch {
    pub fn new(wanted: Quality) -> Result<Self> {
        let blocking = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self {
            shared: Arc::new(Shared {
                session: Mutex::new(None),
                wanted: Mutex::new(wanted),
                http: reqwest::Client::new(),
                blocking,
                meta: Mutex::new(HashMap::new()),
                failure: Mutex::new(HashMap::new()),
            }),
        })
    }

    pub fn set_session(&self, session: Session) {
        *lock(&self.shared.wanted) = session.wanted;
        *lock(&self.shared.session) = Some(session);
    }

    /// What to ask for from the next load on: the shell's setting, which
    /// changes while signed in.
    pub fn set_quality(&self, wanted: Quality) {
        *lock(&self.shared.wanted) = wanted;
        if let Some(session) = lock(&self.shared.session).as_mut() {
            session.wanted = wanted;
        }
    }

    pub fn wanted(&self) -> Quality {
        *lock(&self.shared.wanted)
    }

    /// What the next `load` will ask with, for a test.
    #[cfg(test)]
    pub(crate) fn session(&self) -> Option<Session> {
        lock(&self.shared.session).clone()
    }

    /// What the last `open` of `id` found, taken rather than read: the next
    /// play of the same track records it afresh.
    pub fn take_meta(&self, id: &str) -> Option<Meta> {
        lock(&self.shared.meta).remove(id)
    }

    /// Why the last `load` of `id` failed, taken the same way.
    pub fn take_failure(&self, id: &str) -> Option<String> {
        lock(&self.shared.failure).remove(id)
    }

    /// What `open` does once it has a decoder, for a test with no stream.
    #[cfg(test)]
    pub(crate) fn record_meta(&self, id: &str, meta: Meta) {
        lock(&self.shared.meta).insert(id.to_owned(), meta);
    }

    /// Builds a decoder over `loaded` positioned at `at`. Split from `open`
    /// so a test can hand it a stream without a session.
    fn decoder(&self, id: &str, loaded: &Loaded, at: Duration) -> Result<Positioned> {
        let (decoder, skip) = match &loaded.info.manifest {
            // Held whole in memory and declared seekable, with its length,
            // so symphonia will seek it rather than refuse. `at` goes
            // through the decoder's own seek, which is exact.
            //
            // ponytail: `from_slices` copies the file; a seek costs the
            // copy again. A reader over the `Arc` itself is the upgrade.
            Manifest::Bts { .. } => {
                let bytes = loaded
                    .bytes
                    .as_ref()
                    .ok_or_else(|| anyhow!("a single-file stream with no bytes"))?;
                let mut decoder = rodio::decoder::DecoderBuilder::new()
                    .with_data(SegmentReader::from_slices(vec![bytes.to_vec()]))
                    .with_byte_len(bytes.len() as u64)
                    .with_seekable(true)
                    .build()?;
                if !at.is_zero() {
                    decoder.try_seek(at)?;
                }
                (decoder, Duration::ZERO)
            }
            // A fragmented stream has no index to seek in, so the stream is
            // rebuilt from the segment holding `at` and the rest of that
            // segment decoded and discarded. One fetch, and the samples
            // come out identical to playing straight through.
            Manifest::Dash {
                init,
                segments,
                segment_durations,
            } => {
                let (start, skip) = restart_point(segment_durations, at);
                let reader = SegmentReader::new(
                    self.shared.blocking.clone(),
                    init.clone(),
                    segments.get(start..).unwrap_or_default().to_vec(),
                );
                (rodio::Decoder::new(reader)?, skip)
            }
        };
        lock(&self.shared.meta).insert(
            id.to_owned(),
            Meta {
                bit_depth: loaded.info.bit_depth,
                delivered: loaded.info.delivered,
                sample_rate: decoder.sample_rate().get(),
            },
        );
        Ok(decoder.skip_duration(skip))
    }
}

type Positioned = rodio::source::SkipDuration<rodio::Decoder<SegmentReader>>;

/// What starts the engine on a TIDAL fetch: sonora's `PlaybackFactory`,
/// which both the provider and the shell's catalogue hand out.
pub struct TidalPlayback {
    fetch: TidalFetch,
}

impl TidalPlayback {
    pub fn new(fetch: TidalFetch) -> Self {
        Self { fetch }
    }
}

impl music::PlaybackFactory for TidalPlayback {
    fn start(
        &self,
        config: music::PlaybackConfig,
    ) -> (Box<dyn music::Player>, Box<dyn music::PlaybackEvents>) {
        music::engine::start(self.fetch.clone(), config)
    }
}

#[async_trait]
impl Fetch for TidalFetch {
    type Loaded = Loaded;
    type Source = Positioned;

    fn name(&self) -> &'static str {
        "tidal"
    }

    async fn load(&self, id: &str) -> Result<Loaded> {
        let track = id
            .parse::<u64>()
            .map(TrackId::from)
            .map_err(|_| anyhow!("not a TIDAL track id: {id}"))?;
        let Some(session) = lock(&self.shared.session).clone() else {
            bail!("not signed in");
        };
        // What the user asked for, not what this build prefers. The
        // response still says what was actually delivered.
        let client = Client::new(session.token);
        let info = match client.playback_info(track, session.wanted).await {
            Ok(info) => info,
            Err(TidalError::Unauthorized) => return Err(TidalError::Unauthorized.into()),
            Err(error) => {
                lock(&self.shared.failure).insert(id.to_owned(), error.to_string());
                return Err(error.into());
            }
        };
        let bytes = match &info.manifest {
            Manifest::Bts { url } => {
                let body = self
                    .shared
                    .http
                    .get(url)
                    .send()
                    .await?
                    .error_for_status()?
                    .bytes()
                    .await?;
                Some(Arc::new(body.to_vec()))
            }
            Manifest::Dash { .. } => None,
        };
        Ok(Loaded {
            info: Arc::new(info),
            bytes,
        })
    }

    /// TIDAL measures ReplayGain per track and sends it with the stream.
    fn loudness(&self, loaded: &Loaded) -> Option<music::engine::Loudness> {
        Some(music::engine::Loudness::replay_gain(
            loaded.info.replay_gain?,
            loaded.info.peak,
        ))
    }

    fn gated(&self, error: &anyhow::Error) -> bool {
        matches!(
            error.downcast_ref::<TidalError>(),
            Some(TidalError::Unauthorized)
        )
    }

    fn open(&self, id: &str, loaded: &Loaded, at: Duration) -> Option<Positioned> {
        match self.decoder(id, loaded, at) {
            Ok(source) => Some(source),
            Err(error) => {
                tracing::warn!("could not open track {id} at {at:?}: {error:#}");
                lock(&self.shared.failure).insert(id.to_owned(), error.to_string());
                None
            }
        }
    }

    fn reopen_to_seek(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("tests/fixtures/dash/{name}")).unwrap()
    }

    /// Serves `tests/fixtures/dash` over HTTP for as long as the test runs.
    fn serve_fixtures() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut sock in listener.incoming().flatten() {
                let mut buf = [0u8; 1024];
                let n = sock.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let name = request
                    .split_whitespace()
                    .nth(1)
                    .and_then(|path| path.rsplit('/').next())
                    .unwrap_or("")
                    .to_owned();
                let body = fixture(&name);
                let _ = write!(
                    sock,
                    "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(&body);
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    /// The fixture manifest: three segments of mono 44.1 kHz FLAC, as
    /// `out.mpd` declares them.
    fn dash(base: &str) -> Loaded {
        let timescale = 44_100.0;
        Loaded {
            info: Arc::new(PlaybackInfo {
                manifest: Manifest::Dash {
                    init: format!("{base}/init.mp4"),
                    segments: (1..=3).map(|n| format!("{base}/{n}.m4s")).collect(),
                    segment_durations: [179_712.0, 179_712.0, 173_664.0]
                        .into_iter()
                        .map(|d| Duration::from_secs_f64(d / timescale))
                        .collect(),
                },
                delivered: Quality::HiResLossless,
                bit_depth: Some(24),
                sample_rate: Some(44_100),
                replay_gain: None,
                peak: None,
            }),
            bytes: None,
        }
    }

    #[test]
    fn a_dash_stream_opened_at_an_offset_is_the_rest_of_the_track() {
        // What the engine relies on when it reopens to seek: the source it
        // gets back starts at `at`, not at the segment boundary before it.
        let base = serve_fixtures();
        let fetch = TidalFetch::new(Quality::Lossless).unwrap();
        let loaded = dash(&base);

        let whole = fetch.open("1", &loaded, Duration::ZERO).expect("the track").count();
        let at = Duration::from_secs(5);
        let rest = fetch.open("1", &loaded, at).expect("the seek").count();

        let skipped = whole - rest;
        let expected = (at.as_secs_f64() * 44_100.0) as usize;
        // Within one FLAC block: the skip decodes and discards up to `at`,
        // and the segment durations are the file's own, so a wide margin
        // here would hide a seek that landed on the segment start instead.
        assert!(
            skipped.abs_diff(expected) <= 4096,
            "skipped {skipped} samples, expected about {expected}"
        );
    }

    #[test]
    fn opening_records_what_the_badge_needs() {
        // Bit depth and tier come from the response, the rate from the
        // decoder: the engine's own events carry none of them.
        let base = serve_fixtures();
        let fetch = TidalFetch::new(Quality::Lossless).unwrap();
        let loaded = dash(&base);

        assert_eq!(fetch.take_meta("7"), None, "nothing opened yet");
        let _ = fetch.open("7", &loaded, Duration::ZERO).expect("the track");
        assert_eq!(
            fetch.take_meta("7"),
            Some(Meta {
                bit_depth: Some(24),
                delivered: Quality::HiResLossless,
                sample_rate: 44_100,
            })
        );
        assert_eq!(fetch.take_meta("7"), None, "taken, not read");
    }

    #[test]
    fn a_single_file_stream_opens_from_memory() {
        // The whole file is in hand by the time `open` runs, and the
        // decoder's own seek positions it.
        let bytes: Vec<u8> = ["init.mp4", "1.m4s", "2.m4s", "3.m4s"]
            .into_iter()
            .flat_map(fixture)
            .collect();
        let fetch = TidalFetch::new(Quality::Lossless).unwrap();
        let loaded = Loaded {
            info: Arc::new(PlaybackInfo {
                manifest: Manifest::Bts {
                    url: "unused".into(),
                },
                delivered: Quality::Lossless,
                bit_depth: None,
                sample_rate: None,
                replay_gain: None,
                peak: None,
            }),
            bytes: Some(Arc::new(bytes)),
        };
        let whole = fetch.open("2", &loaded, Duration::ZERO).expect("the file").count();
        assert!(whole > 44_100 * 11, "about twelve seconds of audio: {whole}");
    }

    #[test]
    fn a_changed_quality_reaches_the_next_load_signed_in_or_not() {
        // The setting changes while signed in, and the stream is asked for
        // by the fetch: what it learns has to land in the session, or the
        // next track plays at the old quality.
        let fetch = TidalFetch::new(Quality::Lossless).unwrap();
        fetch.set_quality(Quality::HiResLossless);
        fetch.set_session(Session {
            token: StoredToken::default(),
            wanted: fetch.wanted(),
        });
        assert_eq!(fetch.session().map(|s| s.wanted), Some(Quality::HiResLossless));

        fetch.set_quality(Quality::High);
        assert_eq!(fetch.session().map(|s| s.wanted), Some(Quality::High));
        assert_eq!(fetch.wanted(), Quality::High);
    }

    #[test]
    fn loading_without_a_session_fails_before_the_network() {
        let fetch = TidalFetch::new(Quality::Lossless).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let error = rt.block_on(fetch.load("655911")).unwrap_err();
        assert!(error.to_string().contains("not signed in"), "{error}");
        let error = rt.block_on(fetch.load("not-a-number")).unwrap_err();
        assert!(error.to_string().contains("not a TIDAL track id"), "{error}");
    }

    #[test]
    fn a_rejected_session_is_gated_not_unavailable() {
        // The engine turns a gated load into a sign-in prompt and any other
        // failure into "unavailable", so the distinction has to survive
        // the trip through `anyhow`.
        let fetch = TidalFetch::new(Quality::Lossless).unwrap();
        assert!(fetch.gated(&anyhow::Error::from(TidalError::Unauthorized)));
        assert!(!fetch.gated(&anyhow!("the CDN is down")));
    }

    #[test]
    fn playback_asks_for_the_configured_quality() {
        // The setting existed in the file for weeks and was read nowhere:
        // both request sites named HI_RES_LOSSLESS outright, so changing it
        // did nothing at all. What is checked is that the request site has
        // not gone back to naming a tier of its own.
        let source = include_str!("fetch.rs");
        let body = source.split("mod tests").next().unwrap();
        let asks: Vec<&str> = body
            .lines()
            .filter(|l| l.contains(".playback_info("))
            .collect();
        assert!(!asks.is_empty(), "playback_info is called somewhere");
        for line in asks {
            assert!(
                !line.contains("Quality::"),
                "this asks for a fixed quality rather than the configured one: {line}"
            );
        }
    }
}
