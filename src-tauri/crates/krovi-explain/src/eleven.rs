//! ElevenLabs text-to-speech: the one OPTIONAL, opt-in, user-keyed network feature.
//!
//! Off unless the user pastes their own ElevenLabs API key and switches the voice on. Krovi has
//! no relay and no shared key. What is sent, exactly (see `tts_request`):
//!   * `POST https://api.elevenlabs.io/v1/text-to-speech/<voice id>?output_format=mp3_44100_128`
//!   * headers: `xi-api-key` (the user's key), `Content-Type`, `Accept`
//!   * body: `{"text": <the sentences being read>, "model_id": <model>, "voice_settings": {"speed": <n>}}`
//! and, to fill the voice picker and check a key, `GET https://api.elevenlabs.io/v2/voices?...`
//! with the key header only. Nothing else: no identifiers, no telemetry, no other text.
//!
//! Everything except the two functions at the bottom is pure and unit-tested; the audio comes
//! back as bytes in memory and is never written to disk.

use serde::Serialize;
use serde_json::{json, Value};

pub const API_BASE: &str = "https://api.elevenlabs.io";
/// Where a user gets a key (opened in their default browser on request; Krovi sends nothing).
pub const KEYS_PAGE_URL: &str = "https://elevenlabs.io/app/settings/api-keys";
pub const DEFAULT_MODEL: &str = "eleven_flash_v2_5";
/// MP3 is available on every plan and Windows Media decodes it natively.
pub const OUTPUT_FORMAT: &str = "mp3_44100_128";
/// Refuse absurdly large audio responses (a 10 000 character request is a few MB).
pub const MAX_AUDIO_BYTES: usize = 48 * 1024 * 1024;
/// The sentence Test voice speaks - fixed, so a test never sends the user's text.
pub const TEST_SENTENCE: &str = "Hello from Krovi. This is how your chosen voice sounds.";

pub struct Model {
    pub id: &'static str,
    pub label: &'static str,
    /// Credits charged per character (Flash models are half price).
    pub credits_per_char: f32,
}

pub const MODELS: &[Model] = &[
    Model {
        id: "eleven_flash_v2_5",
        label: "Flash v2.5 - fast, half the credits",
        credits_per_char: 0.5,
    },
    Model {
        id: "eleven_multilingual_v2",
        label: "Multilingual v2 - richest quality",
        credits_per_char: 1.0,
    },
];

pub fn model(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.id == id)
}

/// Credits for reading `chars` characters with `model_id` (rounded up).
pub fn credits(chars: usize, model_id: &str) -> u64 {
    let per = model(model_id).map(|m| m.credits_per_char).unwrap_or(1.0);
    (chars as f32 * per).ceil() as u64
}

/// ElevenLabs voice ids are 20 letters/digits. Anything else must never reach a URL path.
pub fn valid_voice_id(id: &str) -> bool {
    (10..=40).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Tidy a pasted key: trim, drop wrapping quotes, reject anything that cannot be a key.
pub fn clean_key(raw: &str) -> Result<String, String> {
    let k = raw
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .trim();
    if k.is_empty() {
        return Err("Paste your ElevenLabs API key first.".into());
    }
    if k.len() < 16 || k.len() > 200 {
        return Err("That doesn't look like a full ElevenLabs API key. Copy the whole key and paste it again.".into());
    }
    if !k.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(
            "That key has spaces or odd characters in it. Copy only the key itself.".into(),
        );
    }
    Ok(k.to_string())
}

/// ElevenLabs accepts 0.7 .. 1.2 as voice speed.
pub fn speed_for_rate(rate: f32) -> f32 {
    if rate.is_finite() {
        rate.clamp(0.7, 1.2)
    } else {
        1.0
    }
}

fn pct_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A fully built request, so what leaves the machine can be inspected (and unit-tested)
/// before any socket exists.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Option<Value>,
}

pub fn tts_request(
    key: &str,
    voice_id: &str,
    model_id: &str,
    text: &str,
    speed: f32,
) -> Result<Request, String> {
    if !valid_voice_id(voice_id) {
        return Err("That voice id is not valid.".into());
    }
    if model(model_id).is_none() {
        return Err("That ElevenLabs model is not supported.".into());
    }
    Ok(Request {
        method: "POST",
        url: format!("{API_BASE}/v1/text-to-speech/{voice_id}?output_format={OUTPUT_FORMAT}"),
        headers: vec![
            ("xi-api-key", key.to_string()),
            ("Content-Type", "application/json".to_string()),
            ("Accept", "audio/mpeg".to_string()),
        ],
        body: Some(json!({
            "text": text,
            "model_id": model_id,
            "voice_settings": { "speed": speed_for_rate(speed) },
        })),
    })
}

