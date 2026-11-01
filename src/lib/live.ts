// Live data stores (owner: live-data) — docs/CONTRACTS.md §2.
//
// The UI never polls `invoke` for these: a Rust background hub pushes events and this
// module turns them into Svelte stores.
//   * TAURI: listen for `clipboard-changed` / `media-changed` / `stats-tick`, then seed
//     from `live_snapshot` (listeners first, so nothing emitted in between is lost; a
//     snapshot never overwrites a store an event has already updated).
//   * DEMO (plain browser): fixtures plus a simulated copy every ~12 s, a gently
//     random-walking RAM/CPU, and a media item whose position ticks — so the website
//     demo feels alive. Demo data never exists on the Tauri path.

import { get, writable } from 'svelte/store';
import type { ClipboardChanged, ClipEntry, ClipKind, LiveSnapshot, MediaState, SystemStats } from './types';
import { invoke, isTauri, listen } from './bridge';
import { visible } from './clip';
import * as fx from './fixtures';

export type { MediaState, SystemStats } from './types';

/** Clipboard history in display order (pinned first, then newest first). */
export const clips = writable<ClipEntry[]>([]);
/** What the OS says is playing, or null. Interpolate the position with `mediaPosition`. */
export const media = writable<MediaState | null>(null);
export const stats = writable<SystemStats | null>(null);
/** Increments on every NEW clipboard entry — drives the pill's arrival glow. */
export const pulse = writable<number>(0);

// ---------------------------------------------------------------------------
// Pure helpers (unit-tested)
// ---------------------------------------------------------------------------

/**
 * Where playback is now. The backend samples the position at `sampledAt`; while
 * playing, the UI advances it by the wall time since then (clamped to the duration).
 */
export function mediaPosition(m: MediaState | null, nowMs: number): number {
  if (!m) return 0;
  let pos = m.positionSec;
  if (m.playing && nowMs > m.sampledAt) pos += (nowMs - m.sampledAt) / 1000;
  if (m.durationSec > 0) pos = Math.min(pos, m.durationSec);
  return Math.max(0, pos);
}

/** `Link` for a single http(s)/ftp/mailto/www token, else `Text` (mirrors krovi-sys). */
export function classifyText(s: string): ClipKind {
  const t = s.trim();
  if (!t || /\s/.test(t)) return 'Text';
  return /^(https?:\/\/|ftps?:\/\/|mailto:)\S+/i.test(t) || /^www\.[^.\s]+\.\S+/i.test(t) ? 'Link' : 'Text';
}

/** Unpinned entries kept, like `krovi_core::DEFAULT_CAP`. */
export const CLIP_CAP = 200;

/**
 * Add (or promote) a copy the way `ClipboardHistory::add` does: the same kind+content
 * is promoted to the front (keeping its pin, getting a fresh id and time) instead of
 * duplicated; the oldest unpinned entries beyond `CLIP_CAP` fall off. `list` and the
 * result are in display order (pinned first, then newest first).
 */
export function addClip(
  list: ClipEntry[],
  content: string,
  now: number,
  kind: ClipKind = classifyText(content),
): { list: ClipEntry[]; id: number } {
  const dup = list.find((e) => e.kind === kind && e.content === content);
  const id = list.reduce((m, e) => Math.max(m, e.id), 0) + 1;
  const entry: ClipEntry = { id, kind, content, pinned: dup?.pinned ?? false, created_at: now };
  const next = visible([entry, ...list.filter((e) => e !== dup)]);
  let unpinned = next.filter((e) => !e.pinned).length;
  const kept = [...next];
  for (let i = kept.length - 1; i >= 0 && unpinned > CLIP_CAP; i--) {
    if (!kept[i].pinned) {
      kept.splice(i, 1);
      unpinned--;
    }
  }
  return { list: kept, id };
}

/** One step of the demo RAM/CPU random walk. `rnd` is a [0,1) source (injectable for tests). */
export function nextDemoStats(prev: SystemStats, rnd: () => number = Math.random): SystemStats {
  const GB = 1024 ** 3;
  const memMin = 0.35 * prev.memTotal;
  const memMax = 0.85 * prev.memTotal;
  const memUsed = Math.min(memMax, Math.max(memMin, prev.memUsed + (rnd() - 0.5) * 0.3 * GB));
  const cpu = Math.min(60, Math.max(2, prev.cpu + (rnd() - 0.5) * 14));
  return { ...prev, memUsed, cpu: Math.round(cpu * 10) / 10 };
}

/** Demo media: resample the position at `now`, wrapping to the next queue item at the end. */
export function advanceDemoMedia(m: MediaState, now: number): MediaState {
  const pos = mediaPosition(m, now);
  if (m.playing && m.durationSec > 0 && pos >= m.durationSec - 0.5) {
    const i = fx.demoMediaQueue.findIndex((t) => t.title === m.title);
    const next = fx.demoMediaQueue[(i + 1) % fx.demoMediaQueue.length];
    return { ...next, playing: true, positionSec: 0, sampledAt: now };
  }
  return { ...m, positionSec: pos, sampledAt: now };
}

