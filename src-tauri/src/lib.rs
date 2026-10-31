//! Krovi desktop shell (Tauri 2).
//!
//! This crate is deliberately **thin**: each `#[tauri::command]` validates its
//! input, calls into `krovi-core`, persists the whole collection through the
//! `krovi-store` `MemStore`, and returns typed JSON (serde DTOs). No product
//! logic lives here — ordering, eviction, pinning, LIFO, etc. are all in the
//! tested core (see `crates/krovi-core`). The frontend in `../src` talks to
//! these commands via `invoke`, and falls back to in-memory fixtures in the
//! browser demo (see `src/lib/bridge.ts`).

use std::sync::Mutex;

use serde::Serialize;
use tauri::{Manager, State};

use krovi_core::{
    ClipKind, ClipboardHistory, FileShelf, Notes, PasteStack, Pomodoro, ScreenTime, TodoList,
    DEFAULT_SHELF,
};
use krovi_store::{screen_time_records, stored_shelves, MemStore, Store};
use krovi_net::{Rates, WeatherData};
use krovi_sys::{MediaTrack, Sampler, TransportCommand};

// ---------------------------------------------------------------------------
// Serializable DTOs (the wire shape mirrors src/lib/types.ts exactly).
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ClipDto {
    pub id: u64,
    pub kind: String,
    pub content: String,
    pub pinned: bool,
    pub created_at: u64,
}

#[derive(Serialize)]
pub struct NoteDto {
    pub id: u64,
    pub body: String,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Serialize)]
pub struct TaskDto {
    pub id: u64,
    pub title: String,
    pub done: bool,
    pub created_at: u64,
    pub completed_at: Option<u64>,
}

#[derive(Serialize)]
pub struct ShelfItemDto {
    pub id: u64,
    pub name: String,
    pub stored_path: String,
    pub size_bytes: u64,
    pub added_at: u64,
}

#[derive(Serialize)]
pub struct ShelfDto {
    pub name: String,
    pub items: Vec<ShelfItemDto>,
}

#[derive(Serialize)]
pub struct ScreenTimeDto {
    pub day: u32,
    pub app: String,
    pub ms: u64,
}

fn kind_str(k: ClipKind) -> &'static str {
    match k {
        ClipKind::Text => "Text",
        ClipKind::Link => "Link",
        ClipKind::Image => "Image",
    }
}

fn parse_kind(s: &str) -> Result<ClipKind, String> {
    match s {
        "Text" => Ok(ClipKind::Text),
        "Link" => Ok(ClipKind::Link),
        "Image" => Ok(ClipKind::Image),
        other => Err(format!("unknown clip kind: {other}")),
    }
}

// ---------------------------------------------------------------------------
// Shared state: the core engines plus the persistence store.
// ---------------------------------------------------------------------------

pub struct AppState {
    clipboard: Mutex<ClipboardHistory>,
    notes: Mutex<Notes>,
    tasks: Mutex<TodoList>,
    paste: Mutex<PasteStack>,
    shelf: Mutex<FileShelf>,
    screen: Mutex<ScreenTime>,
    #[allow(dead_code)]
    pomodoro: Mutex<Pomodoro>,
    store: Mutex<MemStore>,
    /// Screen-time interval tracker driven by `foreground_tick` (SPEC C11).
    sampler: Mutex<Sampler>,
}

