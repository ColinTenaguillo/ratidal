//! Scrobbling: telling Last.fm, Libre.fm, ListenBrainz or Maloja what played.
//!
//! The services and their protocols come from sonora's `music` crate. This
//! links the ones the config names, keeps their accounts in a file beside
//! the token, turns a track into a listen, and sends what the shell puts in
//! the outbox. Whether a listen counts is the services' own rule, and lives
//! with them: thirty seconds at least, and half the track or four minutes.
//!
//! ponytail: a submission that fails is logged and dropped. A queue kept
//! for the network coming back is the next step, if listens go missing.

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::SystemTime;

use music::scrobble::{Account, Play, Secret, Service};

use crate::config::ScrobbleConfig;
use crate::domain::Track;

/// A service with the account it is linked to.
#[derive(Clone)]
pub struct Linked {
    pub service: Arc<dyn Service>,
    pub account: Account,
}

impl std::fmt::Debug for Linked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} as {}", self.service.id(), self.account.name)
    }
}

impl Linked {
    /// What the status line calls the account: the service's name, or the
    /// server's for one the user hosts.
    pub fn label(&self) -> String {
        match self.service.id() {
            "lastfm" => "Last.fm".to_string(),
            "librefm" => "Libre.fm".to_string(),
            "listenbrainz" => "ListenBrainz".to_string(),
            _ if !self.account.name.is_empty() => self.account.name.clone(),
            other => other.to_string(),
        }
    }
}

/// What the config gives a service to link with, or nothing when it does
/// not name the service.
pub fn secret_for(config: &ScrobbleConfig, id: &str) -> Option<Secret> {
    match id {
        "lastfm" => config.lastfm.as_ref().map(|keys| Secret::Keys {
            key: keys.key.clone(),
            secret: keys.secret.clone(),
        }),
        "librefm" => config.librefm.then_some(Secret::None),
        "listenbrainz" => config
            .listenbrainz
            .as_ref()
            .filter(|t| !t.trim().is_empty())
            .map(|t| Secret::Token(t.clone())),
        "maloja" => config.maloja.as_ref().map(|m| Secret::Server {
            url: m.url.clone(),
            key: m.key.clone(),
        }),
        _ => None,
    }
}

/// Whether the config names any service at all, so a run without one
/// never opens the account file or says anything about scrobbling.
pub fn configured(config: &ScrobbleConfig) -> bool {
    music::scrobble::services()
        .iter()
        .any(|s| secret_for(config, s.id()).is_some())
}

/// The services the config names, linked: from the account file where one
/// is kept, by `connect` otherwise -- which for Last.fm and Libre.fm opens
/// the browser and waits for the approval, one after the other, since both
/// listen on the same port for it. The accounts are written back.
pub async fn link(config: &ScrobbleConfig, file: Option<&Path>) -> Vec<Linked> {
    let stored = file.map(load).unwrap_or_default();
    let mut linked = Vec::new();
    for service in music::scrobble::services() {
        let Some(secret) = secret_for(config, service.id()) else {
            continue;
        };
        if let Some(account) = stored.get(service.id()).filter(|a| a.linked()) {
            linked.push(Linked {
                service,
                account: account.clone(),
            });
            continue;
        }
        match service.connect(secret).await {
            Ok(account) => {
                tracing::info!("scrobbling: linked {} as {}", service.id(), account.name);
                linked.push(Linked { service, account });
            }
            Err(error) => {
                tracing::warn!("scrobbling: could not link {}: {error:#}", service.id())
            }
        }
    }
    if let Some(file) = file {
        if let Err(error) = save(file, &linked) {
            tracing::warn!("scrobbling: could not keep the accounts: {error:#}");
        }
    }
    linked
}

