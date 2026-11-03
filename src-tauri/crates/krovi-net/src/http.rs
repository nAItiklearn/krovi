//! The one place in Krovi that opens a socket. **CI-only.**
//!
//! Compiled only under the `net` feature (see the crate docs and DECISIONS.md
//! D-009). It performs a plain HTTPS GET with a short timeout using `ureq` over
//! its pure-Rust rustls TLS stack — no OpenSSL / native-TLS C dependency. There
//! is no retry, no cookies, no redirect of bodies anywhere; the response text is
//! handed straight back to a pure parser.
//!
//! NOTE: this module is not compiled or exercised on the local 32-bit-gcc host;
//! it is built and run only in CI on a full toolchain. No local test results are
//! claimed for it.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::whisper_models::{check_url, resolve_redirect, ModelError, MAX_REDIRECTS};
use crate::NetError;

const TIMEOUT: Duration = Duration::from_secs(10);

/// Model downloads are large, so there is no overall deadline; instead the
/// connection must open within 15 s and no single read may stall for more than
/// 30 s.
const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_READ_STALL_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_WRITE_TIMEOUT: Duration = Duration::from_secs(15);

/// GET `url` and return the response body as text.
pub fn get(url: &str) -> crate::Result<String> {
    let resp = ureq::get(url)
        .timeout(TIMEOUT)
        .call()
        .map_err(|e| NetError::Request(e.to_string()))?;
    resp.into_string()
        .map_err(|e| NetError::Request(e.to_string()))
}

/// Largest response body [`post_json`] reads (a text answer is a few KB; this is a ceiling).
const JSON_MAX_BODY: u64 = 4 * 1024 * 1024;
const JSON_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// A vision model can take a while to think about a screenshot.
const JSON_READ_TIMEOUT: Duration = Duration::from_secs(120);
const JSON_WRITE_TIMEOUT: Duration = Duration::from_secs(60);

/// Status and body of a JSON POST. Any HTTP status comes back as a value, not an error, so the
/// caller can read the provider's own error message.
pub(crate) struct JsonReply {
    pub status: u16,
    pub body: String,
}

/// POST `body` (JSON text) to `url` with `headers`, once. Used only by [`crate::vision`], which
/// checks the URL against its three provider hosts first.
///
/// No redirects are followed (`redirects(0)`: a 3xx is returned as a reply, never chased, so
/// a header carrying a key can never be replayed to another host), no cookies, no proxy from
/// the environment, a `Krovi/<version>` User-Agent. Transport errors carry only the failure
/// kind, never the URL or any header.
pub(crate) fn post_json(
    url: &str,
    headers: &[(&'static str, String)],
    body: &str,
) -> crate::Result<JsonReply> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(JSON_CONNECT_TIMEOUT)
        .timeout_read(JSON_READ_TIMEOUT)
        .timeout_write(JSON_WRITE_TIMEOUT)
        .user_agent(concat!("Krovi/", env!("CARGO_PKG_VERSION")))
        .build();
    let mut req = agent.post(url).set("Content-Type", "application/json");
    for (name, value) in headers {
        req = req.set(name, value);
    }
    let resp = match req.send_string(body) {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(ureq::Error::Transport(t)) => return Err(NetError::Request(t.kind().to_string())),
    };
    let status = resp.status();
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(JSON_MAX_BODY)
        .read_to_end(&mut bytes)
        .map_err(|_| NetError::Request("the reply was cut short".to_string()))?;
    Ok(JsonReply {
        status,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// An opened (status 200) model download, ready to be streamed.
pub struct Download {
    /// The response body.
    pub reader: Box<dyn Read + Send + Sync + 'static>,
    /// `Content-Length`, only when the body is not content-encoded (so it is
    /// comparable with the catalog size).
    pub content_length: Option<u64>,
}

/// GET `url` for a model download, following redirects **by hand**.
///
/// ureq's own redirect following is switched off (`redirects(0)`) so that every
/// hop (the initial URL and each `Location`) is checked against the Hugging Face
/// host allow-list before a connection is made, at most [`MAX_REDIRECTS`] hops
/// are followed, and nothing but a plain GET is ever sent: no body, no cookies
/// (ureq's `cookies` feature is not enabled), no `Authorization`, and
/// `Accept-Encoding: identity` so the bytes hashed are the bytes on the wire.
/// `cancel` is polled between hops.
pub fn open_download(url: &str, cancel: &AtomicBool) -> Result<Download, ModelError> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(DOWNLOAD_CONNECT_TIMEOUT)
        .timeout_read(DOWNLOAD_READ_STALL_TIMEOUT)
        .timeout_write(DOWNLOAD_WRITE_TIMEOUT)
        .user_agent(concat!("Krovi/", env!("CARGO_PKG_VERSION")))
        .build();

    let mut current = url.to_string();
    let mut hops = 0usize;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(ModelError::Cancelled);
        }
        check_url(&current)?;
        let resp = agent
            .get(&current)
            .set("Accept-Encoding", "identity")
            .call()
            .map_err(|e| match e {
                ureq::Error::Status(code, _) => ModelError::Http(code),
                // Not `e.to_string()`: that would echo the (signed) CDN URL.
                ureq::Error::Transport(t) => ModelError::Request(match t.message() {
                    Some(m) => format!("{}: {m}", t.kind()),
                    None => t.kind().to_string(),
                }),
            })?;

        let status = resp.status();
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            if hops >= MAX_REDIRECTS {
                return Err(ModelError::TooManyRedirects);
            }
            let location = resp.header("Location").ok_or_else(|| {
                ModelError::RedirectRefused("redirect without a Location header".to_string())
            })?;
            current = resolve_redirect(&current, location)?;
            hops += 1;
            continue;
        }
        if status != 200 {
            return Err(ModelError::Http(status));
        }

        let encoded = resp
            .header("Content-Encoding")
            .map_or(false, |e| !e.trim().eq_ignore_ascii_case("identity"));
        let content_length = if encoded {
            None
        } else {
            resp.header("Content-Length")
                .and_then(|v| v.trim().parse::<u64>().ok())
        };
        return Ok(Download {
            reader: resp.into_reader(),
            content_length,
        });
    }
}
