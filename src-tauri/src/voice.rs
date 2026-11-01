//! Local voice (owner: voice): dictation -> notes, voice commands, point-and-ask context and
//! voice-to-action. See docs/CONTRACTS.md §3 and docs/DECISIONS.md D-012 / D-013.
//!
//! Thin Tauri layer over the `krovi-voice` crate, which does all recognition on-device through
//! the Windows SAPI 5 recognizer. Nothing here (or in the crate) touches the network.
//!
//! Commands:  `voice_available`, `voice_start(mode, captureContext?)`, `voice_stop`,
//!            `voice_transcribe_file(path, mode, captureContext?, context?)`,
//!            `voice_get_settings`, `voice_set_settings(settings)`,
//!            `voice_known_targets` (the closed folder/app lists), `voice_open_result(path)`
//! Events:    `voice-state {state, mode, error?}`, `voice-partial {text}`, `voice-result VoiceResult`
//!
//! What happens to a finished session:
//! * `note` mode: the (optionally cleaned) text becomes a note via `commands::create_note_from`.
//! * `command` mode: data actions whose state lives in Rust or in the OS are carried out here
//!   (`add-reminder` -> calendar, `media` -> the media session, and Voice v3: `open-folder`,
//!   `open-app`, `search-files`, `list-folder` -> Explorer / Start menu / Windows Search index /
//!   the file system) and the result says `executed: true` (v3 also fills `results`); everything
//!   that is UI state (open a tool, timers, pomodoro, shelf, Quick Transfer, creating the note for
//!   `new-note`, collapse) is only reported, and the UI acts.
//!
//! Voice v3 makes no network call: see docs/SECURITY-network-audit.md "Voice v3".

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use krovi_voice::{
    Catalog, ContextCapture, MediaCommand, Mode, ResultItem, ResultKind, Sink, StartOptions,
    State as VoiceState, StateChange, Voice, VoiceCommand, VoiceContext, VoiceResult,
    VoiceSettings,
};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::AppState;

const SETTINGS_KEY: &str = "voice.settings";

/// How long the folder / app catalog is trusted before the next session refreshes it in the
/// background (the session that notices uses the old one, so starting never waits).
const CATALOG_STALE: Duration = Duration::from_secs(10 * 60);

/// The folders and apps "open ..." may name, enumerated from the OS (Voice v3). Cached because
/// enumerating the Start menu takes ~1-2 s; refreshed on a background thread when stale.
struct CatalogCache {
    slot: Mutex<Option<(Arc<Catalog>, Instant)>>,
    refreshing: AtomicBool,
}

impl CatalogCache {
    fn new() -> Arc<Self> {
        Arc::new(CatalogCache {
            slot: Mutex::new(None),
            refreshing: AtomicBool::new(false),
        })
    }

    /// The catalog to use for a session starting now. Never blocks on the OS: with nothing cached
    /// yet it returns just the Known Folders (instant) and starts the full scan in the background.
    fn current(self: &Arc<Self>) -> Arc<Catalog> {
        let cached = self.slot.lock().ok().and_then(|s| s.clone());
        match cached {
            Some((catalog, at)) => {
                if at.elapsed() > CATALOG_STALE {
                    self.refresh_in_background();
                }
                catalog
            }
            None => {
                self.refresh_in_background();
                quick_catalog()
            }
        }
    }

    /// What is cached, if anything (does not trigger a scan).
    fn peek(&self) -> Option<(Arc<Catalog>, Instant)> {
        self.slot.lock().ok().and_then(|s| s.clone())
    }

    fn refresh_in_background(self: &Arc<Self>) {
        if self.refreshing.swap(true, Ordering::SeqCst) {
            return; // a scan is already running
        }
        let me = self.clone();
        let spawned = std::thread::Builder::new()
            .name("krovi-voice-catalog".into())
            .spawn(move || {
                if let Some(fresh) = scan_catalog() {
                    if let Ok(mut slot) = me.slot.lock() {
                        *slot = Some((Arc::new(fresh), Instant::now()));
                    }
                }
                me.refreshing.store(false, Ordering::SeqCst);
            });
        if spawned.is_err() {
            self.refreshing.store(false, Ordering::SeqCst);
        }
    }
}

