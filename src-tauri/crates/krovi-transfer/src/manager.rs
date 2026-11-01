//! The share registry and the server lifecycle.
//!
//! One HTTP server exists only while at least one share is open. It is bound to a single
//! private LAN IPv4 address and a random high port, and is torn down (listener closed) the
//! moment the last share closes (stop, expiry, lockout or app quit).

use crate::error::TransferError;
use crate::events::{CloseReason, Event, EventSink};
use crate::names::sanitize_filename;
use crate::share::{FileEntry, Mode, ShareInfo, ShareState};
use crate::{net, qr, server, token};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const DEFAULT_TTL: Duration = Duration::from_secs(10 * 60);
pub const DEFAULT_MAX_UPLOAD: u64 = 2 * 1024 * 1024 * 1024;
const MAX_FILES_PER_SHARE: usize = 500;
/// Concurrent request handlers; beyond this the server answers 503 instead of spawning threads.
const MAX_HANDLERS: usize = 48;
const PORT_LO: u16 = 49152;
const PORT_HI: u16 = 65534;

#[derive(Debug, Clone)]
pub struct Config {
    /// Where phone uploads land. Default: `%USERPROFILE%\Downloads\Krovi`.
    pub receive_dir: PathBuf,
    /// Cap per uploaded file AND total bytes accepted by one receive share.
    pub max_upload_bytes: u64,
    pub ttl: Duration,
    pub require_pin: bool,
    /// Tests and the example only: listen on this address instead of the detected LAN
    /// interface. `0.0.0.0` and multicast are always refused.
    pub lan_override: Option<Ipv4Addr>,
}

impl Config {
    pub fn default_receive_dir() -> PathBuf {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Downloads")
            .join("Krovi")
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            receive_dir: Self::default_receive_dir(),
            max_upload_bytes: DEFAULT_MAX_UPLOAD,
            ttl: DEFAULT_TTL,
            require_pin: false,
            lan_override: None,
        }
    }
}

/// Per-share overrides of the configured defaults.
#[derive(Debug, Clone, Default)]
pub struct ShareOptions {
    pub pin: Option<bool>,
    pub ttl_secs: Option<u64>,
}

pub(crate) struct Running {
    pub server: Arc<tiny_http::Server>,
    pub stop: Arc<AtomicBool>,
    pub ip: Ipv4Addr,
    pub port: u16,
    handle: Option<JoinHandle<()>>,
}

