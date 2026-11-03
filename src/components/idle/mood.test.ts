import { describe, expect, it } from 'vitest';
import type { SystemStats } from '../../lib/types';
import { MIN_DWELL_MS, initMood, nextMood, type MoodState } from './mood';

const GB = 1024 ** 3;
const s = (o: Partial<SystemStats> = {}): SystemStats => ({
  memUsed: 6 * GB,
  memTotal: 16 * GB,
  diskUsed: 1,
  diskTotal: 2,
  diskLabel: 'C:',
  cpu: 35,
  battery: null,
  ...o,
});

/** Feed the same sample every 2 s for `ms`. */
function run(st: MoodState, sample: SystemStats, from: number, ms: number): MoodState {
  let x = st;
  for (let t = from; t <= from + ms; t += 2000) x = nextMood(x, sample, t);
  return x;
}

describe('companion mood (real stats only)', () => {
  it('stays neutral under ordinary load and with no battery', () => {
    expect(run(initMood(), s(), 0, 60_000).mood).toBe('neutral');
  });

  it('is sad on a low battery that is not charging, and recovers with hysteresis', () => {
    let m = run(initMood(), s({ battery: { percent: 12, pluggedIn: false, charging: false } }), 0, 30_000);
    expect(m.mood).toBe('sad');
    // 18 % is above the enter threshold but below the leave threshold: still sad
    m = run(m, s({ battery: { percent: 18, pluggedIn: false, charging: false } }), 32_000, 30_000);
    expect(m.mood).toBe('sad');
    // plugged in: no longer sad
    m = run(m, s({ battery: { percent: 18, pluggedIn: true, charging: true } }), 64_000, 30_000);
    expect(m.mood).not.toBe('sad');
  });

  it('is happy when the battery is comfortably charged', () => {
    const m = run(initMood(), s({ battery: { percent: 96, pluggedIn: true, charging: false } }), 0, 30_000);
    expect(m.mood).toBe('happy');
  });

  it('is stressed only under sustained real RAM pressure (one spike is not enough)', () => {
    let m = initMood();
    m = nextMood(m, s({ memUsed: 15.5 * GB }), 20_000);
    m = nextMood(m, s(), 22_000);
    expect(m.mood).toBe('neutral');
    m = run(m, s({ memUsed: 15.5 * GB }), 24_000, 40_000);
    expect(m.mood).toBe('stressed');
  });

  it('is calm when the CPU stays low', () => {
    expect(run(initMood(), s({ cpu: 3 }), 0, 40_000).mood).toBe('calm');
  });

  it('never flips faster than the minimum dwell', () => {
    let m = run(initMood(), s({ cpu: 3 }), 0, 40_000);
    expect(m.mood).toBe('calm');
    const since = m.since;
    m = run(m, s({ memUsed: 15.9 * GB, cpu: 3 }), 42_000, 4_000);
    expect(m.mood).toBe('calm'); // RAM spiked but the dwell + smoothing hold the mood
    m = run(m, s({ memUsed: 15.9 * GB, cpu: 3 }), 48_000, MIN_DWELL_MS + 20_000);
    expect(m.mood).toBe('stressed');
    expect(m.since).toBeGreaterThanOrEqual(since + MIN_DWELL_MS);
  });

  it('sad outranks stressed and happy', () => {
    const m = run(
      initMood(),
      s({ memUsed: 15.9 * GB, battery: { percent: 5, pluggedIn: false, charging: false } }),
      0,
      40_000,
    );
    expect(m.mood).toBe('sad');
  });
});
