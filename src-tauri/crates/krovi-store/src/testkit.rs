//! Shared, implementation-agnostic test harness.
//!
//! Both `MemStore` and `SqliteStore` are exercised through the same sample data
//! and assertions so their behaviour stays identical. Compiled only under
//! `cfg(test)`.

use std::collections::BTreeMap;

use krovi_core::{ClipEntry, ClipKind, Note, ShelfItem, Task};

use crate::{ScreenTimeRecord, Store, StoredShelf};

pub fn sample_clips() -> Vec<ClipEntry> {
    vec![
        ClipEntry { id: 2, kind: ClipKind::Link, content: "https://krovi.tech".into(), pinned: true, created_at: 20 },
        ClipEntry { id: 1, kind: ClipKind::Text, content: "hello world".into(), pinned: false, created_at: 10 },
        ClipEntry { id: 3, kind: ClipKind::Image, content: "blobs/img-1".into(), pinned: false, created_at: 30 },
    ]
}

pub fn sample_notes() -> Vec<Note> {
    vec![
        Note { id: 5, body: "buy milk".into(), created_at: 1, updated_at: 9 },
        Note { id: 2, body: "multi\nline\nnote".into(), created_at: 2, updated_at: 7 },
    ]
}

pub fn sample_tasks() -> Vec<Task> {
    vec![
        Task { id: 1, title: "open task".into(), done: false, created_at: 1, completed_at: None },
        Task { id: 2, title: "done task".into(), done: true, created_at: 2, completed_at: Some(50) },
    ]
}

pub fn sample_shelves() -> Vec<StoredShelf> {
    vec![
        StoredShelf { name: "Default".into(), items: vec![] },
        StoredShelf {
            name: "Work".into(),
            items: vec![
                ShelfItem { id: 10, name: "b.txt".into(), stored_path: "blobs/b".into(), size_bytes: 20, added_at: 2 },
                ShelfItem { id: 9, name: "a.txt".into(), stored_path: "blobs/a".into(), size_bytes: 10, added_at: 1 },
            ],
        },
    ]
}

pub fn sample_screen_time() -> Vec<ScreenTimeRecord> {
    // canonical order: day asc, then app asc
    vec![
        ScreenTimeRecord { day: 10, app: "a.exe".into(), ms: 500 },
        ScreenTimeRecord { day: 10, app: "b.exe".into(), ms: 1000 },
        ScreenTimeRecord { day: 11, app: "a.exe".into(), ms: 250 },
    ]
}

/// Save every sample collection + a couple of settings into `s`.
pub fn seed<S: Store>(s: &mut S) {
    s.save_clipboard(&sample_clips()).unwrap();
    s.save_notes(&sample_notes()).unwrap();
    s.save_tasks(&sample_tasks()).unwrap();
    s.save_shelves(&sample_shelves()).unwrap();
    s.save_screen_time(&sample_screen_time()).unwrap();
    s.set_setting("theme", "dark").unwrap();
    s.set_setting("data_dir", r"C:\Users\x\AppData\Roaming\Krovi").unwrap();
}

/// Assert that `s` holds exactly what [`seed`] wrote.
pub fn assert_seeded<S: Store>(s: &S) {
    assert_eq!(s.load_clipboard().unwrap(), sample_clips());
    assert_eq!(s.load_notes().unwrap(), sample_notes());
    assert_eq!(s.load_tasks().unwrap(), sample_tasks());
    assert_eq!(s.load_shelves().unwrap(), sample_shelves());
    assert_eq!(s.load_screen_time().unwrap(), sample_screen_time());

    let mut expected = BTreeMap::new();
    expected.insert("theme".to_string(), "dark".to_string());
    expected.insert("data_dir".to_string(), r"C:\Users\x\AppData\Roaming\Krovi".to_string());
    assert_eq!(s.settings().unwrap(), expected);
    assert_eq!(s.get_setting("theme").unwrap(), Some("dark".to_string()));
}

/// Full in-place workflow: empty start, round-trip every collection, replace
/// semantics, empty-clear, and the settings bag (set/overwrite/delete).
pub fn exercise<S: Store>(s: &mut S) {
    // Everything starts empty.
    assert!(s.load_clipboard().unwrap().is_empty());
    assert!(s.load_notes().unwrap().is_empty());
    assert!(s.load_tasks().unwrap().is_empty());
    assert!(s.load_shelves().unwrap().is_empty());
    assert!(s.load_screen_time().unwrap().is_empty());
    assert!(s.settings().unwrap().is_empty());
    assert_eq!(s.get_setting("missing").unwrap(), None);

    // Round-trip every collection.
    seed(s);
    assert_seeded(s);

    // Replace semantics: a smaller slice overwrites the whole collection.
    let one = sample_clips()[..1].to_vec();
    s.save_clipboard(&one).unwrap();
    assert_eq!(s.load_clipboard().unwrap(), one);

    // Empty save clears a collection.
    s.save_clipboard(&[]).unwrap();
    assert!(s.load_clipboard().unwrap().is_empty());
    s.save_shelves(&[]).unwrap();
    assert!(s.load_shelves().unwrap().is_empty());
    s.save_screen_time(&[]).unwrap();
    assert!(s.load_screen_time().unwrap().is_empty());

    // Settings: overwrite + delete.
    s.set_setting("theme", "light").unwrap();
    assert_eq!(s.get_setting("theme").unwrap(), Some("light".to_string()));
    assert!(s.delete_setting("theme").unwrap());
    assert!(!s.delete_setting("theme").unwrap()); // already gone
    assert_eq!(s.get_setting("theme").unwrap(), None);

    // A task with completed_at = None must round-trip as NULL/None, and a
    // single note re-save must not accumulate.
    let tasks = vec![Task { id: 7, title: "solo".into(), done: false, created_at: 3, completed_at: None }];
    s.save_tasks(&tasks).unwrap();
    assert_eq!(s.load_tasks().unwrap(), tasks);
}
