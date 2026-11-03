//! Agent Mode (owner: agent). A thin Tauri layer over the `krovi-agent` crate.
//!
//! All decisions (what a sentence means, what is safe, what needs the user's OK, which steps may
//! run) are made in the crate and unit-tested there. This file only: supplies the clock, the UTC
//! offset and the installed-app list; keeps the plans and tasks (`Session`); executes the steps the
//! crate dispatches; persists the MCP registry; and turns task changes into events.
//!
//! Commands:  `agent_warm`, `agent_plan(text)`, `agent_run(planId, confirmed)`, `agent_tasks`,
//!            `agent_cancel(taskId)`, `agent_integrations`, `agent_mcp_list`,
//!            `agent_mcp_save(server)`, `agent_mcp_remove(id)`
//! Events:    `agent-task`  a `Task` whose status changed (camelCase, see krovi_agent::Task)
//!            `agent-timer` `{ seconds, label }`: the webview starts the timer (timers live in the
//!            frontend; the step counts as done once the event is emitted)
//!            `agent-voice-plan` `{ planId, text, plan, error }`: a sentence spoken to the Agent
//!            shortcut (see `handle_voice`); `plan` is the same `Plan` `agent_plan` returns
//!
//! Talking to the Agent: `dictation.rs` records and transcribes (its Agent shortcut) and calls
//! [`handle_voice`] with the words. They take the same road as a typed sentence: planned by
//! `make_plan`, Safe steps run by `run_plan`, steps that need an OK wait as cards. The panel is
//! then opened on the Agent tool and shown the plan. The words are never typed anywhere.
//!
//! What it touches: nothing leaves the PC from here. Links are handed to the user's own browser or
//! mail app. Krovi never sends or reads email. MCP servers are stored (setting `agent.mcp.v1`) and
//! never started or contacted. The user's sentence is parsed in memory and never logged.
//!
//! Safety nets on top of the crate's rules: every link goes through `check_openable` again right
//! before it is opened (http/https only, or a `mailto:` this crate built); an app is launched only
//! if its target is in the cached Start-menu list *and* `WinHost::launch_app` accepts it; a
//! reminder whose time slipped into the past while its card was open is refused, not silently
//! stored with the alert off.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use krovi_agent::civil::{local_to_utc_ms, Date};
use krovi_agent::{
    check_openable, integrations, plan_text, Action, AppEntry, Integration, McpRegistry, McpServer,
    ParseContext, Plan, Session, Step, Task, MCP_SETTINGS_KEY,
};
use krovi_core::calendar::ZoneOffset;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::{lock_err, now_ms, CmdResult};
use crate::reminders::OsZone;
use crate::state::AppState;

/// How long the Start-menu list is trusted before the next plan refreshes it in the background
/// (that plan still uses the old list, so asking never waits).
const APPS_STALE: Duration = Duration::from_secs(10 * 60);

// ---------------------------------------------------------------------------------------------
// the installed-app list (Start menu), cached
// ---------------------------------------------------------------------------------------------

#[cfg(all(windows, feature = "native"))]
fn scan_apps() -> Vec<AppEntry> {
    krovi_voice::os::scan_catalog()
        .apps
        .into_iter()
        .map(|e| AppEntry {
            name: e.name,
            target: e.path,
        })
        .collect()
}

#[cfg(not(all(windows, feature = "native")))]
fn scan_apps() -> Vec<AppEntry> {
    Vec::new()
}

#[cfg(all(windows, feature = "native"))]
fn launch_app(target: &str) -> Result<(), String> {
    use krovi_voice::actions::Host;
    // refuses anything that is not an AppsFolder entry or an existing Start-menu shortcut
    krovi_voice::os::WinHost.launch_app(target)
}

#[cfg(not(all(windows, feature = "native")))]
fn launch_app(_target: &str) -> Result<(), String> {
    Err("Opening apps needs the native build.".into())
}

struct AppCache {
    slot: Mutex<Option<(Arc<Vec<AppEntry>>, Instant)>>,
    refreshing: AtomicBool,
}

impl AppCache {
    fn new() -> Arc<Self> {
        Arc::new(AppCache {
            slot: Mutex::new(None),
            refreshing: AtomicBool::new(false),
        })
    }

    fn peek(&self) -> Option<(Arc<Vec<AppEntry>>, Instant)> {
        self.slot.lock().ok().and_then(|s| s.clone())
    }

