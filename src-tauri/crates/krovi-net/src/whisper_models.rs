//! Whisper model catalog and the verified, user-initiated model downloader.
//!
//! Speech recognition in Krovi runs on-device. The one network-adjacent step is
//! fetching a ggml Whisper model file, and it only ever happens when the user
//! presses "download" for a model they picked. Nothing here is prefetched,
//! scheduled or retried in the background.
//!
//! ## What is pure (always compiled, unit-tested offline)
//! * the [`catalog`]: a static list of quantized (`q5_1`) ggml models from the
//!   official `ggerganov/whisper.cpp` repository on Hugging Face, each with its
//!   exact size in bytes and exact SHA-256 (taken from the repository's Git LFS
//!   pointers; none is invented);
//! * the redirect policy ([`is_allowed_download_url`]): https only, host must be
//!   `huggingface.co`, `*.huggingface.co` or `*.hf.co`, default port only, no
//!   credentials in the URL;
//! * the streaming core `write_verified`: copies any [`Read`] into
//!   `<file>.part` while hashing, enforces the exact expected size, checks the
//!   SHA-256, and only then renames to the final name. On any failure or cancel
//!   the `.part` file is removed;
//! * [`model_status`], [`delete_model`] and [`verify_model`].
//!
//! ## What needs the `net` feature
//! Only the socket: [`download_model`] opens the connection through
//! `http::open_download`, which follows at most [`MAX_REDIRECTS`] redirects
//! by hand and refuses any hop outside the allow-list. Without `net`,
//! `download_model` returns [`ModelError::Disabled`] and no socket code is
//! compiled.
//!
//! ## What is sent
//! A plain `GET` per hop with a `Krovi/<version>` User-Agent. No request body, no
//! cookies, no account, no identifiers, no telemetry. See
//! `docs/SECURITY-network-audit.md`.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use sha2::{Digest, Sha256};

/// Base URL every catalog download is built from (the `resolve` endpoint, which
/// redirects to Hugging Face's CDN).
pub const MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/";

/// Licence string shown next to every model.
pub const MODEL_LICENSE: &str = "MIT, OpenAI Whisper weights via ggerganov/whisper.cpp";

/// Maximum number of HTTP redirects followed for one download.
pub const MAX_REDIRECTS: usize = 5;

/// Suffix of the in-progress file next to the final model file.
const PART_SUFFIX: &str = ".part";

/// Read/write granularity of the streaming loop (also the progress cadence).
const CHUNK: usize = 64 * 1024;

// ---------------------------------------------------------------- catalog ----

/// One downloadable ggml Whisper model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperModel {
    /// Short stable id (`"base"`, `"base.en"`).
    pub id: &'static str,
    /// File name in the repository and on disk (`ggml-base-q5_1.bin`).
    pub file_name: &'static str,
    /// Human-readable name for the settings UI.
    pub display_name: &'static str,
    /// `false` for English-only (`.en`) models.
    pub multilingual: bool,
    /// Exact size of the file in bytes.
    pub size_bytes: u64,
    /// Exact SHA-256 of the file, 64 lowercase hex characters.
    pub sha256: &'static str,
    /// Licence line.
    pub license: &'static str,
}

impl WhisperModel {
    /// The download URL: [`MODEL_BASE_URL`] + the file name.
    pub fn url(&self) -> String {
        format!("{MODEL_BASE_URL}{}", self.file_name)
    }
}

/// The catalog. Sizes and SHA-256 values are the `size` and `oid sha256:` of
/// each file's Git LFS pointer in `ggerganov/whisper.cpp` (revision `main`).
pub static MODELS: [WhisperModel; 4] = [
    WhisperModel {
        id: "tiny",
        file_name: "ggml-tiny-q5_1.bin",
        display_name: "Tiny (multilingual)",
        multilingual: true,
        size_bytes: 32_152_673,
        sha256: "818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7",
        license: MODEL_LICENSE,
    },
    WhisperModel {
        id: "base",
        file_name: "ggml-base-q5_1.bin",
        display_name: "Base (multilingual)",
        multilingual: true,
        size_bytes: 59_707_625,
        sha256: "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898",
        license: MODEL_LICENSE,
    },
    WhisperModel {
        id: "small",
        file_name: "ggml-small-q5_1.bin",
        display_name: "Small (multilingual)",
        multilingual: true,
        size_bytes: 190_085_487,
        sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
        license: MODEL_LICENSE,
    },
    WhisperModel {
        id: "base.en",
        file_name: "ggml-base.en-q5_1.bin",
        display_name: "Base (English only)",
        multilingual: false,
        size_bytes: 59_721_011,
        sha256: "4baf70dd0d7c4247ba2b81fafd9c01005ac77c2f9ef064e00dcf195d0e2fdd2f",
        license: MODEL_LICENSE,
    },
];

/// All catalog models, smallest first.
pub fn catalog() -> &'static [WhisperModel] {
    &MODELS
}

/// Look a model up by [`WhisperModel::id`].
pub fn find_model(id: &str) -> Option<&'static WhisperModel> {
    MODELS.iter().find(|m| m.id == id)
}

// ----------------------------------------------------------------- errors ----

