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
//! The pipeline itself (capture, VAD, Whisper, clean-up, typing) lives in `krovi-voice`
//! and is unit-tested there; this file is wiring: settings, the shortcut, threads, events.
//!
//! Events: `dictation-state` `{state, level?}` (`idle` | `listening` | `loading-model` |
//! `transcribing`), `dictation-result` `{ok, text?, chars?, nothingHeard?, error?}`,
//! `voice-model-progress` `{id, downloaded, total}`, `voice-model-done` `{id, ok, error?}`.

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

pub struct Dictation {
    settings: Mutex<DictationSettings>,
    models_dir: Option<PathBuf>,
    rec: Mutex<Option<Recorder>>,
    mic_test: Mutex<Option<Recorder>>,
    engine: Mutex<Option<LoadedEngine>>,
    cancel: Mutex<Arc<AtomicBool>>,
    busy: AtomicBool,
    registered: Mutex<Option<Shortcut>>,
    download: Mutex<Option<(String, Arc<AtomicBool>)>>,
}

fn lk<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn emit_state(app: &AppHandle, state: &str) {
    let _ = app.emit("dictation-state", json!({ "state": state }));
}

fn emit_error(app: &AppHandle, msg: impl Into<String>) {
    let _ = app.emit(
        "dictation-result",
        json!({ "ok": false, "error": msg.into() }),
    );
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

fn start(app: &AppHandle) {
    let d = app.state::<Dictation>();
    if lk(&d.rec).is_some() {
        return; // key repeat while held
    }
    if d.busy.load(Ordering::SeqCst) {
        emit_error(app, "Still working on the last dictation.");
        return;
    }
    let s = lk(&d.settings).clone();
    if let Err(e) = installed_path(&d, &s.model) {
        emit_error(app, e);
        return;
    }
    *lk(&d.cancel) = Arc::new(AtomicBool::new(false));
    let app2 = app.clone();
    // Opening the device can take a moment; never block the shortcut thread.
    thread::spawn(move || {
        let d = app2.state::<Dictation>();
        match Recorder::start(s.device.clone()) {
            Ok(r) => {
                *lk(&d.rec) = Some(r);
                emit_state(&app2, "listening");
                level_loop(app2.clone());
            }
            Err(e) => emit_error(&app2, e),
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
                Some(r) if !r.finished() => Some(r.level()),
                Some(_) => None, // device lost or the length cap hit
                None => return,
            }
        };
        match level {
            Some(level) => {
                let _ = app.emit(
                    "dictation-state",
                    json!({ "state": "listening", "level": level }),
                );
            }
            None => {
                finish(&app); // keep what was recorded
                return;
            }
        }
    });
}

fn finish(app: &AppHandle) {
    let d = app.state::<Dictation>();
    let Some(rec) = lk(&d.rec).take() else {
        return;
    };
    d.busy.store(true, Ordering::SeqCst);
    let app2 = app.clone();
    thread::spawn(move || {
        let d = app2.state::<Dictation>();
        let result = run_pipeline(&app2, &d, rec);
        d.busy.store(false, Ordering::SeqCst);
        emit_state(&app2, "idle");
        match result {
            Ok(Outcome::Typed {
                text,
                chars,
                language,
            }) => {
                let _ = app2.emit(
                    "dictation-result",
                    json!({ "ok": true, "text": text, "chars": chars, "language": language }),
                );
            }
            Ok(Outcome::NothingHeard) => {
                let _ = app2.emit(
                    "dictation-result",
                    json!({ "ok": true, "nothingHeard": true }),
                );
            }
            Ok(Outcome::NotTyped { text, reason }) => {
                let _ = app2.emit(
                    "dictation-result",
                    json!({ "ok": false, "text": text, "error": reason }),
                );
            }
            Err(e) if e == "Cancelled" => {}
            Err(e) => emit_error(&app2, e),
        }
        schedule_unload(app2.clone());
    });
}

fn run_pipeline(app: &AppHandle, d: &Dictation, rec: Recorder) -> Result<Outcome, String> {
    let audio = rec.stop()?;
    let s = lk(&d.settings).clone();
    let cancel = lk(&d.cancel).clone();
    let engine = engine_for(app, d, &s.model)?;
    emit_state(app, "transcribing");
    let stt = SttOptions {
        language: (s.language != "auto").then(|| s.language.clone()),
        prompt: dictate::prompt_for(&s.vocabulary, &s.options),
        threads: None,
    };
    dictate::finish(&audio, &*engine, &stt, &s.options, cancel, true)
}

/// The loaded engine for `model`, loading it on first use (or after a model change).
fn engine_for(app: &AppHandle, d: &Dictation, model: &str) -> Result<Arc<Engine>, String> {
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
    emit_state(app, "loading-model");
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
    if let Some(r) = lk(&d.rec).take() {
        r.cancel();
    }
    emit_state(app, "idle");
}

// ---------------------------------------------------------------------------------------
// Shortcut
// ---------------------------------------------------------------------------------------

fn on_shortcut(app: &AppHandle, state: ShortcutState) {
    let (mode, recording) = {
        let d = app.state::<Dictation>();
        let mode = lk(&d.settings).mode;
        let recording = lk(&d.rec).is_some();
        (mode, recording)
    };
    match (mode, state) {
        (TalkMode::Hold, ShortcutState::Pressed) => start(app),
        (TalkMode::Hold, ShortcutState::Released) => finish(app),
        (TalkMode::Toggle, ShortcutState::Pressed) if recording => finish(app),
        (TalkMode::Toggle, ShortcutState::Pressed) => start(app),
        (TalkMode::Toggle, ShortcutState::Released) => {}
    }
}

fn register(app: &AppHandle, s: &DictationSettings) -> Result<(), String> {
    let d = app.state::<Dictation>();
    if let Some(old) = lk(&d.registered).take() {
        let _ = app.global_shortcut().unregister(old);
    }
    if !s.enabled {
        return Ok(());
    }
    let sc = crate::shell::parse_shortcut(&s.shortcut)?;
    app.global_shortcut()
        .on_shortcut(sc, move |app, _sc, ev| on_shortcut(app, ev.state()))
        .map_err(|e| format!("Could not use {} as the dictation shortcut: {e}", s.shortcut))?;
    *lk(&d.registered) = Some(sc);
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
    crate::shell::parse_shortcut(&s.shortcut).map(|_| ())
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
    register(&app, &settings)?;
    setting_set(&state, KEY, &settings)?;
    *lk(&app.state::<Dictation>().settings) = settings.clone();
    Ok(settings)
}

/// Start / stop from the UI (the pill's mic button), same as the shortcut in toggle mode.
#[tauri::command]
pub fn dictation_toggle(app: AppHandle) {
    let recording = lk(&app.state::<Dictation>().rec).is_some();
    if recording {
        finish(&app);
    } else {
        start(&app);
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
