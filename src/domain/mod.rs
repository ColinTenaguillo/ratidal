use std::str::FromStr;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize)]
pub struct TrackId(pub u64);

impl std::fmt::Display for TrackId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality { HiResLossless, Lossless, High, Low }

impl Quality {
    pub fn as_param(&self) -> &'static str {
        match self {
            Quality::HiResLossless => "HI_RES_LOSSLESS",
            Quality::Lossless      => "LOSSLESS",
            Quality::High          => "HIGH",
            Quality::Low           => "LOW",
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown audio quality: {0}")]
pub struct UnknownQuality(String);

impl FromStr for Quality {
    type Err = UnknownQuality;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "HI_RES_LOSSLESS" => Ok(Quality::HiResLossless),
            "LOSSLESS"        => Ok(Quality::Lossless),
            "HIGH"            => Ok(Quality::High),
            "LOW"             => Ok(Quality::Low),
            other             => Err(UnknownQuality(other.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Track {
    pub id: TrackId,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: Duration,
    pub cover: Option<String>,
    pub tags: Vec<String>,
    /// When the user added this to their favourites, ISO 8601. Only
    /// favourites carry it; a track reached through an album or playlist has
    /// none.
    pub added: Option<String>,
    /// TIDAL's explicit-content flag, shown as the E badge.
    pub explicit: bool,
    /// TIDAL's AI-generated flag. The API sends `ai` on every track; it was
    /// false on all 159 in the captured responses, so the filter that reads
    /// it has never been seen to fire against real content.
    pub ai: bool,
    /// The radio that continues from this track, when the API named one.
    /// What autoplay follows when the queue runs out.
    pub radio: Option<String>,
    /// The album this track is on, so a key can open it. The name alone
    /// was carried for years and could only ever be printed.
    pub album_id: Option<u64>,
    /// The first credited artist, for the same reason. TIDAL lists several
    /// on a collaboration; the first is the one whose page the web opens.
    pub artist_id: Option<u64>,
}

impl Track {
    /// A track with only the fields a caller cares about set. Tests and
    /// previews want a title and an artist, not eight fields of ceremony.
    pub fn sample(title: &str, artist: &str, duration: Duration) -> Self {
        Self {
            id: TrackId(0),
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            duration,
            cover: None,
            tags: Vec::new(),
            added: None,
            explicit: false,
            ai: false,
            radio: None,
            album_id: None,
            artist_id: None,
        }
    }

    /// Whether this track may be played, given what the user allows.
    ///
    /// TIDAL's own settings, and its own wording: a blocked track is not
    /// hidden, it is shown greyed out and refuses to start. Hiding it would
    /// leave holes in an album with no way to tell why.
    pub fn allowed(&self, allow_explicit: bool, allow_ai: bool) -> bool {
        (allow_explicit || !self.explicit) && (allow_ai || !self.ai)
    }

    pub fn is_hires(&self) -> bool {
        self.tags.iter().any(|t| t == "HIRES_LOSSLESS")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_maps_to_api_param() {
        assert_eq!(Quality::HiResLossless.as_param(), "HI_RES_LOSSLESS");
        assert_eq!(Quality::Lossless.as_param(), "LOSSLESS");
        assert_eq!(Quality::High.as_param(), "HIGH");
        assert_eq!(Quality::Low.as_param(), "LOW");
    }

    #[test]
    fn quality_parses_from_api_response() {
        // The API echoes back what it actually delivered, which may be lower
        // than requested — that value must round-trip.
        assert_eq!("HI_RES_LOSSLESS".parse::<Quality>().unwrap(), Quality::HiResLossless);
        assert_eq!("HIGH".parse::<Quality>().unwrap(), Quality::High);
        assert!("NONSENSE".parse::<Quality>().is_err());
    }
    #[test]
    fn a_track_is_blocked_only_by_a_flag_the_user_turned_off() {
        let flagged = |explicit, ai| Track {
            explicit,
            ai,
            ..Track::sample("A Track", "Someone", std::time::Duration::from_secs(200))
        };

        // Everything allowed: nothing is blocked, whatever it carries.
        assert!(flagged(true, true).allowed(true, true));
        assert!(flagged(false, false).allowed(true, true));

        // Each flag blocks only its own kind.
        assert!(!flagged(true, false).allowed(false, true), "explicit is off");
        assert!(flagged(false, true).allowed(false, true), "but this is not explicit");
        assert!(!flagged(false, true).allowed(true, false), "AI is off");
        assert!(flagged(true, false).allowed(true, false), "but this is not AI");

        // A track carrying both needs both allowed.
        assert!(!flagged(true, true).allowed(true, false));
        assert!(!flagged(true, true).allowed(false, true));
        assert!(flagged(true, true).allowed(true, true));
    }

}