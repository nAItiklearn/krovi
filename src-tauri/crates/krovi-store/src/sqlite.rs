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
    ClipEntry, ClipKind, Note, Result, ScreenTimeRecord, ShelfItem, Store, StoreError, StoredShelf,
    Task,
};

/// The current on-disk schema version.
pub const SCHEMA_VERSION: i64 = 1;

/// A SQLite-backed store opened at a caller-provided path.
pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Open (creating if needed) a database file at `path` and run migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
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
             CREATE TABLE IF NOT EXISTS settings (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             COMMIT;",
        )?;

        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))?;
        if count == 0 {
            self.conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![SCHEMA_VERSION],
            )?;
        }
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
