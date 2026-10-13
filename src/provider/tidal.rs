//! TIDAL behind sonora's `MusicProvider`, `MusicApi` and `PlaybackFactory`.
//!
//! Signing in is the device flow this crate already runs: a code and a
//! link, confirmed in a browser, which is exactly what `SignInPrompt::Code`
//! describes. The catalogue is `library`, `browse` and `search`, mapped in
//! `map.rs`. The sound is `playback::fetch`, started by the engine.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context as _, Result};
use async_trait::async_trait;
use music::{
    Album, AlbumDetail, Artist, ArtistCatalogue, ArtistProfile, Capabilities, GenreDetail,
    HomeFeed, InputSource, MediaKind, MusicApi, MusicProvider, PlaybackConfig, PlaybackEvents,
    PlaybackFactory, Player, Playlist, PlaylistDetail, PromptSink, ProviderSession, SavedArtist,
    Shape, SignIn, SignInFailure, SignInProblem, SignInPrompt, Track, UserProfile,
};

use crate::auth::{self, store, PollOutcome, StoredToken};
use crate::browse::{self, Tab};
use crate::config::AuthConfig;
use crate::domain::{Quality, TrackId};
use crate::library;
use crate::playback::fetch::{Session, TidalFetch};
use crate::provider::map;
use crate::search;
use crate::tidal::Client;

struct Inner {
    auth: AuthConfig,
    /// The quality asked of every stream. The response says what came.
    wanted: Quality,
    http: reqwest::Client,
    token: Mutex<Option<StoredToken>>,
    fetch: TidalFetch,
    /// Set by `abandon` while a sign-in waits on the browser.
    abandoned: AtomicBool,
}

/// A poisoned lock holds nothing worse than a stale token, so it is taken
/// anyway rather than panicking.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A TIDAL id, which sonora carried as a string.
fn num(id: &str) -> Result<u64> {
    id.parse().map_err(|_| anyhow!("not a TIDAL id: {id}"))
}

impl Inner {
    /// Takes the token on, so every request and every stream sees it.
    fn adopt(&self, token: StoredToken) {
        self.fetch.set_session(Session {
            token: token.clone(),
            wanted: self.wanted,
        });
        *lock(&self.token) = Some(token);
    }

    /// A client on the current token, refreshed first when it has run out.
    /// Refresh tokens outlive access tokens by days, so this is what gets
    /// an overnight session back without a browser.
    async fn client(&self) -> Result<Client> {
        let token = lock(&self.token)
            .clone()
            .ok_or(SignInFailure(SignInProblem::Credentials))?;
        if !token.is_expired_at(now_unix()) {
            return Ok(Client::new(token));
        }
        let fresh = auth::refresh(&self.http, &self.auth, &token.refresh_token)
            .await
            .context("the TIDAL session could not be refreshed")?;
        if let Err(error) = store::save(&fresh) {
            tracing::warn!("could not persist the refreshed token: {error}");
        }
        self.adopt(fresh.clone());
        Ok(Client::new(fresh))
    }

    fn profile(&self) -> UserProfile {
        let id = lock(&self.token)
            .as_ref()
            .map(|t| t.user_id.to_string())
            .unwrap_or_default();
        UserProfile {
            display_name: format!("TIDAL {id}"),
            id,
            avatar: None,
        }
    }

    fn session(self: &Arc<Self>) -> ProviderSession {
        ProviderSession {
            profile: self.profile(),
            api: Arc::new(TidalApi {
                inner: Arc::clone(self),
            }),
            playback: Arc::new(TidalPlayback {
                fetch: self.fetch.clone(),
            }),
            shape: Shape::Saved,
            authenticated: true,
            capabilities: Capabilities {
                follow_artists: true,
                radio: true,
                playcounts: false,
                library: false,
                pins: false,
            },
        }
    }

    /// The tracks behind a playlist id, which is a mix when prefixed.
    async fn list_tracks(&self, id: &str) -> Result<Vec<Track>> {
        let client = self.client().await?;
        let tracks = match id.strip_prefix(map::MIX) {
            Some(mix) => library::mix_tracks(&client, mix).await?,
            None => library::playlist_tracks(&client, id).await?,
        };
        Ok(tracks.into_iter().map(map::track).collect())
    }
}

pub struct TidalProvider {
    inner: Arc<Inner>,
}

