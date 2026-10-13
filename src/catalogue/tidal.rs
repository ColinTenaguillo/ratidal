//! TIDAL, asked directly: the requests the shell has always made.

use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;

use crate::auth::StoredToken;
use crate::browse::{self, Home, Mixes, Tab};
use crate::catalogue::{Catalogue, Favourable, Result};
use crate::domain::{Quality, Track, TrackId};
use crate::library::{self, Album, Artist, ArtistPage, FavouriteIds, Playlist};
use crate::playback::fetch::{Session, TidalFetch, TidalPlayback};
use crate::search::{self, Results};
use crate::shell::carousel::Card;
use crate::tidal::Client;

pub struct Tidal {
    token: Mutex<StoredToken>,
    wanted: Mutex<Quality>,
    fetch: TidalFetch,
    playback: Arc<TidalPlayback>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Tidal {
    pub fn new(token: StoredToken, wanted: Quality) -> anyhow::Result<Self> {
        let fetch = TidalFetch::new()?;
        let playback = Arc::new(TidalPlayback::new(fetch.clone()));
        let this = Self {
            token: Mutex::new(token),
            wanted: Mutex::new(wanted),
            fetch,
            playback,
        };
        this.tell_the_fetch();
        Ok(this)
    }

    /// The engine's fetch asks with the same token and quality as the
    /// catalogue, so a renewal or a changed setting reaches the next stream.
    fn tell_the_fetch(&self) {
        self.fetch.set_session(Session {
            token: lock(&self.token).clone(),
            wanted: *lock(&self.wanted),
        });
    }

    /// A client on the current token. Built per request, as the shell
    /// always did: it is a reqwest client and a token, nothing to keep.
    fn client(&self) -> Client {
        Client::new(lock(&self.token).clone())
    }
}

#[async_trait]
impl Catalogue for Tidal {
    fn name(&self) -> &'static str {
        "TIDAL"
    }

    fn playback(&self) -> Arc<dyn music::PlaybackFactory> {
        self.playback.clone()
    }

    fn stream_meta(&self) -> Arc<dyn crate::playback::StreamMeta> {
        Arc::new(self.fetch.clone())
    }

    fn token(&self) -> Option<StoredToken> {
        Some(lock(&self.token).clone())
    }

    fn adopt(&self, token: StoredToken) {
        *lock(&self.token) = token;
        self.tell_the_fetch();
    }

    fn set_quality(&self, wanted: Quality) {
        *lock(&self.wanted) = wanted;
        self.tell_the_fetch();
    }

    async fn playlists(&self) -> Result<Vec<Playlist>> {
        library::playlists(&self.client()).await
    }

    async fn albums(&self) -> Result<Vec<Album>> {
        library::albums(&self.client()).await
    }

    async fn artists(&self) -> Result<Vec<Artist>> {
        library::artists(&self.client()).await
    }

    async fn favourite_tracks(&self) -> Result<Vec<Track>> {
        library::favourite_tracks(&self.client()).await
    }

    async fn favourite_ids(&self) -> Result<FavouriteIds> {
        library::favourite_ids(&self.client()).await
    }

    async fn set_track_favourite(&self, id: &TrackId, on: bool) -> Result<()> {
        let client = self.client();
        match on {
            true => library::add_favourite_track(&client, id.clone()).await,
            false => library::remove_favourite_track(&client, id.clone()).await,
        }
    }

    async fn set_favourite(&self, what: &Favourable, on: bool) -> Result<()> {
        let client = self.client();
        match (what, on) {
            (Favourable::Artist(id), true) => library::add_favourite_artist(&client, id).await,
            (Favourable::Artist(id), false) => {
                library::remove_favourite_artist(&client, id).await
            }
            (Favourable::Album(id), true) => library::add_favourite_album(&client, id).await,
            (Favourable::Album(id), false) => library::remove_favourite_album(&client, id).await,
            (Favourable::Playlist(u), true) => library::add_favourite_playlist(&client, u).await,
            (Favourable::Playlist(u), false) => {
                library::remove_favourite_playlist(&client, u).await
            }
            (Favourable::Mix(id), true) => library::save_mix(&client, id).await,
            (Favourable::Mix(id), false) => library::unsave_mix(&client, id).await,
        }
    }

    async fn playlist_tracks(&self, uuid: &str) -> Result<Vec<Track>> {
        library::playlist_tracks(&self.client(), uuid).await
    }

    async fn album_page(&self, id: &str, title: &str) -> Result<ArtistPage> {
        library::album_page(&self.client(), id, title).await
    }

    async fn artist_page(&self, id: &str) -> Result<ArtistPage> {
        library::artist_page(&self.client(), id).await
    }

    async fn mix_tracks(&self, id: &str) -> Result<Vec<Track>> {
        library::mix_tracks(&self.client(), id).await
    }

    async fn track(&self, id: &TrackId) -> Result<Track> {
        library::track(&self.client(), id.clone()).await
    }

    async fn search(&self, query: &str) -> Result<Results> {
        search::search(&self.client(), query).await
    }

    async fn tab_page(&self, tab: Tab) -> Result<Home> {
        browse::tab_page(&self.client(), tab).await
    }

    async fn module_items(&self, path: &str, wanted: u32) -> Result<Vec<Card>> {
        browse::module_items(&self.client(), path, wanted).await
    }

    async fn mixes(&self) -> Result<Mixes> {
        browse::mixes(&self.client()).await
    }

    async fn page_of_rows(&self, path: &str) -> Result<Home> {
        browse::page_of_rows(&self.client(), path).await
    }

    async fn feed(&self) -> Result<Vec<Card>> {
        browse::feed(&self.client()).await
    }

    async fn explore(&self) -> Result<Home> {
        browse::explore(&self.client()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_renewed_token_and_a_changed_quality_reach_the_engines_fetch() {
        // The stream is asked for by the fetch, not by the catalogue, so
        // what the catalogue learns has to be handed on or the next track
        // plays on the old token at the old quality.
        let t = Tidal::new(
            StoredToken {
                user_id: 1,
                access_token: "old".into(),
                ..StoredToken::default()
            },
            Quality::Lossless,
        )
        .unwrap();
        assert_eq!(t.token().map(|t| t.access_token).as_deref(), Some("old"));

        t.adopt(StoredToken {
            user_id: 1,
            access_token: "new".into(),
            ..StoredToken::default()
        });
        t.set_quality(Quality::HiResLossless);
        let session = t.fetch.session().expect("told");
        assert_eq!(session.token.access_token, "new");
        assert_eq!(session.wanted, Quality::HiResLossless);
    }
}
