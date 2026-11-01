import { beforeEach, describe, expect, it } from 'vitest';
import {
  LIVE_FOR_MS,
  PAST_GRACE_MS,
  __test,
  addReminder,
  bannerText,
  canDismiss,
  canSnooze,
  clockOf,
  dismissReminder,
  dueMs,
  environmentNote,
  formatIn,
  isPastBy,
  loadReminders,
  normalizeEvent,
  reminderBadge,
  reminderState,
  setReminderAlert,
  snoozeReminder,
  startReminders,
  type ReminderEvent,
  type ReminderFired,
} from './reminders';

const ev = (over: Partial<ReminderEvent> = {}): ReminderEvent => ({
  id: 1,
  date: '2030-01-02',
  title: 'Tea',
  time: '10:00',
  remind: true,
  sound: true,
  ...over,
});
const at = (h: number, m: number) => new Date(2030, 0, 2, h, m).getTime();
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

describe('time helpers', () => {
  it('dueMs reads date+time on the local wall clock', () => {
    expect(dueMs('2026-11-01', '17:30')).toBe(new Date(2026, 10, 1, 17, 30).getTime());
    expect(dueMs('2026-11-01', '00:00')).toBe(new Date(2026, 10, 1, 0, 0).getTime());
  });

  it('dueMs is null for all-day, impossible dates and bad times', () => {
    expect(dueMs('2026-11-01', undefined)).toBeNull();
    expect(dueMs('2026-11-01', null)).toBeNull();
    expect(dueMs('2026-11-01', '')).toBeNull();
    expect(dueMs('2026-02-31', '10:00')).toBeNull(); // would roll into March
    expect(dueMs('2026-11-01', '25:00')).toBeNull();
    expect(dueMs('2026-11-01', '9:00')).toBeNull();
    expect(dueMs('nope', '10:00')).toBeNull();
  });

  it('clockOf pads to HH:MM', () => {
    expect(clockOf(new Date(2026, 10, 1, 7, 5).getTime())).toBe('07:05');
    expect(clockOf(new Date(2026, 10, 1, 23, 59).getTime())).toBe('23:59');
  });

  it('isPastBy allows the grace window and ignores all-day events', () => {
    const due = at(10, 0);
    expect(isPastBy('2030-01-02', '10:00', due - 1)).toBe(false);
    expect(isPastBy('2030-01-02', '10:00', due + PAST_GRACE_MS)).toBe(false);
    expect(isPastBy('2030-01-02', '10:00', due + PAST_GRACE_MS + 1)).toBe(true);
    expect(isPastBy('2030-01-02', undefined, due + 9e9)).toBe(false);
  });

  it('formatIn is short and honest', () => {
    expect(formatIn(0)).toBe('');
    expect(formatIn(-5)).toBe('');
    expect(formatIn(30_000)).toBe('in under a minute');
    expect(formatIn(4 * 60_000)).toBe('in 4 min');
    expect(formatIn(120 * 60_000)).toBe('in 2 h');
    expect(formatIn(125 * 60_000)).toBe('in 2 h 5 min');
    expect(formatIn(25 * 3_600_000)).toBe('');
  });
});

describe('normalizeEvent', () => {
  it('a timed event alerts with sound by default; all-day does not', () => {
    const t = normalizeEvent({ date: '2030-01-02', title: 'x', time: '10:00' });
    expect(t).toMatchObject({ remind: true, sound: true, missed: false });
    const d = normalizeEvent({ date: '2030-01-02', title: 'x' });
    expect(d).toMatchObject({ remind: false, sound: true });
  });

  it('keeps what the backend sent', () => {
    const n = normalizeEvent(ev({ remind: false, sound: false, missed: true, firedAt: 5 }));
    expect(n).toMatchObject({ remind: false, sound: false, missed: true, firedAt: 5 });
  });
});

