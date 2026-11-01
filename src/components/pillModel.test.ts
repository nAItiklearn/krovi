import { describe, it, expect } from 'vitest';
import {
  PILL_MAX_W,
  PILL_MIN_W,
  clipLabel,
  clockText,
  diskPercent,
  freeDisk,
  latestClip,
  memPercent,
  morphRadius,
  pillMode,
  pillSize,
  snippet,
} from './pillModel';
import type { SystemStats } from '../lib/live';
import type { ClipEntry } from '../lib/types';

const GB = 1024 ** 3;
const stats = (o: Partial<SystemStats> = {}): SystemStats => ({
  memUsed: 8 * GB,
  memTotal: 16 * GB,
  diskUsed: 262 * GB,
  diskTotal: 476 * GB,
  diskLabel: 'C:',
  cpu: 12,
  ...o,
});
const clip = (o: Partial<ClipEntry>): ClipEntry => ({
  id: 1,
  kind: 'Text',
  content: 'x',
  pinned: false,
  created_at: 0,
  ...o,
});

describe('pill stats', () => {
  it('computes RAM and disk percentages, null when unavailable', () => {
    expect(memPercent(stats())).toBe(50);
    expect(diskPercent(stats())).toBeCloseTo((262 / 476) * 100, 5);
    expect(memPercent(null)).toBeNull();
    expect(memPercent(stats({ memTotal: 0 }))).toBeNull();
  });

  it('formats free disk space in GB, switching to TB at a terabyte', () => {
    expect(freeDisk(stats())?.text).toBe('214 GB');
    expect(freeDisk(stats({ diskTotal: 2048 * GB, diskUsed: 512 * GB }))?.text).toBe('1.5 TB');
    expect(freeDisk(stats({ diskUsed: 600 * GB }))?.text).toBe('0 GB');
    expect(freeDisk(null)).toBeNull();
  });
});

describe('live-activity text', () => {
  it('flattens whitespace and truncates with an ellipsis', () => {
    expect(snippet('  hello\n   world  ')).toBe('hello world');
    const long = 'a'.repeat(60);
    expect(snippet(long, 10)).toBe('a'.repeat(9) + '…');
  });

  it('describes links by host and path, images by kind', () => {
    expect(clipLabel(clip({ kind: 'Link', content: 'https://www.krovi.tech/features/glass' }))).toBe(
      'krovi.tech/features/glass',
    );
    expect(clipLabel(clip({ kind: 'Image', content: 'blobs/x.png' }))).toBe('Image');
    expect(clipLabel(clip({ kind: 'Link', content: 'not a url' }))).toBe('not a url');
  });

  it('picks the newest clip even when a pinned one sorts first', () => {
    const list = [clip({ id: 1, pinned: true, created_at: 10 }), clip({ id: 2, created_at: 99 }), clip({ id: 3, created_at: 50 })];
    expect(latestClip(list)?.id).toBe(2);
    expect(latestClip([])).toBeNull();
  });

  it('formats a countdown', () => {
    expect(clockText(0)).toBe('0:00');
    expect(clockText(65_000)).toBe('1:05');
    expect(clockText(59_100)).toBe('1:00'); // rounds up so the pill never shows 0:00 early
    expect(clockText(3_725_000)).toBe('1:02:05');
  });
});

describe('pill geometry', () => {
  it('voice beats activity beats idle', () => {
    expect(pillMode({ voice: true, activity: true })).toBe('voice');
    expect(pillMode({ voice: false, activity: true })).toBe('activity');
    expect(pillMode({ voice: false, activity: false })).toBe('idle');
  });

  it('clamps the pill to the contract envelope', () => {
    expect(pillSize('idle', 10).w).toBe(PILL_MIN_W);
    expect(pillSize('activity', 900).w).toBe(PILL_MAX_W);
    expect(pillSize('idle', 200).w).toBe(212);
    expect(pillSize('activity', 300).h).toBeGreaterThan(pillSize('idle', 300).h);
    expect(pillSize('activity', 300).h).toBeLessThanOrEqual(44);
  });

  it('keeps the pill fully round and relaxes toward the panel radius', () => {
    expect(morphRadius(0, 262, 36, 36, 30)).toBe(18);
    expect(morphRadius(1, 900, 620, 36, 30)).toBeCloseTo(30, 6);
    expect(morphRadius(0.5, 400, 300, 36, 30)).toBeGreaterThan(18);
    // never rounder than half the shape, whatever the progress
    expect(morphRadius(1, 40, 20, 36, 30)).toBe(10);
  });
});
