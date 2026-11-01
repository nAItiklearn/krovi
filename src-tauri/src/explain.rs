//! Explain tool (owner: explain). See docs/CONTRACTS.md section 10.
//!
//! A thin Tauri layer over the `krovi-explain` crate: summary (on-device), read-aloud (Windows
//! voices by default, optional ElevenLabs with the user's own key). All behaviour lives in the
//! crate and is unit-tested there; this file only holds state, persists the non-secret settings
//! and turns crate events into Tauri events.
//!
//! Commands: explain_summarize, explain_engines, explain_get_settings, explain_set_settings,
//! explain_set_key, explain_eleven_voices, explain_test_voice, explain_speak,
//! explain_stop_speaking, explain_open_keys_page.
//! Event: `explain-speak` (a `SpeakEvent`: speaking | notice | done | failed | stopped).
//!
//! Network: none, except ElevenLabs when (and only when) the user saved a key and asks for it:
//! `explain_set_key` / `explain_eleven_voices` / `explain_test_voice` / `explain_speak` with the
//! ElevenLabs engine. The key is read from Windows Credential Manager at the moment of the call
//! and is never returned to the webview, logged or written to disk.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use krovi_explain::eleven::{self, ErrorKind};
use krovi_explain::secrets;
use krovi_explain::settings;
use krovi_explain::speak::{self, Engine, SpeakEvent, SpeakInfo, SpeakRequest, SpeakSink, Speaker};
use krovi_explain::win;
use krovi_explain::{summarize, Settings, Summary, SummaryLength, VoiceEngine};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};

/// Most text one ElevenLabs read-aloud request set will accept (a guard on credit spend).
const ELEVEN_MAX_CHARS: usize = 30_000;

pub struct ExplainState {
    settings: Mutex<Settings>,
    path: Mutex<Option<PathBuf>>,
    speaker: Speaker,
}

struct TauriSink(tauri::AppHandle);

impl SpeakSink for TauriSink {
    fn event(&self, ev: SpeakEvent) {
        let _ = self.0.emit("explain-speak", ev);
    }
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Settings are read from disk here; no sockets, no keys, no audio are touched at startup.
    let path = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("explain.json"));
    let loaded = path.as_deref().map(settings::load).unwrap_or_default();
    app.manage(ExplainState {
        settings: Mutex::new(loaded),
        path: Mutex::new(path),
        speaker: Speaker::new(),
    });

    // Stop talking when the app quits.
    let closer = tauri::plugin::Builder::<tauri::Wry, ()>::new("krovi-explain")
        .on_event(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<ExplainState>() {
                    state.speaker.stop();
                }
            }
        })
        .build();
    app.handle().plugin(closer)?;
    Ok(())
}

fn lock_err<T>(_: std::sync::PoisonError<T>) -> String {
    "internal lock error".to_string()
}

fn current(state: &ExplainState) -> Result<Settings, String> {
    Ok(state.settings.lock().map_err(lock_err)?.clone())
}

fn persist(state: &ExplainState, s: &Settings) -> Result<(), String> {
    *state.settings.lock().map_err(lock_err)? = s.clone();
    let path = state.path.lock().map_err(lock_err)?.clone();
    match path {
        Some(p) => settings::save(&p, s),
        None => Ok(()),
    }
}

fn eleven_key() -> Result<Option<String>, String> {
    secrets::read(secrets::ELEVEN_TARGET)
}

// ---------------------------------------------------------------------------------------
// Summary (on-device)
// ---------------------------------------------------------------------------------------

