//! The pure part of Settings -> AI: what is stored under `ai.v1`, and the rules around it.
//!
//! **Never a key.** The keys live in Windows Credential Manager only (`Krovi/ai/<id>`). This
//! struct holds the user's choices: which provider, which model per provider, whether the user
//! accepted the Gemini notice, and the Snowflake account URL (an address, not a secret). The app
//! glue (`src-tauri/src/ai.rs`) wraps it with the settings table and the Tauri commands; every
//! rule is here so it is unit-tested without the app.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ai::{self, Provider};

/// The version of the Gemini consent screen's wording. Changing the copy means bumping this,
/// which asks every user again before Gemini works.
pub const GEMINI_CONSENT_VERSION: u32 = 1;

/// Stored under the `ai.v1` settings key.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    /// The provider features use (`anthropic`, `openai`, `gemini`, `groq`, `snowflake`), if the
    /// user chose one. Kept as text so an id from a newer or older build never breaks loading.
    pub provider: Option<String>,
    /// The model the user chose, per provider id. Missing = the provider's default.
    pub models: BTreeMap<String, String>,
    /// The user ticked the Gemini notice.
    pub gemini_consent: bool,
    /// When (Unix epoch milliseconds), for the record.
    pub gemini_consent_at: Option<u64>,
    /// Which version of the notice they accepted ([`GEMINI_CONSENT_VERSION`]).
    pub gemini_consent_version: u32,
    /// `https://<account>.snowflakecomputing.com`, normalised. Not a secret.
    pub snowflake_account: Option<String>,
}

/// Why a provider cannot be used yet although it may have a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Gemini stays off until the user accepts the notice.
    ConsentRequired,
    /// Snowflake needs the account URL.
    AccountRequired,
}

impl Gate {
    /// One sentence for the user, with the next step.
    pub fn message(self) -> &'static str {
        match self {
            Gate::ConsentRequired => {
                "Gemini is off until you accept the notice in Settings \u{2192} AI."
            }
            Gate::AccountRequired => {
                "Add your Snowflake account URL in Settings \u{2192} AI first."
            }
        }
    }
}

impl AiSettings {
    /// The provider the user chose, if it is one this build knows.
    pub fn selected(&self) -> Option<Provider> {
        self.provider.as_deref().and_then(Provider::from_id)
    }

    /// The user accepted the current Gemini notice.
    pub fn gemini_accepted(&self) -> bool {
        self.gemini_consent && self.gemini_consent_version == GEMINI_CONSENT_VERSION
    }

    /// The model the user chose for `p`, if any.
    pub fn model_choice(&self, p: Provider) -> Option<&str> {
        self.models
            .get(p.id())
            .map(|m| m.trim())
            .filter(|m| !m.is_empty())
    }

    /// The model to use for text with `p`.
    pub fn text_model(&self, p: Provider) -> String {
        p.text_model_for(self.model_choice(p))
    }

    /// The model to use for a picture with `p` (the choice when it can read pictures, otherwise
    /// the provider's image model).
    pub fn vision_model(&self, p: Provider) -> String {
        p.vision_model_for(self.model_choice(p))
    }

    /// The account URL `p` needs (`None` for providers with a fixed host, or when not set).
    pub fn account_url(&self, p: Provider) -> Option<&str> {
        if p.needs_account_url() {
            self.snowflake_account.as_deref()
        } else {
            None
        }
    }

    /// Is `p` blocked by a step the user has not done (consent, account URL)?
    pub fn gate(&self, p: Provider) -> Result<(), Gate> {
        match p {
            Provider::Gemini if !self.gemini_accepted() => Err(Gate::ConsentRequired),
            Provider::Snowflake if self.snowflake_account.is_none() => Err(Gate::AccountRequired),
            _ => Ok(()),
        }
    }

    /// `p` has a saved key and nothing else is missing.
    pub fn usable(&self, p: Provider, key_saved: bool) -> bool {
        key_saved && self.gate(p).is_ok()
    }

