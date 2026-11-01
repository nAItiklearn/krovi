//! Data commands (notes, tasks, paste stack, shelves, screen time, weather, FX,
//! calendar, clipboard CRUD). Owner: live-data agent (see docs/CONTRACTS.md).
//! Thin: validate → call krovi-core → persist → return typed DTOs.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use krovi_core::{ClipKind, ClipboardHistory, Notes, PasteStack, TodoList};
use krovi_net::{Rates, WeatherData};
use krovi_store::{screen_time_records, stored_shelves, Store};
use krovi_sys::{MediaTrack, TransportCommand};

use crate::state::AppState;
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
pub fn load_clipboard(state: State<AppState>) -> CmdResult<Vec<ClipDto>> {
    let h = state.clipboard.lock().map_err(lock_err)?;
    Ok(clipboard_dtos(&h))
}

#[tauri::command]
pub fn add_clip(
    state: State<AppState>,
    kind: String,
    content: String,
    now: u64,
) -> CmdResult<Vec<ClipDto>> {
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
pub fn toggle_clip_pin(state: State<AppState>, id: u64) -> CmdResult<Vec<ClipDto>> {
    let mut h = state.clipboard.lock().map_err(lock_err)?;
    if !h.toggle_pin(id) {
        return Err(format!("unknown clip id: {id}"));
    }
    persist_clipboard(&state, &h)?;
    Ok(clipboard_dtos(&h))
}

#[tauri::command]
pub fn remove_clip(state: State<AppState>, id: u64) -> CmdResult<Vec<ClipDto>> {
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
pub fn load_paste_stack(state: State<AppState>) -> CmdResult<Vec<String>> {
    let p = state.paste.lock().map_err(lock_err)?;
    Ok(paste_vec(&p))
}

#[tauri::command]
pub fn push_paste(state: State<AppState>, value: String) -> CmdResult<Vec<String>> {
    if value.is_empty() {
        return Err("nothing to push".into());
    }
    let mut p = state.paste.lock().map_err(lock_err)?;
    p.push(value);
    Ok(paste_vec(&p))
}

#[tauri::command]
pub fn pop_paste(state: State<AppState>) -> CmdResult<Vec<String>> {
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
pub fn load_notes(state: State<AppState>) -> CmdResult<Vec<NoteDto>> {
    let n = state.notes.lock().map_err(lock_err)?;
    Ok(note_dtos(&n))
}

#[tauri::command]
pub fn create_note(state: State<AppState>, body: String, now: u64) -> CmdResult<Vec<NoteDto>> {
    let mut n = state.notes.lock().map_err(lock_err)?;
    n.create(body, now);
    persist_notes(&state, &n)?;
    Ok(note_dtos(&n))
}

#[tauri::command]
pub fn update_note(state: State<AppState>, id: u64, body: String, now: u64) -> CmdResult<Vec<NoteDto>> {
    let mut n = state.notes.lock().map_err(lock_err)?;
    if !n.update(id, body, now) {
        return Err(format!("unknown note id: {id}"));
    }
    persist_notes(&state, &n)?;
    Ok(note_dtos(&n))
}

#[tauri::command]
pub fn delete_note(state: State<AppState>, id: u64) -> CmdResult<Vec<NoteDto>> {
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
pub fn load_tasks(state: State<AppState>) -> CmdResult<Vec<TaskDto>> {
    let t = state.tasks.lock().map_err(lock_err)?;
    Ok(task_dtos(&t))
}

#[tauri::command]
pub fn add_task(state: State<AppState>, title: String, now: u64) -> CmdResult<Vec<TaskDto>> {
    if title.trim().is_empty() {
        return Err("empty task title".into());
    }
    let mut t = state.tasks.lock().map_err(lock_err)?;
    t.add(title, now);
    persist_tasks(&state, &t)?;
    Ok(task_dtos(&t))
}

#[tauri::command]
pub fn toggle_task(state: State<AppState>, id: u64, now: u64) -> CmdResult<Vec<TaskDto>> {
    let mut t = state.tasks.lock().map_err(lock_err)?;
    if !t.toggle(id, now) {
        return Err(format!("unknown task id: {id}"));
    }
    persist_tasks(&state, &t)?;
    Ok(task_dtos(&t))
}

// ---------------------------------------------------------------------------
// File shelf (SPEC C4) and screen time (SPEC C11) â€” read bridges.
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn load_shelves(state: State<AppState>) -> CmdResult<Vec<ShelfDto>> {
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
pub fn load_screen_time(state: State<AppState>) -> CmdResult<Vec<ScreenTimeDto>> {
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
pub fn load_weather() -> serde_json::Value {
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
pub fn load_media() -> serde_json::Value {
    serde_json::json!({
        "title": "Midnight Drive", "artist": "Neon Harbor", "album": "After Hours",
        "playing": true, "positionSec": 72, "durationSec": 214
    })
}

#[tauri::command]
pub fn load_world_clocks() -> serde_json::Value {
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
pub fn load_currency_rates() -> serde_json::Value {
    serde_json::json!({
        "USD": 1.0, "EUR": 0.92, "GBP": 0.79, "INR": 83.2, "JPY": 149.5, "AUD": 1.52, "CAD": 1.36
    })
}

#[tauri::command]
pub fn load_calendar() -> serde_json::Value {
    serde_json::json!([])
}

// ---------------------------------------------------------------------------
// Integration commands (SPEC C5/C7/C10/C11). These are the thin bridges over
// krovi-net (keyless weather + FX â€” the app's ONLY outbound calls) and krovi-sys
// (read-only GSMTC media, foreground-process screen-time sampling). All product
// logic â€” URL building, parsing, conversion math, caching, interval tracking â€”
// lives in those crates; these commands only validate, call, and return typed
// JSON. The live network/OS paths require the `net`/`native` features (built in
// CI); without them the crates return a disabled/absent result, so the shell
// still links and the seed commands above keep the demo UI populated.
// ---------------------------------------------------------------------------

/// Fetch live weather for a coordinate (SPEC C7). The coordinate is validated
/// and **coarsened** before any request, so only a coarse lat/long leaves the
/// device â€” the only weather data ever sent.
#[tauri::command]
pub fn fetch_weather(lat: f64, lon: f64) -> CmdResult<WeatherData> {
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
pub fn fetch_currency_rates(base: String) -> CmdResult<Rates> {
    if base.trim().is_empty() {
        return Err("empty base currency".into());
    }
    krovi_net::fetch_rates(&base).map_err(|e| e.to_string())
}

/// Convert an amount between currencies using already-fetched (cached) rates â€”
/// no network call, works offline (SPEC C10).
#[tauri::command]
pub fn convert_currency(amount: f64, from: String, to: String, rates: Rates) -> CmdResult<f64> {
    krovi_net::convert(amount, &from, &to, &rates).map_err(|e| e.to_string())
}

/// Read the current system media session (SPEC C5): track metadata + transport
/// state. Read-only â€” no audio capture. `None` when nothing is playing (or in a
/// build without the `native` feature).
#[tauri::command]
pub fn now_playing() -> CmdResult<Option<MediaTrack>> {
    krovi_sys::read_now_playing().map_err(|e| e.to_string())
}

/// Send a transport command to the active media session (SPEC C5):
/// `play` / `pause` / `playpause` / `next` / `previous`.
#[tauri::command]
pub fn media_transport(command: String) -> CmdResult<()> {
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
pub fn foreground_tick(state: State<AppState>, now: u64) -> CmdResult<Option<ScreenTimeDto>> {
    let process = krovi_sys::foreground_process().map_err(|e| e.to_string())?;
    let mut sampler = state.sampler.lock().map_err(lock_err)?;
    match sampler.tick(process, now) {
        Some((day, app, ms)) => {
            let mut st = state.screen.lock().map_err(lock_err)?;
            st.record(day, app.as_str(), ms);
            let records = screen_time_records(&st);
            let mut store = state.store.lock().map_err(lock_err)?;
            store
                .save_screen_time(&records)
                .map_err(|e| e.to_string())?;
            Ok(Some(ScreenTimeDto { day, app, ms }))
        }
        None => Ok(None),
    }
}

/// Shared entry point for other modules (voice): create a note, persist it, and
/// notify the UI with `notes-changed` (payload: NoteDto[]).
pub fn create_note_from(app: &AppHandle, body: &str) -> CmdResult<Vec<NoteDto>> {
    let state = app.state::<AppState>();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let list = {
        let mut n = state.notes.lock().map_err(lock_err)?;
        n.create(body.to_string(), now);
        persist_notes(&state, &n)?;
        note_dtos(&n)
    };
    let _ = app.emit("notes-changed", &list);
    Ok(list)
}
