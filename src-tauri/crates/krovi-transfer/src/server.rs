//! Request handling. Every request lands in `handle`, which maps it through the pure router
//! (`route::parse`) and answers one of:
//!
//!   * the same plain 404 for anything unknown, any wrong/expired token, any wrong kind of
//!     token (a send token on a receive route and vice versa);
//!   * 405 for a wrong method on a route whose token IS valid;
//!   * the page / file / upload result.
//!
//! Files are streamed from disk through a 256 KB buffer (never loaded into memory); uploads
//! are streamed to a `.part` file and renamed when complete. Nothing is logged.

use crate::events::{CloseReason, Direction, Event, Throttle};
use crate::manager::Inner;
use crate::names::{content_disposition, reserve_unique, sanitize_filename};
use crate::pages;
use crate::range::{parse_range, RangeSpec};
use crate::route::{self, Route};
use crate::share::{FileEntry, Mode, PinCheck, ShareState};
use serde_json::json;
use std::fs::File;
use std::io::{self, BufReader, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tiny_http::{Header, Method, Request, Response, StatusCode};

const FILE_BUF: usize = 256 * 1024;
const UPLOAD_BUF: usize = 128 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Verb {
    Get,
    Head,
    Post,
    Other,
}

type Body = Response<Cursor<Vec<u8>>>;

const SECURITY: [(&str, &str); 7] = [
    ("Cache-Control", "no-store"),
    ("X-Content-Type-Options", "nosniff"),
    ("Referrer-Policy", "no-referrer"),
    ("X-Frame-Options", "DENY"),
    ("Cross-Origin-Resource-Policy", "same-origin"),
    (
        "Content-Security-Policy",
        "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
    ),
    ("Server", "Krovi"),
];

fn hdr(name: &str, value: &str) -> Option<Header> {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).ok()
}

/// Security headers on every response, and Content-Length framing (never chunked) so that
/// phones can show real download progress and resume.
fn secure<R: Read>(mut r: Response<R>) -> Response<R> {
    for (k, v) in SECURITY {
        if let Some(h) = hdr(k, v) {
            r.add_header(h);
        }
    }
    r.with_chunked_threshold(usize::MAX)
}

fn text(status: u16, ctype: &str, body: impl Into<Vec<u8>>) -> Body {
    let body: Vec<u8> = body.into();
    let len = body.len();
    let mut r = Response::new(
        StatusCode(status),
        vec![],
        Cursor::new(body),
        Some(len),
        None,
    );
    if let Some(h) = hdr("Content-Type", ctype) {
        r.add_header(h);
    }
    secure(r)
}

fn html(status: u16, body: String) -> Body {
    text(status, "text/html; charset=utf-8", body)
}

fn json(status: u16, v: serde_json::Value) -> Body {
    text(status, "application/json", v.to_string())
}

/// The one answer for everything unknown.
pub(crate) fn not_found() -> Body {
    text(404, "text/plain; charset=utf-8", pages::NOT_FOUND_BODY)
}

pub(crate) fn busy() -> Body {
    text(503, "text/plain; charset=utf-8", "Busy")
}

fn method_not_allowed(allow: &str) -> Body {
    let mut r = text(405, "text/plain; charset=utf-8", "Method not allowed");
    if let Some(h) = hdr("Allow", allow) {
        r.add_header(h);
    }
    r
}

fn header_value<'a>(req: &'a Request, name: &'static str) -> Option<&'a str> {
    req.headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str())
}

fn kind(mode: Mode) -> &'static str {
    if mode == Mode::Send {
        "s"
    } else {
        "r"
    }
}

pub(crate) fn handle(inner: &Arc<Inner>, req: Request) {
    let verb = match req.method() {
        Method::Get => Verb::Get,
        Method::Head => Verb::Head,
        Method::Post => Verb::Post,
        _ => Verb::Other,
    };
    let target = req.url().to_string();
    match route::parse(&target) {
        Route::NotFound => {
            let _ = req.respond(not_found());
        }
        Route::SendPage { token, query } => page(inner, req, verb, token, query, Mode::Send),
        Route::ReceivePage { token, query } => page(inner, req, verb, token, query, Mode::Receive),
        Route::SendFile { token, index } => send_file(inner, req, verb, token, index),
        Route::ReceiveUpload { token, query } => upload(inner, req, verb, token, query),
    }
}

/// Look up a live share by token and note that a phone reached us.
fn open(inner: &Arc<Inner>, token: &str, mode: Mode) -> Option<Arc<ShareState>> {
    let s = inner.find_share(token, mode)?;
    if !s.reached.swap(true, Ordering::SeqCst) {
        inner.emit(Event::Reached { id: s.id.clone() });
    }
    Some(s)
}

