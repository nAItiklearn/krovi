// The companion's brain: a pure, seedable scheduler. No DOM, no timers, no Svelte.
//
// It owns four virtual deadlines (next gaze shift, next blink, next micro-behaviour, end of the
// current pose). `step(now)` performs everything that is due and returns the actions to apply plus
// the single time at which it wants to be called again. The runner turns that into ONE setTimeout,
// so the whole companion is one timer chain at human-scale intervals (never a frame loop).

import type { Personality, PoseId } from '../../lib/idle';

export type Rand = () => number;

/** Fastest the scheduler may ask to be called again: keeps a bad personality from spinning. */
export const MIN_STEP_MS = 160;
/** How long the cursor keeps the eyes' attention after its last `shell-cursor` event. */
export const CURSOR_HOLD_MS = 1100;

/** After attention (eyes appearing, a cursor glance) the cadence relaxes by up to SETTLE_MAX over this long. */
export const SETTLE_MS = 45_000;
export const SETTLE_MAX = 3;

/** Hold time of each pose, [min, max] ms. */
export const POSE_HOLD: Record<PoseId, readonly [number, number]> = {
  droop: [1600, 2800],
  tilt: [1100, 2000],
  happy: [1200, 2200],
  perk: [520, 820],
  doze: [3500, 6500],
};

/** Poses during which the eyes are closed/arced: no blinking, no wandering. */
const EYES_SHUT: ReadonlySet<PoseId> = new Set<PoseId>(['happy', 'doze']);

export type Action =
  | { type: 'gaze'; x: number; y: number }
  | { type: 'blink'; double: boolean }
  /** `side` is -1 or 1: which way the head tilts, which eye is raised */
  | { type: 'pose'; pose: PoseId; side: 1 | -1 }
  | { type: 'pose-end' };

export interface Schedule {
  nextGaze: number;
  nextBlink: number;
  nextMicro: number;
  /** end of the current pose, or null when neutral */
  poseEnd: number | null;
  pose: PoseId | null;
  /** the cursor holds the eyes until this time */
  cursorUntil: number;
  gx: number;
  gy: number;
  /** when attention began (eyes appeared / last cursor glance): the boredom clock */
  since: number;
  /** after this long without attention the eyes doze off for good (no timers at all) */
  dozeAfter: number;
  /** relax the cadence as attention fades */
  settle: boolean;
  asleep: boolean;
}

export const between = (r: Rand, [lo, hi]: readonly [number, number]): number => lo + r() * (hi - lo);

/** Deadlines for the moment the eyes appear: a first look and a waking blink come quickly. */
export function initSchedule(
  p: Personality,
  now: number,
  rand: Rand,
  opts: { dozeAfterMs?: number; settle?: boolean } = {},
): Schedule {
  return {
    nextGaze: now + between(rand, [500, 1500]),
    nextBlink: now + between(rand, [700, 1800]),
    nextMicro: now + between(rand, p.micro) * 0.6,
    poseEnd: null,
    pose: null,
    cursorUntil: 0,
    gx: 0,
    gy: 0,
    since: now,
    dozeAfter: opts.dozeAfterMs ?? p.dozeAfterMs,
    settle: opts.settle ?? true,
    asleep: false,
  };
}

/** A new gaze target. Always a visible move, mostly horizontal (eyes in a 36px pill barely look up). */
export function pickGaze(p: Personality, rand: Rand, prev: { x: number; y: number }): { x: number; y: number } {
  const lim = Math.min(1, p.range);
  if (rand() < p.centerBias) {
    // drift back to (about) the middle
    let x = (rand() * 2 - 1) * 0.12 * lim;
    const y = (rand() * 2 - 1) * 0.1;
    // never "move" to where we already are
    if (Math.abs(x - prev.x) < 0.1 && Math.abs(y - prev.y) < 0.1) x = prev.x + (x >= prev.x ? 0.15 : -0.15);
    return { x, y };
  }
  let x = (rand() * 2 - 1) * lim;
  const y = (rand() * 2 - 1) * lim * 0.55;
  // a real shift: flip to the other side if the roll landed near where we already look
  if (Math.abs(x - prev.x) < 0.3) x = prev.x >= 0 ? -Math.abs(x) - 0.3 : Math.abs(x) + 0.3;
  return { x: Math.max(-lim, Math.min(lim, x)), y };
}

/** Weighted pick among the personality's poses; null when it has none. */
export function pickPose(p: Personality, rand: Rand): PoseId | null {
  const entries = (Object.entries(p.poses) as [PoseId, number][]).filter(([, w]) => w > 0);
  const total = entries.reduce((a, [, w]) => a + w, 0);
  if (total <= 0) return null;
  let roll = rand() * total;
  for (const [pose, w] of entries) {
    roll -= w;
    if (roll < 0) return pose;
  }
  return entries[entries.length - 1][0];
}

