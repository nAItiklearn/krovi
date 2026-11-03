//! Circle to Ask and the temporary screen-awareness session (docs/DECISIONS.md D-021).
//!
//! Thin Tauri glue. The rules that matter live in crates with unit tests:
//! `krovi_sys::capture` (clipping, one-shot GDI capture), `krovi_sys::ocr` (Windows OCR),
//! `krovi_sys::session` (the 30-minute cap, the 2-minute image slot), `krovi_sys::web` (the only
//! addresses that may be opened) and `krovi_net::vision` (the BYOK request builders and error
//! classification).
//!
//! ## Privacy shape (non-negotiable)
//! * Nothing captures at startup, on a timer or in the background. `setup` only reads settings
//!   and registers the shortcut. There is no capture loop anywhere in this file.
//! * **Circle to Ask**: the shortcut (default `Ctrl+Alt+C`) shows a dimmed overlay on the monitor
//!   under the cursor. Only when the user releases a selection does Rust hide the overlay, wait a
//!   moment, and capture *that rectangle*, once. Esc, right-click, the shortcut again, or 90
//!   seconds of nothing cancel it without capturing.
//! * **Screen awareness**: only after `aware_start` (an explicit user action), for at most 30
//!   minutes (hard cap, monotonic clock), announced by the `aware-state` event, ended by
//!   `aware_stop`. The screen is captured only inside `aware_ask`, when the user asks a question.
//! * Frames live in memory only: nothing is written to disk, nothing is logged (no type here
//!   derives `Debug` for pixel or text data), and a circled image is dropped after 2 minutes, on
//!   `circle_clear`, or when the next one replaces it.
//! * Krovi's own windows are excluded from the frame (`SetWindowDisplayAffinity`) so the pill or
//!   panel does not cover what the user is asking about.
//! * Nothing leaves the PC unless the user has saved a key for a provider (Credential Manager,
//!   target `Krovi/ai/<provider>`) **and** confirms, for that one request, the disclosure that
//!   names the provider. Pictures never go to any other host (`krovi_net::vision`).
//! * **Which provider and model** is no longer chosen here: Settings -> AI (`ai.rs`, the `ai.v1`
//!   row) owns the choice, the keys and the Gemini notice / Snowflake account gates. This module
//!   asks `crate::ai` which provider is usable and which model reads pictures. (`circle.v1` once
//!   had `provider` and `models`; `ai::setup` carries them over once and they are ignored here.)
//!
//! ## Commands
//! circle_start, circle_cancel, circle_overlay_ready, circle_capture, circle_ask_vision,
//! circle_clear, circle_search, circle_ocr_status, circle_get_settings, circle_set_settings,
//! circle_set_key, circle_open_keys_page, aware_start, aware_stop, aware_status, aware_ask.
//!
//! ## Events
//! * `circle-arm` `{token, x, y, width, height}`: sent to the overlay window when it is armed again.
//! * `circle-result` (a [`ReadResult`]): a circled area was read (or could not be). The main window
//!   listens, opens the panel and shows `AskPanel`.
//! * `aware-state` (an [`AwareStatus`], `reason` = started | stopped | expired): drives the pill's
//!   always-visible "screen awareness is on" indicator.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use krovi_net::vision::{self, ImagePart, Prompt, Provider};
use krovi_sys::capture::{self, Image, Rect, UPLOAD_MAX_EDGE};
use krovi_sys::ocr::{self, OcrLine};
use krovi_sys::session::{self, Session, Slot, SLOT_TTL};
use krovi_sys::web;

use crate::ai::keys as ai_keys;
use crate::ai::{fail, AiFailure, NO_PROVIDER};
use crate::commands::{setting_get, setting_set, CmdResult};
use crate::state::AppState;

const KEY: &str = "circle.v1";
/// Window label of the selection overlay (also listed in `capabilities/default.json`).
const LABEL: &str = "circle";
/// The overlay is the same bundle as the app, told apart by the URL fragment (`src/main.ts`).
const OVERLAY_URL: &str = "index.html#circle";
/// After hiding the overlay, wait this long for Windows to stop drawing it before capturing.
const HIDE_SETTLE: Duration = Duration::from_millis(70);
/// After excluding our own windows from capture, wait for the compositor to apply it.
const EXCLUDE_SETTLE: Duration = Duration::from_millis(90);
/// An overlay nobody finished with cancels itself after this long.
const ARM_TIMEOUT: Duration = Duration::from_secs(90);
/// Ignore a repeated shortcut press (key repeat) for this long.
const PRESS_DEBOUNCE: Duration = Duration::from_millis(400);