    /// Choose `p`. `model`: `None` keeps the model as it is, an empty string goes back to the
    /// provider's default, anything else is checked ([`ai::clean_model`]: banned and excluded
    /// families are refused) and saved. Whether `p` has a key or passes its [`Gate`] is for the
    /// caller to check first.
    pub fn select(&mut self, p: Provider, model: Option<&str>) -> Result<(), String> {
        match model {
            None => {}
            Some(m) if m.trim().is_empty() => {
                self.models.remove(p.id());
            }
            Some(m) => {
                let cleaned = ai::clean_model(p, m)?;
                self.models.insert(p.id().to_string(), cleaned);
            }
        }
        self.provider = Some(p.id().to_string());
        Ok(())
    }

    /// Forget the choice of provider (the key was removed, or the provider became unusable).
    pub fn deselect(&mut self, p: Provider) {
        if self.selected() == Some(p) {
            self.provider = None;
        }
    }

    /// Record that the user accepted (or withdrew) the Gemini notice at `now_ms`. Withdrawing it
    /// also deselects Gemini.
    pub fn set_gemini_consent(&mut self, accepted: bool, now_ms: u64) {
        if accepted {
            self.gemini_consent = true;
            self.gemini_consent_at = Some(now_ms);
            self.gemini_consent_version = GEMINI_CONSENT_VERSION;
        } else {
            self.gemini_consent = false;
            self.gemini_consent_at = None;
            self.gemini_consent_version = 0;
            self.deselect(Provider::Gemini);
        }
    }

    /// Save (or, with `None` / blank, clear) the Snowflake account URL. The URL is checked and
    /// normalised ([`ai::parse_snowflake_account`]).
    pub fn set_snowflake_account(&mut self, url: Option<&str>) -> Result<(), String> {
        match url.map(str::trim).filter(|u| !u.is_empty()) {
            None => {
                self.snowflake_account = None;
                self.deselect(Provider::Snowflake);
            }
            Some(u) => self.snowflake_account = Some(ai::parse_snowflake_account(u)?),
        }
        Ok(())
    }

    /// The same settings with everything unusable dropped: an unknown provider id, a model that
    /// is invalid or banned, a bad account URL, an out-of-date consent, a selected provider that
    /// is blocked by its [`Gate`]. Run on load so a damaged or hand-edited row cannot put a
    /// banned model or a bad address into a request.
    pub fn cleaned(mut self) -> AiSettings {
        let mut models = BTreeMap::new();
        for (id, m) in &self.models {
            let Some(p) = Provider::from_id(id) else {
                continue;
            };
            if m.trim().is_empty() {
                continue;
            }
            if let Ok(cleaned) = ai::clean_model(p, m) {
                models.insert(p.id().to_string(), cleaned);
            }
        }
        self.models = models;
        self.snowflake_account = self
            .snowflake_account
            .as_deref()
            .and_then(|u| ai::parse_snowflake_account(u).ok());
        if self.gemini_consent && self.gemini_consent_version != GEMINI_CONSENT_VERSION {
            // An older notice: ask again.
            self.gemini_consent = false;
            self.gemini_consent_at = None;
            self.gemini_consent_version = 0;
        }
        self.provider = match self.selected() {
            Some(p) if self.gate(p).is_ok() => Some(p.id().to_string()),
            _ => None,
        };
        self
    }

