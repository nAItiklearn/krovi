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

use krovi_core::screen_time::retention_cutoff;
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

/// Only the rows of a [`ScreenTime`] that changed since they were last written, as
/// absolute totals (day asc, app asc) — the input for [`Store::upsert_screen_time`].
pub fn screen_time_dirty_records(st: &ScreenTime) -> Vec<ScreenTimeRecord> {
    st.dirty_rows()
        .into_iter()
        .map(|(day, app, ms)| ScreenTimeRecord { day, app, ms })
        .collect()
}

/// Rebuild a [`ScreenTime`] aggregate from persisted records. Records with
/// `ms == 0` are ignored, matching `ScreenTime::record`'s semantics. What was just
/// loaded is by definition already stored, so nothing in the result is dirty.
pub fn screen_time_from_records(records: &[ScreenTimeRecord]) -> ScreenTime {
    let mut st = ScreenTime::new();
    for r in records {
        st.record(r.day, r.app.clone(), r.ms);
    }
    st.clear_dirty();
    st
}

/// Retention and batched, incremental persistence for screen time — the one call the
/// app makes after each sample.
///
/// * **Retention.** Once after the aggregate is loaded and then once per local day
///   (`today` is the caller's local day index), days older than
///   [`krovi_core::screen_time::RETENTION_DAYS`] are dropped from memory and storage.
/// * **Batching.** Changed rows are upserted at most once per
///   [`krovi_core::screen_time::FLUSH_EVERY_MS`]; `force` (used on quit) writes them now.
///   Only the `(day, app)` rows that changed since the last write go out, never the whole
///   table.
/// * **Failure.** A failed write leaves the rows dirty and backs off a full interval; the
///   first error is returned and nothing is lost from memory.
///
/// `now_ms` and `today` come from the caller (no clock in this crate).
pub fn sync_screen_time(
    store: &mut dyn Store,
    st: &mut ScreenTime,
    now_ms: u64,
    today: u32,
    force: bool,
) -> Result<()> {
    let prune = st.needs_prune(today);
    let mut first_err: Option<StoreError> = None;
    if prune {
        let cutoff = retention_cutoff(today);
        st.prune_before(cutoff);
        match store.prune_screen_time(cutoff) {
            Ok(_) => st.mark_pruned(today),
            Err(e) => first_err = Some(e),
        }
    }
    if st.has_dirty() && (force || st.flush_due(now_ms)) {
        match store.upsert_screen_time(&screen_time_dirty_records(st)) {
            Ok(()) => st.mark_flushed(now_ms),
            Err(e) => {
                st.defer_flush(now_ms);
                first_err.get_or_insert(e);
            }
        }
    }
    first_err.map_or(Ok(()), Err)
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
    /// Insert or overwrite just the given `(day, app)` totals (absolute values, not
    /// deltas, so a retry is harmless). Rows not listed are left untouched. This is
    /// what the app calls while sampling; [`Store::save_screen_time`] stays for tests
    /// and one-off imports. Backends should skip rows whose total is unchanged.
    ///
    /// The default merges through load + replace, so any `Store` works; the shipped
    /// backends override it with a true incremental write.
    fn upsert_screen_time(&mut self, records: &[ScreenTimeRecord]) -> Result<()> {
        let mut all: BTreeMap<(u32, String), u64> = self
            .load_screen_time()?
            .into_iter()
            .map(|r| ((r.day, r.app), r.ms))
            .collect();
        for r in records {
            all.insert((r.day, r.app.clone()), r.ms);
        }
        let merged: Vec<ScreenTimeRecord> = all
            .into_iter()
            .map(|((day, app), ms)| ScreenTimeRecord { day, app, ms })
            .collect();
        self.save_screen_time(&merged)
    }
    /// Delete every screen-time row whose `day` is before `min_day` (retention).
    /// Returns how many rows were removed.
    fn prune_screen_time(&mut self, min_day: u32) -> Result<usize> {
        let all = self.load_screen_time()?;
        let keep: Vec<ScreenTimeRecord> =
            all.iter().filter(|r| r.day >= min_day).cloned().collect();
        let removed = all.len() - keep.len();
        if removed > 0 {
            self.save_screen_time(&keep)?;
        }
        Ok(removed)
    }

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
    fn a_rebuilt_aggregate_is_clean_and_only_new_time_is_dirty() {
        let recs = vec![
            ScreenTimeRecord {
                day: 10,
                app: "a.exe".into(),
                ms: 5,
            },
            ScreenTimeRecord {
                day: 10,
                app: "b.exe".into(),
                ms: 7,
            },
        ];
        let mut st = screen_time_from_records(&recs);
        assert!(screen_time_dirty_records(&st).is_empty(), "just loaded");

        st.record(10, "b.exe", 3);
        assert_eq!(
            screen_time_dirty_records(&st),
            vec![ScreenTimeRecord {
                day: 10,
                app: "b.exe".into(),
                ms: 10 // the full total, not the 3 ms delta
            }]
        );
    }

    /// Forwards every `Store` method except the two screen-time write paths to the
    /// `MemStore` in the field `$inner`; the caller supplies those (or leaves them to the
    /// trait defaults).
    macro_rules! forward_store {
        ($ty:ty, $inner:tt, { $($extra:item)* }) => {
            impl Store for $ty {
                fn load_clipboard(&self) -> Result<Vec<ClipEntry>> {
                    self.$inner.load_clipboard()
                }
                fn save_clipboard(&mut self, e: &[ClipEntry]) -> Result<()> {
                    self.$inner.save_clipboard(e)
                }
                fn load_notes(&self) -> Result<Vec<Note>> {
                    self.$inner.load_notes()
                }
                fn save_notes(&mut self, n: &[Note]) -> Result<()> {
                    self.$inner.save_notes(n)
                }
                fn load_tasks(&self) -> Result<Vec<Task>> {
                    self.$inner.load_tasks()
                }
                fn save_tasks(&mut self, t: &[Task]) -> Result<()> {
                    self.$inner.save_tasks(t)
                }
                fn load_shelves(&self) -> Result<Vec<StoredShelf>> {
                    self.$inner.load_shelves()
                }
                fn save_shelves(&mut self, s: &[StoredShelf]) -> Result<()> {
                    self.$inner.save_shelves(s)
                }
                fn load_screen_time(&self) -> Result<Vec<ScreenTimeRecord>> {
                    self.$inner.load_screen_time()
                }
                fn save_screen_time(&mut self, r: &[ScreenTimeRecord]) -> Result<()> {
                    self.$inner.save_screen_time(r)
                }
                fn load_calendar(&self) -> Result<Vec<CalendarEvent>> {
                    self.$inner.load_calendar()
                }
                fn save_calendar(&mut self, e: &[CalendarEvent]) -> Result<()> {
                    self.$inner.save_calendar(e)
                }
                fn get_setting(&self, k: &str) -> Result<Option<String>> {
                    self.$inner.get_setting(k)
                }
                fn set_setting(&mut self, k: &str, v: &str) -> Result<()> {
                    self.$inner.set_setting(k, v)
                }
                fn delete_setting(&mut self, k: &str) -> Result<bool> {
                    self.$inner.delete_setting(k)
                }
                fn settings(&self) -> Result<BTreeMap<String, String>> {
                    self.$inner.settings()
                }
                $($extra)*
            }
        };
    }

    /// Only the required methods: the trait's default `upsert_screen_time` /
    /// `prune_screen_time` are what runs.
    struct LoadSaveOnly(MemStore);
    forward_store!(LoadSaveOnly, 0, {});

    #[test]
    fn default_upsert_and_prune_work_for_any_store() {
        let mut s = LoadSaveOnly(MemStore::new());
        crate::testkit::exercise_screen_time_incremental(&mut s);
    }

    /// Records exactly what `sync_screen_time` asks the store to do, and can be told to
    /// fail its writes.
    #[derive(Default)]
    struct Spy {
        inner: MemStore,
        upserts: Vec<Vec<ScreenTimeRecord>>,
        prunes: Vec<u32>,
        fail_writes: bool,
    }
    forward_store!(Spy, inner, {
        fn upsert_screen_time(&mut self, records: &[ScreenTimeRecord]) -> Result<()> {
            if self.fail_writes {
                return Err(StoreError::Message("disk full".into()));
            }
            self.upserts.push(records.to_vec());
            self.inner.upsert_screen_time(records)
        }
        fn prune_screen_time(&mut self, min_day: u32) -> Result<usize> {
            if self.fail_writes {
                return Err(StoreError::Message("disk full".into()));
            }
            self.prunes.push(min_day);
            self.inner.prune_screen_time(min_day)
        }
    });

    fn srec(day: u32, app: &str, ms: u64) -> ScreenTimeRecord {
        ScreenTimeRecord {
            day,
            app: app.into(),
            ms,
        }
    }

    use krovi_core::screen_time::{retention_cutoff, FLUSH_EVERY_MS};

    const TODAY: u32 = 20_000;

    #[test]
    fn sampling_every_five_seconds_writes_one_batch_a_minute_with_only_changed_rows() {
        let mut spy = Spy::default();
        let mut st = ScreenTime::new();
        let mut now = 1_000_000u64;

        // tick 0: the first change is written at once (nothing has been written yet)
        st.record(TODAY, "code.exe", 5_000);
        st.record(TODAY, "idle-tool.exe", 100);
        sync_screen_time(&mut spy, &mut st, now, TODAY, false).unwrap();
        assert_eq!(spy.upserts.len(), 1);
        assert_eq!(
            spy.upserts[0],
            vec![
                srec(TODAY, "code.exe", 5_000),
                srec(TODAY, "idle-tool.exe", 100)
            ]
        );

        // eleven more 5 s ticks that only touch code.exe: nothing is written...
        for _ in 0..11 {
            now += 5_000;
            st.record(TODAY, "code.exe", 5_000);
            sync_screen_time(&mut spy, &mut st, now, TODAY, false).unwrap();
        }
        assert_eq!(spy.upserts.len(), 1, "no write inside the 60 s window");

        // ...until the minute is up: one write, carrying only the row that changed
        now += 5_000;
        st.record(TODAY, "code.exe", 5_000);
        sync_screen_time(&mut spy, &mut st, now, TODAY, false).unwrap();
        assert_eq!(spy.upserts.len(), 2);
        assert_eq!(spy.upserts[1], vec![srec(TODAY, "code.exe", 5_000 * 13)]);
        assert!(!st.has_dirty());

        // a quiet tick right after writes nothing at all
        now += 5_000;
        sync_screen_time(&mut spy, &mut st, now, TODAY, false).unwrap();
        assert_eq!(spy.upserts.len(), 2);

        // what is stored is what is in memory
        assert_eq!(spy.load_screen_time().unwrap(), screen_time_records(&st));
    }

    #[test]
    fn quitting_writes_what_is_still_waiting_without_waiting_for_the_batch() {
        let mut spy = Spy::default();
        let mut st = ScreenTime::new();
        st.record(TODAY, "a.exe", 10);
        sync_screen_time(&mut spy, &mut st, 0, TODAY, false).unwrap();
        st.record(TODAY, "a.exe", 7); // inside the 60 s window: held back
        sync_screen_time(&mut spy, &mut st, 10_000, TODAY, false).unwrap();
        assert_eq!(
            spy.load_screen_time().unwrap(),
            vec![srec(TODAY, "a.exe", 10)]
        );

        sync_screen_time(&mut spy, &mut st, 11_000, TODAY, true).unwrap(); // quit
        assert_eq!(
            spy.load_screen_time().unwrap(),
            vec![srec(TODAY, "a.exe", 17)]
        );
        assert_eq!(spy.upserts.last().unwrap(), &vec![srec(TODAY, "a.exe", 17)]);

        // forcing with nothing changed writes nothing
        let writes = spy.upserts.len();
        sync_screen_time(&mut spy, &mut st, 12_000, TODAY, true).unwrap();
        assert_eq!(spy.upserts.len(), writes);
    }

    #[test]
    fn data_older_than_ninety_days_is_pruned_on_the_first_sync_after_loading() {
        // a database that has grown past the window
        let mut spy = Spy::default();
        spy.inner
            .save_screen_time(&[
                srec(TODAY - 400, "ancient.exe", 1),
                srec(TODAY - 91, "old.exe", 2),
                srec(TODAY - 89, "edge.exe", 3), // 90th day back: kept
                srec(TODAY, "now.exe", 4),
            ])
            .unwrap();
        let mut st = screen_time_from_records(&spy.load_screen_time().unwrap());
        assert!(!st.has_dirty(), "loading must not make everything dirty");

        sync_screen_time(&mut spy, &mut st, 0, TODAY, false).unwrap();

        assert_eq!(spy.prunes, vec![retention_cutoff(TODAY)]);
        assert_eq!(
            spy.load_screen_time().unwrap(),
            vec![srec(TODAY - 89, "edge.exe", 3), srec(TODAY, "now.exe", 4)]
        );
        assert_eq!(screen_time_records(&st), spy.load_screen_time().unwrap());
        assert!(spy.upserts.is_empty(), "pruning alone writes no rows");
    }

    #[test]
    fn retention_runs_once_per_local_day_not_every_tick() {
        let mut spy = Spy::default();
        let mut st = ScreenTime::new();
        for i in 0..5 {
            sync_screen_time(&mut spy, &mut st, i * 5_000, TODAY, false).unwrap();
        }
        assert_eq!(spy.prunes.len(), 1, "same day: one prune");
        sync_screen_time(&mut spy, &mut st, 90_000, TODAY + 1, false).unwrap();
        assert_eq!(
            spy.prunes,
            vec![retention_cutoff(TODAY), retention_cutoff(TODAY + 1)]
        );
    }

    #[test]
    fn a_failing_store_keeps_the_rows_and_backs_off_instead_of_hammering_it() {
        let mut spy = Spy {
            fail_writes: true,
            ..Spy::default()
        };
        let mut st = ScreenTime::new();
        st.record(TODAY, "a.exe", 10);

        let err = sync_screen_time(&mut spy, &mut st, 0, TODAY, false).unwrap_err();
        assert!(err.to_string().contains("disk full"));
        assert!(st.has_dirty(), "nothing was written, nothing is forgotten");
        assert_eq!(st.app_total(TODAY, "a.exe"), 10);

        // within the back-off the store is not touched again (no error spam either)
        spy.fail_writes = false;
        sync_screen_time(&mut spy, &mut st, FLUSH_EVERY_MS - 1, TODAY, false).unwrap();
        assert!(spy.upserts.is_empty());

        // after the interval the retry succeeds and clears the backlog
        st.record(TODAY, "a.exe", 5);
        sync_screen_time(&mut spy, &mut st, FLUSH_EVERY_MS, TODAY, false).unwrap();
        assert_eq!(
            spy.load_screen_time().unwrap(),
            vec![srec(TODAY, "a.exe", 15)]
        );
        assert!(!st.has_dirty());
    }

    #[test]
    fn sync_works_through_the_apps_locked_boxed_store() {
        // the exact shape `AppState` holds: Mutex<Box<dyn Store + Send>>
        let store: std::sync::Mutex<Box<dyn Store + Send>> =
            std::sync::Mutex::new(Box::new(MemStore::new()));
        let mut st = ScreenTime::new();
        st.record(TODAY, "a.exe", 3);
        {
            let mut guard = store.lock().unwrap();
            sync_screen_time(&mut **guard, &mut st, 0, TODAY, false).unwrap();
        }
        assert_eq!(
            store.lock().unwrap().load_screen_time().unwrap(),
            vec![srec(TODAY, "a.exe", 3)]
        );
    }

    #[test]
    fn a_failed_prune_is_retried_on_the_next_sync() {
        let mut spy = Spy {
            fail_writes: true,
            ..Spy::default()
        };
        let mut st = ScreenTime::new();
        assert!(sync_screen_time(&mut spy, &mut st, 0, TODAY, false).is_err());
        assert!(
            st.needs_prune(TODAY),
            "not marked done: it will be tried again"
        );
        spy.fail_writes = false;
        sync_screen_time(&mut spy, &mut st, 5_000, TODAY, false).unwrap();
        assert!(!st.needs_prune(TODAY));
        assert_eq!(spy.prunes.len(), 1);
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