impl TidalProvider {
    pub fn new(auth: AuthConfig, wanted: Quality) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(Inner {
                auth,
                wanted,
                http: reqwest::Client::new(),
                token: Mutex::new(None),
                fetch: TidalFetch::new()?,
                abandoned: AtomicBool::new(false),
            }),
        })
    }

    /// From the user's config file: their credentials and quality setting.
    pub fn from_config() -> Result<Self> {
        let config = crate::config::Config::load()?;
        let wanted = config.audio.quality();
        Self::new(config.auth, wanted)
    }

    /// Runs the device flow: shows the code, then polls until the browser
    /// confirms it, the code expires, or `abandon` is called.
    async fn device_flow(&self, prompt: PromptSink) -> Result<StoredToken> {
        let inner = &self.inner;
        inner.abandoned.store(false, Ordering::SeqCst);
        let code = auth::start_login(&inner.http, &inner.auth)
            .await
            .map_err(|error| match error.is_refusal() {
                true => anyhow!(SignInFailure(SignInProblem::Refused)),
                false => anyhow!(SignInFailure(SignInProblem::Network)),
            })?;
        prompt(SignInPrompt::Code {
            code: code.user_code.clone(),
            url: code.verification_uri.clone(),
        });
        let deadline = Instant::now() + Duration::from_secs(code.expires_in_secs.max(1));
        let mut interval = Duration::from_secs(code.interval_secs.max(1));
        loop {
            tokio::time::sleep(interval).await;
            if inner.abandoned.load(Ordering::SeqCst) {
                bail!(SignInFailure(SignInProblem::Cancelled));
            }
            if Instant::now() >= deadline {
                bail!(SignInFailure(SignInProblem::Cancelled));
            }
            match auth::poll_once(&inner.http, &inner.auth, &code).await {
                Ok(PollOutcome::Granted(token)) => return Ok(token),
                Ok(PollOutcome::Pending) => {}
                // What the spec says to do, and what TIDAL expects.
                Ok(PollOutcome::SlowDown) => interval += Duration::from_secs(5),
                Ok(PollOutcome::Expired) => bail!(SignInFailure(SignInProblem::Cancelled)),
                Err(error) if error.is_refusal() => {
                    bail!(SignInFailure(SignInProblem::Refused))
                }
                // A blip on the network is not an answer; the next poll is.
                Err(error) => tracing::warn!("polling the device code: {error}"),
            }
        }
    }
}

#[async_trait]
impl MusicProvider for TidalProvider {
    fn name(&self) -> &'static str {
        "TIDAL"
    }

    fn slug(&self) -> &'static str {
        "tidal"
    }

    fn sign_in_options(&self) -> Vec<SignIn> {
        vec![SignIn::Default]
    }

    fn stored(&self) -> bool {
        matches!(store::load(), Ok(Some(_)))
    }

    fn reach(&self) -> Option<String> {
        Some("api.tidal.com".into())
    }

    fn public_art(&self) -> bool {
        true
    }

    async fn restore(&self) -> Result<Option<ProviderSession>> {
        let Some(token) = store::load()? else {
            return Ok(None);
        };
        self.inner.adopt(token);
        // Refreshed now rather than on the first request, so a dead refresh
        // token sends the user to the sign-in screen instead of failing
        // every page.
        match self.inner.client().await {
            Ok(_) => Ok(Some(self.inner.session())),
            Err(error) => {
                let refused = error
                    .downcast_ref::<auth::AuthError>()
                    .is_some_and(auth::AuthError::is_refusal);
                if refused {
                    tracing::warn!("the stored TIDAL session was refused; forgetting it");
                    self.sign_out();
                    return Ok(None);
                }
                Err(error)
            }
        }
    }

    async fn sign_in(
        &self,
        method: SignIn,
        prompt: PromptSink,
        _input: InputSource,
    ) -> Result<ProviderSession> {
        if method != SignIn::Default {
            bail!("TIDAL signs in with a code confirmed in the browser");
        }
        let token = self.device_flow(prompt).await?;
        if let Err(error) = store::save(&token) {
            tracing::warn!("could not persist the TIDAL token: {error}");
        }
        self.inner.adopt(token);
        Ok(self.inner.session())
    }

    fn abandon(&self) {
        self.inner.abandoned.store(true, Ordering::SeqCst);
    }

    fn sign_out(&self) {
        *lock(&self.inner.token) = None;
        if let Err(error) = store::clear() {
            tracing::warn!("could not clear the TIDAL token: {error}");
        }
    }
}

