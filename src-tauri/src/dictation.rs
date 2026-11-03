//! Global dictation (v0.3, D-020): hold (or toggle) a shortcut, speak, and the text is
//! typed into whatever app has focus. Everything is local:
//!
//! * The microphone opens only while the shortcut is held / toggled on (or during the mic
//!   test the user starts). Nothing listens otherwise.
//! * Speech is recognised by whisper.cpp on this PC; the model is a file the user chose to
//!   download (`voice_model_download`, SHA-256 verified). No audio leaves the device.
//! * The model is loaded on first use and released after [`IDLE_UNLOAD`] without use.
//! * Text is typed with `SendInput` Unicode keystrokes: the clipboard is never touched.
//! * Dictated text is never interpreted as a command.
//!
//! Talking to Agent Mode is a *separate* shortcut (`agent_shortcut`, off by default, `Ctrl+Alt+A`).
//! It records through exactly the same Recorder / VAD / Whisper path, but the words are never
//! typed: they go to Agent Mode's planner (`crate::agent::handle_voice`), which plans them, runs
//! the Safe steps as a typed command would, and opens the panel on the Agent tool. The only
//! difference between the two is the [`SessionMode`] the recording carries.
//!
//! The pipeline itself (capture, VAD, Whisper, clean-up, typing) lives in `krovi-voice`
//! and is unit-tested there; this file is wiring: settings, the shortcuts, threads, events.
//!
//! Events: `dictation-state` `{state, level?, mode}` (`idle` | `listening` | `loading-model` |
//! `transcribing`), `dictation-result` `{ok, text?, chars?, nothingHeard?, error?, mode}`
//! (`mode` is `"dictate"` or `"agent"`), `voice-model-progress` `{id, downloaded, total}`,
//! `voice-model-done` `{id, ok, error?}`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use krovi_net::whisper_models::{self, ModelStatus, WhisperModel};
use krovi_voice::capture::{list_input_devices, InputDevice, Recorder};
use krovi_voice::dictate::{self, Outcome};
use krovi_voice::dictation::{validate_replacements, DictationOptions};
use krovi_voice::speech::SttOptions;
use krovi_voice::stt::Engine;

use crate::commands::{setting_get, setting_set, CmdResult};
use crate::state::AppState;

const KEY: &str = "dictation.v1";
/// Release the speech model's memory after this long without dictation.
const IDLE_UNLOAD: Duration = Duration::from_secs(5 * 60);
/// The mic test stops itself after this long.
const MIC_TEST_MAX: Duration = Duration::from_secs(20);
const MAX_VOCABULARY: usize = 100;
const MAX_TERM: usize = 60;
/// Language codes offered in Settings (Whisper supports more; these are the ones Krovi lists).
const LANGUAGES: &[&str] = &[
    "auto", "en", "hi", "es", "fr", "de", "pt", "it", "nl", "ja", "zh", "ko", "ru", "ar", "bn",
    "ta", "te", "mr", "gu", "ur",
];

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum TalkMode {
    /// Speak while the shortcut is held.
    Hold,
    /// Press once to start, again to stop.
    Toggle,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct DictationSettings {
    pub enabled: bool,
    pub shortcut: String,
    pub mode: TalkMode,
    /// Catalog id from `krovi_net::whisper_models` ("tiny", "base", "small", "base.en").
    pub model: String,
    /// "auto" or an ISO 639-1 code.
    pub language: String,
    /// Microphone endpoint id; `None` = the Windows default.
    pub device: Option<String>,
    /// Names / product terms that bias recognition.
    pub vocabulary: Vec<String>,
    pub options: DictationOptions,
    /// Talk to Agent Mode: a second shortcut whose words go to the planner, never into an app.
    pub agent_enabled: bool,
    pub agent_shortcut: String,
}

impl Default for DictationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            shortcut: "Ctrl+Alt+D".into(),
            mode: TalkMode::Hold,
            model: "base".into(),
            language: "auto".into(),
            device: None,
            vocabulary: Vec::new(),
            options: DictationOptions::default(),
            agent_enabled: false,
            agent_shortcut: "Ctrl+Alt+A".into(),
        }
    }
}

