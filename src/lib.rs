//! ratidal — a terminal client for TIDAL.
//!
//! Modules are business capabilities, each exposing a facade while its
//! submodules stay `pub(crate)`. `domain` is the dependency-free core that
//! everything else points at.
//!
//! `tidal` is infrastructure rather than a capability. The design doc argues
//! it should be crate-private, and its submodules are — but the module itself
//! stays public because `library`'s queries take a `&Client`, so any caller
//! outside the crate needs to construct one. Making `library` own that
//! construction would hide it properly; until then this is the honest
//! boundary rather than a pretended one.

pub mod auth;
pub mod browse;
pub mod config;
pub mod domain;
pub mod library;
pub mod playback;
pub mod shell;
pub mod tidal;