pub fn voices_request(key: &str, page_token: Option<&str>) -> Request {
    let mut url = format!("{API_BASE}/v2/voices?page_size=100&sort=name&sort_direction=asc");
    if let Some(t) = page_token {
        url.push_str("&next_page_token=");
        url.push_str(&pct_encode(t));
    }
    Request {
        method: "GET",
        url,
        headers: vec![
            ("xi-api-key", key.to_string()),
            ("Accept", "application/json".to_string()),
        ],
        body: None,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
    pub id: String,
    pub name: String,
    pub category: Option<String>,
    /// "female · young · american" from the voice's labels.
    pub detail: String,
}

/// Parse one page of `GET /v2/voices`: the voices and the token of the next page, if any.
pub fn parse_voices(body: &str) -> Result<(Vec<Voice>, Option<String>), String> {
    let v: Value = serde_json::from_str(body)
        .map_err(|_| "ElevenLabs sent a reply Krovi could not read.".to_string())?;
    let list = v
        .get("voices")
        .and_then(Value::as_array)
        .ok_or("ElevenLabs sent a reply Krovi could not read.")?;
    let mut out = Vec::new();
    for item in list {
        let id = item
            .get("voice_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
        if !valid_voice_id(id) || name.is_empty() {
            continue;
        }
        let labels = item.get("labels").and_then(Value::as_object);
        let detail = ["gender", "age", "accent", "descriptive", "use_case"]
            .iter()
            .filter_map(|k| labels.and_then(|l| l.get(*k)).and_then(Value::as_str))
            .filter(|s| !s.is_empty())
            .map(|s| s.replace('_', " ").to_lowercase())
            .collect::<Vec<_>>()
            .join(" \u{b7} ");
        out.push(Voice {
            id: id.to_string(),
            name: name.chars().take(80).collect(),
            category: item
                .get("category")
                .and_then(Value::as_str)
                .map(str::to_string),
            detail,
        });
    }
    let has_more = v.get("has_more").and_then(Value::as_bool).unwrap_or(false);
    let next = if has_more {
        v.get("next_page_token")
            .and_then(Value::as_str)
            .map(str::to_string)
    } else {
        None
    };
    Ok((out, next))
}

// ---------------------------------------------------------------------------------------
// Errors, in words a person can act on
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorKind {
    InvalidKey,
    MissingPermission,
    OutOfCredits,
    RateLimited,
    VoiceNotFound,
    Offline,
    Unavailable,
    BadText,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VoiceError {
    pub kind: ErrorKind,
    pub message: String,
}

impl std::fmt::Display for VoiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for VoiceError {}

impl VoiceError {
    pub fn new(kind: ErrorKind) -> Self {
        let message = match kind {
            ErrorKind::InvalidKey => "ElevenLabs didn't accept this key. Copy the whole key from your ElevenLabs account and paste it again.",
            ErrorKind::MissingPermission => "This key isn't allowed to do that. In ElevenLabs, edit the key and allow Text to Speech and Voices (read).",
            ErrorKind::OutOfCredits => "Your ElevenLabs account is out of credits or over its limit. Add credits in ElevenLabs, or switch back to the Windows voice.",
            ErrorKind::RateLimited => "ElevenLabs is limiting requests from this key right now. Wait a moment and try again.",
            ErrorKind::VoiceNotFound => "That voice isn't available on your ElevenLabs account. Pick another voice.",
            ErrorKind::Offline => "Couldn't reach ElevenLabs. Check your internet connection.",
            ErrorKind::Unavailable => "ElevenLabs is having trouble right now. Try again in a minute.",
            ErrorKind::BadText => "ElevenLabs could not read that text aloud.",
            ErrorKind::Other => "ElevenLabs returned an error.",
        };
        Self {
            kind,
            message: message.to_string(),
        }
    }
}

/// Map an HTTP status and error body to a [`VoiceError`]. The body is only inspected for
/// ElevenLabs' machine-readable status words; it is never echoed to the user.
pub fn classify(status: u16, body: &str) -> VoiceError {
    let b = body.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| b.contains(n));
    let kind = if has(&["invalid_api_key", "invalid api key", "needs_authorization"]) {
        ErrorKind::InvalidKey
    } else if has(&[
        "quota_exceeded",
        "exceeds your quota",
        "out of credits",
        "insufficient_credits",
        "not enough credits",
        "character_limit",
    ]) {
        ErrorKind::OutOfCredits
    } else if has(&["missing_permissions", "missing permission"]) {
        ErrorKind::MissingPermission
    } else if has(&["voice_not_found", "voice not found", "voice_does_not_exist"]) {
        ErrorKind::VoiceNotFound
    } else if status == 429 || has(&["too_many_concurrent", "rate_limit", "system_busy"]) {
        ErrorKind::RateLimited
    } else {
        match status {
            401 => ErrorKind::InvalidKey,
            402 => ErrorKind::OutOfCredits,
            403 => ErrorKind::MissingPermission,
            404 => ErrorKind::VoiceNotFound,
            422 => ErrorKind::BadText,
            500..=599 => ErrorKind::Unavailable,
            _ => ErrorKind::Other,
        }
    };
    let mut e = VoiceError::new(kind);
    if kind == ErrorKind::Other {
        e.message = format!("ElevenLabs returned an error (HTTP {status}).");
    }
    e
}

/// Remove every occurrence of `secret` from `text` (defence in depth for transport errors).
pub fn scrub(text: &str, secret: &str) -> String {
    if secret.len() < 8 {
        return text.to_string();
    }
    text.replace(secret, "[key]")
}

// ---------------------------------------------------------------------------------------
// The sockets (feature `net`)
// ---------------------------------------------------------------------------------------

#[cfg(feature = "net")]
mod http {
    use std::io::Read;
    use std::time::Duration;

    use super::*;

    fn agent() -> ureq::Agent {
        // No proxy from the environment, no redirects to other hosts, no cookies.
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(8))
            .timeout_read(Duration::from_secs(45))
            .timeout_write(Duration::from_secs(20))
            .redirects(0)
            .user_agent("Krovi-Explain/0.1")
            .build()
    }

    fn send(key: &str, req: &Request) -> Result<ureq::Response, VoiceError> {
        let mut r = match req.method {
            "POST" => agent().post(&req.url),
            _ => agent().get(&req.url),
        };
        for (k, v) in &req.headers {
            r = r.set(k, v);
        }
        let result = match &req.body {
            Some(b) => r.send_string(&b.to_string()),
            None => r.call(),
        };
        match result {
            Ok(resp) => Ok(resp),
            Err(ureq::Error::Status(code, resp)) => {
                let mut body = String::new();
                let _ = resp.into_reader().take(4096).read_to_string(&mut body);
                Err(classify(code, &scrub(&body, key)))
            }
            Err(ureq::Error::Transport(_)) => Err(VoiceError::new(ErrorKind::Offline)),
        }
    }

    /// Every voice on the user's account (up to 3 pages of 100). Also the cheapest way to check
    /// that a key works.
    pub fn list_voices(key: &str) -> Result<Vec<Voice>, VoiceError> {
        let mut all = Vec::new();
        let mut token: Option<String> = None;
        for _ in 0..3 {
            let resp = send(key, &voices_request(key, token.as_deref()))?;
            let mut body = String::new();
            resp.into_reader()
                .take(4 * 1024 * 1024)
                .read_to_string(&mut body)
                .map_err(|_| VoiceError::new(ErrorKind::Offline))?;
            let (voices, next) = parse_voices(&body).map_err(|m| VoiceError {
                kind: ErrorKind::Other,
                message: m,
            })?;
            all.extend(voices);
            match next {
                Some(t) => token = Some(t),
                None => break,
            }
        }
        Ok(all)
    }

    /// MP3 bytes for `text`, in memory only.
    pub fn synthesize(
        key: &str,
        voice_id: &str,
        model_id: &str,
        text: &str,
        speed: f32,
    ) -> Result<Vec<u8>, VoiceError> {
        let req = tts_request(key, voice_id, model_id, text, speed).map_err(|m| VoiceError {
            kind: ErrorKind::Other,
            message: m,
        })?;
        let resp = send(key, &req)?;
        let mut audio = Vec::new();
        resp.into_reader()
            .take(MAX_AUDIO_BYTES as u64 + 1)
            .read_to_end(&mut audio)
            .map_err(|_| VoiceError::new(ErrorKind::Offline))?;
        if audio.len() > MAX_AUDIO_BYTES || audio.is_empty() {
            return Err(VoiceError::new(ErrorKind::Other));
        }
        Ok(audio)
    }
}

#[cfg(feature = "net")]
pub use http::{list_voices, synthesize};

#[cfg(not(feature = "net"))]
pub fn list_voices(_key: &str) -> Result<Vec<Voice>, VoiceError> {
    Err(VoiceError {
        kind: ErrorKind::Other,
        message: "This build of Krovi has no network support.".into(),
    })
}
#[cfg(not(feature = "net"))]
pub fn synthesize(
    _key: &str,
    _voice_id: &str,
    _model_id: &str,
    _text: &str,
    _speed: f32,
) -> Result<Vec<u8>, VoiceError> {
    Err(VoiceError {
        kind: ErrorKind::Other,
        message: "This build of Krovi has no network support.".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "sk_test_0123456789abcdef0123456789abcdef";
    const VOICE: &str = "21m00Tcm4TlvDq8ikWAM";

    #[test]
    fn tts_request_sends_exactly_the_text_the_voice_and_the_key_header() {
        let r = tts_request(KEY, VOICE, "eleven_flash_v2_5", "Read this sentence.", 1.0).unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.url, "https://api.elevenlabs.io/v1/text-to-speech/21m00Tcm4TlvDq8ikWAM?output_format=mp3_44100_128");
        let names: Vec<&str> = r.headers.iter().map(|h| h.0).collect();
        assert_eq!(names, ["xi-api-key", "Content-Type", "Accept"]);
        assert_eq!(r.headers[0].1, KEY);
        let body = r.body.unwrap();
        let obj = body.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["model_id", "text", "voice_settings"]);
        assert_eq!(obj["text"], "Read this sentence.");
        assert_eq!(obj["model_id"], "eleven_flash_v2_5");
        assert_eq!(obj["voice_settings"]["speed"], 1.0);
        // the key is a header only - never in the URL or body
        assert!(!r.url.contains(KEY) && !body.to_string().contains(KEY));
    }

    #[test]
    fn host_is_pinned_to_elevenlabs() {
        assert!(API_BASE.starts_with("https://api.elevenlabs.io"));
        let host = voices_request(KEY, None).url;
        assert!(host.starts_with("https://api.elevenlabs.io/v2/voices?"));
        assert!(KEYS_PAGE_URL.starts_with("https://elevenlabs.io/"));
    }

    #[test]
    fn voice_ids_cannot_inject_into_the_url_path() {
        for bad in [
            "",
            "short",
            "../../v1/user",
            "abc/def/ghi/jklmn",
            "voice id with spaces!!",
            "21m00Tcm4TlvDq8ikWAM?x=1",
            "21m00Tcm4TlvDq8ikWAM\n",
        ] {
            assert!(!valid_voice_id(bad), "{bad:?}");
            assert!(tts_request(KEY, bad, "eleven_flash_v2_5", "x", 1.0).is_err());
        }
        assert!(valid_voice_id(VOICE));
    }

    #[test]
    fn model_must_be_a_known_one() {
        assert!(tts_request(KEY, VOICE, "eleven_made_up", "x", 1.0).is_err());
        assert!(model(DEFAULT_MODEL).is_some());
    }

    #[test]
    fn speed_is_clamped_to_what_elevenlabs_accepts() {
        assert_eq!(speed_for_rate(0.6), 0.7);
        assert_eq!(speed_for_rate(1.8), 1.2);
        assert_eq!(speed_for_rate(1.0), 1.0);
        assert_eq!(speed_for_rate(f32::NAN), 1.0);
        let r = tts_request(KEY, VOICE, "eleven_multilingual_v2", "x", 3.0).unwrap();
        assert_eq!(r.body.unwrap()["voice_settings"]["speed"], 1.2f32 as f64);
    }

    #[test]
    fn credit_estimate_follows_the_model() {
        assert_eq!(credits(1000, "eleven_flash_v2_5"), 500);
        assert_eq!(credits(1001, "eleven_flash_v2_5"), 501);
        assert_eq!(credits(1000, "eleven_multilingual_v2"), 1000);
        assert_eq!(credits(0, "eleven_multilingual_v2"), 0);
    }

    #[test]
    fn keys_are_cleaned_and_junk_is_rejected() {
        assert_eq!(
            clean_key("  sk_0123456789abcdef  ").unwrap(),
            "sk_0123456789abcdef"
        );
        assert_eq!(
            clean_key("\"sk_0123456789abcdef\"").unwrap(),
            "sk_0123456789abcdef"
        );
        assert!(clean_key("").is_err());
        assert!(clean_key("short").is_err());
        assert!(clean_key("two words in here that is long enough").is_err());
        assert!(clean_key(&"x".repeat(500)).is_err());
    }

    #[test]
    fn voices_request_pages_and_encodes_the_token() {
        let r = voices_request(KEY, Some("a b/c+d=="));
        assert!(
            r.url.ends_with("&next_page_token=a%20b%2Fc%2Bd%3D%3D"),
            "{}",
            r.url
        );
        assert_eq!(r.headers.len(), 2);
        assert!(r.body.is_none());
    }

    #[test]
    fn parses_a_voices_page() {
        let body = r#"{"voices":[
          {"voice_id":"21m00Tcm4TlvDq8ikWAM","name":"Rachel","category":"premade","labels":{"accent":"american","gender":"female","age":"young","use_case":"narration"},"preview_url":"https://x"},
          {"voice_id":"bad id","name":"Broken"},
          {"voice_id":"AZnzlk1XvdvUeBnXmlld","name":"Domi","category":"premade","labels":{}}
        ],"has_more":true,"next_page_token":"tok123","total_count":3}"#;
        let (v, next) = parse_voices(body).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "Rachel");
        assert_eq!(
            v[0].detail,
            "female \u{b7} young \u{b7} american \u{b7} narration"
        );
        assert_eq!(v[1].detail, "");
        assert_eq!(next.as_deref(), Some("tok123"));
        let (_, none) =
            parse_voices(r#"{"voices":[],"has_more":false,"next_page_token":null}"#).unwrap();
        assert!(none.is_none());
        assert!(parse_voices("<html>").is_err());
        assert!(parse_voices("{}").is_err());
    }

    #[test]
    fn errors_are_classified_into_plain_words() {
        let c = |s, b: &str| classify(s, b).kind;
        assert_eq!(
            c(
                401,
                r#"{"detail":{"status":"invalid_api_key","message":"Invalid API key"}}"#
            ),
            ErrorKind::InvalidKey
        );
        assert_eq!(
            c(
                401,
                r#"{"detail":{"status":"quota_exceeded","message":"This request exceeds your quota of 10000"}}"#
            ),
            ErrorKind::OutOfCredits
        );
        assert_eq!(
            c(
                401,
                r#"{"detail":{"status":"missing_permissions","message":"The API key is missing the permission text_to_speech"}}"#
            ),
            ErrorKind::MissingPermission
        );
        assert_eq!(c(401, ""), ErrorKind::InvalidKey);
        assert_eq!(c(402, ""), ErrorKind::OutOfCredits);
        assert_eq!(
            c(404, r#"{"detail":{"status":"voice_not_found"}}"#),
            ErrorKind::VoiceNotFound
        );
        assert_eq!(c(429, ""), ErrorKind::RateLimited);
        assert_eq!(
            c(
                400,
                r#"{"detail":{"status":"too_many_concurrent_requests"}}"#
            ),
            ErrorKind::RateLimited
        );
        assert_eq!(c(422, "{}"), ErrorKind::BadText);
        assert_eq!(c(503, ""), ErrorKind::Unavailable);
        assert_eq!(c(418, ""), ErrorKind::Other);
        assert!(classify(418, "").message.contains("418"));
        // every kind has a human sentence that ends in a full stop
        for k in [
            ErrorKind::InvalidKey,
            ErrorKind::MissingPermission,
            ErrorKind::OutOfCredits,
            ErrorKind::RateLimited,
            ErrorKind::VoiceNotFound,
            ErrorKind::Offline,
            ErrorKind::Unavailable,
            ErrorKind::BadText,
            ErrorKind::Other,
        ] {
            assert!(VoiceError::new(k).message.ends_with('.'));
        }
    }

    #[test]
    fn error_messages_never_contain_the_key() {
        let body = format!("{{\"detail\":\"bad key {KEY}\"}}");
        let e = classify(400, &scrub(&body, KEY));
        assert!(!e.message.contains(KEY));
        assert_eq!(scrub(&format!("x {KEY} y"), KEY), "x [key] y");
        assert_eq!(scrub("abc", "k"), "abc"); // too short to be a secret: left alone
    }
}