struct TidalPlayback {
    fetch: TidalFetch,
}

impl PlaybackFactory for TidalPlayback {
    fn start(&self, config: PlaybackConfig) -> (Box<dyn Player>, Box<dyn PlaybackEvents>) {
        music::engine::start(self.fetch.clone(), config)
    }
}

struct TidalApi {
    inner: Arc<Inner>,
}

#[async_trait]
impl MusicApi for TidalApi {
    fn share_url(&self, kind: MediaKind, id: &str) -> Option<String> {
        let kind = match kind {
            MediaKind::Track => "track",
            MediaKind::Album => "album",
            MediaKind::Artist => "artist",
            MediaKind::Playlist => match id.starts_with(map::MIX) {
                true => return Some(format!("https://tidal.com/browse/mix/{}", &id[map::MIX.len()..])),
                false => "playlist",
            },
        };
        Some(format!("https://tidal.com/browse/{kind}/{id}"))
    }

    async fn profile(&self) -> Result<UserProfile> {
        Ok(self.inner.profile())
    }

    async fn artist(&self, artist_id: &str) -> Result<Artist> {
        let client = self.inner.client().await?;
        let page = library::artist_page(&client, num(artist_id)?).await?;
        Ok(Artist {
            name: page.name,
            cover_large: page.picture,
            biography: page.bio,
            monthly_listeners: page.fans,
            top_tracks: page.top_tracks.into_iter().map(map::track).collect(),
            albums: page
                .albums
                .into_iter()
                .chain(page.singles)
                .map(map::album)
                .collect(),
        })
    }

    async fn artist_catalogue(&self, artist_id: &str, _known: &[Track]) -> Result<ArtistCatalogue> {
        let client = self.inner.client().await?;
        let page = library::artist_page(&client, num(artist_id)?).await?;
        Ok(ArtistCatalogue {
            albums: Vec::new(),
            top_tracks: Vec::new(),
            appears_on: page.appears_on.into_iter().map(map::album).collect(),
        })
    }

    async fn artist_profile(&self, artist_id: &str) -> Result<ArtistProfile> {
        let client = self.inner.client().await?;
        let page = library::artist_page(&client, num(artist_id)?).await?;
        Ok(ArtistProfile {
            name: page.name,
            cover_large: page.picture,
            biography: page.bio,
        })
    }

    /// Every artist this hands out already carries its picture.
    async fn artist_images(&self, _ids: Vec<String>) -> Result<HashMap<String, String>> {
        Ok(HashMap::new())
    }

    async fn saved_tracks(&self) -> Result<Vec<Track>> {
        let client = self.inner.client().await?;
        let tracks = library::favourite_tracks(&client).await?;
        Ok(tracks.into_iter().map(map::track).collect())
    }

    async fn set_track_saved(&self, track_id: &str, saved: bool) -> Result<()> {
        let client = self.inner.client().await?;
        let id = TrackId(num(track_id)?);
        match saved {
            true => library::add_favourite_track(&client, id).await?,
            false => library::remove_favourite_track(&client, id).await?,
        }
        Ok(())
    }

    async fn track(&self, track_id: &str) -> Result<Track> {
        let client = self.inner.client().await?;
        let track = library::track(&client, TrackId(num(track_id)?)).await?;
        Ok(map::track(track))
    }

    async fn track_playcount(&self, _track_id: &str) -> Result<Option<u64>> {
        Ok(None)
    }

    async fn playlists(&self) -> Result<Vec<Playlist>> {
        let client = self.inner.client().await?;
        let lists = library::playlists(&client).await?;
        Ok(lists.into_iter().map(map::playlist).collect())
    }

    // ponytail: playlist editing waits for the endpoints. Favouriting a
    // playlist is the one write TIDAL already has here.
    async fn create_playlist(&self, _name: &str) -> Result<String> {
        bail!("creating playlists is not supported yet")
    }

    async fn rename_playlist(&self, _playlist_id: &str, _name: &str) -> Result<()> {
        bail!("renaming playlists is not supported yet")
    }

    async fn delete_playlist(&self, _playlist_id: &str) -> Result<()> {
        bail!("deleting playlists is not supported yet")
    }

