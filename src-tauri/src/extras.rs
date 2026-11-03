//! Extras (owner: extras): Quick Launcher names, snippets, file-organizer rules, workflows and
//! file-name search. A thin Tauri layer over the `krovi-extras` crate.
//!
//! All behaviour (validation, the safety rules, the moves, the search walk) lives in the crate and
//! is unit-tested there. This file only: stores the user's data in Krovi's own settings table,
//! holds the last organizer preview and the last search, and turns crate callbacks into events.
//!
//! Commands
//!   launcher     `extras_apps`
//!   snippets     `snippets_list`, `snippets_save(snippet)`, `snippets_remove(id)`, `snippets_use(id)`
//!   organizer    `organizer_folders`, `organizer_rules`, `organizer_save_rule(rule)`,
//!                `organizer_remove_rule(id)`, `organizer_preview(ruleIds)`, `organizer_run(previewId)`,
//!                `organizer_undo_info`, `organizer_undo`
//!   workflows    `workflows_list`, `workflows_save(workflow)`, `workflows_remove(id)`
//!   file search  `filesearch_start(query, folders)`, `filesearch_cancel`,
//!                `filesearch_open(searchId, hitId, reveal)`
//! Events
//!   * `filesearch-hits` `{searchId, hits}`  as each folder finishes
//!   * `filesearch-done` `{searchId, ok, error, summary}`  when a search ends
//!
//! What it touches, honestly:
//!   * Nothing leaves the PC from here. No network.
//!   * Snippets: copied to the clipboard on purpose, one at a time. There is no keyboard hook, no
//!     auto-expansion and nothing watches typing. The text is NOT added to Krovi's clipboard
//!     history (the write is acknowledged to the clipboard watcher, like a copy-back).
//!   * Organizer: Preview -> Confirm -> Run, never automatic, never in the background. The webview
//!     sends a rule or a preview id; it never sends a path to move. Moves only, never deletes,
//!     never overwrites, never follows a link, only inside the six known folders.
//!   * Workflows: stored here only. Running one happens in the webview through the ordinary Agent
//!     Mode commands (`agent_plan` / `agent_run`), so every Safe / Confirm / Blocked rule applies.
//!   * File search: names only, no index, no background work, no file contents read. A result is
//!     opened or revealed by id from the last search (never by a path from the webview), after a
//!     fresh check that it is still inside a known folder and not a link.
//!   * Paths and the text of snippets are never logged: only counts go to stderr.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use krovi_core::calendar::ZoneOffset;
use krovi_extras::{
    expand, parse_query, plan_moves, run_moves, search_names, undo_moves, uses_clipboard,
    ExpandCtx, Hit, KnownFolder, KnownFolders, PlanOptions, Preview, Rule, Rules, RunReport,
    SearchOptions, Snippet, Snippets, Summary, UndoJournal, UndoReport, Workflow, Workflows,
    RULES_KEY, SNIPPETS_KEY, UNDO_KEY, WORKFLOWS_KEY,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::agent::AgentState;
use crate::commands::{lock_err, now_ms, setting_get, setting_set, CmdResult};
use crate::reminders::OsZone;
use crate::state::AppState;

/// A preview older than this is refused: the folders may have changed a lot since it was made.
const PREVIEW_TTL_MS: u64 = 10 * 60 * 1000;

// ---------------------------------------------------------------------------------------------
// state
// ---------------------------------------------------------------------------------------------

/// The last search: its id, how to stop it, and the hits (so a result can be opened by id).
#[derive(Default)]
struct SearchRun {
    id: u64,
    cancel: Option<Arc<AtomicBool>>,
    hits: Vec<Hit>,
}

#[derive(Default)]
struct Inner {
    /// Serialises read-modify-write of the stored settings (snippets, workflows, rules, undo).
    edit: Mutex<()>,
    /// The preview the user is looking at: the only thing a run may act on.
    preview: Mutex<Option<Arc<Preview>>>,
    /// A preview, run or undo is in progress (they never overlap).
    busy: AtomicBool,
    /// Last preview id handed out.
    preview_seq: AtomicU64,
    search: Mutex<SearchRun>,
    search_seq: AtomicU64,
}

pub struct ExtrasState(Arc<Inner>);

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
                "Another step of the organizer is still running. Wait for it to finish.".to_string()
            })?;
        Ok(Busy(Arc::clone(inner)))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Nothing is read from disk or the OS here; every feature works only when the user asks.
    app.manage(ExtrasState(Arc::new(Inner::default())));
    Ok(())
}

