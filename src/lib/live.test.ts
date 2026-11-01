import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import {
  CLIP_CAP,
  addClip,
  advanceDemoMedia,
  classifyText,
  clearClips,
  clips,
  copyClip,
  media,
  mediaPosition,
  mediaTransport,
  nextDemoStats,
  pulse,
  removeClip,
  startLive,
  stats,
  stopLive,
  togglePin,
} from './live';
import type { ClipEntry, MediaState } from './types';
import { demoMediaQueue, statsFixture } from './fixtures';

const e = (id: number, content: string, pinned = false, kind: ClipEntry['kind'] = 'Text'): ClipEntry => ({
  id,
  kind,
  content,
  pinned,
  created_at: id,
});

const track = (over: Partial<MediaState> = {}): MediaState => ({
  title: 'Midnight Drive',
  artist: 'Neon Harbor',
  album: 'After Hours',
  playing: true,
  positionSec: 10,
  durationSec: 200,
  sampledAt: 1_000,
  sourceApp: 'Spotify.exe',
  artwork: null,
  ...over,
});

describe('mediaPosition', () => {
  it('advances by wall time while playing', () => {
    expect(mediaPosition(track(), 4_000)).toBe(13);
  });
  it('is frozen while paused', () => {
    expect(mediaPosition(track({ playing: false }), 9_000)).toBe(10);
  });
  it('clamps to the duration and never goes negative', () => {
    expect(mediaPosition(track({ positionSec: 199 }), 11_000)).toBe(200);
    expect(mediaPosition(track({ positionSec: -3, playing: false }), 0)).toBe(0);
  });
  it('is 0 with no session, and ignores a clock that went backwards', () => {
    expect(mediaPosition(null, 5)).toBe(0);
    expect(mediaPosition(track(), 500)).toBe(10);
  });
  it('does not clamp when the duration is unknown (live stream)', () => {
    expect(mediaPosition(track({ durationSec: 0 }), 31_000)).toBe(40);
  });
});

describe('classifyText', () => {
  it('detects links and plain text', () => {
    expect(classifyText('https://krovi.tech')).toBe('Link');
    expect(classifyText('  www.example.com ')).toBe('Link');
    expect(classifyText('mailto:a@b.co')).toBe('Link');
    expect(classifyText('see https://krovi.tech')).toBe('Text');
    expect(classifyText('')).toBe('Text');
    expect(classifyText('www.nodot')).toBe('Text');
  });
});

describe('addClip', () => {
  it('puts a new copy first among the unpinned, below pinned ones', () => {
    const list = [e(3, 'pinned', true), e(2, 'b'), e(1, 'a')];
    const { list: out, id } = addClip(list, 'fresh', 99);
    expect(id).toBe(4);
    expect(out.map((x) => x.content)).toEqual(['pinned', 'fresh', 'b', 'a']);
  });
  it('promotes a duplicate instead of adding another, keeping its pin', () => {
    const list = [e(3, 'keep', true), e(2, 'b'), e(1, 'a')];
    const { list: out } = addClip(list, 'a', 99);
    expect(out.map((x) => x.content)).toEqual(['keep', 'a', 'b']);
    const pinnedDup = addClip(list, 'keep', 100).list;
    expect(pinnedDup).toHaveLength(3);
    expect(pinnedDup[0]).toMatchObject({ content: 'keep', pinned: true, id: 4 });
  });
  it('classifies links', () => {
    expect(addClip([], 'https://x.y', 1).list[0].kind).toBe('Link');
  });
  it('caps the unpinned entries but never drops pinned ones', () => {
    let list: ClipEntry[] = [e(1000, 'pinned', true)];
    for (let i = 0; i < CLIP_CAP + 5; i++) list = addClip(list, `c${i}`, i).list;
    expect(list.filter((x) => !x.pinned)).toHaveLength(CLIP_CAP);
    expect(list.filter((x) => x.pinned)).toHaveLength(1);
    expect(list.at(-1)?.content).toBe('c5'); // c0..c4 fell off the end
  });
});

