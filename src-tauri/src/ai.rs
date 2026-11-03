//! Settings -> AI: bring-your-own-key providers (docs/DECISIONS.md D-022, docs/product/AI_PROVIDERS.md).
//!
//! Thin Tauri glue. The rules that matter live in `krovi-net` with unit tests:
//! `krovi_net::ai` (the provider registry, requests, parsing, error classification, test
//! connection, model lists, banned models) and `krovi_net::ai_settings` (what `ai.v1` holds and
//! the consent / account rules around it).
//!
//! ## Privacy shape (non-negotiable)
//! * **Keys live in Windows Credential Manager only** (generic credential, target
//!   `Krovi/ai/<provider>`). Never in SQLite, localStorage, a log, a backup or an error message.
//!   The webview sends a key in (`ai_set_key`) and never gets one back: it learns only "a key is
//!   saved" and its last four characters (when the key is long enough for that to be harmless).
//! * `ai.v1` (the settings row) holds the choices: provider, model per provider, the Gemini
//!   consent (a flag, a time and the version of the notice) and the Snowflake account URL.
//! * Nothing is sent anywhere until the user presses a button that needs it: Test connection,
//!   a model list, or a feature's own "ask" button. `setup` reads one settings row and nothing
//!   else: no key is read, no socket is opened.
//! * Requests go straight from this PC to the provider's own host. Krovi has no account, no
//!   server and no proxy.
//! * **Gemini is off until the user accepts the notice** (free-tier prompts may be reviewed by
//!   people at Google and used to improve its products). **Snowflake is experimental** and needs
//!   the user's account URL. See `krovi_net::ai_settings::AiSettings::gate`.
//!
//! ## Commands
//! ai_status, ai_set_key, ai_remove_key, ai_set_account, ai_test, ai_models, ai_select,
//! ai_set_gemini_consent, ai_complete, ai_open_keys_page.
//!
//! ## Used by other modules
//! `circle.rs` (Circle to Ask) reads the key helpers in [`keys`], asks [`check_usable`] /
//! [`usable`] / [`selected`] / [`vision_model`] which provider and model to use, and sends its
//! picture through [`ask_vision`]. [`AiFailure`] is the error every AI command returns.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use serde::Serialize;
use tauri::{Manager, State};

use krovi_explain::secrets;
use krovi_net::ai::{
    self as net, Credentials, ErrorKind, ModelInfo, Prompt, Provider, TestReport, TextPrompt,
    VisionError,
};
use krovi_net::ai_settings::{AiSettings, Gate, GEMINI_CONSENT_VERSION};
use krovi_sys::web;

use crate::commands::{setting_get, setting_set, CmdResult};
use crate::state::AppState;

/// The settings row. Never holds a key.
const KEY: &str = "ai.v1";
/// The row Circle to Ask used to keep its own provider and model choice in.
const LEGACY_CIRCLE_KEY: &str = "circle.v1";

/// The sentence every "no provider" refusal starts with (the UI shows it as is).
pub const NO_PROVIDER: &str = "Set up an AI provider in Settings \u{2192} AI";

// ---------------------------------------------------------------------------------------
// AI keys: the one place that touches Windows Credential Manager for AI providers.
// Target names are `Krovi/ai/<provider>` (see `Provider::credential_target`). A key is never
// returned to the webview, logged, or written to disk.
// ---------------------------------------------------------------------------------------

pub(crate) mod keys {
    use super::{net, secrets, Provider};

    /// The saved key for `p`, if any.
    pub fn read(p: Provider) -> Result<Option<String>, String> {
        secrets::read(p.credential_target())
    }

    /// Check (length, no spaces, not another provider's key) and save `raw` as `p`'s key,
    /// replacing any earlier one.
    pub fn store(p: Provider, raw: &str) -> Result<(), String> {
        let key = net::clean_key_for(p, raw)?;
        secrets::store(p.credential_target(), &key)
    }

    /// Remove `p`'s key; `true` when one was removed.
    pub fn delete(p: Provider) -> Result<bool, String> {
        secrets::delete(p.credential_target())
    }

