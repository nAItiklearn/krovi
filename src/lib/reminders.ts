// Reminders & alarms (owner: reminders). docs/CONTRACTS.md §8.
//
// A reminder is a calendar event with a time. The Rust scheduler (src-tauri/src/reminders.rs)
// rings it at that time with a real Windows toast, whatever the window mode; this module is
// the UI side: the shared event shape, the pure labelling/banner helpers (unit-tested), thin
// wrappers over the backend commands, and the in-app banner for `reminder-fired`.
//
//   * Tauri: invoke/listen wrappers over `reminder_*` / `reminders_status`.
//   * Browser demo: there is no scheduler and no OS toast, so alerts are stored (remind/sound
//     flags and snoozes are kept in a local overlay on the demo calendar) but never ring.
//
// This file imports only `bridge` and types, so it stays free of UI/store imports; the app
// hands it the toast/pill/refresh callbacks (`startReminders`).

import { bridge, invoke, isTauri, listen } from './bridge';
import type { CalendarEvent } from './types';

/** Fields the backend adds to a calendar event (see krovi_store::wire::CalendarDto). */
export interface ReminderFields {
  /** Alert at `time`. Absent only on a draft; use {@link normalizeEvent}. */
  remind?: boolean;
  /** Play the Windows reminder sound with the alert. */
  sound?: boolean;
  /** Epoch ms of the last alert; absent while one is still pending. */
  firedAt?: number;
  /** Epoch ms the alert was snoozed to. */
  snoozedUntil?: number;
  /** The last alert was delivered late (the app was closed / the PC asleep). */
  missed?: boolean;
}
export type ReminderEvent = CalendarEvent & ReminderFields;

/** Payload of the `reminder-fired` event. */
export interface ReminderFired {
  id: number;
  title: string;
  date: string;
  time?: string;
  missed: boolean;
  firedAt: number;
  sound: boolean;
  /** QA test-fire: the stored reminder was not changed. */
  test: boolean;
}

/** One row of `reminders_status().pending`. */
export interface PendingReminder {
  id: number;
  title: string;
  date: string;
  time?: string;
  dueAt: number;
  inMs: number;
  dueLocal: string;
  sound: boolean;
  snoozed: boolean;
}

export interface RemindersStatus {
  now: number;
  nowLocal: string;
  utcOffsetMinutes: number;
  pending: PendingReminder[];
  next: PendingReminder | null;
  scheduler: { running: boolean; startedAt: number; passes: number; fires: number; lastFire: unknown; lastError: string | null };
  environment: {
    toastIdentity: 'installed' | 'dev';
    appUserModelId: string;
    toasts: string;
    focus: string;
    toastMayBeHidden: boolean;
  };
  persistenceError: string | null;
}

/** Snooze lengths offered in the UI (minutes). */
export const SNOOZE_CHOICES = [5, 10] as const;
/** A time this far (or less) in the past still rings immediately; older is stored with the alert off. */
export const PAST_GRACE_MS = 90_000;

// ---------------------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------------------

const HM = /^([01]\d|2[0-3]):([0-5]\d)$/;
const ISO = /^(\d{4})-(\d{2})-(\d{2})$/;

/** The local-clock instant (epoch ms) of an event, or `null` for all-day / unparseable. */
export function dueMs(date: string, time?: string | null): number | null {
  if (!time) return null;
  const d = ISO.exec(date);
  const t = HM.exec(time);
  if (!d || !t) return null;
  const when = new Date(Number(d[1]), Number(d[2]) - 1, Number(d[3]), Number(t[1]), Number(t[2]), 0, 0);
  // Date silently rolls 2026-02-31 into March; reject that instead
  if (when.getFullYear() !== Number(d[1]) || when.getMonth() !== Number(d[2]) - 1 || when.getDate() !== Number(d[3])) {
    return null;
  }
  return when.getTime();
}