impl Running {
    /// Close the listener and wait (briefly) for the dispatcher to finish.
    fn shutdown(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.server.unblock();
        let handle = self.handle.take();
        drop(self);
        if let Some(h) = handle {
            if h.thread().id() != thread::current().id() {
                let _ = h.join();
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct Registry {
    pub shares: Vec<Arc<ShareState>>,
    pub server: Option<Running>,
}

pub(crate) struct Inner {
    pub cfg: Mutex<Config>,
    pub reg: Mutex<Registry>,
    pub sink: Arc<dyn EventSink>,
    pub inflight: AtomicUsize,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Inner {
    pub fn emit(&self, e: Event) {
        self.sink.emit(e);
    }

    /// Constant-work token lookup across all open shares of the given mode.
    pub fn find_share(&self, tok: &str, mode: Mode) -> Option<Arc<ShareState>> {
        let reg = self.reg.lock().unwrap();
        let now = Instant::now();
        let mut found = None;
        for s in &reg.shares {
            let hit = token::ct_eq(s.token.as_bytes(), tok.as_bytes());
            if hit && s.mode == mode && !s.expired(now) {
                found = Some(s.clone());
            }
        }
        found
    }

    /// Close one share. Returns false if it was not open.
    pub fn close(&self, id: &str, reason: CloseReason) -> bool {
        let (share, running) = {
            let mut reg = self.reg.lock().unwrap();
            let Some(pos) = reg.shares.iter().position(|s| s.id == id) else {
                return false;
            };
            let share = reg.shares.remove(pos);
            let running = if reg.shares.is_empty() {
                reg.server.take()
            } else {
                None
            };
            (share, running)
        };
        // Expiry lets a transfer that is already running finish; everything else aborts it.
        if reason != CloseReason::Expired {
            share.cancel.store(true, Ordering::SeqCst);
        }
        self.emit(Event::Closed {
            id: share.id.clone(),
            reason,
        });
        if let Some(r) = running {
            r.shutdown();
        }
        true
    }

    pub fn reap(&self) {
        let now = Instant::now();
        let expired: Vec<String> = {
            let reg = self.reg.lock().unwrap();
            reg.shares
                .iter()
                .filter(|s| s.expired(now))
                .map(|s| s.id.clone())
                .collect()
        };
        for id in expired {
            self.close(&id, CloseReason::Expired);
        }
    }

    fn close_all(&self, reason: CloseReason) {
        let ids: Vec<String> = self
            .reg
            .lock()
            .unwrap()
            .shares
            .iter()
            .map(|s| s.id.clone())
            .collect();
        for id in ids {
            self.close(&id, reason);
        }
    }
}

struct InflightGuard(Arc<Inner>);
impl Drop for InflightGuard {
    fn drop(&mut self) {
        self.0.inflight.fetch_sub(1, Ordering::SeqCst);
    }
}

fn dispatch(inner: Arc<Inner>, srv: Arc<tiny_http::Server>, stop: Arc<AtomicBool>) {
    let mut last_reap = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        match srv.recv_timeout(Duration::from_millis(100)) {
            Ok(Some(req)) => {
                if inner.inflight.fetch_add(1, Ordering::SeqCst) >= MAX_HANDLERS {
                    inner.inflight.fetch_sub(1, Ordering::SeqCst);
                    let _ = req.respond(server::busy());
                    continue;
                }
                let guard = InflightGuard(inner.clone());
                let inner2 = inner.clone();
                // If the thread cannot be spawned the closure (and the Request) is dropped,
                // which makes tiny_http answer 500 by itself.
                let _ = thread::Builder::new()
                    .name("krovi-transfer-req".into())
                    .spawn(move || {
                        let _g = guard;
                        server::handle(&inner2, req);
                    });
            }
            Ok(None) => {}
            Err(_) => break,
        }
        if last_reap.elapsed() >= Duration::from_millis(250) {
            inner.reap();
            last_reap = Instant::now();
        }
    }
    // Anything that raced in after the stop signal gets the same 404 as everything unknown.
    while let Ok(Some(req)) = srv.try_recv() {
        let _ = req.respond(server::not_found());
    }
}

fn bind(ip: Ipv4Addr) -> Result<(tiny_http::Server, u16), TransferError> {
    for _ in 0..24 {
        let port = token::rand_in(PORT_LO, PORT_HI)?;
        if let Ok(s) = tiny_http::Server::http(SocketAddr::V4(SocketAddrV4::new(ip, port))) {
            return Ok((s, port));
        }
    }
    // Extremely unlikely: 24 random high ports were all taken. Let the OS pick one.
    let s = tiny_http::Server::http(SocketAddr::V4(SocketAddrV4::new(ip, 0)))
        .map_err(|e| TransferError::Bind(e.to_string()))?;
    let port = s
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .ok_or_else(|| TransferError::Bind("no local port".into()))?;
    Ok((s, port))
}

fn validate_bind(ip: Ipv4Addr) -> Result<(), TransferError> {
    if ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast() {
        Err(TransferError::BadBind(ip))
    } else {
        Ok(())
    }
}

pub struct TransferManager {
    inner: Arc<Inner>,
}

impl TransferManager {
    pub fn new(cfg: Config, sink: Arc<dyn EventSink>) -> Self {
        Self {
            inner: Arc::new(Inner {
                cfg: Mutex::new(cfg),
                reg: Mutex::new(Registry::default()),
                sink,
                inflight: AtomicUsize::new(0),
            }),
        }
    }

    pub fn config(&self) -> Config {
        self.inner.cfg.lock().unwrap().clone()
    }

    pub fn set_config(&self, cfg: Config) {
        *self.inner.cfg.lock().unwrap() = cfg;
    }

    /// Share local files. Folders are rejected (v1 shares files only).
    pub fn share_files(
        &self,
        paths: &[PathBuf],
        opts: ShareOptions,
    ) -> Result<ShareInfo, TransferError> {
        if paths.is_empty() {
            return Err(TransferError::NoFiles);
        }
        if paths.len() > MAX_FILES_PER_SHARE {
            return Err(TransferError::BadFile(format!(
                "Too many files (limit {MAX_FILES_PER_SHARE})."
            )));
        }
        let mut files: Vec<crate::share::SendFile> = Vec::new();
        for p in paths {
            let meta = std::fs::metadata(p)
                .map_err(|e| TransferError::BadFile(format!("Cannot read {}: {e}", display(p))))?;
            if meta.is_dir() {
                return Err(TransferError::BadFile(format!(
                    "{} is a folder. Quick Transfer shares files; select the files inside it.",
                    display(p)
                )));
            }
            if !meta.is_file() {
                return Err(TransferError::BadFile(format!(
                    "{} is not a regular file.",
                    display(p)
                )));
            }
            let path = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
            if files.iter().any(|f| f.path == path) {
                continue;
            }
            let raw = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            files.push(crate::share::SendFile {
                name: sanitize_filename(&raw),
                path,
                size: meta.len(),
            });
        }
        self.create_share(Mode::Send, files, opts)
    }

    /// Open an upload-only share (the phone sends files here).
    pub fn receive(&self, opts: ShareOptions) -> Result<ShareInfo, TransferError> {
        let dir = self.config().receive_dir;
        std::fs::create_dir_all(&dir)
            .map_err(|e| TransferError::Io(format!("Cannot create {}: {e}", display(&dir))))?;
        self.create_share(Mode::Receive, vec![], opts)
    }

    fn create_share(
        &self,
        mode: Mode,
        files: Vec<crate::share::SendFile>,
        opts: ShareOptions,
    ) -> Result<ShareInfo, TransferError> {
        let inner = &self.inner;
        let cfg = self.config();

        let ip = match cfg.lan_override {
            Some(ip) => {
                validate_bind(ip)?;
                ip
            }
            None => net::detect().ok_or(TransferError::NoLan)?.ip,
        };

        let ttl = opts
            .ttl_secs
            .map(Duration::from_secs)
            .unwrap_or(cfg.ttl)
            .clamp(Duration::from_secs(1), Duration::from_secs(24 * 3600));
        let pin = if opts.pin.unwrap_or(cfg.require_pin) {
            Some(token::pin()?)
        } else {
            None
        };
        let tok = token::token()?;
        let id = token::share_id()?;

        let mut reg = inner.reg.lock().unwrap();
        let started_here = reg.server.is_none();
        let port = match &reg.server {
            Some(r) if r.ip == ip => r.port,
            Some(r) => return Err(TransferError::NetworkChanged { had: r.ip, now: ip }),
            None => {
                let (srv, port) = bind(ip)?;
                let srv = Arc::new(srv);
                let stop = Arc::new(AtomicBool::new(false));
                let handle = {
                    let (i, s, st) = (inner.clone(), srv.clone(), stop.clone());
                    thread::Builder::new()
                        .name("krovi-transfer-http".into())
                        .spawn(move || dispatch(i, s, st))
                        .map_err(|e| TransferError::Bind(e.to_string()))?
                };
                reg.server = Some(Running {
                    server: srv,
                    stop,
                    ip,
                    port,
                    handle: Some(handle),
                });
                port
            }
        };

        let kind = if mode == Mode::Send { "s" } else { "r" };
        let url = format!("http://{ip}:{port}/{kind}/{tok}");
        let Some(qr_svg) = qr::svg_for(&url) else {
            let orphan = if started_here {
                reg.server.take()
            } else {
                None
            };
            drop(reg);
            if let Some(r) = orphan {
                r.shutdown();
            }
            return Err(TransferError::Io("Could not build the QR code.".into()));
        };

        let created_ms = now_ms();
        let state = Arc::new(ShareState {
            id,
            token: tok,
            mode,
            files,
            received: Mutex::new(Vec::<FileEntry>::new()),
            url,
            qr_svg,
            lan_ip: ip,
            port,
            created_ms,
            expires_ms: created_ms + ttl.as_millis() as u64,
            expires: Instant::now() + ttl,
            pin,
            pin_state: Default::default(),
            downloads: AtomicU32::new(0),
            uploads: AtomicU32::new(0),
            reached: AtomicBool::new(false),
            received_bytes: AtomicU64::new(0),
            max_bytes: cfg.max_upload_bytes,
            save_dir: cfg.receive_dir.clone(),
            cancel: AtomicBool::new(false),
        });
        let info = state.info();
        reg.shares.push(state);
        Ok(info)
    }

    pub fn status(&self) -> Vec<ShareInfo> {
        self.inner
            .reg
            .lock()
            .unwrap()
            .shares
            .iter()
            .map(|s| s.info())
            .collect()
    }

    /// The address and port the server currently listens on, if any (diagnostics and tests).
    pub fn listening(&self) -> Option<(Ipv4Addr, u16)> {
        self.inner
            .reg
            .lock()
            .unwrap()
            .server
            .as_ref()
            .map(|r| (r.ip, r.port))
    }

    /// Stop one share (`Some(id)`) or all of them (`None`). Returns how many were closed.
    pub fn stop(&self, id: Option<&str>) -> usize {
        match id {
            Some(id) => usize::from(self.inner.close(id, CloseReason::Stopped)),
            None => {
                let n = self.inner.reg.lock().unwrap().shares.len();
                self.inner.close_all(CloseReason::Stopped);
                n
            }
        }
    }

    /// App quit: close everything, abort running transfers, close the listener.
    pub fn shutdown(&self) {
        self.inner.close_all(CloseReason::Quit);
    }
}

impl Drop for TransferManager {
    fn drop(&mut self) {
        self.inner.close_all(CloseReason::Quit);
    }
}

fn display(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string_lossy().into_owned())
}