/// What a recording is for. Both shortcuts record the same way; only the destination differs
/// (D-020: dictated text is typed and never a command, a spoken command is never typed).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SessionMode {
    /// Type the text into the focused app.
    Dictate,
    /// Hand the text to Agent Mode's planner.
    Agent,
}

impl SessionMode {
    /// The value of the `mode` field on `dictation-state` / `dictation-result`.
    fn as_str(self) -> &'static str {
        match self {
            SessionMode::Dictate => "dictate",
            SessionMode::Agent => "agent",
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub size_bytes: u64,
    pub multilingual: bool,
    pub license: String,
    pub status: ModelStatus,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DictationStatus {
    pub settings: DictationSettings,
    pub recording: bool,
    pub busy: bool,
    pub model_loaded: bool,
    pub downloading: Option<String>,
    pub models: Vec<ModelInfo>,
    pub models_dir: Option<String>,
}

type LoadedEngine = (Arc<Engine>, String, Instant);
/// The open recording and what it is for.
type ActiveRec = (Recorder, SessionMode);

pub struct Dictation {
    settings: Mutex<DictationSettings>,
    models_dir: Option<PathBuf>,
    rec: Mutex<Option<ActiveRec>>,
    mic_test: Mutex<Option<Recorder>>,
    engine: Mutex<Option<LoadedEngine>>,
    cancel: Mutex<Arc<AtomicBool>>,
    busy: AtomicBool,
    registered: Mutex<Option<Shortcut>>,
    agent_registered: Mutex<Option<Shortcut>>,
    download: Mutex<Option<(String, Arc<AtomicBool>)>>,
}

fn lk<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn emit_state(app: &AppHandle, state: &str, mode: SessionMode) {
    let _ = app.emit(
        "dictation-state",
        json!({ "state": state, "mode": mode.as_str() }),
    );
}

/// `dictation-result` with the session's `mode` added to `payload` (a JSON object).
fn emit_result(app: &AppHandle, mode: SessionMode, mut payload: serde_json::Value) {
    payload["mode"] = json!(mode.as_str());
    let _ = app.emit("dictation-result", payload);
}

fn emit_error(app: &AppHandle, mode: SessionMode, msg: impl Into<String>) {
    emit_result(app, mode, json!({ "ok": false, "error": msg.into() }));
}

fn model_by_id(id: &str) -> Result<&'static WhisperModel, String> {
    whisper_models::find_model(id).ok_or_else(|| format!("Unknown speech model {id:?}."))
}

fn models_info(dir: Option<&PathBuf>) -> Vec<ModelInfo> {
    whisper_models::catalog()
        .iter()
        .map(|m| ModelInfo {
            id: m.id.into(),
            display_name: m.display_name.into(),
            size_bytes: m.size_bytes,
            multilingual: m.multilingual,
            license: m.license.into(),
            status: dir
                .map(|d| whisper_models::model_status(m, d))
                .unwrap_or(ModelStatus::Missing),
        })
        .collect()
}

fn installed_path(d: &Dictation, id: &str) -> Result<PathBuf, String> {
    let dir = d
        .models_dir
        .as_ref()
        .ok_or("Krovi has no data folder for speech models.")?;
    match whisper_models::model_status(model_by_id(id)?, dir) {
        ModelStatus::Installed { path, .. } => Ok(path),
        _ => Err("Download a speech model first (Settings › Voice).".into()),
    }
}

// ---------------------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------------------

fn start(app: &AppHandle, mode: SessionMode) {
    let d = app.state::<Dictation>();
    if lk(&d.rec).is_some() {
        return; // key repeat while held, or the other shortcut is recording
    }
    if d.busy.load(Ordering::SeqCst) {
        emit_error(
            app,
            mode,
            match mode {
                SessionMode::Dictate => "Still working on the last dictation.",
                SessionMode::Agent => "Still working on the last voice command.",
            },
        );
        return;
    }
    let s = lk(&d.settings).clone();
    if let Err(e) = installed_path(&d, &s.model) {
        emit_error(app, mode, e);
        return;
    }
    *lk(&d.cancel) = Arc::new(AtomicBool::new(false));
    let app2 = app.clone();
    // Opening the device can take a moment; never block the shortcut thread.
    thread::spawn(move || {
        let d = app2.state::<Dictation>();
        match Recorder::start(s.device.clone()) {
            Ok(r) => {
                *lk(&d.rec) = Some((r, mode));
                emit_state(&app2, "listening", mode);
                level_loop(app2.clone());
            }
            Err(e) => emit_error(&app2, mode, e),
        }
    });
}

/// Live input level for the listening indicator (~8 Hz) while recording.
fn level_loop(app: AppHandle) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(125));
        let level = {
            let d = app.state::<Dictation>();
            let rec = lk(&d.rec);
            match rec.as_ref() {
                Some((r, mode)) if !r.finished() => Some((r.level(), *mode)),
                Some(_) => None, // device lost or the length cap hit
                None => return,
            }
        };
        match level {
            Some((level, mode)) => {
                let _ = app.emit(
                    "dictation-state",
                    json!({ "state": "listening", "level": level, "mode": mode.as_str() }),
                );
            }
            None => {
                finish(&app, None); // keep what was recorded
                return;
            }
        }
    });
}