#[cfg(all(windows, feature = "native"))]
fn scan_catalog() -> Option<Catalog> {
    Some(krovi_voice::os::scan_catalog())
}

#[cfg(not(all(windows, feature = "native")))]
fn scan_catalog() -> Option<Catalog> {
    None
}

#[cfg(all(windows, feature = "native"))]
fn quick_catalog() -> Arc<Catalog> {
    Arc::new(Catalog::new(krovi_voice::os::known_folders(), Vec::new()))
}

#[cfg(not(all(windows, feature = "native")))]
fn quick_catalog() -> Arc<Catalog> {
    Arc::new(Catalog::default())
}

/// Managed state: the single microphone controller.
pub struct VoiceHub {
    voice: Arc<Voice>,
    catalog: Arc<CatalogCache>,
    /// The rows of the latest result: the only things `voice_open_result` will open.
    last_results: Arc<Mutex<Vec<ResultItem>>>,
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let hub = VoiceHub {
        voice: Arc::new(Voice::new()),
        catalog: CatalogCache::new(),
        last_results: Arc::new(Mutex::new(Vec::new())),
    };
    // Warm the catalog a few seconds after launch, off the startup path, so the first spoken
    // "open ..." already knows the user's apps.
    let cache = hub.catalog.clone();
    let _ = std::thread::Builder::new()
        .name("krovi-voice-prewarm".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(4));
            cache.refresh_in_background();
        });
    app.manage(hub);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// settings (persisted in the app's local settings bag; never sent anywhere)
// ---------------------------------------------------------------------------------------------

fn load_settings(app: &AppHandle) -> VoiceSettings {
    let state = app.state::<AppState>();
    let raw = state
        .store
        .lock()
        .ok()
        .and_then(|s| s.get_setting(SETTINGS_KEY).ok().flatten());
    raw.and_then(|r| serde_json::from_str::<VoiceSettings>(&r).ok())
        .unwrap_or_default()
        .normalized()
}

fn save_settings(app: &AppHandle, settings: &VoiceSettings) -> Result<(), String> {
    let raw = serde_json::to_string(settings).map_err(|e| e.to_string())?;
    let state = app.state::<AppState>();
    let mut store = state
        .store
        .lock()
        .map_err(|_| "internal lock error".to_string())?;
    store
        .set_setting(SETTINGS_KEY, &raw)
        .map_err(|e| e.to_string())
}

/// The persisted voice privacy settings. `localOnly` is always true and `storeAudio` always
/// false: they are shown, but locked.
#[tauri::command]
pub fn voice_get_settings(app: AppHandle) -> VoiceSettings {
    load_settings(&app)
}

/// Save the voice settings; returns what was actually stored (locked values re-imposed).
#[tauri::command]
pub fn voice_set_settings(
    app: AppHandle,
    settings: VoiceSettings,
) -> Result<VoiceSettings, String> {
    let settings = settings.normalized();
    save_settings(&app, &settings)?;
    Ok(settings)
}

// ---------------------------------------------------------------------------------------------
// context capture (explicit voice command only, at the moment the session starts)
// ---------------------------------------------------------------------------------------------

struct Captured {
    context: Option<VoiceContext>,
    hint: Option<String>,
}

#[cfg(all(windows, feature = "native"))]
fn capture_context(app: &AppHandle, level: ContextCapture) -> Captured {
    let c = krovi_voice::context::capture(level, |fg| {
        // The opt-in Ctrl+C fallback touches the OS clipboard: do it under Krovi's clipboard I/O
        // lock and tell the change gate the new sequence number is ours, so the clipboard
        // history does not record the transient copy.
        let state = app.state::<AppState>();
        let mut gate = state.live.clip_io.lock().ok()?;
        let result = krovi_voice::context::copy_via_clipboard(fg);
        gate.acknowledge(krovi_sys::change_token());
        result.ok().flatten()
    });
    Captured {
        context: c.context,
        hint: c.hint,
    }
}

#[cfg(not(all(windows, feature = "native")))]
fn capture_context(_app: &AppHandle, _level: ContextCapture) -> Captured {
    Captured {
        context: None,
        hint: Some("Reading the selection is not available in this build.".into()),
    }
}