/// Errors from the model catalog / downloader.
///
/// Hand-rolled (no `thiserror`), like [`crate::NetError`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModelError {
    /// Networking is not compiled into this build (the `net` feature is off).
    Disabled,
    /// The caller passed something invalid (a model that is not in the catalog,
    /// an unsafe file name). No request is made.
    InvalidInput(String),
    /// The caller's cancel flag was set. Any partial file has been removed.
    Cancelled,
    /// The server answered with an unexpected HTTP status.
    Http(u16),
    /// The connection or the body stream failed (connect, TLS, stall timeout).
    Request(String),
    /// A URL or redirect target was refused by the host allow-list.
    RedirectRefused(String),
    /// More than [`MAX_REDIRECTS`] redirects.
    TooManyRedirects,
    /// The server sent more bytes than the catalog size. Download aborted.
    TooLarge {
        /// Expected size in bytes.
        expected: u64,
    },
    /// The stream ended with a different byte count than the catalog size.
    SizeMismatch {
        /// Expected size in bytes.
        expected: u64,
        /// Bytes actually received / found.
        actual: u64,
    },
    /// The SHA-256 of the data does not match the catalog.
    HashMismatch,
    /// A local file-system error.
    Io(String),
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelError::Disabled => write!(
                f,
                "networking is disabled in this build (enable the `net` feature)"
            ),
            ModelError::InvalidInput(m) => write!(f, "invalid input: {m}"),
            ModelError::Cancelled => write!(f, "download cancelled"),
            ModelError::Http(code) => write!(f, "the server answered HTTP {code}"),
            ModelError::Request(m) => write!(f, "download failed: {m}"),
            ModelError::RedirectRefused(m) => write!(f, "refused to follow redirect: {m}"),
            ModelError::TooManyRedirects => {
                write!(f, "too many redirects (more than {MAX_REDIRECTS})")
            }
            ModelError::TooLarge { expected } => write!(
                f,
                "the server sent more data than the expected {expected} bytes"
            ),
            ModelError::SizeMismatch { expected, actual } => {
                write!(f, "wrong size: expected {expected} bytes, got {actual}")
            }
            ModelError::HashMismatch => {
                write!(f, "SHA-256 mismatch: the file is not the expected model")
            }
            ModelError::Io(m) => write!(f, "file error: {m}"),
        }
    }
}

impl std::error::Error for ModelError {}

fn io_err(e: io::Error) -> ModelError {
    ModelError::Io(e.to_string())
}

// ------------------------------------------------------- redirect policy ----

/// `true` if `host` (already lowercase, no port) is an allowed download host:
/// `huggingface.co`, any `*.huggingface.co`, or any `*.hf.co`.
///
/// The check is on whole labels: `evilhuggingface.co` and
/// `huggingface.co.evil.com` do not match.
fn is_allowed_host(host: &str) -> bool {
    host == "huggingface.co" || host.ends_with(".huggingface.co") || host.ends_with(".hf.co")
}

/// Parse `url` as `https://host[:443]/...` and return the lowercase host, or a
/// short static reason why it is not acceptable. Rejects whitespace/control
/// characters, backslashes, `user:pass@` credentials, any port but 443, and any
/// host with characters outside `[A-Za-z0-9.-]`.
fn https_host(url: &str) -> Result<String, &'static str> {
    if url
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
    {
        return Err("invalid characters in URL");
    }
    let rest = match url.get(..8) {
        Some(p) if p.eq_ignore_ascii_case("https://") => &url[8..],
        _ => return Err("not an https URL"),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err("missing host");
    }
    if authority.contains('@') {
        return Err("credentials in URL");
    }
    let host = match authority.split_once(':') {
        None => authority,
        Some((h, port)) => {
            if port != "443" {
                return Err("non-default port");
            }
            h
        }
    };
    if host.is_empty()
        || host.starts_with('.')
        || host.ends_with('.')
        || host.contains("..")
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
    {
        return Err("invalid host");
    }
    Ok(host.to_ascii_lowercase())
}

/// `true` if `url` may be requested for a model download (the initial URL or
/// any redirect hop): https, default port, no credentials, and an allowed host
/// (`huggingface.co`, `*.huggingface.co`, `*.hf.co`).
pub fn is_allowed_download_url(url: &str) -> bool {
    https_host(url).is_ok_and(|h| is_allowed_host(&h))
}

/// Like [`is_allowed_download_url`] but with a reason. Only the (validated,
/// charset-restricted) host is ever echoed, never the full URL.
#[cfg_attr(not(feature = "net"), allow(dead_code))]
pub(crate) fn check_url(url: &str) -> Result<(), ModelError> {
    let host = https_host(url).map_err(|r| ModelError::RedirectRefused(r.to_string()))?;
    if is_allowed_host(&host) {
        Ok(())
    } else {
        Err(ModelError::RedirectRefused(format!(
            "host not allowed: {host}"
        )))
    }
}

/// Resolve a redirect `Location` against the URL it came from and check the
/// result against the allow-list. Accepts an absolute `https://` URL, a
/// scheme-relative `//host/path` and an absolute-path `/path`; anything else
/// (including `http://`, other schemes and bare relative paths) is refused.
#[cfg_attr(not(feature = "net"), allow(dead_code))]
pub(crate) fn resolve_redirect(current: &str, location: &str) -> Result<String, ModelError> {
    let is_https = location
        .get(..8)
        .is_some_and(|p| p.eq_ignore_ascii_case("https://"));
    let candidate = if is_https {
        location.to_string()
    } else if location.starts_with("//") {
        format!("https:{location}")
    } else if location.starts_with('/') {
        // `current` was validated before it was requested, so its authority is
        // a plain `host[:443]`.
        let after_scheme = current.get(8..).unwrap_or("");
        let end = after_scheme
            .find(['/', '?', '#'])
            .unwrap_or(after_scheme.len());
        format!("{}{location}", current.get(..8 + end).unwrap_or(""))
    } else {
        return Err(ModelError::RedirectRefused(
            "unsupported redirect target".to_string(),
        ));
    };
    check_url(&candidate)?;
    Ok(candidate)
}

