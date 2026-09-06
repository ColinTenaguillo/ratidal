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
    Io { path: PathBuf, source: std::io::Error },
    #[error("{path} is not valid JSON: {source}")]
    Parse { path: PathBuf, source: serde_json::Error },
}

pub fn save_to(path: &Path, token: &StoredToken) -> Result<(), StoreError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|source| StoreError::Io { path: path.to_path_buf(), source })?;
    }
    let text = serde_json::to_string_pretty(token)
        .map_err(|source| StoreError::Parse { path: path.to_path_buf(), source })?;

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
    let mut file = options
        .open(path)
        .map_err(|source| StoreError::Io { path: path.to_path_buf(), source })?;

    use std::io::Write as _;
    file.write_all(text.as_bytes())
        .map_err(|source| StoreError::Io { path: path.to_path_buf(), source })?;

    // .mode() only applies when the file is created; if an older file exists
    // with looser permissions, truncate reuses its mode. So also chmod here
    // to normalise a pre-existing file.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|source| StoreError::Io { path: path.to_path_buf(), source })?;
    }
    Ok(())
}

pub fn load_from(path: &Path) -> Result<Option<StoredToken>, StoreError> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)
        .map_err(|source| StoreError::Io { path: path.to_path_buf(), source })?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|source| StoreError::Parse { path: path.to_path_buf(), source })
}

pub fn save(token: &StoredToken) -> Result<(), StoreError> {
    let path = crate::config::paths::token_file().ok_or(StoreError::NoDataDir)?;
    save_to(&path, token)
}

pub fn load() -> Result<Option<StoredToken>, StoreError> {
    let path = crate::config::paths::token_file().ok_or(StoreError::NoDataDir)?;
    load_from(&path)
}

pub fn clear() -> Result<(), StoreError> {
    let path = crate::config::paths::token_file().ok_or(StoreError::NoDataDir)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|source| StoreError::Io { path, source })?;
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
        assert_eq!(mode & 0o077, 0, "token file must not be group/world readable");
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
        assert_eq!(mode & 0o077, 0, "token file must be fixed to 0o600 even if pre-existing");
    }

    #[test]
    fn expiry_is_reported_against_a_supplied_clock() {
        let t = sample();
        assert!(!t.is_expired_at(1_700_000_000));
        assert!(t.is_expired_at(1_900_000_000));
    }
}