/// Stop recording and process what was said. `only` restricts this to a recording made for that
/// mode: releasing the Agent shortcut must not end a dictation that is still running, and the
/// other way round.
fn finish(app: &AppHandle, only: Option<SessionMode>) {
    let d = app.state::<Dictation>();
    let taken = {
        let mut slot = lk(&d.rec);
        if slot
            .as_ref()
            .is_some_and(|(_, mode)| only.is_none_or(|o| o == *mode))
        {
            slot.take()
        } else {
            None
        }
    };
    let Some((rec, mode)) = taken else {
        return;
    };
    d.busy.store(true, Ordering::SeqCst);
    let app2 = app.clone();
    thread::spawn(move || {
        let d = app2.state::<Dictation>();
        let result = run_pipeline(&app2, &d, rec, mode);
        d.busy.store(false, Ordering::SeqCst);
        emit_state(&app2, "idle", mode);
        match result {
            // A recognised voice command went to Agent Mode and the panel shows it: nothing to announce.
            Ok(Outcome::Typed { .. }) if mode == SessionMode::Agent => {}
            Ok(Outcome::Typed {
                text,
                chars,
                language,
            }) => emit_result(
                &app2,
                mode,
                json!({ "ok": true, "text": text, "chars": chars, "language": language }),
            ),
            Ok(Outcome::NothingHeard) => {
                emit_result(&app2, mode, json!({ "ok": true, "nothingHeard": true }))
            }
            Ok(Outcome::NotTyped { text, reason }) => emit_result(
                &app2,
                mode,
                json!({ "ok": false, "text": text, "error": reason }),
            ),
            Err(e) if e == "Cancelled" => {}
            Err(e) => emit_error(&app2, mode, e),
        }
        schedule_unload(app2.clone());
    });
}

fn run_pipeline(
    app: &AppHandle,
    d: &Dictation,
    rec: Recorder,
    mode: SessionMode,
) -> Result<Outcome, String> {
    let audio = rec.stop()?;
    let s = lk(&d.settings).clone();
    let cancel = lk(&d.cancel).clone();
    let engine = engine_for(app, d, &s.model, mode)?;
    emit_state(app, "transcribing", mode);
    // A spoken command is not prose: no writing style, replacements or added punctuation
    // (fillers still go). Dictation keeps the user's own options.
    let opts = match mode {
        SessionMode::Dictate => s.options.clone(),
        SessionMode::Agent => DictationOptions {
            replacements: Vec::new(),
            remove_fillers: s.options.remove_fillers,
            auto_punctuate: false,
            ..DictationOptions::default()
        },
    };
    let stt = SttOptions {
        language: (s.language != "auto").then(|| s.language.clone()),
        prompt: dictate::prompt_for(&s.vocabulary, &opts),
        threads: None,
    };
    // `type_it` is false for the Agent: the words are returned, never typed.
    let outcome = dictate::finish(
        &audio,
        &*engine,
        &stt,
        &opts,
        cancel.clone(),
        mode == SessionMode::Dictate,
    )?;
    if let (SessionMode::Agent, Outcome::Typed { text, .. }) = (mode, &outcome) {
        if cancel.load(Ordering::SeqCst) {
            return Err("Cancelled".into()); // cancelled on the pill: nothing may run
        }
        crate::agent::handle_voice(app, text.clone())?;
    }
    Ok(outcome)
}