    async fn remove_playlist_from_library(&self, playlist_id: &str) -> Result<()> {
        let client = self.inner.client().await?;
        match playlist_id.strip_prefix(map::MIX) {
            Some(mix) => library::unsave_mix(&client, mix).await?,
            None => library::remove_favourite_playlist(&client, playlist_id).await?,
        }
        Ok(())
    }

    async fn add_playlist_to_library(&self, playlist_id: &str) -> Result<()> {
        let client = self.inner.client().await?;
        match playlist_id.strip_prefix(map::MIX) {
            Some(mix) => library::save_mix(&client, mix).await?,
            None => library::add_favourite_playlist(&client, playlist_id).await?,
        }
        Ok(())
    }

    async fn set_playlist_public(&self, _playlist_id: &str, _public: bool) -> Result<()> {
        bail!("playlist visibility is not supported yet")
    }

    async fn add_track_to_playlist(&self, _playlist_id: &str, _track_id: &str) -> Result<()> {
        bail!("editing playlists is not supported yet")
    }

    async fn remove_track_from_playlist(&self, _playlist_id: &str, _track_id: &str) -> Result<()> {
        bail!("editing playlists is not supported yet")
    }

    async fn saved_albums(&self) -> Result<Vec<Album>> {
        let client = self.inner.client().await?;
        let albums = library::albums(&client).await?;
        Ok(albums.into_iter().map(map::album).collect())
    }

    async fn set_album_saved(&self, album_id: &str, saved: bool) -> Result<()> {
        let client = self.inner.client().await?;
        let id = num(album_id)?;
        match saved {
            true => library::add_favourite_album(&client, id).await?,
            false => library::remove_favourite_album(&client, id).await?,
        }
        Ok(())
    }

    async fn saved_artists(&self) -> Result<Vec<SavedArtist>> {
        let client = self.inner.client().await?;
        let artists = library::artists(&client).await?;
        Ok(artists.into_iter().map(map::artist).collect())
    }

    async fn set_artist_saved(&self, artist_id: &str, saved: bool) -> Result<()> {
        let client = self.inner.client().await?;
        let id = num(artist_id)?;
        match saved {
            true => library::add_favourite_artist(&client, id).await?,
            false => library::remove_favourite_artist(&client, id).await?,
        }
        Ok(())
    }

    async fn album(&self, album_id: &str) -> Result<AlbumDetail> {
        let id = num(album_id)?;
        let client = self.inner.client().await?;
        let (album, tracks) = tokio::join!(
            library::album(&client, id),
            library::album_tracks(&client, id)
        );
        Ok(AlbumDetail {
            album: map::album(album?),
            tracks: tracks?.into_iter().map(map::track).collect(),
        })
    }

    async fn album_tracks(&self, album_id: &str) -> Result<Vec<Track>> {
        let client = self.inner.client().await?;
        let tracks = library::album_tracks(&client, num(album_id)?).await?;
        Ok(tracks.into_iter().map(map::track).collect())
    }

    async fn playlist(&self, playlist_id: &str) -> Result<PlaylistDetail> {
        let playlist = match playlist_id.strip_prefix(map::MIX) {
            // A mix has no header request of its own; its card named it,
            // and the caller that has the card keeps the name.
            Some(mix) => map::mix(mix, "Mix", None),
            None => {
                let client = self.inner.client().await?;
                map::playlist(library::playlist(&client, playlist_id).await?)
            }
        };
        let tracks = self.inner.list_tracks(playlist_id).await?;
        Ok(PlaylistDetail {
            playlist,
            tracks,
            continuation: None,
        })
    }

    async fn playlist_tracks(&self, playlist_id: &str) -> Result<Vec<Track>> {
        self.inner.list_tracks(playlist_id).await
    }

    async fn playlist_covers(&self, playlist_id: &str, wanted: usize) -> Result<Vec<String>> {
        let tracks = self.inner.list_tracks(playlist_id).await?;
        Ok(music::distinct_covers(&tracks, wanted))
    }

