//! In-memory [`Store`] implementation.
//!
//! Zero external dependencies: used as the default backend when SQLite is not
//! compiled in, and by unit tests everywhere. Holds owned clones of each
//! collection, so `save_*` is a plain replace and order is preserved exactly.

use std::collections::BTreeMap;

use crate::{CalendarEvent, ClipEntry, Note, Result, ScreenTimeRecord, Store, StoredShelf, Task};

/// A fully in-memory store. Non-durable: state is lost when dropped.
#[derive(Debug, Default, Clone)]
pub struct MemStore {
    clipboard: Vec<ClipEntry>,
    notes: Vec<Note>,
    tasks: Vec<Task>,
    shelves: Vec<StoredShelf>,
    screen_time: Vec<ScreenTimeRecord>,
    calendar: Vec<CalendarEvent>,
    settings: BTreeMap<String, String>,
}

impl MemStore {
    /// Create an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Store for MemStore {
    fn load_clipboard(&self) -> Result<Vec<ClipEntry>> {
        Ok(self.clipboard.clone())
    }
    fn save_clipboard(&mut self, entries: &[ClipEntry]) -> Result<()> {
        self.clipboard = entries.to_vec();
        Ok(())
    }

    fn load_notes(&self) -> Result<Vec<Note>> {
        Ok(self.notes.clone())
    }
    fn save_notes(&mut self, notes: &[Note]) -> Result<()> {
        self.notes = notes.to_vec();
        Ok(())
    }

    fn load_tasks(&self) -> Result<Vec<Task>> {
        Ok(self.tasks.clone())
    }
    fn save_tasks(&mut self, tasks: &[Task]) -> Result<()> {
        self.tasks = tasks.to_vec();
        Ok(())
    }

    fn load_shelves(&self) -> Result<Vec<StoredShelf>> {
        Ok(self.shelves.clone())
    }
    fn save_shelves(&mut self, shelves: &[StoredShelf]) -> Result<()> {
        self.shelves = shelves.to_vec();
        Ok(())
    }

    fn load_screen_time(&self) -> Result<Vec<ScreenTimeRecord>> {
        Ok(self.screen_time.clone())
    }
    fn save_screen_time(&mut self, records: &[ScreenTimeRecord]) -> Result<()> {
        self.screen_time = records.to_vec();
        Ok(())
    }

    fn load_calendar(&self) -> Result<Vec<CalendarEvent>> {
        Ok(self.calendar.clone())
    }
    fn save_calendar(&mut self, events: &[CalendarEvent]) -> Result<()> {
        self.calendar = events.to_vec();
        Ok(())
    }

    fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self.settings.get(key).cloned())
    }
    fn set_setting(&mut self, key: &str, value: &str) -> Result<()> {
        self.settings.insert(key.to_string(), value.to_string());
        Ok(())
    }
    fn delete_setting(&mut self, key: &str) -> Result<bool> {
        Ok(self.settings.remove(key).is_some())
    }
    fn settings(&self) -> Result<BTreeMap<String, String>> {
        Ok(self.settings.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit;

    #[test]
    fn mem_store_round_trips_everything() {
        let mut s = MemStore::new();
        testkit::exercise(&mut s);
    }

    #[test]
    fn default_is_empty() {
        let s = MemStore::default();
        assert!(s.load_clipboard().unwrap().is_empty());
        assert!(s.settings().unwrap().is_empty());
    }

    #[test]
    fn independent_instances_do_not_share_state() {
        let mut a = MemStore::new();
        a.set_setting("k", "v").unwrap();
        let b = MemStore::new();
        assert_eq!(b.get_setting("k").unwrap(), None);
        assert_eq!(a.get_setting("k").unwrap(), Some("v".to_string()));
    }
}
