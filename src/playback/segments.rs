use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};

/// A `Read + Seek` view over an init segment followed by numbered media
/// segments, byte-concatenated into one stream.
///
/// Fetched bytes are retained: format probing seeks backward, and rodio's
/// decoder requires `Seek`. `ponytail:` a whole hi-res track is ~40MB in
/// memory; if that ever matters the upgrade is a disk-backed cache behind the
/// same interface.
pub struct SegmentReader {
    source: Source,
    /// URLs not yet fetched, in order.
    pending: VecDeque<String>,
    /// Everything fetched so far, concatenated.
    buf: Vec<u8>,
    /// Logical read cursor into `buf`.
    pos: u64,
}

enum Source {
    Http(reqwest::blocking::Client),
    /// Test-only: parts supplied directly.
    Memory(VecDeque<Vec<u8>>),
}

impl SegmentReader {
    pub fn new(
        client: reqwest::blocking::Client,
        init: String,
        segments: Vec<String>,
    ) -> Self {
        let mut pending: VecDeque<String> = segments.into();
        // The init segment carries the moov box and must come first.
        pending.push_front(init);
        Self {
            source: Source::Http(client),
            pending,
            buf: Vec::new(),
            pos: 0,
        }
    }

    /// In-memory construction, for tests.
    pub fn from_slices(parts: Vec<Vec<u8>>) -> Self {
        Self {
            source: Source::Memory(parts.into()),
            pending: VecDeque::new(),
            buf: Vec::new(),
            pos: 0,
        }
    }

    /// Append one more segment to `buf`. Returns false when exhausted.
    fn fetch_more(&mut self) -> std::io::Result<bool> {
        match &mut self.source {
            Source::Memory(parts) => match parts.pop_front() {
                Some(p) => {
                    self.buf.extend_from_slice(&p);
                    Ok(true)
                }
                None => Ok(false),
            },
            Source::Http(client) => {
                let Some(url) = self.pending.pop_front() else {
                    return Ok(false);
                };
                let resp = client
                    .get(&url)
                    .send()
                    .map_err(|e| std::io::Error::other(format!("fetching segment: {e}")))?;
                if !resp.status().is_success() {
                    return Err(std::io::Error::other(format!(
                        "fetching segment: HTTP {}",
                        resp.status()
                    )));
                }
                let bytes = resp
                    .bytes()
                    .map_err(|e| std::io::Error::other(format!("reading segment: {e}")))?;
                self.buf.extend_from_slice(&bytes);
                Ok(true)
            }
        }
    }
}

