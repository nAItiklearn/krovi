//! `krovi-store` — the local-first persistence layer for Krovi.
//!
//! This crate turns the in-memory, UI-independent models from `krovi-core`
//! (clipboard history, notes, to-dos, file shelves, screen-time) into durable
//! state. It exposes a single, UI-agnostic [`Store`] trait with a **load /
//! replace** operation per collection plus a free-form key/value settings bag,
//! and two implementations:
//!
//! * [`MemStore`] — an in-memory implementation used by the rest of the app and
//!   by tests. It has no external dependencies and always compiles and passes.
//! * [`SqliteStore`] — a `rusqlite` + bundled-SQLite implementation that persists
//!   to a `krovi.db` file (SPEC §8). It lives behind the **`sqlite`** Cargo
//!   feature because the bundled SQLite build needs a C/MSVC compiler; when the
//!   feature is off the type simply does not exist, so the crate still builds.
//!
//! Design invariants (consistent with `DECISIONS.md` D-007):
//! * **No network, no telemetry.** Everything is local disk I/O.
//! * **UI-agnostic.** The trait moves plain data (reusing the public `krovi-core`
//!   structs where possible); it knows nothing about Tauri or the frontend.
//! * **Deterministic data.** Timestamps/ids come from the caller's models, never
//!   from a clock inside this crate.

use std::collections::BTreeMap;

use krovi_core::{FileShelf, ScreenTime, Shelf};

// Re-export the core record types the store round-trips, so callers (and the
// Tauri layer) can depend on `krovi-store` alone for the persistence contract.
pub use krovi_core::{CalendarEvent, ClipEntry, ClipKind, Note, ShelfItem, Task};

mod mem;
pub use mem::MemStore;

mod restore;
pub use restore::{restore_all, Restored};

pub mod wire;

#[cfg(feature = "sqlite")]
mod sqlite;
#[cfg(feature = "sqlite")]
pub use sqlite::{SqliteStore, SCHEMA_VERSION};

#[cfg(test)]
pub(crate) mod testkit;

/// Errors returned by a [`Store`]. Hand-rolled (thiserror-style) to keep the
/// default dependency surface empty; the SQLite variant only exists when the
/// `sqlite` feature is enabled.
#[derive(Debug)]
#[non_exhaustive]
pub enum StoreError {
    /// A logical/data problem, e.g. an unrecognised persisted enum value.
    Message(String),
    /// A lower-level SQLite failure (only with the `sqlite` feature).
    #[cfg(feature = "sqlite")]
    Sqlite(rusqlite::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Message(m) => write!(f, "store error: {m}"),
            #[cfg(feature = "sqlite")]
            StoreError::Sqlite(e) => write!(f, "sqlite error: {e}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoreError::Message(_) => None,
            #[cfg(feature = "sqlite")]
            StoreError::Sqlite(e) => Some(e),
        }
    }
}

#[cfg(feature = "sqlite")]
impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sqlite(e)
    }
}

/// Convenience alias for store fallible operations.
pub type Result<T> = std::result::Result<T, StoreError>;

/// A persistable per-day, per-app screen-time total (milliseconds).
///
/// [`ScreenTime`] keeps its aggregation private, so the store flattens it into
/// these records. Use [`screen_time_records`] / [`screen_time_from_records`] to
/// bridge to and from the core aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenTimeRecord {
    pub day: u32,
    pub app: String,
    pub ms: u64,
}

/// A persistable shelf: its name plus its items in display (newest-first) order.
///
/// Mirrors [`krovi_core::Shelf`], whose item list is private.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredShelf {
    pub name: String,
    pub items: Vec<ShelfItem>,
}

impl StoredShelf {
    /// Snapshot a core [`Shelf`] into a persistable record.
    pub fn from_core(shelf: &Shelf) -> Self {
        Self {
            name: shelf.name.clone(),
            items: shelf.items().to_vec(),
        }
    }
}

/// Flatten a [`ScreenTime`] aggregate into canonical records, ordered by day
/// ascending then app name ascending.
pub fn screen_time_records(st: &ScreenTime) -> Vec<ScreenTimeRecord> {
    let mut out = Vec::new();
    for day in st.days() {
        for (app, ms) in st.top_apps(day, 0) {
            out.push(ScreenTimeRecord { day, app, ms });
        }
    }
    out.sort_by(|a, b| a.day.cmp(&b.day).then_with(|| a.app.cmp(&b.app)));
    out
}

/// Rebuild a [`ScreenTime`] aggregate from persisted records. Records with
/// `ms == 0` are ignored, matching `ScreenTime::record`'s semantics.
pub fn screen_time_from_records(records: &[ScreenTimeRecord]) -> ScreenTime {
    let mut st = ScreenTime::new();
    for r in records {
        st.record(r.day, r.app.clone(), r.ms);
    }
    st
}

/// Flatten every shelf of a [`FileShelf`], preserving shelf order.
pub fn stored_shelves(fs: &FileShelf) -> Vec<StoredShelf> {
    fs.shelf_names()
        .into_iter()
        .filter_map(|n| fs.shelf(n))
        .map(StoredShelf::from_core)
        .collect()
}

