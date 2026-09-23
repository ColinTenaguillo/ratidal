use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct StoredToken {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds. Compared against a clock passed in, never read directly,
    /// so this stays testable.
    pub expires_at: u64,
    pub country_code: String,
    pub user_id: u64,
}

impl Default for StoredToken {
    fn default() -> Self {
        Self {
            access_token: String::new(),
            refresh_token: String::new(),
            expires_at: 0,
            country_code: "US".into(),
            user_id: 0,
        }
    }
}

impl StoredToken {
    pub fn is_expired_at(&self, now_unix: u64) -> bool {
        // 60s of slack so a request does not race the boundary.
        self.expires_at.saturating_sub(60) <= now_unix
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("could not determine a data directory for this platform")]
    NoDataDir,
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not valid JSON: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
}

pub fn save_to(path: &Path, token: &StoredToken) -> Result<(), StoreError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|source| StoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    }
    let text = serde_json::to_string_pretty(token).map_err(|source| StoreError::Parse {
        path: path.to_path_buf(),
        source,
    })?;

    // The file holds a bearer token. Create it owner-only from the start:
    // writing first and chmod'ing after leaves a window where the token is
    // world-readable under a default umask.
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    use std::io::Write as _;
    file.write_all(text.as_bytes())
        .map_err(|source| StoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;

    // .mode() only applies when the file is created; if an older file exists
    // with looser permissions, truncate reuses its mode. So also chmod here
    // to normalise a pre-existing file.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(
            |source| StoreError::Io {
                path: path.to_path_buf(),
                source,
            },
        )?;
    }
    Ok(())
}

pub fn load_from(path: &Path) -> Result<Option<StoredToken>, StoreError> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|source| StoreError::Parse {
            path: path.to_path_buf(),
            source,
        })
}

pub fn save(token: &StoredToken) -> Result<(), StoreError> {
    let path = crate::config::paths::token_file().ok_or(StoreError::NoDataDir)?;
    save_to(&path, token)
}

pub fn load() -> Result<Option<StoredToken>, StoreError> {
    let path = crate::config::paths::token_file().ok_or(StoreError::NoDataDir)?;
    if let Some(token) = load_from(&path)? {
        return Ok(Some(token));
    }

    // The token used to live in the data directory. Move it rather than making
    // an already-signed-in user sign in again because a file moved.
    let Some(legacy) = crate::config::paths::legacy_token_file() else {
        return Ok(None);
    };
    match load_from(&legacy)? {
        Some(token) => {
            tracing::info!("moving the stored session beside the config");
            save_to(&path, &token)?;
            if let Err(e) = std::fs::remove_file(&legacy) {
                // Not fatal: the new copy is the one that will be read.
                tracing::warn!("could not remove the old token file: {e}");
            }
            Ok(Some(token))
        }
        None => Ok(None),
    }
}

/// Remove the stored session.
///
/// Logs the removal itself rather than leaving each caller to. A token that
/// vanishes with nothing in the log to say who removed it costs rounds of
/// guessing, and this has already cost several.
pub fn clear() -> Result<(), StoreError> {
    let path = crate::config::paths::token_file().ok_or(StoreError::NoDataDir)?;
    clear_at(&path)
}

