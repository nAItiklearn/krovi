// Reminders inside the Tauri shell: bridge is mocked, so this proves the command calls and the
// reminder-fired -> banner wiring without a backend.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ReminderFired } from './reminders';

const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
const handlers = new Map<string, (payload: unknown) => void>();
let listeners = 0;

vi.mock('./bridge', () => ({
  isTauri: () => true,
  bridge: { loadCalendar: async () => [{ id: 1, date: '2030-01-02', title: 'Tea', time: '10:00' }] },
  listen: async (event: string, cb: (payload: unknown) => void) => {
    handlers.set(event, cb);
    listeners++;
    return () => {
      handlers.delete(event);
      listeners--;
    };
  },
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    calls.push({ cmd, args });
    // every command resolves to the whole list, like the backend
    if (cmd === 'reminders_status') return { pending: [] };
    return [{ id: 1, date: '2030-01-02', title: 'Tea', time: '10:00', remind: true, sound: true }];
  },
}));

const emit = (event: string, payload: unknown) => handlers.get(event)?.(payload);
const fired = (over: Partial<ReminderFired> = {}): ReminderFired => ({
  id: 1,
  title: 'Tea',
  date: '2030-01-02',
  time: '10:00',
  missed: false,
  firedAt: 0,
  sound: true,
  test: false,
  ...over,
});

async function fresh() {
  vi.resetModules();
  return await import('./reminders');
}

beforeEach(() => {
  vi.useFakeTimers();
  calls.length = 0;
  handlers.clear();
  listeners = 0;
});
afterEach(() => {
  vi.useRealTimers();
});

describe('tauri: commands', () => {
  it('addReminder sends the reminder defaults for a timed event', async () => {
    const r = await fresh();
    await r.addReminder({ date: '2030-01-02', title: 'Tea', time: '10:00' });
    expect(calls[0]).toEqual({
      cmd: 'reminder_add',
      args: { date: '2030-01-02', title: 'Tea', time: '10:00', remind: true, sound: true },
    });
  });

  it('an all-day event never asks for an alert; explicit choices pass through', async () => {
    const r = await fresh();
    await r.addReminder({ date: '2030-01-02', title: 'Holiday', remind: true });
    await r.addReminder({ date: '2030-01-02', title: 'Quiet', time: '08:00', remind: false, sound: false });
    expect(calls[0].args).toMatchObject({ time: null, remind: false });
    expect(calls[1].args).toMatchObject({ time: '08:00', remind: false, sound: false });
  });

  it('set / snooze / dismiss / status / test-fire hit the matching commands', async () => {
    const r = await fresh();
    await r.setReminderAlert(7, { sound: false });
    await r.snoozeReminder(7, 10);
    await r.dismissReminder(7);
    await r.remindersStatus();
    await r.testFireReminder(7);
    expect(calls).toEqual([
      { cmd: 'reminder_set', args: { id: 7, remind: null, sound: false } },
      { cmd: 'reminder_snooze', args: { id: 7, minutes: 10 } },
      { cmd: 'reminder_dismiss', args: { id: 7 } },
      { cmd: 'reminders_status', args: undefined },
      { cmd: 'reminder_test_fire', args: { id: 7, ignoreFocus: false } },
    ]);
  });

  it('events from the backend come back with the defaults filled in', async () => {
    const r = await fresh();
    const [e] = await r.loadReminders();
    expect(e).toMatchObject({ remind: true, sound: true, missed: false });
  });
});

describe('tauri: the in-app banner', () => {
  const sink = () => ({ toast: vi.fn(), activity: vi.fn(), refresh: vi.fn() });

  it('subscribes to reminder-fired once and shows one banner per alert', async () => {
    const r = await fresh();
    const s = sink();
    await r.startReminders(s);
    await r.startReminders(s); // idempotent
    expect(listeners).toBe(1);
    expect([...handlers.keys()]).toEqual(['reminder-fired']);

    emit('reminder-fired', fired());
    expect(s.toast).not.toHaveBeenCalled(); // batching window
    vi.advanceTimersByTime(r.BANNER_BATCH_MS);
    expect(s.toast).toHaveBeenCalledTimes(1);
    expect(s.toast).toHaveBeenCalledWith('Reminder: Tea', 6000);
    expect(s.activity).toHaveBeenCalledWith('Reminder: Tea');
    expect(s.refresh).toHaveBeenCalledTimes(1);
  });

  it('says so when it was missed', async () => {
    const r = await fresh();
    const s = sink();
    await r.startReminders(s);
    emit('reminder-fired', fired({ missed: true }));
    vi.advanceTimersByTime(r.BANNER_BATCH_MS);
    expect(s.toast).toHaveBeenCalledWith('Missed reminder: Tea', 6000);
  });

  it('a burst becomes a single banner', async () => {
    const r = await fresh();
    const s = sink();
    await r.startReminders(s);
    for (const title of ['A', 'B', 'C']) emit('reminder-fired', fired({ title }));
    vi.advanceTimersByTime(r.BANNER_BATCH_MS);
    expect(s.toast).toHaveBeenCalledTimes(1);
    expect(s.toast).toHaveBeenCalledWith('3 reminders: A, B, +1 more', 6000);
  });

  it('two alerts far apart are two banners', async () => {
    const r = await fresh();
    const s = sink();
    await r.startReminders(s);
    emit('reminder-fired', fired({ title: 'A' }));
    vi.advanceTimersByTime(r.BANNER_BATCH_MS + 1);
    emit('reminder-fired', fired({ title: 'B' }));
    vi.advanceTimersByTime(r.BANNER_BATCH_MS + 1);
    expect(s.toast.mock.calls.map((c) => c[0])).toEqual(['Reminder: A', 'Reminder: B']);
  });

  it('stopping unsubscribes and drops a pending banner', async () => {
    const r = await fresh();
    const s = sink();
    const off = await r.startReminders(s);
    emit('reminder-fired', fired());
    off();
    expect(listeners).toBe(0);
    vi.advanceTimersByTime(5_000);
    expect(s.toast).not.toHaveBeenCalled();
    // and it can be started again afterwards
    await r.startReminders(s);
    expect(listeners).toBe(1);
  });
});