// ---------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct CircleSettings {
    /// Registers the Circle to Ask shortcut. Nothing is captured until the user uses it.
    pub enabled: bool,
    pub shortcut: String,
    /// Default length of an awareness session (1 to 30).
    pub session_minutes: u32,
}

impl Default for CircleSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            shortcut: "Ctrl+Alt+C".into(),
            session_minutes: session::DEFAULT_SESSION_MINUTES,
        }
    }
}

/// Check settings from the webview and return the cleaned copy to store.
fn validate(s: &CircleSettings) -> Result<CircleSettings, String> {
    let mut s = s.clone();
    s.shortcut = s.shortcut.trim().to_string();
    crate::shell::parse_shortcut(&s.shortcut)?;
    s.session_minutes =
        session::clamp_minutes(Some(s.session_minutes), session::DEFAULT_SESSION_MINUTES);
    Ok(s)
}

// AI keys, provider choice and models: see `crate::ai` (Settings -> AI). `ai_keys` (imported
// above) is that module's Credential Manager helpers under the name this file always used.

// ---------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Overlay {
    /// The monitor the overlay covers, in physical virtual-screen pixels.
    monitor: Rect,
    token: u64,
}

struct AwareRun {
    session: Session,
    provider: Option<Provider>,
    /// Dropping this wakes the timer thread so it exits at once.
    _stop: mpsc::Sender<()>,
}

pub struct Circle {
    settings: Mutex<CircleSettings>,
    registered: Mutex<Option<Shortcut>>,
    shortcut_error: Mutex<Option<String>>,
    last_press: Mutex<Instant>,
    /// `Some` while the overlay is up and waiting for a selection.
    overlay: Mutex<Option<Overlay>>,
    /// The circled image, held for an optional "Ask AI" for at most [`SLOT_TTL`].
    slot: Mutex<Slot<Image>>,
    aware: Mutex<Option<AwareRun>>,
    seq: AtomicU64,
    /// A capture is being read right now (one at a time).
    busy: Arc<AtomicBool>,
}

fn lk<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Busy(Arc<AtomicBool>);

impl Busy {
    fn try_take(flag: &Arc<AtomicBool>) -> Option<Busy> {
        (!flag.swap(true, Ordering::SeqCst)).then(|| Busy(flag.clone()))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------------------
// Wire types (camelCase). None derives `Debug`: they carry screen text.
// ---------------------------------------------------------------------------------------

/// What was read from the screen. Sent as the `circle-result` event and returned by the commands.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReadResult {
    pub ok: bool,
    /// `circle` (a circled area) or `aware` (a session question).
    pub source: &'static str,
    pub text: String,
    pub lines: Vec<OcrLine>,
    pub width: u32,
    pub height: u32,
    pub language: Option<String>,
    /// Why no text was read although the picture was captured (for example no OCR language is
    /// installed). "Ask AI" can still work because a vision model reads the picture itself.
    pub ocr_error: Option<String>,
    /// The captured area is one flat colour (protected video, or nothing there).
    pub blank: bool,
    pub error: Option<String>,
    /// Seconds the picture stays available to "Ask AI" (0 = not held).
    pub expires_in_secs: u64,
}

impl ReadResult {
    fn failed(source: &'static str, error: impl Into<String>) -> Self {
        ReadResult {
            ok: false,
            source,
            text: String::new(),
            lines: Vec::new(),
            width: 0,
            height: 0,
            language: None,
            ocr_error: None,
            blank: false,
            error: Some(error.into()),
            expires_in_secs: 0,
        }
    }
}

// `AiFailure` (a failed or refused AI request: `kind` is a kebab-case `ErrorKind` such as `auth`,
// `billing`, `quota`, `rate-limit`, `eligibility`, `model`, `blocked`, `unavailable`, `network`,
// `other`, or one of ours: `no-provider`, `consent-required`, `expired`, `inactive`,
// `not-allowed`), `fail` and `NO_PROVIDER` live in `crate::ai` and are imported above.

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AiReply {
    pub answer: String,
    pub truncated: bool,
    pub provider: &'static str,
    pub provider_name: &'static str,
    pub model: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: &'static str,
    pub name: &'static str,
    /// The provider's default model for pictures.
    pub default_model: &'static str,
    /// The model that will read the picture (Settings -> AI's choice if it can see, else the
    /// provider's image model).
    pub model: String,
    /// Here "has a key" means ready to use: a key is saved **and** nothing else is missing
    /// (Gemini's notice accepted, Snowflake's account URL). Whether a key alone is saved is for
    /// Settings -> AI to show.
    pub has_key: bool,
    pub key_hint: Option<String>,
}

/// The sentences the UI must show, word for word, before anything is sent.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Disclosure {
    pub provider: &'static str,
    pub provider_name: &'static str,
    pub model: String,
    pub image: String,
    pub screen: String,
}

