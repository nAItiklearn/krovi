// Drives one companion element from the pure engine. DOM-side only:
//   gaze   -> two CSS variables (--gx/--gy, -1..1); the stylesheet owns the transition
//   pose   -> `data-pose` on the root; the stylesheet owns the transforms
//   blink  -> one Web Animation per eyelid (compositor-only transform, no timer needed)
//
// Exactly ONE timer is ever alive: the next engine deadline. Nothing here runs per frame.
// It stands down by itself while the document is hidden and does not start under
// prefers-reduced-motion (the eyes then simply stay still).

import type { IdleStyleDef, Personality } from '../../lib/idle';
import { cursorGaze, initSchedule, noteCursor, step, type Action, type Rand, type Schedule } from './engine';

export interface CursorEvent {
  dx: number;
  dy: number;
  dist?: number;
}

interface LidLike {
  animate?(keyframes: Keyframe[], options: KeyframeAnimationOptions): unknown;
  getAnimations?(): { cancel(): void }[];
}

/** The slice of HTMLElement the runner touches (so it can be tested with a plain object). */
export interface CompanionEl {
  style: { setProperty(name: string, value: string): void; removeProperty(name: string): unknown };
  dataset: Record<string, string | undefined>;
  querySelectorAll(selector: string): ArrayLike<LidLike>;
}

export interface RunnerEnv {
  now(): number;
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
  rand: Rand;
  /** true when the window is hidden */
  hidden(): boolean;
  reducedMotion(): boolean;
  onVisibility(cb: () => void): () => void;
}

export const realEnv: RunnerEnv = {
  now: () => performance.now(),
  setTimeout: (fn, ms) => setTimeout(fn, ms),
  clearTimeout: (h) => clearTimeout(h as number),
  rand: Math.random,
  hidden: () => typeof document !== 'undefined' && document.hidden,
  reducedMotion: () => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches,
  onVisibility: (cb) => {
    if (typeof document === 'undefined') return () => {};
    document.addEventListener('visibilitychange', cb);
    return () => document.removeEventListener('visibilitychange', cb);
  },
};

export type StyleTraits = Pick<IdleStyleDef, 'quantize' | 'blinkTo'>;

export interface Runner {
  /** Feed a `shell-cursor` event (≤10 Hz). Ignored while stopped/paused. */
  cursor(ev: CursorEvent): void;
  stop(): void;
}

const NOOP: Runner = { cursor() {}, stop() {} };
/** Cursor events closer together than this are dropped (the Rust side already caps at 10 Hz). */
const CURSOR_MIN_GAP_MS = 80;

const quant = (v: number, q: number): number => (q > 0 ? Math.round(v * q) / q : v);