// ----------------------------------------------------------------- status ----

/// Whether a model file is on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ModelStatus {
    /// The final file exists and has exactly the catalog size. (Size only; run
    /// [`verify_model`] for a full hash check.)
    Installed {
        /// Full path of the model file.
        path: PathBuf,
        /// Its size in bytes.
        bytes: u64,
    },
    /// Neither the final file nor a `.part` file exists.
    Missing,
    /// There is a leftover `<file>.part` (an interrupted download) or a final
    /// file whose size differs from the catalog. Not usable; re-download.
    Partial,
}

/// Reject names that could escape `dir`: only `[A-Za-z0-9._-]`, must start with
/// an alphanumeric, no `..`.
fn validate_file_name(name: &str) -> Result<(), ModelError> {
    let ok = !name.is_empty()
        && name.len() <= 128
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && !name.contains("..")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(ModelError::InvalidInput(format!(
            "unsafe model file name: {name:?}"
        )))
    }
}

fn final_path(dir: &Path, file_name: &str) -> PathBuf {
    dir.join(file_name)
}

fn part_path(dir: &Path, file_name: &str) -> PathBuf {
    dir.join(format!("{file_name}{PART_SUFFIX}"))
}

/// Report whether `model` is installed in `dir`. Cheap: stats the file, never
/// hashes. See [`ModelStatus`] for the three outcomes.
pub fn model_status(model: &WhisperModel, dir: &Path) -> ModelStatus {
    if validate_file_name(model.file_name).is_err() {
        return ModelStatus::Missing;
    }
    let path = final_path(dir, model.file_name);
    match fs::metadata(&path) {
        Ok(m) if m.is_file() => {
            if m.len() == model.size_bytes {
                return ModelStatus::Installed {
                    path,
                    bytes: m.len(),
                };
            }
            return ModelStatus::Partial;
        }
        _ => {}
    }
    match fs::metadata(part_path(dir, model.file_name)) {
        Ok(m) if m.is_file() => ModelStatus::Partial,
        _ => ModelStatus::Missing,
    }
}

/// Delete the model file and any leftover `.part` from `dir`. Idempotent: a
/// file that is already gone is not an error.
pub fn delete_model(model: &WhisperModel, dir: &Path) -> Result<(), ModelError> {
    validate_file_name(model.file_name)?;
    let mut first_err = None;
    for p in [
        final_path(dir, model.file_name),
        part_path(dir, model.file_name),
    ] {
        match fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(io_err(e));
                }
            }
        }
    }
    first_err.map_or(Ok(()), Err)
}

/// Fully verify an installed model: size, then SHA-256 over the whole file
/// (a second or two for the largest catalog entry). `Ok(())` means the file is
/// byte-for-byte the catalog model.
pub fn verify_model(model: &WhisperModel, dir: &Path) -> Result<(), ModelError> {
    validate_file_name(model.file_name)?;
    let path = final_path(dir, model.file_name);
    let mut file = File::open(&path).map_err(io_err)?;
    let len = file.metadata().map_err(io_err)?.len();
    if len != model.size_bytes {
        return Err(ModelError::SizeMismatch {
            expected: model.size_bytes,
            actual: len,
        });
    }
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut total = 0u64;
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(io_err(e)),
        };
        total += n as u64;
        hasher.update(&buf[..n]);
    }
    if total != model.size_bytes {
        return Err(ModelError::SizeMismatch {
            expected: model.size_bytes,
            actual: total,
        });
    }
    if to_hex(&hasher.finalize()).eq_ignore_ascii_case(model.sha256) {
        Ok(())
    } else {
        Err(ModelError::HashMismatch)
    }
}

// -------------------------------------------------------- streaming core ----

fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    out
}

/// Removes the `.part` file on drop unless disarmed (covers early returns and
/// panics, e.g. in the caller's progress callback).
struct PartGuard<'a> {
    path: &'a Path,
    armed: bool,
}

impl Drop for PartGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(self.path);
        }
    }
}