/// The PIN gate. `None` = access granted. `Some(response)` = send this instead.
fn pin_gate(
    inner: &Arc<Inner>,
    share: &Arc<ShareState>,
    req: &Request,
    base: &str,
    query: &str,
    is_page: bool,
) -> Option<Body> {
    if !share.needs_pin() {
        return None;
    }
    if let Some(cookie) = header_value(req, "Cookie") {
        if let Some(v) = route::cookie_get(cookie, &share.cookie_name()) {
            if share.session_ok(v) {
                return None;
            }
        }
    }
    if !is_page {
        return Some(text(403, "text/plain; charset=utf-8", "PIN required"));
    }
    match route::query_get(query, "pin") {
        None => Some(html(200, pages::pin_page(base, None))),
        Some(p) => match share.check_pin(p.trim()) {
            PinCheck::Ok(key) => {
                let left = share.secs_left(Instant::now()).max(1);
                let cookie = format!(
                    "{}={key}; Path={base}; HttpOnly; SameSite=Strict; Max-Age={left}",
                    share.cookie_name()
                );
                let mut r = Response::new(
                    StatusCode(303),
                    vec![],
                    Cursor::new(Vec::new()),
                    Some(0),
                    None,
                );
                for h in [hdr("Location", base), hdr("Set-Cookie", &cookie)]
                    .into_iter()
                    .flatten()
                {
                    r.add_header(h);
                }
                Some(secure(r))
            }
            PinCheck::Wrong { left } => Some(html(
                403,
                pages::pin_page(
                    base,
                    Some(&format!(
                        "Wrong PIN. {left} {} left.",
                        if left == 1 { "try" } else { "tries" }
                    )),
                ),
            )),
            PinCheck::Locked => {
                inner.close(&share.id, CloseReason::Locked);
                Some(text(
                    429,
                    "text/plain; charset=utf-8",
                    "Too many wrong PINs. This link is closed.",
                ))
            }
        },
    }
}

fn page(inner: &Arc<Inner>, req: Request, verb: Verb, token: &str, query: &str, mode: Mode) {
    let Some(share) = open(inner, token, mode) else {
        let _ = req.respond(not_found());
        return;
    };
    if !matches!(verb, Verb::Get | Verb::Head) {
        let _ = req.respond(method_not_allowed("GET, HEAD"));
        return;
    }
    let base = format!("/{}/{}", kind(mode), share.token);
    if let Some(resp) = pin_gate(inner, &share, &req, &base, query, true) {
        let _ = req.respond(resp);
        return;
    }
    let left = share.secs_left(Instant::now());
    let body = match mode {
        Mode::Send => {
            let list: Vec<(String, u64)> = share
                .files
                .iter()
                .map(|f| (f.name.clone(), f.size))
                .collect();
            pages::send_page(&share.token, &list, left)
        }
        Mode::Receive => pages::receive_page(&share.token, share.max_bytes, left),
    };
    let _ = req.respond(html(200, body));
}

// ---------------------------------------------------------------------------
// Downloads
// ---------------------------------------------------------------------------

struct ProgressReader<R> {
    inner: R,
    sent: Arc<AtomicU64>,
    base: u64,
    total: u64,
    file: String,
    share: Arc<ShareState>,
    hub: Arc<Inner>,
    throttle: Throttle,
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.share.cancel.load(Ordering::Relaxed) {
            // ConnectionAborted is what tiny_http treats as "client went away": quiet stop.
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "share closed",
            ));
        }
        let n = self.inner.read(buf)?;
        if n > 0 {
            let s = self.sent.fetch_add(n as u64, Ordering::Relaxed) + n as u64;
            if self.throttle.ready(Instant::now()) {
                self.hub.emit(Event::Progress {
                    id: self.share.id.clone(),
                    file: self.file.clone(),
                    bytes: self.base + s,
                    total: self.total,
                    direction: Direction::Send,
                });
            }
        }
        Ok(n)
    }
}

