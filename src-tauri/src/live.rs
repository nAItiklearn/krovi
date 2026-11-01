//! Live hub (owner: live-data). Background watchers that push events to the UI:
//! `clipboard-changed`, `media-changed`, `stats-tick` — see docs/CONTRACTS.md §2.
//!
//! * **Clipboard**: a thread polls the OS clipboard sequence number every 250 ms and
//!   ingests real changes (text / link / file list / image thumbnail), skipping
//!   anything a password manager flagged. Copy-back (`clipboard_copy`) writes under
//!   the same lock and acknowledges the new token, so it is never re-ingested.
//! * **Media**: the GSMTC worker (`krovi_sys::MediaWorker`) runs on its own MTA thread
//!   and calls back into this hub with each meaningful change.
//! * **Stats**: a thread samples RAM / system drive / CPU every 2 s.
//! * **Screen time**: a 5 s tick samples the foreground process, independent of the UI.
//! * **Persistence**: the SQLite database in the app data dir is opened and loaded
//!   here, at startup, before any thread starts.
//!
//! The UI never polls these: it listens for the events and seeds from `live_snapshot`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use krovi_core::ClipKind;
use krovi_store::wire::ClipChanged;
use krovi_sys::{
    CaptureKind, ClipCapture, ClipGate, MediaState, MediaWorker, ReadOutcome, StatsSampler,
    SystemStats, TransportCommand,
};

use crate::commands::{
    clipboard_dtos, lock_err, now_ms, persist_clipboard, tick_screen_time, ClipDto, CmdResult,
};
use crate::state::AppState;

const CLIP_POLL: Duration = Duration::from_millis(250);
const STATS_EVERY: Duration = Duration::from_millis(2000);
const SCREEN_TIME_EVERY: Duration = Duration::from_millis(5000);

/// Start persistence and every background watcher. Runs in the Tauri setup hook, so
/// the state is hydrated before the first command can be served.
pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    open_database(&handle);
    spawn_clipboard_watcher(handle.clone());
    spawn_media(handle.clone());
    spawn_stats(handle.clone());
    spawn_screen_time(handle);
    Ok(())
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

fn record_persistence_failure(state: &AppState, why: String) {
    eprintln!("[krovi] persistence unavailable, running in memory only: {why}");
    if let Ok(mut slot) = state.live.persistence_error.lock() {
        *slot = Some(why);
    }
}

fn open_database(app: &AppHandle) {
    let state = app.state::<AppState>();
    let dir = match app.path().app_data_dir() {
        Ok(d) => d,
        Err(e) => return record_persistence_failure(&state, format!("no app data dir: {e}")),
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return record_persistence_failure(&state, format!("creating {}: {e}", dir.display()));
    }
    if let Ok(mut slot) = state.live.data_dir.lock() {
        *slot = Some(dir.clone());
    }
    open_sqlite(&state, &dir.join("krovi.db"));
    prune_clip_images(app);
}

#[cfg(feature = "sqlite")]
fn open_sqlite(state: &AppState, path: &Path) {
    let result = krovi_store::SqliteStore::open(path)
        .map_err(|e| format!("opening {}: {e}", path.display()))
        .and_then(|store| state.hydrate(Box::new(store)));
    if let Err(why) = result {
        record_persistence_failure(state, why);
    }
}

#[cfg(not(feature = "sqlite"))]
fn open_sqlite(state: &AppState, _path: &Path) {
    record_persistence_failure(state, "built without the `sqlite` feature".into());
}

// ---------------------------------------------------------------------------
// Clipboard images: a full-resolution PNG sidecar per image clip, so copy-back is
// lossless even though the history row only holds a small thumbnail.
// ---------------------------------------------------------------------------

use krovi_sys::imaging::{is_sidecar_name, sidecar_name};

fn images_dir(app: &AppHandle) -> Option<PathBuf> {
    let state = app.state::<AppState>();
    let dir = state.live.data_dir.lock().ok()?.clone()?;
    Some(dir.join("clipimg"))
}

fn store_clip_image(app: &AppHandle, content: &str, png: &[u8]) {
    let Some(dir) = images_dir(app) else { return };
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join(sidecar_name(content)), png);
    }
}

/// The best PNG for an image clip: its full-resolution sidecar, else the thumbnail
/// carried in the clip's own `data:` URL.
fn load_clip_png(app: &AppHandle, content: &str) -> Option<Vec<u8>> {
    if let Some(dir) = images_dir(app) {
        if let Ok(bytes) = std::fs::read(dir.join(sidecar_name(content))) {
            return Some(bytes);
        }
    }
    krovi_sys::imaging::parse_data_url(content).map(|(_, bytes)| bytes)
}

