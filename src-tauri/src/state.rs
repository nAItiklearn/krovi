//! Shared application state. Owner: live-data agent (see docs/CONTRACTS.md).

use std::sync::Mutex;

use krovi_core::{
    ClipKind, ClipboardHistory, FileShelf, Notes, PasteStack, Pomodoro, ScreenTime, TodoList,
    DEFAULT_SHELF,
};
use krovi_store::MemStore;
use krovi_sys::Sampler;
// ---------------------------------------------------------------------------
// Shared state: the core engines plus the persistence store.
// ---------------------------------------------------------------------------

pub struct AppState {
    pub clipboard: Mutex<ClipboardHistory>,
    pub notes: Mutex<Notes>,
    pub tasks: Mutex<TodoList>,
    pub paste: Mutex<PasteStack>,
    pub shelf: Mutex<FileShelf>,
    pub screen: Mutex<ScreenTime>,
    #[allow(dead_code)]
    pub pomodoro: Mutex<Pomodoro>,
    pub store: Mutex<MemStore>,
    /// Screen-time interval tracker driven by `foreground_tick` (SPEC C11).
    pub sampler: Mutex<Sampler>,
}

impl AppState {
    pub fn seeded() -> Self {
        // A little starter content so the shell isn't empty on first launch.
        let mut clipboard = ClipboardHistory::default();
        clipboard.add(
            ClipKind::Text,
            "Welcome to Krovi â€” your everything shelf",
            0,
        );
        clipboard.add(ClipKind::Link, "https://krovi.tech", 1);

        let mut notes = Notes::new();
        notes.create("First note â€” autosaves locally.", 0);

        let mut tasks = TodoList::new();
        tasks.add("Try the command palette (Ctrl/Cmd+K)", 0);

        let mut shelf = FileShelf::new();
        shelf.add_item(DEFAULT_SHELF, "readme.txt", "blobs/readme", 1024, 0);

        Self {
            clipboard: Mutex::new(clipboard),
            notes: Mutex::new(notes),
            tasks: Mutex::new(tasks),
            paste: Mutex::new(PasteStack::new()),
            shelf: Mutex::new(shelf),
            screen: Mutex::new(ScreenTime::new()),
            pomodoro: Mutex::new(Pomodoro::default()),
            store: Mutex::new(MemStore::new()),
            sampler: Mutex::new(Sampler::new()),
        }
    }
}


