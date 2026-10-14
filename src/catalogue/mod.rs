//! What the shell asks a signed-in service for, in this crate's own types.
//!
//! Every service comes through sonora's `music` crate: a `MusicProvider`
//! signs in and hands back a `ProviderSession`, whose `MusicApi` the
//! `Session` here reads into the shell's types. TIDAL is one such provider
//! (`crate::provider`), with a `Fetch` of its own behind the engine. The
//! shell holds a `Session` and does not know which service answers.
//!
//! The error is still `TidalError`: it is the vocabulary the shell already
//! matches on, and the one variant it inspects, `Unauthorized`, means the
//! same thing from any service. ponytail: rename it when a second service
//! grows an error of its own worth naming.

pub(crate) mod session;

use std::sync::Arc;

use music::MusicProvider;

use crate::config::{Config, ProviderKind};
use crate::playback::{NoMeta, StreamMeta};
use crate::tidal::TidalError;

pub use session::Session;

pub type Result<T> = std::result::Result<T, TidalError>;

/// What `F` can favourite: the three kinds the account keeps a list of.
/// A TIDAL mix is a playlist here, under its prefixed id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Favourable {
    Artist(String),
    Album(String),
    Playlist(String),
}

/// A service the shell can sign in to: sonora's trait, plus what the
/// engine's events leave out about a stream where the service's own fetch
/// records it. The quality badge reads that; a service without one shows
/// no badge.
pub trait Provider: MusicProvider {
    fn stream_meta(&self) -> Arc<dyn StreamMeta> {
        Arc::new(NoMeta)
    }
}

impl Provider for music::subsonic::SubsonicProvider {}

/// The shell's state derives `Debug`; a provider prints as its name.
impl std::fmt::Debug for dyn Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// The service the config names, signed out.
pub fn provider_for(config: &Config) -> anyhow::Result<Arc<dyn Provider>> {
    Ok(match config.provider.kind {
        ProviderKind::Tidal => Arc::new(crate::provider::TidalProvider::new(
            config.auth.clone(),
            config.audio.quality(),
        )?),
        ProviderKind::Subsonic => Arc::new(music::subsonic::SubsonicProvider::new()),
    })
}
