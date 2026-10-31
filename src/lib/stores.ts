// Small shared UI state. Plain `svelte/store` writables keep this usable from
// both components and non-component modules, and work identically in demo and
// Tauri builds. Theme is persisted per-viewer via localStorage (best-effort).

import { writable } from 'svelte/store';
import type { ToolId } from './types';

export type ThemePref = 'light' | 'dark' | 'auto';

const THEME_KEY = 'krovi.theme';

function readTheme(): ThemePref {
  try {
    const v = localStorage.getItem(THEME_KEY);
    if (v === 'light' || v === 'dark' || v === 'auto') return v;
  } catch {
    /* private mode / blocked storage — fall through */
  }
  return 'auto';
}

export const themePref = writable<ThemePref>(readTheme());

themePref.subscribe((v) => {
  try {
    localStorage.setItem(THEME_KEY, v);
  } catch {
    /* ignore */
  }
  applyTheme(v);
});

/** Resolve `auto` against the OS preference and set data-theme on <html>. */
export function applyTheme(pref: ThemePref): void {
  if (typeof document === 'undefined') return;
  let resolved: 'light' | 'dark' = 'light';
  if (pref === 'auto') {
    const prefersDark =
      typeof matchMedia === 'function' && matchMedia('(prefers-color-scheme: dark)').matches;
    resolved = prefersDark ? 'dark' : 'light';
  } else {
    resolved = pref;
  }
  document.documentElement.setAttribute('data-theme', resolved);
}

export const activeTool = writable<ToolId>('home');
export const paletteOpen = writable<boolean>(false);
export const pinned = writable<boolean>(false);

/** A shared 1-second clock tick (epoch ms) so clocks/timers update together. */
export const now = writable<number>(Date.now());

let ticking = false;
export function startClock(): void {
  if (ticking || typeof window === 'undefined') return;
  ticking = true;
  setInterval(() => now.set(Date.now()), 1000);
}
