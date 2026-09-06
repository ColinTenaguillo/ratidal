//! HTTP against the TIDAL API.
//!
//! Infrastructure rather than a capability: the components consume it, nothing
//! outside the crate should. `dto` stays crate-visible because `library` maps
//! those wire shapes into domain types, but neither module escapes the crate.

pub(crate) mod dto;
pub(crate) mod http;

pub use http::{Api, Client, TidalError};
