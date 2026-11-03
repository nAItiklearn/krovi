//! Time-boxing for the screen-awareness session and the held capture (docs/DECISIONS.md D-021).
//!
//! Pure bookkeeping, no OS calls and no clock reads of its own (every function takes the time
//! as an argument), so the rules that protect the user are unit-tested here:
//!
//! * A session lasts [`DEFAULT_SESSION_MINUTES`] unless the user chose otherwise, never less
//!   than [`MIN_SESSION_MINUTES`] and **never more than [`MAX_SESSION_MINUTES`]**, whatever the
//!   caller (or a compromised webview) asks for ([`clamp_minutes`]).
//! * The end of a session is measured on the **monotonic** clock ([`Instant`]), so changing
//!   the PC's date or time cannot lengthen it. The wall-clock end time is only for display.
//! * A captured image waits in memory for at most [`SLOT_TTL`] for an optional "ask the AI"
//!   follow-up, then it is gone ([`Slot`]).

use std::time::{Duration, Instant};

/// Session length when the user does not pick one.
pub const DEFAULT_SESSION_MINUTES: u32 = 10;
/// Shortest session.
pub const MIN_SESSION_MINUTES: u32 = 1;
/// Hard cap on a session, whatever the settings or the request say.
pub const MAX_SESSION_MINUTES: u32 = 30;
/// How long a circled image stays in memory waiting for an optional AI question.
pub const SLOT_TTL: Duration = Duration::from_secs(120);

/// The session length to use: `requested` (or `default` when none), clamped to
/// [`MIN_SESSION_MINUTES`]..=[`MAX_SESSION_MINUTES`].
pub fn clamp_minutes(requested: Option<u32>, default: u32) -> u32 {
    requested
        .unwrap_or(default)
        .clamp(MIN_SESSION_MINUTES, MAX_SESSION_MINUTES)
}

/// One explicit, time-boxed awareness session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Distinguishes this session from an earlier or later one (a stale timer thread checks it).
    pub id: u64,
    /// Unix epoch milliseconds when the user started it (display only).
    pub started_ms: u64,
    /// Unix epoch milliseconds when it ends by itself (display only).
    pub ends_ms: u64,
    /// The monotonic moment it ends. This, not `ends_ms`, decides when it is over.
    pub deadline: Instant,
    /// The user agreed, at the start, that pictures of the screen may be sent to their AI
    /// provider when they ask a question with the picture option on.
    pub vision: bool,
}

impl Session {
    /// Start a session of `minutes`. The length is clamped here as well, so a `Session` can
    /// never exceed the cap even if a caller forgot [`clamp_minutes`].
    pub fn start(id: u64, now_ms: u64, now: Instant, minutes: u32, vision: bool) -> Session {
        let minutes = minutes.clamp(MIN_SESSION_MINUTES, MAX_SESSION_MINUTES);
        let length = Duration::from_secs(u64::from(minutes) * 60);
        Session {
            id,
            started_ms: now_ms,
            ends_ms: now_ms.saturating_add(length.as_millis() as u64),
            deadline: now + length,
            vision,
        }
    }

    pub fn is_over(&self, now: Instant) -> bool {
        now >= self.deadline
    }

    /// Time left; zero once over.
    pub fn remaining(&self, now: Instant) -> Duration {
        self.deadline.saturating_duration_since(now)
    }
}

/// A single value that expires. Used to hold the circled image: one at a time, replaced by the
/// next capture, gone after its time-to-live, never written anywhere.
#[derive(Debug)]
pub struct Slot<T> {
    held: Option<Held<T>>,
    ttl: Duration,
    next_generation: u64,
}

#[derive(Debug)]
struct Held<T> {
    value: T,
    stored: Instant,
    generation: u64,
}

impl<T> Slot<T> {
    pub fn new(ttl: Duration) -> Self {
        Slot {
            held: None,
            ttl,
            next_generation: 1,
        }
    }

    /// Store `value` (dropping any previous one) and return its generation, which a watchdog can
    /// later pass to [`Slot::clear_generation`] so it only clears the value it was started for.
    pub fn put(&mut self, value: T, now: Instant) -> u64 {
        let generation = self.next_generation;
        self.next_generation += 1;
        self.held = Some(Held {
            value,
            stored: now,
            generation,
        });
        generation
    }

    fn live(&self, now: Instant) -> bool {
        self.held
            .as_ref()
            .is_some_and(|h| now.saturating_duration_since(h.stored) < self.ttl)
    }

    /// The held value, unless it has expired (an expired value is dropped right here).
    pub fn get(&mut self, now: Instant) -> Option<&T> {
        if !self.live(now) {
            self.held = None;
            return None;
        }
        self.held.as_ref().map(|h| &h.value)
    }

    /// Whether a value is held and has not expired (does not drop it).
    pub fn has_value(&self, now: Instant) -> bool {
        self.live(now)
    }

    /// Drop the held value now.
    pub fn clear(&mut self) {
        self.held = None;
    }

