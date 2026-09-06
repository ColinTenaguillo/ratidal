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
}

impl Default for AuthConfig {
    fn default() -> Self {
        // Shipped so first run needs no setup. Overridable because TIDAL caps
        // client_ids that attract traffic — when that happens the user edits
        // this field instead of waiting for a release.
        Self {
            client_id: "fX2JxdmntZWK0ixT".into(),
            client_secret: "GZ9ov5PjPZrmzDbRxIrNAJZ7Fnkl5Km3rEbUdBEC".into(),
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        // Never lower this. Requesting LOSSLESS on the default client_id
        // returns HIGH (AAC), not FLAC.
        Self { quality: "HI_RES_LOSSLESS".into() }
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