/// The loaded engine for `model`, loading it on first use (or after a model change).
fn engine_for(
    app: &AppHandle,
    d: &Dictation,
    model: &str,
    mode: SessionMode,
) -> Result<Arc<Engine>, String> {
    {
        let mut slot = lk(&d.engine);
        if let Some((e, id, used)) = slot.as_mut() {
            if id == model {
                *used = Instant::now();
                return Ok(e.clone());
            }
        }
        *slot = None; // a different model: release the old one first
    }
    emit_state(app, "loading-model", mode);
    let path = installed_path(d, model)?;
    let engine = Arc::new(Engine::load(&path)?);
    *lk(&d.engine) = Some((engine.clone(), model.to_string(), Instant::now()));
    Ok(engine)
}

fn schedule_unload(app: AppHandle) {
    thread::spawn(move || {
        thread::sleep(IDLE_UNLOAD + Duration::from_secs(1));
        let d = app.state::<Dictation>();
        if d.busy.load(Ordering::SeqCst) || lk(&d.rec).is_some() {
            return;
        }
        let mut slot = lk(&d.engine);
        if slot
            .as_ref()
            .is_some_and(|(_, _, used)| used.elapsed() >= IDLE_UNLOAD)
        {
            *slot = None; // the model's memory goes back to Windows
        }
    });
}

fn cancel_session(app: &AppHandle) {
    let d = app.state::<Dictation>();
    lk(&d.cancel).store(true, Ordering::SeqCst);
    let mode = lk(&d.rec)
        .take()
        .map(|(r, mode)| {
            r.cancel();
            mode
        })
        .unwrap_or(SessionMode::Dictate);
    emit_state(app, "idle", mode);
}

// ---------------------------------------------------------------------------------------
// Shortcuts
// ---------------------------------------------------------------------------------------

/// Hold / toggle behaviour (the one setting both shortcuts share) for the shortcut that
/// belongs to `mode`.
fn on_shortcut(app: &AppHandle, state: ShortcutState, mode: SessionMode) {
    let (talk, recording) = {
        let d = app.state::<Dictation>();
        let talk = lk(&d.settings).mode;
        let recording = lk(&d.rec).as_ref().is_some_and(|(_, m)| *m == mode);
        (talk, recording)
    };
    match (talk, state) {
        (TalkMode::Hold, ShortcutState::Pressed) => start(app, mode),
        (TalkMode::Hold, ShortcutState::Released) => finish(app, Some(mode)),
        (TalkMode::Toggle, ShortcutState::Pressed) if recording => finish(app, Some(mode)),
        (TalkMode::Toggle, ShortcutState::Pressed) => start(app, mode),
        (TalkMode::Toggle, ShortcutState::Released) => {}
    }
}