/** `HH:MM` on the local wall clock for an epoch-ms instant. */
export function clockOf(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

/** Fill the reminder defaults the backend would: a timed event alerts, with sound. */
export function normalizeEvent(e: ReminderEvent): ReminderEvent {
  return { ...e, remind: e.remind ?? !!e.time, sound: e.sound ?? true, missed: e.missed ?? false };
}
export const normalizeEvents = (l: ReminderEvent[]): ReminderEvent[] => l.map(normalizeEvent);

/** `true` when `date` `time` already passed by more than the grace, so no alert could ring. */
export function isPastBy(date: string, time: string | null | undefined, nowMs: number, graceMs = PAST_GRACE_MS): boolean {
  const due = dueMs(date, time);
  return due !== null && due + graceMs < nowMs;
}

/** "in 4 min", "in 2 h 5 min", "in under a minute"; empty when it is over a day away or past. */
export function formatIn(ms: number): string {
  if (ms <= 0) return '';
  if (ms < 60_000) return 'in under a minute';
  const mins = Math.round(ms / 60_000);
  if (mins < 60) return `in ${mins} min`;
  if (mins < 24 * 60) {
    const h = Math.floor(mins / 60);
    const m = mins % 60;
    return m ? `in ${h} h ${m} min` : `in ${h} h`;
  }
  return '';
}

export type ReminderState =
  /** all-day: nothing can ring */
  | 'all-day'
  /** timed, but the alert is switched off */
  | 'off'
  /** waiting to ring at its time */
  | 'upcoming'
  /** snoozed, ringing again later */
  | 'snoozed'
  /** rang on time */
  | 'fired'
  /** rang late (the app was closed / the PC asleep) */
  | 'missed';

export function reminderState(e: ReminderEvent): ReminderState {
  if (!e.time) return 'all-day';
  const n = normalizeEvent(e);
  if (!n.remind) return 'off';
  if (n.firedAt !== undefined) return n.missed ? 'missed' : 'fired';
  if (n.snoozedUntil !== undefined) return 'snoozed';
  return 'upcoming';
}

export type Tone = 'quiet' | 'accent' | 'warn';
export interface Badge {
  state: ReminderState;
  text: string;
  tone: Tone;
}

/** The status chip for an agenda row. `nowMs` is the shared 1 s clock. */
export function reminderBadge(e: ReminderEvent, nowMs: number): Badge {
  const state = reminderState(e);
  switch (state) {
    case 'all-day':
      return { state, text: '', tone: 'quiet' };
    case 'off':
      return { state, text: 'Alert off', tone: 'quiet' };
    case 'snoozed':
      return { state, text: `Snoozed to ${clockOf(e.snoozedUntil as number)}`, tone: 'accent' };
    case 'fired':
      return { state, text: `Rang ${clockOf(e.firedAt as number)}`, tone: 'quiet' };
    case 'missed':
      return { state, text: 'Missed', tone: 'warn' };
    case 'upcoming': {
      const due = dueMs(e.date, e.time);
      // pending with its time already gone only happens without a scheduler (the browser demo)
      if (due !== null && due <= nowMs) return { state, text: 'Time passed', tone: 'quiet' };
      const when = due === null ? '' : formatIn(due - nowMs);
      return { state, text: when ? `Rings ${when}` : e.sound === false ? 'Alert on, silent' : 'Alert on', tone: 'accent' };
    }
  }
}

/** A rung reminder stays "live" (quick Snooze buttons) for this long after it rang. */
export const LIVE_FOR_MS = 30 * 60_000;

/** Offer Snooze 5 / 10: the alert just rang (or was missed), or it is already snoozed. */
export function canSnooze(e: ReminderEvent, nowMs: number): boolean {
  const s = reminderState(e);
  if (s === 'snoozed') return true;
  return (s === 'fired' || s === 'missed') && e.firedAt !== undefined && nowMs - e.firedAt <= LIVE_FOR_MS;
}
/** Offer Dismiss: there is a snooze to cancel. */
export function canDismiss(e: ReminderEvent): boolean {
  return reminderState(e) === 'snoozed';
}

/** The in-app banner line for one or a burst of alerts. */
export function bannerText(events: ReminderFired[]): string {
  if (events.length === 0) return '';
  if (events.length === 1) {
    const e = events[0];
    return `${e.test ? 'Test reminder' : e.missed ? 'Missed reminder' : 'Reminder'}: ${e.title}`;
  }
  const names = events.slice(0, 2).map((e) => e.title);
  const more = events.length - names.length;
  return `${events.length} reminders: ${names.join(', ')}${more > 0 ? `, +${more} more` : ''}`;
}

// ---------------------------------------------------------------------------------------
// Backend calls (demo: a local overlay on the demo calendar)
// ---------------------------------------------------------------------------------------

const demoMeta = new Map<number, ReminderFields>();

function withDemoMeta(list: CalendarEvent[]): ReminderEvent[] {
  return list.map((e) => normalizeEvent({ ...e, ...(e.id !== undefined ? demoMeta.get(e.id) : undefined) }));
}

/** The whole calendar with reminder state, chronological. */
export async function loadReminders(): Promise<ReminderEvent[]> {
  const list = (await bridge.loadCalendar()) as ReminderEvent[];
  return isTauri() ? normalizeEvents(list) : withDemoMeta(list);
}

/**
 * Add an event. With a time it is a reminder: `remind` defaults to on, `sound` to on. A time
 * already past is stored with the alert off (it could never ring). Resolves to the new list.
 */
export async function addReminder(ev: {
  date: string;
  title: string;
  time?: string | null;
  remind?: boolean;
  sound?: boolean;
}): Promise<ReminderEvent[]> {
  const time = ev.time || null;
  if (isTauri()) {
    const list = await invoke<ReminderEvent[]>('reminder_add', {
      date: ev.date,
      title: ev.title,
      time,
      remind: time ? (ev.remind ?? true) : false,
      sound: ev.sound ?? true,
    });
    return normalizeEvents(list);
  }
  const before = new Set((await bridge.loadCalendar()).map((e) => e.id));
  const list = await bridge.addCalendarEvent({ date: ev.date, title: ev.title, time });
  const added = list.find((e) => !before.has(e.id));
  if (added?.id !== undefined) {
    const wantsAlert = !!time && (ev.remind ?? true) && !isPastBy(ev.date, time, Date.now());
    demoMeta.set(added.id, { remind: wantsAlert, sound: ev.sound ?? true });
  }
  return withDemoMeta(list);
}

/** Switch an event's alert and/or sound (turning the alert on re-arms it). */
export async function setReminderAlert(id: number, patch: { remind?: boolean; sound?: boolean }): Promise<ReminderEvent[]> {
  if (isTauri()) {
    return normalizeEvents(await invoke<ReminderEvent[]>('reminder_set', { id, remind: patch.remind ?? null, sound: patch.sound ?? null }));
  }
  const cur = demoMeta.get(id) ?? {};
  const next: ReminderFields = { ...cur, ...patch };
  if (patch.remind === true) {
    delete next.firedAt;
    delete next.snoozedUntil;
    next.missed = false;
  }
  if (patch.remind === false) delete next.snoozedUntil;
  demoMeta.set(id, next);
  return withDemoMeta(await bridge.loadCalendar());
}

/** Ring again in `minutes`. */
export async function snoozeReminder(id: number, minutes: number): Promise<ReminderEvent[]> {
  if (isTauri()) return normalizeEvents(await invoke<ReminderEvent[]>('reminder_snooze', { id, minutes }));
  const next: ReminderFields = { ...demoMeta.get(id), remind: true, snoozedUntil: Date.now() + minutes * 60_000, missed: false };
  delete next.firedAt;
  demoMeta.set(id, next);
  return withDemoMeta(await bridge.loadCalendar());
}

/** Acknowledge: stop a snooze / pending alert without ringing. */
export async function dismissReminder(id: number): Promise<ReminderEvent[]> {
  if (isTauri()) return normalizeEvents(await invoke<ReminderEvent[]>('reminder_dismiss', { id }));
  const cur = demoMeta.get(id);
  const next: ReminderFields = { ...cur, firedAt: cur?.firedAt ?? Date.now() };
  delete next.snoozedUntil;
  demoMeta.set(id, next);
  return withDemoMeta(await bridge.loadCalendar());
}

/** Scheduler diagnostics, upcoming alerts and the toast environment (Tauri only). */
export async function remindersStatus(): Promise<RemindersStatus | null> {
  return isTauri() ? invoke<RemindersStatus>('reminders_status') : null;
}

/** QA: ring a stored reminder right now without changing it (Tauri only). `ignoreFocus` plays the sound even under Do Not Disturb. */
export async function testFireReminder(id: number, ignoreFocus = false): Promise<unknown> {
  return isTauri() ? invoke('reminder_test_fire', { id, ignoreFocus }) : null;
}

/**
 * Why a Windows pop-up (and sound) may not show right now, in one plain sentence; `null` when
 * nothing is known to get in the way. The alert is never lost: it waits in the notification
 * centre, and the Calendar / in-app banner still show it.
 */
export function environmentNote(env: Pick<RemindersStatus['environment'], 'toasts' | 'focus'>): string | null {
  switch (env.toasts) {
    case 'disabled-in-settings':
      return 'Windows notifications are turned off in Settings, so reminders will not pop up.';
    case 'disabled-for-app':
    case 'disabled-for-user':
    case 'disabled-by-policy':
    case 'disabled-by-manifest':
      return 'Notifications for this app are turned off in Windows, so reminders will not pop up.';
  }
  switch (env.focus) {
    case 'do-not-disturb':
      return 'Do Not Disturb is on: reminders go to the notification centre without a pop-up or sound.';
    case 'alarms-only':
      return 'Windows is set to alarms only: reminders go to the notification centre without a pop-up or sound.';
    case 'quiet-time':
      return 'Windows quiet time is on: reminders go to the notification centre without a pop-up or sound.';
    case 'busy':
    case 'fullscreen':
    case 'presentation':
      return 'A full-screen app or presentation is in front, so Windows may hold the pop-up back.';
    default:
      return null;
  }
}

// ---------------------------------------------------------------------------------------
// In-app banner
// ---------------------------------------------------------------------------------------

export interface BannerSink {
  /** A toast in the open panel. */
  toast(text: string, ms?: number): void;
  /** A line on the collapsed pill. */
  activity(text: string): void;
  /** Reload anything showing reminders. */
  refresh(): void;
}

/** Alerts that arrive within this window are shown as one banner ("3 reminders: ..."). */
export const BANNER_BATCH_MS = 400;
const BANNER_TOAST_MS = 6_000;

let stop: (() => void) | null = null;

/**
 * Show a banner whenever the backend rings a reminder (`reminder-fired`), on top of the real
 * Windows toast: it covers Focus Assist / Do Not Disturb, where Windows holds the toast back.
 * Idempotent; resolves to an unsubscribe function. A no-op in the browser demo.
 */
export async function startReminders(sink: BannerSink): Promise<() => void> {
  if (stop) return stop;
  if (!isTauri()) return () => {};
  let batch: ReminderFired[] = [];
  let timer: ReturnType<typeof setTimeout> | null = null;
  const flush = () => {
    timer = null;
    const text = bannerText(batch);
    batch = [];
    if (!text) return;
    sink.toast(text, BANNER_TOAST_MS);
    sink.activity(text);
    sink.refresh();
  };
  const off = await listen<ReminderFired>('reminder-fired', (p) => {
    batch.push(p);
    if (timer === null) timer = setTimeout(flush, BANNER_BATCH_MS);
  });
  stop = () => {
    off();
    if (timer !== null) clearTimeout(timer);
    timer = null;
    batch = [];
    stop = null;
  };
  return stop;
}

/** Test seam. */
export const __test = {
  reset(): void {
    stop?.();
    demoMeta.clear();
  },
};