    /// Scan now (blocking: call from a worker thread) and cache the result.
    fn refresh_now(&self) -> Arc<Vec<AppEntry>> {
        let list = Arc::new(scan_apps());
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some((list.clone(), Instant::now()));
        }
        list
    }

    fn refresh_in_background(self: &Arc<Self>) {
        if self.refreshing.swap(true, Ordering::SeqCst) {
            return; // a scan is already running
        }
        let me = self.clone();
        let spawned = std::thread::Builder::new()
            .name("krovi-agent-apps".into())
            .spawn(move || {
                me.refresh_now();
                me.refreshing.store(false, Ordering::SeqCst);
            });
        if spawned.is_err() {
            self.refreshing.store(false, Ordering::SeqCst);
        }
    }

    /// Is this launch target one the Start-menu scan handed out?
    fn knows(&self, target: &str) -> bool {
        self.peek()
            .is_some_and(|(list, _)| list.iter().any(|a| a.target == target))
    }
}

/// The list to plan against: the cached one (refreshing it in the background when stale), or, on
/// the very first use, a scan done off the main thread.
async fn apps_for_plan(cache: &Arc<AppCache>) -> Arc<Vec<AppEntry>> {
    if let Some((apps, at)) = cache.peek() {
        if at.elapsed() > APPS_STALE {
            cache.refresh_in_background();
        }
        return apps;
    }
    let worker = cache.clone();
    tauri::async_runtime::spawn_blocking(move || worker.refresh_now())
        .await
        .unwrap_or_else(|_| Arc::new(Vec::new()))
}

// ---------------------------------------------------------------------------------------------
// state
// ---------------------------------------------------------------------------------------------

/// Managed state: plans and tasks (in memory), the app list, and a lock that serialises MCP
/// registry edits (read-modify-write of the stored setting).
pub struct AgentState {
    session: Mutex<Session>,
    apps: Arc<AppCache>,
    plan_seq: AtomicU64,
    mcp_lock: Mutex<()>,
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Nothing is scanned or opened at start-up: the Start-menu list is read the first time the
    // Agent panel is used.
    app.manage(AgentState {
        session: Mutex::new(Session::new()),
        apps: AppCache::new(),
        plan_seq: AtomicU64::new(0),
        mcp_lock: Mutex::new(()),
    });
    Ok(())
}

impl AgentState {
    /// The Start-menu app names, for the palette's Quick Launcher (`extras.rs`). It is the SAME list
    /// Agent Mode plans against, from the same cache with the same refresh rules, so the palette
    /// never starts a second scan of its own. Names only; launch targets stay in this module.
    pub(crate) async fn app_names(&self) -> Vec<String> {
        apps_for_plan(&self.apps)
            .await
            .iter()
            .map(|a| a.name.clone())
            .collect()
    }
}

fn local_offset_minutes(now: u64) -> i32 {
    (OsZone.offset_ms_at(now as i64) / 60_000) as i32
}

fn emit_task(app: &AppHandle, task: &Task) {
    let _ = app.emit("agent-task", task);
}

// ---------------------------------------------------------------------------------------------
// MCP registry persistence (registry only: nothing here starts or contacts a server)
// ---------------------------------------------------------------------------------------------

fn load_mcp(state: &AppState) -> McpRegistry {
    let raw = state
        .store
        .lock()
        .ok()
        .and_then(|s| s.get_setting(MCP_SETTINGS_KEY).ok().flatten());
    McpRegistry::from_json(raw.as_deref())
}

fn save_mcp(state: &AppState, reg: &McpRegistry) -> CmdResult<()> {
    let mut store = state.store.lock().map_err(lock_err)?;
    store
        .set_setting(MCP_SETTINGS_KEY, &reg.to_json())
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------------------------
// executing a step (the crate already validated and approved it)
// ---------------------------------------------------------------------------------------------

#[cfg(windows)]
fn os_open(url: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // The link has no spaces or quotes (the crate refuses them), is passed as one argument and
    // never goes through a shell. `url.dll,FileProtocolHandler` is what ShellExecute "open" does
    // for a URL: it hands it to the default browser or mail app.
    std::process::Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|_| "Windows could not open it.".to_string())
}

#[cfg(not(windows))]
fn os_open(_url: &str) -> Result<(), String> {
    Err("Opening links needs Windows.".into())
}

/// Open a web link or a `mailto:` draft in the user's own app, after one more safety check.
fn open_link(url: &str) -> Result<(), String> {
    check_openable(url).map_err(|e| e.to_string())?;
    os_open(url)
}

/// Is a local `yyyy-mm-dd` + `HH:MM` still in the future? (Offset taken at "now"; a DST change
/// between now and then can move this by an hour, which only matters for a reminder due within
/// that hour.)
fn reminder_still_ahead(date: &str, time: &str, now: u64) -> bool {
    let (Some(d), Some((h, m))) = (Date::parse_iso(date), time.split_once(':')) else {
        return false;
    };
    let (Ok(h), Ok(m)) = (h.parse::<u32>(), m.parse::<u32>()) else {
        return false;
    };
    local_to_utc_ms(d, h * 60 + m, local_offset_minutes(now)) > now as i64
}