#[tauri::command]
pub async fn explain_summarize(text: String, length: SummaryLength) -> Result<Summary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        summarize(&text, length).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------------------
// Engines and settings
// ---------------------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelDto {
    id: &'static str,
    label: &'static str,
    credits_per_char: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowsEngine {
    available: bool,
    voices: Vec<win::WinVoice>,
    default_voice_id: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ElevenEngine {
    has_key: bool,
    key_hint: Option<String>,
    models: Vec<ModelDto>,
    keys_url: &'static str,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Engines {
    windows: WindowsEngine,
    elevenlabs: ElevenEngine,
    settings: Settings,
}

/// What the Settings panel needs. Local only: lists installed Windows voices and checks whether a
/// key is saved. No network request is made.
#[tauri::command]
pub async fn explain_engines(state: State<'_, ExplainState>) -> Result<Engines, String> {
    let settings = current(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let (voices, default_voice_id, werr) = match win::voices() {
            Ok(v) => (v, win::default_voice_id(), None),
            Err(e) => (Vec::new(), None, Some(e)),
        };
        let (has_key, key_hint, eerr) = match eleven_key() {
            Ok(Some(k)) => (true, secrets::key_hint(&k), None),
            Ok(None) => (false, None, None),
            Err(e) => (false, None, Some(e)),
        };
        Engines {
            windows: WindowsEngine {
                available: !voices.is_empty(),
                voices,
                default_voice_id,
                error: werr,
            },
            elevenlabs: ElevenEngine {
                has_key,
                key_hint,
                models: eleven::MODELS
                    .iter()
                    .map(|m| ModelDto {
                        id: m.id,
                        label: m.label,
                        credits_per_char: m.credits_per_char,
                    })
                    .collect(),
                keys_url: eleven::KEYS_PAGE_URL,
                error: eerr,
            },
            settings,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn explain_get_settings(state: State<'_, ExplainState>) -> Result<Settings, String> {
    current(&state)
}

#[tauri::command]
pub fn explain_set_settings(
    state: State<'_, ExplainState>,
    settings: Settings,
) -> Result<Settings, String> {
    let mut s = settings.sanitized();
    // ElevenLabs cannot be the engine without a saved key and a chosen voice.
    if s.voice_engine == VoiceEngine::Elevenlabs {
        let has_key = matches!(eleven_key(), Ok(Some(_)));
        if !has_key || s.eleven_voice_id.is_none() {
            s.voice_engine = VoiceEngine::Windows;
        }
    }
    persist(&state, &s)?;
    Ok(s)
}

// ---------------------------------------------------------------------------------------
// ElevenLabs key + voices (the optional, user-keyed part)
// ---------------------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyResult {
    /// The key is now saved.
    saved: bool,
    has_key: bool,
    key_hint: Option<String>,
    voices: Vec<eleven::Voice>,
    /// One plain sentence for the user: what happened and what to do next.
    message: String,
    /// "ok" | "warning" | "error"
    tone: &'static str,
    settings: Settings,
}

fn key_result(
    saved: bool,
    tone: &'static str,
    message: String,
    voices: Vec<eleven::Voice>,
    settings: Settings,
) -> KeyResult {
    let key = eleven_key().ok().flatten();
    KeyResult {
        saved,
        has_key: key.is_some(),
        key_hint: key.as_deref().and_then(secrets::key_hint),
        voices,
        message,
        tone,
        settings,
    }
}

/// Save (`key = Some`) or remove (`key = None`) the user's ElevenLabs key. Saving checks the key
/// against ElevenLabs first by loading the account's voices (one request, the key header only).
#[tauri::command]
pub async fn explain_set_key(
    state: State<'_, ExplainState>,
    provider: String,
    key: Option<String>,
) -> Result<KeyResult, String> {
    if provider != "elevenlabs" {
        return Err("Unknown provider.".into());
    }
    let mut settings = current(&state)?;

    let Some(raw) = key else {
        // Remove key: delete from Credential Manager and fall back to the Windows voice.
        secrets::delete(secrets::ELEVEN_TARGET)?;
        settings.voice_engine = VoiceEngine::Windows;
        settings.eleven_voice_id = None;
        settings.eleven_voice_name = None;
        settings.eleven_consented = false;
        persist(&state, &settings)?;
        return Ok(key_result(
            false,
            "ok",
            "Your ElevenLabs key was removed from this PC. Reading aloud uses the Windows voice."
                .into(),
            Vec::new(),
            settings,
        ));
    };

    let key = match eleven::clean_key(&raw) {
        Ok(k) => k,
        Err(m) => return Ok(key_result(false, "error", m, Vec::new(), settings)),
    };
    let check_key = key.clone();
    let checked = tauri::async_runtime::spawn_blocking(move || eleven::list_voices(&check_key))
        .await
        .map_err(|e| e.to_string())?;

    let (message, tone, voices) = match checked {
        Ok(v) if v.is_empty() => ("Key saved, but your ElevenLabs account has no voices yet. Add a voice in ElevenLabs, then press Refresh.".to_string(), "warning", v),
        Ok(v) => (format!("Key saved. {} voices found on your account.", v.len()), "ok", v),
        Err(e) if e.kind == ErrorKind::InvalidKey => {
            // Do not keep a key ElevenLabs just rejected.
            return Ok(key_result(false, "error", e.message, Vec::new(), settings));
        }
        Err(e) if e.kind == ErrorKind::MissingPermission => (
            "Key saved, but it isn't allowed to list voices. In ElevenLabs, edit the key and allow Voices (read) and Text to Speech.".to_string(),
            "warning",
            Vec::new(),
        ),
        Err(e) if e.kind == ErrorKind::OutOfCredits => ("Key saved. Your ElevenLabs account is out of credits right now, so voices couldn't be loaded.".to_string(), "warning", Vec::new()),
        Err(e) => (format!("Key saved, but it couldn't be checked right now. {}", e.message), "warning", Vec::new()),
    };
    secrets::store(secrets::ELEVEN_TARGET, &key)?;
    if settings.eleven_voice_id.is_none() {
        if let Some(v) = voices.first() {
            settings.eleven_voice_id = Some(v.id.clone());
            settings.eleven_voice_name = Some(v.name.clone());
        }
    }
    persist(&state, &settings)?;
    Ok(key_result(true, tone, message, voices, settings))
}

/// Reload the voice list from the user's account (button: Refresh).
#[tauri::command]
pub async fn explain_eleven_voices(state: State<'_, ExplainState>) -> Result<KeyResult, String> {
    let settings = current(&state)?;
    let Some(key) = eleven_key()? else {
        return Ok(key_result(
            false,
            "error",
            "Paste your ElevenLabs key first.".into(),
            Vec::new(),
            settings,
        ));
    };
    let r = tauri::async_runtime::spawn_blocking(move || eleven::list_voices(&key))
        .await
        .map_err(|e| e.to_string())?;
    Ok(match r {
        Ok(v) => {
            let msg = format!("{} voices on your ElevenLabs account.", v.len());
            key_result(true, "ok", msg, v, settings)
        }
        Err(e) => key_result(true, "error", e.message, Vec::new(), settings),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    ok: bool,
    /// Machine-readable reason when `ok` is false (invalid-key, out-of-credits, offline, ...).
    kind: Option<ErrorKind>,
    message: String,
    ms: u64,
}

/// "Test voice": speak a short fixed sample with the chosen ElevenLabs voice and say clearly
/// whether it worked. Sends only that fixed sentence (never the user's text).
#[tauri::command]
pub async fn explain_test_voice(
    app: tauri::AppHandle,
    state: State<'_, ExplainState>,
) -> Result<TestResult, String> {
    let settings = current(&state)?;
    let fail = |kind: ErrorKind, message: String| TestResult {
        ok: false,
        kind: Some(kind),
        message,
        ms: 0,
    };
    let Some(key) = eleven_key()? else {
        return Ok(fail(
            ErrorKind::InvalidKey,
            "Paste and save your ElevenLabs key first.".into(),
        ));
    };
    let Some(voice_id) = settings.eleven_voice_id.clone() else {
        return Ok(fail(
            ErrorKind::VoiceNotFound,
            "Choose a voice first.".into(),
        ));
    };
    let model = settings.eleven_model.clone();
    let speed = settings.rate;
    let started = std::time::Instant::now();
    let r = tauri::async_runtime::spawn_blocking(move || {
        eleven::synthesize(&key, &voice_id, &model, eleven::TEST_SENTENCE, speed)
    })
    .await
    .map_err(|e| e.to_string())?;
    match r {
        Ok(bytes) => {
            let ms = started.elapsed().as_millis() as u64;
            let label = settings
                .eleven_voice_name
                .clone()
                .unwrap_or_else(|| "ElevenLabs".into());
            let sink = Arc::new(TauriSink(app));
            state.speaker.play_clip(
                win::Audio {
                    bytes,
                    mime: "audio/mpeg",
                },
                label,
                sink,
            )?;
            let credits = eleven::credits(eleven::TEST_SENTENCE.len(), &settings.eleven_model);
            Ok(TestResult {
                ok: true,
                kind: None,
                message: format!("It works. Playing a short sample (about {credits} credits)."),
                ms,
            })
        }
        Err(e) => Ok(fail(e.kind, e.message)),
    }
}

/// Open the ElevenLabs page where a user creates an API key, in their default browser.
/// The address is fixed in the crate; the webview cannot choose a URL.
#[tauri::command]
pub fn explain_open_keys_page() -> Result<(), String> {
    win::open_in_browser(eleven::KEYS_PAGE_URL)
}

// ---------------------------------------------------------------------------------------
// Read aloud
// ---------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakArgs {
    /// The text to read (the summary the user is looking at).
    text: String,
    /// "windows" | "elevenlabs" - what the button said it would use.
    engine: String,
    /// The user answered the one-time "send text to ElevenLabs?" question with yes.
    #[serde(default)]
    confirm_cloud: bool,
}

#[tauri::command]
pub async fn explain_speak(
    app: tauri::AppHandle,
    state: State<'_, ExplainState>,
    args: SpeakArgs,
) -> Result<SpeakInfo, String> {
    let mut settings = current(&state)?;
    let sink: Arc<dyn SpeakSink> = Arc::new(TauriSink(app));
    let engine = match args.engine.as_str() {
        "windows" => Engine::Windows {
            voice_id: settings.windows_voice.clone(),
        },
        "elevenlabs" => {
            // Every rule for leaving the machine is checked here, not in the webview.
            if args.text.chars().count() > ELEVEN_MAX_CHARS {
                return Err(format!("That is a lot of text for ElevenLabs (over {ELEVEN_MAX_CHARS} characters). Read a shorter summary, or use the Windows voice."));
            }
            let key =
                eleven_key()?.ok_or("Paste your ElevenLabs key in the voice settings first.")?;
            let voice_id = settings
                .eleven_voice_id
                .clone()
                .ok_or("Choose an ElevenLabs voice in the voice settings first.")?;
            if !settings.eleven_consented {
                if !args.confirm_cloud {
                    return Err("consent-required".into());
                }
                settings.eleven_consented = true;
                persist(&state, &settings)?;
            }
            Engine::Eleven {
                key,
                voice_id,
                model: settings.eleven_model.clone(),
            }
        }
        _ => return Err("Unknown voice engine.".into()),
    };
    let text = args.text;
    let rate = settings.rate;
    let speaker = &state.speaker;
    speaker.speak(SpeakRequest { text, engine, rate }, sink)
}

#[tauri::command]
pub fn explain_stop_speaking(state: State<'_, ExplainState>) {
    state.speaker.stop();
}

/// Characters and ElevenLabs credits a read-aloud of `text` would use (local arithmetic only).
#[tauri::command]
pub fn explain_estimate(
    state: State<'_, ExplainState>,
    text: String,
) -> Result<serde_json::Value, String> {
    let s = current(&state)?;
    let e = speak::estimate(&text);
    Ok(
        serde_json::json!({ "sentences": e.sentences, "chars": e.chars, "credits": eleven::credits(e.chars, &s.eleven_model) }),
    )
}
