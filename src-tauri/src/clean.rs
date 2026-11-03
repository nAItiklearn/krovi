//! Clean My OS (owner: clean). Scan -> review the real paths and sizes -> confirm -> Recycle Bin.
//!
//! A thin Tauri layer over the `krovi-clean` crate. All behaviour (what is scanned, the link and
//! path safety rules, the Recycle Bin call) lives in the crate and is unit-tested there; this
//! file only holds the last scan, the cancel flag and turns crate callbacks into Tauri events.
//!
//! Commands: clean_categories, clean_scan, clean_cancel, clean_delete, clean_recycle_bin_size.
//! Events:
//!   * `clean-progress` `{category, files, bytes}` while scanning (what has been looked at so far)
//!   * `clean-done` `{ok, cancelled, error, scan}` when a scan ends (`scan` is the whole result)
//!   * `clean-delete-progress` `{done, total}` while items are being moved
//!
//! Safety, enforced in the crate and only wired here:
//!   * `clean_delete` takes ids from the stored scan (plus the scan's id), never paths.
//!   * Items go to the Recycle Bin (`SHFileOperationW` + `FOF_ALLOWUNDO`); the Recycle Bin itself
//!     is only measured, never emptied. No elevation, no network.
//!   * Paths are never logged: only counts go to stderr.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use krovi_clean::{
    categories, category_infos, run_delete, scan, select, CategoryInfo, DeleteOptions,
    DeleteReport, DeleteRequest, Env, Progress, RecycleBinSize, Scan, ScanOptions,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::CmdResult;

/// Progress events are throttled to this, so a huge folder cannot flood the webview.
const PROGRESS_EVERY: Duration = Duration::from_millis(120);

#[derive(Default)]
struct Inner {
    /// The scan the user is looking at: the only thing a delete may act on.
    scan: Mutex<Option<Arc<Scan>>>,
    /// A scan or a clean is running (they never overlap).
    busy: AtomicBool,
    /// Raised by `clean_cancel`; the scan walk and the delete batches check it.
    cancel: AtomicBool,
    /// Last scan id handed out.
    seq: AtomicU64,
}

pub struct CleanState(Arc<Inner>);

fn lk<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Holds the "busy" flag; releases it when dropped, even if the worker panics.
struct Busy(Arc<Inner>);

impl Busy {
    fn acquire(inner: &Arc<Inner>) -> CmdResult<Busy> {
        inner
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                "A scan or clean is already running. Wait for it to finish.".to_string()
            })?;
        inner.cancel.store(false, Ordering::Relaxed);
        Ok(Busy(Arc::clone(inner)))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Nothing is read from disk or the OS here; scanning only happens when the user asks.
    app.manage(CleanState(Arc::new(Inner::default())));
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent<'a> {
    category: &'a str,
    files: u64,
    bytes: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DoneEvent<'a> {
    ok: bool,
    cancelled: bool,
    error: Option<String>,
    scan: Option<&'a Scan>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DeleteProgressEvent {
    done: u64,
    total: u64,
}

// ---------------------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------------------

/// The fixed list of categories with their plain-English descriptions and risk notes.
/// Reads nothing from disk, so it is instant.
#[tauri::command]
pub fn clean_categories() -> Vec<CategoryInfo> {
    category_infos()
}

/// Starts a scan of the given category ids (empty = all) on its own thread and returns at once.
/// Progress arrives as `clean-progress`, the result as `clean-done`. Only reads.
#[tauri::command]
pub fn clean_scan(
    app: AppHandle,
    state: State<'_, CleanState>,
    categories: Vec<String>,
) -> CmdResult<()> {
    let inner = Arc::clone(&state.0);
    let busy = Busy::acquire(&inner)?;
    std::thread::Builder::new()
        .name("krovi-clean-scan".into())
        .spawn(move || run_scan(app, inner, busy, categories))
        .map(|_| ())
        .map_err(|e| format!("could not start the scan: {e}"))
}