fn disclosure_for(p: Provider, model: String) -> Disclosure {
    let name = p.display_name();
    // A free Gemini key feeds Google's product improvement and human review (AI_PROVIDERS.md 9.4).
    let free_tier = if p == Provider::Gemini {
        " If your key is on the free tier, Google may use and review this image."
    } else {
        ""
    };
    Disclosure {
        provider: p.id(),
        provider_name: name,
        model,
        image: format!("This sends the selected image to {name} using your key.{free_tier}"),
        screen: format!(
            "This sends a picture of your screen to {name} using your key, each time you ask with the picture option on.{free_tier}"
        ),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CircleStatus {
    pub settings: CircleSettings,
    pub providers: Vec<ProviderInfo>,
    /// The provider "Ask AI" would use right now; `None` when no provider has a saved key.
    pub active_provider: Option<&'static str>,
    /// The disclosure for `active_provider`.
    pub disclosure: Option<Disclosure>,
    /// One disclosure per provider that has a saved key (for a provider picker).
    pub disclosures: Vec<Disclosure>,
    pub max_session_minutes: u32,
    pub shortcut_error: Option<String>,
}

fn status_of(c: &Circle) -> CircleStatus {
    let settings = lk(&c.settings).clone();
    let providers: Vec<ProviderInfo> = Provider::ALL
        .into_iter()
        .map(|p| {
            let (_, key_hint) = ai_keys::status(p);
            ProviderInfo {
                id: p.id(),
                name: p.display_name(),
                default_model: p.default_vision_model(),
                model: crate::ai::vision_model(p),
                has_key: crate::ai::usable(p),
                key_hint,
            }
        })
        .collect();
    let active = vision::pick_provider(crate::ai::selected(), |p| {
        providers.iter().any(|i| i.id == p.id() && i.has_key)
    });
    let disclosure = active.map(|p| disclosure_for(p, crate::ai::vision_model(p)));
    let disclosures = Provider::ALL
        .into_iter()
        .filter(|p| providers.iter().any(|i| i.id == p.id() && i.has_key))
        .map(|p| disclosure_for(p, crate::ai::vision_model(p)))
        .collect();
    CircleStatus {
        providers,
        active_provider: active.map(Provider::id),
        disclosure,
        disclosures,
        max_session_minutes: session::MAX_SESSION_MINUTES,
        shortcut_error: lk(&c.shortcut_error).clone(),
        settings,
    }
}

/// The provider to send to: the one asked for (which must be usable: a key is saved and any
/// notice or account step is done), otherwise the one chosen in Settings -> AI or the first
/// usable provider.
fn resolve_provider(requested: Option<Provider>) -> Result<Provider, AiFailure> {
    match requested {
        Some(p) => crate::ai::check_usable(p).map(|()| p),
        None => vision::pick_provider(crate::ai::selected(), crate::ai::usable)
            .ok_or_else(|| fail("no-provider", format!("{NO_PROVIDER}."))),
    }
}

// ---------------------------------------------------------------------------------------
// Capture + OCR
// ---------------------------------------------------------------------------------------

fn own_hwnds(app: &AppHandle) -> Vec<(&'static str, isize)> {
    ["main", LABEL]
        .into_iter()
        .filter_map(|label| {
            let w = app.get_webview_window(label)?;
            let h = w.hwnd().ok()?;
            Some((label, h.0 as isize))
        })
        .collect()
}

/// One still image of `region` with Krovi's own windows left out of it. Windows that cannot be
/// excluded (older Windows, or the call is refused) stay in the frame, except that with
/// `hide_fallback` the main window is hidden for the moment of capture if it is on screen.
fn capture_clear_of_krovi(
    app: &AppHandle,
    region: Rect,
    hide_fallback: bool,
) -> Result<Image, String> {
    let mut excluded = Vec::new();
    let mut hidden_main = false;
    for (label, hwnd) in own_hwnds(app) {
        if capture::exclude_window_from_capture(hwnd, true) {
            excluded.push(hwnd);
        } else if hide_fallback && label == "main" {
            if let Some(w) = app.get_webview_window("main") {
                if w.is_visible().unwrap_or(false) {
                    let _ = w.hide();
                    hidden_main = true;
                }
            }
        }
    }
    if !excluded.is_empty() || hidden_main {
        thread::sleep(EXCLUDE_SETTLE);
    }

    let result = capture::capture_region(region);

    for hwnd in excluded {
        capture::exclude_window_from_capture(hwnd, false);
    }
    if hidden_main {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.show();
        }
    }
    result.map_err(|e| e.to_string())
}

/// Capture `region`, then read its text. The image is returned (never stored here) so the caller
/// decides whether to hold it.
fn read_region(
    app: &AppHandle,
    region: Rect,
    source: &'static str,
    hide_fallback: bool,
) -> (Option<Image>, ReadResult) {
    let image = match capture_clear_of_krovi(app, region, hide_fallback) {
        Ok(i) => i,
        Err(e) => return (None, ReadResult::failed(source, e)),
    };
    let blank = image.is_uniform();
    let (text, lines, language, ocr_error) = match ocr::recognize(&image) {
        Ok(o) => (o.text, o.lines, o.language, None),
        Err(e) => (String::new(), Vec::new(), None, Some(e.to_string())),
    };
    let result = ReadResult {
        ok: true,
        source,
        text,
        lines,
        width: image.width,
        height: image.height,
        language,
        ocr_error,
        blank,
        error: None,
        expires_in_secs: 0,
    };
    (Some(image), result)
}

/// Keep the circled image for [`SLOT_TTL`], then drop it.
fn hold(app: &AppHandle, image: Image) {
    let c = app.state::<Circle>();
    let generation = lk(&c.slot).put(image, Instant::now());
    let app2 = app.clone();
    thread::spawn(move || {
        thread::sleep(SLOT_TTL + Duration::from_secs(1));
        lk(&app2.state::<Circle>().slot).clear_generation(generation);
    });
}

// ---------------------------------------------------------------------------------------
// Circle to Ask: overlay lifecycle
// ---------------------------------------------------------------------------------------

fn arm_payload(o: &Overlay) -> serde_json::Value {
    json!({
        "token": o.token,
        "x": o.monitor.x,
        "y": o.monitor.y,
        "width": o.monitor.w,
        "height": o.monitor.h,
    })
}

fn place(w: &tauri::WebviewWindow, m: Rect) {
    let pos = tauri::PhysicalPosition::new(m.x, m.y);
    let size = tauri::PhysicalSize::new(m.w.max(1) as u32, m.h.max(1) as u32);
    // Position, size, position again: moving between monitors of different DPI can rescale the
    // window after the first call.
    let _ = w.set_position(pos);
    let _ = w.set_size(size);
    let _ = w.set_position(pos);
}

fn hide_overlay(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
    }
}

