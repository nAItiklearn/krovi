//! Non-secret settings, persisted as JSON in the app data directory. Keys are NOT here:
//! they live in Windows Credential Manager (see `secrets`).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::eleven;
use crate::summary::SummaryLength;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VoiceEngine {
    /// Windows built-in on-device voices. The default; no network.
    #[default]
    Windows,
    /// ElevenLabs, with the user's own key. Sends the text being read to ElevenLabs.
    Elevenlabs,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub summary_length: SummaryLength,
    pub voice_engine: VoiceEngine,
    /// Windows voice id (`None` = the system default voice).
    pub windows_voice: Option<String>,
    /// Speaking speed, 1.0 = normal.
    pub rate: f32,
    pub eleven_voice_id: Option<String>,
    pub eleven_voice_name: Option<String>,
    pub eleven_model: String,
    /// The user has confirmed, once, that text may be sent to ElevenLabs.
    pub eleven_consented: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            summary_length: SummaryLength::Medium,
            voice_engine: VoiceEngine::Windows,
            windows_voice: None,
            rate: 1.0,
            eleven_voice_id: None,
            eleven_voice_name: None,
            eleven_model: eleven::DEFAULT_MODEL.to_string(),
            eleven_consented: false,
        }
    }
}

pub const RATE_MIN: f32 = 0.6;
pub const RATE_MAX: f32 = 1.8;

impl Settings {
    /// Clamp and validate everything that came from the webview.
    pub fn sanitized(mut self) -> Self {
        self.rate = if self.rate.is_finite() {
            self.rate.clamp(RATE_MIN, RATE_MAX)
        } else {
            1.0
        };
        if eleven::model(&self.eleven_model).is_none() {
            self.eleven_model = eleven::DEFAULT_MODEL.to_string();
        }
        if let Some(id) = &self.eleven_voice_id {
            if !eleven::valid_voice_id(id) {
                self.eleven_voice_id = None;
                self.eleven_voice_name = None;
            }
        }
        if let Some(name) = &mut self.eleven_voice_name {
            *name = name.chars().take(80).collect();
        }
        if let Some(v) = &mut self.windows_voice {
            *v = v.chars().take(300).collect();
            if v.trim().is_empty() {
                self.windows_voice = None;
            }
        }
        self
    }
}

/// Load settings; a missing or unreadable file gives the defaults.
pub fn load(path: &Path) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
        .unwrap_or_default()
        .sanitized()
}

/// Write settings atomically (temp file + rename).
pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_local_and_private() {
        let s = Settings::default();
        assert_eq!(s.voice_engine, VoiceEngine::Windows);
        assert!(!s.eleven_consented);
        assert!(s.eleven_voice_id.is_none());
        assert_eq!(s.rate, 1.0);
    }

    #[test]
    fn sanitizing_clamps_and_rejects_junk() {
        let s = Settings {
            rate: 99.0,
            eleven_model: "not-a-model".into(),
            eleven_voice_id: Some("../../etc/passwd".into()),
            eleven_voice_name: Some("x".repeat(500)),
            windows_voice: Some("   ".into()),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.rate, RATE_MAX);
        assert_eq!(s.eleven_model, eleven::DEFAULT_MODEL);
        assert!(s.eleven_voice_id.is_none() && s.eleven_voice_name.is_none());
        assert!(s.windows_voice.is_none());
        assert_eq!(
            Settings {
                rate: f32::NAN,
                ..Settings::default()
            }
            .sanitized()
            .rate,
            1.0
        );
    }

    #[test]
    fn json_round_trip_and_unknown_fields() {
        let s = Settings {
            summary_length: SummaryLength::Points,
            voice_engine: VoiceEngine::Elevenlabs,
            ..Settings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            json.contains("\"summaryLength\":\"points\"")
                && json.contains("\"voiceEngine\":\"elevenlabs\""),
            "{json}"
        );
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        // an older / newer file with extra or missing fields still loads
        let partial: Settings =
            serde_json::from_str(r#"{"rate":1.25,"somethingNew":true}"#).unwrap();
        assert_eq!(partial.rate, 1.25);
        assert_eq!(partial.voice_engine, VoiceEngine::Windows);
    }

    #[test]
    fn the_settings_type_has_no_field_that_could_hold_a_key() {
        let json = serde_json::to_value(Settings::default()).unwrap();
        for k in json.as_object().unwrap().keys() {
            let k = k.to_lowercase();
            assert!(
                !k.contains("key") && !k.contains("secret") && !k.contains("token"),
                "suspicious field {k}"
            );
        }
    }

    #[test]
    fn save_and_load_through_a_file() {
        let dir = std::env::temp_dir().join(format!("krovi-explain-test-{}", std::process::id()));
        let path = dir.join("explain.json");
        let s = Settings {
            rate: 1.4,
            ..Settings::default()
        };
        save(&path, &s).unwrap();
        assert_eq!(load(&path).rate, 1.4);
        assert_eq!(load(&dir.join("missing.json")), Settings::default());
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load(&path), Settings::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
