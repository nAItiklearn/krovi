//! SQLite-backed [`Store`] implementation (feature `sqlite`).
//!
//! Persists Krovi's durable state to a `rusqlite` database (SPEC §8: `krovi.db`).
//! Only compiled when the `sqlite` feature is enabled, because the bundled
//! SQLite build requires a C/MSVC compiler.
//!
//! Schema notes:
//! * A single-row `schema_version` table records [`SCHEMA_VERSION`].
//! * Ordered collections carry an explicit `ord` column so `load_*` returns rows
//!   in exactly the order they were saved (SQLite row order is otherwise not
//!   guaranteed across reopen).
//! * `u64`/`u32` values are stored as SQLite `INTEGER` (i64) and cast back on
//!   read; all values in Krovi's models fit comfortably in `i64`.
//! * All writes to a collection run inside a transaction (delete-all + insert),
//!   giving atomic replace semantics.
//! * Every query is parameterised — user content (clip text, note bodies, file
//!   names, settings) never touches SQL string interpolation.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::{
    CalendarEvent, ClipEntry, ClipKind, Note, Result, ScreenTimeRecord, ShelfItem, Store,
    StoreError, StoredShelf, Task,
};

/// The current on-disk schema version. v2 added the `calendar_events` table
/// (migrations are additive `CREATE TABLE IF NOT EXISTS`, so v1 files upgrade in place).
/// v3 added the reminder columns to `calendar_events` (`remind`, `sound`, `fired_at`,
/// `snoozed_until`, `missed`), added in place by `SqliteStore::upgrade_calendar_columns`:
/// existing rows are kept, timed ones become reminders, all-day ones do not.
pub const SCHEMA_VERSION: i64 = 3;

/// Reminder columns added to `calendar_events` in v3, as `(name, definition)`.
const CALENDAR_REMINDER_COLUMNS: [(&str, &str); 5] = [
    ("remind", "INTEGER NOT NULL DEFAULT 1"),
    ("sound", "INTEGER NOT NULL DEFAULT 1"),
    ("fired_at", "INTEGER"),
    ("snoozed_until", "INTEGER"),
    ("missed", "INTEGER NOT NULL DEFAULT 0"),
];

