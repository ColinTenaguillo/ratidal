//! What the shell asks a signed-in service for, in this crate's own types.
//!
//! Two things answer: TIDAL directly (`tidal`), with every row and path
//! the web client has, and any provider sonora's `music` crate knows
//! (`sonora`), through its `MusicApi`. The shell holds one behind
//! `Arc<dyn Catalogue>` and does not know which.
//!
//! The error is still `TidalError`: it is the vocabulary the shell already
//! matches on, and the one variant it inspects, `Unauthorized`, means the
//! same thing from any service. ponytail: rename it when a second service
//! grows an error of its own worth naming.

pub(crate) mod sonora;
pub(crate) mod subsonic;
pub(crate) mod tidal;

use std::sync::Arc;

use async_trait::async_trait;

use crate::auth::StoredToken;
use crate::browse::{Home, Mixes, Tab};
use crate::domain::{Quality, Track, TrackId};
use crate::library::{Album, Artist, ArtistPage, FavouriteIds, Playlist};
use crate::search::Results;
use crate::shell::carousel::Card;
use crate::tidal::TidalError;

pub use sonora::Sonora;
pub use tidal::Tidal;

pub type Result<T> = std::result::Result<T, TidalError>;

/// What `F` can favourite: the four kinds the account keeps a list of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Favourable {
    Artist(String),
    Album(String),
    Playlist(String),
    Mix(String),
}

/// The shell's state derives `Debug`; a catalogue prints as its name.
impl std::fmt::Debug for dyn Catalogue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[async_trait]
pub trait Catalogue: Send + Sync {
    /// What the service is called, for the log and the status line.
    fn name(&self) -> &'static str;

    /// The engine that plays this catalogue's tracks.
    fn playback(&self) -> Arc<dyn music::PlaybackFactory>;

    /// What the engine's events leave out about a stream, where the
    /// service's fetch records it. The badge reads it.
    fn stream_meta(&self) -> Arc<dyn crate::playback::StreamMeta> {
        Arc::new(crate::playback::NoMeta)
    }

    /// The TIDAL token behind the session, where there is one. Renewal and
    /// the token file still speak it; another service answers `None`.
    fn token(&self) -> Option<StoredToken> {
        None
    }

    /// A renewed token, for the catalogue that has one.
    fn adopt(&self, _token: StoredToken) {}

    /// What to ask for from now on. A service with one quality ignores it.
    fn set_quality(&self, _wanted: Quality) {}

    async fn playlists(&self) -> Result<Vec<Playlist>>;
    async fn albums(&self) -> Result<Vec<Album>>;
    async fn artists(&self) -> Result<Vec<Artist>>;
    async fn favourite_tracks(&self) -> Result<Vec<Track>>;
    async fn favourite_ids(&self) -> Result<FavouriteIds>;
    async fn set_track_favourite(&self, id: &TrackId, on: bool) -> Result<()>;
    async fn set_favourite(&self, what: &Favourable, on: bool) -> Result<()>;

    async fn playlist_tracks(&self, uuid: &str) -> Result<Vec<Track>>;
    async fn album_page(&self, id: &str, title: &str) -> Result<ArtistPage>;
    async fn artist_page(&self, id: &str) -> Result<ArtistPage>;
    async fn mix_tracks(&self, id: &str) -> Result<Vec<Track>>;
    async fn track(&self, id: &TrackId) -> Result<Track>;

    async fn search(&self, query: &str) -> Result<Results>;

    async fn tab_page(&self, tab: Tab) -> Result<Home>;
    /// The rest of a row the page cut, `wanted` items at most.
    async fn module_items(&self, path: &str, wanted: u32) -> Result<Vec<Card>>;
    async fn mixes(&self) -> Result<Mixes>;
    async fn page_of_rows(&self, path: &str) -> Result<Home>;
    async fn feed(&self) -> Result<Vec<Card>>;
    async fn explore(&self) -> Result<Home>;
}