export function runCompanion(
  el: CompanionEl,
  read: () => { personality: Personality; style: StyleTraits },
  env: RunnerEnv = realEnv,
): Runner {
  if (env.reducedMotion()) return NOOP;

  let timer: unknown = null;
  let sched: Schedule | null = null;
  let stopped = false;
  let lastCursor = -Infinity;

  const setGaze = (x: number, y: number, ms: number) => {
    const q = read().style.quantize;
    el.style.setProperty('--gx', quant(x, q).toFixed(3));
    el.style.setProperty('--gy', quant(y, q).toFixed(3));
    // A glance is a ballistic dart: one drawn frame per ~60 ms of its length (84 ms = a single
    // jump). Each drawn frame costs the GPU process ~4 ms, so frames are the budget, and a
    // compositor animation also keeps ticking for as long as it is alive, so it is kept short.
    const steps = Math.max(1, Math.round(ms / 60));
    el.style.setProperty('--look-steps', String(steps));
    el.style.setProperty('--look-ms', `${steps === 1 ? 16 : Math.round(ms)}ms`);
  };

  // ONE Web Animation per lid per blink, transform only, STEPPED: open, shut, half, open — three
  // drawn frames instead of one per vsync (a classic limited-animation blink). A double blink is a
  // single animation with two dips (two overlapping animations on one target cannot be composited).
  const blink = (p: Personality, style: StyleTraits, double: boolean) => {
    const lids = el.querySelectorAll('.lid');
    const half = `scaleY(${(1 + style.blinkTo) / 2})`;
    const shut = `scaleY(${style.blinkTo})`;
    const hold = 'steps(1, jump-end)'; // keep each state until the next keyframe
    const dip = (at: number, w: number): Keyframe[] => [
      { transform: 'scaleY(1)', offset: at, easing: hold },
      { transform: shut, offset: at + w * 0.2, easing: hold },
      { transform: half, offset: at + w * 0.7, easing: hold },
    ];
    const frames: Keyframe[] = double
      ? [...dip(0, 0.45), ...dip(0.55, 0.45), { transform: 'scaleY(1)', offset: 1 }]
      : [...dip(0, 1), { transform: 'scaleY(1)', offset: 1 }];
    const duration = double ? Math.round(p.blinkMs * 2.2) : p.blinkMs;
    for (let i = 0; i < lids.length; i++) lids[i].animate?.(frames, { duration, easing: 'linear' });
  };

  // The eyes open when they appear. A Web Animation (not a css @keyframes) so the lid only ever has
  // WAAPI animations, which keeps every one of them compositor-eligible.
  const wake = () => {
    const lids = el.querySelectorAll('.lid');
    for (let i = 0; i < lids.length; i++) {
      lids[i].animate?.([{ transform: 'scaleY(0.06)' }, { transform: 'scaleY(1)' }], {
        duration: 280,
        easing: 'steps(6, jump-end)',
      });
    }
  };

  const apply = (actions: Action[]) => {
    const { personality: p, style } = read();
    for (const a of actions) {
      if (a.type === 'gaze') setGaze(a.x, a.y, p.lookMs);
      else if (a.type === 'blink') blink(p, style, a.double);
      else if (a.type === 'pose') {
        el.dataset.pose = a.pose;
        el.style.setProperty('--side', String(a.side));
      } else delete el.dataset.pose;
    }
  };

  const schedule = () => {
    if (!sched) return;
    const at = Math.min(sched.nextGaze, sched.nextBlink, sched.nextMicro, sched.poseEnd ?? Infinity);
    timer = env.setTimeout(tick, Math.max(0, at - env.now()));
  };

  function tick() {
    timer = null;
    if (stopped || !sched || env.hidden()) return;
    const { actions, next } = step(sched, read().personality, env.now(), env.rand);
    apply(actions);
    // `Infinity` = the eyes dozed off: no timer at all until the cursor wakes them
    if (Number.isFinite(next)) timer = env.setTimeout(tick, Math.max(0, next - env.now()));
  }

  const clearTimer = () => {
    if (timer !== null) env.clearTimeout(timer);
    timer = null;
  };

  const reset = () => {
    delete el.dataset.pose;
    for (const v of ['--gx', '--gy', '--look-ms', '--look-steps', '--side']) el.style.removeProperty(v);
    const lids = el.querySelectorAll('.lid');
    for (let i = 0; i < lids.length; i++) lids[i].getAnimations?.().forEach((a) => a.cancel());
  };

  const begin = () => {
    sched = initSchedule(read().personality, env.now(), env.rand);
    wake();
    schedule();
  };

  // Hidden window: stand down completely (no timer, no animations) and restart fresh on return.
  const offVis = env.onVisibility(() => {
    if (stopped) return;
    if (env.hidden()) {
      clearTimer();
      sched = null;
      reset();
    } else if (!sched) {
      begin();
    }
  });

  if (!env.hidden()) begin();

  return {
    cursor(ev) {
      if (stopped || !sched) return;
      const now = env.now();
      if (now - lastCursor < CURSOR_MIN_GAP_MS) return;
      lastCursor = now;
      const { personality: p } = read();
      const g = cursorGaze(ev.dx, ev.dy, p.range);
      sched.gx = g.x;
      sched.gy = g.y;
      if (noteCursor(sched, now)) delete el.dataset.pose; // closed eyes open to look
      setGaze(g.x, g.y, Math.min(p.lookMs, 130));
      if (timer === null) schedule(); // woken from a doze: the timer chain starts again
    },
    stop() {
      stopped = true;
      clearTimer();
      offVis();
      sched = null;
      reset();
    },
  };
}
