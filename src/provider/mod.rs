//! TIDAL as a sonora provider.
//!
//! sonora's `music` crate defines what a streaming service looks like to a
//! player: `MusicProvider` for signing in, `MusicApi` for the catalogue and
//! `PlaybackFactory` for the sound. This module answers all three with the
//! requests the rest of this crate already makes, so the same TIDAL code
//! serves this client and any other that speaks those traits.

pub(crate) mod map;
pub(crate) mod tidal;

pub use tidal::TidalProvider;
