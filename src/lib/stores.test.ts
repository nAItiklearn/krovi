import { describe, it, expect } from 'vitest';
import { get } from 'svelte/store';
import {
  POMO,
  pomodoro,
  pomodoroResetCycle,
  pomodoroStart,
  timerControl,
  timers,
  pomoElapsed,
  pomoLabel,
  pomoRemaining,
  timerDone,
  timerElapsed,
  timerProgress,
  timerRemaining,
  swElapsed,
  type PomodoroState,
  type TimerItem,
} from './stores';

const timer = (o: Partial<TimerItem> = {}): TimerItem => ({
  id: 1,
  label: 'Tea',
  duration: 4 * 60_000,
  accumulated: 0,
  runningSince: null,
  ...o,
});

describe('timers', () => {
  it('counts a running timer from its start and a paused one from what it banked', () => {
    const t = timer({ runningSince: 1_000 });
    expect(timerElapsed(t, 31_000)).toBe(30_000);
    expect(timerRemaining(t, 31_000)).toBe(4 * 60_000 - 30_000);
    expect(timerElapsed(timer({ accumulated: 90_000 }), 999_999)).toBe(90_000);
  });

  it('finishes exactly at the duration and clamps remaining at zero', () => {
    const t = timer({ runningSince: 0 });
    expect(timerDone(t, 4 * 60_000 - 1)).toBe(false);
    expect(timerDone(t, 4 * 60_000)).toBe(true);
    expect(timerRemaining(t, 10 * 60_000)).toBe(0);
  });

  it('reports progress between 0 and 1', () => {
    expect(timerProgress(timer(), 0)).toBe(0);
    expect(timerProgress(timer({ accumulated: 120_000 }), 0)).toBeCloseTo(0.5, 6);
    expect(timerProgress(timer({ accumulated: 9e9 }), 0)).toBe(1);
  });

  it('never goes backwards if the clock is behind the start', () => {
    expect(timerElapsed(timer({ runningSince: 5_000 }), 1_000)).toBe(0);
  });
});

describe('pomodoro', () => {
  const p = (o: Partial<PomodoroState> = {}): PomodoroState => ({
    phase: 'work',
    duration: POMO.work,
    accumulated: 0,
    runningSince: null,
    finished: false,
    completed: 0,
    day: '2026-11-1',
    ...o,
  });

  it('uses the classic 25 / 5 / 15 cycle', () => {
    expect(POMO.work).toBe(25 * 60_000);
    expect(POMO.short).toBe(5 * 60_000);
    expect(POMO.long).toBe(15 * 60_000);
    expect(POMO.every).toBe(4);
  });

  it('measures elapsed and remaining time', () => {
    const run = p({ runningSince: 0 });
    expect(pomoElapsed(run, 60_000)).toBe(60_000);
    expect(pomoRemaining(run, 60_000)).toBe(POMO.work - 60_000);
    expect(pomoRemaining(run, POMO.work * 2)).toBe(0);
  });

  it('labels the phases for humans', () => {
    expect(pomoLabel('work')).toBe('Focus');
    expect(pomoLabel('short')).toBe('Short break');
    expect(pomoLabel('long')).toBe('Long break');
  });
});

describe('stopwatch', () => {
  it('adds the running segment to what is banked', () => {
    expect(swElapsed({ accumulated: 2_000, runningSince: 10_000, laps: [] }, 13_500)).toBe(5_500);
    expect(swElapsed({ accumulated: 2_000, runningSince: null, laps: [] }, 99_000)).toBe(2_000);
  });
});

describe('timerControl (voice: "pause the timer")', () => {
  const setTimers = (l: TimerItem[]) => timers.set(l);
  const now = () => Date.now();

  it('pauses what is running and resumes what is paused', () => {
    setTimers([timer({ id: 1, label: 'Tea', runningSince: now() - 5_000 }), timer({ id: 2, label: 'Standup' })]);
    expect(timerControl('pause', 'timer')).toBe('Tea paused');
    let [tea] = get(timers);
    expect(tea.runningSince).toBeNull();
    expect(tea.accumulated).toBeGreaterThan(4_000);

    expect(timerControl('resume', 'timer')).toBe('Tea resumed');
    [tea] = get(timers);
    expect(tea.runningSince).not.toBeNull();
  });

  it('stops by resetting, and says plainly when there is nothing to act on', () => {
    setTimers([timer({ id: 1, label: 'Tea', runningSince: now() - 5_000 })]);
    expect(timerControl('stop', 'timer')).toBe('Tea stopped');
    expect(get(timers)[0]).toMatchObject({ accumulated: 0, runningSince: null });

    setTimers([timer({ id: 1 })]);
    expect(timerControl('pause', 'timer')).toBe('No timer is running');
    expect(timerControl('resume', 'timer')).toBe('No timer is paused');
    expect(timerControl('stop', 'timer')).toBe('No timer is running');
  });

  it('names several timers instead of one', () => {
    setTimers([timer({ id: 1, runningSince: now() - 1_000 }), timer({ id: 2, label: 'B', runningSince: now() - 1_000 })]);
    expect(timerControl('pause', 'timer')).toBe('2 timers paused');
  });

  it('controls the pomodoro', () => {
    pomodoroResetCycle();
    expect(timerControl('pause', 'pomodoro')).toBe('The pomodoro isn’t running');
    pomodoroStart();
    expect(timerControl('resume', 'pomodoro')).toBe('The pomodoro is already running');
    expect(timerControl('pause', 'pomodoro')).toBe('Pomodoro paused');
    expect(timerControl('resume', 'pomodoro')).toBe('Pomodoro resumed');
    expect(timerControl('stop', 'pomodoro')).toBe('Pomodoro stopped');
    expect(get(pomodoro).runningSince).toBeNull();
  });
});
