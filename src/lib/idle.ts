// Idle pill companion: settings, registries and the "is the pill idle?" state machine
// (owner: personalization) — docs/CONTRACTS.md §7.
//
// This file is the SLOW part: it only ever holds ONE timer (the idle countdown) and it re-arms
// lazily, so `markActivity()` is a timestamp write and is safe to call from every hover/click.
// The eyes themselves (saccades, blinks, poses) live in src/components/idle/{engine,runner}.ts.
//
//   idleSettings   {enabled, style, personality, delayMs}  persisted, free, no locked styles
//   isIdle         true once `delayMs` passed with no activity (never while disabled/paused)
//   markActivity() the pill calls it on hover/click/pulse/shortcut/mode change; wakes instantly
//   startIdle()    idempotent; adds the safety-net listeners and the pause gates

import { get, writable, type Readable, type Writable } from 'svelte/store';

// ------------------------------------------------------------------------------ registries

export type IdleStyleId = 'dot' | 'capsule' | 'sleepy' | 'pixel' | 'visor' | 'ring';
export type PersonalityId = 'curious' | 'calm' | 'sleepy' | 'hyper';
/** A held "micro-behaviour". Between poses the eyes are neutral. */
export type PoseId = 'droop' | 'tilt' | 'happy' | 'perk' | 'doze';

export interface IdleStyleDef {
  id: IdleStyleId;
  name: string;
  blurb: string;
  /** how the gaze is rounded: 0 = smooth, otherwise steps per unit (Pixel snaps to its grid) */
  quantize: number;
  /** vertical scale the blink closes to */
  blinkTo: number;
}

export const IDLE_STYLES = [
  { id: 'dot', name: 'Dot', blurb: 'Round eyes with a little glint', quantize: 0, blinkTo: 0.08 },
  { id: 'capsule', name: 'Capsule', blurb: 'Tall rounded bars', quantize: 0, blinkTo: 0.1 },
  { id: 'sleepy', name: 'Sleepy', blurb: 'Heavy-lidded and a little tired', quantize: 0, blinkTo: 0.12 },
  { id: 'pixel', name: 'Pixel', blurb: 'Blocky retro eyes that snap to a grid', quantize: 2, blinkTo: 0.2 },
  { id: 'visor', name: 'Visor', blurb: 'One band with a sweeping core', quantize: 0, blinkTo: 0.18 },
  { id: 'ring', name: 'Ring', blurb: 'Hollow rings, roaming pupils', quantize: 0, blinkTo: 0.08 },
] as const satisfies readonly IdleStyleDef[];

export interface Personality {
  id: PersonalityId;
  name: string;
  blurb: string;
  /** ms between gaze shifts [min, max] */
  saccade: readonly [number, number];
  /** ms between blinks */
  blink: readonly [number, number];
  /** ms between micro-behaviours (measured from the end of the last one) */
  micro: readonly [number, number];
  /** chance that a blink is a double blink */
  doubleBlink: number;
  /** 0..1: how far the eyes travel */
  range: number;
  /** chance that a gaze shift just returns to centre */
  centerBias: number;
  /** ms a gaze shift takes (the css transition) */
  lookMs: number;
  /** ms a blink takes */
  blinkMs: number;
  /** attention span: with nothing to look at for this long the eyes close and stay closed (free) */
  dozeAfterMs: number;
  /** relative weights of the poses this personality shows */
  poses: Partial<Record<PoseId, number>>;
}

export const PERSONALITIES = [
  {
    id: 'curious', name: 'Curious', blurb: 'Looks around a lot, tilts its head',
    saccade: [1500, 3800], blink: [3000, 7000], micro: [9000, 17000],
    doubleBlink: 0.1, range: 1, centerBias: 0.18, lookMs: 84, blinkMs: 90, dozeAfterMs: 60_000,
    poses: { tilt: 5, perk: 2, happy: 2, droop: 0.5 },
  },
  {
    id: 'calm', name: 'Calm', blurb: 'Slow, settled, the odd contented squint',
    saccade: [3200, 7500], blink: [4000, 9000], micro: [14000, 26000],
    doubleBlink: 0.05, range: 0.6, centerBias: 0.45, lookMs: 140, blinkMs: 110, dozeAfterMs: 90_000,
    poses: { happy: 4, droop: 2, tilt: 1 },
  },
  {
    id: 'sleepy', name: 'Sleepy', blurb: 'Slow blinks, drooping, dozing off',
    saccade: [4000, 9000], blink: [2500, 5500], micro: [7000, 15000],
    doubleBlink: 0, range: 0.45, centerBias: 0.5, lookMs: 196, blinkMs: 180, dozeAfterMs: 25_000,
    poses: { droop: 6, doze: 4, happy: 1 },
  },
  {
    id: 'hyper', name: 'Hyper', blurb: 'Quick darts, double blinks, little hops',
    saccade: [800, 2000], blink: [1800, 4200], micro: [5000, 11000],
    doubleBlink: 0.3, range: 1, centerBias: 0.1, lookMs: 56, blinkMs: 72, dozeAfterMs: 40_000,
    poses: { perk: 5, tilt: 3, happy: 3 },
  },
] as const satisfies readonly Personality[];