    /// Whether a key is saved, and its last four characters when long enough to show them.
    pub fn status(p: Provider) -> (bool, Option<String>) {
        match read(p) {
            Ok(Some(k)) => (true, secrets::key_hint(&k)),
            _ => (false, None),
        }
    }

    pub fn has(p: Provider) -> bool {
        matches!(read(p), Ok(Some(_)))
    }
}

// ---------------------------------------------------------------------------------------
// Errors every AI command returns
// ---------------------------------------------------------------------------------------

/// A failed AI request or a refused one. `kind` is a `krovi_net::ai::ErrorKind` in kebab-case
/// (`auth`, `billing`, `quota`, `rate-limit`, `eligibility`, `model`, `blocked`, `unavailable`,
/// `network`, `other`) or one of ours: `no-provider`, `consent-required`, `expired`, `inactive`,
/// `not-allowed`.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AiFailure {
    pub kind: String,
    pub message: String,
    pub detail: Option<String>,
    pub status: Option<u16>,
}

pub fn fail(kind: &str, message: impl Into<String>) -> AiFailure {
    AiFailure {
        kind: kind.into(),
        message: message.into(),
        detail: None,
        status: None,
    }
}

fn kind_str(k: ErrorKind) -> String {
    serde_json::to_value(k)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "other".into())
}

impl From<VisionError> for AiFailure {
    fn from(e: VisionError) -> Self {
        AiFailure {
            kind: kind_str(e.kind),
            message: e.message,
            detail: e.detail,
            status: e.status,
        }
    }
}

fn gate_failure(g: Gate) -> AiFailure {
    let kind = match g {
        Gate::ConsentRequired => "consent-required",
        Gate::AccountRequired => "no-provider",
    };
    fail(kind, g.message())
}

fn parse_provider(id: &str) -> Result<Provider, AiFailure> {
    Provider::from_id(id).ok_or_else(|| fail("other", "Unknown AI provider."))
}

// ---------------------------------------------------------------------------------------
// State: the settings (read often, written rarely) and this session's usage counters
// ---------------------------------------------------------------------------------------

fn cell() -> &'static Mutex<AiSettings> {
    static CELL: OnceLock<Mutex<AiSettings>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(AiSettings::default()))
}

fn lk<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn settings() -> AiSettings {
    lk(cell()).clone()
}

/// Write `next` to the settings row, then make it the live copy.
fn save(state: &AppState, next: AiSettings) -> Result<(), AiFailure> {
    setting_set(state, KEY, &next).map_err(|m| fail("other", m))?;
    *lk(cell()) = next;
    Ok(())
}

#[derive(Clone, Default)]
struct Usage {
    requests: u32,
    input_tokens: u64,
    output_tokens: u64,
    last_error: Option<(ErrorKind, u64)>,
}

/// Counters for this run of Krovi only: how many requests were answered, how many tokens, and the
/// last error. Never stored, never contains a question, an answer or a key.
fn usage_cell() -> &'static Mutex<BTreeMap<&'static str, Usage>> {
    static CELL: OnceLock<Mutex<BTreeMap<&'static str, Usage>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn record_ok(p: Provider, input: Option<u64>, output: Option<u64>) {
    let mut g = lk(usage_cell());
    let u = g.entry(p.id()).or_default();
    u.requests = u.requests.saturating_add(1);
    u.input_tokens = u.input_tokens.saturating_add(input.unwrap_or(0));
    u.output_tokens = u.output_tokens.saturating_add(output.unwrap_or(0));
}

fn record_err(p: Provider, kind: ErrorKind) {
    let mut g = lk(usage_cell());
    g.entry(p.id()).or_default().last_error = Some((kind, krovi_sys::now_ms()));
}

/// Count a request's outcome and hand the result back unchanged.
pub(crate) fn tracked(
    p: Provider,
    r: Result<net::Answer, VisionError>,
) -> Result<net::Answer, VisionError> {
    match &r {
        Ok(a) => record_ok(p, a.input_tokens, a.output_tokens),
        Err(e) => record_err(p, e.kind),
    }
    r
}