/// Delete sidecars no longer referenced by any image clip (evicted / removed / cleared).
pub fn prune_clip_images(app: &AppHandle) {
    let Some(dir) = images_dir(app) else { return };
    let keep: std::collections::HashSet<String> = {
        let state = app.state::<AppState>();
        let Ok(h) = state.clipboard.lock() else {
            return;
        };
        h.visible()
            .into_iter()
            .filter(|e| e.kind == ClipKind::Image)
            .map(|e| sidecar_name(&e.content))
            .collect()
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_sidecar_name(&name) && !keep.contains(&name) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

// ---------------------------------------------------------------------------
// Clipboard events
// ---------------------------------------------------------------------------

/// Emit `clipboard-changed` (`{ entries, newId }` — see `krovi_store::wire::ClipChanged`).
pub fn emit_clipboard(app: &AppHandle, entries: &[ClipDto], new_id: Option<u64>) {
    let _ = app.emit("clipboard-changed", ClipChanged { entries, new_id });
}

fn to_clip_kind(k: CaptureKind) -> ClipKind {
    match k {
        CaptureKind::Text => ClipKind::Text,
        CaptureKind::Link => ClipKind::Link,
        CaptureKind::Image => ClipKind::Image,
    }
}

fn ingest(app: &AppHandle, state: &AppState, capture: ClipCapture) {
    let kind = to_clip_kind(capture.kind);
    let added = {
        let Ok(mut h) = state.clipboard.lock() else {
            return;
        };
        // `add_if_new`: a repeat of the newest entry (apps that write the clipboard
        // several times per copy) is neither a second entry nor a second glow.
        match h.add_if_new(kind, capture.content.as_str(), now_ms()) {
            Some(id) => {
                if let Err(e) = persist_clipboard(state, &h) {
                    eprintln!("[krovi] saving clipboard history failed: {e}");
                }
                Some((id, clipboard_dtos(&h)))
            }
            None => None,
        }
    };
    let Some((id, dtos)) = added else { return };
    if kind == ClipKind::Image {
        if let Some(png) = &capture.full_png {
            store_clip_image(app, &capture.content, png);
        }
        prune_clip_images(app);
    }
    emit_clipboard(app, &dtos, Some(id));
}

fn clipboard_poll_once(app: &AppHandle) {
    let state = app.state::<AppState>();
    // Hold the I/O lock from the token check through ingest: a concurrent copy-back
    // (which writes the clipboard under the same lock) can never interleave.
    let Ok(mut gate) = state.live.clip_io.lock() else {
        return;
    };
    let token = krovi_sys::change_token();
    if !gate.should_read(token) {
        return;
    }
    let outcome = krovi_sys::read_capture();
    gate.record(token, &outcome);
    if let ReadOutcome::Capture(capture) = outcome {
        ingest(app, &state, capture);
    }
}

fn spawn_clipboard_watcher(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("krovi-clipboard".into())
        .spawn(move || {
            {
                // With history already restored, treat whatever is on the clipboard now
                // as seen (don't promote a stale item at every launch); on a fresh,
                // empty history, ingest it.
                let state = app.state::<AppState>();
                let has_history = state
                    .clipboard
                    .lock()
                    .map(|h| !h.is_empty())
                    .unwrap_or(false);
                if let Ok(mut gate) = state.live.clip_io.lock() {
                    *gate = ClipGate::new(has_history.then(krovi_sys::change_token));
                };
            }
            loop {
                std::thread::sleep(CLIP_POLL);
                clipboard_poll_once(&app);
            }
        });
    if let Err(e) = spawned {
        eprintln!("[krovi] could not start the clipboard watcher: {e}");
    }
}

// ---------------------------------------------------------------------------
// Media, stats, screen time
// ---------------------------------------------------------------------------

fn spawn_media(app: AppHandle) {
    let cb_app = app.clone();
    let worker = MediaWorker::spawn(move |media| {
        let state = cb_app.state::<AppState>();
        if let Ok(mut slot) = state.live.media.lock() {
            *slot = media.clone();
        }
        let _ = cb_app.emit("media-changed", &media);
    });
    match worker {
        Ok(w) => {
            if let Ok(mut slot) = app.state::<AppState>().live.media_worker.lock() {
                *slot = Some(w);
            }
        }
        Err(e) => eprintln!("[krovi] media session unavailable: {e}"),
    }
}

fn spawn_stats(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("krovi-stats".into())
        .spawn(move || {
            let mut sampler = StatsSampler::new();
            // Prime the CPU counters so the first emitted sample has a real delta.
            if sampler.sample().is_err() {
                eprintln!("[krovi] system stats unavailable");
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
            loop {
                if let Ok(stats) = sampler.sample() {
                    if let Ok(mut slot) = app.state::<AppState>().live.stats.lock() {
                        *slot = Some(stats.clone());
                    }
                    let _ = app.emit("stats-tick", &stats);
                }
                std::thread::sleep(STATS_EVERY);
            }
        });
    if let Err(e) = spawned {
        eprintln!("[krovi] could not start the stats sampler: {e}");
    }
}

fn spawn_screen_time(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("krovi-screen-time".into())
        .spawn(move || loop {
            let state = app.state::<AppState>();
            if let Err(e) = tick_screen_time(&state, now_ms()) {
                eprintln!("[krovi] screen-time tick failed: {e}");
            }
            std::thread::sleep(SCREEN_TIME_EVERY);
        });
    if let Err(e) = spawned {
        eprintln!("[krovi] could not start the screen-time sampler: {e}");
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct Snapshot {
    clipboard: Vec<ClipDto>,
    media: Option<MediaState>,
    stats: Option<SystemStats>,
}

/// Everything the live UI needs to render immediately; events keep it current.
#[tauri::command]
pub fn live_snapshot(state: State<AppState>) -> CmdResult<Snapshot> {
    let clipboard = {
        let h = state.clipboard.lock().map_err(lock_err)?;
        clipboard_dtos(&h)
    };
    let media = state
        .live
        .media
        .lock()
        .map_err(lock_err)?
        .as_ref()
        .map(|m| m.advanced_to(now_ms()));
    let stats = state.live.stats.lock().map_err(lock_err)?.clone();
    Ok(Snapshot {
        clipboard,
        media,
        stats,
    })
}

fn copy_back(app: &AppHandle, id: u64) -> CmdResult<Vec<ClipDto>> {
    let state = app.state::<AppState>();
    // Same lock the watcher holds while reading: the write + token acknowledgement
    // below is atomic with respect to it.
    let mut gate = state.live.clip_io.lock().map_err(lock_err)?;
    let entry = state
        .clipboard
        .lock()
        .map_err(lock_err)?
        .get(id)
        .cloned()
        .ok_or_else(|| format!("unknown clip id: {id}"))?;

    match entry.kind {
        ClipKind::Image => {
            let png = load_clip_png(app, &entry.content)
                .ok_or_else(|| "the image is no longer available".to_string())?;
            krovi_sys::write_image_png(&png).map_err(|e| e.to_string())?;
        }
        _ => krovi_sys::write_text(&entry.content).map_err(|e| e.to_string())?,
    }
    // The OS bumped the sequence number while we wrote; mark the new value as ours so
    // the watcher does not ingest our own write as a fresh (duplicate) copy.
    gate.acknowledge(krovi_sys::change_token());

    // Promote (dedup-on-recopy keeps the pin and refreshes the timestamp).
    let dtos = {
        let mut h = state.clipboard.lock().map_err(lock_err)?;
        h.add(entry.kind, entry.content.clone(), now_ms());
        persist_clipboard(&state, &h)?;
        clipboard_dtos(&h)
    };
    drop(gate);
    emit_clipboard(app, &dtos, None);
    Ok(dtos)
}

/// Write a history entry back to the OS clipboard and promote it to the top. Never
/// creates a duplicate entry.
#[tauri::command]
pub async fn clipboard_copy(app: AppHandle, id: u64) -> CmdResult<Vec<ClipDto>> {
    tauri::async_runtime::spawn_blocking(move || copy_back(&app, id))
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}

/// Remove every unpinned entry (pinned ones stay).
#[tauri::command]
pub fn clipboard_clear(app: AppHandle, state: State<AppState>) -> CmdResult<Vec<ClipDto>> {
    let dtos = {
        let mut h = state.clipboard.lock().map_err(lock_err)?;
        h.clear_unpinned();
        persist_clipboard(&state, &h)?;
        clipboard_dtos(&h)
    };
    prune_clip_images(&app);
    emit_clipboard(&app, &dtos, None);
    Ok(dtos)
}

fn transport(app: &AppHandle, command: &str) -> CmdResult<()> {
    let cmd = TransportCommand::from_str(command)
        .ok_or_else(|| format!("unknown transport command: {command}"))?;
    let state = app.state::<AppState>();
    let worker = state.live.media_worker.lock().map_err(lock_err)?;
    match worker.as_ref() {
        Some(w) => w.transport(cmd).map_err(|e| e.to_string()),
        None => Err("no media session is available".into()),
    }
}

/// Send `playpause` / `next` / `previous` (also `play` / `pause`) to the active
/// media session. Runs on the media worker's MTA thread.
#[tauri::command]
pub async fn media_transport(app: AppHandle, command: String) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || transport(&app, &command))
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}
