//! Data commands (clipboard CRUD, paste stack, notes, tasks, shelves, screen time,
//! calendar, weather, FX). Owner: live-data agent (see docs/CONTRACTS.md).
//!
//! Thin: validate -> call krovi-core -> persist -> return typed DTOs. **No command
//! here returns fabricated data**: collections come from the persisted engines,
//! weather and FX come from the keyless fetches (cached in the store, flagged stale
//! when offline), and an unknown value is `null`/empty — never a placeholder.
//!
//! Anything that can block (network, WinRT location) is an `async` command that
//! hops onto `spawn_blocking`, because synchronous Tauri commands run on the main
//! thread and would freeze the window.

use std::collections::BTreeMap;

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use krovi_core::{ClipboardHistory, Notes, PasteStack, TodoList};
use krovi_net::{Cached, FxReply, Place, PlaceDto, Rates, WeatherData, WeatherReply};
use krovi_store::wire::{calendar_dtos, clip_dtos, parse_clip_kind, CalendarDto};
use krovi_store::{screen_time_records, stored_shelves};
use krovi_sys::{MediaState, MediaTrack};

pub use krovi_store::wire::ClipDto;

use crate::state::AppState;

// ---------------------------------------------------------------------------
// Serializable DTOs (the wire shape mirrors src/lib/types.ts exactly). The
// clipboard and calendar shapes live in `krovi_store::wire`, the weather/FX replies
// in `krovi_net::reply` — both unit-tested there.
// ---------------------------------------------------------------------------

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

pub(crate) type CmdResult<T> = Result<T, String>;

pub(crate) fn lock_err<T>(_e: std::sync::PoisonError<T>) -> String {
    "internal lock error".to_string()
}

pub(crate) fn now_ms() -> u64 {
    krovi_sys::now_ms()
}

/// Run blocking work off the main thread.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}

// ---------------------------------------------------------------------------
// Persisted settings as JSON (weather place + caches, FX cache)
// ---------------------------------------------------------------------------

fn setting_get<T: DeserializeOwned>(state: &AppState, key: &str) -> Option<T> {
    let store = state.store.lock().ok()?;
    let raw = store.get_setting(key).ok().flatten()?;
    serde_json::from_str(&raw).ok()
}

