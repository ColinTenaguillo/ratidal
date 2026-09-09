//! Playing audio: manifests, segment streaming, and the engine thread.
//!
//! The submodules are `pub(crate)`: the facade below is the whole public
//! surface, so nothing outside can couple to `SegmentReader`'s buffering or
//! the engine's internals.

pub(crate) mod engine;
pub(crate) mod queue;
pub(crate) mod manifest;
pub(crate) mod segments;

pub use engine::{spawn, Cmd, PlaybackEvent};
pub use queue::{clock_rng, Queue, Repeat, Source};
pub use manifest::{Manifest, ManifestError, PlaybackInfo};
pub use segments::SegmentReader;
