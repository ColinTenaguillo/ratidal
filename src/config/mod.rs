use std::path::PathBuf;

pub mod paths {
    use std::path::PathBuf;

    fn project() -> Option<directories::ProjectDirs> {
        directories::ProjectDirs::from("", "", "ratidal")
    }

    pub fn config_file() -> Option<PathBuf> {
        project().map(|p| p.config_dir().join("config.toml"))
    }

    /// The session token, kept beside the config rather than in a separate
    /// data directory. Two locations for a handful of files is a thing to
    /// explain and a thing to get wrong; one is neither.
    pub fn token_file() -> Option<PathBuf> {
        project().map(|p| p.config_dir().join("token.json"))
    }

    /// Where the token used to live. Only for migrating it forward on the
    /// first run after the move — a user who is signed in should not be
    /// signed out by a change to where a file sits.
    pub fn legacy_token_file() -> Option<PathBuf> {
        project().map(|p| p.data_dir().join("token.json"))
    }

    pub fn cache_dir() -> Option<PathBuf> {
        project().map(|p| p.cache_dir().to_path_buf())
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    pub auth: AuthConfig,
    pub audio: AudioConfig,
    pub ui: UiConfig,
    pub playback: PlaybackConfig,
    /// Rebound keys, as `action = "key"`. Empty is the normal case, and the
    /// defaults are in `shell::keymap::ACTIONS`.
    #[serde(default)]
    pub keys: std::collections::HashMap<String, String>,
}

/// How the interface is drawn.
/// What may be played, and what happens when the queue ends.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PlaybackConfig {
    /// Whether to keep playing something similar when the queue runs out.
    ///
    /// Off by default: an app that starts playing on its own after the last
    /// track is one the user has to go and stop.
    pub autoplay: bool,
    /// Whether tracks marked explicit can be played. On, as TIDAL has it --
    /// turning it off is a choice, not a default.
    pub explicit: bool,
    /// Whether tracks marked AI-generated can be played.
    pub ai: bool,
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            autoplay: false,
            explicit: true,
            ai: true,
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// Whether to use nerd-font glyphs for the icons.
    ///
    /// Off by default, and set by hand rather than detected. A terminal
    /// does not say what font it is using, and the glyphs cannot be
    /// measured either: nerd fonts set the x-advance of every glyph to one
    /// cell, which is also what a font without them advances when it draws
    /// a replacement box. lazygit, starship and yazi all ask rather than
    /// guess, for the same reason.
    ///
    /// The Settings row draws the glyphs beside the value, so the answer is
    /// one keypress away rather than a matter of detection.
    pub nerd_font: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub quality: String,
    /// Output level, 0.0 to 1.0. Applied to the sink rather than to the
    /// samples, so turning it down costs nothing in fidelity at 1.0.
    pub volume: f32,
}

impl Default for AuthConfig {
    fn default() -> Self {
        // These belong to one of TIDAL's own applications, not to this one:
        // full playback is granted only to their first-party clients, so
        // every third-party client sends a first-party `client_id` and TIDAL
        // believes it is talking to that application. It is a
        // misrepresentation, and it is here because there is no alternative
        // that works — a client registered on TIDAL's developer portal is
        // refused for the device flow ("Client is not a Limited Input Device
        // client") and capped at 30-second previews besides. The README says
        // so plainly; if TIDAL ever opens a path for third-party device
        // clients, this should be the first thing to go.
        //
        // Overridable because TIDAL caps client_ids that attract traffic --
        // when that happens the user edits this field rather than waiting for
        // a release.
        Self {
            client_id: "fX2JxdmntZWK0ixT".into(),
            client_secret: "GZ9ov5PjPZrmzDbRxIrNAJZ7Fnkl5Km3rEbUdBEC".into(),
        }
    }
}