/// Remove the session at a given path.
///
/// Every other operation here takes a path (`save_to`, `load_from`) and only
/// `clear` did not — so a test that exercised the sign-out path deleted the
/// developer's own session, on every run of the suite. That cost hours of
/// hunting for something external that was deleting the file.
pub fn clear_at(path: &Path) -> Result<(), StoreError> {
    if path.exists() {
        tracing::warn!("deleting the stored session at {}", path.display());
        std::fs::remove_file(path).map_err(|source| StoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    } else {
        tracing::info!("asked to delete the stored session, but there is none");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StoredToken {
        StoredToken {
            access_token: "at".into(),
            refresh_token: "rt".into(),
            expires_at: 1_800_000_000,
            country_code: "TH".into(),
            user_id: 10_000_001,
        }
    }

    /// A directory unique to one test. Tests run in parallel, and two sharing
    /// a path race — one calls `remove_dir_all` while the other is mid-write.
    fn test_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ratidal-test-{name}"))
    }

    #[test]
    fn round_trips_through_a_file() {
        let dir = test_dir("round-trip");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("token.json");

        save_to(&path, &sample()).unwrap();
        let back = load_from(&path).unwrap().unwrap();
        assert_eq!(back.access_token, "at");
        assert_eq!(back.country_code, "TH");
    }

    #[test]
    fn missing_file_is_none_not_an_error() {
        let path = test_dir("absent").join("absent.json");
        let _ = std::fs::remove_file(&path);
        assert!(load_from(&path).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn token_file_is_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = test_dir("perms");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("token.json");

        save_to(&path, &sample()).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o077,
            0,
            "token file must not be group/world readable"
        );
    }

    #[cfg(unix)]
    #[test]
    fn token_file_normalises_pre_existing_loose_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = test_dir("perms-existing");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("token.json");

        // Pre-create the file with loose permissions.
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        // save_to must fix it to 0o600 even though the file already exists.
        save_to(&path, &sample()).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o077,
            0,
            "token file must be fixed to 0o600 even if pre-existing"
        );
    }

    #[test]
    fn the_token_and_the_config_live_together() {
        // Two directories for a handful of files is a thing to explain and a
        // thing to get wrong. If these ever diverge again, say why in a
        // comment rather than letting it happen quietly.
        let token = crate::config::paths::token_file().expect("token path");
        let config = crate::config::paths::config_file().expect("config path");
        assert_eq!(
            token.parent(),
            config.parent(),
            "the token belongs beside the config"
        );
    }

    #[test]
    fn no_test_reaches_for_the_real_session() {
        // The suite used to delete the developer's own token on every run,
        // through a test that exercised the sign-out path. The sign-outs it
        // caused were blamed on the app, on macOS, on an antivirus — on
        // everything except the tests — for hours.
        //
        // Every path-free entry point here (`save`, `load`, `clear`) reads or
        // writes the real session, so no test may name one. The path-taking
        // forms are what tests are for.
        let sources = [
            ("auth/store.rs", include_str!("store.rs")),
            ("shell/mod.rs", include_str!("../shell/mod.rs")),
            ("library/mod.rs", include_str!("../library/mod.rs")),
        ];
        for (name, source) in sources {
            let Some(tests) = source.split("mod tests {").nth(1) else {
                continue;
            };
            for forbidden in ["store::clear()", "store::save(", "store::load()"] {
                assert!(
                    !tests.contains(forbidden),
                    "{name}'s tests call {forbidden}, which touches the real session; \
                     use the path-taking form instead"
                );
            }
        }
    }

    #[test]
    fn expiry_is_reported_against_a_supplied_clock() {
        let t = sample();
        assert!(!t.is_expired_at(1_700_000_000));
        assert!(t.is_expired_at(1_900_000_000));
    }

    #[test]
    fn the_slack_is_sixty_seconds_before_the_boundary() {
        // A request must not race the expiry, so a token counts as expired
        // a minute early. Nothing exercised either side of that minute.
        let mut t = sample();
        t.expires_at = 1_000_000;
        assert!(
            !t.is_expired_at(1_000_000 - 61),
            "more than a minute to go: still good"
        );
        assert!(
            t.is_expired_at(1_000_000 - 60),
            "a minute out it is already spent"
        );
        assert!(t.is_expired_at(1_000_000), "and at the boundary itself");
    }

    #[test]
    fn a_token_expiring_inside_the_first_minute_does_not_wrap() {
        // `expires_at` under sixty subtracts below zero. Wrapping there
        // gives an enormous number and the token reads as valid for ever —
        // which no test noticed, since none used a small clock.
        let mut t = sample();
        t.expires_at = 30;
        assert!(
            t.is_expired_at(31),
            "a token that expired at 30 is expired at 31"
        );
        assert!(
            t.is_expired_at(0),
            "and at the epoch, being inside the slack"
        );
    }
}
