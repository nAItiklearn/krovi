//! Alarms: the native "this must ring" half of timers and Pomodoro phases.
//!
//! The panel owns the timer UI (start/pause/laps), but a webview that is hidden or
//! throttled cannot be trusted to notice that a countdown ended. So whenever a countdown
//! is running, the UI tells the backend *when* it ends ([`AlarmBook::sync`]); the
//! backend's single scheduler thread rings it at that instant, whatever the window mode.
//!
//! Invariants (all tested below):
//! * **Exactly once.** [`AlarmBook::collect_due`] marks an alarm fired in the same step
//!   that hands it out. Re-syncing the same key with the same due time keeps the mark,
//!   so a restart, a re-render or a duplicate sync can never ring it again.
//! * **Re-armable.** Syncing a key with a *different* due time (timer restarted, phase
//!   advanced) is a new alarm and may ring again.
//! * **Missed while closed.** Anything already due when collected fires once, flagged
//!   `missed` when more than [`MISSED_AFTER_MS`] late; more than [`STALE_AFTER_MS`] late
//!   is marked fired but `silent` (never announced).
//! * Times are epoch milliseconds (UTC). No wall-clock maths happens here.
//!
//! Like the rest of `krovi-core` this module has no dependencies; the app layer
//! persists [`AlarmBook::list`] and restores it with [`AlarmBook::from_alarms`].

/// Later than this when collected = "missed" (announced as such).
pub const MISSED_AFTER_MS: u64 = 60_000;
/// Later than this = marked fired but not announced.
pub const STALE_AFTER_MS: u64 = 24 * 60 * 60 * 1000;
/// Fired alarms no longer in the synced set are forgotten this long after firing.
pub const KEEP_FIRED_MS: u64 = 24 * 60 * 60 * 1000;
/// Longest countdown accepted (the timer UI allows at most 24 h; 7 days is headroom).
pub const MAX_AHEAD_MS: u64 = 7 * 24 * 60 * 60 * 1000;
pub const MAX_ALARMS: usize = 64;
pub const MAX_KEY: usize = 64;
pub const MAX_LABEL: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlarmKind {
    Timer,
    Pomodoro,
}

impl AlarmKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AlarmKind::Timer => "timer",
            AlarmKind::Pomodoro => "pomodoro",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "timer" => Some(AlarmKind::Timer),
            "pomodoro" => Some(AlarmKind::Pomodoro),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alarm {
    pub key: String,
    pub label: String,
    pub kind: AlarmKind,
    pub due_at: u64,
    pub fired_at: Option<u64>,
}

/// What the UI asks for: "this countdown ends at `due_at`".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlarmSpec {
    pub key: String,
    pub label: String,
    pub kind: AlarmKind,
    pub due_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DueAlarm {
    pub alarm: Alarm,
    pub late_ms: u64,
    pub missed: bool,
    /// Too old to announce (only marked).
    pub silent: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AlarmError {
    TooMany,
    BadKey,
    BadLabel,
    TooFarAhead,
}

impl std::fmt::Display for AlarmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AlarmError::TooMany => "too many running countdowns",
            AlarmError::BadKey => "invalid countdown id",
            AlarmError::BadLabel => "countdown label is too long",
            AlarmError::TooFarAhead => "countdown ends too far in the future",
        })
    }
}

impl std::error::Error for AlarmError {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlarmBook {
    alarms: Vec<Alarm>,
}

fn valid_key(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= MAX_KEY
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_' | '.'))
}

impl AlarmBook {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn list(&self) -> &[Alarm] {
        &self.alarms
    }

    /// Restore a persisted book. Entries with an invalid key or a duplicate key are
    /// dropped (a corrupt store must not stop the scheduler); labels are clipped.
    pub fn from_alarms(alarms: Vec<Alarm>) -> Self {
        let mut out: Vec<Alarm> = Vec::with_capacity(alarms.len().min(MAX_ALARMS * 2));
        for mut a in alarms {
            if !valid_key(&a.key) || out.iter().any(|o| o.key == a.key && o.due_at == a.due_at) {
                continue;
            }
            if a.label.chars().count() > MAX_LABEL {
                a.label = a.label.chars().take(MAX_LABEL).collect();
            }
            out.push(a);
            if out.len() >= MAX_ALARMS * 2 {
                break;
            }
        }
        Self { alarms: out }
    }