/// Copy `reader` into `<dest_dir>/<file_name>.part`, hashing as it goes, then
/// verify and atomically rename to `<dest_dir>/<file_name>`.
///
/// * Never reads more than `expected_size + 1` bytes: one byte past the end is
///   enough to prove the stream is too long, and the `.part` never grows past
///   `expected_size`.
/// * `cancel` is checked before every read; `progress(downloaded, expected)` is
///   called once at the start and after every chunk (at most 64 KiB).
/// * On any error, cancel, or panic the `.part` is deleted and a pre-existing
///   final file is left untouched.
///
/// This is the test seam: it takes any [`Read`], so the size, hash and cancel
/// logic is exercised without a socket.
#[cfg_attr(not(feature = "net"), allow(dead_code))]
pub(crate) fn write_verified<R: Read>(
    mut reader: R,
    dest_dir: &Path,
    file_name: &str,
    expected_size: u64,
    expected_sha256: &str,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<PathBuf, ModelError> {
    validate_file_name(file_name)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(ModelError::Cancelled);
    }
    fs::create_dir_all(dest_dir).map_err(io_err)?;
    let part = part_path(dest_dir, file_name);
    let dest = final_path(dest_dir, file_name);

    let mut guard = PartGuard {
        path: &part,
        armed: true,
    };
    let mut hasher = Sha256::new();
    {
        // The file handle is dropped at the end of this block, before the guard
        // can remove it and before the rename.
        let mut file = File::create(&part).map_err(io_err)?;
        let mut buf = vec![0u8; CHUNK];
        let mut downloaded = 0u64;
        progress(0, expected_size);
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(ModelError::Cancelled);
            }
            // At most one byte beyond the expected end.
            let want = (expected_size - downloaded)
                .saturating_add(1)
                .min(CHUNK as u64) as usize;
            let n = match reader.read(&mut buf[..want]) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(ModelError::Request(e.to_string())),
            };
            if downloaded + n as u64 > expected_size {
                return Err(ModelError::TooLarge {
                    expected: expected_size,
                });
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n]).map_err(io_err)?;
            downloaded += n as u64;
            progress(downloaded, expected_size);
        }
        if downloaded != expected_size {
            return Err(ModelError::SizeMismatch {
                expected: expected_size,
                actual: downloaded,
            });
        }
        file.flush().map_err(io_err)?;
        file.sync_all().map_err(io_err)?;
    }

    if !to_hex(&hasher.finalize()).eq_ignore_ascii_case(expected_sha256) {
        return Err(ModelError::HashMismatch);
    }
    fs::rename(&part, &dest).map_err(io_err)?;
    guard.armed = false;
    Ok(dest)
}

// ------------------------------------------------------------- downloader ----

/// Only exact catalog entries may be downloaded: the URL host, file name, size
/// and hash are all pinned in this crate, never taken from the caller.
fn require_catalog_member(model: &WhisperModel) -> Result<(), ModelError> {
    match find_model(model.id) {
        Some(m) if m == model => Ok(()),
        _ => Err(ModelError::InvalidInput(format!(
            "{:?} is not an unmodified catalog model",
            model.id
        ))),
    }
}

/// Download `model` into `dest_dir` (created if missing), verified.
///
/// **User-initiated only.** Streams `<file>.part`, hashing with SHA-256 as it
/// goes; aborts if more than `model.size_bytes` arrive; checks the hash; then
/// atomically renames to `<dest_dir>/<file_name>` (replacing any older file of
/// that name) and returns that path. On any failure or cancel the `.part` is
/// deleted.
///
/// * `progress(downloaded, total)` is called on the calling thread, after every
///   chunk of at most 64 KiB; throttle in the UI layer if needed.
/// * `cancel` is polled before every read; set it from another thread to abort
///   (returns [`ModelError::Cancelled`]). A stalled connection ends after the
///   30 s read timeout.
/// * Only models that are unmodified [`catalog`] entries are accepted.
/// * Callers must not run two downloads of the same model into the same
///   directory at once (they would share the `.part` file).
///
/// Without the `net` feature this returns [`ModelError::Disabled`] (after the
/// same input checks) and no socket code exists in the build.
pub fn download_model(
    model: &WhisperModel,
    dest_dir: &Path,
    mut progress: impl FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<PathBuf, ModelError> {
    require_catalog_member(model)?;
    validate_file_name(model.file_name)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(ModelError::Cancelled);
    }
    download_inner(model, dest_dir, &mut progress, cancel)
}