fn cancel(app: &AppHandle) {
    let c = app.state::<Circle>();
    *lk(&c.overlay) = None;
    hide_overlay(app);
}

/// Arm the overlay on the monitor under the cursor. A second call while it is up cancels it.
/// Must not run on the main thread when it has to create the window (the shortcut handler and the
/// async command both call it from a worker).
fn start_overlay(app: &AppHandle) -> Result<(), String> {
    let c = app.state::<Circle>();
    if !lk(&c.settings).enabled {
        return Err("Circle to Ask is turned off.".into());
    }
    if c.busy.load(Ordering::SeqCst) {
        return Err("Still reading the last area.".into());
    }
    let window = app.get_webview_window(LABEL);
    let showing = window
        .as_ref()
        .is_some_and(|w| w.is_visible().unwrap_or(false));
    if lk(&c.overlay).is_some() && showing {
        cancel(app); // the shortcut again = cancel
        return Ok(());
    }

    let monitor = capture::monitor_at_cursor()
        .or_else(capture::primary_monitor)
        .or_else(capture::virtual_screen)
        .ok_or("Windows did not report any screen.")?;
    let token = c.seq.fetch_add(1, Ordering::SeqCst) + 1;
    let ov = Overlay { monitor, token };
    *lk(&c.overlay) = Some(ov);

    match window {
        Some(w) => {
            place(&w, monitor);
            // The page is already loaded: tell it to reset; it answers with circle_overlay_ready.
            let _ = app.emit_to(LABEL, "circle-arm", arm_payload(&ov));
        }
        None => {
            // First use: build the window hidden. Its page calls circle_overlay_ready when it
            // has mounted, and only then is it shown.
            let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(OVERLAY_URL.into()))
                .title("Krovi \u{2013} Circle to Ask")
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .maximizable(false)
                .minimizable(false)
                .visible(false)
                .focused(false)
                .build();
            match built {
                Ok(w) => place(&w, monitor),
                Err(e) => {
                    *lk(&c.overlay) = None;
                    return Err(format!("Couldn't open the selection overlay ({e})."));
                }
            }
        }
    }

    // An overlay nobody finished with goes away by itself.
    let app2 = app.clone();
    thread::spawn(move || {
        thread::sleep(ARM_TIMEOUT);
        let still = lk(&app2.state::<Circle>().overlay).is_some_and(|o| o.token == token);
        if still {
            cancel(&app2);
        }
    });
    Ok(())
}