/// Register the enabled shortcuts (dictation, Agent), dropping the previous ones first.
fn register(app: &AppHandle, s: &DictationSettings) -> Result<(), String> {
    let d = app.state::<Dictation>();
    for slot in [&d.registered, &d.agent_registered] {
        if let Some(old) = lk(slot).take() {
            let _ = app.global_shortcut().unregister(old);
        }
    }
    if s.enabled {
        let sc = crate::shell::parse_shortcut(&s.shortcut)?;
        app.global_shortcut()
            .on_shortcut(sc, move |app, _sc, ev| {
                on_shortcut(app, ev.state(), SessionMode::Dictate)
            })
            .map_err(|e| {
                format!(
                    "Could not use {} as the dictation shortcut: {e}",
                    s.shortcut
                )
            })?;
        *lk(&d.registered) = Some(sc);
    }
    if s.agent_enabled {
        let sc = crate::shell::parse_shortcut(&s.agent_shortcut)?;
        app.global_shortcut()
            .on_shortcut(sc, move |app, _sc, ev| {
                on_shortcut(app, ev.state(), SessionMode::Agent)
            })
            .map_err(|e| {
                format!(
                    "Could not use {} as the Agent shortcut: {e}",
                    s.agent_shortcut
                )
            })?;
        *lk(&d.agent_registered) = Some(sc);
    }
    Ok(())
}