/** Map the cursor offset from the pill centre (logical px) to a gaze in [-1, 1]. */
export function cursorGaze(dx: number, dy: number, range = 1): { x: number; y: number } {
  const amp = 0.55 + 0.45 * Math.max(0, Math.min(1, range));
  return { x: Math.tanh(dx / 150) * amp, y: Math.tanh(dy / 130) * amp * 0.9 };
}

/**
 * The cursor was just seen: it owns the gaze for a moment, and wandering resumes after it.
 * Returns true when closed eyes had to open to look (the caller drops the pose).
 */
export function noteCursor(s: Schedule, now: number): boolean {
  s.cursorUntil = now + CURSOR_HOLD_MS;
  s.nextGaze = Math.max(s.nextGaze, s.cursorUntil);
  // someone is here: attention restarts, the cadence tightens again and the doze clock resets
  s.since = now;
  if (s.asleep) {
    // woken by the cursor: a blink to clear the sleep, then back to normal life
    s.asleep = false;
    s.pose = null;
    s.poseEnd = null;
    s.nextBlink = now + 500;
    s.nextMicro = Math.max(s.nextMicro, now + 6000);
    return true;
  }
  if (s.pose !== null && EYES_SHUT.has(s.pose)) {
    s.pose = null;
    s.poseEnd = null;
    return true;
  }
  return false;
}

/** How much slower than its profile the eyes move, 1 when fresh, SETTLE_MAX once settled. */
export function settleScale(s: Schedule, now: number): number {
  if (!s.settle) return 1;
  return 1 + (SETTLE_MAX - 1) * Math.min(1, Math.max(0, (now - s.since) / SETTLE_MS));
}

/**
 * Do everything that is due at `now`. Mutates `s`; returns the actions and when to come back
 * (`Infinity` = asleep: no timer is needed until the cursor wakes the eyes).
 *
 * Late calls (a throttled background timer) are fine: each due event runs once and reschedules
 * from `now`, so the eyes never "catch up" with a burst.
 *
 * Attention fades: the longer nothing has caught the eyes' interest, the slower they move
 * (up to SETTLE_MAX times), and after `dozeAfter` they close and stay closed. A sleeping
 * companion costs nothing: no timer, no frames.
 */
export function step(s: Schedule, p: Personality, now: number, rand: Rand): { actions: Action[]; next: number } {
  const actions: Action[] = [];
  if (s.asleep) return { actions, next: Infinity };

  if (now - s.since >= s.dozeAfter) {
    s.asleep = true;
    s.pose = 'doze';
    s.poseEnd = null;
    actions.push({ type: 'pose', pose: 'doze', side: 1 });
    return { actions, next: Infinity };
  }

  const k = settleScale(s, now);

  if (s.poseEnd !== null && now >= s.poseEnd) {
    s.pose = null;
    s.poseEnd = null;
    actions.push({ type: 'pose-end' });
  }

  if (now >= s.nextGaze) {
    s.nextGaze = now + between(rand, p.saccade) * k;
    const shut = s.pose !== null && EYES_SHUT.has(s.pose);
    if (!shut && now >= s.cursorUntil) {
      const g = pickGaze(p, rand, { x: s.gx, y: s.gy });
      s.gx = g.x;
      s.gy = g.y;
      actions.push({ type: 'gaze', x: g.x, y: g.y });
    }
  }

  if (now >= s.nextBlink) {
    s.nextBlink = now + between(rand, p.blink) * k;
    const shut = s.pose !== null && EYES_SHUT.has(s.pose);
    if (!shut) actions.push({ type: 'blink', double: rand() < p.doubleBlink });
  }

  if (now >= s.nextMicro) {
    if (s.pose === null) {
      const pose = pickPose(p, rand);
      if (pose) {
        const hold = between(rand, POSE_HOLD[pose]);
        s.pose = pose;
        s.poseEnd = now + hold;
        // the next micro-behaviour is measured from when this one ends
        s.nextMicro = s.poseEnd + between(rand, p.micro) * k;
        actions.push({ type: 'pose', pose, side: rand() < 0.5 ? -1 : 1 });
      } else {
        s.nextMicro = now + between(rand, p.micro) * k;
      }
    } else {
      s.nextMicro = (s.poseEnd ?? now) + 400;
    }
  }

  let next = Math.min(s.nextGaze, s.nextBlink, s.nextMicro, s.since + s.dozeAfter);
  if (s.poseEnd !== null) next = Math.min(next, s.poseEnd);
  return { actions, next: Math.max(next, now + MIN_STEP_MS) };
}