#[cfg(feature = "net")]
fn download_inner(
    model: &WhisperModel,
    dest_dir: &Path,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<PathBuf, ModelError> {
    fs::create_dir_all(dest_dir).map_err(io_err)?;
    let dl = crate::http::open_download(&model.url(), cancel)?;
    // `content_length` is only reported when the body is not content-encoded, so
    // a mismatch is a wrong file; fail before writing a byte.
    if let Some(len) = dl.content_length {
        if len != model.size_bytes {
            return Err(ModelError::SizeMismatch {
                expected: model.size_bytes,
                actual: len,
            });
        }
    }
    write_verified(
        dl.reader,
        dest_dir,
        model.file_name,
        model.size_bytes,
        model.sha256,
        progress,
        cancel,
    )
}

#[cfg(not(feature = "net"))]
fn download_inner(
    model: &WhisperModel,
    dest_dir: &Path,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<PathBuf, ModelError> {
    let _ = (model, dest_dir, progress, cancel);
    Err(ModelError::Disabled)
}

// ------------------------------------------------------------------ tests ----

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::atomic::AtomicUsize;

    const ABC: &[u8] = b"abc";
    const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const ZERO_SHA: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    /// A model whose real content is the 3 bytes "abc", for status/verify tests.
    const ABC_MODEL: WhisperModel = WhisperModel {
        id: "test",
        file_name: "ggml-test.bin",
        display_name: "Test",
        multilingual: true,
        size_bytes: 3,
        sha256: ABC_SHA,
        license: MODEL_LICENSE,
    };

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Unique temp directory, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let p =
                std::env::temp_dir().join(format!("krovi-net-whisper-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
        fn is_empty(&self) -> bool {
            fs::read_dir(&self.0).unwrap().count() == 0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sha_hex(data: &[u8]) -> String {
        to_hex(&Sha256::digest(data))
    }

    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i.wrapping_mul(31) % 251) as u8).collect()
    }

    /// Yields at most `step` bytes per read.
    struct Trickle<'a> {
        data: &'a [u8],
        pos: usize,
        step: usize,
    }

    impl Read for Trickle<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.step.min(buf.len()).min(self.data.len() - self.pos);
            buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    /// Counts the bytes handed out by the inner reader.
    struct Counting<R> {
        inner: R,
        total: usize,
    }

    impl<R: Read> Read for Counting<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.total += n;
            Ok(n)
        }
    }

    /// Serves `data`, then fails with an I/O error instead of EOF.
    struct FailAfter<'a> {
        data: &'a [u8],
        pos: usize,
    }

    impl Read for FailAfter<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos >= self.data.len() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "stalled"));
            }
            let n = buf.len().min(self.data.len() - self.pos);
            buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    fn no_progress() -> impl FnMut(u64, u64) {
        |_, _| {}
    }

    // ---- catalog ----

    #[test]
    fn catalog_has_the_required_models() {
        for id in ["tiny", "base", "small", "base.en"] {
            assert!(find_model(id).is_some(), "missing {id}");
        }
        assert_eq!(catalog().len(), MODELS.len());
        assert!(find_model("nope").is_none());
    }

    #[test]
    fn catalog_ids_and_files_are_unique() {
        let ids: HashSet<_> = catalog().iter().map(|m| m.id).collect();
        let files: HashSet<_> = catalog().iter().map(|m| m.file_name).collect();
        assert_eq!(ids.len(), catalog().len());
        assert_eq!(files.len(), catalog().len());
    }

    #[test]
    fn catalog_entries_are_well_formed() {
        for m in catalog() {
            let url = m.url();
            assert!(
                url.starts_with("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/"),
                "{url}"
            );
            assert!(url.ends_with(m.file_name), "{url}");
            assert!(is_allowed_download_url(&url), "{url}");
            assert!(validate_file_name(m.file_name).is_ok(), "{}", m.file_name);
            assert_eq!(m.sha256.len(), 64, "{}", m.id);
            assert!(
                m.sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{} hash must be lowercase hex",
                m.id
            );
            assert!(
                m.size_bytes > 1_000_000 && m.size_bytes < 1_000_000_000,
                "{}",
                m.id
            );
            assert_eq!(
                m.license,
                "MIT, OpenAI Whisper weights via ggerganov/whisper.cpp"
            );
            assert!(!m.display_name.is_empty());
            assert_eq!(m.multilingual, !m.id.ends_with(".en"), "{}", m.id);
            assert_eq!(m.file_name, format!("ggml-{}-q5_1.bin", m.id), "{}", m.id);
        }
    }

    #[test]
    fn catalog_hashes_are_distinct() {
        let hashes: HashSet<_> = catalog().iter().map(|m| m.sha256).collect();
        assert_eq!(hashes.len(), catalog().len());
    }

    #[test]
    fn catalog_serializes_camel_case() {
        let v = serde_json::to_value(find_model("base").unwrap()).unwrap();
        assert_eq!(v["fileName"], "ggml-base-q5_1.bin");
        assert_eq!(v["sizeBytes"], 59_707_625u64);
        assert_eq!(v["multilingual"], true);
    }

    // ---- redirect host allow-list ----

    #[test]
    fn allows_huggingface_hosts() {
        for url in [
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/x.bin",
            "https://cdn-lfs.hf.co/repos/ab/cd/file?response-content-disposition=x",
            "https://cas-bridge.xethub.hf.co/xet-bridge-us/abc/def?X-Amz-Signature=1",
            "https://cdn-lfs-us-1.hf.co/repos/x",
            "https://cdn-lfs.huggingface.co/x",
            "https://HuggingFace.co/x",
            "https://huggingface.co:443/x",
            "HTTPS://huggingface.co/x",
            "https://huggingface.co",
        ] {
            assert!(is_allowed_download_url(url), "should allow {url}");
        }
    }

    #[test]
    fn rejects_foreign_and_lookalike_hosts() {
        for url in [
            "https://evil.com/x",
            "https://huggingface.co.evil.com/x",
            "https://evilhuggingface.co/x",
            "https://evil-hf.co/x",
            "https://hf.co.evil.com/x",
            "https://notcdn.hf.co.evil.com/x",
            "https://huggingface.co@evil.com/x",
            "https://evil.com@huggingface.co/x",
            "https://huggingface.co:8443/x",
            "https://huggingface.co:/x",
            "https://huggingface.co./x",
            "https://.huggingface.co/x",
            "https://huggingface..co/x",
            "https://huggingface.co\\.evil.com/x",
            "https://huggingface.co%2eevil.com/x",
            "https://[::1]/x",
            "https://127.0.0.1/x",
            "https://hf.co/x",
            "https://github.com/x",
            "https:///x",
            "https://",
            "https://huggingface.co /x",
            "https://huggingface.co\n/x",
            "https://huggingface.cö/x",
            "",
        ] {
            assert!(!is_allowed_download_url(url), "should reject {url:?}");
        }
    }

    #[test]
    fn rejects_non_https_schemes() {
        for url in [
            "http://huggingface.co/x",
            "http://cdn-lfs.hf.co/x",
            "ftp://huggingface.co/x",
            "file:///C:/x",
            "javascript:alert(1)",
            "//huggingface.co/x",
            "/x",
            "huggingface.co/x",
        ] {
            assert!(!is_allowed_download_url(url), "should reject {url:?}");
        }
    }

    #[test]
    fn check_url_reports_only_the_host() {
        match check_url("https://evil.com/secret?sig=abc") {
            Err(ModelError::RedirectRefused(m)) => {
                assert!(m.contains("evil.com"));
                assert!(!m.contains("secret") && !m.contains("sig="));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn resolve_redirect_handles_each_form() {
        let cur = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/f.bin";
        // Absolute to the CDN.
        assert_eq!(
            resolve_redirect(cur, "https://cdn-lfs.hf.co/repos/a/b?sig=1").unwrap(),
            "https://cdn-lfs.hf.co/repos/a/b?sig=1"
        );
        // Absolute path on the same host.
        assert_eq!(
            resolve_redirect(cur, "/api/resolve-cache/models/x").unwrap(),
            "https://huggingface.co/api/resolve-cache/models/x"
        );
        // Same, from a URL with a query string and an explicit port.
        assert_eq!(
            resolve_redirect("https://cdn-lfs.hf.co:443?a=b", "/z").unwrap(),
            "https://cdn-lfs.hf.co:443/z"
        );
        // Scheme-relative to an allowed host.
        assert_eq!(
            resolve_redirect(cur, "//cdn-lfs.hf.co/x").unwrap(),
            "https://cdn-lfs.hf.co/x"
        );
    }

    #[test]
    fn resolve_redirect_refuses_bad_targets() {
        let cur = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/f.bin";
        for loc in [
            "https://evil.com/x",
            "http://cdn-lfs.hf.co/x",
            "//evil.com/x",
            "///evil.com/x",
            "/\\evil.com/x",
            "relative/path",
            "",
            "javascript:alert(1)",
            "https://huggingface.co.evil.com/x",
            "https://user:pw@cdn-lfs.hf.co/x",
            "data:text/plain,hi",
        ] {
            assert!(
                matches!(
                    resolve_redirect(cur, loc),
                    Err(ModelError::RedirectRefused(_))
                ),
                "should refuse {loc:?}"
            );
        }
    }

    // ---- streaming core: hash + size enforcement ----

    #[test]
    fn known_hash_vectors_match_the_hasher() {
        assert_eq!(sha_hex(ABC), ABC_SHA);
        assert_eq!(sha_hex(b""), EMPTY_SHA);
    }

    #[test]
    fn writes_verifies_and_renames() {
        let dir = TempDir::new();
        let mut calls = Vec::new();
        let path = write_verified(
            ABC,
            dir.path(),
            "m.bin",
            3,
            ABC_SHA,
            &mut |d, t| calls.push((d, t)),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(path, dir.path().join("m.bin"));
        assert_eq!(fs::read(&path).unwrap(), ABC);
        assert!(!part_path(dir.path(), "m.bin").exists());
        assert_eq!(calls.first(), Some(&(0, 3)));
        assert_eq!(calls.last(), Some(&(3, 3)));
    }

    #[test]
    fn creates_the_destination_directory() {
        let dir = TempDir::new();
        let nested = dir.path().join("a").join("b");
        let path = write_verified(
            ABC,
            &nested,
            "m.bin",
            3,
            ABC_SHA,
            &mut no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(fs::read(path).unwrap(), ABC);
    }

    #[test]
    fn multi_chunk_trickling_reader_verifies_and_reports_monotonic_progress() {
        let dir = TempDir::new();
        let data = pattern(3 * CHUNK + 12_345);
        let sha = sha_hex(&data);
        let mut seen: Vec<u64> = Vec::new();
        let reader = Trickle {
            data: &data,
            pos: 0,
            step: 10_007,
        };
        let path = write_verified(
            reader,
            dir.path(),
            "big.bin",
            data.len() as u64,
            &sha,
            &mut |d, t| {
                assert_eq!(t, data.len() as u64);
                seen.push(d);
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(fs::read(path).unwrap(), data);
        assert_eq!(seen.first(), Some(&0));
        assert_eq!(seen.last(), Some(&(data.len() as u64)));
        assert!(seen.windows(2).all(|w| w[0] <= w[1]));
        assert!(seen.len() > 4, "progress should be reported per chunk");
    }

    #[test]
    fn uppercase_expected_hash_is_accepted() {
        let dir = TempDir::new();
        write_verified(
            ABC,
            dir.path(),
            "m.bin",
            3,
            &ABC_SHA.to_uppercase(),
            &mut no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap();
    }

    #[test]
    fn hash_mismatch_deletes_part_and_keeps_old_file() {
        let dir = TempDir::new();
        fs::write(dir.path().join("m.bin"), b"old").unwrap();
        let err = write_verified(
            ABC,
            dir.path(),
            "m.bin",
            3,
            ZERO_SHA,
            &mut no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert_eq!(err, ModelError::HashMismatch);
        assert!(!part_path(dir.path(), "m.bin").exists());
        assert_eq!(fs::read(dir.path().join("m.bin")).unwrap(), b"old");
    }

    #[test]
    fn too_large_stream_aborts_after_one_extra_byte() {
        let dir = TempDir::new();
        let mut counting = Counting {
            inner: io::repeat(0xAB),
            total: 0,
        };
        let err = write_verified(
            &mut counting,
            dir.path(),
            "m.bin",
            1_000,
            ZERO_SHA,
            &mut no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert_eq!(err, ModelError::TooLarge { expected: 1_000 });
        assert_eq!(counting.total, 1_001, "must stop at expected+1 bytes");
        assert!(dir.is_empty(), "no .part or final file may remain");
    }

    #[test]
    fn exact_size_with_trailing_byte_is_too_large() {
        let dir = TempDir::new();
        let err = write_verified(
            &b"abcd"[..],
            dir.path(),
            "m.bin",
            3,
            ABC_SHA,
            &mut no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert_eq!(err, ModelError::TooLarge { expected: 3 });
        assert!(dir.is_empty());
    }

    #[test]
    fn short_stream_is_a_size_mismatch() {
        let dir = TempDir::new();
        let err = write_verified(
            &b"ab"[..],
            dir.path(),
            "m.bin",
            3,
            ABC_SHA,
            &mut no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert_eq!(
            err,
            ModelError::SizeMismatch {
                expected: 3,
                actual: 2
            }
        );
        assert!(dir.is_empty());
    }

    #[test]
    fn empty_stream_is_a_size_mismatch() {
        let dir = TempDir::new();
        let err = write_verified(
            io::empty(),
            dir.path(),
            "m.bin",
            3,
            ABC_SHA,
            &mut no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(matches!(err, ModelError::SizeMismatch { actual: 0, .. }));
        assert!(dir.is_empty());
    }

    #[test]
    fn read_error_mid_stream_deletes_part() {
        let dir = TempDir::new();
        let data = pattern(100_000);
        let err = write_verified(
            FailAfter {
                data: &data,
                pos: 0,
            },
            dir.path(),
            "m.bin",
            200_000,
            ZERO_SHA,
            &mut no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(matches!(err, ModelError::Request(_)), "{err:?}");
        assert!(dir.is_empty());
    }

    // ---- cancel ----

    #[test]
    fn cancel_mid_stream_deletes_part() {
        let dir = TempDir::new();
        let data = pattern(4 * CHUNK);
        let sha = sha_hex(&data);
        let cancel = AtomicBool::new(false);
        let mut saw_part = false;
        let part = part_path(dir.path(), "m.bin");
        let err = write_verified(
            &data[..],
            dir.path(),
            "m.bin",
            data.len() as u64,
            &sha,
            &mut |d, _| {
                if d >= CHUNK as u64 {
                    saw_part = part.exists();
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            &cancel,
        )
        .unwrap_err();
        assert_eq!(err, ModelError::Cancelled);
        assert!(saw_part, ".part must exist while the download runs");
        assert!(!part.exists(), ".part must be deleted on cancel");
        assert!(!dir.path().join("m.bin").exists());
        assert!(dir.is_empty());
    }

    #[test]
    fn cancel_before_start_creates_nothing() {
        let dir = TempDir::new();
        let nested = dir.path().join("sub");
        let err = write_verified(
            ABC,
            &nested,
            "m.bin",
            3,
            ABC_SHA,
            &mut no_progress(),
            &AtomicBool::new(true),
        )
        .unwrap_err();
        assert_eq!(err, ModelError::Cancelled);
        assert!(!nested.exists());
    }

    #[test]
    fn panicking_progress_callback_still_removes_part() {
        let dir = TempDir::new();
        let part = part_path(dir.path(), "m.bin");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = write_verified(
                ABC,
                dir.path(),
                "m.bin",
                3,
                ABC_SHA,
                &mut |d, _| {
                    if d > 0 {
                        panic!("ui went away");
                    }
                },
                &AtomicBool::new(false),
            );
        }));
        assert!(result.is_err());
        assert!(!part.exists());
    }

    #[test]
    fn replaces_an_existing_final_file_on_success() {
        let dir = TempDir::new();
        fs::write(dir.path().join("m.bin"), b"stale").unwrap();
        write_verified(
            ABC,
            dir.path(),
            "m.bin",
            3,
            ABC_SHA,
            &mut no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(fs::read(dir.path().join("m.bin")).unwrap(), ABC);
    }

    #[test]
    fn unsafe_file_names_are_rejected() {
        let dir = TempDir::new();
        for name in [
            "",
            "../evil.bin",
            "a/b.bin",
            "a\\b.bin",
            "C:evil.bin",
            "..",
            ".hidden",
            "a..b",
            "x y.bin",
        ] {
            let err = write_verified(
                ABC,
                dir.path(),
                name,
                3,
                ABC_SHA,
                &mut no_progress(),
                &AtomicBool::new(false),
            )
            .unwrap_err();
            assert!(matches!(err, ModelError::InvalidInput(_)), "{name:?}");
        }
        assert!(dir.is_empty());
    }

    // ---- status / delete / verify ----

    #[test]
    fn status_missing_partial_installed() {
        let dir = TempDir::new();
        let m = &ABC_MODEL;
        assert_eq!(model_status(m, dir.path()), ModelStatus::Missing);

        fs::write(part_path(dir.path(), m.file_name), b"ab").unwrap();
        assert_eq!(model_status(m, dir.path()), ModelStatus::Partial);

        fs::write(dir.path().join(m.file_name), ABC).unwrap();
        assert_eq!(
            model_status(m, dir.path()),
            ModelStatus::Installed {
                path: dir.path().join(m.file_name),
                bytes: 3
            }
        );
    }

    #[test]
    fn wrong_sized_final_file_is_partial() {
        let dir = TempDir::new();
        fs::write(dir.path().join(ABC_MODEL.file_name), b"abcd").unwrap();
        assert_eq!(model_status(&ABC_MODEL, dir.path()), ModelStatus::Partial);
    }

    #[test]
    fn status_of_a_missing_directory_is_missing() {
        let dir = TempDir::new();
        assert_eq!(
            model_status(&ABC_MODEL, &dir.path().join("nope")),
            ModelStatus::Missing
        );
    }

    #[test]
    fn status_serializes_with_a_state_tag() {
        let v = serde_json::to_value(ModelStatus::Missing).unwrap();
        assert_eq!(v["state"], "missing");
        let v = serde_json::to_value(ModelStatus::Installed {
            path: PathBuf::from("x"),
            bytes: 3,
        })
        .unwrap();
        assert_eq!(v["state"], "installed");
        assert_eq!(v["bytes"], 3);
    }

    #[test]
    fn delete_removes_file_and_part_and_is_idempotent() {
        let dir = TempDir::new();
        let m = &ABC_MODEL;
        fs::write(dir.path().join(m.file_name), ABC).unwrap();
        fs::write(part_path(dir.path(), m.file_name), b"a").unwrap();
        delete_model(m, dir.path()).unwrap();
        assert!(dir.is_empty());
        assert_eq!(model_status(m, dir.path()), ModelStatus::Missing);
        delete_model(m, dir.path()).unwrap();
    }

    #[test]
    fn delete_leaves_other_files_alone() {
        let dir = TempDir::new();
        fs::write(dir.path().join("other.bin"), b"keep").unwrap();
        fs::write(dir.path().join(ABC_MODEL.file_name), ABC).unwrap();
        delete_model(&ABC_MODEL, dir.path()).unwrap();
        assert_eq!(fs::read(dir.path().join("other.bin")).unwrap(), b"keep");
    }

    #[test]
    fn verify_accepts_the_right_bytes() {
        let dir = TempDir::new();
        fs::write(dir.path().join(ABC_MODEL.file_name), ABC).unwrap();
        verify_model(&ABC_MODEL, dir.path()).unwrap();
    }

    #[test]
    fn verify_detects_tampering_wrong_size_and_absence() {
        let dir = TempDir::new();
        let path = dir.path().join(ABC_MODEL.file_name);

        fs::write(&path, b"abd").unwrap();
        assert_eq!(
            verify_model(&ABC_MODEL, dir.path()),
            Err(ModelError::HashMismatch)
        );

        fs::write(&path, b"abcd").unwrap();
        assert_eq!(
            verify_model(&ABC_MODEL, dir.path()),
            Err(ModelError::SizeMismatch {
                expected: 3,
                actual: 4
            })
        );

        fs::remove_file(&path).unwrap();
        assert!(matches!(
            verify_model(&ABC_MODEL, dir.path()),
            Err(ModelError::Io(_))
        ));
    }

    #[test]
    fn verify_after_write_verified_round_trips() {
        let dir = TempDir::new();
        let data = pattern(2 * CHUNK + 1);
        let sha = sha_hex(&data);
        write_verified(
            &data[..],
            dir.path(),
            "rt.bin",
            data.len() as u64,
            &sha,
            &mut no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap();
        // `verify_model` takes a catalog-shaped model; build one for this file.
        let sha_static: &'static str = Box::leak(sha.into_boxed_str());
        let m = WhisperModel {
            id: "rt",
            file_name: "rt.bin",
            display_name: "rt",
            multilingual: true,
            size_bytes: data.len() as u64,
            sha256: sha_static,
            license: MODEL_LICENSE,
        };
        verify_model(&m, dir.path()).unwrap();
        assert!(matches!(
            model_status(&m, dir.path()),
            ModelStatus::Installed { .. }
        ));
    }

    // ---- download_model entry checks (no socket in any build) ----

    #[test]
    fn download_refuses_models_that_are_not_catalog_entries() {
        let dir = TempDir::new();
        // Unknown id.
        let err = download_model(
            &ABC_MODEL,
            dir.path(),
            no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(matches!(err, ModelError::InvalidInput(_)), "{err:?}");
        // Known id but a tampered hash / size / file name.
        let mut forged = *find_model("base").unwrap();
        forged.sha256 = ZERO_SHA;
        assert!(matches!(
            download_model(&forged, dir.path(), no_progress(), &AtomicBool::new(false)),
            Err(ModelError::InvalidInput(_))
        ));
        let mut forged = *find_model("base").unwrap();
        forged.file_name = "../base.bin";
        assert!(matches!(
            download_model(&forged, dir.path(), no_progress(), &AtomicBool::new(false)),
            Err(ModelError::InvalidInput(_))
        ));
        assert!(dir.is_empty());
    }

    #[test]
    fn download_with_cancel_already_set_does_nothing() {
        let dir = TempDir::new();
        let mut called = false;
        let err = download_model(
            find_model("tiny").unwrap(),
            dir.path(),
            |_, _| called = true,
            &AtomicBool::new(true),
        )
        .unwrap_err();
        assert_eq!(err, ModelError::Cancelled);
        assert!(!called);
        assert!(dir.is_empty());
    }

    #[cfg(not(feature = "net"))]
    #[test]
    fn download_is_disabled_without_the_net_feature() {
        let dir = TempDir::new();
        let err = download_model(
            find_model("tiny").unwrap(),
            dir.path(),
            no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert_eq!(err, ModelError::Disabled);
        assert!(dir.is_empty());
    }

    #[test]
    fn error_messages_are_human_readable() {
        assert!(ModelError::HashMismatch.to_string().contains("SHA-256"));
        assert!(ModelError::Cancelled.to_string().contains("cancel"));
        assert!(ModelError::Http(404).to_string().contains("404"));
        assert!(ModelError::TooManyRedirects
            .to_string()
            .contains("redirect"));
        assert!(ModelError::SizeMismatch {
            expected: 2,
            actual: 1
        }
        .to_string()
        .contains("expected 2"));
    }
}