fn run_scan(app: AppHandle, inner: Arc<Inner>, busy: Busy, wanted: Vec<String>) {
    let id = inner.seq.fetch_add(1, Ordering::Relaxed) + 1;
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut specs = categories(&Env::from_os());
        if !wanted.is_empty() {
            specs = select(specs, &wanted);
        }
        let mut last: Option<Instant> = None;
        let mut on_progress = |p: &Progress| {
            if last.is_some_and(|t| t.elapsed() < PROGRESS_EVERY) {
                return;
            }
            last = Some(Instant::now());
            let _ = app.emit(
                "clean-progress",
                ProgressEvent {
                    category: p.category,
                    files: p.files,
                    bytes: p.bytes,
                },
            );
        };
        scan(
            &specs,
            id,
            &ScanOptions::default(),
            &inner.cancel,
            &krovi_clean::system::recycle_bin_size,
            &mut on_progress,
        )
    }));
    match outcome {
        Ok(result) => {
            let cancelled = result.cancelled;
            let items: usize = result.categories.iter().map(|c| c.items.len()).sum();
            eprintln!(
                "[krovi-clean] scan {id}: {} categories, {items} items{}",
                result.categories.len(),
                if cancelled { " (stopped early)" } else { "" }
            );
            let result = Arc::new(result);
            *lk(&inner.scan) = Some(Arc::clone(&result));
            // free the "busy" flag first, so the UI can act the moment it hears `clean-done`
            drop(busy);
            let _ = app.emit(
                "clean-done",
                DoneEvent {
                    ok: true,
                    cancelled,
                    error: None,
                    scan: Some(&*result),
                },
            );
        }
        Err(_) => {
            eprintln!("[krovi-clean] scan {id}: the scan thread panicked");
            *lk(&inner.scan) = None;
            drop(busy);
            let _ = app.emit(
                "clean-done",
                DoneEvent {
                    ok: false,
                    cancelled: false,
                    error: Some("The scan stopped unexpectedly. Try again.".into()),
                    scan: None,
                },
            );
        }
    }
}

/// Stops the scan or the clean that is running (the Stop button). A scan returns what it found
/// so far; a clean stops before the next batch.
#[tauri::command]
pub fn clean_cancel(state: State<'_, CleanState>) {
    state.0.cancel.store(true, Ordering::Relaxed);
}

/// Moves the ticked items to the Recycle Bin. `scan_id` must be the id of the stored scan, and
/// `item_ids` / `category_ids` must come from it; anything else is refused. Every item is
/// re-checked against the disk first (see `krovi_clean::delete`). Returns what happened,
/// including each item that could not be moved and why.
#[tauri::command]
pub async fn clean_delete(
    app: AppHandle,
    state: State<'_, CleanState>,
    scan_id: u64,
    item_ids: Vec<String>,
    category_ids: Vec<String>,
) -> CmdResult<DeleteReport> {
    let inner = Arc::clone(&state.0);
    let stored = lk(&inner.scan)
        .clone()
        .ok_or("There is no scan to clean. Scan first.")?;
    let recycler = krovi_clean::system::recycler()
        .ok_or("Moving files to the Recycle Bin needs the Windows app.")?;
    let busy = Busy::acquire(&inner)?;
    let worker = Arc::clone(&inner);

    let report = tauri::async_runtime::spawn_blocking(move || {
        let _busy = busy; // released when the batch run ends, before the result is returned
        let request = DeleteRequest {
            scan_id,
            item_ids,
            category_ids,
        };
        let mut last: Option<Instant> = None;
        let mut on_progress = |done: usize, total: usize| {
            let finished = done >= total;
            if !finished && last.is_some_and(|t| t.elapsed() < PROGRESS_EVERY) {
                return;
            }
            last = Some(Instant::now());
            let _ = app.emit(
                "clean-delete-progress",
                DeleteProgressEvent {
                    done: done as u64,
                    total: total as u64,
                },
            );
        };
        run_delete(
            &stored,
            &request,
            &*recycler,
            &DeleteOptions::default(),
            &worker.cancel,
            &mut on_progress,
        )
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))??;

    eprintln!(
        "[krovi-clean] clean: {} moved, {} failed, {} already gone{}",
        report.deleted,
        report.failed.len(),
        report.already_gone,
        if report.cancelled {
            " (stopped early)"
        } else {
            ""
        }
    );
    // The scan is spent: a later clean needs a fresh scan and a fresh confirmation.
    *lk(&inner.scan) = None;
    Ok(report)
}

/// Size and item count of the Recycle Bin (`null` if Windows will not say). Measured only; Krovi
/// never empties it.
#[tauri::command]
pub async fn clean_recycle_bin_size() -> CmdResult<Option<RecycleBinSize>> {
    tauri::async_runtime::spawn_blocking(krovi_clean::system::recycle_bin_size)
        .await
        .map_err(|e| format!("background task failed: {e}"))
}
