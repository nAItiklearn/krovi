//! Rebuilding the in-memory `krovi-core` engines from a [`Store`] at startup.
//!
//! The Tauri layer keeps live engines (clipboard history, notes, tasks, ...) and
//! writes whole collections back after each mutation. On launch it calls
//! [`restore_all`] once to turn what is on disk back into those engines, with every
//! id counter continuing *after* the restored ids so a new copy can never collide
//! with a persisted one.
//!
//! All-or-nothing: if any collection cannot be read, the whole restore fails and the
//! caller must keep running on its previous (empty) state **without adopting the
//! store**, so replace-semantics saves can never overwrite data that merely failed
//! to load.

use krovi_core::{Calendar, ClipboardHistory, FileShelf, Notes, ScreenTime, TodoList, DEFAULT_CAP};

use crate::{screen_time_from_records, Result, Store, StoreError};

/// Everything restored from a store.
#[derive(Debug, Clone)]
pub struct Restored {
    pub clipboard: ClipboardHistory,
    pub notes: Notes,
    pub tasks: TodoList,
    pub shelf: FileShelf,
    pub screen: ScreenTime,
    pub calendar: Calendar,
}

fn ctx<T>(what: &str, r: Result<T>) -> Result<T> {
    r.map_err(|e| StoreError::Message(format!("loading {what}: {e}")))
}

