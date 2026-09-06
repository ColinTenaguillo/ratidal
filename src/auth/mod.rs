//! Signing in and keeping the session alive.
//!
//! `store` stays public because the shell persists and clears tokens through
//! it; `device_flow` is an implementation detail behind the re-exports below.

pub(crate) mod device_flow;
pub mod store;

pub use device_flow::{poll_once, refresh, start_login, AuthError, DeviceCode, PollOutcome};
pub use store::StoredToken;