    /// Replace the set of *wanted* alarms. Validates everything first; on error nothing
    /// changes. Keeps the fired mark of a key re-synced with an unchanged due time, drops
    /// unfired alarms that are no longer wanted, and keeps recently fired ones (forgotten
    /// [`KEEP_FIRED_MS`] after firing) so an echo of the same spec cannot re-ring.
    pub fn sync(&mut self, specs: &[AlarmSpec], now: u64) -> Result<(), AlarmError> {
        if specs.len() > MAX_ALARMS {
            return Err(AlarmError::TooMany);
        }
        for s in specs {
            if !valid_key(&s.key) {
                return Err(AlarmError::BadKey);
            }
            if s.label.chars().count() > MAX_LABEL {
                return Err(AlarmError::BadLabel);
            }
            if s.due_at > now.saturating_add(MAX_AHEAD_MS) {
                return Err(AlarmError::TooFarAhead);
            }
        }
        let mut next: Vec<Alarm> = Vec::with_capacity(specs.len());
        for s in specs {
            if next.iter().any(|a| a.key == s.key) {
                continue; // first spec for a key wins
            }
            let prior = self
                .alarms
                .iter()
                .find(|a| a.key == s.key && a.due_at == s.due_at);
            next.push(Alarm {
                key: s.key.clone(),
                label: s.label.trim().to_string(),
                kind: s.kind,
                due_at: s.due_at,
                fired_at: prior.and_then(|a| a.fired_at),
            });
        }
        // keep recently fired alarms that the UI no longer lists (echo protection)
        for a in &self.alarms {
            if let Some(f) = a.fired_at {
                let wanted = next.iter().any(|n| n.key == a.key && n.due_at == a.due_at);
                if !wanted && now.saturating_sub(f) < KEEP_FIRED_MS {
                    next.push(a.clone());
                }
            }
        }
        self.alarms = next;
        Ok(())
    }

    /// Everything due at `now`, each marked fired in the same step.
    pub fn collect_due(&mut self, now: u64) -> Vec<DueAlarm> {
        let mut out = Vec::new();
        for a in self.alarms.iter_mut() {
            if a.fired_at.is_none() && a.due_at <= now {
                a.fired_at = Some(now);
                let late_ms = now - a.due_at;
                out.push(DueAlarm {
                    alarm: a.clone(),
                    late_ms,
                    missed: late_ms > MISSED_AFTER_MS,
                    silent: late_ms > STALE_AFTER_MS,
                });
            }
        }
        out.sort_by_key(|d| d.alarm.due_at);
        out
    }

    /// Earliest unfired due time.
    pub fn next_due(&self) -> Option<u64> {
        self.alarms
            .iter()
            .filter(|a| a.fired_at.is_none())
            .map(|a| a.due_at)
            .min()
    }

