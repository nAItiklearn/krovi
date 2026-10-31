//! `krovi-sys` — Krovi's Windows OS integration layer.
//!
//! This crate wraps the few native Windows APIs Krovi needs:
//!   * [`media`] — read-only system media session via GSMTC plus transport
//!     (play/pause/next/prev). **Never captures audio** — it only reads the
//!     metadata Windows already exposes (SPEC C5).
//!   * [`foreground`] — the foreground window's **process name** for local
//!     screen-time sampling. **Never** window titles, URLs, or document content
//!     (SPEC C11).
//!   * [`location`] — a **coarse** latitude/longitude for weather, nothing else
//!     (SPEC C7).
//!
//! ## Pure by default, native behind a feature (DECISIONS.md D-009)
//! The local host (32-bit MinGW gcc) cannot be relied upon to compile windows-rs,
//! and the native calls cannot be exercised without a real desktop session. So
//! every piece of pure logic — process-name normalization, the day-index math,
//! the screen-time interval [`Sampler`], coordinate validation/coarsening, and
//! the transport-command mapping — has **no** OS bindings and is unit-tested
//! here. The actual windows-rs calls live behind `#[cfg(all(windows, feature =
//! "native"))]`; the public entry points exist in every build and degrade
//! gracefully when `native` is off (reads yield `None`/absence; actions return
//! [`SysError::Unsupported`]). No local test results are claimed for the native
//! paths — they are built and verified only in CI.

pub mod foreground;
pub mod location;
pub mod media;

pub use foreground::{day_index, foreground_process, normalize_process, Sampler};
pub use location::{coarsen, current_location, valid_coords};
pub use media::{read_now_playing, send_transport, MediaTrack, TransportCommand};

/// Errors from the OS integration layer. Hand-rolled to keep the dependency
/// surface small, matching `krovi-core` / `krovi-store`.
#[derive(Debug)]
#[non_exhaustive]
pub enum SysError {
    /// The capability is not available in this build or on this platform
    /// (e.g. the `native` feature is off, or there is no active media session).
    Unsupported,
    /// A native call failed; carries the OS error text.
    Message(String),
}

impl std::fmt::Display for SysError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SysError::Unsupported => write!(f, "operation not supported in this build"),
            SysError::Message(m) => write!(f, "system error: {m}"),
        }
    }
}

impl std::error::Error for SysError {}

/// Convenience alias for fallible OS-layer operations.
pub type Result<T> = std::result::Result<T, SysError>;
