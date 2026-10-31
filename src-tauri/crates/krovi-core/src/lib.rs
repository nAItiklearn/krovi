//! `krovi-core` — the pure, UI-independent logic core for Krovi.
//!
//! This crate contains **no** Tauri, OS bindings, networking, or I/O. Everything
//! here is deterministic and unit-testable in isolation (timestamps and other
//! environmental inputs are supplied by callers). Persistence lives in
//! `krovi-store`; OS/network access lives in `krovi-sys` / `krovi-net`.

pub mod clipboard;
pub mod paste_stack;
pub mod timer;

pub use clipboard::{ClipEntry, ClipKind, ClipboardHistory, DEFAULT_CAP};
pub use paste_stack::{PasteStack, MAX_SLOTS};
pub use timer::{CountdownTimer, Stopwatch, TimerState};