export const styleById = (id: IdleStyleId): (typeof IDLE_STYLES)[number] =>
  IDLE_STYLES.find((s) => s.id === id) ?? IDLE_STYLES[0];
export const personalityById = (id: PersonalityId): (typeof PERSONALITIES)[number] =>
  PERSONALITIES.find((p) => p.id === id) ?? PERSONALITIES[0];

// ------------------------------------------------------------------------------ settings

export interface IdleSettings {
  enabled: boolean;
  style: IdleStyleId;
  personality: PersonalityId;
  /** ms of inactivity before the companion appears */
  delayMs: number;
}

export const IDLE_DELAY_DEFAULT = 5500;
export const IDLE_DELAY_MIN = 1500;
export const IDLE_DELAY_MAX = 60_000;

export const DEFAULT_IDLE: Readonly<IdleSettings> = {
  enabled: true,
  style: 'dot',
  personality: 'curious',
  delayMs: IDLE_DELAY_DEFAULT,
};

const isStyle = (v: unknown): v is IdleStyleId => IDLE_STYLES.some((s) => s.id === v);
const isPersonality = (v: unknown): v is PersonalityId => PERSONALITIES.some((p) => p.id === v);

/** Whatever is in storage (or a bad patch) becomes a valid settings object; never throws. */
export function sanitizeIdleSettings(raw: unknown): IdleSettings {
  const o = raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : {};
  const d = typeof o.delayMs === 'number' && Number.isFinite(o.delayMs) ? o.delayMs : DEFAULT_IDLE.delayMs;
  return {
    enabled: typeof o.enabled === 'boolean' ? o.enabled : DEFAULT_IDLE.enabled,
    style: isStyle(o.style) ? o.style : DEFAULT_IDLE.style,
    personality: isPersonality(o.personality) ? o.personality : DEFAULT_IDLE.personality,
    delayMs: Math.round(Math.min(IDLE_DELAY_MAX, Math.max(IDLE_DELAY_MIN, d))),
  };
}

export const IDLE_KEY = 'krovi.idle';

export type StorageLike = Pick<Storage, 'getItem' | 'setItem'>;

function getStorage(): StorageLike | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

export function loadIdleSettings(storage: StorageLike | null): IdleSettings {
  if (!storage) return { ...DEFAULT_IDLE };
  try {
    const raw = storage.getItem(IDLE_KEY);
    return raw == null ? { ...DEFAULT_IDLE } : sanitizeIdleSettings(JSON.parse(raw));
  } catch {
    return { ...DEFAULT_IDLE };
  }
}

export function saveIdleSettings(storage: StorageLike | null, s: IdleSettings): boolean {
  if (!storage) return false;
  try {
    storage.setItem(IDLE_KEY, JSON.stringify(s));
    return true;
  } catch {
    return false;
  }
}

function createSettingsStore(): Writable<IdleSettings> {
  const inner = writable<IdleSettings>(loadIdleSettings(getStorage()));
  const set = (v: IdleSettings) => {
    const next = sanitizeIdleSettings(v);
    inner.set(next);
    saveIdleSettings(getStorage(), next);
  };
  return { subscribe: inner.subscribe, set, update: (fn) => set(fn(get(inner))) };
}

/** Persisted companion settings. Every field is free: there are no locked styles. */
export const idleSettings: Writable<IdleSettings> = createSettingsStore();

// ------------------------------------------------------------------------------ state machine

export interface TimerApi {
  now(): number;
  set(fn: () => void, ms: number): unknown;
  clear(handle: unknown): void;
}

