//! Per-share state and the `Share` wire shape (docs/CONTRACTS.md section 6).

use crate::events::Direction;
use crate::token;
use serde::Serialize;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// Wrong PIN attempts allowed before the share is closed.
pub const MAX_PIN_FAILURES: u32 = 5;
/// Verified PIN sessions kept per share (oldest dropped).
const MAX_SESSIONS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Send,
    Receive,
}

impl Mode {
    pub fn direction(self) -> Direction {
        match self {
            Mode::Send => Direction::Send,
            Mode::Receive => Direction::Receive,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub size: u64,
}

/// What the UI receives. Fields up to `port` are the contract; the rest are additive.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareInfo {
    pub id: String,
    pub url: String,
    pub qr_svg: String,
    /// Send: the shared files. Receive: the files received so far (saved names).
    pub files: Vec<FileEntry>,
    pub mode: Mode,
    /// Epoch milliseconds.
    pub expires_at: u64,
    pub downloads: u32,
    pub lan_ip: String,
    pub port: u16,
    // ---- additive fields ----
    pub created_at: u64,
    pub uploads: u32,
    /// A phone has made at least one valid request to this share.
    pub reached: bool,
    /// The 4-digit PIN to show on the desktop, when one is required.
    pub pin: Option<String>,
    /// Receive: per-file and per-share cap in bytes.
    pub max_upload_bytes: u64,
    pub received_bytes: u64,
    /// Receive: the folder files land in.
    pub save_dir: Option<String>,
}

pub(crate) struct SendFile {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Default)]
pub(crate) struct PinState {
    failures: u32,
    sessions: Vec<String>,
}

pub enum PinCheck {
    /// Correct: hand this session value to the browser as a cookie.
    Ok(String),
    Wrong {
        left: u32,
    },
    /// Too many failures: the share must be closed.
    Locked,
}

pub(crate) struct ShareState {
    pub id: String,
    pub token: String,
    pub mode: Mode,
    pub files: Vec<SendFile>,
    pub received: Mutex<Vec<FileEntry>>,
    pub url: String,
    pub qr_svg: String,
    pub lan_ip: Ipv4Addr,
    pub port: u16,
    pub created_ms: u64,
    pub expires_ms: u64,
    pub expires: Instant,
    pub pin: Option<String>,
    pub pin_state: Mutex<PinState>,
    pub downloads: AtomicU32,
    pub uploads: AtomicU32,
    pub reached: AtomicBool,
    pub received_bytes: AtomicU64,
    pub max_bytes: u64,
    pub save_dir: PathBuf,
    /// Set when the share is stopped/locked/quit: in-flight transfers abort at the next chunk.
    /// (Expiry does not set it: a transfer already running is allowed to finish.)
    pub cancel: AtomicBool,
}

impl ShareState {
    pub fn cookie_name(&self) -> String {
        format!("kv{}", self.id)
    }

    pub fn expired(&self, now: Instant) -> bool {
        now >= self.expires
    }

    pub fn secs_left(&self, now: Instant) -> u64 {
        self.expires.saturating_duration_since(now).as_secs()
    }

    pub fn needs_pin(&self) -> bool {
        self.pin.is_some()
    }

    /// Is this cookie value a PIN session we issued?
    pub fn session_ok(&self, value: &str) -> bool {
        let st = self.pin_state.lock().unwrap();
        // compare against every session without early exit
        st.sessions.iter().fold(false, |acc, s| {
            acc | token::ct_eq(s.as_bytes(), value.as_bytes())
        })
    }

    pub fn check_pin(&self, candidate: &str) -> PinCheck {
        let Some(pin) = &self.pin else {
            return PinCheck::Ok(String::new());
        };
        let mut st = self.pin_state.lock().unwrap();
        if st.failures >= MAX_PIN_FAILURES {
            return PinCheck::Locked;
        }
        if token::ct_eq(pin.as_bytes(), candidate.as_bytes()) {
            let Ok(key) = token::session_key() else {
                return PinCheck::Locked;
            };
            st.sessions.push(key.clone());
            if st.sessions.len() > MAX_SESSIONS {
                st.sessions.remove(0);
            }
            return PinCheck::Ok(key);
        }
        st.failures += 1;
        if st.failures >= MAX_PIN_FAILURES {
            PinCheck::Locked
        } else {
            PinCheck::Wrong {
                left: MAX_PIN_FAILURES - st.failures,
            }
        }
    }