    /// The radio TIDAL names on the track, served whole: there is no
    /// continuation to follow.
    async fn track_radio(
        &self,
        track_id: &str,
        _from: Option<&str>,
    ) -> Result<(Vec<Track>, Option<String>)> {
        let client = self.inner.client().await?;
        let track = library::track(&client, TrackId(num(track_id)?)).await?;
        let Some(radio) = track.radio else {
            return Ok((Vec::new(), None));
        };
        let tracks = library::mix_tracks(&client, &radio).await?;
        Ok((tracks.into_iter().map(map::track).collect(), None))
    }

    async fn search(&self, query: &str) -> Result<Vec<Track>> {
        let client = self.inner.client().await?;
        let found = search::search(&client, query).await?;
        Ok(found.tracks.into_iter().map(map::track).collect())
    }

    async fn search_albums(&self, query: &str) -> Result<Vec<Album>> {
        let client = self.inner.client().await?;
        let found = search::search(&client, query).await?;
        Ok(found.albums.into_iter().map(map::album).collect())
    }

    async fn search_playlists(&self, query: &str) -> Result<Vec<Playlist>> {
        let client = self.inner.client().await?;
        let found = search::search(&client, query).await?;
        Ok(found.playlists.into_iter().map(map::playlist).collect())
    }

    async fn home(&self) -> Result<HomeFeed> {
        let client = self.inner.client().await?;
        let home = browse::tab_page(&client, Tab::ForYou).await?;
        Ok(HomeFeed {
            listen_again: Vec::new(),
            quick_picks: None,
            sections: map::sections(home),
        })
    }

    async fn genres(&self) -> Result<Vec<music::Genre>> {
        let client = self.inner.client().await?;
        Ok(map::genres(browse::explore(&client).await?))
    }

    async fn genre(&self, genre_id: &str) -> Result<GenreDetail> {
        let client = self.inner.client().await?;
        let home = browse::page_of_rows(&client, genre_id).await?;
        Ok(GenreDetail {
            name: String::new(),
            sections: map::sections(home),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> TidalProvider {
        TidalProvider::new(AuthConfig::default(), Quality::HiResLossless).unwrap()
    }

    fn api() -> TidalApi {
        TidalApi {
            inner: Arc::clone(&provider().inner),
        }
    }

    #[test]
    fn it_signs_in_with_a_code_and_nothing_else() {
        let p = provider();
        assert_eq!(p.sign_in_options(), [SignIn::Default]);
        assert_eq!(p.slug(), "tidal");
        assert!(!p.protected(), "no Widevine in sight");
    }

    #[test]
    fn share_urls_are_the_web_clients() {
        let api = api();
        assert_eq!(
            api.share_url(MediaKind::Track, "655911").as_deref(),
            Some("https://tidal.com/browse/track/655911")
        );
        assert_eq!(
            api.share_url(MediaKind::Playlist, "u-1").as_deref(),
            Some("https://tidal.com/browse/playlist/u-1")
        );
        assert_eq!(
            api.share_url(MediaKind::Playlist, "mix:000a").as_deref(),
            Some("https://tidal.com/browse/mix/000a")
        );
    }

    #[test]
    fn a_string_id_that_is_not_a_number_is_refused_before_the_network() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let api = api();
        let error = rt.block_on(api.album("not-an-id")).unwrap_err();
        assert!(error.to_string().contains("not a TIDAL id"), "{error}");
    }

    #[test]
    fn without_a_token_the_api_asks_for_credentials() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let api = api();
        let error = rt.block_on(api.saved_tracks()).unwrap_err();
        assert!(
            matches!(
                error.downcast_ref::<SignInFailure>(),
                Some(SignInFailure(SignInProblem::Credentials))
            ),
            "{error}"
        );
    }

    #[test]
    fn adopting_a_token_reaches_the_fetch_too() {
        // The engine's fetch and the api share one session: a token taken
        // on for the catalogue is the one the next stream is asked with.
        let p = provider();
        p.inner.adopt(StoredToken {
            user_id: 42,
            ..StoredToken::default()
        });
        assert_eq!(p.inner.profile().id, "42");
        // What `load` would see: `set_session` was called, so this fails on
        // the network rather than on "not signed in".
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let error = rt
            .block_on(music::engine::Fetch::load(&p.inner.fetch, "1"))
            .unwrap_err();
        assert!(!error.to_string().contains("not signed in"), "{error}");
    }

    #[test]
    fn abandoning_stops_the_next_poll() {
        let p = provider();
        p.abandon();
        assert!(p.inner.abandoned.load(Ordering::SeqCst));
    }
}