fn blocking_err(e: tauri::Error) -> String {
    format!("background task failed: {e}")
}

fn load_values(state: &AppState, key: &str) -> Vec<serde_json::Value> {
    setting_get::<Vec<serde_json::Value>>(state, key).unwrap_or_default()
}

fn local_offset_minutes(now: u64) -> i32 {
    (OsZone.offset_ms_at(now as i64) / 60_000) as i32
}

// ---------------------------------------------------------------------------------------------
// Quick Launcher: the Start-menu names Agent Mode already scans
// ---------------------------------------------------------------------------------------------

/// The installed-app names for the palette. This is Agent Mode's own list (same cache, same
/// refresh): the first call after start-up waits for that one scan, later calls are instant. Names
/// only. Launching goes through `agent_plan` / `agent_run`, so an app opens as a Safe "Open <app>"
/// step and nothing is launched from here.
#[tauri::command]
pub async fn extras_apps(agent: State<'_, AgentState>) -> CmdResult<Vec<String>> {
    Ok(agent.app_names().await)
}

// ---------------------------------------------------------------------------------------------
// Snippets
// ---------------------------------------------------------------------------------------------

fn load_snippets(state: &AppState) -> Snippets {
    Snippets::from_values(load_values(state, SNIPPETS_KEY))
}

fn save_snippets(state: &AppState, s: &Snippets) -> CmdResult<()> {
    setting_set(state, SNIPPETS_KEY, &s.list())
}

#[tauri::command]
pub fn snippets_list(state: State<'_, AppState>) -> CmdResult<Vec<Snippet>> {
    Ok(load_snippets(&state).list().to_vec())
}

/// Add a snippet, or update the one with the same id. Validated; returns the whole list.
#[tauri::command]
pub fn snippets_save(
    extras: State<'_, ExtrasState>,
    state: State<'_, AppState>,
    snippet: Snippet,
) -> CmdResult<Vec<Snippet>> {
    let _edit = lk(&extras.0.edit);
    let mut all = load_snippets(&state);
    all.upsert(snippet).map_err(|e| e.to_string())?;
    save_snippets(&state, &all)?;
    Ok(all.list().to_vec())
}