/// A SQLite-backed store opened at a caller-provided path.
pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Open (creating if needed) a database file at `path` and run migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        // WAL keeps writes cheap and crash-safe; NORMAL sync is the documented
        // pairing for WAL (a power cut can lose the last commit, never corrupt).
        // Failures here (e.g. a network drive) are non-fatal: fall back to defaults.
        let _ = conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;");
        Self::from_conn(conn)
    }

    /// Open a private, in-memory database (handy for tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::from_conn(conn)
    }

    fn from_conn(conn: Connection) -> Result<Self> {
        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    /// The schema version recorded in the database.
    pub fn schema_version(&self) -> Result<i64> {
        let v = self
            .conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
                r.get(0)
            })?;
        Ok(v)
    }

    fn migrate(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS clipboard (
                 id INTEGER PRIMARY KEY,
                 kind TEXT NOT NULL,
                 content TEXT NOT NULL,
                 pinned INTEGER NOT NULL,
                 created_at INTEGER NOT NULL,
                 ord INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS notes (
                 id INTEGER PRIMARY KEY,
                 body TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 ord INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS tasks (
                 id INTEGER PRIMARY KEY,
                 title TEXT NOT NULL,
                 done INTEGER NOT NULL,
                 created_at INTEGER NOT NULL,
                 completed_at INTEGER,
                 ord INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS shelves (
                 name TEXT PRIMARY KEY,
                 ord INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS shelf_items (
                 id INTEGER PRIMARY KEY,
                 shelf TEXT NOT NULL,
                 name TEXT NOT NULL,
                 stored_path TEXT NOT NULL,
                 size_bytes INTEGER NOT NULL,
                 added_at INTEGER NOT NULL,
                 ord INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS screen_time (
                 day INTEGER NOT NULL,
                 app TEXT NOT NULL,
                 ms INTEGER NOT NULL,
                 PRIMARY KEY (day, app)
             );
             CREATE TABLE IF NOT EXISTS calendar_events (
                 id INTEGER PRIMARY KEY,
                 date TEXT NOT NULL,
                 title TEXT NOT NULL,
                 time TEXT,
                 ord INTEGER NOT NULL,
                 remind INTEGER NOT NULL DEFAULT 1,
                 sound INTEGER NOT NULL DEFAULT 1,
                 fired_at INTEGER,
                 snoozed_until INTEGER,
                 missed INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS settings (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             COMMIT;",
        )?;
        self.upgrade_calendar_columns()?;

        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))?;
        if count == 0 {
            self.conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![SCHEMA_VERSION],
            )?;
        } else {
            // Upgrade an older file in place (tables above are additive).
            self.conn.execute(
                "UPDATE schema_version SET version = ?1 WHERE version < ?1",
                params![SCHEMA_VERSION],
            )?;
        }
        Ok(())
    }

    /// Add the v3 reminder columns to a `calendar_events` table that predates them.
    ///
    /// `ALTER TABLE .. ADD COLUMN` is not idempotent, so the existing columns are read
    /// first and only the missing ones are added, all inside one transaction (a crash
    /// halfway leaves the old table and the next open simply retries). Rows that already
    /// exist are kept: the ones with a time become reminders (`remind = 1`) so the alerts
    /// the old version silently never rang now work, all-day rows get no alert.
    fn upgrade_calendar_columns(&mut self) -> Result<()> {
        let have: Vec<String> = {
            let mut stmt = self.conn.prepare("PRAGMA table_info(calendar_events)")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let missing: Vec<_> = CALENDAR_REMINDER_COLUMNS
            .iter()
            .filter(|(name, _)| !have.iter().any(|h| h == name))
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        for (name, def) in &missing {
            tx.execute_batch(&format!(
                "ALTER TABLE calendar_events ADD COLUMN {name} {def};"
            ))?;
        }
        if missing.iter().any(|(name, _)| *name == "remind") {
            tx.execute_batch(
                "UPDATE calendar_events
                 SET remind = CASE WHEN time IS NULL OR time = '' THEN 0 ELSE 1 END;",
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

fn kind_str(kind: ClipKind) -> &'static str {
    match kind {
        ClipKind::Text => "text",
        ClipKind::Image => "image",
        ClipKind::Link => "link",
    }
}

fn kind_from(s: &str) -> Result<ClipKind> {
    match s {
        "text" => Ok(ClipKind::Text),
        "image" => Ok(ClipKind::Image),
        "link" => Ok(ClipKind::Link),
        other => Err(StoreError::Message(format!("unknown clip kind: {other:?}"))),
    }
}

impl Store for SqliteStore {
    fn load_clipboard(&self) -> Result<Vec<ClipEntry>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, kind, content, pinned, created_at FROM clipboard ORDER BY ord")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, kind, content, pinned, created_at) = row?;
            out.push(ClipEntry {
                id: id as u64,
                kind: kind_from(&kind)?,
                content,
                pinned: pinned != 0,
                created_at: created_at as u64,
            });
        }
        Ok(out)
    }

    fn save_clipboard(&mut self, entries: &[ClipEntry]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM clipboard", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO clipboard (id, kind, content, pinned, created_at, ord)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (i, e) in entries.iter().enumerate() {
                stmt.execute(params![
                    e.id as i64,
                    kind_str(e.kind),
                    e.content,
                    e.pinned as i64,
                    e.created_at as i64,
                    i as i64
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn load_notes(&self) -> Result<Vec<Note>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, body, created_at, updated_at FROM notes ORDER BY ord")?;
        let rows = stmt.query_map([], |r| {
            Ok(Note {
                id: r.get::<_, i64>(0)? as u64,
                body: r.get::<_, String>(1)?,
                created_at: r.get::<_, i64>(2)? as u64,
                updated_at: r.get::<_, i64>(3)? as u64,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn save_notes(&mut self, notes: &[Note]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM notes", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO notes (id, body, created_at, updated_at, ord)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (i, n) in notes.iter().enumerate() {
                stmt.execute(params![
                    n.id as i64,
                    n.body,
                    n.created_at as i64,
                    n.updated_at as i64,
                    i as i64
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn load_tasks(&self) -> Result<Vec<Task>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, title, done, created_at, completed_at FROM tasks ORDER BY ord")?;
        let rows = stmt.query_map([], |r| {
            let completed: Option<i64> = r.get(4)?;
            Ok(Task {
                id: r.get::<_, i64>(0)? as u64,
                title: r.get::<_, String>(1)?,
                done: r.get::<_, i64>(2)? != 0,
                created_at: r.get::<_, i64>(3)? as u64,
                completed_at: completed.map(|v| v as u64),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn save_tasks(&mut self, tasks: &[Task]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM tasks", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO tasks (id, title, done, created_at, completed_at, ord)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (i, t) in tasks.iter().enumerate() {
                stmt.execute(params![
                    t.id as i64,
                    t.title,
                    t.done as i64,
                    t.created_at as i64,
                    t.completed_at.map(|v| v as i64),
                    i as i64
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn load_shelves(&self) -> Result<Vec<StoredShelf>> {
        let names: Vec<String> = {
            let mut stmt = self.conn.prepare("SELECT name FROM shelves ORDER BY ord")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let mut out = Vec::with_capacity(names.len());
        let mut item_stmt = self.conn.prepare(
            "SELECT id, name, stored_path, size_bytes, added_at
             FROM shelf_items WHERE shelf = ?1 ORDER BY ord",
        )?;
        for name in names {
            let items = item_stmt
                .query_map(params![name], |r| {
                    Ok(ShelfItem {
                        id: r.get::<_, i64>(0)? as u64,
                        name: r.get::<_, String>(1)?,
                        stored_path: r.get::<_, String>(2)?,
                        size_bytes: r.get::<_, i64>(3)? as u64,
                        added_at: r.get::<_, i64>(4)? as u64,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            out.push(StoredShelf { name, items });
        }
        Ok(out)
    }

    fn save_shelves(&mut self, shelves: &[StoredShelf]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM shelf_items", [])?;
        tx.execute("DELETE FROM shelves", [])?;
        {
            let mut shelf_stmt = tx.prepare("INSERT INTO shelves (name, ord) VALUES (?1, ?2)")?;
            let mut item_stmt = tx.prepare(
                "INSERT INTO shelf_items (id, shelf, name, stored_path, size_bytes, added_at, ord)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for (si, shelf) in shelves.iter().enumerate() {
                shelf_stmt.execute(params![shelf.name, si as i64])?;
                for (ii, item) in shelf.items.iter().enumerate() {
                    item_stmt.execute(params![
                        item.id as i64,
                        shelf.name,
                        item.name,
                        item.stored_path,
                        item.size_bytes as i64,
                        item.added_at as i64,
                        ii as i64
                    ])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn load_screen_time(&self) -> Result<Vec<ScreenTimeRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT day, app, ms FROM screen_time ORDER BY day, app")?;
        let rows = stmt.query_map([], |r| {
            Ok(ScreenTimeRecord {
                day: r.get::<_, i64>(0)? as u32,
                app: r.get::<_, String>(1)?,
                ms: r.get::<_, i64>(2)? as u64,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn save_screen_time(&mut self, records: &[ScreenTimeRecord]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM screen_time", [])?;
        {
            let mut stmt =
                tx.prepare("INSERT INTO screen_time (day, app, ms) VALUES (?1, ?2, ?3)")?;
            for r in records {
                stmt.execute(params![r.day as i64, r.app, r.ms as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn load_calendar(&self) -> Result<Vec<CalendarEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, date, title, time, remind, sound, fired_at, snoozed_until, missed
             FROM calendar_events ORDER BY ord",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(CalendarEvent {
                id: r.get::<_, i64>(0)? as u64,
                date: r.get::<_, String>(1)?,
                title: r.get::<_, String>(2)?,
                time: r.get::<_, Option<String>>(3)?,
                remind: r.get::<_, i64>(4)? != 0,
                sound: r.get::<_, i64>(5)? != 0,
                fired_at: r.get::<_, Option<i64>>(6)?.map(|v| v as u64),
                snoozed_until: r.get::<_, Option<i64>>(7)?.map(|v| v as u64),
                missed: r.get::<_, i64>(8)? != 0,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn save_calendar(&mut self, events: &[CalendarEvent]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM calendar_events", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO calendar_events
                     (id, date, title, time, ord, remind, sound, fired_at, snoozed_until, missed)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            for (i, e) in events.iter().enumerate() {
                stmt.execute(params![
                    e.id as i64,
                    e.date,
                    e.title,
                    e.time,
                    i as i64,
                    e.remind as i64,
                    e.sound as i64,
                    e.fired_at.map(|v| v as i64),
                    e.snoozed_until.map(|v| v as i64),
                    e.missed as i64
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let v = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(v)
    }

    fn set_setting(&mut self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    fn delete_setting(&mut self, key: &str) -> Result<bool> {
        let n = self
            .conn
            .execute("DELETE FROM settings WHERE key = ?1", params![key])?;
        Ok(n > 0)
    }

    fn settings(&self) -> Result<BTreeMap<String, String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM settings ORDER BY key")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut map = BTreeMap::new();
        for row in rows {
            let (k, v) = row?;
            map.insert(k, v);
        }
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit;

    fn temp_db_path() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("krovi_store_test_{}_{}.db", std::process::id(), n));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn in_memory_round_trips_everything() {
        let mut s = SqliteStore::open_in_memory().unwrap();
        testkit::exercise(&mut s);
    }

    #[test]
    fn migrate_sets_schema_version() {
        let s = SqliteStore::open_in_memory().unwrap();
        assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn reopening_a_file_preserves_all_data() {
        let path = temp_db_path();
        {
            let mut s = SqliteStore::open(&path).unwrap();
            testkit::seed(&mut s);
        } // dropped: connection closed, data flushed to disk
        {
            let s = SqliteStore::open(&path).unwrap();
            // migrate() on an existing db must not wipe or double-count.
            assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
            testkit::assert_seeded(&s);
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn v1_file_upgrades_in_place_to_current_schema() {
        let path = temp_db_path();
        {
            // Simulate a v1 file: schema_version = 1 and no calendar table.
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE schema_version (version INTEGER NOT NULL);
                 INSERT INTO schema_version (version) VALUES (1);
                 CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO settings VALUES ('keep', 'me');",
            )
            .unwrap();
        }
        {
            let mut s = SqliteStore::open(&path).unwrap();
            assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
            assert_eq!(s.get_setting("keep").unwrap(), Some("me".to_string()));
            s.save_calendar(&testkit::sample_calendar()).unwrap();
        }
        {
            let s = SqliteStore::open(&path).unwrap();
            assert_eq!(s.load_calendar().unwrap(), testkit::sample_calendar());
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    fn remove_db_files(path: &std::path::Path) {
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
        }
    }

    #[test]
    fn v2_calendar_rows_gain_reminder_columns_in_place_and_keep_their_data() {
        let path = temp_db_path();
        {
            // A real v2 file: the calendar table as shipped before reminders existed.
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE schema_version (version INTEGER NOT NULL);
                 INSERT INTO schema_version (version) VALUES (2);
                 CREATE TABLE calendar_events (
                     id INTEGER PRIMARY KEY,
                     date TEXT NOT NULL,
                     title TEXT NOT NULL,
                     time TEXT,
                     ord INTEGER NOT NULL
                 );
                 INSERT INTO calendar_events VALUES (1, '2026-11-01', 'Dentist', '09:30', 0);
                 INSERT INTO calendar_events VALUES (2, '2026-11-02', 'Holiday', NULL, 1);
                 INSERT INTO calendar_events VALUES (3, '2026-11-03', 'Blank time', '', 2);",
            )
            .unwrap();
        }
        {
            let s = SqliteStore::open(&path).unwrap();
            assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
            let events = s.load_calendar().unwrap();
            assert_eq!(events.len(), 3);
            // data kept
            assert_eq!(events[0].title, "Dentist");
            assert_eq!(events[0].time.as_deref(), Some("09:30"));
            // timed rows become reminders (sound on, not yet fired); all-day rows do not
            assert!(events[0].remind && events[0].sound);
            assert_eq!(
                (
                    events[0].fired_at,
                    events[0].snoozed_until,
                    events[0].missed
                ),
                (None, None, false)
            );
            assert!(!events[1].remind, "all-day has no alert");
            assert!(!events[2].remind, "an empty time is all-day");
        }
        {
            // opening again must be a no-op (no duplicate-column error, nothing reset)
            let mut s = SqliteStore::open(&path).unwrap();
            let mut events = s.load_calendar().unwrap();
            events[0].fired_at = Some(1_700_000_000_000);
            events[0].missed = true;
            events[0].sound = false;
            events[1].snoozed_until = Some(42);
            s.save_calendar(&events).unwrap();
        }
        {
            let s = SqliteStore::open(&path).unwrap();
            let events = s.load_calendar().unwrap();
            assert_eq!(events[0].fired_at, Some(1_700_000_000_000));
            assert!(events[0].missed && !events[0].sound);
            assert_eq!(events[1].snoozed_until, Some(42));
            assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
        }
        remove_db_files(&path);
    }

    #[test]
    fn a_half_upgraded_calendar_table_is_completed() {
        // e.g. an interrupted earlier attempt left only some of the columns behind
        let path = temp_db_path();
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE schema_version (version INTEGER NOT NULL);
                 INSERT INTO schema_version (version) VALUES (2);
                 CREATE TABLE calendar_events (
                     id INTEGER PRIMARY KEY, date TEXT NOT NULL, title TEXT NOT NULL,
                     time TEXT, ord INTEGER NOT NULL, sound INTEGER NOT NULL DEFAULT 1
                 );
                 INSERT INTO calendar_events VALUES (1, '2026-11-01', 'A', '10:00', 0, 0);",
            )
            .unwrap();
        }
        let s = SqliteStore::open(&path).unwrap();
        let events = s.load_calendar().unwrap();
        assert_eq!(events.len(), 1);
        assert!(!events[0].sound, "an existing column keeps its value");
        assert!(events[0].remind);
        drop(s);
        remove_db_files(&path);
    }

    #[test]
    fn unknown_clip_kind_surfaces_as_error() {
        let s = SqliteStore::open_in_memory().unwrap();
        s.conn
            .execute(
                "INSERT INTO clipboard (id, kind, content, pinned, created_at, ord)
                 VALUES (1, 'bogus', 'x', 0, 0, 0)",
                [],
            )
            .unwrap();
        let err = s.load_clipboard().unwrap_err();
        assert!(format!("{err}").contains("unknown clip kind"));
    }

    #[test]
    fn content_with_sql_metacharacters_is_safe() {
        let mut s = SqliteStore::open_in_memory().unwrap();
        let nasty = ClipEntry {
            id: 1,
            kind: ClipKind::Text,
            content: "Robert'); DROP TABLE clipboard;--".into(),
            pinned: false,
            created_at: 1,
        };
        s.save_clipboard(std::slice::from_ref(&nasty)).unwrap();
        assert_eq!(s.load_clipboard().unwrap(), vec![nasty]);
    }
}