    pub fn info(&self) -> ShareInfo {
        let files = match self.mode {
            Mode::Send => self
                .files
                .iter()
                .map(|f| FileEntry {
                    name: f.name.clone(),
                    size: f.size,
                })
                .collect(),
            Mode::Receive => self.received.lock().unwrap().clone(),
        };
        ShareInfo {
            id: self.id.clone(),
            url: self.url.clone(),
            qr_svg: self.qr_svg.clone(),
            files,
            mode: self.mode,
            expires_at: self.expires_ms,
            downloads: self.downloads.load(Ordering::Relaxed),
            lan_ip: self.lan_ip.to_string(),
            port: self.port,
            created_at: self.created_ms,
            uploads: self.uploads.load(Ordering::Relaxed),
            reached: self.reached.load(Ordering::Relaxed),
            pin: self.pin.clone(),
            max_upload_bytes: self.max_bytes,
            received_bytes: self.received_bytes.load(Ordering::Relaxed),
            save_dir: (self.mode == Mode::Receive)
                .then(|| self.save_dir.to_string_lossy().into_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn state(pin: Option<&str>) -> ShareState {
        let now = Instant::now();
        ShareState {
            id: "abc123abc123".into(),
            token: "T".repeat(27),
            mode: Mode::Send,
            files: vec![],
            received: Mutex::new(vec![]),
            url: "http://192.168.1.2:50000/s/x".into(),
            qr_svg: "<svg/>".into(),
            lan_ip: Ipv4Addr::new(192, 168, 1, 2),
            port: 50000,
            created_ms: 1000,
            expires_ms: 601_000,
            expires: now + Duration::from_secs(600),
            pin: pin.map(String::from),
            pin_state: Mutex::new(PinState::default()),
            downloads: AtomicU32::new(0),
            uploads: AtomicU32::new(0),
            reached: AtomicBool::new(false),
            received_bytes: AtomicU64::new(0),
            max_bytes: 1 << 20,
            save_dir: PathBuf::from("C:\\x"),
            cancel: AtomicBool::new(false),
        }
    }

    #[test]
    fn expiry_math() {
        let s = state(None);
        let now = Instant::now();
        assert!(!s.expired(now));
        assert!(s.expired(now + Duration::from_secs(601)));
        assert!(s.secs_left(now) >= 599);
        assert_eq!(s.secs_left(now + Duration::from_secs(9999)), 0);
    }

    #[test]
    fn correct_pin_issues_a_session_that_validates() {
        let s = state(Some("4821"));
        let PinCheck::Ok(key) = s.check_pin("4821") else {
            panic!("expected ok")
        };
        assert!(s.session_ok(&key));
        assert!(!s.session_ok("forged"));
        assert!(!s.session_ok(""));
    }

    #[test]
    fn five_wrong_pins_lock_the_share_even_for_the_right_pin() {
        let s = state(Some("4821"));
        for left in (1..=4).rev() {
            match s.check_pin("0000") {
                PinCheck::Wrong { left: l } => assert_eq!(l, left),
                _ => panic!("expected wrong"),
            }
        }
        assert!(matches!(s.check_pin("0000"), PinCheck::Locked));
        assert!(matches!(s.check_pin("4821"), PinCheck::Locked));
    }

    #[test]
    fn info_serializes_with_contract_field_names() {
        let v = serde_json::to_value(state(Some("1234")).info()).unwrap();
        for k in [
            "id",
            "url",
            "qrSvg",
            "files",
            "mode",
            "expiresAt",
            "downloads",
            "lanIp",
            "port",
        ] {
            assert!(v.get(k).is_some(), "missing {k}");
        }
        assert_eq!(v["mode"], "send");
        assert_eq!(v["pin"], "1234");
        assert_eq!(v["saveDir"], serde_json::Value::Null);
    }
}
