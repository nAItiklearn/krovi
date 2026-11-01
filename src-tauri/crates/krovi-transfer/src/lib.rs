//! Krovi Quick Transfer (owner: transfer agent). See docs/CONTRACTS.md section 6.
//!
//! LAN-only file transfer between this PC and a phone, with no cloud, relay or account.
//!
//! * A tiny HTTP server (`tiny_http`) exists only while at least one share is open. It is
//!   bound to ONE private IPv4 address of the active network interface (never `0.0.0.0`) on a
//!   random high port, and serves nothing except unguessable token URLs.
//! * `/s/<token>` is a download page, `/r/<token>` an upload page. Everything else is a plain 404.
//! * Tokens are 160 bits from the OS CSPRNG. Shares expire (default 10 min), can be stopped,
//!   and everything closes on app quit. An optional 4-digit PIN adds a second factor.
//!
//! The crate has no Tauri dependency: events go through the [`EventSink`] trait.

mod error;
mod events;
pub mod firewall;
mod manager;
pub mod names;
pub mod net;
pub mod pages;
pub mod qr;
pub mod range;
pub mod route;
mod server;
mod share;
pub mod token;

pub use error::TransferError;
pub use events::{CloseReason, Direction, Event, EventSink, NullSink, Throttle};
pub use manager::{now_ms, Config, ShareOptions, TransferManager, DEFAULT_MAX_UPLOAD, DEFAULT_TTL};
pub use share::{FileEntry, Mode, ShareInfo, MAX_PIN_FAILURES};

/// Native "choose files" dialog (Windows IFileOpenDialog via `rfd`). Returns the chosen paths,
/// or an empty list if cancelled. Blocks until the dialog closes: call from a worker thread.
#[cfg(all(windows, feature = "dialog"))]
pub fn pick_files<W>(parent: Option<&W>) -> Vec<std::path::PathBuf>
where
    W: raw_window_handle::HasWindowHandle + raw_window_handle::HasDisplayHandle,
{
    let mut d = rfd::FileDialog::new().set_title("Choose files to send to your phone");
    if let Some(p) = parent {
        d = d.set_parent(p);
    }
    d.pick_files().unwrap_or_default()
}

/// No native dialog on this build (non-Windows or the `dialog` feature is off).
#[cfg(not(all(windows, feature = "dialog")))]
pub fn pick_files<W>(_parent: Option<&W>) -> Vec<std::path::PathBuf> {
    Vec::new()
}