fn validate(s: &DictationSettings) -> Result<(), String> {
    model_by_id(&s.model)?;
    if !LANGUAGES.contains(&s.language.as_str()) {
        return Err(format!("Unsupported language {:?}.", s.language));
    }
    let bad_term = |t: &String| {
        t.trim().is_empty() || t.chars().count() > MAX_TERM || t.chars().any(char::is_control)
    };
    if s.vocabulary.len() > MAX_VOCABULARY || s.vocabulary.iter().any(bad_term) {
        return Err(format!(
            "Vocabulary: up to {MAX_VOCABULARY} terms of 1-{MAX_TERM} characters."
        ));
    }
    validate_replacements(&s.options.replacements)?;
    let dictate_sc = crate::shell::parse_shortcut(&s.shortcut)?;
    let agent_sc = crate::shell::parse_shortcut(&s.agent_shortcut)?;
    if s.enabled && s.agent_enabled && dictate_sc == agent_sc {
        return Err("Dictation and the Agent shortcut need different keys.".into());
    }
    Ok(())
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let models_dir = app.path().app_data_dir().ok().map(|d| d.join("models"));
    let settings: DictationSettings =
        setting_get(&app.state::<AppState>(), KEY).unwrap_or_default();
    app.manage(Dictation {
        settings: Mutex::new(settings.clone()),
        models_dir,
        rec: Mutex::new(None),
        mic_test: Mutex::new(None),
        engine: Mutex::new(None),
        cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
        busy: AtomicBool::new(false),
        registered: Mutex::new(None),
        agent_registered: Mutex::new(None),
        download: Mutex::new(None),
    });
    // A taken shortcut must not stop Krovi from starting; Settings shows the error.
    if let Err(e) = register(app.handle(), &settings) {
        eprintln!("[krovi] dictation shortcut: {e}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------------------

#[tauri::command]
pub fn dictation_status(d: State<'_, Dictation>) -> DictationStatus {
    DictationStatus {
        settings: lk(&d.settings).clone(),
        recording: lk(&d.rec).is_some(),
        busy: d.busy.load(Ordering::SeqCst),
        model_loaded: lk(&d.engine).is_some(),
        downloading: lk(&d.download).as_ref().map(|(id, _)| id.clone()),
        models: models_info(d.models_dir.as_ref()),
        models_dir: d.models_dir.as_ref().map(|p| p.display().to_string()),
    }
}

#[tauri::command]
pub fn dictation_set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: DictationSettings,
) -> CmdResult<DictationSettings> {
    validate(&settings)?;
    if let Err(e) = register(&app, &settings) {
        // Two shortcuts can half-apply: put the saved ones back so a refused change leaves
        // the old keys working.
        let saved = lk(&app.state::<Dictation>().settings).clone();
        let _ = register(&app, &saved);
        return Err(e);
    }
    setting_set(&state, KEY, &settings)?;
    *lk(&app.state::<Dictation>().settings) = settings.clone();
    Ok(settings)
}

/// Start / stop from the UI (the pill's mic button), same as the shortcut in toggle mode.
#[tauri::command]
pub fn dictation_toggle(app: AppHandle) {
    let recording = lk(&app.state::<Dictation>().rec).is_some();
    if recording {
        finish(&app, None);
    } else {
        start(&app, SessionMode::Dictate);
    }
}

#[tauri::command]
pub fn dictation_cancel(app: AppHandle) {
    cancel_session(&app);
}

#[tauri::command]
pub async fn voice_input_devices() -> CmdResult<Vec<InputDevice>> {
    tauri::async_runtime::spawn_blocking(list_input_devices)
        .await
        .map_err(|e| e.to_string())?
}

/// Open the chosen microphone for a level test (stops by itself after 20 s).
#[tauri::command]
pub async fn mic_test_start(app: AppHandle, device: Option<String>) -> CmdResult<()> {
    let r = tauri::async_runtime::spawn_blocking(move || Recorder::start(device))
        .await
        .map_err(|e| e.to_string())??;
    let old = lk(&app.state::<Dictation>().mic_test).replace(r);
    if let Some(old) = old {
        old.cancel();
    }
    let app2 = app.clone();
    thread::spawn(move || {
        thread::sleep(MIC_TEST_MAX);
        let r = lk(&app2.state::<Dictation>().mic_test).take();
        if let Some(r) = r {
            r.cancel();
        }
    });
    Ok(())
}

/// Current mic-test level (RMS 0..1), or `None` when no test is running.
#[tauri::command]
pub fn mic_test_level(d: State<'_, Dictation>) -> Option<f32> {
    lk(&d.mic_test).as_ref().map(Recorder::level)
}

#[tauri::command]
pub fn mic_test_stop(d: State<'_, Dictation>) {
    let r = lk(&d.mic_test).take();
    if let Some(r) = r {
        r.cancel(); // the test audio is discarded, never stored
    }
}

/// Download a speech model the user picked (the only network use of the voice feature).
#[tauri::command]
pub fn voice_model_download(app: AppHandle, id: String) -> CmdResult<()> {
    let model = model_by_id(&id)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let dir = {
        let d = app.state::<Dictation>();
        let dir = d
            .models_dir
            .clone()
            .ok_or("Krovi has no data folder for speech models.")?;
        let mut slot = lk(&d.download);
        if slot.is_some() {
            return Err("A model is already downloading.".into());
        }
        *slot = Some((id, cancel.clone()));
        dir
    };
    let app2 = app.clone();
    thread::spawn(move || {
        let _ = std::fs::create_dir_all(&dir);
        let mut last: Option<Instant> = None;
        let res = whisper_models::download_model(
            model,
            &dir,
            |done, total| {
                if last.is_none_or(|t| t.elapsed() >= Duration::from_millis(250)) || done == total {
                    last = Some(Instant::now());
                    let _ = app2.emit(
                        "voice-model-progress",
                        json!({ "id": model.id, "downloaded": done, "total": total }),
                    );
                }
            },
            &cancel,
        );
        *lk(&app2.state::<Dictation>().download) = None;
        let _ = app2.emit(
            "voice-model-done",
            match res {
                Ok(_) => json!({ "id": model.id, "ok": true }),
                Err(e) => json!({ "id": model.id, "ok": false, "error": e.to_string() }),
            },
        );
    });
    Ok(())
}

#[tauri::command]
pub fn voice_model_cancel_download(d: State<'_, Dictation>) {
    if let Some((_, c)) = lk(&d.download).as_ref() {
        c.store(true, Ordering::SeqCst);
    }
}

#[tauri::command]
pub fn voice_model_delete(d: State<'_, Dictation>, id: String) -> CmdResult<Vec<ModelInfo>> {
    let model = model_by_id(&id)?;
    {
        let mut slot = lk(&d.engine);
        if slot.as_ref().is_some_and(|(_, m, _)| m == &id) {
            *slot = None; // release the file before deleting it
        }
    }
    let dir = d.models_dir.as_ref().ok_or("no data folder")?;
    whisper_models::delete_model(model, dir).map_err(|e| e.to_string())?;
    Ok(models_info(d.models_dir.as_ref()))
}