impl AudioConfig {
    /// The configured quality, or the default when the file names one this
    /// build does not know.
    ///
    /// A typo must not silently drop the stream to AAC, and it must not stop
    /// the app either: an unreadable value falls back to the best tier and
    /// says so in the log.
    pub fn quality(&self) -> crate::domain::Quality {
        match self.quality.parse() {
            Ok(q) => q,
            Err(e) => {
                tracing::warn!("{e}; using the default");
                crate::domain::Quality::HiResLossless
            }
        }
    }

    pub fn set_quality(&mut self, q: crate::domain::Quality) {
        self.quality = q.as_param().to_string();
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        // Never lower this. Requesting LOSSLESS on the default client_id
        // returns HIGH (AAC), not FLAC.
        Self { quality: "HI_RES_LOSSLESS".into(), volume: 1.0 }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not determine a config directory for this platform")]
    NoConfigDir,
    #[error("reading {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("writing {path}: {source}")]
    Write { path: PathBuf, source: std::io::Error },
    #[error("{path} is not valid TOML: {source}")]
    Parse { path: PathBuf, source: toml::de::Error },
}

impl Config {
    /// Load the config, creating it with defaults on first run.
    pub fn load() -> Result<Self, ConfigError> {
        let path = paths::config_file().ok_or(ConfigError::NoConfigDir)?;

        if !path.exists() {
            let config = Self::default();
            let text = toml::to_string_pretty(&config)
                .expect("Config serializes; it has no maps with non-string keys");
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)
                    .map_err(|source| ConfigError::Write { path: path.clone(), source })?;
            }
            std::fs::write(&path, text)
                .map_err(|source| ConfigError::Write { path: path.clone(), source })?;
            return Ok(config);
        }

        let text = std::fs::read_to_string(&path)
            .map_err(|source| ConfigError::Read { path: path.clone(), source })?;
        toml::from_str(&text).map_err(|source| ConfigError::Parse { path, source })
    }

    /// Write the config back, so a setting changed in the app is still set
    /// on the next run.
    ///
    /// Written to a temporary file and renamed over the old one: a crash
    /// midway through writing would otherwise leave a truncated config,
    /// which is worse than the setting not sticking.
    pub fn save(&self) -> Result<(), ConfigError> {
        self.save_to(&paths::config_file().ok_or(ConfigError::NoConfigDir)?)
    }

    /// Write to a named path. What `save` does, with the location given
    /// rather than looked up — a test that wrote through `save` put its
    /// settings in the real config file and left this machine on LOW.
    pub fn save_to(&self, path: &std::path::Path) -> Result<(), ConfigError> {
        let path = path.to_path_buf();
        let text = toml::to_string_pretty(self)
            .expect("Config serializes; it has no maps with non-string keys");
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|source| ConfigError::Write { path: path.clone(), source })?;
        }
        let tmp = path.with_extension("toml.new");
        std::fs::write(&tmp, text)
            .map_err(|source| ConfigError::Write { path: tmp.clone(), source })?;
        std::fs::rename(&tmp, &path)
            .map_err(|source| ConfigError::Write { path: path.clone(), source })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_through_toml() {
        let c = Config::default();
        let text = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.auth.client_id, c.auth.client_id);
        assert_eq!(back.audio.quality, c.audio.quality);
    }

    #[test]
    fn default_quality_is_hires() {
        // Requesting anything lower silently yields AAC on the shipped
        // client_id — see the spec's §2.1.
        assert_eq!(Config::default().audio.quality, "HI_RES_LOSSLESS");
    }

    #[test]
    fn partial_config_fills_missing_fields_from_defaults() {
        // A user editing only client_id must not have to restate everything.
        let text = r#"
            [auth]
            client_id = "replaced"
            client_secret = "also-replaced"
        "#;
        let c: Config = toml::from_str(text).unwrap();
        assert_eq!(c.auth.client_id, "replaced");
        assert_eq!(c.audio.quality, "HI_RES_LOSSLESS");
    }
}