fn start_options(
    app: &AppHandle,
    settings: &VoiceSettings,
    mode: Mode,
    capture: bool,
    given: Option<VoiceContext>,
    catalog: Option<Arc<Catalog>>,
) -> StartOptions {
    let mut opts = StartOptions {
        clean_transcription: settings.clean_transcription,
        ..Default::default()
    };
    if mode == Mode::Command {
        opts.catalog = catalog;
        if let Some(ctx) = given {
            opts.context = Some(ctx);
        } else if capture {
            let c = capture_context(app, settings.context_capture);
            opts.context = c.context;
            opts.context_hint = c.hint;
        }
    }
    opts
}

// ---------------------------------------------------------------------------------------------
// events and actions
// ---------------------------------------------------------------------------------------------

/// Forwards everything a voice session produces to the UI, carries out the data actions that
/// live in Rust, and files finished dictation as a note.
struct TauriSink {
    app: AppHandle,
    mode: Mode,
    keep_transcripts: bool,
    /// Set when saving the note failed: the `error` state must not be wiped by the `idle` that
    /// the session emits right after its result.
    save_failed: AtomicBool,
    /// Where the rows of this result are remembered for `voice_open_result`.
    last_results: Arc<Mutex<Vec<ResultItem>>>,
}

impl TauriSink {
    fn new(
        app: AppHandle,
        mode: Mode,
        settings: &VoiceSettings,
        last_results: Arc<Mutex<Vec<ResultItem>>>,
    ) -> Self {
        TauriSink {
            app,
            mode,
            keep_transcripts: settings.keep_transcripts,
            save_failed: AtomicBool::new(false),
            last_results,
        }
    }

    /// Act on a finished result, emit `voice-result`, and return exactly what the UI received.
    fn deliver(&self, mut result: VoiceResult) -> VoiceResult {
        match result.mode {
            Mode::Note => {
                if !result.text.trim().is_empty() {
                    if let Err(e) = crate::commands::create_note_from(&self.app, &result.text) {
                        // The text is still in the result below, so the UI can show it; just say why it was not saved.
                        self.save_failed.store(true, Ordering::SeqCst);
                        let _ = self.app.emit(
                            "voice-state",
                            StateChange::error(Mode::Note, format!("Could not save the note: {e}")),
                        );
                    }
                }
            }
            Mode::Command => {
                execute(&self.app, &mut result);
                if let Ok(mut last) = self.last_results.lock() {
                    // Only rows this very result shows can be opened from the panel afterwards.
                    *last = result.results.clone();
                }
                if !self.keep_transcripts {
                    // "Keep transcripts" is off: the spoken words of a command are not retained
                    // anywhere. The parsed command (and what it did) is still reported.
                    result.text.clear();
                }
            }
        }
        let _ = self.app.emit("voice-result", &result);
        result
    }
}

impl Sink for TauriSink {
    fn state(&self, change: StateChange) {
        if change.state == VoiceState::Idle && self.save_failed.swap(false, Ordering::SeqCst) {
            return; // keep the "could not save" error visible
        }
        let _ = self.app.emit("voice-state", &change);
    }

    fn partial(&self, text: &str) {
        if self.mode == Mode::Command && !self.keep_transcripts {
            return; // no live transcript of a command either
        }
        let _ = self.app.emit("voice-partial", json!({ "text": text }));
    }

    fn result(&self, result: VoiceResult) {
        let _ = self.deliver(result);
    }
}

/// Carry out the commands whose state lives in Rust (or in the OS), and fill in `feedback`,
/// `executed` and `results`.
fn execute(app: &AppHandle, result: &mut VoiceResult) {
    match result.command.clone() {
        Some(VoiceCommand::AddReminder { date, time, title }) => {
            match crate::commands::add_calendar_event_from(app, &date, time.as_deref(), &title) {
                Ok(_) => {
                    result.executed = true;
                    // States the exact resolved date and time, so nothing is ambiguous.
                    result.feedback = Some(krovi_voice::actions::reminder_feedback(
                        &date,
                        time.as_deref(),
                        &title,
                        krovi_voice::LocalNow::system().date,
                    ));
                }
                Err(e) => result.feedback = Some(format!("Could not add the reminder: {e}")),
            }
        }
        Some(VoiceCommand::Media { command }) => run_media(app, command, result),
        Some(cmd) if krovi_voice::actions::is_os_command(&cmd) => run_os_action(result),
        _ => {}
    }
}