    /// Drop the held value only if it is still the one stored as `generation`.
    pub fn clear_generation(&mut self, generation: u64) -> bool {
        if self
            .held
            .as_ref()
            .is_some_and(|h| h.generation == generation)
        {
            self.held = None;
            true
        } else {
            false
        }
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_are_clamped_to_the_hard_cap() {
        assert_eq!(clamp_minutes(None, 10), 10);
        assert_eq!(clamp_minutes(Some(5), 10), 5);
        assert_eq!(clamp_minutes(Some(0), 10), 1);
        assert_eq!(clamp_minutes(Some(30), 10), 30);
        assert_eq!(clamp_minutes(Some(31), 10), 30);
        assert_eq!(clamp_minutes(Some(u32::MAX), 10), 30);
        // a bad stored default is clamped too
        assert_eq!(clamp_minutes(None, 0), 1);
        assert_eq!(clamp_minutes(None, 500), 30);
    }

    #[test]
    fn a_session_can_never_outlive_the_cap() {
        let t0 = Instant::now();
        let s = Session::start(1, 1_000, t0, u32::MAX, true);
        assert_eq!(s.ends_ms - s.started_ms, 30 * 60_000);
        assert_eq!(s.deadline - t0, Duration::from_secs(30 * 60));
        let s = Session::start(1, 1_000, t0, 0, false);
        assert_eq!(s.ends_ms - s.started_ms, 60_000);
        assert_eq!(s.deadline - t0, Duration::from_secs(60));
        let s = Session::start(1, u64::MAX - 5, t0, 10, false);
        assert_eq!(s.ends_ms, u64::MAX); // saturates instead of wrapping into the past
    }

    #[test]
    fn a_session_ends_exactly_on_time() {
        let t0 = Instant::now();
        let s = Session::start(7, 10_000, t0, 10, true);
        assert_eq!(s.ends_ms, 10_000 + 600_000);
        assert!(s.vision);
        assert!(!s.is_over(t0));
        assert!(!s.is_over(t0 + Duration::from_millis(599_999)));
        assert!(s.is_over(t0 + Duration::from_secs(600)));
        assert!(s.is_over(t0 + Duration::from_secs(86_400)));
        assert_eq!(s.remaining(t0), Duration::from_secs(600));
        assert_eq!(
            s.remaining(t0 + Duration::from_secs(300)),
            Duration::from_secs(300)
        );
        assert_eq!(s.remaining(t0 + Duration::from_secs(700)), Duration::ZERO);
    }

    #[test]
    fn the_wall_clock_has_no_say_in_when_a_session_is_over() {
        // is_over / remaining take only the monotonic clock; a session whose wall-clock end time
        // is in the past is still running until the deadline passes.
        let t0 = Instant::now();
        let s = Session::start(1, 0, t0, 10, false);
        assert!(!s.is_over(t0 + Duration::from_secs(1)));
        assert!(s.ends_ms < crate::now_ms());
    }

    #[test]
    fn slot_holds_one_value_until_it_expires() {
        let t0 = Instant::now();
        let mut slot = Slot::new(Duration::from_secs(120));
        assert_eq!(slot.get(t0), None);
        slot.put("first", t0);
        assert_eq!(slot.get(t0 + Duration::from_secs(119)), Some(&"first"));
        assert!(slot.has_value(t0 + Duration::from_secs(119)));
        // exactly at the TTL it is gone, and stays gone
        assert_eq!(slot.get(t0 + Duration::from_secs(120)), None);
        assert!(!slot.has_value(t0));
        assert_eq!(slot.get(t0), None);
    }

    #[test]
    fn a_new_value_replaces_the_old_and_restarts_the_clock() {
        let t0 = Instant::now();
        let mut slot = Slot::new(Duration::from_secs(10));
        slot.put(1, t0);
        slot.put(2, t0 + Duration::from_secs(8));
        assert_eq!(slot.get(t0 + Duration::from_secs(15)), Some(&2));
        assert_eq!(slot.get(t0 + Duration::from_secs(18)), None);
    }

    #[test]
    fn a_watchdog_only_clears_the_value_it_was_started_for() {
        let t0 = Instant::now();
        let mut slot = Slot::new(Duration::from_secs(120));
        let g1 = slot.put("old", t0);
        let g2 = slot.put("new", t0);
        assert_ne!(g1, g2);
        assert!(
            !slot.clear_generation(g1),
            "a stale watchdog must not clear the newer image"
        );
        assert_eq!(slot.get(t0), Some(&"new"));
        assert!(slot.clear_generation(g2));
        assert_eq!(slot.get(t0), None);
        assert!(!slot.clear_generation(g2));
    }

    #[test]
    fn clear_drops_immediately() {
        let t0 = Instant::now();
        let mut slot = Slot::new(Duration::from_secs(120));
        slot.put(vec![1u8, 2, 3], t0);
        slot.clear();
        assert!(!slot.has_value(t0));
        assert_eq!(slot.ttl(), Duration::from_secs(120));
    }

    #[test]
    fn replacing_or_dropping_a_slot_drops_the_value() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        struct Probe(Arc<AtomicUsize>);
        impl Drop for Probe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let t0 = Instant::now();
        let mut slot = Slot::new(SLOT_TTL);
        slot.put(Probe(drops.clone()), t0);
        slot.put(Probe(drops.clone()), t0); // replaced: the first is dropped now
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        drop(slot);
        assert_eq!(drops.load(Ordering::SeqCst), 2);
    }
}