describe('state and badge', () => {
  it('maps every stored shape to one state', () => {
    expect(reminderState(ev({ time: undefined }))).toBe('all-day');
    expect(reminderState(ev({ remind: false }))).toBe('off');
    expect(reminderState(ev())).toBe('upcoming');
    expect(reminderState(ev({ snoozedUntil: 5 }))).toBe('snoozed');
    expect(reminderState(ev({ firedAt: 5 }))).toBe('fired');
    expect(reminderState(ev({ firedAt: 5, missed: true }))).toBe('missed');
  });

  it('upcoming counts down, then falls back to a plain label when far away', () => {
    const due = at(10, 0);
    expect(reminderBadge(ev(), due - 4 * 60_000)).toMatchObject({ state: 'upcoming', text: 'Rings in 4 min', tone: 'accent' });
    expect(reminderBadge(ev(), due - 3 * 86_400_000).text).toBe('Alert on');
    expect(reminderBadge(ev({ sound: false }), due - 3 * 86_400_000).text).toBe('Alert on, silent');
  });

  it('a pending reminder whose time is gone (no scheduler, e.g. the browser demo) says so', () => {
    const due = at(10, 0);
    expect(reminderBadge(ev(), due + 1)).toMatchObject({ state: 'upcoming', text: 'Time passed', tone: 'quiet' });
  });

  it('fired / missed / snoozed / off / all-day', () => {
    const t = at(10, 0);
    expect(reminderBadge(ev({ firedAt: t }), t).text).toBe('Rang 10:00');
    expect(reminderBadge(ev({ firedAt: t, missed: true }), t)).toMatchObject({ text: 'Missed', tone: 'warn' });
    expect(reminderBadge(ev({ snoozedUntil: at(10, 15) }), t)).toMatchObject({ text: 'Snoozed to 10:15', tone: 'accent' });
    expect(reminderBadge(ev({ remind: false }), t)).toMatchObject({ text: 'Alert off', tone: 'quiet' });
    expect(reminderBadge(ev({ time: undefined }), t).text).toBe('');
  });
});

describe('quick actions', () => {
  const t = at(10, 0);
  it('snooze is offered right after it rang, and while snoozed', () => {
    expect(canSnooze(ev({ firedAt: t }), t + 60_000)).toBe(true);
    expect(canSnooze(ev({ firedAt: t, missed: true }), t + 60_000)).toBe(true);
    expect(canSnooze(ev({ snoozedUntil: t + 300_000 }), t)).toBe(true);
  });

  it('not for an upcoming, off or long-gone reminder', () => {
    expect(canSnooze(ev(), t)).toBe(false);
    expect(canSnooze(ev({ remind: false }), t)).toBe(false);
    expect(canSnooze(ev({ firedAt: t }), t + LIVE_FOR_MS + 1)).toBe(false);
    expect(canSnooze(ev({ time: undefined }), t)).toBe(false);
  });

  it('dismiss only cancels a snooze', () => {
    expect(canDismiss(ev({ snoozedUntil: t }))).toBe(true);
    expect(canDismiss(ev({ firedAt: t }))).toBe(false);
    expect(canDismiss(ev())).toBe(false);
  });
});

describe('bannerText', () => {
  it('names one reminder and says when it was missed or a test', () => {
    expect(bannerText([fired()])).toBe('Reminder: Tea');
    expect(bannerText([fired({ missed: true })])).toBe('Missed reminder: Tea');
    expect(bannerText([fired({ test: true })])).toBe('Test reminder: Tea');
    expect(bannerText([])).toBe('');
  });

  it('collapses a burst', () => {
    expect(bannerText([fired({ title: 'A' }), fired({ title: 'B' })])).toBe('2 reminders: A, B');
    expect(bannerText([fired({ title: 'A' }), fired({ title: 'B' }), fired({ title: 'C' }), fired({ title: 'D' })])).toBe(
      '4 reminders: A, B, +2 more',
    );
  });
});