    pub fn pending(&self) -> usize {
        self.alarms.iter().filter(|a| a.fired_at.is_none()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(key: &str, due: u64) -> AlarmSpec {
        AlarmSpec {
            key: key.into(),
            label: format!("label {key}"),
            kind: AlarmKind::Timer,
            due_at: due,
        }
    }

    #[test]
    fn fires_exactly_once_at_due_time() {
        let mut b = AlarmBook::new();
        b.sync(&[spec("timer:1", 10_000)], 0).unwrap();
        assert!(b.collect_due(9_999).is_empty());
        let due = b.collect_due(10_000);
        assert_eq!(due.len(), 1);
        assert!(!due[0].missed);
        assert!(b.collect_due(10_001).is_empty());
        assert!(b.collect_due(99_999).is_empty());
    }

    #[test]
    fn resync_of_same_spec_keeps_fired_mark() {
        let mut b = AlarmBook::new();
        b.sync(&[spec("timer:1", 10_000)], 0).unwrap();
        assert_eq!(b.collect_due(10_000).len(), 1);
        // UI re-renders / app restarts and sends the same running timer again
        b.sync(&[spec("timer:1", 10_000)], 11_000).unwrap();
        assert!(b.collect_due(12_000).is_empty());
    }

    #[test]
    fn echo_after_drop_does_not_rering() {
        let mut b = AlarmBook::new();
        b.sync(&[spec("timer:1", 10_000)], 0).unwrap();
        b.collect_due(10_000);
        b.sync(&[], 10_500).unwrap(); // UI marked it done
        b.sync(&[spec("timer:1", 10_000)], 10_600).unwrap(); // stale echo
        assert!(b.collect_due(10_700).is_empty());
    }

    #[test]
    fn new_due_time_rearms_same_key() {
        let mut b = AlarmBook::new();
        b.sync(&[spec("pomodoro", 10_000)], 0).unwrap();
        b.collect_due(10_000);
        b.sync(&[spec("pomodoro", 20_000)], 10_100).unwrap(); // next phase
        assert_eq!(b.collect_due(20_000).len(), 1);
    }

    #[test]
    fn cancelling_unfired_removes_it() {
        let mut b = AlarmBook::new();
        b.sync(&[spec("timer:1", 10_000), spec("timer:2", 20_000)], 0)
            .unwrap();
        b.sync(&[spec("timer:2", 20_000)], 1_000).unwrap();
        assert_eq!(b.pending(), 1);
        assert_eq!(b.next_due(), Some(20_000));
        assert!(b.collect_due(15_000).is_empty());
    }

    #[test]
    fn paused_timer_moves_due_time() {
        let mut b = AlarmBook::new();
        b.sync(&[spec("timer:1", 10_000)], 0).unwrap();
        b.sync(&[], 5_000).unwrap(); // paused: no longer running
        b.sync(&[spec("timer:1", 18_000)], 13_000).unwrap(); // resumed
        assert!(b.collect_due(17_000).is_empty());
        assert_eq!(b.collect_due(18_000).len(), 1);
    }

    #[test]
    fn missed_and_stale_classification() {
        let mut b = AlarmBook::new();
        b.sync(&[spec("a", 1_000), spec("b", 2_000), spec("c", 3_000)], 0)
            .unwrap();
        let now = 3_000 + MISSED_AFTER_MS + 1;
        let due = b.collect_due(now);
        assert_eq!(due.len(), 3);
        assert!(due.iter().all(|d| d.missed && !d.silent));
        assert_eq!(due[0].alarm.key, "a"); // ordered by due time

        let mut b = AlarmBook::new();
        b.sync(&[spec("old", 1_000)], 0).unwrap();
        let due = b.collect_due(1_000 + STALE_AFTER_MS + 1);
        assert!(due[0].silent);
    }

    #[test]
    fn simultaneous_timers_each_fire_once() {
        let mut b = AlarmBook::new();
        b.sync(&[spec("t1", 5_000), spec("t2", 5_000)], 0).unwrap();
        assert_eq!(b.collect_due(5_000).len(), 2);
        assert!(b.collect_due(6_000).is_empty());
    }

    #[test]
    fn validation_rejects_and_changes_nothing() {
        let mut b = AlarmBook::new();
        b.sync(&[spec("ok", 1_000)], 0).unwrap();
        assert_eq!(b.sync(&[spec("bad key!", 1)], 0), Err(AlarmError::BadKey));
        assert_eq!(b.sync(&[spec("", 1)], 0), Err(AlarmError::BadKey));
        let far = spec("far", MAX_AHEAD_MS + 10);
        assert_eq!(b.sync(&[far], 0), Err(AlarmError::TooFarAhead));
        let mut long = spec("long", 1);
        long.label = "x".repeat(MAX_LABEL + 1);
        assert_eq!(b.sync(&[long], 0), Err(AlarmError::BadLabel));
        let many: Vec<_> = (0..=MAX_ALARMS)
            .map(|i| spec(&format!("k{i}"), 1))
            .collect();
        assert_eq!(b.sync(&many, 0), Err(AlarmError::TooMany));
        assert_eq!(b.pending(), 1);
    }

    #[test]
    fn fired_entries_are_forgotten_after_keep_window() {
        let mut b = AlarmBook::new();
        b.sync(&[spec("t", 1_000)], 0).unwrap();
        b.collect_due(1_000);
        b.sync(&[], 1_000 + KEEP_FIRED_MS + 1).unwrap();
        assert!(b.list().is_empty());
    }

    #[test]
    fn restore_round_trip_keeps_fired_marks() {
        let mut b = AlarmBook::new();
        b.sync(&[spec("t", 1_000), spec("u", 5_000)], 0).unwrap();
        b.collect_due(1_000);
        let back = AlarmBook::from_alarms(b.list().to_vec());
        assert_eq!(b, back);
        // after a "restart", the fired one stays quiet, the other still rings
        let mut back = back;
        back.sync(&[spec("t", 1_000), spec("u", 5_000)], 2_000)
            .unwrap();
        let due = back.collect_due(6_000);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].alarm.key, "u");
    }

    #[test]
    fn restore_drops_corrupt_entries() {
        let good = Alarm {
            key: "ok".into(),
            label: "y".repeat(MAX_LABEL + 5),
            kind: AlarmKind::Pomodoro,
            due_at: 1,
            fired_at: None,
        };
        let bad = Alarm {
            key: "no spaces".into(),
            ..good.clone()
        };
        let b = AlarmBook::from_alarms(vec![good.clone(), bad, good]);
        assert_eq!(b.list().len(), 1);
        assert_eq!(b.list()[0].label.chars().count(), MAX_LABEL);
    }

    #[test]
    fn kind_strings_round_trip() {
        for k in [AlarmKind::Timer, AlarmKind::Pomodoro] {
            assert_eq!(AlarmKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(AlarmKind::parse("alarm"), None);
    }
}
