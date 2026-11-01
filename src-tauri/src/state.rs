//! Shared application state. Owner: live-data agent (see docs/CONTRACTS.md).
//!
//! `AppState` starts **empty** (no seeded content, ever — a first run is a real empty
//! state) and is *hydrated* from the SQLite database in `live::setup`, once the app
//! data directory is known. Until then (and as the fallback if the database cannot
//! be opened) the store is an in-memory `MemStore`.
//!
//! Logic worth testing lives in the library crates (`krovi_store::restore_all`,
//! `ClipboardHistory::add_if_new`, ...): this crate's own test binary cannot start on
//! Windows (the `tauri` crate needs the app manifest at load time), so it stays thin.

use std::path::PathBuf;
use std::sync::Mutex;

use krovi_core::{
    Calendar, ClipboardHistory, FileShelf, Notes, PasteStack, Pomodoro, ScreenTime, TodoList,
};
use krovi_store::{MemStore, Store};
use krovi_sys::{ClipGate, MediaState, MediaWorker, Sampler, SystemStats};

/// The persistence backend behind a lock: `SqliteStore` in the app, `MemStore` as the
/// fallback when the database cannot be opened.
pub type DynStore = Box<dyn Store + Send>;

/// Handles and latest values owned by the live hub (`live.rs`).
pub struct LiveShared {
    /// Serializes every touch of the OS clipboard — the watcher's read and a
    /// copy-back's write — and holds the change gate. Lock order is always
    /// `clip_io` -> `clipboard` -> `store`.
    pub clip_io: Mutex<ClipGate>,
    /// Latest media state from the GSMTC worker (position as of `sampled_at`).
    pub media: Mutex<Option<MediaState>>,
    /// The dedicated MTA media thread; transport commands go through it.
    pub media_worker: Mutex<Option<MediaWorker>>,
    /// Latest system stats sample.
    pub stats: Mutex<Option<SystemStats>>,
    /// App data directory (database, clipboard image sidecars).
    pub data_dir: Mutex<Option<PathBuf>>,
    /// Set when the on-disk database could not be opened/read and the app is running
    /// on the in-memory fallback (nothing will survive a restart).
    pub persistence_error: Mutex<Option<String>>,
}

pub struct AppState {
    pub clipboard: Mutex<ClipboardHistory>,
    pub notes: Mutex<Notes>,
    pub tasks: Mutex<TodoList>,
    pub paste: Mutex<PasteStack>,
    pub shelf: Mutex<FileShelf>,
    pub screen: Mutex<ScreenTime>,
    pub calendar: Mutex<Calendar>,
    #[allow(dead_code)]
    pub pomodoro: Mutex<Pomodoro>,
    pub store: Mutex<DynStore>,
    /// Screen-time interval tracker, driven by the 5 s background tick in `live.rs`.
    pub sampler: Mutex<Sampler>,
    pub live: LiveShared,
}

impl AppState {
    /// A completely empty state backed by an in-memory store.
    pub fn new() -> Self {
        Self {
            clipboard: Mutex::new(ClipboardHistory::default()),
            notes: Mutex::new(Notes::new()),
            tasks: Mutex::new(TodoList::new()),
            paste: Mutex::new(PasteStack::new()),
            shelf: Mutex::new(FileShelf::new()),
            screen: Mutex::new(ScreenTime::new()),
            calendar: Mutex::new(Calendar::new()),
            pomodoro: Mutex::new(Pomodoro::default()),
            store: Mutex::new(Box::new(MemStore::new())),
            sampler: Mutex::new(Sampler::new()),
            live: LiveShared {
                clip_io: Mutex::new(ClipGate::new(None)),
                media: Mutex::new(None),
                media_worker: Mutex::new(None),
                stats: Mutex::new(None),
                data_dir: Mutex::new(None),
                persistence_error: Mutex::new(None),
            },
        }
    }

    /// Historical constructor name still referenced by `lib.rs`. It no longer seeds
    /// anything: the result is identical to [`AppState::new`].
    #[doc(hidden)]
    pub fn seeded() -> Self {
        Self::new()
    }

    /// Load every collection from `store` into the in-memory engines, then adopt
    /// `store` as the persistence backend.
    ///
    /// All-or-nothing: if *any* collection fails to load, nothing is adopted and the
    /// error is returned, so a half-read database can never be overwritten with
    /// replace-semantics saves.
    pub fn hydrate(&self, store: DynStore) -> Result<(), String> {
        fn poisoned<T>(_: std::sync::PoisonError<T>) -> String {
            "internal lock error".to_string()
        }
        let r = krovi_store::restore_all(&*store).map_err(|e| e.to_string())?;
        *self.clipboard.lock().map_err(poisoned)? = r.clipboard;
        *self.notes.lock().map_err(poisoned)? = r.notes;
        *self.tasks.lock().map_err(poisoned)? = r.tasks;
        *self.shelf.lock().map_err(poisoned)? = r.shelf;
        *self.screen.lock().map_err(poisoned)? = r.screen;
        *self.calendar.lock().map_err(poisoned)? = r.calendar;
        *self.store.lock().map_err(poisoned)? = store;
        Ok(())
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
