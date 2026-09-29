//! The history, kept between runs.
//!
//! A track heard yesterday is one the user may want to name today, and a
//! history that starts blank on every launch cannot answer. Plain JSON in
//! the cache directory: small, readable, and nothing to migrate.

use std::path::Path;

use crate::domain::Track;

/// What the file holds, newest first, or nothing when there is no file or
/// it cannot be read. A history that does not load is not worth a dialog.
pub fn load_history(path: &Path) -> Vec<Track> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    match serde_json::from_str(&text) {
        Ok(tracks) => tracks,
        Err(e) => {
            tracing::warn!("ignoring the history at {}: {e}", path.display());
            Vec::new()
        }
    }
}

/// Write the whole history, newest first. Written beside the file and
/// renamed over it, so a crash mid-write leaves the old one whole.
pub fn save_history<'a>(path: &Path, tracks: impl Iterator<Item = &'a Track>) {
    let tracks: Vec<&Track> = tracks.collect();
    let text = match serde_json::to_string(&tracks) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("could not encode the history: {e}");
            return;
        }
    };
    let staging = path.with_extension("json.tmp");
    let written = std::fs::write(&staging, text).and_then(|()| std::fs::rename(&staging, path));
    if let Err(e) = written {
        tracing::warn!("could not save the history to {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ratidal-history-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("history.json")
    }

    #[test]
    fn what_is_saved_comes_back_in_the_same_order() {
        let path = scratch("round-trip");
        let played = [
            Track::sample("Now", "Band", Duration::from_secs(200)),
            Track::sample("Earlier", "Band", Duration::from_secs(100)),
        ];
        save_history(&path, played.iter());
        let back = load_history(&path);
        let titles: Vec<&str> = back.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["Now", "Earlier"]);
        assert_eq!(back[0].duration, Duration::from_secs(200), "the fields survive");
        assert!(!path.with_extension("json.tmp").exists(), "the staging file is gone");
    }

    #[test]
    fn no_file_and_a_broken_file_both_load_as_nothing() {
        let path = scratch("broken");
        assert!(load_history(&path).is_empty(), "nothing saved yet");
        std::fs::write(&path, "not json").unwrap();
        assert!(load_history(&path).is_empty(), "a broken file is skipped, not fatal");
    }
}
