import { describe, it, expect } from 'vitest';
import { formatClock, formatStopwatch, relativeTime, formatBytes, pad2 } from './time';

describe('time formatting', () => {
  it('pads to two digits', () => {
    expect(pad2(3)).toBe('03');
    expect(pad2(42)).toBe('42');
  });

  it('formats clock durations, dropping hours when zero', () => {
    expect(formatClock(0)).toBe('0:00');
    expect(formatClock(65_000)).toBe('1:05');
    expect(formatClock(25 * 60_000)).toBe('25:00');
    expect(formatClock(3_661_000)).toBe('1:01:01');
  });

  it('clamps negative durations to zero', () => {
    expect(formatClock(-5000)).toBe('0:00');
  });

  it('formats a stopwatch with centiseconds', () => {
    expect(formatStopwatch(0)).toBe('0:00.00');
    expect(formatStopwatch(61_230)).toBe('1:01.23');
  });

  it('describes relative time', () => {
    const now = 1_000_000_000;
    expect(relativeTime(now, now)).toBe('just now');
    expect(relativeTime(now - 4 * 60_000, now)).toBe('4m ago');
    expect(relativeTime(now - 23 * 3_600_000, now)).toBe('23h ago');
    expect(relativeTime(now - 3 * 86_400_000, now)).toBe('3d ago');
  });

  it('formats byte sizes', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(48 * 1024)).toBe('48 KB');
    expect(formatBytes(Math.round(2.3 * 1024 * 1024))).toBe('2.3 MB');
  });
});