/// The accounts kept from the last run, by service id.
fn load(file: &Path) -> HashMap<String, Account> {
    std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The accounts, readable by the owner alone: a session key is a credential.
fn save(file: &Path, linked: &[Linked]) -> anyhow::Result<()> {
    let accounts: HashMap<&str, &Account> = linked
        .iter()
        .map(|l| (l.service.id(), &l.account))
        .collect();
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(&accounts)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write as _;
    options.open(file)?.write_all(text.as_bytes())?;
    Ok(())
}

/// A track as a listen that started `at`. A track with no artist or no
/// title is nothing a service can file, so it is not one.
pub fn play_of(track: &Track, at: SystemTime) -> Option<Play> {
    if track.artist.trim().is_empty() || track.title.trim().is_empty() {
        return None;
    }
    Some(Play {
        artist: track.artist.clone(),
        title: track.title.clone(),
        album: (!track.album.trim().is_empty()).then(|| track.album.clone()),
        duration: track.duration,
        at,
    })
}

/// What the shell has decided and not yet sent: the track that just
/// started, and the listens that earned a submission.
#[derive(Debug, Default)]
pub struct Outbox {
    pub now_playing: Option<Play>,
    pub finished: Vec<Play>,
}

type Sending = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Everything in the outbox, as one request per service, and the outbox
/// emptied. The caller runs them: spawned while the app runs, awaited on
/// the way out.
pub fn drain(linked: &[Linked], outbox: &mut Outbox) -> Vec<Sending> {
    let mut sending: Vec<Sending> = Vec::new();
    if let Some(play) = outbox.now_playing.take() {
        for l in linked.iter().filter(|l| l.account.enabled).cloned() {
            let play = play.clone();
            sending.push(Box::pin(async move {
                if let Err(error) = l.service.now_playing(&l.account, &play).await {
                    tracing::info!("scrobbling: {} now playing failed: {error:#}", l.service.id());
                }
            }));
        }
    }
    let plays: Vec<Play> = std::mem::take(&mut outbox.finished);
    if !plays.is_empty() {
        for l in linked.iter().filter(|l| l.account.enabled).cloned() {
            let plays = plays.clone();
            sending.push(Box::pin(async move {
                match l.service.scrobble(&l.account, &plays).await {
                    Ok(()) => tracing::info!(
                        "scrobbled {} to {}",
                        plays.len(),
                        l.service.id()
                    ),
                    Err(error) => {
                        tracing::warn!("scrobbling: {} refused: {error:#}", l.service.id())
                    }
                }
            }));
        }
    }
    sending
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::config::{ApiKeys, MalojaConfig};

    #[test]
    fn the_config_gives_each_service_what_it_links_with() {
        let config = ScrobbleConfig {
            lastfm: Some(ApiKeys {
                key: "k".into(),
                secret: "s".into(),
            }),
            librefm: true,
            listenbrainz: Some("  ".into()),
            maloja: Some(MalojaConfig {
                url: "https://m".into(),
                key: "mk".into(),
            }),
        };
        assert!(matches!(secret_for(&config, "lastfm"), Some(Secret::Keys { key, secret }) if key == "k" && secret == "s"));
        assert!(matches!(secret_for(&config, "librefm"), Some(Secret::None)));
        assert!(secret_for(&config, "listenbrainz").is_none(), "a blank token names nothing");
        assert!(matches!(secret_for(&config, "maloja"), Some(Secret::Server { url, .. }) if url == "https://m"));
        assert!(secret_for(&ScrobbleConfig::default(), "lastfm").is_none());
    }

    #[test]
    fn a_track_is_a_listen_only_with_an_artist_and_a_title() {
        let mut track = Track::sample("Song", "Someone", Duration::from_secs(200));
        track.album = String::new();
        let play = play_of(&track, SystemTime::UNIX_EPOCH).expect("a listen");
        assert_eq!((play.artist.as_str(), play.title.as_str()), ("Someone", "Song"));
        assert!(play.album.is_none(), "an empty album is no album");
        assert!(play.earned(Duration::from_secs(100)), "half of it");
        assert!(!play.earned(Duration::from_secs(99)));

        track.artist = String::new();
        assert!(play_of(&track, SystemTime::UNIX_EPOCH).is_none());
    }

    #[test]
    fn the_outbox_is_emptied_into_one_request_per_service_and_kind() {
        let service = music::scrobble::services().remove(2);
        let linked = Linked {
            service,
            account: Account {
                session: "t".into(),
                enabled: true,
                ..Account::default()
            },
        };
        let play = play_of(
            &Track::sample("Song", "Someone", Duration::from_secs(200)),
            SystemTime::UNIX_EPOCH,
        )
        .expect("a listen");
        let mut outbox = Outbox {
            now_playing: Some(play.clone()),
            finished: vec![play.clone(), play],
        };
        let sending = drain(std::slice::from_ref(&linked), &mut outbox);
        assert_eq!(sending.len(), 2, "one now-playing, one batch of listens");
        assert!(outbox.now_playing.is_none() && outbox.finished.is_empty());

        let off = Linked {
            account: Account {
                enabled: false,
                ..linked.account.clone()
            },
            ..linked
        };
        outbox.finished.push(play_of(&Track::sample("S", "A", Duration::from_secs(60)), SystemTime::UNIX_EPOCH).unwrap());
        assert!(drain(&[off], &mut outbox).is_empty(), "a switched-off account sends nothing");
        assert_eq!(linked_label(), "ListenBrainz");
    }

    fn linked_label() -> String {
        Linked {
            service: music::scrobble::services().remove(2),
            account: Account::default(),
        }
        .label()
    }
}