fn setting_set<T: Serialize>(state: &AppState, key: &str, value: &T) -> CmdResult<()> {
    let raw = serde_json::to_string(value).map_err(|e| e.to_string())?;
    let mut store = state.store.lock().map_err(lock_err)?;
    store.set_setting(key, &raw).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Clipboard (SPEC C2). The OS watcher lives in live.rs; these are the CRUD
// commands. Every mutation emits `clipboard-changed`.
// ---------------------------------------------------------------------------

pub(crate) fn clipboard_dtos(h: &ClipboardHistory) -> Vec<ClipDto> {
    clip_dtos(h)
}

pub(crate) fn persist_clipboard(state: &AppState, h: &ClipboardHistory) -> CmdResult<()> {
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
    app: AppHandle,
    state: State<AppState>,
    kind: String,
    content: String,
    now: u64,
) -> CmdResult<Vec<ClipDto>> {
    let kind = parse_clip_kind(&kind)?;
    if content.trim().is_empty() {
        return Err("empty clipboard content".into());
    }
    let (dtos, id) = {
        let mut h = state.clipboard.lock().map_err(lock_err)?;
        let id = h.add(kind, content, now);
        persist_clipboard(&state, &h)?;
        (clipboard_dtos(&h), id)
    };
    crate::live::emit_clipboard(&app, &dtos, Some(id));
    Ok(dtos)
}

#[tauri::command]
pub fn toggle_clip_pin(app: AppHandle, state: State<AppState>, id: u64) -> CmdResult<Vec<ClipDto>> {
    let dtos = {
        let mut h = state.clipboard.lock().map_err(lock_err)?;
        if !h.toggle_pin(id) {
            return Err(format!("unknown clip id: {id}"));
        }
        persist_clipboard(&state, &h)?;
        clipboard_dtos(&h)
    };
    crate::live::emit_clipboard(&app, &dtos, None);
    Ok(dtos)
}

#[tauri::command]
pub fn remove_clip(app: AppHandle, state: State<AppState>, id: u64) -> CmdResult<Vec<ClipDto>> {
    let dtos = {
        let mut h = state.clipboard.lock().map_err(lock_err)?;
        h.remove(id);
        persist_clipboard(&state, &h)?;
        clipboard_dtos(&h)
    };
    crate::live::prune_clip_images(&app);
    crate::live::emit_clipboard(&app, &dtos, None);
    Ok(dtos)
}

// ---------------------------------------------------------------------------
// Paste stack (SPEC O2) — session-scoped by design (not persisted).
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
pub fn update_note(
    state: State<AppState>,
    id: u64,
    body: String,
    now: u64,
) -> CmdResult<Vec<NoteDto>> {
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

/// Shared entry point for other modules (voice): create a note, persist it, and
/// notify the UI with `notes-changed` (payload: NoteDto[]).
#[allow(dead_code)] // called by voice.rs (voice agent)
pub fn create_note_from(app: &AppHandle, body: &str) -> CmdResult<Vec<NoteDto>> {
    let state = app.state::<AppState>();
    let list = {
        let mut n = state.notes.lock().map_err(lock_err)?;
        n.create(body.to_string(), now_ms());
        persist_notes(&state, &n)?;
        note_dtos(&n)
    };
    let _ = app.emit("notes-changed", &list);
    Ok(list)
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
// File shelf (SPEC C4) and screen time (SPEC C11)
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

/// Close the previous foreground interval, credit it, and persist the aggregate.
/// Driven by the 5 s background tick in `live.rs`; the `foreground_tick` command
/// below calls the same code so a UI-driven tick can never double count (the
/// sampler credits only the time since the previous tick, whoever made it).
pub(crate) fn tick_screen_time(state: &AppState, now: u64) -> CmdResult<Option<ScreenTimeDto>> {
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

/// Sample the foreground app for screen-time (SPEC C11). The live hub already does
/// this every 5 s, so the UI does not need to call it; kept for compatibility.
#[tauri::command]
pub fn foreground_tick(state: State<AppState>, now: u64) -> CmdResult<Option<ScreenTimeDto>> {
    tick_screen_time(&state, now)
}

// ---------------------------------------------------------------------------
// Calendar (persisted)
// ---------------------------------------------------------------------------

fn current_calendar(state: &AppState) -> CmdResult<Vec<CalendarDto>> {
    let c = state.calendar.lock().map_err(lock_err)?;
    Ok(calendar_dtos(c.list()))
}

#[tauri::command]
pub fn load_calendar(state: State<AppState>) -> CmdResult<Vec<CalendarDto>> {
    current_calendar(&state)
}

// A timed event is a reminder (SPEC C6, CONTRACTS §8): creation goes through
// `reminders::add_event`, which applies the reminder defaults (alert on, sound on), switches
// the alert off for a time already past, persists, and wakes the scheduler.
#[tauri::command]
pub fn add_calendar_event(
    app: AppHandle,
    date: String,
    title: String,
    time: Option<String>,
) -> CmdResult<Vec<CalendarDto>> {
    crate::reminders::add_event(&app, &date, &title, time.as_deref(), Default::default())
}

/// Shared entry point for other modules (voice): add a calendar reminder, persist it, and
/// notify the UI with `calendar-changed` (payload: CalendarDto[], chronological).
/// Same validation as `add_calendar_event` (real `yyyy-mm-dd` date, optional `HH:MM`, non-empty title).
/// With a time it is a reminder: it wakes the scheduler and alerts (toast + sound) at that time.
pub fn add_calendar_event_from(
    app: &AppHandle,
    date: &str,
    time: Option<&str>,
    title: &str,
) -> CmdResult<Vec<CalendarDto>> {
    let list = crate::reminders::add_event(app, date, title, time, Default::default())?;
    let _ = app.emit("calendar-changed", &list);
    Ok(list)
}

#[tauri::command]
pub fn remove_calendar_event(
    app: AppHandle,
    state: State<AppState>,
    id: u64,
) -> CmdResult<Vec<CalendarDto>> {
    {
        let mut c = state.calendar.lock().map_err(lock_err)?;
        if !c.remove(id) {
            return Err(format!("unknown calendar event id: {id}"));
        }
        let mut store = state.store.lock().map_err(lock_err)?;
        store.save_calendar(c.list()).map_err(|e| e.to_string())?;
    }
    crate::reminders::wake_scheduler(&app);
    current_calendar(&state)
}

// ---------------------------------------------------------------------------
// Weather (SPEC C7): real Open-Meteo data, cached in the store.
//
// Place: the user's saved city, else Windows' coarse location, else "needs-city"
// (the UI asks for a city name). Only a coarse (0.1 degree) coordinate is ever sent
// to the forecast host; the only other outbound call is the optional city-name
// geocode when the user types a city.
// ---------------------------------------------------------------------------

const WEATHER_TTL_MS: u64 = 20 * 60_000;
const FX_TTL_MS: u64 = 6 * 3_600_000;
const AUTO_PLACE_TTL_MS: u64 = 6 * 3_600_000;
const K_PLACE: &str = "weather.place";
const K_WEATHER: &str = "weather.cache";
const K_FX: &str = "fx.cache";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct StoredPlace {
    city: String,
    lat: f64,
    lon: f64,
    /// Detected from Windows Location (re-detected periodically) vs. chosen by the user.
    #[serde(default)]
    auto: bool,
    #[serde(default)]
    set_at: u64,
}

/// Serializes weather/FX resolution: Home and the Weather view both ask at startup,
/// and the second caller should find the first one's fresh cache instead of repeating
/// the location lookup and the request.
static WEATHER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static FX_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Resolve the place, then the (cached or fetched) forecast, into the UI reply.
fn build_weather(app: &AppHandle, force: bool) -> WeatherReply {
    let _one_at_a_time = WEATHER_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let state = app.state::<AppState>();
    let now = now_ms();

    let mut place: Option<StoredPlace> = setting_get(&state, K_PLACE);
    let redetect = match &place {
        None => true,
        Some(p) => p.auto && now.saturating_sub(p.set_at) > AUTO_PLACE_TTL_MS,
    };
    if redetect {
        if let Ok(Some((lat, lon))) = krovi_sys::current_location() {
            if krovi_sys::valid_coords(lat, lon) {
                let p = StoredPlace {
                    city: "Current location".into(),
                    lat,
                    lon,
                    auto: true,
                    set_at: now,
                };
                let _ = setting_set(&state, K_PLACE, &p);
                place = Some(p);
            }
        }
    }
    let Some(place) = place else {
        return krovi_net::needs_city();
    };

    let (lat, lon) = krovi_sys::coarsen(place.lat, place.lon);
    let key = format!("{lat:.1},{lon:.1}");
    let cached: Option<Cached<WeatherData>> = setting_get(&state, K_WEATHER);
    let r = krovi_net::resolve(cached.as_ref(), &key, now, WEATHER_TTL_MS, force, || {
        krovi_net::fetch_forecast(lat, lon).map_err(|e| e.to_string())
    });
    if let Some(u) = &r.updated {
        let _ = setting_set(&state, K_WEATHER, u);
    }
    krovi_net::weather_reply(
        PlaceDto {
            city: place.city,
            lat,
            lon,
        },
        r,
    )
}

/// Current weather (or `null`: no place set / offline with nothing cached). Prefer
/// [`weather_status`], which also says *why* it is null.
#[tauri::command]
pub async fn load_weather(app: AppHandle) -> CmdResult<Option<WeatherData>> {
    blocking(move || Ok(build_weather(&app, false).weather)).await
}

/// Weather plus status/staleness/place. `force` bypasses the 20-minute cache.
#[tauri::command]
pub async fn weather_status(app: AppHandle, force: Option<bool>) -> CmdResult<WeatherReply> {
    blocking(move || Ok(build_weather(&app, force.unwrap_or(false)))).await
}

/// Look up candidate places for a typed city name (sends only that text).
#[tauri::command]
pub async fn weather_search(query: String) -> CmdResult<Vec<Place>> {
    blocking(move || krovi_net::search_places(&query).map_err(|e| e.to_string())).await
}

/// Choose a place (from [`weather_search`]) and load its weather.
#[tauri::command]
pub async fn weather_set_place(
    app: AppHandle,
    name: String,
    lat: f64,
    lon: f64,
) -> CmdResult<WeatherReply> {
    blocking(move || {
        if !krovi_sys::valid_coords(lat, lon) {
            return Err("invalid coordinates".into());
        }
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 120 {
            return Err("invalid place name".into());
        }
        let (lat, lon) = krovi_sys::coarsen(lat, lon);
        let p = StoredPlace {
            city: name.to_string(),
            lat,
            lon,
            auto: false,
            set_at: now_ms(),
        };
        setting_set(&app.state::<AppState>(), K_PLACE, &p)?;
        Ok(build_weather(&app, true))
    })
    .await
}

/// Geocode a typed city name, take the best match, and load its weather.
#[tauri::command]
pub async fn weather_set_city(app: AppHandle, query: String) -> CmdResult<WeatherReply> {
    blocking(move || {
        let places = krovi_net::search_places(&query).map_err(|e| e.to_string())?;
        let best = places
            .into_iter()
            .next()
            .ok_or_else(|| format!("No place found for \"{}\"", query.trim()))?;
        let (lat, lon) = krovi_sys::coarsen(best.lat, best.lon);
        let p = StoredPlace {
            city: best.name,
            lat,
            lon,
            auto: false,
            set_at: now_ms(),
        };
        setting_set(&app.state::<AppState>(), K_PLACE, &p)?;
        Ok(build_weather(&app, true))
    })
    .await
}

/// Fetch live weather for a coordinate (SPEC C7). The coordinate is validated and
/// **coarsened** before any request. Uncached; prefer [`weather_status`].
#[tauri::command]
pub async fn fetch_weather(lat: f64, lon: f64) -> CmdResult<WeatherData> {
    blocking(move || {
        if !krovi_sys::valid_coords(lat, lon) {
            return Err("invalid coordinates".into());
        }
        let (lat, lon) = krovi_sys::coarsen(lat, lon);
        krovi_net::fetch_forecast(lat, lon).map_err(|e| e.to_string())
    })
    .await
}

// ---------------------------------------------------------------------------
// Currency (SPEC C10): real keyless ECB rates, cached; works offline from the cache.
// ---------------------------------------------------------------------------

fn build_fx(app: &AppHandle, force: bool) -> FxReply {
    let _one_at_a_time = FX_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let state = app.state::<AppState>();
    let now = now_ms();
    let cached: Option<Cached<Rates>> = setting_get(&state, K_FX);
    let r = krovi_net::resolve(
        cached.as_ref(),
        krovi_net::DEFAULT_BASE,
        now,
        FX_TTL_MS,
        force,
        || krovi_net::fetch_rates(krovi_net::DEFAULT_BASE).map_err(|e| e.to_string()),
    );
    if let Some(u) = &r.updated {
        let _ = setting_set(&state, K_FX, u);
    }
    krovi_net::fx_reply(krovi_net::DEFAULT_BASE, r)
}

/// Rates per 1 USD as a flat table; empty when offline with nothing cached.
#[tauri::command]
pub async fn load_currency_rates(app: AppHandle) -> CmdResult<BTreeMap<String, f64>> {
    blocking(move || Ok(build_fx(&app, false).rates)).await
}

/// Rates plus when they were fetched, the publisher's date, and staleness/error.
#[tauri::command]
pub async fn load_currency_info(app: AppHandle, force: Option<bool>) -> CmdResult<FxReply> {
    blocking(move || Ok(build_fx(&app, force.unwrap_or(false)))).await
}

/// Fetch the latest keyless FX rates for `base` (SPEC C10), uncached. Only the base
/// code is sent. Prefer [`load_currency_info`].
#[tauri::command]
pub async fn fetch_currency_rates(base: String) -> CmdResult<Rates> {
    blocking(move || {
        if base.trim().is_empty() {
            return Err("empty base currency".into());
        }
        krovi_net::fetch_rates(&base).map_err(|e| e.to_string())
    })
    .await
}

/// Convert an amount between currencies using already-fetched (cached) rates —
/// no network call, works offline (SPEC C10).
#[tauri::command]
pub fn convert_currency(amount: f64, from: String, to: String, rates: Rates) -> CmdResult<f64> {
    krovi_net::convert(amount, &from, &to, &rates).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// World clocks: a preset list of IANA zones (configuration, not data). The webview
// knows the machine's own time zone, so `Local` carries an empty `tz` and the bridge
// fills it from `Intl`.
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn load_world_clocks() -> serde_json::Value {
    serde_json::json!([
        {"label":"Local","tz":""},
        {"label":"San Francisco","tz":"America/Los_Angeles"},
        {"label":"New York","tz":"America/New_York"},
        {"label":"London","tz":"Europe/London"},
        {"label":"Mumbai","tz":"Asia/Kolkata"},
        {"label":"Tokyo","tz":"Asia/Tokyo"}
    ])
}

// ---------------------------------------------------------------------------
// Media readers (the live feed is the `media-changed` event; the transport command
// lives in live.rs). These read the hub's latest sample — they never call WinRT on
// the command thread.
// ---------------------------------------------------------------------------

/// Latest media state (position advanced to now), or `null` when nothing is playing.
#[tauri::command]
pub fn load_media(state: State<AppState>) -> CmdResult<Option<MediaState>> {
    let m = state.live.media.lock().map_err(lock_err)?;
    Ok(m.as_ref().map(|s| s.advanced_to(now_ms())))
}

/// Legacy whole-second view of [`load_media`].
#[tauri::command]
pub fn now_playing(state: State<AppState>) -> CmdResult<Option<MediaTrack>> {
    let m = state.live.media.lock().map_err(lock_err)?;
    Ok(m.as_ref().map(|s| s.advanced_to(now_ms()).as_track()))
}
