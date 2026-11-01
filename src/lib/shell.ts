// Island shell API (owner: panel-shell) — docs/CONTRACTS.md §1.
//
// Two backends behind ONE API:
//   * Tauri: thin invoke/listen wrappers over src-tauri/src/shell.rs. Rust owns the
//     window; this module only relays and keeps the `shellMode` store in sync.
//   * Browser demo (`isTauri() === false`): the same API as pure state, so the website
//     demo animates exactly like the native app without any backend.
//
// Guarantees (so a slow or broken UI can never wedge the window):
//   * `shellMode` always reflects what Rust actually applied (shell-mode events win).
//   * A `collapse` that the UI never commits is committed here after COLLAPSE_GUARD_MS,
//     and Rust's own 1500ms watchdog sits behind that.
//   * With no UI listener registered, Rust-initiated requests are answered immediately.
import { writable, type Readable } from 'svelte/store';
import { isTauri } from './bridge';

export type ShellMode = 'pill' | 'panel' | 'hidden';
export type ShellAction = 'expand' | 'collapse' | 'toggle';
export interface ShellConfig {
  /** Accelerator string, e.g. "Ctrl+Alt+Space". */
  shortcut: string;
  showPill: boolean;
  collapseOnBlur: boolean;
  launchAtLogin: boolean;
  /** Hide the pill while a fullscreen app (game, video, slides) is in the foreground. */
  hideInFullscreen: boolean;
}
export interface HitRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

// Geometry — MUST match src-tauri/src/shell.rs and docs/CONTRACTS.md §1.
export const PILL_WIN = { w: 440, h: 88 };
export const PANEL_WIN = { w: 960, h: 690 };
export const PANEL = { w: 900, h: 620 };
export const ISLAND_PAD = 10;

/** Commit a collapse ourselves if the UI hasn't within this long (Rust watchdog is 1500ms). */
const COLLAPSE_GUARD_MS = 1200;

const _mode = writable<ShellMode>('pill');
let current: ShellMode = 'pill';
_mode.subscribe((m) => (current = m));
/** Readable store: what the window currently is. */
export const shellMode: Readable<ShellMode> = { subscribe: _mode.subscribe };

const listeners = new Set<(a: ShellAction) => void>();
const toolListeners = new Set<(tool: string) => void>();

export function isIsland(): boolean {
  return isTauri();
}

// ---------------------------------------------------------------------------
// Tauri plumbing
// ---------------------------------------------------------------------------

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const api = await import('@tauri-apps/api/core');
  return api.invoke<T>(cmd, args);
}

let wired: Promise<void> | null = null;
function wire(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  if (!wired) {
    wired = (async () => {
      const ev = await import('@tauri-apps/api/event');
      await ev.listen<{ mode: ShellMode }>('shell-mode', (e) => {
        const m = e.payload?.mode;
        if (m === 'pill' || m === 'panel' || m === 'hidden') {
          collapsing = false;
          _mode.set(m);
        }
      });
      await ev.listen<{ action: ShellAction }>('shell-request', (e) => handleRequest(e.payload?.action));
      await ev.listen<{ tool: string }>('shell-open-tool', (e) => {
        const t = e.payload?.tool;
        if (t) toolListeners.forEach((l) => l(t));
      });
      try {
        // Catch up on anything emitted before we were listening.
        const m = await invoke<ShellMode>('shell_get_mode');
        if (m === 'pill' || m === 'panel' || m === 'hidden') _mode.set(m);
      } catch {
        /* Rust will push shell-mode on the next transition */
      }
    })().catch((err) => {
      console.warn('[shell] event wiring failed', err);
    });
  }
  return wired;
}
if (isTauri()) void wire();

/** A Rust-initiated request (shortcut, tray, focus loss, second launch). */
function handleRequest(action: ShellAction | undefined): void {
  if (!action) return;
  if (listeners.size === 0) {
    // No UI to animate: answer straight away so Rust doesn't wait for its watchdog.
    if (action === 'expand') void commitMode('panel');
    else if (action === 'collapse') void commitMode('pill');
    else void commitMode(current === 'panel' ? 'pill' : 'panel');
    return;
  }
  if (action === 'collapse') armCollapseGuard();
  listeners.forEach((l) => l(action));
}

let collapsing = false;
function armCollapseGuard(): void {
  collapsing = true;
  setTimeout(() => {
    if (collapsing && current === 'panel') {
      collapsing = false;
      void commitMode('pill');
    }
  }, COLLAPSE_GUARD_MS);
}