impl Read for SegmentReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        // Pull segments until we have bytes at `pos`, or run out. The loop
        // (not an `if`) is what makes an empty segment a no-op rather than EOF.
        while self.pos as usize >= self.buf.len() {
            if !self.fetch_more()? {
                return Ok(0);
            }
        }
        let start = self.pos as usize;
        let n = (self.buf.len() - start).min(out.len());
        out[..n].copy_from_slice(&self.buf[start..start + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for SegmentReader {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        let target: i64 = match from {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::Current(d) => self.pos as i64 + d,
            SeekFrom::End(d) => {
                // The total length is unknown until everything is fetched.
                while self.fetch_more()? {}
                self.buf.len() as i64 + d
            }
        };
        if target < 0 {
            return Err(std::io::Error::other("seek to a negative position"));
        }
        // Seeking past what we hold: fetch forward until we reach it, or the
        // stream ends (in which case the next read returns EOF).
        while (target as usize) > self.buf.len() {
            if !self.fetch_more()? {
                break;
            }
        }
        self.pos = target as u64;
        Ok(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, SeekFrom};

    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("tests/fixtures/dash/{name}")).unwrap()
    }

    #[test]
    fn concatenates_parts_in_order() {
        let mut r = SegmentReader::from_slices(vec![
            b"aaa".to_vec(),
            b"bbb".to_vec(),
            b"ccc".to_vec(),
        ]);
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"aaabbbccc");
    }

    #[test]
    fn honours_small_read_buffers_across_part_boundaries() {
        // A decoder reads in arbitrary chunk sizes; a part boundary must not
        // truncate the stream or duplicate bytes.
        let mut r = SegmentReader::from_slices(vec![b"ab".to_vec(), b"cd".to_vec()]);
        let mut buf = [0u8; 1];
        let mut got = Vec::new();
        while r.read(&mut buf).unwrap() == 1 {
            got.push(buf[0]);
        }
        assert_eq!(got, b"abcd");
    }

    #[test]
    fn an_empty_part_is_skipped_not_treated_as_eof() {
        let mut r = SegmentReader::from_slices(vec![
            b"a".to_vec(),
            Vec::new(),
            b"b".to_vec(),
        ]);
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"ab");
    }

    #[test]
    fn reports_eof_after_the_last_part() {
        let mut r = SegmentReader::from_slices(vec![b"x".to_vec()]);
        let mut buf = [0u8; 8];
        assert_eq!(r.read(&mut buf).unwrap(), 1);
        assert_eq!(r.read(&mut buf).unwrap(), 0);
        assert_eq!(r.read(&mut buf).unwrap(), 0, "EOF must be repeatable");
    }

    #[test]
    fn seeks_backward_and_forward_across_parts() {
        // Format probing seeks backward. Without this the decoder cannot even
        // identify the container.
        let mut r = SegmentReader::from_slices(vec![b"abc".to_vec(), b"def".to_vec()]);
        let mut two = [0u8; 2];

        r.read_exact(&mut two).unwrap();
        assert_eq!(&two, b"ab");

        r.seek(SeekFrom::Start(0)).unwrap();
        r.read_exact(&mut two).unwrap();
        assert_eq!(&two, b"ab", "backward seek must re-read from memory");

        r.seek(SeekFrom::Start(4)).unwrap();
        r.read_exact(&mut two).unwrap();
        assert_eq!(&two, b"ef", "forward seek must pull the next part");

        assert_eq!(r.seek(SeekFrom::End(0)).unwrap(), 6, "End must know total length");
    }

    #[test]
    fn seeking_before_the_start_is_an_error_not_a_panic() {
        let mut r = SegmentReader::from_slices(vec![b"ab".to_vec()]);
        assert!(r.seek(SeekFrom::Current(-5)).is_err());
    }

    #[test]
    fn an_unreachable_segment_url_is_an_error_not_a_short_track() {
        // The HTTP source is otherwise untested: every other test here uses
        // Source::Memory. A fetch failure must surface as an io::Error, never
        // as a silent EOF — that would decode as a truncated track with no
        // indication anything went wrong.
        //
        // Binding a port and dropping it gives an address nothing listens on,
        // so this fails fast and needs no network.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map(|a| a.port())
            .expect("binding an ephemeral port");

        let mut reader = SegmentReader::new(
            reqwest::blocking::Client::new(),
            format!("http://127.0.0.1:{port}/init.mp4"),
            vec![format!("http://127.0.0.1:{port}/1.m4s")],
        );

        let mut buf = [0u8; 64];
        let err = reader.read(&mut buf).expect_err("a refused connection must error");
        assert!(
            err.to_string().contains("fetching segment"),
            "the error should say which stage failed, got: {err}"
        );
    }

    #[test]
    fn satisfies_rodio_decoder_bounds() {
        // rodio::DecoderBuilder is implemented only for
        // R: Read + Seek + Send + Sync + 'static. A regression here shows up
        // as a confusing trait error at the call site instead of here.
        fn assert_bounds<T: std::io::Read + std::io::Seek + Send + Sync + 'static>() {}
        assert_bounds::<SegmentReader>();
    }

    #[test]
    fn rodio_decodes_the_concatenated_fixture_segments() {
        // The load-bearing test: init + 3 media segments, byte-concatenated,
        // must decode as FLAC-in-fMP4. Verified to yield exactly 529200
        // samples at 44100 Hz (12.00s).
        let reader = SegmentReader::from_slices(vec![
            fixture("init.mp4"),
            fixture("1.m4s"),
            fixture("2.m4s"),
            fixture("3.m4s"),
        ]);
        let decoder = rodio::Decoder::new(reader)
            .expect("concatenated DASH segments must be decodable");

        use rodio::Source;
        assert_eq!(
            decoder.sample_rate(),
            std::num::NonZero::new(44100).unwrap()
        );
        assert_eq!(decoder.count(), 529_200);
    }
}