/// Called by the overlay page once it has mounted (or has been re-armed). Shows the window and
/// returns where it sits; `None` means nothing is armed and the page should stay out of the way.
#[tauri::command]
pub fn circle_overlay_ready(app: AppHandle) -> Option<serde_json::Value> {
    let c = app.state::<Circle>();
    let ov = *lk(&c.overlay);
    let Some(ov) = ov else {
        hide_overlay(&app);
        return None;
    };
    if let Some(w) = app.get_webview_window(LABEL) {
        place(&w, ov.monitor);
        let _ = w.show();
        let _ = w.set_always_on_top(true);
        let _ = w.set_focus();
    }
    Some(arm_payload(&ov))
}

/// Show the selection overlay (the same as the shortcut). Used by a button in the panel.
#[tauri::command]
pub async fn circle_start(app: AppHandle) -> CmdResult<()> {
    start_overlay(&app)
}

/// Close the overlay without capturing anything (Esc, right-click).
#[tauri::command]
pub fn circle_cancel(app: AppHandle) {
    cancel(&app);
}

#[derive(Deserialize)]
pub struct PxRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The user released a selection. `rect` is in **physical pixels relative to the overlay's
/// top-left corner** (= the monitor's top-left). Rust hides the overlay, waits for Windows to
/// remove it, captures that rectangle once, reads its text and holds the picture for two minutes.
///
/// Errors (`Err`) mean nothing was captured and the overlay is still up (a bad rectangle, or not
/// armed). A capture or OCR problem comes back as `Ok` with `ok: false` / `ocrError`, after the
/// overlay has already gone.
#[tauri::command]
pub async fn circle_capture(app: AppHandle, rect: PxRect) -> CmdResult<ReadResult> {
    let local = Rect::from_f64(rect.x, rect.y, rect.width, rect.height)
        .ok_or("That selection isn't valid.")?;
    let c = app.state::<Circle>();
    let Some(guard) = Busy::try_take(&c.busy) else {
        return Err("Still reading the last area.".into());
    };
    // Exactly one capture per arming: consume it.
    let ov = lk(&c.overlay)
        .take()
        .ok_or("Circle to Ask isn't active. Press the shortcut again.")?;
    let region = local
        .translated(ov.monitor.x, ov.monitor.y)
        .ok_or("That selection isn't valid.")?;
    hide_overlay(&app);

    let app2 = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _busy = guard;
        thread::sleep(HIDE_SETTLE);
        let (image, mut result) = read_region(&app2, region, "circle", false);
        if let Some(image) = image {
            hold(&app2, image);
            result.expires_in_secs = SLOT_TTL.as_secs();
        }
        let _ = app2.emit("circle-result", &result);
        result
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(result)
}

/// Drop the held picture now (the panel's "Discard" button and when it closes).
#[tauri::command]
pub fn circle_clear(app: AppHandle) {
    lk(&app.state::<Circle>().slot).clear();
}

// ---------------------------------------------------------------------------------------
// Ask AI (BYOK)
// ---------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskArgs {
    pub question: String,
    /// The user confirmed the disclosure that names the provider, for this request.
    #[serde(default)]
    pub confirm: bool,
    /// Provider id; `null` = the preferred / first provider with a key.
    #[serde(default)]
    pub provider: Option<Provider>,
}

fn ask_provider(
    provider: Provider,
    model: &str,
    question: &str,
    png_b64: &str,
) -> Result<AiReply, AiFailure> {
    let key = ai_keys::read(provider)
        .map_err(|m| fail("other", m))?
        .ok_or_else(|| fail("no-provider", format!("{NO_PROVIDER}.")))?;
    let prompt = Prompt::new(
        question,
        Some(ImagePart {
            mime: "image/png",
            base64: png_b64,
        }),
    );
    let a = crate::ai::ask_vision(provider, &key, model, &prompt)?;
    Ok(AiReply {
        answer: a.text,
        truncated: a.truncated,
        provider: provider.id(),
        provider_name: provider.display_name(),
        model: vision::clean_model(provider, model)
            .unwrap_or_else(|_| provider.default_vision_model().to_string()),
    })
}