/// Voice v3: open a known folder, launch an installed app, search the Windows Search index, or
/// list a folder. All local; see docs/SECURITY-network-audit.md "Voice v3".
#[cfg(all(windows, feature = "native"))]
fn run_os_action(result: &mut VoiceResult) {
    krovi_voice::actions::apply(result, &krovi_voice::os::WinHost);
}

#[cfg(not(all(windows, feature = "native")))]
fn run_os_action(result: &mut VoiceResult) {
    result.feedback = Some("Opening folders and apps needs the native voice build.".into());
}

fn run_media(app: &AppHandle, command: MediaCommand, result: &mut VoiceResult) {
    if command == MediaCommand::WhatsPlaying {
        let state = app.state::<AppState>();
        let now = state.live.media.lock().ok().and_then(|m| m.clone());
        result.executed = true;
        result.feedback = Some(match now {
            Some(m) if !m.title.is_empty() => {
                let by = if m.artist.is_empty() {
                    String::new()
                } else {
                    format!(" by {}", m.artist)
                };
                format!(
                    "{}: {}{by}",
                    if m.playing { "Now playing" } else { "Paused" },
                    m.title
                )
            }
            _ => "Nothing is playing.".to_string(),
        });
        return;
    }
    // Same logic the Media tool uses (crate::live::media_transport -> the GSMTC worker).
    let word = command.as_str().to_string();
    match tauri::async_runtime::block_on(crate::live::media_transport(app.clone(), word)) {
        Ok(()) => {
            result.executed = true;
            result.feedback = Some(
                match command {
                    MediaCommand::Next => "Skipped to the next track.",
                    MediaCommand::Previous => "Back to the previous track.",
                    MediaCommand::Pause => "Paused.",
                    MediaCommand::Play => "Playing.",
                    _ => "Toggled playback.",
                }
                .to_string(),
            );
        }
        Err(_) => result.feedback = Some("No media is playing right now.".to_string()),
    }
}

fn parse_mode(mode: &str) -> Result<Mode, String> {
    Mode::parse(mode)
        .ok_or_else(|| format!("unknown voice mode {mode:?} (expected \"note\" or \"command\")"))
}

// ---------------------------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------------------------

/// What the on-device engine can do. Probed once and cached; never opens the microphone.
#[tauri::command]
pub async fn voice_available() -> Value {
    let probe = tauri::async_runtime::spawn_blocking(krovi_voice::availability).await;
    match probe {
        Ok(a) => json!({ "dictation": a.dictation, "commands": a.commands, "engine": a.engine }),
        Err(_) => json!({ "dictation": false, "commands": false, "engine": "speech probe failed" }),
    }
}

