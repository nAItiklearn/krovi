//! Events the crate emits. The Tauri layer turns each into an `app.emit(name, payload)`;
//! the crate itself knows nothing about Tauri, so it is testable with a collecting sink.

use serde::Serialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// From the desktop's point of view: `Send` = the phone downloads, `Receive` = the phone uploads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Send,
    Receive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CloseReason {
    Expired,
    Stopped,
    Quit,
    /// Too many wrong PIN attempts.
    Locked,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Progress {
        id: String,
        file: String,
        bytes: u64,
        total: u64,
        direction: Direction,
    },
    Done {
        id: String,
        file: String,
        direction: Direction,
        size: u64,
        saved_path: Option<String>,
    },
    Failed {
        id: String,
        file: String,
        direction: Direction,
        message: String,
    },
    /// First valid request from a phone for this share (clears the "can't connect" hint).
    Reached {
        id: String,
    },
    Closed {
        id: String,
        reason: CloseReason,
    },
}

impl Event {
    /// Tauri event name (docs/CONTRACTS.md section 6).
    pub fn name(&self) -> &'static str {
        match self {
            Event::Progress { .. } => "transfer-progress",
            Event::Done { .. } => "transfer-done",
            Event::Failed { .. } => "transfer-failed",
            Event::Reached { .. } => "transfer-reached",
            Event::Closed { .. } => "transfer-closed",
        }
    }

    pub fn payload(&self) -> Value {
        match self {
            Event::Progress {
                id,
                file,
                bytes,
                total,
                direction,
            } => {
                json!({ "id": id, "file": file, "bytes": bytes, "total": total, "direction": direction })
            }
            Event::Done {
                id,
                file,
                direction,
                size,
                saved_path,
            } => {
                json!({ "id": id, "file": file, "direction": direction, "size": size, "savedPath": saved_path })
            }
            Event::Failed {
                id,
                file,
                direction,
                message,
            } => {
                json!({ "id": id, "file": file, "direction": direction, "message": message })
            }
            Event::Reached { id } => json!({ "id": id }),
            Event::Closed { id, reason } => json!({ "id": id, "reason": reason }),
        }
    }
}

pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: Event);
}

/// Discards everything (CLI/tests that do not care).
pub struct NullSink;
impl EventSink for NullSink {
    fn emit(&self, _event: Event) {}
}

/// Caps progress events at roughly 10 per second per transfer.
pub struct Throttle {
    last: Option<Instant>,
    every: Duration,
}

impl Throttle {
    pub fn new(every: Duration) -> Self {
        Self { last: None, every }
    }
    /// ~10 events per second.
    pub fn per_100ms() -> Self {
        Self::new(Duration::from_millis(100))
    }
    /// True if an event may be emitted at `now` (and records it).
    pub fn ready(&mut self, now: Instant) -> bool {
        match self.last {
            Some(l) if now.saturating_duration_since(l) < self.every => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throttle_allows_first_then_about_ten_per_second() {
        let mut t = Throttle::per_100ms();
        let t0 = Instant::now();
        assert!(t.ready(t0));
        assert!(!t.ready(t0 + Duration::from_millis(30)));
        assert!(!t.ready(t0 + Duration::from_millis(99)));
        assert!(t.ready(t0 + Duration::from_millis(100)));
        assert!(!t.ready(t0 + Duration::from_millis(150)));
        // 1 second of 1ms ticks yields ~10 events
        let mut t = Throttle::per_100ms();
        let n = (0..1000)
            .filter(|ms| t.ready(t0 + Duration::from_millis(*ms)))
            .count();
        assert_eq!(n, 10);
    }

    #[test]
    fn payload_shapes_match_the_contract() {
        let p = Event::Progress {
            id: "a".into(),
            file: "f".into(),
            bytes: 5,
            total: 10,
            direction: Direction::Send,
        }
        .payload();
        assert_eq!(
            p,
            json!({"id":"a","file":"f","bytes":5,"total":10,"direction":"send"})
        );
        let d = Event::Done {
            id: "a".into(),
            file: "f".into(),
            direction: Direction::Receive,
            size: 3,
            saved_path: Some("C:\\x".into()),
        };
        assert_eq!(d.name(), "transfer-done");
        assert_eq!(d.payload()["savedPath"], "C:\\x");
        assert_eq!(d.payload()["direction"], "receive");
        let c = Event::Closed {
            id: "a".into(),
            reason: CloseReason::Expired,
        };
        assert_eq!(c.name(), "transfer-closed");
        assert_eq!(c.payload(), json!({"id":"a","reason":"expired"}));
        assert_eq!(
            Event::Closed {
                id: "a".into(),
                reason: CloseReason::Locked
            }
            .payload()["reason"],
            "locked"
        );
    }
}
