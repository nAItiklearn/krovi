import { describe, expect, it } from 'vitest';
import { delayLabel, delayToPos, posToDelay } from './settingsModel';

describe('delayLabel', () => {
  it('reads naturally', () => {
    expect(delayLabel(5500)).toBe('5.5 s');
    expect(delayLabel(12000)).toBe('12 s');
    expect(delayLabel(60000)).toBe('1 min');
  });
});

describe('delay slider mapping', () => {
  const MIN = 1500;
  const MAX = 60000;

  it('round-trips and snaps to the step', () => {
    for (const ms of [1500, 3000, 5500, 10000, 30000, 60000]) {
      expect(posToDelay(delayToPos(ms, MIN, MAX), MIN, MAX)).toBe(ms);
    }
  });

  it('gives the everyday range most of the track', () => {
    expect(delayToPos(5500, MIN, MAX)).toBeGreaterThan(0.25);
    expect(delayToPos(10000, MIN, MAX)).toBeGreaterThan(0.38);
  });

  it('clamps out-of-range values and positions', () => {
    expect(posToDelay(-1, MIN, MAX)).toBe(MIN);
    expect(posToDelay(2, MIN, MAX)).toBe(MAX);
    expect(delayToPos(0, MIN, MAX)).toBe(0);
    expect(delayToPos(999999, MIN, MAX)).toBe(1);
  });
});
