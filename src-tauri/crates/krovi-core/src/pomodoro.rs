//! Pomodoro focus mode (SPEC O3).
//!
//! A thin state machine layered on [`CountdownTimer`](crate::timer::CountdownTimer):
//! alternating Work / Short-break / Long-break phases with a completed-session
//! counter. A long break replaces the short break every `long_break_every`
//! completed work sessions. Time is injected (`now`, epoch ms) so it stays
//! deterministic and testable, like the rest of the core.

use crate::timer::{CountdownTimer, TimerState};

/// Which part of the cycle the user is currently in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Work,
    ShortBreak,
    LongBreak,
}

/// Durations (ms) and cadence for a Pomodoro cycle.
#[derive(Debug, Clone, Copy)]
pub struct PomodoroConfig {
    pub work_ms: u64,
    pub short_break_ms: u64,
    pub long_break_ms: u64,
    /// A long break is taken every N completed work sessions (clamped to ≥ 1).
    pub long_break_every: u32,
}

impl Default for PomodoroConfig {
    fn default() -> Self {
        // Classic 25 / 5 / 15 minutes, long break every 4 work sessions.
        Self {
            work_ms: 25 * 60_000,
            short_break_ms: 5 * 60_000,
            long_break_ms: 15 * 60_000,
            long_break_every: 4,
        }
    }
}

/// A Pomodoro session driver.
#[derive(Debug, Clone)]
pub struct Pomodoro {
    config: PomodoroConfig,
    phase: Phase,
    timer: CountdownTimer,
    completed_work_sessions: u32,
}

impl Default for Pomodoro {
    fn default() -> Self {
        Self::new(PomodoroConfig::default())
    }
}

impl Pomodoro {
    pub fn new(config: PomodoroConfig) -> Self {
        let config = PomodoroConfig {
            long_break_every: config.long_break_every.max(1),
            ..config
        };
        Self {
            timer: CountdownTimer::new(config.work_ms),
            phase: Phase::Work,
            completed_work_sessions: 0,
            config,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn completed_work_sessions(&self) -> u32 {
        self.completed_work_sessions
    }

    pub fn start(&mut self, now: u64) {
        self.timer.start(now);
    }

    pub fn pause(&mut self, now: u64) {
        self.timer.pause(now);
    }

    pub fn remaining(&self, now: u64) -> u64 {
        self.timer.remaining(now)
    }

    pub fn timer_state(&self, now: u64) -> TimerState {
        self.timer.state(now)
    }

    pub fn is_phase_finished(&self, now: u64) -> bool {
        self.timer.is_finished(now)
    }

    /// Transition to the next phase and start its timer at `now`.
    ///
    /// Leaving a **Work** phase increments the completed-session counter and
    /// chooses a long break every `long_break_every` sessions, otherwise a short
    /// break. Leaving any break returns to Work. Returns the new phase.
    pub fn advance(&mut self, now: u64) -> Phase {
        let next = match self.phase {
            Phase::Work => {
                self.completed_work_sessions += 1;
                if self.completed_work_sessions % self.config.long_break_every == 0 {
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                }
            }
            Phase::ShortBreak | Phase::LongBreak => Phase::Work,
        };
        let dur = match next {
            Phase::Work => self.config.work_ms,
            Phase::ShortBreak => self.config.short_break_ms,
            Phase::LongBreak => self.config.long_break_ms,
        };
        self.phase = next;
        self.timer = CountdownTimer::new(dur);
        self.timer.start(now);
        next
    }

    /// Reset to a fresh Work phase with the counter cleared.
    pub fn reset(&mut self) {
        self.phase = Phase::Work;
        self.completed_work_sessions = 0;
        self.timer = CountdownTimer::new(self.config.work_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> PomodoroConfig {
        PomodoroConfig {
            work_ms: 100,
            short_break_ms: 20,
            long_break_ms: 60,
            long_break_every: 4,
        }
    }

    #[test]
    fn starts_in_work_phase() {
        let p = Pomodoro::new(cfg());
        assert_eq!(p.phase(), Phase::Work);
        assert_eq!(p.completed_work_sessions(), 0);
        assert_eq!(p.remaining(0), 100);
    }

    #[test]
    fn default_config_is_25_5_15() {
        let c = PomodoroConfig::default();
        assert_eq!(c.work_ms, 1_500_000);
        assert_eq!(c.short_break_ms, 300_000);
        assert_eq!(c.long_break_ms, 900_000);
        assert_eq!(c.long_break_every, 4);
    }

    #[test]
    fn work_advances_to_short_break_and_counts() {
        let mut p = Pomodoro::new(cfg());
        p.start(0);
        let next = p.advance(0);
        assert_eq!(next, Phase::ShortBreak);
        assert_eq!(p.completed_work_sessions(), 1);
        assert_eq!(p.remaining(0), 20); // short-break timer, freshly started
        assert_eq!(p.timer_state(0), TimerState::Running);
    }

    #[test]
    fn break_returns_to_work_without_counting() {
        let mut p = Pomodoro::new(cfg());
        p.start(0);
        p.advance(0); // Work -> ShortBreak (count 1)
        let back = p.advance(5); // ShortBreak -> Work
        assert_eq!(back, Phase::Work);
        assert_eq!(p.completed_work_sessions(), 1);
        assert_eq!(p.remaining(5), 100);
    }

    #[test]
    fn long_break_every_fourth_work_session() {
        let mut p = Pomodoro::new(cfg());
        p.start(0);
        let mut now = 0;
        for i in 1..=4 {
            let ph = p.advance(now); // finish a Work phase
            now += 1;
            if i < 4 {
                assert_eq!(ph, Phase::ShortBreak, "session {i} should be a short break");
            } else {
                assert_eq!(ph, Phase::LongBreak, "4th session should be a long break");
            }
            assert_eq!(p.completed_work_sessions(), i);
            let back = p.advance(now); // break -> Work
            now += 1;
            assert_eq!(back, Phase::Work);
        }
    }

    #[test]
    fn long_break_every_is_clamped() {
        let mut c = cfg();
        c.long_break_every = 0;
        let mut p = Pomodoro::new(c);
        p.start(0);
        // With clamp to 1, every work session yields a long break.
        assert_eq!(p.advance(0), Phase::LongBreak);
    }

    #[test]
    fn reset_clears_phase_and_count() {
        let mut p = Pomodoro::new(cfg());
        p.start(0);
        p.advance(0);
        p.advance(0);
        p.reset();
        assert_eq!(p.phase(), Phase::Work);
        assert_eq!(p.completed_work_sessions(), 0);
        assert_eq!(p.remaining(0), 100);
    }
}
