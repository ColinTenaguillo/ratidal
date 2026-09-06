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
    pub duration: Duration,
    pub cover: Option<String>,
    pub tags: Vec<String>,
}

impl Track {
    /// True when TIDAL flags this track as available in hi-res.
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
}