// ---------------------------------------------------------------------------
// startLive
// ---------------------------------------------------------------------------

let started = false;
let stopFns: Array<() => void> = [];

/** Idempotent. Call once at app start (App.svelte does). */
export function startLive(): void {
  if (started) return;
  started = true;
  if (isTauri()) void startNative();
  else startDemo();
}

/** Tear everything down and allow `startLive()` again (tests; hot reload). */
export function stopLive(): void {
  for (const f of stopFns) f();
  stopFns = [];
  started = false;
}

async function startNative(): Promise<void> {
  let gotClip = false;
  let gotMedia = false;
  let gotStats = false;
  try {
    const offs = await Promise.all([
      listen<ClipboardChanged>('clipboard-changed', (p) => {
        gotClip = true;
        clips.set(p.entries);
        if (p.newId !== null && p.newId !== undefined) pulse.update((n) => n + 1);
      }),
      listen<MediaState | null>('media-changed', (p) => {
        gotMedia = true;
        media.set(p);
      }),
      listen<SystemStats>('stats-tick', (p) => {
        gotStats = true;
        stats.set(p);
      }),
    ]);
    stopFns.push(...offs);
    const snap = await invoke<LiveSnapshot>('live_snapshot');
    if (!gotClip) clips.set(snap.clipboard);
    if (!gotMedia) media.set(snap.media);
    if (!gotStats && snap.stats) stats.set(snap.stats);
  } catch (e) {
    console.error('[krovi] the live feed failed to start', e);
    stopLive(); // allow a retry on the next startLive()
  }
}

function startDemo(): void {
  clips.set(visible(structuredClone(fx.clipboardFixture)));
  stats.set({ ...fx.statsFixture });
  const first = fx.demoMediaQueue[0];
  media.set({ ...first, playing: true, positionSec: 72, sampledAt: Date.now() });

  let snippet = 0;
  const timers = [
    // a simulated copy every ~12 s
    setInterval(() => {
      const content = fx.demoCopySnippets[snippet++ % fx.demoCopySnippets.length];
      const { list } = addClip(get(clips), content, Date.now());
      clips.set(list);
      pulse.update((n) => n + 1);
    }, 12_000),
    // RAM/CPU wander every 2 s, like the real stats-tick
    setInterval(() => stats.update((s) => (s ? nextDemoStats(s) : s)), 2_000),
    // re-anchor the media position every 5 s, like the real resync
    setInterval(() => media.update((m) => (m ? advanceDemoMedia(m, Date.now()) : m)), 5_000),
  ];
  stopFns.push(() => timers.forEach((t) => clearInterval(t)));
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/** Write an entry back to the OS clipboard and promote it (never duplicates it). */
export async function copyClip(id: number): Promise<void> {
  if (isTauri()) {
    clips.set(await invoke<ClipEntry[]>('clipboard_copy', { id }));
    return;
  }
  const entry = get(clips).find((e) => e.id === id);
  if (!entry) return;
  try {
    await navigator.clipboard?.writeText(entry.content);
  } catch {
    /* clipboard permission denied in the demo: promotion below still happens */
  }
  clips.set(addClip(get(clips), entry.content, Date.now(), entry.kind).list);
}

export async function togglePin(id: number): Promise<void> {
  if (isTauri()) {
    clips.set(await invoke<ClipEntry[]>('toggle_clip_pin', { id }));
    return;
  }
  clips.update((list) => visible(list.map((e) => (e.id === id ? { ...e, pinned: !e.pinned } : e))));
}

export async function removeClip(id: number): Promise<void> {
  if (isTauri()) {
    clips.set(await invoke<ClipEntry[]>('remove_clip', { id }));
    return;
  }
  clips.update((list) => list.filter((e) => e.id !== id));
}

/** Remove every unpinned entry (pinned ones stay). */
export async function clearClips(): Promise<void> {
  if (isTauri()) {
    clips.set(await invoke<ClipEntry[]>('clipboard_clear'));
    return;
  }
  clips.update((list) => list.filter((e) => e.pinned));
}

export type TransportCmd = 'playpause' | 'next' | 'previous';

export async function mediaTransport(cmd: TransportCmd): Promise<void> {
  if (isTauri()) {
    await invoke<void>('media_transport', { command: cmd });
    return; // the media-changed event carries the result
  }
  media.update((m) => {
    if (!m) return m;
    const now = Date.now();
    if (cmd === 'playpause') return { ...m, playing: !m.playing, positionSec: mediaPosition(m, now), sampledAt: now };
    const q = fx.demoMediaQueue;
    const i = Math.max(0, q.findIndex((t) => t.title === m.title));
    const next = q[(i + (cmd === 'next' ? 1 : q.length - 1)) % q.length];
    return { ...next, playing: m.playing, positionSec: 0, sampledAt: now };
  });
}
