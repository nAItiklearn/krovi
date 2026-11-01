//! `krovi-core` — the pure, UI-independent logic core for Krovi.
//!
//! This crate contains **no** Tauri, OS bindings, networking, or I/O. Everything
//! here is deterministic and unit-testable in isolation (timestamps and other
//! environmental inputs are supplied by callers). Persistence lives in
//! `krovi-store`; OS/network access lives in `krovi-sys` / `krovi-net`.

pub mod calendar;
pub mod clipboard;
pub mod file_shelf;
pub mod notes;
pub mod paste_stack;
pub mod pomodoro;
pub mod screen_time;
pub mod timer;
pub mod todo;

pub use calendar::{Calendar, CalendarError, CalendarEvent};
pub use clipboard::{ClipEntry, ClipKind, ClipboardHistory, DEFAULT_CAP};
pub use file_shelf::{FileShelf, Shelf, ShelfItem, DEFAULT_SHELF};
pub use notes::{Note, Notes};
pub use paste_stack::{PasteStack, MAX_SLOTS};
pub use pomodoro::{Phase, Pomodoro, PomodoroConfig};
pub use screen_time::ScreenTime;
pub use timer::{CountdownTimer, Stopwatch, TimerState};
pub use todo::{Task, TodoList};
