import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

// The Tauri event bridge is replaced by a recorder, so a test can "emit" backend events.
const handlers = vi.hoisted(() => new Map<string, (payload: unknown) => void>());
vi.mock('./bridge', async (original) => {
  const real = await original<typeof import('./bridge')>();
  return {
    ...real,
    listen: async (event: string, cb: (payload: unknown) => void) => {
      handlers.set(event, cb);
      return () => {
        handlers.delete(event);
      };
    },
  };
});

// reactions.ts keeps module-level state, so every test gets a fresh copy of it (and of stores.ts).
async function load() {
  vi.resetModules();
  handlers.clear();
  const reactions = await import('./reactions');
  const stores = await import('./stores');
  // the browser demo seeds a running "Tea" timer; start every test from a quiet app
  stores.timers.set([]);
  stores.pomodoroResetCycle();
  return { ...reactions, stores };
}
const emit = (event: string, payload: unknown) => handlers.get(event)?.(payload);
/** let the `listen().then(...)` registrations settle */
const settle = () => vi.advanceTimersByTimeAsync(0);

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-11-03T10:00:00Z'));
});
afterEach(() => {
  vi.useRealTimers();
});

describe('react() and the reaction store', () => {
  it('shows a success for its duration, then clears', async () => {
    const { react, reaction } = await load();
    expect(get(reaction)).toBeNull();
    react('success');
    expect(get(reaction)).toEqual({ kind: 'success', still: false });
    vi.advanceTimersByTime(1_599);
    expect(get(reaction)?.kind).toBe('success');
    vi.advanceTimersByTime(1);
    expect(get(reaction)).toBeNull();
  });

  it('error outranks success, and spamming a kind is ignored (cooldown)', async () => {
    const { react, reaction } = await load();
    react('success');
    react('error');
    expect(get(reaction)?.kind).toBe('error');
    vi.advanceTimersByTime(2_000);
    expect(get(reaction)).toBeNull();
    react('error'); // inside the 3.5 s error cooldown
    expect(get(reaction)).toBeNull();
    vi.advanceTimersByTime(1_500);
    react('error');
    expect(get(reaction)?.kind).toBe('error');
  });

  it('notifies subscribers only when the visible reaction changes', async () => {
    const { react, reaction } = await load();
    const seen: (string | null)[] = [];
    const off = reaction.subscribe((r) => seen.push(r?.kind ?? null));
    react('success');
    react('success');
    vi.advanceTimersByTime(5_000);
    off();
    expect(seen).toEqual([null, 'success', null]);
  });

  it('keeps no timer running when nothing is pending', async () => {
    const { react } = await load();
    react('success');
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(2_000);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe('cursor lingering -> curious', () => {
  it('shows after the linger time and goes when the cursor leaves', async () => {
    const { noteCursorDistance, reaction } = await load();
    noteCursorDistance(120);
    vi.advanceTimersByTime(699);
    expect(get(reaction)).toBeNull();
    vi.advanceTimersByTime(1);
    expect(get(reaction)?.kind).toBe('curious');
    noteCursorDistance(null);
    expect(get(reaction)).toBeNull();
  });
});

describe('built-in sources', () => {
  it('dictation-result: typed = success, failed = error, nothing heard = nothing', async () => {
    const { startReactions, reaction } = await load();
    const stop = startReactions();
    await settle();

    emit('dictation-result', { ok: true, nothingHeard: true });
    expect(get(reaction)).toBeNull();
    emit('dictation-result', { ok: true, text: 'hello', chars: 5 });
    expect(get(reaction)?.kind).toBe('success');
    vi.advanceTimersByTime(3_000);
    emit('dictation-result', { ok: false, error: 'No microphone' });
    expect(get(reaction)?.kind).toBe('error');
    vi.advanceTimersByTime(2_000);
    emit('dictation-result', { ok: false, text: 'blocked text', error: 'Could not type' }); // cooling down
    expect(get(reaction)).toBeNull();
    stop();
  });

  it('alarm-fired: success, but not for a missed alarm', async () => {
    const { startReactions, reaction } = await load();
    const stop = startReactions();
    await settle();

    emit('alarm-fired', { key: 'a', label: 'Tea', kind: 'timer', missed: true });
    expect(get(reaction)).toBeNull();
    emit('alarm-fired', { key: 'a', label: 'Tea', kind: 'timer', missed: false });
    expect(get(reaction)?.kind).toBe('success');
    stop();
  });

  it('focused while a timer runs, and not once it is stopped', async () => {
    const { startReactions, reaction, stores } = await load();
    const stop = startReactions();
    await settle();
    expect(get(reaction)).toBeNull();

    stores.timerAdd('Tea', 5, true);
    expect(get(reaction)?.kind).toBe('focused');
    const id = get(stores.timers)[0].id;
    stores.timerReset(id);
    expect(get(reaction)).toBeNull();
    stop();
  });

  it('focused during a pomodoro work phase, but not a break', async () => {
    const { startReactions, reaction, stores } = await load();
    const stop = startReactions();
    await settle();

    stores.pomodoroResetCycle();
    stores.pomodoroStart();
    expect(get(reaction)?.kind).toBe('focused');
    stores.pomodoro.update((p) => ({ ...p, phase: 'short' }));
    expect(get(reaction)).toBeNull();
    stores.pomodoroResetCycle();
    stop();
  });

  it('a timer-finished activity is a success; other activities, and the one already there, are not', async () => {
    const { startReactions, reaction, stores } = await load();
    stores.pushActivity('timer', 'Old', 'Timer done'); // before wiring: old news
    const stop = startReactions();
    await settle();
    expect(get(reaction)).toBeNull();

    stores.pushActivity('copy', 'Copied');
    expect(get(reaction)).toBeNull();
    stores.pushActivity('timer', 'Tea', 'Timer done');
    expect(get(reaction)?.kind).toBe('success');
    stop();
  });

  it('is refcounted: sources stay live until the last caller stops', async () => {
    const { startReactions, reaction } = await load();
    const a = startReactions();
    const b = startReactions();
    await settle();
    expect(handlers.has('dictation-result')).toBe(true);

    a();
    a(); // a second release of the same handle changes nothing
    emit('alarm-fired', {});
    expect(get(reaction)?.kind).toBe('success');

    b();
    expect(handlers.size).toBe(0);
    expect(get(reaction)).toBeNull();
  });
});