fn execute(app: &AppHandle, cache: &AppCache, step: &Step) -> Result<(), String> {
    match &step.action {
        Action::OpenUrl { url, .. } | Action::Search { url, .. } => open_link(url),
        Action::ComposeEmail { url, .. } => open_link(url),
        Action::OpenApp { target, .. } => {
            if !cache.knows(target) {
                return Err("That app is no longer in your Start menu list.".into());
            }
            launch_app(target)
        }
        Action::Timer { seconds, label } => app
            .emit("agent-timer", json!({ "seconds": seconds, "label": label }))
            .map_err(|e| e.to_string()),
        Action::Reminder { title, date, time } => {
            if !reminder_still_ahead(date, time, now_ms()) {
                return Err("That time has already passed.".into());
            }
            crate::commands::add_calendar_event_from(app, date, Some(time.as_str()), title)
                .map(|_| ())
        }
        Action::Note { body } => crate::commands::create_note_from(app, body).map(|_| ()),
        // never dispatched (it is always Blocked), but refuse it here as well
        Action::Unsupported { .. } => Err("This step cannot be run.".into()),
    }
}

// ---------------------------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------------------------

/// Start reading the Start-menu app list in the background so the first command is quick.
/// Called when the Agent panel opens; does nothing if the list is already cached.
#[tauri::command]
pub fn agent_warm(state: State<'_, AgentState>) {
    if state.apps.peek().is_none() {
        state.apps.refresh_in_background();
    }
}

/// Understand a sentence and remember the plan (so it can be run or confirmed later). Shared by
/// the typed command and the voice path, so a spoken sentence is planned exactly like a typed one.
async fn make_plan(state: &AgentState, text: &str) -> CmdResult<Plan> {
    let apps = apps_for_plan(&state.apps).await;
    let now = now_ms();
    let seq = state.plan_seq.fetch_add(1, Ordering::Relaxed) + 1;
    let id = format!("p{now}-{seq}");
    let ctx = ParseContext {
        now_utc_ms: now as i64,
        utc_offset_minutes: local_offset_minutes(now),
        apps: apps.as_slice(),
    };
    let plan = plan_text(&id, text, &ctx);
    state
        .session
        .lock()
        .map_err(lock_err)?
        .add_plan(plan.clone());
    Ok(plan)
}

/// Understand a sentence and return the plan: steps, a plain summary of each, and whether each is
/// safe, needs confirmation, or is blocked. Nothing is executed. The text is not logged.
#[tauri::command]
pub async fn agent_plan(state: State<'_, AgentState>, text: String) -> CmdResult<Plan> {
    make_plan(&state, &text).await
}

/// Run a plan: every Safe step, plus the Confirm steps whose ids are in `confirmed`. Blocked
/// steps never run; a step already run (or cancelled) is not run again. Steps that are still
/// waiting for confirmation become `awaiting-confirmation` tasks. Returns the tasks this call
/// created or changed; each change is also emitted as `agent-task`.
#[tauri::command]
pub async fn agent_run(
    app: AppHandle,
    state: State<'_, AgentState>,
    plan_id: String,
    confirmed: Vec<String>,
) -> CmdResult<Vec<Task>> {
    run_plan(&app, &state, &plan_id, &confirmed).await
}

/// The body of `agent_run`, shared with the voice path.
async fn run_plan(
    app: &AppHandle,
    state: &AgentState,
    plan_id: &str,
    confirmed: &[String],
) -> CmdResult<Vec<Task>> {
    let (touched, dispatches, started) = {
        let mut session = state.session.lock().map_err(lock_err)?;
        let out = session
            .begin_run(plan_id, confirmed, now_ms())
            .map_err(|e| e.to_string())?;
        let started: Vec<Task> = out
            .touched
            .iter()
            .filter_map(|id| session.task(*id))
            .collect();
        (out.touched, out.dispatches, started)
    };
    for task in &started {
        emit_task(app, task);
    }

    if !dispatches.is_empty() {
        // Opening links and apps can block for a moment: never on the main thread.
        let worker_app = app.clone();
        let cache = state.apps.clone();
        let results = tauri::async_runtime::spawn_blocking(move || {
            dispatches
                .iter()
                .map(|d| (d.task_id, execute(&worker_app, &cache, &d.step)))
                .collect::<Vec<(u64, Result<(), String>)>>()
        })
        .await
        .map_err(|e| e.to_string())?;

        let mut finished = Vec::new();
        {
            let mut session = state.session.lock().map_err(lock_err)?;
            for (task_id, result) in results {
                if let Ok(task) = session.finish(task_id, result, now_ms()) {
                    finished.push(task);
                }
            }
        }
        for task in &finished {
            emit_task(app, task);
        }
    }

    let session = state.session.lock().map_err(lock_err)?;
    Ok(touched.iter().filter_map(|id| session.task(*id)).collect())
}

