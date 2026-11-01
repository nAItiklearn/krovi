import { describe, expect, it } from 'vitest';
import { PERSONALITIES, styleById } from '../../lib/idle';
import { runCompanion, type CompanionEl, type RunnerEnv } from './runner';

// ---- fakes -----------------------------------------------------------------------------------

function seeded(seed: number) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function fakeEnv(over: Partial<RunnerEnv> = {}) {
  let t = 0;
  let id = 0;
  const pending = new Map<number, { at: number; fn: () => void }>();
  let maxAlive = 0;
  let visCb: (() => void) | null = null;
  const state = { hidden: false, reduced: false };
  const env: RunnerEnv = {
    now: () => t,
    setTimeout: (fn, ms) => {
      pending.set(++id, { at: t + ms, fn });
      maxAlive = Math.max(maxAlive, pending.size);
      return id;
    },
    clearTimeout: (h) => void pending.delete(h as number),
    rand: seeded(42),
    hidden: () => state.hidden,
    reducedMotion: () => state.reduced,
    onVisibility: (cb) => {
      visCb = cb;
      return () => (visCb = null);
    },
    ...over,
  };
  const advance = (ms: number) => {
    const end = t + ms;
    for (;;) {
      const due = [...pending.entries()].filter(([, v]) => v.at <= end).sort((a, b) => a[1].at - b[1].at)[0];
      if (!due) break;
      pending.delete(due[0]);
      t = due[1].at;
      due[1].fn();
    }
    t = end;
  };
  return {
    env,
    state,
    advance,
    alive: () => pending.size,
    maxAlive: () => maxAlive,
    toggleHidden(h: boolean) {
      state.hidden = h;
      visCb?.();
    },
  };
}

interface Anim {
  frames: Keyframe[];
  opts: KeyframeAnimationOptions;
  cancelled: boolean;
}

function fakeEl() {
  const vars: Record<string, string> = {};
  const dataset: Record<string, string | undefined> = {};
  const anims: Anim[] = [];
  const lid = () => ({
    animate(frames: Keyframe[], opts: KeyframeAnimationOptions) {
      const a: Anim = { frames, opts, cancelled: false };
      anims.push(a);
      return a;
    },
    getAnimations: () => anims.filter((a) => !a.cancelled).map((a) => ({ cancel: () => void (a.cancelled = true) })),
  });
  const lids = [lid(), lid()];
  const el: CompanionEl = {
    style: {
      setProperty: (n, v) => void (vars[n] = v),
      removeProperty: (n) => void delete vars[n],
    },
    dataset,
    querySelectorAll: () => lids,
  };
  return { el, vars, dataset, anims };
}

// tests are about the live behaviour, so by default the eyes never doze off (see "dozing" below)
const read = (pid = 'curious', sid: Parameters<typeof styleById>[0] = 'dot', dozeAfterMs = Infinity) => () => ({
  personality: { ...PERSONALITIES.find((p) => p.id === pid)!, dozeAfterMs },
  style: styleById(sid),
});

// ---- tests -----------------------------------------------------------------------------------

