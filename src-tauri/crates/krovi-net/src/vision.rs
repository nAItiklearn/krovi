//! Questions about a picture, for Circle to Ask and the screen-awareness session.
//!
//! A thin wrapper over [`crate::ai`], where the provider registry, the request builders, the
//! response parsers and the error classification now live (Settings -> AI, plain-text questions
//! and test connection share them). This module keeps the names Circle to Ask was written
//! against: [`Provider`], [`ErrorKind`], [`VisionError`], [`Prompt`], [`ImagePart`], [`ask`].
//!
//! What leaves the device, and when: only when the user presses an explicit "Ask AI" and has
//! confirmed the disclosure that names the provider: the question text and the one picture they
//! chose, straight to the provider's API host over HTTPS. Krovi runs no relay. The key is read
//! from Windows Credential Manager at the moment of the call, sent in a header, and never put in
//! a URL, a log line or an error message.

use crate::NetError;

use crate::ai::{self, Credentials};
pub use crate::ai::{
    classify, clean_key, clean_model, clean_question, is_allowed_endpoint, parse_response,
    pick_provider, scrub, Answer, ErrorKind, HttpRequest, ImagePart, Prompt, Provider, RawReply,
    VisionError, DEFAULT_MAX_TOKENS, DEFAULT_QUESTION, MAX_IMAGE_B64, MAX_QUESTION_CHARS,
    SYSTEM_PROMPT,
};

/// Build `provider`'s request for `prompt` with `model` (empty = the provider's default for
/// pictures) and `key`. Snowflake needs its account URL as well: use [`crate::ai`] for it.
pub fn build_request(
    provider: Provider,
    key: &str,
    model: &str,
    prompt: &Prompt<'_>,
) -> Result<HttpRequest, VisionError> {
    ai::build_vision_request(provider, &Credentials::new(key), model, prompt)
}

/// Build the request, hand it to `send`, and turn the reply into an [`Answer`] or a
/// [`VisionError`]. `send` is the only thing that touches a network: [`ask`] passes the real
/// one, tests pass a fake.
pub fn run(
    provider: Provider,
    key: &str,
    model: &str,
    prompt: &Prompt<'_>,
    send: impl FnMut(&HttpRequest) -> Result<RawReply, NetError>,
) -> Result<Answer, VisionError> {
    ai::run_vision(provider, &Credentials::new(key), model, prompt, send)
}

/// Ask `provider` over HTTPS with the user's `key` (without the `net` feature there is no socket
/// code and the answer is always an error). Snowflake needs its account URL as well: use
/// [`ai::ask_vision`] for it.
pub fn ask(
    provider: Provider,
    key: &str,
    model: &str,
    prompt: &Prompt<'_>,
) -> Result<Answer, VisionError> {
    ai::ask_vision(provider, &Credentials::new(key), model, prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "sk-test-0123456789abcdefABCDEF";
    const PNG_B64: &str = "iVBORw0KGgo=";

    fn img() -> ImagePart<'static> {
        ImagePart {
            mime: "image/png",
            base64: PNG_B64,
        }
    }

    // The detailed tests live in `ai.rs`; these prove the names Circle to Ask uses still work.

    #[test]
    fn the_old_names_still_build_and_parse() {
        let r = build_request(
            Provider::Openai,
            KEY,
            "",
            &Prompt::new("What?", Some(img())),
        )
        .unwrap();
        assert_eq!(r.url, "https://api.openai.com/v1/chat/completions");
        assert!(is_allowed_endpoint(&r.url));
        let body = r#"{"choices":[{"message":{"content":"A cat."},"finish_reason":"stop"}]}"#;
        assert_eq!(
            parse_response(Provider::Openai, body).unwrap().text,
            "A cat."
        );
        assert_eq!(classify(Provider::Openai, 401, "").kind, ErrorKind::Auth);
    }

    #[test]
    fn run_sends_one_request_and_parses_the_answer() {
        let mut seen = None;
        let a = run(
            Provider::Anthropic,
            KEY2,
            "",
            &Prompt::new("q", Some(img())),
            |req| {
                seen = Some((req.url.clone(), req.headers.len()));
                Ok(RawReply {
                    status: 200,
                    body: r#"{"content":[{"type":"text","text":"fine"}],"stop_reason":"end_turn"}"#
                        .into(),
                })
            },
        )
        .unwrap();
        assert_eq!(a.text, "fine");
        assert_eq!(
            seen,
            Some(("https://api.anthropic.com/v1/messages".to_string(), 2))
        );
    }

    const KEY2: &str = "sk-ant-test-0123456789abcdefABCDEF";

    #[cfg(not(feature = "net"))]
    #[test]
    fn without_the_net_feature_ask_is_an_error_and_opens_no_socket() {
        let e = ask(Provider::Openai, KEY, "", &Prompt::new("q", None)).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Other);
        assert!(e.message.contains("no network support"));
    }
}