// ---------------------------------------------------------------------------------------------
// talking to the Agent (the voice shortcut in dictation.rs)
// ---------------------------------------------------------------------------------------------

/// Plan what was said, run its Safe steps exactly as `agent_run(planId, [])` would for a typed
/// sentence, and tell the webview (`agent-voice-plan`). Steps that need an OK stay
/// `awaiting-confirmation` and show as cards in the panel; blocked steps never run. A run that
/// fails is reported in the event's `error` and the plan is still shown.
async fn voice_plan(app: &AppHandle, text: String) -> CmdResult<()> {
    let state = app.state::<AgentState>();
    let plan = make_plan(&state, &text).await?;
    let run_error = if plan.steps.iter().any(|s| !s.is_blocked()) {
        run_plan(app, &state, &plan.id, &[]).await.err()
    } else {
        None
    };
    let _ = app.emit(
        "agent-voice-plan",
        json!({ "planId": plan.id, "text": text, "plan": plan, "error": run_error }),
    );
    Ok(())
}

/// Bring the panel up on the Agent tool, the way the tray's "Settings..." opens Settings: ask
/// the UI to expand (so the pill morphs), wait for it, then ask for the tool. Blocks for up to
/// a couple of seconds: call it from a worker thread, never the UI or an async task.
fn show_panel(app: &AppHandle) {
    use crate::shell::{shell_get_mode, shell_set_mode, Mode};
    match shell_get_mode(app.clone()) {
        Mode::Panel => {}
        Mode::Hidden => {
            // no pill on screen to morph from
            shell_set_mode(app.clone(), Mode::Panel);
            std::thread::sleep(Duration::from_millis(200));
        }
        Mode::Pill => {
            let _ = app.emit("shell-request", json!({ "action": "expand" }));
            for _ in 0..40 {
                if shell_get_mode(app.clone()) == Mode::Panel {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if shell_get_mode(app.clone()) != Mode::Panel {
                // nobody answered the request (no UI listening yet): expand directly
                shell_set_mode(app.clone(), Mode::Panel);
            }
            // let the panel mount before asking for a tool
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    let _ = app.emit("shell-open-tool", json!({ "tool": "agent" }));
}

/// The Agent shortcut's entry point: the words you spoke become a plan (Safe steps run now, the
/// rest wait for your OK) and the panel opens on it. Blocking; call from a worker thread.
/// The words are not logged.
pub fn handle_voice(app: &AppHandle, text: String) -> CmdResult<()> {
    tauri::async_runtime::block_on(voice_plan(app, text))?;
    show_panel(app);
    Ok(())
}

/// Every task, newest first (history is capped at 50).
#[tauri::command]
pub fn agent_tasks(state: State<'_, AgentState>) -> CmdResult<Vec<Task>> {
    Ok(state.session.lock().map_err(lock_err)?.tasks())
}

/// Cancel a task that has not started (a confirmation card's Cancel). A running or finished task
/// cannot be cancelled.
#[tauri::command]
pub fn agent_cancel(app: AppHandle, state: State<'_, AgentState>, task_id: u64) -> CmdResult<Task> {
    let task = state
        .session
        .lock()
        .map_err(lock_err)?
        .cancel(task_id, now_ms())
        .map_err(|e| e.to_string())?;
    emit_task(&app, &task);
    Ok(task)
}

/// The integration catalog (what Agent Mode can do, what is not available yet) plus one
/// "not connected" entry per registered MCP server.
#[tauri::command]
pub fn agent_integrations(state: State<'_, AppState>) -> CmdResult<Vec<Integration>> {
    let reg = load_mcp(&state);
    Ok(integrations(reg.list()))
}

#[tauri::command]
pub fn agent_mcp_list(state: State<'_, AppState>) -> CmdResult<Vec<McpServer>> {
    Ok(load_mcp(&state).list().to_vec())
}

/// Add an MCP server to the registry, or update the one with the same id. Validated; nothing is
/// started or contacted. Returns the whole registry.
#[tauri::command]
pub fn agent_mcp_save(
    agent: State<'_, AgentState>,
    state: State<'_, AppState>,
    server: McpServer,
) -> CmdResult<Vec<McpServer>> {
    let _edit = agent.mcp_lock.lock().map_err(lock_err)?;
    let mut reg = load_mcp(&state);
    reg.upsert(server).map_err(|e| e.to_string())?;
    save_mcp(&state, &reg)?;
    Ok(reg.list().to_vec())
}

#[tauri::command]
pub fn agent_mcp_remove(
    agent: State<'_, AgentState>,
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<Vec<McpServer>> {
    let _edit = agent.mcp_lock.lock().map_err(lock_err)?;
    let mut reg = load_mcp(&state);
    reg.remove(&id);
    save_mcp(&state, &reg)?;
    Ok(reg.list().to_vec())
}
