import { describe, expect, it } from 'vitest';
import { PERSONALITIES, type Personality, type PoseId } from '../../lib/idle';
import {
  CURSOR_HOLD_MS,
  MIN_STEP_MS,
  POSE_HOLD,
  SETTLE_MAX,
  SETTLE_MS,
  settleScale,
  between,
  cursorGaze,
  initSchedule,
  noteCursor,
  pickGaze,
  pickPose,
  step,
  type Action,
  type Rand,
} from './engine';

/** mulberry32: a tiny seedable PRNG so every simulation below is reproducible. */
function seeded(seed: number): Rand {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

interface Logged {
  at: number;
  a: Action;
}

/** Run the scheduler the way the runner does: sleep until `next`, step, repeat. */
function simulate(p: Personality, ms: number, seed = 1) {
  const rand = seeded(seed);
  // the long-run statistics below are about the profile itself: no settling, no dozing off
  const s = initSchedule(p, 0, rand, { dozeAfterMs: Infinity, settle: false });
  const log: Logged[] = [];
  const wakeups: number[] = [];
  let now = Math.min(s.nextGaze, s.nextBlink, s.nextMicro);
  while (now <= ms) {
    wakeups.push(now);
    const { actions, next } = step(s, p, now, rand);
    for (const a of actions) log.push({ at: now, a });
    now = next;
  }
  return { log, wakeups, s };
}

const only = <T extends Action['type']>(log: Logged[], type: T) =>
  log.filter((l): l is Logged & { a: Extract<Action, { type: T }> } => l.a.type === type);

describe.each(PERSONALITIES.map((p) => [p.id, p] as const))('engine: %s', (_id, p) => {
  const run = simulate(p, 30 * 60_000); // half an hour of idling

  it('wakes at human-scale intervals only (never a tight loop)', () => {
    const gaps = run.wakeups.slice(1).map((t, i) => t - run.wakeups[i]);
    expect(Math.min(...gaps)).toBeGreaterThanOrEqual(MIN_STEP_MS);
    const perSecond = run.wakeups.length / (30 * 60);
    expect(perSecond).toBeLessThan(4); // a frame loop would be 60
  });

  it('shifts the gaze no more often than the profile says, always within range', () => {
    const gazes = only(run.log, 'gaze');
    expect(gazes.length).toBeGreaterThan(10);
    for (let i = 1; i < gazes.length; i++) {
      expect(gazes[i].at - gazes[i - 1].at).toBeGreaterThanOrEqual(p.saccade[0] - 1);
    }
    for (const g of gazes) {
      expect(Math.abs(g.a.x)).toBeLessThanOrEqual(1);
      expect(Math.abs(g.a.y)).toBeLessThanOrEqual(1);
      expect(Math.abs(g.a.x)).toBeLessThanOrEqual(p.range + 0.15);
    }
  });

  it('never "moves" to where it already looks', () => {
    const gazes = only(run.log, 'gaze');
    for (let i = 1; i < gazes.length; i++) {
      const dx = Math.abs(gazes[i].a.x - gazes[i - 1].a.x);
      const dy = Math.abs(gazes[i].a.y - gazes[i - 1].a.y);
      expect(dx + dy).toBeGreaterThan(0.09);
    }
  });

  it('blinks irregularly and never faster than the profile', () => {
    const blinks = only(run.log, 'blink');
    expect(blinks.length).toBeGreaterThan(10);
    const gaps = blinks.slice(1).map((b, i) => b.at - blinks[i].at);
    expect(Math.min(...gaps)).toBeGreaterThanOrEqual(p.blink[0] - 1);
    // irregular: the gaps are not all (nearly) the same
    const mean = gaps.reduce((a, b) => a + b, 0) / gaps.length;
    const sd = Math.sqrt(gaps.reduce((a, b) => a + (b - mean) ** 2, 0) / gaps.length);
    expect(sd).toBeGreaterThan(mean * 0.1);
    const doubles = blinks.filter((b) => b.a.double).length / blinks.length;
    if (p.doubleBlink === 0) expect(doubles).toBe(0);
    else expect(Math.abs(doubles - p.doubleBlink)).toBeLessThan(0.1);
  });

  it('holds each pose for its own time, one at a time, from its own repertoire', () => {
    let open: { pose: PoseId; at: number } | null = null;
    let seen = 0;
    for (const { at, a } of run.log) {
      if (a.type === 'pose') {
        expect(open).toBeNull();
        expect(Object.keys(p.poses)).toContain(a.pose);
        expect([-1, 1]).toContain(a.side);
        open = { pose: a.pose, at };
        seen++;
      } else if (a.type === 'pose-end') {
        expect(open).not.toBeNull();
        const held = at - open!.at;
        const [lo, hi] = POSE_HOLD[open!.pose];
        expect(held).toBeGreaterThanOrEqual(lo - 1);
        expect(held).toBeLessThanOrEqual(hi + 2000); // a late timer may overshoot the end a little
        open = null;
      }
    }
    expect(seen).toBeGreaterThan(5);
  });

  it('does not blink or wander while the eyes are shut (happy / doze)', () => {
    let shut = false;
    for (const { a } of run.log) {
      if (a.type === 'pose') shut = a.pose === 'happy' || a.pose === 'doze';
      else if (a.type === 'pose-end') shut = false;
      else if (shut) throw new Error(`${a.type} while the eyes were shut`);
    }
  });

  it('is deterministic for a given seed and differs between seeds', () => {
    const again = simulate(p, 60_000, 1);
    const same = simulate(p, 60_000, 1);
    const other = simulate(p, 60_000, 2);
    expect(again.log).toEqual(same.log);
    expect(JSON.stringify(other.log)).not.toEqual(JSON.stringify(same.log));
  });
});

describe('personalities feel different', () => {
  const rate = (id: string, type: Action['type']) => {
    const p = PERSONALITIES.find((x) => x.id === id)!;
    return only(simulate(p, 10 * 60_000).log, type as 'gaze').length;
  };
  it('hyper looks around far more than calm', () => {
    expect(rate('hyper', 'gaze')).toBeGreaterThan(rate('calm', 'gaze') * 2.5);
  });
  it('sleepy mostly droops/dozes, hyper mostly perks', () => {
    const count = (id: string) => {
      const p = PERSONALITIES.find((x) => x.id === id)!;
      const poses = only(simulate(p, 30 * 60_000).log, 'pose').map((l) => l.a.pose);
      const n = (k: PoseId) => poses.filter((x) => x === k).length;
      return { droopDoze: n('droop') + n('doze'), perk: n('perk'), total: poses.length };
    };
    expect(count('sleepy').droopDoze / count('sleepy').total).toBeGreaterThan(0.7);
    expect(count('hyper').perk / count('hyper').total).toBeGreaterThan(0.3);
  });
});

describe('attention fades: settle, then doze off for free', () => {
  /** drive the scheduler like the runner does, but keep the default settle/doze behaviour */
  function live(p: Personality, ms: number, seed = 4, opts?: { dozeAfterMs?: number }) {
    const rand = seeded(seed);
    const s = initSchedule(p, 0, rand, opts);
    const log: Logged[] = [];
    let now = Math.min(s.nextGaze, s.nextBlink, s.nextMicro);
    let last = { actions: [] as Action[], next: 0 };
    while (now <= ms && Number.isFinite(now)) {
      last = step(s, p, now, rand);
      for (const a of last.actions) log.push({ at: now, a });
      now = last.next;
    }
    return { s, log, last };
  }

  it('moves less and less often the longer nothing interests it', () => {
    const p = PERSONALITIES[0];
    const { log } = live(p, 4 * 60_000, 4, { dozeAfterMs: Infinity });
    const gazes = only(log, 'gaze').map((l) => l.at);
    const within = (a: number, b: number) => gazes.filter((t) => t >= a && t < b).length;
    // same-length windows: fresh eyes versus fully settled eyes
    expect(within(0, 30_000)).toBeGreaterThan(within(60_000, 90_000) * 1.8);
    expect(settleScale(initSchedule(p, 0, seeded(1)), 0)).toBe(1);
    expect(settleScale(initSchedule(p, 0, seeded(1)), SETTLE_MS * 10)).toBe(SETTLE_MAX);
  });

  it.each(PERSONALITIES.map((p) => [p.id, p] as const))('%s dozes off at its own attention span and then stops asking for time', (_id, p) => {
    const { s, log, last } = live(p, 10 * 60_000, 6);
    expect(s.asleep).toBe(true);
    // (the sleepy personality also naps briefly mid-life; the LAST doze is the one that sticks)
    const dozes = log.filter((l) => l.a.type === 'pose' && l.a.pose === 'doze');
    const doze = dozes[dozes.length - 1];
    expect(doze).toBeDefined();
    // it dozes within one scheduling step of its attention span (never earlier)
    expect(doze.at).toBeGreaterThanOrEqual(p.dozeAfterMs);
    expect(doze.at).toBeLessThan(p.dozeAfterMs + 12_000);
    // nothing at all after that: no actions, and no further wake-up requested
    expect(log.filter((l) => l.at > doze.at)).toEqual([]);
    expect(last.next).toBe(Infinity);
    expect(step(s, p, 11 * 60_000, seeded(1))).toEqual({ actions: [], next: Infinity });
  });

  it('a cursor glance wakes it, resets the boredom clock and starts blinking again soon', () => {
    const p = PERSONALITIES[2]; // sleepy: 25 s
    const rand = seeded(8);
    const { s } = live(p, 60_000, 8);
    expect(s.asleep).toBe(true);
    expect(noteCursor(s, 70_000)).toBe(true);
    expect(s.asleep).toBe(false);
    expect(s.pose).toBeNull();
    expect(s.since).toBe(70_000);
    const r = step(s, p, 70_600, rand);
    expect(r.actions.some((a) => a.type === 'blink')).toBe(true);
    expect(Number.isFinite(r.next)).toBe(true);
    // and it stays awake for another full attention span
    expect(s.asleep).toBe(false);
    const later = step(s, p, 70_000 + p.dozeAfterMs - 2000, rand);
    expect(later.actions.some((a) => a.type === 'pose' && a.pose === 'doze')).toBe(false);
  });

  it('keeps an attentive companion awake: cursor events postpone the doze', () => {
    const p = PERSONALITIES[3]; // hyper: 40 s
    const s = initSchedule(p, 0, seeded(2));
    for (let t = 0; t < 5 * 60_000; t += 10_000) {
      noteCursor(s, t);
      step(s, p, t + 50, seeded(t));
    }
    expect(s.asleep).toBe(false);
  });

  it('every personality has a sane attention span', () => {
    for (const p of PERSONALITIES) {
      expect(p.dozeAfterMs).toBeGreaterThanOrEqual(15_000);
      expect(p.dozeAfterMs).toBeLessThanOrEqual(5 * 60_000);
    }
  });
});

describe('late timers do not cause a burst', () => {
  it('a 60 s stall runs each due event once, then carries on normally', () => {
    const p = PERSONALITIES[0];
    const rand = seeded(7);
    const s = initSchedule(p, 0, rand);
    const { actions, next } = step(s, p, 60_000, rand);
    expect(actions.filter((a) => a.type === 'gaze').length).toBeLessThanOrEqual(1);
    expect(actions.filter((a) => a.type === 'blink').length).toBeLessThanOrEqual(1);
    expect(next).toBeGreaterThan(60_000);
  });
});

describe('cursor glance', () => {
  it('looks toward the cursor: right/down for a positive offset, left/up for a negative one', () => {
    const r = cursorGaze(120, 80);
    expect(r.x).toBeGreaterThan(0);
    expect(r.y).toBeGreaterThan(0);
    const l = cursorGaze(-120, -80);
    expect(l.x).toBeLessThan(0);
    expect(l.y).toBeLessThan(0);
    expect(cursorGaze(0, 0)).toEqual({ x: 0, y: 0 });
  });

  it('is monotonic in distance and always bounded', () => {
    let prev = -1;
    for (let dx = 0; dx <= 400; dx += 20) {
      const x = cursorGaze(dx, 0).x;
      expect(x).toBeGreaterThanOrEqual(prev);
      expect(x).toBeLessThanOrEqual(1);
      prev = x;
    }
    expect(Math.abs(cursorGaze(1e6, -1e6).x)).toBeLessThanOrEqual(1);
    expect(Math.abs(cursorGaze(1e6, -1e6).y)).toBeLessThanOrEqual(1);
  });

  it('a calmer personality glances less far than a hyper one', () => {
    expect(cursorGaze(200, 0, 0.45).x).toBeLessThan(cursorGaze(200, 0, 1).x);
  });

  it('takes over from wandering and hands back after the hold', () => {
    const p = PERSONALITIES[0];
    const rand = seeded(3);
    const s = initSchedule(p, 0, rand);
    noteCursor(s, 1000);
    expect(s.cursorUntil).toBe(1000 + CURSOR_HOLD_MS);
    // a wander event that falls inside the hold is suppressed (and pushed beyond it)
    s.nextGaze = 1200;
    const inside = step(s, p, 1300, rand);
    expect(inside.actions.some((a) => a.type === 'gaze')).toBe(false);
    // once the hold has passed, wandering resumes
    s.nextGaze = 3000;
    const after = step(s, p, 3000, rand);
    expect(after.actions.some((a) => a.type === 'gaze')).toBe(true);
  });

  it('repeated cursor events keep pushing the next wander out (no timer re-arming needed)', () => {
    const p = PERSONALITIES[0];
    const s = initSchedule(p, 0, seeded(5));
    for (let t = 1000; t < 3000; t += 100) noteCursor(s, t);
    expect(s.nextGaze).toBeGreaterThanOrEqual(2900 + CURSOR_HOLD_MS);
  });

  it('opens closed eyes so they can look', () => {
    const p = PERSONALITIES[2];
    const s = initSchedule(p, 0, seeded(5));
    s.pose = 'doze';
    s.poseEnd = 9000;
    expect(noteCursor(s, 100)).toBe(true);
    expect(s.pose).toBeNull();
    expect(s.poseEnd).toBeNull();
    s.pose = 'droop';
    expect(noteCursor(s, 200)).toBe(false); // droopy eyes can still look
    expect(s.pose).toBe('droop');
  });
});

describe('pickers', () => {
  it('pickGaze stays in range and moves visibly for any seed', () => {
    for (const p of PERSONALITIES) {
      const rand = seeded(11);
      let prev = { x: 0, y: 0 };
      for (let i = 0; i < 500; i++) {
        const g = pickGaze(p, rand, prev);
        expect(Math.abs(g.x)).toBeLessThanOrEqual(1);
        expect(Math.abs(g.y)).toBeLessThanOrEqual(1);
        expect(Math.abs(g.x - prev.x) + Math.abs(g.y - prev.y)).toBeGreaterThan(0.09);
        prev = g;
      }
    }
  });

  it('pickPose follows the weights and returns null when there are none', () => {
    const p = PERSONALITIES.find((x) => x.id === 'curious')!;
    const rand = seeded(9);
    const counts: Record<string, number> = {};
    for (let i = 0; i < 20_000; i++) {
      const k = pickPose(p, rand)!;
      counts[k] = (counts[k] ?? 0) + 1;
    }
    const total = Object.values(p.poses).reduce((a, b) => a + b, 0);
    for (const [k, w] of Object.entries(p.poses)) {
      expect(Math.abs((counts[k] ?? 0) / 20_000 - w / total)).toBeLessThan(0.02);
    }
    expect(pickPose({ ...p, poses: {} }, rand)).toBeNull();
  });

  it('between() stays inside [lo, hi]', () => {
    const rand = seeded(2);
    for (let i = 0; i < 1000; i++) {
      const v = between(rand, [450, 1300]);
      expect(v).toBeGreaterThanOrEqual(450);
      expect(v).toBeLessThanOrEqual(1300);
    }
  });
});
