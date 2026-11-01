// Pure view-model helpers for the Island pill. Kept free of Svelte so they are unit-tested.

import type { ClipEntry } from '../lib/types';
import type { SystemStats } from '../lib/live';

export type PillMode = 'idle' | 'voice' | 'activity';

export const PILL_MIN_W = 212;
export const PILL_MAX_W = 400; // contract: pill <= 400x44 inside the 440x88 window
export const PILL_PAD_X = 6; // breathing room added around the measured content

export const PILL_H: Record<PillMode, number> = { idle: 36, voice: 38, activity: 42 };
/** Tallest pill state: morph progress is measured above this so pill-to-pill resizes read as 0. */
export const PILL_BASE_H = 44;

const GB = 1024 ** 3;
const TB = 1024 ** 4;

/** RAM used as an integer percent, or null while stats are unavailable. */
export function memPercent(s: SystemStats | null | undefined): number | null {
  if (!s || !s.memTotal) return null;
  return Math.min(100, Math.max(0, (s.memUsed / s.memTotal) * 100));
}

export function diskPercent(s: SystemStats | null | undefined): number | null {
  if (!s || !s.diskTotal) return null;
  return Math.min(100, Math.max(0, (s.diskUsed / s.diskTotal) * 100));
}

/** Free space on the system drive, formatted ("214 GB", "1.4 TB"). */
export function freeDisk(s: SystemStats | null | undefined): { value: number; unit: 'GB' | 'TB'; text: string } | null {
  if (!s || !s.diskTotal) return null;
  const free = Math.max(0, s.diskTotal - s.diskUsed);
  if (free >= TB) {
    const v = Math.round((free / TB) * 10) / 10;
    return { value: v, unit: 'TB', text: `${v.toFixed(1)} TB` };
  }
  const v = Math.round(free / GB);
  return { value: v, unit: 'GB', text: `${v} GB` };
}

/** Collapse whitespace and cut long copies to a one-line snippet. */
export function snippet(text: string, max = 36): string {
  const flat = text.replace(/\s+/g, ' ').trim();
  if (flat.length <= max) return flat;
  return flat.slice(0, Math.max(1, max - 1)).trimEnd() + '…';
}

/** The most recently created clip (pinned entries sort first in the store, so scan by time). */
export function latestClip(entries: ClipEntry[]): ClipEntry | null {
  let best: ClipEntry | null = null;
  for (const e of entries) if (!best || e.created_at > best.created_at) best = e;
  return best;
}

/** "03:07" / "1:02:09" for the pill countdown. */
export function clockText(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const p = (n: number) => String(n).padStart(2, '0');
  return h > 0 ? `${h}:${p(m)}:${p(s)}` : `${m}:${p(s)}`;
}

export function pillMode(opts: { voice: boolean; activity: boolean }): PillMode {
  if (opts.voice) return 'voice';
  if (opts.activity) return 'activity';
  return 'idle';
}

/** Pill size from the measured content width, clamped to the contract's envelope. */
export function pillSize(mode: PillMode, contentW: number): { w: number; h: number } {
  const w = Math.round(Math.min(PILL_MAX_W, Math.max(PILL_MIN_W, contentW + PILL_PAD_X * 2)));
  return { w, h: PILL_H[mode] };
}

/** Inner-shape corner radius: a pill is always fully round until the panel takes over. */
export function morphRadius(progress: number, w: number, h: number, pillH: number, panelR: number): number {
  const t = Math.min(1, Math.max(0, progress));
  const eased = 1 - Math.pow(1 - t, 2.2);
  const r = pillH / 2 + (panelR - pillH / 2) * eased;
  return Math.max(0, Math.min(r, h / 2, w / 2));
}

/** One-line description of a clip for the live-activity pill. */
export function clipLabel(c: ClipEntry): string {
  if (c.kind === 'Image') return 'Image';
  if (c.kind === 'Link') {
    try {
      const u = new URL(c.content);
      const path = u.pathname !== '/' ? u.pathname : '';
      return snippet(u.hostname.replace(/^www\./, '') + path, 40);
    } catch {
      /* not a parseable URL: fall through to the plain snippet */
    }
  }
  return snippet(c.content, 40);
}