/// Load every collection from `store` into fresh engines.
pub fn restore_all(store: &dyn Store) -> Result<Restored> {
    let clips = ctx("clipboard", store.load_clipboard())?;
    let notes = ctx("notes", store.load_notes())?;
    let tasks = ctx("tasks", store.load_tasks())?;
    let shelves = ctx("shelves", store.load_shelves())?;
    let screen = ctx("screen time", store.load_screen_time())?;
    let calendar = ctx("calendar", store.load_calendar())?;
    Ok(Restored {
        clipboard: ClipboardHistory::from_entries(DEFAULT_CAP, clips),
        notes: Notes::from_notes(notes),
        tasks: TodoList::from_tasks(tasks),
        shelf: FileShelf::from_parts(shelves.into_iter().map(|s| (s.name, s.items)).collect()),
        screen: screen_time_from_records(&screen),
        calendar: Calendar::from_events(calendar),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{screen_time_records, MemStore};
    use krovi_core::{ClipKind, Note, Task};

    /// Save the engines the way the Tauri layer does after each mutation.
    fn save_engines<S: Store>(
        s: &mut S,
        h: &ClipboardHistory,
        n: &Notes,
        t: &TodoList,
        c: &Calendar,
        st: &ScreenTime,
    ) {
        let clips: Vec<_> = h.visible().into_iter().cloned().collect();
        s.save_clipboard(&clips).unwrap();
        s.save_notes(n.list()).unwrap();
        s.save_tasks(t.tasks()).unwrap();
        s.save_calendar(c.list()).unwrap();
        s.save_screen_time(&screen_time_records(st)).unwrap();
    }

    fn engines() -> (ClipboardHistory, Notes, TodoList, Calendar, ScreenTime) {
        let mut h = ClipboardHistory::default();
        h.add(ClipKind::Text, "first copy", 10);
        let pinned = h.add(ClipKind::Link, "https://krovi.tech", 20);
        h.pin(pinned);
        h.add(ClipKind::Image, "data:image/png;base64,AAAA", 30);
        let mut n = Notes::new();
        n.create("a note", 11);
        n.create("another\nmultiline note", 12);
        let mut t = TodoList::new();
        let id = t.add("ship it", 13);
        t.add("then rest", 14);
        t.toggle(id, 15);
        let mut c = Calendar::new();
        c.add("2026-11-01", "Standup", Some("09:30")).unwrap();
        c.add("2026-11-01", "All day thing", None).unwrap();
        let mut st = ScreenTime::new();
        st.record(20_000, "code.exe", 90_000);
        st.record(20_000, "chrome.exe", 30_000);
        (h, n, t, c, st)
    }

    fn assert_restored(r: &Restored) {
        let order: Vec<_> = r
            .clipboard
            .visible()
            .iter()
            .map(|e| e.content.clone())
            .collect();
        assert_eq!(
            order,
            vec![
                "https://krovi.tech",
                "data:image/png;base64,AAAA",
                "first copy"
            ]
        );
        assert!(r.clipboard.visible()[0].pinned);
        assert_eq!(r.clipboard.newest().unwrap().kind, ClipKind::Image);
        let notes: Vec<_> = r
            .notes
            .list()
            .iter()
            .map(|n: &Note| n.body.clone())
            .collect();
        assert_eq!(notes, vec!["another\nmultiline note", "a note"]);
        let tasks: Vec<_> = r
            .tasks
            .tasks()
            .iter()
            .map(|t: &Task| (t.title.clone(), t.done))
            .collect();
        assert_eq!(
            tasks,
            vec![
                ("ship it".to_string(), true),
                ("then rest".to_string(), false)
            ]
        );
        assert_eq!(r.tasks.tasks()[0].completed_at, Some(15));
        let events: Vec<_> = r.calendar.list().iter().map(|e| e.title.clone()).collect();
        assert_eq!(events, vec!["All day thing", "Standup"]);
        assert_eq!(r.screen.app_total(20_000, "code.exe"), 90_000);
        assert_eq!(r.screen.day_total(20_000), 120_000);
    }

    #[test]
    fn an_empty_store_restores_empty_engines() {
        let r = restore_all(&MemStore::new()).unwrap();
        assert!(r.clipboard.is_empty() && r.notes.is_empty() && r.tasks.is_empty());
        assert!(r.calendar.is_empty());
        assert_eq!(r.shelf.total_items(), 0);
        assert!(r.screen.days().is_empty());
    }

    #[test]
    fn engines_survive_a_save_and_restore_and_ids_continue() {
        let (h, n, t, c, st) = engines();
        let mut store = MemStore::new();
        save_engines(&mut store, &h, &n, &t, &c, &st);
        let mut r = restore_all(&store).unwrap();
        assert_restored(&r);

        // fresh ids continue after the restored ones: no collision with persisted rows
        let max_clip = h.visible().iter().map(|e| e.id).max().unwrap();
        assert!(r.clipboard.add(ClipKind::Text, "after restart", 99) > max_clip);
        assert!(r.notes.create("n", 99) > n.list().iter().map(|x| x.id).max().unwrap());
        assert!(r.tasks.add("t", 99) > t.tasks().iter().map(|x| x.id).max().unwrap());
        assert!(
            r.calendar.add("2026-12-01", "e", None).unwrap()
                > c.list().iter().map(|x| x.id).max().unwrap()
        );
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn data_survives_a_real_process_restart_via_the_sqlite_file() {
        use crate::SqliteStore;
        let mut path = std::env::temp_dir();
        path.push(format!("krovi_restore_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);

        // "first run": mutate the engines, saving after each change like the app does.
        {
            let mut s = SqliteStore::open(&path).unwrap();
            let (h, n, t, c, st) = engines();
            save_engines(&mut s, &h, &n, &t, &c, &st);
        } // connection dropped = process exit

        // "second run": a brand-new connection, nothing in memory.
        {
            let s = SqliteStore::open(&path).unwrap();
            let r = restore_all(&s).unwrap();
            assert_restored(&r);
        }
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
        }
    }

    #[test]
    fn a_collection_that_fails_to_load_fails_the_whole_restore_with_context() {
        struct BadClips(MemStore);
        impl Store for BadClips {
            fn load_clipboard(&self) -> Result<Vec<krovi_core::ClipEntry>> {
                Err(StoreError::Message("corrupt row".into()))
            }
            fn save_clipboard(&mut self, e: &[krovi_core::ClipEntry]) -> Result<()> {
                self.0.save_clipboard(e)
            }
            fn load_notes(&self) -> Result<Vec<Note>> {
                self.0.load_notes()
            }
            fn save_notes(&mut self, n: &[Note]) -> Result<()> {
                self.0.save_notes(n)
            }
            fn load_tasks(&self) -> Result<Vec<Task>> {
                self.0.load_tasks()
            }
            fn save_tasks(&mut self, t: &[Task]) -> Result<()> {
                self.0.save_tasks(t)
            }
            fn load_shelves(&self) -> Result<Vec<crate::StoredShelf>> {
                self.0.load_shelves()
            }
            fn save_shelves(&mut self, s: &[crate::StoredShelf]) -> Result<()> {
                self.0.save_shelves(s)
            }
            fn load_screen_time(&self) -> Result<Vec<crate::ScreenTimeRecord>> {
                self.0.load_screen_time()
            }
            fn save_screen_time(&mut self, r: &[crate::ScreenTimeRecord]) -> Result<()> {
                self.0.save_screen_time(r)
            }
            fn load_calendar(&self) -> Result<Vec<krovi_core::CalendarEvent>> {
                self.0.load_calendar()
            }
            fn save_calendar(&mut self, e: &[krovi_core::CalendarEvent]) -> Result<()> {
                self.0.save_calendar(e)
            }
            fn get_setting(&self, k: &str) -> Result<Option<String>> {
                self.0.get_setting(k)
            }
            fn set_setting(&mut self, k: &str, v: &str) -> Result<()> {
                self.0.set_setting(k, v)
            }
            fn delete_setting(&mut self, k: &str) -> Result<bool> {
                self.0.delete_setting(k)
            }
            fn settings(&self) -> Result<std::collections::BTreeMap<String, String>> {
                self.0.settings()
            }
        }
        let err = restore_all(&BadClips(MemStore::new()))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("loading clipboard") && err.contains("corrupt row"),
            "{err}"
        );
    }
}