/// The persistence contract for Krovi's durable state.
///
/// Each collection has a `load_*` reader and a `save_*` writer with **replace**
/// semantics: `save_*` makes the stored collection exactly equal to the slice
/// passed in (order included), so the caller persists the whole collection from
/// its in-memory `krovi-core` model after a mutation. Settings are an
/// independent string key/value bag.
///
/// Implementations must round-trip: after `save_x(&v)`, `load_x()` returns a
/// value equal to `v`.
pub trait Store {
    /// Load clipboard entries in stored (display) order.
    fn load_clipboard(&self) -> Result<Vec<ClipEntry>>;
    /// Replace all clipboard entries with `entries` (order preserved).
    fn save_clipboard(&mut self, entries: &[ClipEntry]) -> Result<()>;

    /// Load notes in stored order.
    fn load_notes(&self) -> Result<Vec<Note>>;
    /// Replace all notes with `notes` (order preserved).
    fn save_notes(&mut self, notes: &[Note]) -> Result<()>;

    /// Load to-do tasks in stored order.
    fn load_tasks(&self) -> Result<Vec<Task>>;
    /// Replace all tasks with `tasks` (order preserved).
    fn save_tasks(&mut self, tasks: &[Task]) -> Result<()>;

    /// Load shelves (and their items) in stored order.
    fn load_shelves(&self) -> Result<Vec<StoredShelf>>;
    /// Replace all shelves with `shelves` (order preserved).
    fn save_shelves(&mut self, shelves: &[StoredShelf]) -> Result<()>;

    /// Load screen-time records in canonical (day asc, app asc) order.
    fn load_screen_time(&self) -> Result<Vec<ScreenTimeRecord>>;
    /// Replace all screen-time records with `records`.
    fn save_screen_time(&mut self, records: &[ScreenTimeRecord]) -> Result<()>;

    /// Load calendar events in stored (chronological) order.
    fn load_calendar(&self) -> Result<Vec<CalendarEvent>>;
    /// Replace all calendar events with `events` (order preserved).
    fn save_calendar(&mut self, events: &[CalendarEvent]) -> Result<()>;

    /// Get a single setting value, or `None` if unset.
    fn get_setting(&self, key: &str) -> Result<Option<String>>;
    /// Set (insert or overwrite) a setting.
    fn set_setting(&mut self, key: &str, value: &str) -> Result<()>;
    /// Delete a setting. Returns `true` if it existed.
    fn delete_setting(&mut self, key: &str) -> Result<bool>;
    /// All settings as a sorted map.
    fn settings(&self) -> Result<BTreeMap<String, String>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use krovi_core::{FileShelf, ScreenTime};

    #[test]
    fn screen_time_bridge_round_trips() {
        let mut st = ScreenTime::new();
        st.record(10, "a.exe", 5);
        st.record(10, "b.exe", 7);
        st.record(11, "a.exe", 3);

        let recs = screen_time_records(&st);
        assert_eq!(
            recs,
            vec![
                ScreenTimeRecord {
                    day: 10,
                    app: "a.exe".into(),
                    ms: 5
                },
                ScreenTimeRecord {
                    day: 10,
                    app: "b.exe".into(),
                    ms: 7
                },
                ScreenTimeRecord {
                    day: 11,
                    app: "a.exe".into(),
                    ms: 3
                },
            ]
        );

        let st2 = screen_time_from_records(&recs);
        assert_eq!(screen_time_records(&st2), recs);
        assert_eq!(st2.app_total(10, "b.exe"), 7);
        assert_eq!(st2.day_total(10), 12);
    }

    #[test]
    fn screen_time_from_records_drops_zero_ms() {
        let recs = vec![
            ScreenTimeRecord {
                day: 1,
                app: "x".into(),
                ms: 0,
            },
            ScreenTimeRecord {
                day: 1,
                app: "y".into(),
                ms: 4,
            },
        ];
        let st = screen_time_from_records(&recs);
        assert_eq!(
            screen_time_records(&st),
            vec![ScreenTimeRecord {
                day: 1,
                app: "y".into(),
                ms: 4
            }]
        );
    }

    #[test]
    fn stored_shelves_bridge_preserves_order() {
        let mut fs = FileShelf::new();
        fs.add_shelf("Work");
        fs.add_item("Work", "a.txt", "blobs/a", 10, 1);
        fs.add_item("Work", "b.txt", "blobs/b", 20, 2);

        let shelves = stored_shelves(&fs);
        assert_eq!(shelves.len(), 2);
        assert_eq!(shelves[0].name, "Default");
        assert!(shelves[0].items.is_empty());
        assert_eq!(shelves[1].name, "Work");
        // newest-first inside a shelf
        assert_eq!(shelves[1].items[0].name, "b.txt");
        assert_eq!(shelves[1].items[1].name, "a.txt");
    }

    #[test]
    fn stored_shelf_from_core_matches() {
        let mut fs = FileShelf::new();
        fs.add_item("Default", "only.txt", "blobs/only", 3, 9);
        let core = fs.shelf("Default").unwrap();
        let stored = StoredShelf::from_core(core);
        assert_eq!(stored.name, "Default");
        assert_eq!(stored.items, core.items().to_vec());
    }

    #[test]
    fn error_display_and_source() {
        let e = StoreError::Message("boom".into());
        assert!(format!("{e}").contains("boom"));
        assert!(std::error::Error::source(&e).is_none());
    }
}