#[tauri::command]
pub fn snippets_remove(
    extras: State<'_, ExtrasState>,
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<Vec<Snippet>> {
    let _edit = lk(&extras.0.edit);
    let mut all = load_snippets(&state);
    all.remove(&id);
    save_snippets(&state, &all)?;
    Ok(all.list().to_vec())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetUse {
    /// Characters copied.
    chars: usize,
    /// Plain sentences about anything the user should know (an empty or private clipboard, ...).
    notes: Vec<String>,
}

/// The clipboard text for `{clipboard}`, or why there is none. Called under the clipboard I/O lock.
fn clipboard_text(notes: &mut Vec<String>) -> Option<String> {
    use krovi_sys::{CaptureKind, ReadOutcome};
    match krovi_sys::read_capture() {
        ReadOutcome::Capture(c) if matches!(c.kind, CaptureKind::Text | CaptureKind::Link) => {
            Some(c.content)
        }
        ReadOutcome::Capture(_) => {
            notes.push(
                "The clipboard holds an image, not text, so {clipboard} became empty.".into(),
            );
            None
        }
        ReadOutcome::Sensitive => {
            notes.push("The clipboard holds something marked private (like a password), which Krovi does not read, so {clipboard} became empty.".into());
            None
        }
        ReadOutcome::Busy => {
            notes.push(
                "Another program was using the clipboard, so {clipboard} became empty.".into(),
            );
            None
        }
        ReadOutcome::Empty => None,
    }
}

fn use_snippet(app: &AppHandle, id: &str) -> CmdResult<SnippetUse> {
    let state = app.state::<AppState>();
    let snippet = load_snippets(&state)
        .get(id)
        .cloned()
        .ok_or("That snippet no longer exists.")?;
    // The same lock the clipboard watcher holds while it reads: reading the clipboard for
    // {clipboard}, writing the result and acknowledging the new change token are one step.
    let mut gate = state.live.clip_io.lock().map_err(lock_err)?;
    let mut notes: Vec<String> = Vec::new();
    let clip = if uses_clipboard(&snippet.body) {
        clipboard_text(&mut notes)
    } else {
        None
    };
    let now = now_ms();
    let out = expand(
        &snippet.body,
        &ExpandCtx {
            utc_ms: now as i64,
            offset_minutes: local_offset_minutes(now),
            clipboard: clip.as_deref(),
        },
    );
    krovi_sys::write_text(&out.text).map_err(|e| e.to_string())?;
    // The write bumped the clipboard sequence number: mark it as ours so the snippet is not
    // recorded in the clipboard history.
    gate.acknowledge(krovi_sys::change_token());
    drop(gate);
    if out.clipboard_empty && notes.is_empty() {
        notes.push("The clipboard had no text, so {clipboard} became empty.".into());
    }
    if out.clipboard_truncated {
        notes.push("The clipboard text was very long and was cut short.".into());
    }
    Ok(SnippetUse {
        chars: out.text.chars().count(),
        notes,
    })
}

/// Fill in the placeholders and copy the snippet to the clipboard. Does nothing else: it does not
/// type into any window and is never triggered by typing.
#[tauri::command]
pub async fn snippets_use(app: AppHandle, id: String) -> CmdResult<SnippetUse> {
    tauri::async_runtime::spawn_blocking(move || use_snippet(&app, &id))
        .await
        .map_err(blocking_err)?
}

// ---------------------------------------------------------------------------------------------
// Workflows (stored here; run in the webview through the ordinary Agent Mode commands)
// ---------------------------------------------------------------------------------------------

fn load_workflows(state: &AppState) -> Workflows {
    Workflows::from_values(load_values(state, WORKFLOWS_KEY))
}

fn save_workflows(state: &AppState, w: &Workflows) -> CmdResult<()> {
    setting_set(state, WORKFLOWS_KEY, &w.list())
}

#[tauri::command]
pub fn workflows_list(state: State<'_, AppState>) -> CmdResult<Vec<Workflow>> {
    Ok(load_workflows(&state).list().to_vec())
}

#[tauri::command]
pub fn workflows_save(
    extras: State<'_, ExtrasState>,
    state: State<'_, AppState>,
    workflow: Workflow,
) -> CmdResult<Vec<Workflow>> {
    let _edit = lk(&extras.0.edit);
    let mut all = load_workflows(&state);
    all.upsert(workflow).map_err(|e| e.to_string())?;
    save_workflows(&state, &all)?;
    Ok(all.list().to_vec())
}

#[tauri::command]
pub fn workflows_remove(
    extras: State<'_, ExtrasState>,
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<Vec<Workflow>> {
    let _edit = lk(&extras.0.edit);
    let mut all = load_workflows(&state);
    all.remove(&id);
    save_workflows(&state, &all)?;
    Ok(all.list().to_vec())
}

// ---------------------------------------------------------------------------------------------
// File organizer
// ---------------------------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderInfo {
    id: &'static str,
    label: &'static str,
    /// Where Windows says it is, or `null` if it is not on this PC.
    path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoInfo {
    run_id: u64,
    at_ms: u64,
    count: usize,
}

fn load_rules(state: &AppState) -> Rules {
    Rules::from_values(load_values(state, RULES_KEY))
}

fn save_rules(state: &AppState, r: &Rules) -> CmdResult<()> {
    setting_set(state, RULES_KEY, &r.list())
}

fn load_undo(state: &AppState) -> Option<UndoJournal> {
    setting_get::<UndoJournal>(state, UNDO_KEY).filter(|j| !j.moves.is_empty())
}

fn save_undo(state: &AppState, journal: Option<&UndoJournal>) -> CmdResult<()> {
    setting_set(state, UNDO_KEY, &journal)
}

/// The six known folders and where they are on this PC.
#[tauri::command]
pub async fn organizer_folders() -> CmdResult<Vec<FolderInfo>> {
    tauri::async_runtime::spawn_blocking(|| {
        let folders = KnownFolders::from_os();
        KnownFolder::ALL
            .into_iter()
            .map(|k| FolderInfo {
                id: k.id(),
                label: k.label(),
                path: folders.get(k).map(|p| p.to_string_lossy().into_owned()),
            })
            .collect::<Vec<FolderInfo>>()
    })
    .await
    .map_err(blocking_err)
}

#[tauri::command]
pub fn organizer_rules(state: State<'_, AppState>) -> CmdResult<Vec<Rule>> {
    Ok(load_rules(&state).list().to_vec())
}

/// Add a rule, or update the one with the same id. Validated; returns the whole list.
#[tauri::command]
pub fn organizer_save_rule(
    extras: State<'_, ExtrasState>,
    state: State<'_, AppState>,
    rule: Rule,
) -> CmdResult<Vec<Rule>> {
    let _edit = lk(&extras.0.edit);
    let mut all = load_rules(&state);
    all.upsert(rule).map_err(|e| e.to_string())?;
    save_rules(&state, &all)?;
    Ok(all.list().to_vec())
}

#[tauri::command]
pub fn organizer_remove_rule(
    extras: State<'_, ExtrasState>,
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<Vec<Rule>> {
    let _edit = lk(&extras.0.edit);
    let mut all = load_rules(&state);
    all.remove(&id);
    save_rules(&state, &all)?;
    Ok(all.list().to_vec())
}

/// Step 1: list the exact moves the enabled rules (or just `rule_ids`) would make. Only reads.
/// The preview is kept; `organizer_run` accepts only its id.
#[tauri::command]
pub async fn organizer_preview(
    extras: State<'_, ExtrasState>,
    state: State<'_, AppState>,
    rule_ids: Vec<String>,
) -> CmdResult<Preview> {
    let inner = Arc::clone(&extras.0);
    let busy = Busy::acquire(&inner)?;
    let mut rules = load_rules(&state).list().to_vec();
    if !rule_ids.is_empty() {
        rules.retain(|r| rule_ids.contains(&r.id));
    }
    let id = inner.preview_seq.fetch_add(1, Ordering::Relaxed) + 1;

    let preview = tauri::async_runtime::spawn_blocking(move || {
        let _busy = busy; // released when the listing ends
        plan_moves(
            id,
            &rules,
            &KnownFolders::from_os(),
            &PlanOptions::default(),
        )
    })
    .await
    .map_err(blocking_err)?;

    eprintln!(
        "[krovi-extras] organizer preview {id}: {} moves, {} notes",
        preview.moves.len(),
        preview.notes.len()
    );
    *lk(&inner.preview) = Some(Arc::new(preview.clone()));
    Ok(preview)
}

/// Step 3 (after the user confirmed): move what the preview listed. `preview_id` must be the id of
/// the stored preview, and the preview is spent by this call. Every file is re-checked against the
/// disk first (see `krovi_extras::organizer`). Returns what moved and what did not, and why.
#[tauri::command]
pub async fn organizer_run(
    extras: State<'_, ExtrasState>,
    state: State<'_, AppState>,
    preview_id: u64,
) -> CmdResult<RunReport> {
    let inner = Arc::clone(&extras.0);
    let stored = lk(&inner.preview)
        .clone()
        .ok_or("There is no preview to run. Preview first.")?;
    if stored.preview_id != preview_id {
        return Err("This preview is out of date. Preview again before running.".into());
    }
    if now_ms().saturating_sub(stored.created_ms) > PREVIEW_TTL_MS {
        *lk(&inner.preview) = None;
        return Err("This preview is more than ten minutes old, and your folders may have changed. Preview again.".into());
    }
    let busy = Busy::acquire(&inner)?;
    // spent: a preview can be run once
    *lk(&inner.preview) = None;

    let report = tauri::async_runtime::spawn_blocking(move || {
        let _busy = busy;
        run_moves(&stored.moves, &KnownFolders::from_os())
    })
    .await
    .map_err(blocking_err)?;

    eprintln!(
        "[krovi-extras] organizer run {preview_id}: {} moved, {} failed, {} already gone",
        report.moved.len(),
        report.failed.len(),
        report.already_gone
    );
    if !report.moved.is_empty() {
        let _edit = lk(&inner.edit);
        let journal = UndoJournal {
            run_id: preview_id,
            at_ms: now_ms(),
            moves: report.moved.clone(),
        };
        if save_undo(&state, Some(&journal)).is_err() {
            eprintln!("[krovi-extras] could not store the undo list");
        }
    }
    Ok(report)
}

/// What "Undo" would put back, if anything.
#[tauri::command]
pub fn organizer_undo_info(state: State<'_, AppState>) -> CmdResult<Option<UndoInfo>> {
    Ok(load_undo(&state).map(|j| UndoInfo {
        run_id: j.run_id,
        at_ms: j.at_ms,
        count: j.moves.len(),
    }))
}

/// Move the last run's files back, but only the ones nothing has touched since. The undo list is
/// spent by this call.
#[tauri::command]
pub async fn organizer_undo(
    extras: State<'_, ExtrasState>,
    state: State<'_, AppState>,
) -> CmdResult<UndoReport> {
    let inner = Arc::clone(&extras.0);
    let journal = load_undo(&state).ok_or("There is nothing to undo.")?;
    let busy = Busy::acquire(&inner)?;
    let report = tauri::async_runtime::spawn_blocking(move || {
        let _busy = busy;
        undo_moves(&journal, &KnownFolders::from_os())
    })
    .await
    .map_err(blocking_err)?;
    eprintln!(
        "[krovi-extras] organizer undo: {} restored, {} left",
        report.restored,
        report.failed.len()
    );
    let _edit = lk(&inner.edit);
    if save_undo(&state, None).is_err() {
        eprintln!("[krovi-extras] could not clear the undo list");
    }
    Ok(report)
}

// ---------------------------------------------------------------------------------------------
// File-name search
// ---------------------------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct HitsEvent<'a> {
    search_id: u64,
    hits: &'a [Hit],
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DoneEvent {
    search_id: u64,
    ok: bool,
    error: Option<String>,
    summary: Option<Summary>,
}

/// Starts a name search on its own thread and returns its id at once. Hits arrive as
/// `filesearch-hits`, the end as `filesearch-done`. A new search stops the one before it.
/// `folders` limits it to some known folders (empty = all six).
#[tauri::command]
pub fn filesearch_start(
    app: AppHandle,
    extras: State<'_, ExtrasState>,
    query: String,
    folders: Vec<String>,
) -> CmdResult<u64> {
    let tokens = parse_query(&query)?;
    let wanted: Vec<KnownFolder> = folders
        .iter()
        .filter_map(|f| KnownFolder::parse(f))
        .collect();
    let inner = Arc::clone(&extras.0);
    let (id, cancel) = {
        let mut run = lk(&inner.search);
        if let Some(prev) = &run.cancel {
            prev.store(true, Ordering::Relaxed);
        }
        let id = inner.search_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = Arc::new(AtomicBool::new(false));
        *run = SearchRun {
            id,
            cancel: Some(Arc::clone(&cancel)),
            hits: Vec::new(),
        };
        (id, cancel)
    };
    std::thread::Builder::new()
        .name("krovi-extras-search".into())
        .spawn(move || run_search(app, inner, id, cancel, tokens, wanted))
        .map(|_| id)
        .map_err(|e| format!("could not start the search: {e}"))
}

fn run_search(
    app: AppHandle,
    inner: Arc<Inner>,
    id: u64,
    cancel: Arc<AtomicBool>,
    tokens: Vec<String>,
    wanted: Vec<KnownFolder>,
) {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let folders = KnownFolders::from_os();
        let roots: Vec<(KnownFolder, PathBuf)> = folders
            .all()
            .iter()
            .filter(|(k, _)| wanted.is_empty() || wanted.contains(k))
            .cloned()
            .collect();
        if roots.is_empty() {
            return None;
        }
        let mut on_hits = |hits: &[Hit]| {
            {
                let mut run = lk(&inner.search);
                if run.id == id {
                    run.hits.extend_from_slice(hits);
                }
            }
            let _ = app.emit(
                "filesearch-hits",
                HitsEvent {
                    search_id: id,
                    hits,
                },
            );
        };
        Some(search_names(
            &roots,
            &tokens,
            &SearchOptions::default(),
            &cancel,
            &mut on_hits,
        ))
    }));
    let done = match outcome {
        Ok(Some(summary)) => {
            eprintln!(
                "[krovi-extras] search {id}: {} names looked at, {} found",
                summary.scanned, summary.found
            );
            DoneEvent {
                search_id: id,
                ok: true,
                error: None,
                summary: Some(summary),
            }
        }
        Ok(None) => DoneEvent {
            search_id: id,
            ok: false,
            error: Some("None of those folders were found on this PC.".into()),
            summary: None,
        },
        Err(_) => {
            eprintln!("[krovi-extras] search {id}: the search thread panicked");
            DoneEvent {
                search_id: id,
                ok: false,
                error: Some("The search stopped unexpectedly. Try again.".into()),
                summary: None,
            }
        }
    };
    let _ = app.emit("filesearch-done", done);
}

/// Stop the running search (the Stop button). What was found so far stays listed.
#[tauri::command]
pub fn filesearch_cancel(extras: State<'_, ExtrasState>) {
    if let Some(c) = &lk(&extras.0.search).cancel {
        c.store(true, Ordering::Relaxed);
    }
}

/// Open a result, or show it in File Explorer. `search_id` and `hit_id` must name a hit of the
/// latest search; the path is looked up here, never taken from the webview, and checked again
/// (still there, not a link, still inside a known folder). Programs and scripts are never opened,
/// only shown in their folder.
#[tauri::command]
pub async fn filesearch_open(
    extras: State<'_, ExtrasState>,
    search_id: u64,
    hit_id: u32,
    reveal: bool,
) -> CmdResult<String> {
    let hit = {
        let run = lk(&extras.0.search);
        if run.id != search_id {
            return Err("That search was replaced by a newer one. Search again.".into());
        }
        run.hits
            .iter()
            .find(|h| h.id == hit_id)
            .cloned()
            .ok_or("That result is not in the latest search.")?
    };
    tauri::async_runtime::spawn_blocking(move || open_hit(&hit, reveal))
        .await
        .map_err(blocking_err)?
}

/// Re-checks `hit` against the disk and returns its resolved path.
fn checked_path(hit: &Hit) -> Result<PathBuf, String> {
    use krovi_extras::fsx::{canonical, has_wildcard, is_reparse, same_path};
    let path = PathBuf::from(&hit.path);
    if has_wildcard(&path) {
        return Err("That name contains a character Windows treats specially.".into());
    }
    let meta =
        std::fs::symlink_metadata(&path).map_err(|_| "It is no longer there.".to_string())?;
    if is_reparse(&meta) {
        return Err("It is now a link. Krovi never follows links, so it was left alone.".into());
    }
    let canon = canonical(&path).map_err(|_| "Krovi could not resolve it.".to_string())?;
    if !same_path(&canon, &path) {
        return Err("It is no longer where the search found it.".into());
    }
    if KnownFolders::from_os().containing(&canon).is_none() {
        return Err("It is not inside one of your known folders.".into());
    }
    Ok(canon)
}

#[cfg(all(windows, feature = "native"))]
fn open_hit(hit: &Hit, reveal: bool) -> CmdResult<String> {
    use krovi_voice::actions::Host;
    let path = checked_path(hit)?;
    let text = path.to_string_lossy().into_owned();
    if reveal {
        return krovi_voice::os::reveal(&text)
            .map(|_| format!("Showing {} in File Explorer.", hit.name));
    }
    if hit.is_dir {
        return krovi_voice::os::WinHost
            .open_folder(&text)
            .map(|_| format!("Opened {}.", hit.name));
    }
    if krovi_extras::fsx::is_risky_to_open(&hit.name) {
        return Err("Krovi does not open programs or scripts. Use Show in folder instead.".into());
    }
    os_open_file(&text).map(|_| format!("Opened {}.", hit.name))
}

#[cfg(not(all(windows, feature = "native")))]
fn open_hit(_hit: &Hit, _reveal: bool) -> CmdResult<String> {
    Err("Opening files needs the Windows app.".into())
}

#[cfg(all(windows, feature = "native"))]
fn os_open_file(path: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // The same call Agent Mode uses to hand a link to the default app: the path is one argument
    // and never goes through a shell.
    std::process::Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(path)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|_| "Windows could not open it.".to_string())
}