export interface IdleMachine {
  /** Apply settings. Disabled = never idle. */
  configure(c: { enabled: boolean; delayMs: number }): void;
  /** External gate (hidden window, reduced motion). Paused = never idle; resuming restarts the delay. */
  setPaused(paused: boolean): void;
  /** Anything happened. Wakes instantly; otherwise just stamps the time. */
  activity(): void;
  readonly idle: boolean;
  dispose(): void;
}

/**
 * The idle countdown. Pure apart from the injected timers, so it is tested with fake ones.
 *
 * It keeps at most ONE timer. `activity()` while counting down only stamps `last`; when the timer
 * fires and finds activity since it was armed, it re-arms for the remainder. A burst of hover
 * events therefore costs one assignment each, not a clear/set pair.
 */
export function createIdleMachine(api: TimerApi, onChange: (idle: boolean) => void): IdleMachine {
  let enabled = false;
  let paused = false;
  let delay = IDLE_DELAY_DEFAULT;
  let idle = false;
  let last = api.now();
  let handle: unknown = null;

  const eligible = () => enabled && !paused;
  const setIdle = (v: boolean) => {
    if (idle === v) return;
    idle = v;
    onChange(v);
  };
  const clear = () => {
    if (handle !== null) api.clear(handle);
    handle = null;
  };
  const arm = (ms: number) => {
    clear();
    handle = api.set(fire, Math.max(0, ms));
  };
  function fire() {
    handle = null;
    if (!eligible() || idle) return;
    const left = last + delay - api.now();
    if (left > 0) arm(left);
    else setIdle(true);
  }

  return {
    configure(c) {
      const was = eligible();
      enabled = c.enabled;
      delay = Math.min(IDLE_DELAY_MAX, Math.max(IDLE_DELAY_MIN, c.delayMs));
      if (!was && eligible()) last = api.now(); // switching it on counts as activity: a fresh delay
      if (!eligible()) {
        clear();
        setIdle(false);
      } else if (!idle) {
        arm(last + delay - api.now());
      }
    },
    setPaused(p) {
      if (paused === p) return;
      paused = p;
      if (p) {
        clear();
        setIdle(false);
      } else {
        last = api.now();
        if (eligible()) arm(delay);
      }
    },
    activity() {
      last = api.now();
      if (idle) setIdle(false);
      if (eligible() && handle === null) arm(delay);
    },
    get idle() {
      return idle;
    },
    dispose() {
      clear();
      idle = false;
    },
  };
}

// ------------------------------------------------------------------------------ the live instance

const _idle = writable<boolean>(false);
/** True while the pill should show the companion. */
export const isIdle: Readable<boolean> = { subscribe: _idle.subscribe };

const machine = createIdleMachine(
  { now: () => Date.now(), set: (fn, ms) => setTimeout(fn, ms), clear: (h) => clearTimeout(h as number) },
  (v) => _idle.set(v),
);

let stop: (() => void) | null = null;

/**
 * Wire the machine to the page. Idempotent; the companion and `markActivity()` both call it, so
 * nothing has to remember to. Safe where there is no DOM (it just does nothing).
 *
 *  - the companion is paused while the window is hidden and while the user prefers reduced motion
 *  - keydown / pointerdown anywhere count as activity (a safety net next to the pill's own calls)
 */
export function startIdle(): () => void {
  if (stop) return stop;
  if (typeof document === 'undefined' || typeof window === 'undefined') return () => {};

  const off = idleSettings.subscribe((s) => machine.configure({ enabled: s.enabled, delayMs: s.delayMs }));
  const reduced = typeof matchMedia === 'function' ? matchMedia('(prefers-reduced-motion: reduce)') : null;
  const gate = () => machine.setPaused(document.hidden || !!reduced?.matches);
  const wake = () => machine.activity();

  document.addEventListener('visibilitychange', gate);
  reduced?.addEventListener('change', gate);
  window.addEventListener('keydown', wake, { capture: true, passive: true });
  window.addEventListener('pointerdown', wake, { capture: true, passive: true });
  gate();

  const mine = () => {
    off();
    document.removeEventListener('visibilitychange', gate);
    reduced?.removeEventListener('change', gate);
    window.removeEventListener('keydown', wake, true);
    window.removeEventListener('pointerdown', wake, true);
    machine.dispose();
    _idle.set(false);
    if (stop === mine) stop = null;
  };
  stop = mine;
  return mine;
}

/**
 * Something happened on (or to) the pill: hover, click, a copy pulse, a shortcut, a mode change.
 * Cheap enough to call on every pointer event.
 */
export function markActivity(): void {
  startIdle();
  machine.activity();
}