/** Subscribe to Rust/UI-initiated requests. The UI animates, then calls `commitMode`. */
export function onShellRequest(cb: (a: ShellAction) => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** Tray "Settings…" etc. ask the UI to open a tool (`shell-open-tool`). */
export function onOpenTool(cb: (tool: string) => void): () => void {
  toolListeners.add(cb);
  return () => {
    toolListeners.delete(cb);
  };
}

// ---------------------------------------------------------------------------
// Mode API
// ---------------------------------------------------------------------------

async function applyMode(mode: ShellMode): Promise<void> {
  if (!isTauri()) {
    collapsing = false;
    _mode.set(mode);
    return;
  }
  await wire();
  try {
    // Rust returns the mode it really applied ('pill' becomes 'hidden' if the user
    // turned the pill off).
    const applied = await invoke<ShellMode>('shell_set_mode', { mode });
    collapsing = false;
    _mode.set(applied === 'pill' || applied === 'panel' || applied === 'hidden' ? applied : mode);
  } catch (err) {
    console.warn('[shell] shell_set_mode failed', err);
  }
}

/** UI wants the panel: grows the window first (awaits Rust), then the UI animates. */
export async function requestExpand(): Promise<void> {
  await applyMode('panel');
}

/**
 * UI wants the pill. Listeners (the Island) get a 'collapse' action, play the
 * ≈420ms panel→pill morph, then call `commitMode('pill')`. With no listener — or if
 * the listener never commits — this module commits itself.
 */
export async function requestCollapse(): Promise<void> {
  if (current !== 'panel' || collapsing) return;
  if (listeners.size === 0) {
    await applyMode('pill');
    return;
  }
  armCollapseGuard();
  listeners.forEach((l) => l('collapse'));
}

/** Called by the UI when its animation has finished. */
export async function commitMode(mode: ShellMode): Promise<void> {
  await applyMode(mode);
}

// ---------------------------------------------------------------------------
// Click-through
// ---------------------------------------------------------------------------

let lastHit = '';
let hitInFlight = false;
let hitQueued: HitRect | null = null;

async function sendHit(r: HitRect): Promise<void> {
  hitInFlight = true;
  try {
    await invoke('shell_set_hit_rect', { rect: r });
  } catch {
    /* non-fatal: Rust falls back to the contract default rect */
  } finally {
    hitInFlight = false;
    if (hitQueued) {
      const next = hitQueued;
      hitQueued = null;
      void sendHit(next);
    }
  }
}

/**
 * Report the visible shape's bounding rect (window-relative logical px) so clicks on
 * the transparent area pass through. Coalesced and de-duplicated: safe to call from a
 * ResizeObserver or every animation frame during a morph.
 */
export async function setHitRect(r: HitRect): Promise<void> {
  if (!isTauri()) return;
  const n = {
    x: Math.floor(r.x),
    y: Math.floor(r.y),
    w: Math.ceil(r.x + r.w) - Math.floor(r.x),
    h: Math.ceil(r.y + r.h) - Math.floor(r.y),
  };
  const key = `${n.x},${n.y},${n.w},${n.h}`;
  if (key === lastHit) return;
  lastHit = key;
  if (hitInFlight) {
    hitQueued = n;
    return;
  }
  await sendHit(n);
}

// ---------------------------------------------------------------------------
// Quit / hold / config
// ---------------------------------------------------------------------------

/** Quit the whole app (tray, panel header, Ctrl+Q). No-op in the browser demo. */
export async function quit(): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('shell_quit');
  } catch (err) {
    console.warn('[shell] shell_quit failed', err);
  }
}

/** Suppress collapse-on-blur while a native dialog is open. */
export async function hold(b: boolean): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('shell_hold', { hold: b });
  } catch {
    /* non-fatal */
  }
}

const DEMO_DEFAULT: ShellConfig = {
  shortcut: 'Ctrl+Alt+Space',
  showPill: true,
  collapseOnBlur: true,
  launchAtLogin: false,
  hideInFullscreen: true,
};
let demoConfig: ShellConfig = { ...DEMO_DEFAULT };

export async function getConfig(): Promise<ShellConfig> {
  if (!isTauri()) return { ...demoConfig };
  return { ...DEMO_DEFAULT, ...(await invoke<ShellConfig>('shell_get_config')) };
}

/**
 * Validate + apply. Rejects with a human-readable message when the shortcut is
 * invalid or taken (the previous shortcut keeps working).
 */
export async function setConfig(c: ShellConfig): Promise<ShellConfig> {
  if (!isTauri()) {
    const s = c.shortcut.trim();
    const parts = s.split('+').map((p) => p.trim()).filter(Boolean);
    const mods = parts.slice(0, -1).filter((p) => /^(ctrl|control|alt|shift|win|super|cmd)$/i.test(p));
    if (parts.length < 2 || mods.length !== parts.length - 1) {
      throw new Error(`'${s}' needs a modifier (Ctrl, Alt, Shift or Win) and a key.`);
    }
    demoConfig = { ...c, shortcut: s };
    return { ...demoConfig };
  }
  try {
    return await invoke<ShellConfig>('shell_set_config', { config: c });
  } catch (err) {
    // Tauri rejects with the Rust error string; surface it as a proper Error.
    throw err instanceof Error ? err : new Error(String(err));
  }
}