describe('environmentNote', () => {
  const env = (toasts: string, focus: string) => ({ toasts, focus });
  it('is silent when nothing is in the way', () => {
    expect(environmentNote(env('enabled', 'off'))).toBeNull();
    expect(environmentNote(env('unknown', 'unknown'))).toBeNull();
    expect(environmentNote(env('unknown', 'away'))).toBeNull();
  });

  it('explains Do Not Disturb and friends', () => {
    expect(environmentNote(env('unknown', 'do-not-disturb'))).toMatch(/Do Not Disturb is on.*without a pop-up or sound/);
    expect(environmentNote(env('enabled', 'alarms-only'))).toMatch(/alarms only/);
    expect(environmentNote(env('enabled', 'quiet-time'))).toMatch(/quiet time/);
    expect(environmentNote(env('enabled', 'fullscreen'))).toMatch(/full-screen/);
    expect(environmentNote(env('enabled', 'presentation'))).toMatch(/full-screen/);
  });

  it('turned-off notifications win over a focus state', () => {
    expect(environmentNote(env('disabled-in-settings', 'do-not-disturb'))).toMatch(/turned off in Settings/);
    expect(environmentNote(env('disabled-for-app', 'off'))).toMatch(/turned off in Windows/);
  });
});

describe('browser demo (no scheduler: stored, never rings)', () => {
  beforeEach(() => __test.reset());

  const find = (l: ReminderEvent[], title: string) => l.find((x) => x.title === title)!;

  it('a timed event is a reminder with sound; defaults match the backend', async () => {
    const e = find(await addReminder({ date: '2031-06-01', title: '  Call  ', time: '09:00' }), 'Call');
    expect(e).toMatchObject({ time: '09:00', remind: true, sound: true });
  });

  it('all-day never alerts; explicit options are kept', async () => {
    let l = await addReminder({ date: '2031-06-02', title: 'Holiday-ish', remind: true });
    expect(find(l, 'Holiday-ish')).toMatchObject({ remind: false, sound: true });
    l = await addReminder({ date: '2031-06-02', title: 'Quiet one', time: '08:00', remind: true, sound: false });
    expect(find(l, 'Quiet one')).toMatchObject({ remind: true, sound: false });
    l = await addReminder({ date: '2031-06-02', title: 'No alert', time: '08:30', remind: false });
    expect(find(l, 'No alert').remind).toBe(false);
  });

  it('a time already past is stored without an alert', async () => {
    const e = find(await addReminder({ date: '2020-01-01', title: 'Long ago', time: '09:00' }), 'Long ago');
    expect(e.remind).toBe(false);
  });

  it('rejects a blank title like the backend', async () => {
    await expect(addReminder({ date: '2031-06-01', title: '   ', time: '09:00' })).rejects.toThrow();
  });

  it('snooze, dismiss and the alert switch walk the same states as the backend', async () => {
    const e = find(await addReminder({ date: '2031-07-01', title: 'Walk', time: '07:00' }), 'Walk');
    const id = e.id!;
    const before = Date.now();

    let s = find(await snoozeReminder(id, 5), 'Walk');
    expect(reminderState(s)).toBe('snoozed');
    expect(s.snoozedUntil!).toBeGreaterThanOrEqual(before + 5 * 60_000);
    expect(s.snoozedUntil!).toBeLessThan(before + 5 * 60_000 + 5_000);

    s = find(await dismissReminder(id), 'Walk');
    expect(s.snoozedUntil).toBeUndefined();
    expect(reminderState(s)).toBe('fired');

    s = find(await setReminderAlert(id, { remind: false }), 'Walk');
    expect(reminderState(s)).toBe('off');
    s = find(await setReminderAlert(id, { remind: true }), 'Walk');
    expect(reminderState(s)).toBe('upcoming'); // re-armed: fired/snooze cleared
    s = find(await setReminderAlert(id, { sound: false }), 'Walk');
    expect(s.sound).toBe(false);
    expect(reminderState(s)).toBe('upcoming');
  });

  it('loadReminders fills the defaults on fixture events', async () => {
    const l = await loadReminders();
    expect(l.length).toBeGreaterThan(0);
    for (const e of l) expect(typeof e.remind).toBe('boolean');
  });

  it('startReminders is a no-op in the browser', async () => {
    const off = await startReminders({ toast() {}, activity() {}, refresh() {} });
    expect(typeof off).toBe('function');
    off();
  });
});
