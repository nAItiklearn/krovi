//! Countdown timers and a stopwatch (SPEC C3; the countdown also underpins the
//! O3 Pomodoro mode layered on top of it later).
//!
//! All time is injected: every query takes a `now` (epoch milliseconds) from the
//! caller, so the logic is deterministic and unit-testable with no real clock.
//! Completion *notifications* (observing the Idle/Running→Finished transition)
//! are the integration layer's job; this module only reports state as a function
//! of `now`. `saturating_sub` guards against non-monotonic clocks.

/// Lifecycle of a [`CountdownTimer`] at a given instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerState {
    Idle,
    Running,
    Paused,
    Finished,
}

/// A single countdown timer of fixed `duration_ms`.
#[derive(Debug, Clone)]
pub struct CountdownTimer {
    duration_ms: u64,
    /// Elapsed time accumulated over previous running spans (ms).
    accumulated_ms: u64,
    /// `Some(start_now)` while running, `None` while idle/paused.
    running_since: Option<u64>,
}

impl CountdownTimer {
    pub fn new(duration_ms: u64) -> Self {
        Self {
            duration_ms,
            accumulated_ms: 0,
            running_since: None,
        }
    }

    pub fn duration(&self) -> u64 {
        self.duration_ms
    }

    /// Raw elapsed time, uncapped (may exceed `duration_ms`).
    fn raw_elapsed(&self, now: u64) -> u64 {
        let running = self.running_since.map_or(0, |s| now.saturating_sub(s));
        self.accumulated_ms + running
    }

    /// Elapsed time, capped at `duration_ms`.
    pub fn elapsed(&self, now: u64) -> u64 {
        self.raw_elapsed(now).min(self.duration_ms)
    }

    /// Time left before completion (0 once finished).
    pub fn remaining(&self, now: u64) -> u64 {
        self.duration_ms - self.elapsed(now)
    }

    pub fn is_finished(&self, now: u64) -> bool {
        self.raw_elapsed(now) >= self.duration_ms
    }

    pub fn state(&self, now: u64) -> TimerState {
        if self.is_finished(now) {
            TimerState::Finished
        } else if self.running_since.is_some() {
            TimerState::Running
        } else if self.accumulated_ms > 0 {
            TimerState::Paused
        } else {
            TimerState::Idle
        }
    }

    /// Start or resume counting. No-op if already running or already finished.
    pub fn start(&mut self, now: u64) {
        if self.running_since.is_none() && !self.is_finished(now) {
            self.running_since = Some(now);
        }
    }

    /// Pause counting, banking the elapsed time so far. No-op if not running.
    pub fn pause(&mut self, now: u64) {
        if let Some(start) = self.running_since.take() {
            self.accumulated_ms += now.saturating_sub(start);
        }
    }

    /// Return to Idle with the full duration available again.
    pub fn reset(&mut self) {
        self.accumulated_ms = 0;
        self.running_since = None;
    }
}

/// A count-up stopwatch with lap support.
#[derive(Debug, Clone, Default)]
pub struct Stopwatch {
    accumulated_ms: u64,
    running_since: Option<u64>,
    laps: Vec<u64>,
}

impl Stopwatch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_running(&self) -> bool {
        self.running_since.is_some()
    }

    pub fn elapsed(&self, now: u64) -> u64 {
        let running = self.running_since.map_or(0, |s| now.saturating_sub(s));
        self.accumulated_ms + running
    }

    /// Start or resume. No-op if already running.
    pub fn start(&mut self, now: u64) {
        if self.running_since.is_none() {
            self.running_since = Some(now);
        }
    }

    /// Pause, banking elapsed time. No-op if not running.
    pub fn pause(&mut self, now: u64) {
        if let Some(start) = self.running_since.take() {
            self.accumulated_ms += now.saturating_sub(start);
        }
    }

    /// Record a lap at `now`, returning the elapsed time captured.
    pub fn lap(&mut self, now: u64) -> u64 {
        let e = self.elapsed(now);
        self.laps.push(e);
        e
    }

    pub fn laps(&self) -> &[u64] {
        &self.laps
    }

    /// Reset to zero and clear laps.
    pub fn reset(&mut self) {
        self.accumulated_ms = 0;
        self.running_since = None;
        self.laps.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_timer_reports_full_duration() {
        let t = CountdownTimer::new(1000);
        assert_eq!(t.remaining(0), 1000);
        assert_eq!(t.elapsed(0), 0);
        assert_eq!(t.state(0), TimerState::Idle);
        assert!(!t.is_finished(0));
    }

    #[test]
    fn running_elapses_with_now() {
        let mut t = CountdownTimer::new(1000);
        t.start(100);
        assert_eq!(t.state(100), TimerState::Running);
        assert_eq!(t.elapsed(600), 500);
        assert_eq!(t.remaining(600), 500);
    }

    #[test]
    fn pause_freezes_and_resume_continues() {
        let mut t = CountdownTimer::new(1000);
        t.start(0);
        t.pause(300); // 300ms banked
        assert_eq!(t.state(300), TimerState::Paused);
        assert_eq!(t.elapsed(999), 300); // frozen regardless of now while paused
        t.start(1000); // resume
        assert_eq!(t.elapsed(1200), 500); // 300 + (1200 - 1000)
        assert_eq!(t.remaining(1200), 500);
    }

    #[test]
    fn finishes_and_caps_elapsed() {
        let mut t = CountdownTimer::new(500);
        t.start(0);
        assert!(t.is_finished(500));
        assert_eq!(t.remaining(10_000), 0);
        assert_eq!(t.elapsed(10_000), 500); // capped at duration
        assert_eq!(t.state(600), TimerState::Finished);
    }

    #[test]
    fn start_is_noop_once_finished() {
        let mut t = CountdownTimer::new(100);
        t.start(0);
        t.start(1000); // must not restart or extend
        assert_eq!(t.state(1000), TimerState::Finished);
        assert_eq!(t.remaining(1000), 0);
    }

    #[test]
    fn reset_returns_to_idle() {
        let mut t = CountdownTimer::new(1000);
        t.start(0);
        t.pause(400);
        t.reset();
        assert_eq!(t.state(0), TimerState::Idle);
        assert_eq!(t.remaining(0), 1000);
    }

    #[test]
    fn clock_skew_does_not_underflow() {
        let mut t = CountdownTimer::new(1000);
        t.start(500);
        assert_eq!(t.elapsed(400), 0); // now before start → saturating
        assert_eq!(t.remaining(400), 1000);
    }

    #[test]
    fn stopwatch_counts_up_with_pause() {
        let mut s = Stopwatch::new();
        assert_eq!(s.elapsed(0), 0);
        s.start(100);
        assert_eq!(s.elapsed(400), 300);
        s.pause(400);
        assert_eq!(s.elapsed(999), 300); // frozen while paused
        s.start(1000);
        assert_eq!(s.elapsed(1500), 800); // 300 + (1500 - 1000)
    }

    #[test]
    fn stopwatch_laps_and_reset() {
        let mut s = Stopwatch::new();
        s.start(0);
        assert_eq!(s.lap(250), 250);
        assert_eq!(s.lap(600), 600);
        assert_eq!(s.laps(), &[250, 600]);
        s.reset();
        assert_eq!(s.elapsed(10_000), 0);
        assert!(s.laps().is_empty());
    }
}