/// Start listening in `mode`. Resolves once the microphone is open; failures (no microphone,
/// access denied, no engine) reject AND are reported as a `voice-state` `error`.
///
/// `capture_context` (JS: `captureContext`): for `command` mode, read what "this" refers to NOW,
/// before Krovi takes focus (selected text / selected Explorer items / the foreground app's name),
/// as far as the user's Voice settings allow. Start from the pill or the shortcut so the other
/// app still has focus; with the panel in front there is nothing else to read.
#[tauri::command]
pub async fn voice_start(
    app: AppHandle,
    hub: State<'_, VoiceHub>,
    mode: String,
    capture_context: Option<bool>,
) -> Result<(), String> {
    let mode = parse_mode(&mode)?;
    let voice = hub.voice.clone();
    let cache = hub.catalog.clone();
    let last_results = hub.last_results.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let settings = load_settings(&app);
        // The folder / app names go into the recognizer's grammar for this session.
        let catalog = (mode == Mode::Command).then(|| cache.current());
        let opts = start_options(
            &app,
            &settings,
            mode,
            capture_context.unwrap_or(false),
            None,
            catalog,
        );
        let sink: Arc<dyn Sink> = Arc::new(TauriSink::new(app, mode, &settings, last_results));
        voice
            .start_with(mode, sink, opts)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Stop listening. The phrase in progress is finalised and delivered first. Always safe to call.
#[tauri::command]
pub fn voice_stop(hub: State<'_, VoiceHub>) -> Result<(), String> {
    hub.voice.stop();
    Ok(())
}

/// QA hook: run a WAV file through exactly the pipeline the microphone uses (same recognizer,
/// grammars, parser, cleaner and actions). Behaves like a finished session: in `note` mode the note
/// is created and `notes-changed` fires; a command's data action runs; a `voice-result` event is
/// emitted either way. Returns the result.
///
/// `context` supplies a fixed "this" for repeatable tests; otherwise `capture_context: true`
/// reads the real foreground selection now.
#[tauri::command]
pub async fn voice_transcribe_file(
    app: AppHandle,
    hub: State<'_, VoiceHub>,
    path: String,
    mode: String,
    capture_context: Option<bool>,
    context: Option<VoiceContext>,
) -> Result<VoiceResult, String> {
    let mode = parse_mode(&mode)?;
    if !Path::new(&path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
    {
        return Err("voice_transcribe_file only accepts .wav files".into());
    }
    let handle = app.clone();
    let cache = hub.catalog.clone();
    let last_results = hub.last_results.clone();
    let (result, settings) = tauri::async_runtime::spawn_blocking(move || {
        let settings = load_settings(&handle);
        let catalog = (mode == Mode::Command).then(|| cache.current());
        let opts = start_options(
            &handle,
            &settings,
            mode,
            capture_context.unwrap_or(false),
            context,
            catalog,
        );
        krovi_voice::transcribe_wav_with(path, mode, opts)
            .map(|r| (r, settings))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    // Returns exactly what the UI received (the same privacy rules apply to a QA call).
    Ok(TauriSink::new(app, mode, &settings, last_results).deliver(result))
}

/// The closed sets "open ..." and "what's in ..." can name, for a "what can I say?" list.
/// `ready:false` while the first full scan of the Start menu is still running (only the Known
/// Folders are available until then).
#[tauri::command]
pub fn voice_known_targets(hub: State<'_, VoiceHub>) -> Value {
    let (catalog, ready) = match hub.catalog.peek() {
        Some((c, _)) => (c, true),
        None => {
            // Kick off the scan so the next call has it.
            let c = hub.catalog.current();
            (c, false)
        }
    };
    json!({
        "ready": ready,
        "folders": catalog.folders.iter().map(|e| json!({"name": e.name, "path": e.path})).collect::<Vec<_>>(),
        "apps": catalog.apps.iter().map(|e| e.name.clone()).collect::<Vec<_>>(),
    })
}

/// Open one row of the latest voice result: a folder opens in Explorer, a file is revealed in its
/// folder (never executed), an app launches. Only rows the latest `voice-result` showed are
/// accepted; any other path is refused, so this cannot be used to open arbitrary files.
#[tauri::command]
pub async fn voice_open_result(hub: State<'_, VoiceHub>, path: String) -> Result<String, String> {
    let row = hub
        .last_results
        .lock()
        .map_err(|_| "internal lock error".to_string())?
        .iter()
        .find(|r| r.path == path)
        .cloned()
        .ok_or_else(|| "That item is not in the latest voice results.".to_string())?;
    tauri::async_runtime::spawn_blocking(move || open_row(&row))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(all(windows, feature = "native"))]
fn open_row(row: &ResultItem) -> Result<String, String> {
    use krovi_voice::actions::Host;
    let host = krovi_voice::os::WinHost;
    match row.kind {
        ResultKind::Folder => host
            .open_folder(&row.path)
            .map(|_| format!("Opened {}.", row.name)),
        ResultKind::File => krovi_voice::os::reveal(&row.path)
            .map(|_| format!("Showing {} in its folder.", row.name)),
        ResultKind::App => host
            .launch_app(&row.path)
            .map(|_| format!("Launched {}.", row.name)),
    }
}

#[cfg(not(all(windows, feature = "native")))]
fn open_row(_row: &ResultItem) -> Result<String, String> {
    Err("Opening items needs the native voice build.".into())
}