fn finish<T>(p: Provider, r: Result<T, VisionError>) -> Result<T, AiFailure> {
    match r {
        Ok(v) => Ok(v),
        Err(e) => {
            record_err(p, e.kind);
            Err(AiFailure::from(e))
        }
    }
}

// ---------------------------------------------------------------------------------------
// What other modules ask
// ---------------------------------------------------------------------------------------

/// Can `p` be used right now: a key is saved and nothing else is missing (Gemini's notice,
/// Snowflake's account URL)? The error is the sentence to show.
pub(crate) fn check_usable(p: Provider) -> Result<(), AiFailure> {
    settings().gate(p).map_err(gate_failure)?;
    if keys::has(p) {
        Ok(())
    } else {
        Err(fail(
            "no-provider",
            format!("{NO_PROVIDER} (no {} key is saved yet).", p.display_name()),
        ))
    }
}

pub(crate) fn usable(p: Provider) -> bool {
    check_usable(p).is_ok()
}

/// The provider the user chose in Settings -> AI, if any.
pub(crate) fn selected() -> Option<Provider> {
    settings().selected()
}

/// The model to use for a picture with `p`: the user's choice when it can read pictures,
/// otherwise the provider's image model.
pub(crate) fn vision_model(p: Provider) -> String {
    settings().vision_model(p)
}

/// Ask `p` a question about a picture with the user's `key` (for Snowflake this also uses the
/// saved account URL). Counted in the session usage.
pub(crate) fn ask_vision(
    p: Provider,
    key: &str,
    model: &str,
    prompt: &Prompt<'_>,
) -> Result<net::Answer, VisionError> {
    let account = settings().account_url(p).map(String::from);
    let creds = Credentials::new(key).with_account(account.as_deref());
    tracked(p, net::ask_vision(p, &creds, model, prompt))
}

/// Save a key for `p` (checked first), and make `p` the chosen provider when none is chosen yet.
/// Shared with Circle's `circle_set_key`. Gemini's key is refused until the notice is accepted.
pub(crate) fn store_key(state: &AppState, p: Provider, raw: &str) -> Result<(), AiFailure> {
    if p == Provider::Gemini && !settings().gemini_accepted() {
        return Err(gate_failure(Gate::ConsentRequired));
    }
    keys::store(p, raw).map_err(|m| fail("other", m))?;
    let mut next = settings();
    adopt(&mut next, p);
    save(state, next)
}

/// When no usable provider is chosen yet and `p` is usable, choose `p`: the first provider a user
/// sets up becomes the one features use.
fn adopt(next: &mut AiSettings, p: Provider) {
    let current_ok = next
        .selected()
        .is_some_and(|c| next.usable(c, keys::has(c)));
    if !current_ok && next.usable(p, keys::has(p)) {
        let _ = next.select(p, None);
    }
}

// ---------------------------------------------------------------------------------------
// Wire types (camelCase). None carries a key.
// ---------------------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastError {
    /// A kebab-case `ErrorKind`.
    pub kind: String,
    /// Unix epoch milliseconds.
    pub at: u64,
}