/// Send the held circled picture and a question to the user's AI provider. Needs a saved key and
/// `confirm: true` (the UI shows `disclosure.image` first). One request, nothing kept.
#[tauri::command]
pub async fn circle_ask_vision(app: AppHandle, args: AskArgs) -> Result<AiReply, AiFailure> {
    if !args.confirm {
        return Err(fail(
            "consent-required",
            "Confirm that you want to send the image to the AI provider.",
        ));
    }
    let c = app.state::<Circle>();
    let provider = resolve_provider(args.provider)?;
    let model = crate::ai::vision_model(provider);
    let png_b64 = {
        let mut slot = lk(&c.slot);
        slot.get(Instant::now())
            .and_then(|img| img.to_png_base64(UPLOAD_MAX_EDGE))
    }
    .ok_or_else(|| {
        fail(
            "expired",
            "The selected area is no longer held (it is kept for two minutes). Circle it again.",
        )
    })?;
    let question = args.question;
    tauri::async_runtime::spawn_blocking(move || {
        ask_provider(provider, &model, &question, &png_b64)
    })
    .await
    .map_err(|e| fail("other", e.to_string()))?
}

// ---------------------------------------------------------------------------------------
// Small commands: search, OCR status, key pages
// ---------------------------------------------------------------------------------------

/// Open a Google search for `text` in the browser (the first 300 characters). The address is
/// built here; the webview cannot choose a URL.
#[tauri::command]
pub fn circle_search(text: String) -> CmdResult<()> {
    let url = web::google_search_url(&text).ok_or("There is no text to search for.")?;
    web::open_url(&url)
}