fn send_file(inner: &Arc<Inner>, req: Request, verb: Verb, token: &str, index: usize) {
    let Some(share) = open(inner, token, Mode::Send) else {
        let _ = req.respond(not_found());
        return;
    };
    if !matches!(verb, Verb::Get | Verb::Head) {
        let _ = req.respond(method_not_allowed("GET, HEAD"));
        return;
    }
    let base_path = format!("/s/{}", share.token);
    if let Some(resp) = pin_gate(inner, &share, &req, &base_path, "", false) {
        let _ = req.respond(resp);
        return;
    }
    let Some(sf) = share.files.get(index) else {
        let _ = req.respond(not_found());
        return;
    };
    let mut file = match File::open(&sf.path) {
        Ok(f) => f,
        Err(e) => {
            inner.emit(Event::Failed {
                id: share.id.clone(),
                file: sf.name.clone(),
                direction: Direction::Send,
                message: format!("Could not open the file: {e}"),
            });
            let _ = req.respond(text(
                404,
                "text/plain; charset=utf-8",
                "File is no longer available",
            ));
            return;
        }
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(sf.size);
    let spec = parse_range(header_value(&req, "Range"), len);
    let (status, start, end) = match spec {
        RangeSpec::Full => (200u16, 0u64, len.saturating_sub(1)),
        RangeSpec::Partial { start, end } => (206, start, end),
        RangeSpec::Unsatisfiable => {
            let mut r = text(416, "text/plain; charset=utf-8", "Range not satisfiable");
            if let Some(h) = hdr("Content-Range", &format!("bytes */{len}")) {
                r.add_header(h);
            }
            let _ = req.respond(r);
            return;
        }
    };
    let remaining = if len == 0 { 0 } else { end - start + 1 };
    if start > 0 && file.seek(SeekFrom::Start(start)).is_err() {
        let _ = req.respond(text(500, "text/plain; charset=utf-8", "Read error"));
        return;
    }

    let sent = Arc::new(AtomicU64::new(0));
    let reader = ProgressReader {
        inner: BufReader::with_capacity(FILE_BUF, file.take(remaining)),
        sent: sent.clone(),
        base: start,
        total: len,
        file: sf.name.clone(),
        share: share.clone(),
        hub: inner.clone(),
        throttle: Throttle::per_100ms(),
    };
    let mut headers: Vec<Header> = [
        hdr("Content-Type", "application/octet-stream"),
        hdr("Content-Disposition", &content_disposition(&sf.name)),
        hdr("Accept-Ranges", "bytes"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if status == 206 {
        headers.extend(hdr("Content-Range", &format!("bytes {start}-{end}/{len}")));
    }
    let resp = secure(Response::new(
        StatusCode(status),
        headers,
        reader,
        Some(remaining as usize),
        None,
    ));
    let res = req.respond(resp);

    let delivered = sent.load(Ordering::Relaxed);
    let finished_file = len == 0 || end + 1 == len;
    if verb == Verb::Get && res.is_ok() && delivered == remaining && finished_file {
        share.downloads.fetch_add(1, Ordering::Relaxed);
        inner.emit(Event::Done {
            id: share.id.clone(),
            file: sf.name.clone(),
            direction: Direction::Send,
            size: len,
            saved_path: None,
        });
    }
}

// ---------------------------------------------------------------------------
// Uploads
// ---------------------------------------------------------------------------

enum UpErr {
    TooLarge,
    Cancelled,
    Incomplete,
    Io(String),
}

fn stream_to_file(
    hub: &Arc<Inner>,
    share: &Arc<ShareState>,
    reader: &mut dyn Read,
    file: &mut File,
    name: &str,
    declared: Option<u64>,
) -> Result<u64, UpErr> {
    let mut buf = vec![0u8; UPLOAD_BUF];
    let mut written = 0u64;
    let mut throttle = Throttle::per_100ms();
    let rollback = |w: u64| {
        share.received_bytes.fetch_sub(w, Ordering::SeqCst);
    };
    loop {
        if share.cancel.load(Ordering::Relaxed) {
            rollback(written);
            return Err(UpErr::Cancelled);
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                rollback(written);
                return Err(UpErr::Io(e.to_string()));
            }
        };
        let total_now = share.received_bytes.fetch_add(n as u64, Ordering::SeqCst) + n as u64;
        written += n as u64;
        if total_now > share.max_bytes {
            rollback(written);
            return Err(UpErr::TooLarge);
        }
        if let Err(e) = file.write_all(&buf[..n]) {
            rollback(written);
            return Err(UpErr::Io(e.to_string()));
        }
        if throttle.ready(Instant::now()) {
            hub.emit(Event::Progress {
                id: share.id.clone(),
                file: name.to_string(),
                bytes: written,
                total: declared.unwrap_or(0),
                direction: Direction::Receive,
            });
        }
    }
    if let Some(d) = declared {
        if written != d {
            rollback(written);
            return Err(UpErr::Incomplete);
        }
    }
    if let Err(e) = file.flush() {
        rollback(written);
        return Err(UpErr::Io(e.to_string()));
    }
    Ok(written)
}

fn upload(inner: &Arc<Inner>, mut req: Request, verb: Verb, token: &str, query: &str) {
    let Some(share) = open(inner, token, Mode::Receive) else {
        let _ = req.respond(not_found());
        return;
    };
    if verb != Verb::Post {
        let _ = req.respond(method_not_allowed("POST"));
        return;
    }
    let base_path = format!("/r/{}", share.token);
    if let Some(resp) = pin_gate(inner, &share, &req, &base_path, "", false) {
        let _ = req.respond(resp);
        return;
    }

    let name = sanitize_filename(&route::query_get(query, "n").unwrap_or_default());
    let declared = req.body_length().map(|n| n as u64);
    let chunked = header_value(&req, "Transfer-Encoding").is_some();
    if declared.is_none() && !chunked {
        let _ = req.respond(text(411, "text/plain; charset=utf-8", "Length required"));
        return;
    }
    let fail = |inner: &Arc<Inner>, message: &str| {
        inner.emit(Event::Failed {
            id: share.id.clone(),
            file: name.clone(),
            direction: Direction::Receive,
            message: message.to_string(),
        });
    };
    if let Some(d) = declared {
        let already = share.received_bytes.load(Ordering::SeqCst);
        if d > share.max_bytes || already.saturating_add(d) > share.max_bytes {
            fail(inner, "Larger than the upload limit");
            let _ = req.respond(json(413, json!({ "ok": false, "error": "too large" })));
            return;
        }
    }
    if let Err(e) = std::fs::create_dir_all(&share.save_dir) {
        fail(inner, &format!("Cannot create the receive folder: {e}"));
        let _ = req.respond(json(500, json!({ "ok": false, "error": "cannot save" })));
        return;
    }
    let reservation = match reserve_unique(&share.save_dir, &name) {
        Ok(r) => r,
        Err(e) => {
            fail(inner, &format!("Cannot create the file: {e}"));
            let _ = req.respond(json(500, json!({ "ok": false, "error": "cannot save" })));
            return;
        }
    };
    let mut file = reservation.file;
    let outcome = stream_to_file(inner, &share, req.as_reader(), &mut file, &name, declared);
    drop(file);

    let cleanup = || {
        let _ = std::fs::remove_file(&reservation.part_path);
    };
    match outcome {
        Ok(written) => {
            if let Err(e) = std::fs::rename(&reservation.part_path, &reservation.final_path) {
                cleanup();
                share.received_bytes.fetch_sub(written, Ordering::SeqCst);
                fail(inner, &format!("Cannot finish saving: {e}"));
                let _ = req.respond(json(500, json!({ "ok": false, "error": "cannot save" })));
                return;
            }
            mark_of_the_web(&reservation.final_path);
            share.uploads.fetch_add(1, Ordering::Relaxed);
            share.received.lock().unwrap().push(FileEntry {
                name: reservation.final_name.clone(),
                size: written,
            });
            inner.emit(Event::Done {
                id: share.id.clone(),
                file: reservation.final_name.clone(),
                direction: Direction::Receive,
                size: written,
                saved_path: Some(reservation.final_path.to_string_lossy().into_owned()),
            });
            let _ = req.respond(json(
                200,
                json!({ "ok": true, "name": reservation.final_name, "size": written }),
            ));
        }
        Err(e) => {
            cleanup();
            let (status, msg) = match e {
                UpErr::TooLarge => (413, "Larger than the upload limit".to_string()),
                UpErr::Cancelled => (410, "The share was closed".to_string()),
                UpErr::Incomplete => (400, "The upload was interrupted".to_string()),
                UpErr::Io(m) => (500, format!("Could not save the file: {m}")),
            };
            fail(inner, &msg);
            let _ = req.respond(json(status, json!({ "ok": false, "error": msg })));
        }
    }
}

/// Files that arrive from another device are untrusted. Tag them as coming from the
/// Internet zone (Zone.Identifier alternate data stream) so SmartScreen and Office's
/// Protected View treat them like any downloaded file. Best effort (non-NTFS: no-op).
#[cfg(windows)]
fn mark_of_the_web(path: &Path) {
    let mut p = path.as_os_str().to_owned();
    p.push(":Zone.Identifier");
    let _ = std::fs::write(
        std::path::PathBuf::from(p),
        b"[ZoneTransfer]\r\nZoneId=3\r\n",
    );
}

#[cfg(not(windows))]
fn mark_of_the_web(_path: &Path) {}