/// This run's counters for one provider.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageInfo {
    pub requests: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub last_error: Option<LastError>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderState {
    pub id: &'static str,
    pub name: &'static str,
    /// A key is saved in Credential Manager.
    pub has_key: bool,
    /// Last four characters of the key, when it is long enough to show them.
    pub key_hint: Option<String>,
    /// A key is saved and nothing else is missing: features can use this provider.
    pub ready: bool,
    /// What still blocks it: `consent-required` (Gemini) or `account-required` (Snowflake).
    pub gate: Option<&'static str>,
    /// The model that will be used for text (the user's choice, or the default).
    pub model: String,
    pub default_model: String,
    /// The model that will be used for pictures.
    pub vision_model: String,
    pub supports_model_list: bool,
    pub needs_account_url: bool,
    /// The saved account URL (Snowflake). An address, not a secret.
    pub account_url: Option<String>,
    pub usage: UsageInfo,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiConsentInfo {
    /// The user accepted the current notice.
    pub accepted: bool,
    pub accepted_at: Option<u64>,
    /// The version of the notice they accepted (0 = none).
    pub version: u32,
    /// The version of the notice this build shows.
    pub current_version: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    pub providers: Vec<ProviderState>,
    /// The provider features use, when one is chosen and ready.
    pub selected: Option<&'static str>,
    /// Its text model.
    pub model: Option<String>,
    pub gemini: GeminiConsentInfo,
}

fn usage_of(p: Provider) -> UsageInfo {
    let u = lk(usage_cell()).get(p.id()).cloned().unwrap_or_default();
    UsageInfo {
        requests: u.requests,
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        last_error: u.last_error.map(|(k, at)| LastError {
            kind: kind_str(k),
            at,
        }),
    }
}

fn provider_state(s: &AiSettings, p: Provider) -> ProviderState {
    let (has_key, key_hint) = keys::status(p);
    let gate = s.gate(p).err().map(|g| match g {
        Gate::ConsentRequired => "consent-required",
        Gate::AccountRequired => "account-required",
    });
    ProviderState {
        id: p.id(),
        name: p.display_name(),
        has_key,
        key_hint,
        ready: has_key && gate.is_none(),
        gate,
        model: s.text_model(p),
        default_model: p.default_text_model().to_string(),
        vision_model: s.vision_model(p),
        supports_model_list: p.supports_model_list(),
        needs_account_url: p.needs_account_url(),
        account_url: s.account_url(p).map(String::from),
        usage: usage_of(p),
    }
}

fn status() -> AiStatus {
    let s = settings();
    let providers: Vec<ProviderState> = Provider::ALL
        .into_iter()
        .map(|p| provider_state(&s, p))
        .collect();
    let chosen = s
        .selected()
        .filter(|p| providers.iter().any(|ps| ps.id == p.id() && ps.ready));
    AiStatus {
        selected: chosen.map(Provider::id),
        model: chosen.map(|p| s.text_model(p)),
        gemini: GeminiConsentInfo {
            accepted: s.gemini_accepted(),
            accepted_at: s.gemini_consent_at,
            version: s.gemini_consent_version,
            current_version: GEMINI_CONSENT_VERSION,
        },
        providers,
    }
}

// ---------------------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------------------

/// Providers (with whether a key is saved, never the key), the chosen provider and model, the
/// Gemini notice, and this run's usage counters. Reads Credential Manager; sends nothing.
#[tauri::command]
pub fn ai_status() -> AiStatus {
    status()
}

/// Check and save `key` for `provider` in Credential Manager (replacing any earlier one). The key
/// is trimmed, must have the shape of a key and must not look like another provider's. Gemini is
/// refused until the notice is accepted.
#[tauri::command]
pub fn ai_set_key(
    state: State<'_, AppState>,
    provider: String,
    key: String,
) -> Result<AiStatus, AiFailure> {
    let p = parse_provider(&provider)?;
    store_key(&state, p, &key)?;
    Ok(status())
}

/// Remove the saved key for `provider`. If it was the chosen provider, none is chosen any more.
#[tauri::command]
pub fn ai_remove_key(state: State<'_, AppState>, provider: String) -> Result<AiStatus, AiFailure> {
    let p = parse_provider(&provider)?;
    keys::delete(p).map_err(|m| fail("other", m))?;
    let mut next = settings();
    if next.selected() == Some(p) {
        next.deselect(p);
        save(&state, next)?;
    }
    Ok(status())
}

/// Save (or, with `url` = `None` / blank, clear) the account URL of a provider that has one
/// (Snowflake). The URL is checked: https only, host ending in `.snowflakecomputing.com`.
#[tauri::command]
pub fn ai_set_account(
    state: State<'_, AppState>,
    provider: String,
    url: Option<String>,
) -> Result<AiStatus, AiFailure> {
    let p = parse_provider(&provider)?;
    if !p.needs_account_url() {
        return Err(fail("other", "That provider has no account URL."));
    }
    let mut next = settings();
    next.set_snowflake_account(url.as_deref())
        .map_err(|m| fail("other", m))?;
    adopt(&mut next, p);
    save(&state, next)?;
    Ok(status())
}

/// Test the saved key: first that the provider accepts it, then a one-token request that proves it
/// can run requests (a model list alone passes even when billing, quota or eligibility block
/// inference). Sends one or two small requests; no content of the user's.
#[tauri::command]
pub async fn ai_test(provider: String) -> Result<TestReport, AiFailure> {
    let p = parse_provider(&provider)?;
    check_usable(p)?;
    let key = keys::read(p)
        .map_err(|m| fail("other", m))?
        .ok_or_else(|| fail("no-provider", format!("{NO_PROVIDER}.")))?;
    let (model, account) = {
        let s = settings();
        (s.text_model(p), s.account_url(p).map(String::from))
    };
    let report = tauri::async_runtime::spawn_blocking(move || {
        let creds = Credentials::new(&key).with_account(account.as_deref());
        net::test_connection(p, &creds, &model)
    })
    .await
    .map_err(|e| fail("other", e.to_string()))?;
    if report.ok {
        record_ok(p, report.input_tokens, report.output_tokens);
    } else {
        record_err(p, report.kind.unwrap_or(ErrorKind::Other));
    }
    Ok(report)
}

/// The models the saved key can use (banned families filtered out; Snowflake: the documented
/// set). The UI falls back to the provider's default when this fails.
#[tauri::command]
pub async fn ai_models(provider: String) -> Result<Vec<ModelInfo>, AiFailure> {
    let p = parse_provider(&provider)?;
    check_usable(p)?;
    let key = keys::read(p)
        .map_err(|m| fail("other", m))?
        .ok_or_else(|| fail("no-provider", format!("{NO_PROVIDER}.")))?;
    let account = settings().account_url(p).map(String::from);
    let listed = tauri::async_runtime::spawn_blocking(move || {
        let creds = Credentials::new(&key).with_account(account.as_deref());
        net::list_models(p, &creds)
    })
    .await
    .map_err(|e| fail("other", e.to_string()))?;
    finish(p, listed)
}

/// Choose the provider features use, and optionally its model (`None` keeps the model, an empty
/// string goes back to the default). The provider needs a saved key, and Gemini the accepted
/// notice.
#[tauri::command]
pub fn ai_select(
    state: State<'_, AppState>,
    provider: String,
    model: Option<String>,
) -> Result<AiStatus, AiFailure> {
    let p = parse_provider(&provider)?;
    check_usable(p)?;
    let mut next = settings();
    next.select(p, model.as_deref())
        .map_err(|m| fail("model", m))?;
    save(&state, next)?;
    Ok(status())
}

/// Record that the user accepted (or withdrew) the Gemini notice. Until it is accepted Gemini
/// cannot be given a key, chosen or used. Withdrawing it also deselects Gemini.
#[tauri::command]
pub fn ai_set_gemini_consent(
    state: State<'_, AppState>,
    accepted: bool,
) -> Result<AiStatus, AiFailure> {
    let mut next = settings();
    next.set_gemini_consent(accepted, krovi_sys::now_ms());
    if accepted {
        adopt(&mut next, Provider::Gemini);
    }
    save(&state, next)?;
    Ok(status())
}

/// What a text question comes back with.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCompletion {
    pub text: String,
    pub truncated: bool,
    pub provider: &'static str,
    pub provider_name: &'static str,
    pub model: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// Shown with every answer.
    pub label: &'static str,
}

