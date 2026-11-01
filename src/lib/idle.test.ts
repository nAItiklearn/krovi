import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import {
  DEFAULT_IDLE,
  IDLE_DELAY_MAX,
  IDLE_DELAY_MIN,
  IDLE_KEY,
  IDLE_STYLES,
  PERSONALITIES,
  createIdleMachine,
  loadIdleSettings,
  personalityById,
  sanitizeIdleSettings,
  saveIdleSettings,
  styleById,
  type StorageLike,
  type TimerApi,
} from './idle';

// ---- a hand-cranked clock + timer set ---------------------------------------------------------

function fakeTimers() {
  let t = 0;
  let nextId = 1;
  const pending = new Map<number, { at: number; fn: () => void }>();
  let sets = 0;
  const api: TimerApi = {
    now: () => t,
    set: (fn, ms) => {
      const id = nextId++;
      pending.set(id, { at: t + ms, fn });
      sets++;
      return id;
    },
    clear: (h) => void pending.delete(h as number),
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
  return { api, advance, alive: () => pending.size, sets: () => sets, now: () => t };
}

function machine(opts: { enabled?: boolean; delayMs?: number } = {}) {
  const tm = fakeTimers();
  const events: boolean[] = [];
  const m = createIdleMachine(tm.api, (v) => events.push(v));
  m.configure({ enabled: opts.enabled ?? true, delayMs: opts.delayMs ?? 5500 });
  return { ...tm, m, events };
}

describe('idle state machine', () => {
  it('sleeps after the delay with no activity, not before', () => {
    const { m, advance, events } = machine();
    advance(5499);
    expect(m.idle).toBe(false);
    advance(1);
    expect(m.idle).toBe(true);
    expect(events).toEqual([true]);
  });

  it('wakes instantly on activity and then needs a full delay again', () => {
    const { m, advance, events } = machine();
    advance(6000);
    expect(m.idle).toBe(true);
    m.activity();
    expect(m.idle).toBe(false); // synchronous: no waiting for a timer
    expect(events).toEqual([true, false]);
    advance(5499);
    expect(m.idle).toBe(false);
    advance(1);
    expect(m.idle).toBe(true);
  });

  it('postpones sleep when activity arrives mid-countdown', () => {
    const { m, advance } = machine();
    advance(4000);
    m.activity(); // hover
    advance(5499); // 9499 total, 5499 since the hover
    expect(m.idle).toBe(false);
    advance(1);
    expect(m.idle).toBe(true);
  });

  it('keeps postponing for as long as activity keeps coming', () => {
    const { m, advance } = machine();
    for (let i = 0; i < 40; i++) {
      advance(3000);
      m.activity();
    }
    expect(m.idle).toBe(false);
  });

  it('never sleeps while disabled', () => {
    const { m, advance, events, alive } = machine({ enabled: false });
    advance(10 * 60_000);
    expect(m.idle).toBe(false);
    expect(events).toEqual([]);
    expect(alive()).toBe(0);
    m.activity();
    expect(alive()).toBe(0); // activity does not start a timer either
  });

  it('wakes immediately when disabled while asleep, and can be re-enabled', () => {
    const { m, advance, events } = machine();
    advance(6000);
    expect(m.idle).toBe(true);
    m.configure({ enabled: false, delayMs: 5500 });
    expect(m.idle).toBe(false);
    expect(events).toEqual([true, false]);
    m.configure({ enabled: true, delayMs: 5500 });
    advance(5500);
    expect(m.idle).toBe(true);
  });

  it('is paused by a hidden window / reduced motion: never idle, wakes, restarts fresh on resume', () => {
    const { m, advance, events, alive } = machine();
    advance(6000);
    expect(m.idle).toBe(true);
    m.setPaused(true);
    expect(m.idle).toBe(false);
    expect(alive()).toBe(0);
    advance(60_000);
    expect(m.idle).toBe(false);
    m.setPaused(false); // window is back: the delay starts over, no instant re-sleep
    expect(m.idle).toBe(false);
    advance(5499);
    expect(m.idle).toBe(false);
    advance(1);
    expect(m.idle).toBe(true);
    expect(events).toEqual([true, false, true]);
  });

  it('honours a changed delay while counting down', () => {
    const { m, advance } = machine({ delayMs: 20_000 });
    advance(3000);
    m.configure({ enabled: true, delayMs: 5000 });
    advance(1999);
    expect(m.idle).toBe(false);
    advance(1);
    expect(m.idle).toBe(true);
  });

  it('clamps an absurd delay', () => {
    const lo = machine({ delayMs: 5 });
    lo.advance(IDLE_DELAY_MIN - 1);
    expect(lo.m.idle).toBe(false);
    lo.advance(1);
    expect(lo.m.idle).toBe(true);
    const hi = machine({ delayMs: 1e12 });
    hi.advance(IDLE_DELAY_MAX);
    expect(hi.m.idle).toBe(true);
  });

  it('holds at most ONE timer and a burst of activity costs no extra timers', () => {
    const { m, advance, alive, sets } = machine();
    expect(alive()).toBe(1);
    const before = sets();
    for (let i = 0; i < 2000; i++) {
      m.activity();
      advance(1);
    }
    expect(alive()).toBe(1);
    // 2 s of hovering re-armed the countdown at most a couple of times, never per event
    expect(sets() - before).toBeLessThanOrEqual(2);
    advance(5500);
    expect(m.idle).toBe(true);
    expect(alive()).toBe(0); // asleep: nothing left running
  });

  it('dispose clears the timer', () => {
    const { m, alive } = machine();
    m.dispose();
    expect(alive()).toBe(0);
  });
});

// ---- settings ----------------------------------------------------------------------------------

describe('idle settings', () => {
  it('defaults: on, Dot, curious, 5.5 s', () => {
    expect(DEFAULT_IDLE).toEqual({ enabled: true, style: 'dot', personality: 'curious', delayMs: 5500 });
  });

  it('sanitizes anything', () => {
    expect(sanitizeIdleSettings(undefined)).toEqual(DEFAULT_IDLE);
    expect(sanitizeIdleSettings(null)).toEqual(DEFAULT_IDLE);
    expect(sanitizeIdleSettings('nope')).toEqual(DEFAULT_IDLE);
    expect(sanitizeIdleSettings({ style: 'unicorn', personality: 7, enabled: 'yes', delayMs: 'soon' })).toEqual(DEFAULT_IDLE);
    expect(sanitizeIdleSettings({ delayMs: NaN }).delayMs).toBe(5500);
    expect(sanitizeIdleSettings({ delayMs: Infinity }).delayMs).toBe(5500);
    expect(sanitizeIdleSettings({ delayMs: 10 }).delayMs).toBe(IDLE_DELAY_MIN);
    expect(sanitizeIdleSettings({ delayMs: 9e9 }).delayMs).toBe(IDLE_DELAY_MAX);
    expect(sanitizeIdleSettings({ delayMs: 7000.6 }).delayMs).toBe(7001);
  });

  it('keeps every valid field and fixes only the bad ones', () => {
    expect(sanitizeIdleSettings({ enabled: false, style: 'pixel', personality: 'hyper', delayMs: 3000 })).toEqual({
      enabled: false,
      style: 'pixel',
      personality: 'hyper',
      delayMs: 3000,
    });
    expect(sanitizeIdleSettings({ enabled: false, style: 'bogus' })).toEqual({ ...DEFAULT_IDLE, enabled: false });
  });

  const memory = (seed?: string): StorageLike & { data: Record<string, string> } => {
    const data: Record<string, string> = seed === undefined ? {} : { [IDLE_KEY]: seed };
    return { data, getItem: (k) => (k in data ? data[k] : null), setItem: (k, v) => void (data[k] = v) };
  };
  const broken: StorageLike = {
    getItem: () => {
      throw new Error('blocked');
    },
    setItem: () => {
      throw new Error('quota');
    },
  };

  it('round-trips through storage', () => {
    const s = memory();
    const mine = { enabled: false, style: 'visor', personality: 'sleepy', delayMs: 12_000 } as const;
    expect(saveIdleSettings(s, mine)).toBe(true);
    expect(loadIdleSettings(s)).toEqual(mine);
  });

  it('falls back to defaults on missing, corrupt or blocked storage', () => {
    expect(loadIdleSettings(null)).toEqual(DEFAULT_IDLE);
    expect(loadIdleSettings(memory())).toEqual(DEFAULT_IDLE);
    expect(loadIdleSettings(memory('{not json'))).toEqual(DEFAULT_IDLE);
    expect(loadIdleSettings(memory('42'))).toEqual(DEFAULT_IDLE);
    expect(loadIdleSettings(broken)).toEqual(DEFAULT_IDLE);
    expect(saveIdleSettings(broken, DEFAULT_IDLE)).toBe(false);
    expect(saveIdleSettings(null, DEFAULT_IDLE)).toBe(false);
  });

  it('repairs a half-valid stored object instead of dropping it', () => {
    expect(loadIdleSettings(memory('{"style":"ring","personality":"???"}'))).toEqual({
      ...DEFAULT_IDLE,
      style: 'ring',
    });
  });
});

// ---- registries --------------------------------------------------------------------------------

describe('registries', () => {
  it('ships at least five original styles with unique ids and all free', () => {
    const ids = IDLE_STYLES.map((s) => s.id);
    expect(IDLE_STYLES.length).toBeGreaterThanOrEqual(5);
    expect(new Set(ids).size).toBe(ids.length);
    for (const want of ['dot', 'capsule', 'sleepy', 'pixel', 'visor']) expect(ids).toContain(want);
    for (const s of IDLE_STYLES) {
      expect(s.name).toBeTruthy();
      expect(s.blinkTo).toBeGreaterThan(0);
      expect(s.blinkTo).toBeLessThan(0.5);
      expect(s).not.toHaveProperty('locked');
    }
    expect(styleById('nope' as never).id).toBe('dot');
  });

  it('has the four personalities as human-scale timing profiles', () => {
    expect(PERSONALITIES.map((p) => p.id)).toEqual(['curious', 'calm', 'sleepy', 'hyper']);
    for (const p of PERSONALITIES) {
      for (const [lo, hi] of [p.saccade, p.blink, p.micro]) {
        expect(lo).toBeGreaterThanOrEqual(400); // nothing faster than a human glance
        expect(hi).toBeGreaterThan(lo);
      }
      expect(p.range).toBeGreaterThan(0);
      expect(p.range).toBeLessThanOrEqual(1);
      expect(p.lookMs).toBeGreaterThan(40);
      expect(p.blinkMs).toBeGreaterThan(60);
      expect(Object.keys(p.poses).length).toBeGreaterThan(0);
      for (const w of Object.values(p.poses)) expect(w).toBeGreaterThan(0);
    }
    expect(personalityById('nope' as never).id).toBe('curious');
  });

  it('orders the personalities sensibly: hyper is the quickest, sleepy blinks slowest', () => {
    const by = (id: string) => PERSONALITIES.find((p) => p.id === id)!;
    expect(by('hyper').saccade[0]).toBeLessThan(by('curious').saccade[0]);
    expect(by('curious').saccade[0]).toBeLessThan(by('calm').saccade[0]);
    expect(by('sleepy').blinkMs).toBeGreaterThan(by('curious').blinkMs);
    expect(by('hyper').doubleBlink).toBeGreaterThan(by('calm').doubleBlink);
  });
});

// ---- the live instance, against a fake window ---------------------------------------------------

describe('startIdle / markActivity (live instance)', () => {
  type Handler = (e?: unknown) => void;
  let docL: Map<string, Set<Handler>>;
  let winL: Map<string, Set<Handler>>;
  let reducedL: Set<Handler>;
  let hidden: boolean;
  let reduced: boolean;
  const fire = (m: Map<string, Set<Handler>>, name: string) => m.get(name)?.forEach((h) => h());
  const add = (m: Map<string, Set<Handler>>) => (name: string, h: Handler) => {
    if (!m.has(name)) m.set(name, new Set());
    m.get(name)!.add(h);
  };
  const remove = (m: Map<string, Set<Handler>>) => (name: string, h: Handler) => void m.get(name)?.delete(h);

  beforeEach(() => {
    vi.useFakeTimers();
    vi.resetModules();
    docL = new Map();
    winL = new Map();
    reducedL = new Set();
    hidden = false;
    reduced = false;
    vi.stubGlobal('document', {
      get hidden() {
        return hidden;
      },
      addEventListener: add(docL),
      removeEventListener: remove(docL),
    });
    vi.stubGlobal('window', { addEventListener: add(winL), removeEventListener: remove(winL) });
    vi.stubGlobal('matchMedia', () => ({
      get matches() {
        return reduced;
      },
      addEventListener: (_: string, h: Handler) => reducedL.add(h),
      removeEventListener: (_: string, h: Handler) => reducedL.delete(h),
    }));
    vi.stubGlobal('localStorage', {
      getItem: () => null,
      setItem: () => {},
    });
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('goes idle after the default 5.5 s and wakes on markActivity()', async () => {
    const idle = await import('./idle');
    idle.startIdle();
    expect(get(idle.isIdle)).toBe(false);
    vi.advanceTimersByTime(5499);
    expect(get(idle.isIdle)).toBe(false);
    vi.advanceTimersByTime(1);
    expect(get(idle.isIdle)).toBe(true);
    idle.markActivity();
    expect(get(idle.isIdle)).toBe(false);
  });

  it('treats keydown and pointerdown anywhere as activity', async () => {
    const idle = await import('./idle');
    idle.startIdle();
    vi.advanceTimersByTime(6000);
    expect(get(idle.isIdle)).toBe(true);
    fire(winL, 'keydown');
    expect(get(idle.isIdle)).toBe(false);
    vi.advanceTimersByTime(6000);
    expect(get(idle.isIdle)).toBe(true);
    fire(winL, 'pointerdown');
    expect(get(idle.isIdle)).toBe(false);
  });

  it('respects the enabled switch', async () => {
    const idle = await import('./idle');
    idle.idleSettings.update((s) => ({ ...s, enabled: false }));
    idle.startIdle();
    vi.advanceTimersByTime(60_000);
    expect(get(idle.isIdle)).toBe(false);
    idle.idleSettings.update((s) => ({ ...s, enabled: true }));
    vi.advanceTimersByTime(5500);
    expect(get(idle.isIdle)).toBe(true);
  });

  it('pauses while the window is hidden and restarts the delay when it returns', async () => {
    const idle = await import('./idle');
    idle.startIdle();
    vi.advanceTimersByTime(6000);
    expect(get(idle.isIdle)).toBe(true);
    hidden = true;
    fire(docL, 'visibilitychange');
    expect(get(idle.isIdle)).toBe(false);
    vi.advanceTimersByTime(120_000);
    expect(get(idle.isIdle)).toBe(false);
    hidden = false;
    fire(docL, 'visibilitychange');
    vi.advanceTimersByTime(5000);
    expect(get(idle.isIdle)).toBe(false);
    vi.advanceTimersByTime(500);
    expect(get(idle.isIdle)).toBe(true);
  });

  it('never shows under prefers-reduced-motion, and follows the setting live', async () => {
    reduced = true;
    const idle = await import('./idle');
    idle.startIdle();
    vi.advanceTimersByTime(60_000);
    expect(get(idle.isIdle)).toBe(false);
    reduced = false;
    reducedL.forEach((h) => h());
    vi.advanceTimersByTime(5500);
    expect(get(idle.isIdle)).toBe(true);
  });

  it('is idempotent and fully tears down', async () => {
    const idle = await import('./idle');
    const a = idle.startIdle();
    const b = idle.startIdle();
    expect(b).toBe(a);
    expect(docL.get('visibilitychange')?.size).toBe(1);
    expect(winL.get('keydown')?.size).toBe(1);
    a();
    expect(docL.get('visibilitychange')?.size).toBe(0);
    expect(winL.get('keydown')?.size).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
  });
});