impl AppState {
    fn seeded() -> Self {
        // A little starter content so the shell isn't empty on first launch.
        let mut clipboard = ClipboardHistory::default();
        clipboard.add(ClipKind::Text, "Welcome to Krovi — your everything shelf", 0);
        clipboard.add(ClipKind::Link, "https://krovi.tech", 1);

        let mut notes = Notes::new();
        notes.create("First note — autosaves locally.", 0);

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

type CmdResult<T> = Result<T, String>;

fn lock_err<T>(_e: std::sync::PoisonError<T>) -> String {
    "internal lock error".to_string()
}

// ---------------------------------------------------------------------------
// Clipboard (SPEC C2)
// ---------------------------------------------------------------------------

fn clipboard_dtos(h: &ClipboardHistory) -> Vec<ClipDto> {
    h.visible()
        .into_iter()
        .map(|e| ClipDto {
            id: e.id,
            kind: kind_str(e.kind).to_string(),
            content: e.content.clone(),
            pinned: e.pinned,
            created_at: e.created_at,
        })
        .collect()
}

fn persist_clipboard(state: &AppState, h: &ClipboardHistory) -> CmdResult<()> {
    // Replace-semantics persistence: snapshot the whole collection to the store.
    let entries: Vec<_> = h.visible().into_iter().cloned().collect();
    let mut store = state.store.lock().map_err(lock_err)?;
    store.save_clipboard(&entries).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_clipboard(state: State<AppState>) -> CmdResult<Vec<ClipDto>> {
    let h = state.clipboard.lock().map_err(lock_err)?;
    Ok(clipboard_dtos(&h))
}

#[tauri::command]
fn add_clip(state: State<AppState>, kind: String, content: String, now: u64) -> CmdResult<Vec<ClipDto>> {
    let kind = parse_kind(&kind)?;
    if content.trim().is_empty() {
        return Err("empty clipboard content".into());
    }
    let mut h = state.clipboard.lock().map_err(lock_err)?;
    h.add(kind, content, now);
    persist_clipboard(&state, &h)?;
    Ok(clipboard_dtos(&h))
}

#[tauri::command]
fn toggle_clip_pin(state: State<AppState>, id: u64) -> CmdResult<Vec<ClipDto>> {
    let mut h = state.clipboard.lock().map_err(lock_err)?;
    if !h.toggle_pin(id) {
        return Err(format!("unknown clip id: {id}"));
    }
    persist_clipboard(&state, &h)?;
    Ok(clipboard_dtos(&h))
}

#[tauri::command]
fn remove_clip(state: State<AppState>, id: u64) -> CmdResult<Vec<ClipDto>> {
    let mut h = state.clipboard.lock().map_err(lock_err)?;
    h.remove(id);
    persist_clipboard(&state, &h)?;
    Ok(clipboard_dtos(&h))
}

// ---------------------------------------------------------------------------
// Paste stack (SPEC O2)
// ---------------------------------------------------------------------------

fn paste_vec(p: &PasteStack) -> Vec<String> {
    p.slots().into_iter().map(|(_, v)| v.to_string()).collect()
}

#[tauri::command]
fn load_paste_stack(state: State<AppState>) -> CmdResult<Vec<String>> {
    let p = state.paste.lock().map_err(lock_err)?;
    Ok(paste_vec(&p))
}

#[tauri::command]
fn push_paste(state: State<AppState>, value: String) -> CmdResult<Vec<String>> {
    if value.is_empty() {
        return Err("nothing to push".into());
    }
    let mut p = state.paste.lock().map_err(lock_err)?;
    p.push(value);
    Ok(paste_vec(&p))
}

#[tauri::command]
fn pop_paste(state: State<AppState>) -> CmdResult<Vec<String>> {
    let mut p = state.paste.lock().map_err(lock_err)?;
    p.pop();
    Ok(paste_vec(&p))
}

// ---------------------------------------------------------------------------
// Notes (SPEC C8)
// ---------------------------------------------------------------------------

fn note_dtos(n: &Notes) -> Vec<NoteDto> {
    n.list()
        .iter()
        .map(|x| NoteDto {
            id: x.id,
            body: x.body.clone(),
            created_at: x.created_at,
            updated_at: x.updated_at,
        })
        .collect()
}

fn persist_notes(state: &AppState, n: &Notes) -> CmdResult<()> {
    let mut store = state.store.lock().map_err(lock_err)?;
    store.save_notes(n.list()).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_notes(state: State<AppState>) -> CmdResult<Vec<NoteDto>> {
    let n = state.notes.lock().map_err(lock_err)?;
    Ok(note_dtos(&n))
}

#[tauri::command]
fn create_note(state: State<AppState>, body: String, now: u64) -> CmdResult<Vec<NoteDto>> {
    let mut n = state.notes.lock().map_err(lock_err)?;
    n.create(body, now);
    persist_notes(&state, &n)?;
    Ok(note_dtos(&n))
}

#[tauri::command]
fn update_note(state: State<AppState>, id: u64, body: String, now: u64) -> CmdResult<Vec<NoteDto>> {
    let mut n = state.notes.lock().map_err(lock_err)?;
    if !n.update(id, body, now) {
        return Err(format!("unknown note id: {id}"));
    }
    persist_notes(&state, &n)?;
    Ok(note_dtos(&n))
}

#[tauri::command]
fn delete_note(state: State<AppState>, id: u64) -> CmdResult<Vec<NoteDto>> {
    let mut n = state.notes.lock().map_err(lock_err)?;
    n.remove(id);
    persist_notes(&state, &n)?;
    Ok(note_dtos(&n))
}

// ---------------------------------------------------------------------------
// To-do (SPEC O5)
// ---------------------------------------------------------------------------

fn task_dtos(t: &TodoList) -> Vec<TaskDto> {
    t.tasks()
        .iter()
        .map(|x| TaskDto {
            id: x.id,
            title: x.title.clone(),
            done: x.done,
            created_at: x.created_at,
            completed_at: x.completed_at,
        })
        .collect()
}

fn persist_tasks(state: &AppState, t: &TodoList) -> CmdResult<()> {
    let mut store = state.store.lock().map_err(lock_err)?;
    store.save_tasks(t.tasks()).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_tasks(state: State<AppState>) -> CmdResult<Vec<TaskDto>> {
    let t = state.tasks.lock().map_err(lock_err)?;
    Ok(task_dtos(&t))
}

#[tauri::command]
fn add_task(state: State<AppState>, title: String, now: u64) -> CmdResult<Vec<TaskDto>> {
    if title.trim().is_empty() {
        return Err("empty task title".into());
    }
    let mut t = state.tasks.lock().map_err(lock_err)?;
    t.add(title, now);
    persist_tasks(&state, &t)?;
    Ok(task_dtos(&t))
}

#[tauri::command]
fn toggle_task(state: State<AppState>, id: u64, now: u64) -> CmdResult<Vec<TaskDto>> {
    let mut t = state.tasks.lock().map_err(lock_err)?;
    if !t.toggle(id, now) {
        return Err(format!("unknown task id: {id}"));
    }
    persist_tasks(&state, &t)?;
    Ok(task_dtos(&t))
}

// ---------------------------------------------------------------------------
// File shelf (SPEC C4) and screen time (SPEC C11) — read bridges.
// ---------------------------------------------------------------------------

#[tauri::command]
fn load_shelves(state: State<AppState>) -> CmdResult<Vec<ShelfDto>> {
    let fs = state.shelf.lock().map_err(lock_err)?;
    Ok(stored_shelves(&fs)
        .into_iter()
        .map(|s| ShelfDto {
            name: s.name,
            items: s
                .items
                .into_iter()
                .map(|i| ShelfItemDto {
                    id: i.id,
                    name: i.name,
                    stored_path: i.stored_path,
                    size_bytes: i.size_bytes,
                    added_at: i.added_at,
                })
                .collect(),
        })
        .collect())
}

#[tauri::command]
fn load_screen_time(state: State<AppState>) -> CmdResult<Vec<ScreenTimeDto>> {
    let st = state.screen.lock().map_err(lock_err)?;
    Ok(screen_time_records(&st)
        .into_iter()
        .map(|r| ScreenTimeDto {
            day: r.day,
            app: r.app,
            ms: r.ms,
        })
        .collect())
}

// ---------------------------------------------------------------------------
// UI-only tools (no core module yet): weather, media, clocks, FX, calendar.
// These return seeded JSON so the same frontend renders under Tauri. The real
// integrations (keyless weather/FX APIs, GSMTC media) land in krovi-net/sys.
// ---------------------------------------------------------------------------

#[tauri::command]
fn load_weather() -> serde_json::Value {
    serde_json::json!({
        "city": "Pune", "current": 27, "condition": "Partly cloudy", "icon": "partly",
        "high": 31, "low": 22, "feelsLike": 29, "humidity": 58, "windKph": 11, "uvIndex": 5,
        "hourly": [
            {"time":"Now","temp":27,"icon":"partly"},{"time":"13:00","temp":29,"icon":"sun"},
            {"time":"14:00","temp":30,"icon":"sun"},{"time":"15:00","temp":30,"icon":"partly"},
            {"time":"16:00","temp":28,"icon":"cloud"},{"time":"17:00","temp":26,"icon":"cloud"}
        ],
        "forecast": [
            {"label":"Today","icon":"partly","low":22,"high":31},
            {"label":"Thu","icon":"sun","low":23,"high":33},
            {"label":"Fri","icon":"partly","low":22,"high":32},
            {"label":"Sat","icon":"rain","low":21,"high":28},
            {"label":"Sun","icon":"cloud","low":21,"high":29}
        ]
    })
}

#[tauri::command]
fn load_media() -> serde_json::Value {
    serde_json::json!({
        "title": "Midnight Drive", "artist": "Neon Harbor", "album": "After Hours",
        "playing": true, "positionSec": 72, "durationSec": 214
    })
}

#[tauri::command]
fn load_world_clocks() -> serde_json::Value {
    serde_json::json!([
        {"label":"Local","tz":"UTC"},
        {"label":"San Francisco","tz":"America/Los_Angeles"},
        {"label":"New York","tz":"America/New_York"},
        {"label":"London","tz":"Europe/London"},
        {"label":"Mumbai","tz":"Asia/Kolkata"},
        {"label":"Tokyo","tz":"Asia/Tokyo"}
    ])
}

#[tauri::command]
fn load_currency_rates() -> serde_json::Value {
    serde_json::json!({
        "USD": 1.0, "EUR": 0.92, "GBP": 0.79, "INR": 83.2, "JPY": 149.5, "AUD": 1.52, "CAD": 1.36
    })
}

#[tauri::command]
fn load_calendar() -> serde_json::Value {
    serde_json::json!([])
}

// ---------------------------------------------------------------------------
// Integration commands (SPEC C5/C7/C10/C11). These are the thin bridges over
// krovi-net (keyless weather + FX — the app's ONLY outbound calls) and krovi-sys
// (read-only GSMTC media, foreground-process screen-time sampling). All product
// logic — URL building, parsing, conversion math, caching, interval tracking —
// lives in those crates; these commands only validate, call, and return typed
// JSON. The live network/OS paths require the `net`/`native` features (built in
// CI); without them the crates return a disabled/absent result, so the shell
// still links and the seed commands above keep the demo UI populated.
// ---------------------------------------------------------------------------

/// Fetch live weather for a coordinate (SPEC C7). The coordinate is validated
/// and **coarsened** before any request, so only a coarse lat/long leaves the
/// device — the only weather data ever sent.
#[tauri::command]
fn fetch_weather(lat: f64, lon: f64) -> CmdResult<WeatherData> {
    if !krovi_sys::valid_coords(lat, lon) {
        return Err("invalid coordinates".into());
    }
    let (lat, lon) = krovi_sys::coarsen(lat, lon);
    krovi_net::fetch_forecast(lat, lon).map_err(|e| e.to_string())
}

/// Fetch the latest keyless FX rates for `base` (SPEC C10). Only the base code
/// is sent. The frontend caches the result and reuses it offline via
/// [`convert_currency`].
#[tauri::command]
fn fetch_currency_rates(base: String) -> CmdResult<Rates> {
    if base.trim().is_empty() {
        return Err("empty base currency".into());
    }
    krovi_net::fetch_rates(&base).map_err(|e| e.to_string())
}

/// Convert an amount between currencies using already-fetched (cached) rates —
/// no network call, works offline (SPEC C10).
#[tauri::command]
fn convert_currency(amount: f64, from: String, to: String, rates: Rates) -> CmdResult<f64> {
    krovi_net::convert(amount, &from, &to, &rates).map_err(|e| e.to_string())
}

/// Read the current system media session (SPEC C5): track metadata + transport
/// state. Read-only — no audio capture. `None` when nothing is playing (or in a
/// build without the `native` feature).
#[tauri::command]
fn now_playing() -> CmdResult<Option<MediaTrack>> {
    krovi_sys::read_now_playing().map_err(|e| e.to_string())
}

/// Send a transport command to the active media session (SPEC C5):
/// `play` / `pause` / `playpause` / `next` / `previous`.
#[tauri::command]
fn media_transport(command: String) -> CmdResult<()> {
    let cmd = TransportCommand::from_str(&command)
        .ok_or_else(|| format!("unknown transport command: {command}"))?;
    krovi_sys::send_transport(cmd).map_err(|e| e.to_string())
}

/// Sample the foreground app for screen-time (SPEC C11). Called on a timer with
/// the current `now` (epoch ms). Credits the previously-focused app with the
/// elapsed time, records it locally, and persists the aggregate. Returns the
/// recorded slice (or `None` if nothing to credit yet). No titles/URLs, and
/// nothing is ever reported off-device.
#[tauri::command]
fn foreground_tick(state: State<AppState>, now: u64) -> CmdResult<Option<ScreenTimeDto>> {
    let process = krovi_sys::foreground_process().map_err(|e| e.to_string())?;
    let mut sampler = state.sampler.lock().map_err(lock_err)?;
    match sampler.tick(process, now) {
        Some((day, app, ms)) => {
            let mut st = state.screen.lock().map_err(lock_err)?;
            st.record(day, app.as_str(), ms);
            let records = screen_time_records(&st);
            let mut store = state.store.lock().map_err(lock_err)?;
            store.save_screen_time(&records).map_err(|e| e.to_string())?;
            Ok(Some(ScreenTimeDto { day, app, ms }))
        }
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Window summon/dismiss, wired to a global shortcut (default Ctrl+Alt+Space).
// ---------------------------------------------------------------------------

#[cfg(desktop)]
fn toggle_window(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        match win.is_visible() {
            Ok(true) => {
                let _ = win.hide();
            }
            _ => {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }
    }
}

/// Entry point shared by the desktop binary (and mobile, if added later).
pub fn run() {
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());
    }

    builder
        .manage(AppState::seeded())
        .setup(|app| {
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
                let summon = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
                let handle = app.handle().clone();
                app.global_shortcut().on_shortcut(summon, move |_app, _scut, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_window(&handle);
                    }
                })?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_clipboard,
            add_clip,
            toggle_clip_pin,
            remove_clip,
            load_paste_stack,
            push_paste,
            pop_paste,
            load_notes,
            create_note,
            update_note,
            delete_note,
            load_tasks,
            add_task,
            toggle_task,
            load_shelves,
            load_screen_time,
            load_weather,
            load_media,
            load_world_clocks,
            load_currency_rates,
            load_calendar,
            fetch_weather,
            fetch_currency_rates,
            convert_currency,
            now_playing,
            media_transport,
            foreground_tick
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Krovi application");
}