/// Send a text prompt (summarise, answer) to the chosen provider with its saved key. For features
/// to use later: the feature's own UI must have shown the user what is sent and to whom first.
/// Only the chosen provider is ever used; nothing falls back to another one.
#[tauri::command]
pub async fn ai_complete(
    prompt: String,
    system: Option<String>,
    max_tokens: Option<u32>,
) -> Result<AiCompletion, AiFailure> {
    let p = selected().ok_or_else(|| fail("no-provider", format!("{NO_PROVIDER}.")))?;
    check_usable(p)?;
    let key = keys::read(p)
        .map_err(|m| fail("other", m))?
        .ok_or_else(|| fail("no-provider", format!("{NO_PROVIDER}.")))?;
    let (model, account) = {
        let s = settings();
        (s.text_model(p), s.account_url(p).map(String::from))
    };
    let shown_model = model.clone();
    let asked = tauri::async_runtime::spawn_blocking(move || {
        let creds = Credentials::new(&key).with_account(account.as_deref());
        let mut tp = TextPrompt::new(&prompt, system.as_deref());
        if let Some(n) = max_tokens {
            tp.max_tokens = n;
        }
        net::complete_text(p, &creds, &model, &tp)
    })
    .await
    .map_err(|e| fail("other", e.to_string()))?;
    let a = tracked(p, asked).map_err(AiFailure::from)?;
    Ok(AiCompletion {
        text: a.text,
        truncated: a.truncated,
        provider: p.id(),
        provider_name: p.display_name(),
        model: shown_model,
        input_tokens: a.input_tokens,
        output_tokens: a.output_tokens,
        label: net::AI_OUTPUT_LABEL,
    })
}