describe('runCompanion', () => {
  it('holds exactly ONE timer at any moment, for ten simulated minutes', () => {
    const f = fakeEnv();
    const e = fakeEl();
    const r = runCompanion(e.el, read(), f.env);
    expect(f.alive()).toBe(1);
    for (let i = 0; i < 600; i++) {
      f.advance(1000);
      expect(f.alive()).toBe(1);
    }
    expect(f.maxAlive()).toBe(1);
    r.stop();
    expect(f.alive()).toBe(0);
  });

  it('drives the gaze through two css variables and a transition time', () => {
    const f = fakeEnv();
    const e = fakeEl();
    runCompanion(e.el, read(), f.env);
    f.advance(30_000);
    expect(Number(e.vars['--gx'])).toBeGreaterThanOrEqual(-1);
    expect(Number(e.vars['--gx'])).toBeLessThanOrEqual(1);
    expect(e.vars['--gy']).toBeDefined();
    expect(e.vars['--look-ms']).toBe('16ms'); // one jump = one frame of transition
    expect(e.vars['--look-steps']).toBe('1'); // a single jump: drawn frames are the budget
  });

  it('opens the eyes on start with one transform-only animation per lid', () => {
    const f = fakeEnv();
    const e = fakeEl();
    runCompanion(e.el, read(), f.env);
    expect(e.anims).toHaveLength(2);
    for (const a of e.anims) {
      expect(a.frames.map((fr) => Object.keys(fr))).toEqual([['transform'], ['transform']]);
      expect(a.opts.duration).toBe(280);
    }
  });

  it('blinks with ONE transform-only animation per lid (both eyes together); a double blink is one animation too', () => {
    const f = fakeEnv();
    const e = fakeEl();
    runCompanion(e.el, read('hyper'), f.env);
    f.advance(120_000);
    const blinks = e.anims.slice(2); // after the wake-up pair
    expect(blinks.length).toBeGreaterThan(20);
    expect(blinks.length % 2).toBe(0);
    for (const a of blinks) {
      for (const frame of a.frames) {
        expect(Object.keys(frame).every((k) => ['transform', 'offset', 'easing'].includes(k))).toBe(true);
        expect(String(frame.transform)).toMatch(/^scaleY\(/);
      }
      expect(a.opts.delay ?? 0).toBe(0); // never a delayed second animation on the same target
    }
    expect(blinks.some((a) => a.opts.duration === Math.round(72 * 2.2))).toBe(true); // a double blink
    expect(blinks.some((a) => a.opts.duration === 72)).toBe(true);
  });

  it('uses the style traits: Pixel closes less far and its gaze snaps to the grid', () => {
    const f = fakeEnv();
    const e = fakeEl();
    runCompanion(e.el, read('hyper', 'pixel'), f.env);
    f.advance(60_000);
    const blinks = e.anims.slice(2);
    expect(blinks.length).toBeGreaterThan(0);
    expect(blinks.every((a) => a.frames.some((fr) => fr.transform === 'scaleY(0.2)'))).toBe(true); // Pixel closes to 0.2
    const gx = Number(e.vars['--gx']);
    expect(Math.abs(gx * 2 - Math.round(gx * 2))).toBeLessThan(1e-6); // multiples of 1/2
  });

  it('sets and clears poses', () => {
    const f = fakeEnv();
    const e = fakeEl();
    runCompanion(e.el, read('sleepy'), f.env);
    const seen = new Set<string>();
    let cleared = 0;
    for (let i = 0; i < 4000; i++) {
      f.advance(100);
      if (e.dataset.pose) seen.add(e.dataset.pose);
      else if (seen.size) cleared++;
    }
    expect([...seen].every((p) => ['droop', 'doze', 'happy'].includes(p))).toBe(true);
    expect(seen.size).toBeGreaterThan(0);
    expect(cleared).toBeGreaterThan(0);
    expect(['-1', '1']).toContain(e.vars['--side']);
  });

  it('picks up a personality change on the next step without restarting', () => {
    const f = fakeEnv();
    const e = fakeEl();
    let pid = 'calm';
    runCompanion(e.el, () => read(pid)(), f.env);
    f.advance(8000);
    expect(e.vars['--look-ms']).toBe('140ms');
    pid = 'hyper';
    f.advance(40_000);
    expect(e.vars['--look-ms']).toBe('16ms');
  });

  it('stop() removes the timer, the variables, the pose and cancels running animations', () => {
    const f = fakeEnv();
    const e = fakeEl();
    const r = runCompanion(e.el, read('hyper'), f.env);
    f.advance(30_000);
    e.dataset.pose = 'perk';
    r.stop();
    expect(f.alive()).toBe(0);
    expect(e.vars).toEqual({});
    expect(e.dataset.pose).toBeUndefined();
    expect(e.anims.every((a) => a.cancelled)).toBe(true);
    f.advance(60_000);
    expect(f.alive()).toBe(0);
  });

  it('does nothing at all under prefers-reduced-motion', () => {
    const f = fakeEnv();
    f.state.reduced = true;
    const e = fakeEl();
    const r = runCompanion(e.el, read(), f.env);
    expect(f.alive()).toBe(0);
    f.advance(60_000);
    expect(e.anims).toHaveLength(0);
    expect(e.vars).toEqual({});
    r.cursor({ dx: 50, dy: 0 });
    expect(e.vars).toEqual({});
    r.stop();
  });

  it('stands down while hidden (no timer, no animations) and restarts when visible again', () => {
    const f = fakeEnv();
    const e = fakeEl();
    runCompanion(e.el, read(), f.env);
    f.advance(10_000);
    expect(f.alive()).toBe(1);
    f.toggleHidden(true);
    expect(f.alive()).toBe(0);
    const n = e.anims.length;
    f.advance(120_000);
    expect(e.anims.length).toBe(n);
    f.toggleHidden(false);
    expect(f.alive()).toBe(1);
    f.advance(20_000);
    expect(e.anims.length).toBeGreaterThan(n);
  });

  it('does not start when it is created while hidden', () => {
    const f = fakeEnv();
    f.state.hidden = true;
    const e = fakeEl();
    runCompanion(e.el, read(), f.env);
    expect(f.alive()).toBe(0);
    f.toggleHidden(false);
    expect(f.alive()).toBe(1);
  });

  describe('dozing off', () => {
    it('closes the eyes after its attention span and then holds NO timer and draws no frames', () => {
      const f = fakeEnv();
      const e = fakeEl();
      runCompanion(e.el, read('sleepy', 'dot', 25_000), f.env);
      f.advance(40_000);
      expect(e.dataset.pose).toBe('doze');
      expect(f.alive()).toBe(0);
      const n = e.anims.length;
      f.advance(10 * 60_000);
      expect(e.anims.length).toBe(n); // not a single blink while asleep
      expect(e.dataset.pose).toBe('doze');
      expect(f.alive()).toBe(0);
    });

    it('a cursor glance wakes it and the timer chain restarts (still exactly one timer)', () => {
      const f = fakeEnv();
      const e = fakeEl();
      const r = runCompanion(e.el, read('sleepy', 'dot', 25_000), f.env);
      f.advance(40_000);
      expect(f.alive()).toBe(0);
      const n = e.anims.length;
      r.cursor({ dx: 90, dy: 30 });
      expect(e.dataset.pose).toBeUndefined();
      expect(Number(e.vars['--gx'])).toBeGreaterThan(0);
      expect(f.alive()).toBe(1);
      f.advance(6000);
      expect(e.anims.length).toBeGreaterThan(n); // it blinks again
      expect(f.maxAlive()).toBe(1);
    });

    it('stays awake while the cursor keeps coming back', () => {
      const f = fakeEnv();
      const e = fakeEl();
      const r = runCompanion(e.el, read('hyper', 'dot', 40_000), f.env);
      for (let i = 0; i < 40; i++) {
        f.advance(10_000);
        r.cursor({ dx: 100, dy: 20 });
      }
      expect(e.dataset.pose).not.toBe('doze');
      expect(f.alive()).toBe(1);
    });
  });

  describe('cursor glance', () => {
    it('looks toward the cursor and takes priority over wandering', () => {
      const f = fakeEnv();
      const e = fakeEl();
      const r = runCompanion(e.el, read(), f.env);
      f.advance(5000);
      r.cursor({ dx: 180, dy: 60 });
      expect(Number(e.vars['--gx'])).toBeGreaterThan(0.4);
      expect(Number(e.vars['--gy'])).toBeGreaterThan(0);
      const held = e.vars['--gx'];
      f.advance(900); // still inside the hold: wandering must not override it
      expect(e.vars['--gx']).toBe(held);
      r.cursor({ dx: -180, dy: 0 });
      expect(Number(e.vars['--gx'])).toBeLessThan(-0.4);
    });

    it('does not add timers, however many events arrive', () => {
      const f = fakeEnv();
      const e = fakeEl();
      const r = runCompanion(e.el, read(), f.env);
      for (let i = 0; i < 300; i++) {
        f.advance(100);
        r.cursor({ dx: 100 + i, dy: 20 });
      }
      expect(f.maxAlive()).toBe(1);
    });

    it('drops events that arrive faster than 10 Hz', () => {
      const f = fakeEnv();
      const e = fakeEl();
      const r = runCompanion(e.el, read(), f.env);
      f.advance(2000);
      r.cursor({ dx: 100, dy: 0 });
      const first = e.vars['--gx'];
      f.advance(30);
      r.cursor({ dx: -150, dy: 0 });
      expect(e.vars['--gx']).toBe(first);
      f.advance(60);
      r.cursor({ dx: -150, dy: 0 });
      expect(e.vars['--gx']).not.toBe(first);
    });

    it('opens dozing eyes to look, and wandering resumes afterwards', () => {
      const f = fakeEnv();
      const e = fakeEl();
      const r = runCompanion(e.el, read('sleepy'), f.env);
      e.dataset.pose = 'doze';
      // find a moment when the engine itself has put the eyes to sleep, else fake it
      for (let i = 0; i < 6000 && e.dataset.pose !== 'doze'; i++) {
        delete e.dataset.pose;
        f.advance(100);
      }
      if (e.dataset.pose !== 'doze') e.dataset.pose = 'doze';
      r.cursor({ dx: 120, dy: 0 });
      expect(Number(e.vars['--gx'])).toBeGreaterThan(0);
      f.advance(40_000);
      expect(f.alive()).toBe(1);
    });

    it('is ignored after stop()', () => {
      const f = fakeEnv();
      const e = fakeEl();
      const r = runCompanion(e.el, read(), f.env);
      r.stop();
      r.cursor({ dx: 100, dy: 100 });
      expect(e.vars).toEqual({});
    });
  });
});