/// Open the page where the user creates an API key for `provider`.
#[tauri::command]
pub fn circle_open_keys_page(provider: String) -> CmdResult<()> {
    let url = web::keys_page(provider.trim()).ok_or("Unknown AI provider.")?;
    web::open_url(url)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrStatus {
    pub available: bool,
    pub language: Option<String>,
    pub message: Option<String>,
}

/// Is an OCR language installed? (Creates a recogniser; reads no screen.)
#[tauri::command]
pub async fn circle_ocr_status() -> CmdResult<OcrStatus> {
    tauri::async_runtime::spawn_blocking(|| match ocr::available_language() {
        Ok(language) => OcrStatus {
            available: true,
            language: Some(language),
            message: None,
        },
        Err(e) => OcrStatus {
            available: false,
            language: None,
            message: Some(e.to_string()),
        },
    })
    .await
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------------------
// Settings + keys
// ---------------------------------------------------------------------------------------

#[tauri::command]
pub fn circle_get_settings(app: AppHandle) -> CircleStatus {
    status_of(&app.state::<Circle>())
}

#[tauri::command]
pub fn circle_set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: CircleSettings,
) -> CmdResult<CircleStatus> {
    let s = validate(&settings)?;
    register(&app, &s)?;
    setting_set(&state, KEY, &s)?;
    let c = app.state::<Circle>();
    *lk(&c.settings) = s;
    if !lk(&c.settings).enabled {
        cancel(&app);
    }
    Ok(status_of(&c))
}

/// Thin wrapper over `crate::ai` (Settings -> AI owns key entry in the UI): save (`key = Some`,
/// with the same checks and gates as `ai_set_key`) or remove (`key = None`) the key for
/// `provider` in Credential Manager.
#[tauri::command]
pub fn circle_set_key(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: String,
    key: Option<String>,
) -> CmdResult<CircleStatus> {
    let p = Provider::from_id(&provider).ok_or("Unknown AI provider.")?;
    match key {
        Some(raw) => crate::ai::store_key(&state, p, &raw).map_err(|f| f.message)?,
        None => {
            ai_keys::delete(p)?;
        }
    }
    Ok(status_of(&app.state::<Circle>()))
}

// ---------------------------------------------------------------------------------------
// Shortcut
// ---------------------------------------------------------------------------------------

fn register(app: &AppHandle, s: &CircleSettings) -> Result<(), String> {
    let c = app.state::<Circle>();
    if let Some(old) = lk(&c.registered).take() {
        let _ = app.global_shortcut().unregister(old);
    }
    *lk(&c.shortcut_error) = None;
    if !s.enabled {
        return Ok(());
    }
    let sc = crate::shell::parse_shortcut(&s.shortcut)?;
    let registered = app
        .global_shortcut()
        .on_shortcut(sc, move |app, _sc, ev| {
            if ev.state() != ShortcutState::Pressed {
                return;
            }
            {
                let c = app.state::<Circle>();
                let mut last = lk(&c.last_press);
                if last.elapsed() < PRESS_DEBOUNCE {
                    return;
                }
                *last = Instant::now();
            }
            // Creating a window must not happen on the thread that delivers the shortcut.
            let app = app.clone();
            thread::spawn(move || {
                if let Err(e) = start_overlay(&app) {
                    let _ = app.emit("circle-result", ReadResult::failed("circle", e));
                }
            });
        })
        .map_err(|e| {
            format!(
                "Could not use {} as the Circle to Ask shortcut: {e}",
                s.shortcut
            )
        });
    if let Err(e) = &registered {
        *lk(&c.shortcut_error) = Some(e.clone());
    }
    registered?;
    *lk(&c.registered) = Some(sc);
    Ok(())
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Settings and a shortcut registration only. Nothing is captured here, no window is created
    // and no key is read.
    let settings: CircleSettings = setting_get(&app.state::<AppState>(), KEY).unwrap_or_default();
    let settings = validate(&settings).unwrap_or_default();
    app.manage(Circle {
        settings: Mutex::new(settings.clone()),
        registered: Mutex::new(None),
        shortcut_error: Mutex::new(None),
        last_press: Mutex::new(Instant::now() - Duration::from_secs(10)),
        overlay: Mutex::new(None),
        slot: Mutex::new(Slot::new(SLOT_TTL)),
        aware: Mutex::new(None),
        seq: AtomicU64::new(0),
        busy: Arc::new(AtomicBool::new(false)),
    });
    // A taken shortcut must not stop Krovi from starting; the status carries the error.
    if let Err(e) = register(app.handle(), &settings) {
        eprintln!("[krovi] circle shortcut: {e}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Screen awareness: a session the user starts, with an end
// ---------------------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AwareStatus {
    pub active: bool,
    /// Unix epoch milliseconds (display only; the end is decided on the monotonic clock).
    pub started_at: Option<u64>,
    pub ends_at: Option<u64>,
    pub remaining_secs: u64,
    pub max_minutes: u32,
    /// The user allowed pictures of the screen to be sent to `provider_name` during this session.
    pub vision: bool,
    pub provider_name: Option<&'static str>,
    /// On `aware-state` events: `started` | `stopped` | `expired`.
    pub reason: Option<&'static str>,
}

fn aware_status_of(run: Option<&AwareRun>, reason: Option<&'static str>) -> AwareStatus {
    match run {
        Some(r) => AwareStatus {
            active: true,
            started_at: Some(r.session.started_ms),
            ends_at: Some(r.session.ends_ms),
            remaining_secs: r.session.remaining(Instant::now()).as_secs(),
            max_minutes: session::MAX_SESSION_MINUTES,
            vision: r.session.vision,
            provider_name: r.provider.map(Provider::display_name),
            reason,
        },
        None => AwareStatus {
            active: false,
            started_at: None,
            ends_at: None,
            remaining_secs: 0,
            max_minutes: session::MAX_SESSION_MINUTES,
            vision: false,
            provider_name: None,
            reason,
        },
    }
}

/// The tray tooltip is the one indicator that does not depend on the pill being on screen.
fn set_tray_note(app: &AppHandle, until: Option<u64>) {
    let Some(tray) = app.tray_by_id("krovi") else {
        return;
    };
    let base = format!(
        "Krovi ({})",
        crate::shell::shell_get_config(app.clone()).shortcut
    );
    let tip = match until {
        Some(_) => format!("{base} \u{2013} screen awareness is ON"),
        None => base,
    };
    let _ = tray.set_tooltip(Some(tip));
}

/// End the session `id` (or whichever is running when `id` is `None`). Idempotent.
fn end_session(app: &AppHandle, id: Option<u64>, reason: &'static str) {
    let c = app.state::<Circle>();
    let ended = {
        let mut g = lk(&c.aware);
        if g.as_ref()
            .is_some_and(|r| id.is_none_or(|i| r.session.id == i))
        {
            g.take()
        } else {
            None
        }
    };
    if ended.is_some() {
        drop(ended); // wakes the timer thread
        lk(&c.slot).clear(); // nothing from the session stays in memory
        set_tray_note(app, None);
        let _ = app.emit("aware-state", aware_status_of(None, Some(reason)));
    }
}

/// Start a screen-awareness session of `minutes` (default from settings; clamped to 1..=30).
/// `vision` records that the user agreed, after reading the disclosure, that pictures of the
/// screen may be sent to their AI provider while the session lasts (still only when they ask a
/// question with the picture option on). Nothing is captured by starting.
#[tauri::command]
pub fn aware_start(
    app: AppHandle,
    minutes: Option<u32>,
    vision: bool,
) -> Result<AwareStatus, AiFailure> {
    let c = app.state::<Circle>();
    let provider = if vision {
        Some(resolve_provider(None)?)
    } else {
        None
    };
    let default_minutes = lk(&c.settings).session_minutes;
    let minutes = session::clamp_minutes(minutes, default_minutes);
    let id = c.seq.fetch_add(1, Ordering::SeqCst) + 1;
    let sess = Session::start(id, krovi_sys::now_ms(), Instant::now(), minutes, vision);
    let (tx, rx) = mpsc::channel::<()>();
    {
        let mut g = lk(&c.aware);
        if g.is_some() {
            return Err(fail(
                "other",
                "A screen-awareness session is already running. Stop it first.",
            ));
        }
        *g = Some(AwareRun {
            session: sess.clone(),
            provider,
            _stop: tx,
        });
    }

    // Ends the session on time. Stopping drops the sender, which wakes this thread at once.
    let app2 = app.clone();
    let left = sess.remaining(Instant::now());
    let spawned = thread::Builder::new()
        .name("krovi-aware-timer".into())
        .spawn(move || {
            if let Err(mpsc::RecvTimeoutError::Timeout) = rx.recv_timeout(left) {
                end_session(&app2, Some(id), "expired");
            }
        });
    if let Err(e) = spawned {
        // Without its timer a session could outlive its limit: do not run one.
        end_session(&app, Some(id), "stopped");
        return Err(fail("other", format!("Couldn't start the session ({e}).")));
    }

    set_tray_note(&app, Some(sess.ends_ms));
    let status = aware_status_of(lk(&c.aware).as_ref(), Some("started"));
    let _ = app.emit("aware-state", status.clone());
    Ok(status)
}

/// One click: the session ends now.
#[tauri::command]
pub fn aware_stop(app: AppHandle) {
    end_session(&app, None, "stopped");
}

#[tauri::command]
pub fn aware_status(app: AppHandle) -> AwareStatus {
    let c = app.state::<Circle>();
    // A session whose time is up but whose timer has not fired yet is already over.
    let over = lk(&c.aware)
        .as_ref()
        .is_some_and(|r| r.session.is_over(Instant::now()));
    if over {
        end_session(&app, None, "expired");
    }
    let g = lk(&c.aware);
    aware_status_of(g.as_ref(), None)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwareReply {
    pub read: ReadResult,
    pub answer: Option<AiReply>,
    pub ai_error: Option<AiFailure>,
}

/// Ask a question about the screen during a session: captures the primary monitor (where the pill
/// is) once, right now, reads its text and, only if `vision` is true **and** the session was
/// started with the picture option, also asks the AI provider about the picture. The picture is
/// dropped when this returns.
#[tauri::command]
pub async fn aware_ask(
    app: AppHandle,
    question: String,
    vision: bool,
) -> Result<AwareReply, AiFailure> {
    let c = app.state::<Circle>();
    let (over, allowed, provider) = {
        let g = lk(&c.aware);
        match g.as_ref() {
            None => {
                return Err(fail(
                    "inactive",
                    "Start a screen-awareness session first. Krovi doesn't look at the screen otherwise.",
                ))
            }
            Some(r) => (r.session.is_over(Instant::now()), r.session.vision, r.provider),
        }
    };
    if over {
        end_session(&app, None, "expired");
        return Err(fail("inactive", "The screen-awareness session has ended."));
    }
    let provider = if vision {
        if !allowed {
            return Err(fail(
                "not-allowed",
                "This session was started without the picture option. Stop it and start a new one to allow sending pictures.",
            ));
        }
        let p = provider.ok_or_else(|| fail("no-provider", format!("{NO_PROVIDER}.")))?;
        crate::ai::check_usable(p)?;
        Some(p)
    } else {
        None
    };
    let model = provider.map(crate::ai::vision_model);
    let Some(guard) = Busy::try_take(&c.busy) else {
        return Err(fail("other", "Still reading the last capture."));
    };
    let region = capture::primary_monitor()
        .or_else(capture::virtual_screen)
        .ok_or_else(|| fail("other", "Windows did not report any screen."))?;

    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = guard;
        let (image, read) = read_region(&app2, region, "aware", true);
        let (mut answer, mut ai_error) = (None, None);
        if let (Some(image), Some(p), Some(model)) = (image.as_ref(), provider, model) {
            match image.to_png_base64(UPLOAD_MAX_EDGE) {
                Some(b64) => match ask_provider(p, &model, &question, &b64) {
                    Ok(a) => answer = Some(a),
                    Err(e) => ai_error = Some(e),
                },
                None => ai_error = Some(fail("other", "The picture could not be prepared.")),
            }
        }
        // `image` drops here: zeroed, never stored.
        AwareReply {
            read,
            answer,
            ai_error,
        }
    })
    .await
    .map_err(|e| fail("other", e.to_string()))
}