/// Open the page where the user creates a key for `provider` (or, for Snowflake, the page about
/// access tokens) in their browser. The address comes from a fixed list; the webview cannot
/// choose one. Krovi itself sends nothing.
#[tauri::command]
pub fn ai_open_keys_page(provider: String) -> CmdResult<()> {
    let p = Provider::from_id(provider.trim()).ok_or("Unknown AI provider.")?;
    let url = web::keys_page(p.id()).ok_or("Unknown AI provider.")?;
    web::open_url(url)
}

// ---------------------------------------------------------------------------------------
// Start-up
// ---------------------------------------------------------------------------------------

/// Load the settings row (once, carrying over what Circle to Ask used to keep). Reads one settings
/// row only: no key is read and nothing is sent.
pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let state = app.state::<AppState>();
    let stored: Option<AiSettings> = setting_get(&state, KEY);
    let first_run = stored.is_none();
    let loaded = match stored {
        Some(s) => s,
        None => {
            let legacy: Option<serde_json::Value> = setting_get(&state, LEGACY_CIRCLE_KEY);
            legacy
                .and_then(|v| AiSettings::from_legacy_circle(&v))
                .unwrap_or_default()
        }
    }
    .cleaned();
    *lk(cell()) = loaded.clone();
    if first_run {
        // Keep the carried-over choice so this only happens once.
        let _ = setting_set(&state, KEY, &loaded);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_page_the_browser_may_open_is_the_registry_page() {
        for p in Provider::ALL {
            assert_eq!(web::keys_page(p.id()), Some(p.key_page()), "{}", p.id());
        }
    }

    #[test]
    fn a_net_error_becomes_a_failure_with_its_kind() {
        let e = net::classify(
            Provider::Gemini,
            403,
            r#"{"error":{"message":"Your project has been denied access."}}"#,
        );
        let f = AiFailure::from(e);
        assert_eq!(f.kind, "eligibility");
        assert_eq!(f.status, Some(403));
        assert!(f.message.contains("Gemini"));
        let v = serde_json::to_value(&f).unwrap();
        assert!(v.get("kind").is_some() && v.get("message").is_some());
    }

    #[test]
    fn gate_failures_have_kinds_the_ui_knows() {
        assert_eq!(gate_failure(Gate::ConsentRequired).kind, "consent-required");
        assert_eq!(gate_failure(Gate::AccountRequired).kind, "no-provider");
    }

    #[test]
    fn the_status_wire_shape_has_no_key_field() {
        let s = AiSettings::default();
        let p = serde_json::to_value(ProviderState {
            id: "openai",
            name: "OpenAI",
            has_key: true,
            key_hint: Some("abcd".into()),
            ready: true,
            gate: None,
            model: s.text_model(Provider::Openai),
            default_model: "gpt-6-luna".into(),
            vision_model: s.vision_model(Provider::Openai),
            supports_model_list: true,
            needs_account_url: false,
            account_url: None,
            usage: usage_of(Provider::Openai),
        })
        .unwrap();
        let names: Vec<&str> = p.as_object().unwrap().keys().map(String::as_str).collect();
        for n in names {
            assert!(
                !n.eq_ignore_ascii_case("key")
                    && !n.eq_ignore_ascii_case("apikey")
                    && !n.to_lowercase().contains("secret"),
                "{n}"
            );
        }
    }
}