    /// Carry over the provider and models an earlier build kept in `circle.v1` (Circle to Ask
    /// used to have its own choice). `None` when there is nothing to carry over.
    pub fn from_legacy_circle(circle: &Value) -> Option<AiSettings> {
        let provider = circle
            .get("provider")
            .and_then(Value::as_str)
            .map(str::to_string);
        let models: BTreeMap<String, String> = circle
            .get("models")
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_str().map(|m| (k.clone(), m.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        if provider.is_none() && models.is_empty() {
            return None;
        }
        Some(
            AiSettings {
                provider,
                models,
                ..AiSettings::default()
            }
            .cleaned(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_are_empty_and_use_the_provider_defaults() {
        let s = AiSettings::default();
        assert_eq!(s.selected(), None);
        assert!(!s.gemini_accepted());
        assert_eq!(s.text_model(Provider::Anthropic), "claude-haiku-5-5");
        assert_eq!(s.text_model(Provider::Groq), "openai/gpt-oss-120b");
        assert_eq!(s.vision_model(Provider::Groq), "qwen/qwen3.8-27b");
    }

    #[test]
    fn selecting_saves_the_provider_and_a_checked_model() {
        let mut s = AiSettings::default();
        s.select(Provider::Groq, Some("openai/gpt-oss-20b"))
            .unwrap();
        assert_eq!(s.selected(), Some(Provider::Groq));
        assert_eq!(s.text_model(Provider::Groq), "openai/gpt-oss-20b");
        // A text model cannot read pictures on Groq: pictures use the image model.
        assert_eq!(s.vision_model(Provider::Groq), "qwen/qwen3.8-27b");

        // None keeps the model; an empty string resets it.
        s.select(Provider::Groq, None).unwrap();
        assert_eq!(s.text_model(Provider::Groq), "openai/gpt-oss-20b");
        s.select(Provider::Groq, Some("  ")).unwrap();
        assert_eq!(s.text_model(Provider::Groq), "openai/gpt-oss-120b");
    }

    #[test]
    fn a_banned_or_excluded_model_cannot_be_selected() {
        let mut s = AiSettings::default();
        for bad in [
            "muse-spark-1.3",
            "meta-models/muse-spark-open",
            "muse-wonder-2",
            "MUSE-GLIMMER",
            "deepseek-r1",
            "../x",
            "a b",
        ] {
            assert!(s.select(Provider::Groq, Some(bad)).is_err(), "{bad}");
        }
        assert_eq!(
            s.selected(),
            None,
            "a refused model must not change the provider"
        );
        assert!(s.models.is_empty());
    }

    #[test]
    fn gemini_stays_off_until_the_notice_is_accepted() {
        let mut s = AiSettings::default();
        assert_eq!(s.gate(Provider::Gemini), Err(Gate::ConsentRequired));
        assert!(!s.usable(Provider::Gemini, true));
        assert!(s.usable(Provider::Anthropic, true));
        assert!(!s.usable(Provider::Anthropic, false));

        s.set_gemini_consent(true, 1_700_000_000_000);
        assert!(s.gemini_accepted());
        assert_eq!(s.gemini_consent_at, Some(1_700_000_000_000));
        assert!(s.usable(Provider::Gemini, true));

        s.select(Provider::Gemini, None).unwrap();
        assert_eq!(s.selected(), Some(Provider::Gemini));
        // Withdrawing the notice switches Gemini off again and deselects it.
        s.set_gemini_consent(false, 2);
        assert!(!s.gemini_accepted());
        assert_eq!(s.selected(), None);
        assert_eq!(s.gemini_consent_at, None);
    }

    #[test]
    fn an_older_notice_is_not_consent() {
        let s = AiSettings {
            gemini_consent: true,
            gemini_consent_version: GEMINI_CONSENT_VERSION + 1,
            provider: Some("gemini".into()),
            ..AiSettings::default()
        };
        assert!(!s.gemini_accepted());
        let c = s.cleaned();
        assert!(!c.gemini_consent);
        assert_eq!(c.provider, None, "a blocked provider is dropped on load");
    }

    #[test]
    fn snowflake_needs_its_account_url() {
        let mut s = AiSettings::default();
        assert_eq!(s.gate(Provider::Snowflake), Err(Gate::AccountRequired));
        s.set_snowflake_account(Some(" HTTPS://MyOrg-MyAcct.snowflakecomputing.com/ "))
            .unwrap();
        assert_eq!(
            s.snowflake_account.as_deref(),
            Some("https://myorg-myacct.snowflakecomputing.com")
        );
        assert_eq!(s.gate(Provider::Snowflake), Ok(()));
        assert_eq!(
            s.account_url(Provider::Snowflake),
            Some("https://myorg-myacct.snowflakecomputing.com")
        );
        assert_eq!(s.account_url(Provider::Openai), None);

        s.select(Provider::Snowflake, None).unwrap();
        assert!(s.set_snowflake_account(Some("https://evil.com")).is_err());
        assert!(
            s.snowflake_account.is_some(),
            "a bad URL leaves the old one"
        );
        s.set_snowflake_account(None).unwrap();
        assert_eq!(s.snowflake_account, None);
        assert_eq!(s.selected(), None);
    }

    #[test]
    fn loading_drops_whatever_cannot_be_used() {
        let raw = json!({
            "provider": "snowflake",
            "models": {
                "groq": "muse-spark-1.3",
                "openai": "gpt-6-luna",
                "nobody": "x",
                "anthropic": "   ",
                "gemini": "models/gemini-3.5-flash-lite"
            },
            "snowflakeAccount": "http://snowflakecomputing.com.evil.com",
            "geminiConsent": true,
            "geminiConsentVersion": 1,
            "future": 7
        });
        let s: AiSettings = serde_json::from_value(raw).unwrap();
        let c = s.cleaned();
        assert_eq!(c.models.len(), 2, "{:?}", c.models);
        assert_eq!(c.models["openai"], "gpt-6-luna");
        assert_eq!(c.models["gemini"], "gemini-3.5-flash-lite");
        assert_eq!(c.snowflake_account, None);
        assert_eq!(
            c.provider, None,
            "snowflake without an account is not selectable"
        );
        assert!(c.gemini_accepted());
    }

    #[test]
    fn an_unknown_provider_id_does_not_break_loading() {
        let s: AiSettings = serde_json::from_str(r#"{"provider":"deepseek"}"#).unwrap();
        assert_eq!(s.selected(), None);
        assert_eq!(s.cleaned().provider, None);
        let s: AiSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, AiSettings::default());
    }

    #[test]
    fn the_stored_shape_has_no_key_field() {
        let mut s = AiSettings::default();
        s.select(Provider::Anthropic, Some("claude-haiku-5-5"))
            .unwrap();
        let text = serde_json::to_string(&s).unwrap();
        assert!(!text.to_lowercase().contains("apikey"));
        assert!(!text.to_lowercase().contains("secret"));
        assert!(!text.to_lowercase().contains("token"));
        let v: Value = serde_json::from_str(&text).unwrap();
        let mut names: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "geminiConsent",
                "geminiConsentAt",
                "geminiConsentVersion",
                "models",
                "provider",
                "snowflakeAccount"
            ]
        );
    }

    #[test]
    fn the_old_circle_choice_is_carried_over_once() {
        let old = json!({
            "enabled": true,
            "shortcut": "Ctrl+Alt+C",
            "provider": "anthropic",
            "models": { "anthropic": "claude-haiku-5-5", "gemini": "gemini-2.5-pro" },
            "sessionMinutes": 10
        });
        let s = AiSettings::from_legacy_circle(&old).unwrap();
        assert_eq!(s.selected(), Some(Provider::Anthropic));
        assert_eq!(s.models["anthropic"], "claude-haiku-5-5");
        // Gemini needs the notice first, so its old choice is kept but not selected.
        assert_eq!(s.models["gemini"], "gemini-2.5-pro");
        assert!(!s.gemini_accepted());

        let gemini_only = json!({ "provider": "gemini" });
        assert_eq!(
            AiSettings::from_legacy_circle(&gemini_only)
                .unwrap()
                .provider,
            None
        );
        // nothing to carry over
        assert_eq!(
            AiSettings::from_legacy_circle(&json!({ "enabled": true, "models": {} })),
            None
        );
        assert_eq!(AiSettings::from_legacy_circle(&json!(7)), None);
    }

    #[test]
    fn gate_messages_say_what_to_do() {
        assert!(Gate::ConsentRequired.message().contains("Settings"));
        assert!(Gate::AccountRequired.message().contains("account URL"));
    }
}
