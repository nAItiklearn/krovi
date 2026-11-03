//! `krovi-sys` — Krovi's Windows OS integration layer.
//!
//! This crate wraps the few native Windows APIs Krovi needs:
//!   * [`clipboard`] — the OS clipboard: a sequence-number change token, a read
//!     (text / link / file list / image thumbnail) that skips content flagged
//!     sensitive by password managers, and write-back.
//!   * [`stats`] — RAM, system-drive space and CPU % for the live pill.
//!   * [`media`] — read-only system media session via GSMTC plus transport
//!     (play/pause/next/prev), run on a dedicated MTA thread. **Never captures
//!     audio** — it only reads the metadata Windows already exposes (SPEC C5).
//!   * [`foreground`] — the foreground window's **process name** for local
//!     screen-time sampling, plus whether the user is present (idle time and a
//!     locked-desktop check) so away time is not counted. **Never** window titles,
//!     URLs, document content or input (SPEC C11).
//!   * [`location`] — a **coarse** latitude/longitude for weather, nothing else
//!     (SPEC C7).
//!   * [`capture`] — **one-shot** region screen capture (GDI) for Circle to Ask and the
//!     explicit, time-boxed screen-awareness session (DECISIONS.md D-021). No stream, no
//!     timer, no disk: a still image of one rectangle, in memory, when the caller asks.
//!   * [`ocr`] — on-device text recognition (Windows.Media.Ocr) over such an image.
//!   * [`web`] — opens a fixed set of addresses (a Google search for selected text, a
//!     provider's API-key page) in the user's browser; nothing else can be opened.
//!
//! ## Pure by default, native behind a feature
//! Every piece of pure logic — the clipboard change-token gate and text
//! classification, thumbnailing, media change tracking and position math, CPU %
//! math, process-name normalization, the screen-time interval [`Sampler`],
//! coordinate validation/coarsening, and the transport-command mapping — has **no**
//! OS bindings and is unit-tested on any host. The actual windows-rs/arboard calls
//! live behind `#[cfg(all(windows, feature = "native"))]`; the public entry points
//! exist in every build and degrade gracefully when `native` is off (reads yield
//! `None`/absence; actions return [`SysError::Unsupported`]). The native paths are
//! exercised on a real desktop session by `examples/live_probe.rs`
//! (`cargo run -p krovi-sys --features native --example live_probe`).

pub mod capture;
pub mod clipboard;
pub mod foreground;
pub mod fullscreen;
pub mod imaging;
pub mod location;
pub mod media;
pub mod ocr;
pub mod session;
pub mod stats;
pub mod web;

pub use capture::{
    capture_region, clip_to_screen, exclude_window_from_capture, monitor_at, monitor_at_cursor,
    primary_monitor, virtual_screen, ClipError, Image, Rect,
};
pub use clipboard::{
    change_token, classify_text, files_to_text, is_link, is_sensitive, prepare_text, read_capture,
    write_image_png, write_text, CaptureKind, ClipCapture, ClipGate, ReadOutcome,
};
pub use foreground::{
    foreground_process, is_present, local_day_index, normalize_process, presence, Credit, Presence,
    Sampler, IDLE_THRESHOLD_MS, MAX_CREDIT_GAP_MS, SAMPLE_INTERVAL_MS,
};
pub use location::{coarsen, current_location, valid_coords};
pub use media::{
    read_now_playing, send_transport, MediaState, MediaTrack, MediaTracker, MediaWorker,
    TransportCommand,
};
pub use ocr::{OcrError, OcrLine, OcrOutput};
pub use session::{clamp_minutes, Session, Slot, MAX_SESSION_MINUTES, SLOT_TTL};
pub use stats::{cpu_percent, CpuTimes, StatsSampler, SystemStats};

/// Current time as Unix epoch milliseconds (0 if the clock is before 1970).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

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