describe('demo feeds', () => {
  it('RAM/CPU wander but stay in a sane band and keep the totals', () => {
    let s = { ...statsFixture };
    let seed = 7;
    const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
    const seen = new Set<number>();
    for (let i = 0; i < 400; i++) {
      s = nextDemoStats(s, rnd);
      seen.add(s.memUsed);
      expect(s.memUsed).toBeGreaterThanOrEqual(0.35 * s.memTotal);
      expect(s.memUsed).toBeLessThanOrEqual(0.85 * s.memTotal);
      expect(s.cpu).toBeGreaterThanOrEqual(2);
      expect(s.cpu).toBeLessThanOrEqual(60);
      expect(s.memTotal).toBe(statsFixture.memTotal);
    }
    expect(seen.size).toBeGreaterThan(100); // it actually moves
  });
  it('the media position re-anchors, and rolls over to the next track at the end', () => {
    const m = track({ title: demoMediaQueue[0].title, positionSec: 100, sampledAt: 0, durationSec: 214 });
    const re = advanceDemoMedia(m, 5_000);
    expect(re.positionSec).toBe(105);
    expect(re.sampledAt).toBe(5_000);
    const end = advanceDemoMedia({ ...m, positionSec: 213 }, 2_000);
    expect(end.title).toBe(demoMediaQueue[1].title);
    expect(end.positionSec).toBe(0);
    expect(end.playing).toBe(true);
  });
});

describe('demo-mode actions and startLive', () => {
  afterEach(() => {
    stopLive();
    vi.useRealTimers();
    clips.set([]);
    media.set(null);
    stats.set(null);
    pulse.set(0);
  });

  it('startLive is idempotent and seeds the demo feed', () => {
    vi.useFakeTimers();
    startLive();
    startLive();
    expect(get(clips).length).toBeGreaterThan(3);
    expect(get(media)?.title).toBe(demoMediaQueue[0].title);
    expect(get(stats)?.diskLabel).toBe('C:');
    // idempotent: two calls must not have installed two copy timers
    vi.advanceTimersByTime(12_000);
    expect(get(pulse)).toBe(1);
  });

  it('simulates a copy about every 12 s (glow pulse + new top entry), RAM keeps moving', () => {
    vi.useFakeTimers();
    startLive();
    const before = get(clips).length;
    const ram = get(stats)!.memUsed;
    vi.advanceTimersByTime(12_000);
    expect(get(pulse)).toBe(1);
    expect(get(clips).length).toBe(before + 1);
    const top = get(clips).find((c) => !c.pinned)!;
    expect(top.content).toBe('https://krovi.tech/features');
    vi.advanceTimersByTime(24_000);
    expect(get(pulse)).toBe(3);
    expect(get(stats)!.memUsed).not.toBe(ram);
  });

  it('pin / remove / clear / copy-back work on the demo list', async () => {
    clips.set([e(3, 'pinned', true), e(2, 'b'), e(1, 'a')]);
    await togglePin(1);
    expect(get(clips).map((c) => c.content)).toEqual(['pinned', 'a', 'b']); // newly pinned joins the pinned group
    await removeClip(2);
    expect(get(clips).map((c) => c.id)).toEqual([3, 1]);
    await copyClip(1); // promoted, not duplicated
    expect(get(clips)).toHaveLength(2);
    expect(get(clips).find((c) => c.content === 'a')?.pinned).toBe(true);
    await clearClips();
    expect(get(clips).every((c) => c.pinned)).toBe(true);
  });

  it('demo transport: playpause freezes/resumes the position, next/previous cycle the queue', async () => {
    media.set(track({ title: demoMediaQueue[0].title, positionSec: 50, sampledAt: Date.now() - 4_000 }));
    await mediaTransport('playpause');
    const paused = get(media)!;
    expect(paused.playing).toBe(false);
    expect(paused.positionSec).toBeGreaterThanOrEqual(53.9); // the 4 s that had elapsed were kept
    await mediaTransport('next');
    expect(get(media)!.title).toBe(demoMediaQueue[1].title);
    expect(get(media)!.playing).toBe(false); // transport keeps the play state
    await mediaTransport('previous');
    await mediaTransport('previous');
    expect(get(media)!.title).toBe(demoMediaQueue[demoMediaQueue.length - 1].title);
  });

  it('transport with nothing playing is a no-op', async () => {
    media.set(null);
    await mediaTransport('next');
    expect(get(media)).toBeNull();
  });
});
