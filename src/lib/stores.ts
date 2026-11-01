// Shared UI state. Plain `svelte/store` writables so components and non-component modules
// share them. Everything that is "the user's setup" (theme, region, timers) persists to
// localStorage on a best-effort basis; nothing here ever leaves the device.

import { derived, get, writable, type Readable, type Writable } from 'svelte/store';
import { bridge, invoke, isTauri, listen, mode } from './bridge';
import type { IconName } from './icons';
import type { VoiceResultItem } from './voice';
import type { ShelfItem, StoredShelf, ToolId } from './types';

// ---------------------------------------------------------------------------------------
// environment
// ---------------------------------------------------------------------------------------

const search = typeof location !== 'undefined' ? new URLSearchParams(location.search) : new URLSearchParams();
/** `?embed=1`: the website supplies the wallpaper, so the page background stays transparent. */
export const isEmbed = search.get('embed') === '1';

/** Stamp the document so CSS can adapt (`data-env`) before the first paint. */
export function initEnv(): void {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  root.dataset.env = isTauri() ? 'tauri' : 'demo';
  if (isEmbed) root.dataset.embed = '1';
}

/** Invoke a Tauri command when running natively; resolves null in the browser demo or on error. */
export async function native<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!isTauri()) return null;
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    console.warn(`[krovi] ${cmd} failed`, e);
    return null;
  }
}

/** Subscribe to a Tauri event; a no-op (returns a noop unlisten) in the browser demo. */
export async function onNative<T>(event: string, cb: (payload: T) => void): Promise<() => void> {
  try {
    return await listen<T>(event, cb);
  } catch {
    return () => {};
  }
}

function persisted<T>(key: string, initial: T): Writable<T> {
  let start = initial;
  try {
    const raw = localStorage.getItem(key);
    if (raw != null) start = JSON.parse(raw) as T;
  } catch {
    /* blocked storage / corrupt value: fall back to the default */
  }
  const store = writable<T>(start);
  store.subscribe((v) => {
    try {
      localStorage.setItem(key, JSON.stringify(v));
    } catch {
      /* ignore */
    }
  });
  return store;
}

const uid = (): number => Math.floor(Math.random() * 1e9) + 1;

// ---------------------------------------------------------------------------------------
// theme
// ---------------------------------------------------------------------------------------
// Themes live in lib/theme.ts (personalization). The UI reaches them through
// components/integrations.ts (themeChoice, themes, startTheme), which App.svelte starts once.

// ---------------------------------------------------------------------------------------
// navigation + small UI state
// ---------------------------------------------------------------------------------------

export const activeTool = writable<ToolId>('home');
export const paletteOpen = writable<boolean>(false);
/** Holiday dataset region (codes come from `REGIONS` in lib/holidays). */
export const region = persisted<string>('krovi.region', 'IN');

export type Intent = 'expand' | 'collapse' | 'toggle';
const intentListeners = new Set<(i: Intent) => void>();
/** Ask the Island to change shape. Components never talk to the springs directly. */
export function emitIntent(i: Intent): void {
  intentListeners.forEach((l) => l(i));
}
export function onIntent(cb: (i: Intent) => void): () => void {
  intentListeners.add(cb);
  return () => intentListeners.delete(cb);
}
/** Switch tool and make sure the panel is open (voice commands, tray, palette). */
export function openTool(id: ToolId): void {
  activeTool.set(id);
  emitIntent('expand');
}

/** A shared 1-second clock (epoch ms) so clocks and timers tick together. */
export const now = writable<number>(Date.now());
let ticking = false;
export function startClock(): void {
  if (ticking || typeof window === 'undefined') return;
  ticking = true;
  const tick = () => {
    const t = Date.now();
    now.set(t);
    tickTimers(t);
    tickPomodoro(t);
  };
  // align to wall-clock seconds so digits roll exactly on the second
  setTimeout(() => {
    tick();
    setInterval(tick, 1000);
  }, 1000 - (Date.now() % 1000));
}

// ---------------------------------------------------------------------------------------
// live activity (the pill's transient line) + toasts
// ---------------------------------------------------------------------------------------

export interface Activity {
  id: number;
  kind: 'copy' | 'timer' | 'info';
  text: string;
  detail?: string;
  /** a small capsule after the text, e.g. what a voice command used as "this" */
  chip?: { icon: IconName; label: string; title?: string };
}
export const activity = writable<Activity | null>(null);
let activityTimer: ReturnType<typeof setTimeout> | null = null;

export function pushActivity(
  kind: Activity['kind'],
  text: string,
  detail?: string,
  ms = 2200,
  chip?: Activity['chip'],
): void {
  if (activityTimer) clearTimeout(activityTimer);
  const id = uid();
  activity.set({ id, kind, text, detail, chip });
  activityTimer = setTimeout(() => {
    activity.update((a) => (a && a.id === id ? null : a));
  }, ms);
}

export interface Toast {
  id: number;
  text: string;
  /** 'error' swaps the check for an alert glyph */
  tone?: 'error';
}
export const toast = writable<Toast | null>(null);
let toastTimer: ReturnType<typeof setTimeout> | null = null;
export function pushToast(text: string, ms = 1800, tone?: 'error'): void {
  if (toastTimer) clearTimeout(toastTimer);
  const id = uid();
  toast.set({ id, text, tone });
  toastTimer = setTimeout(() => toast.update((t) => (t && t.id === id ? null : t)), ms);
}

// ---------------------------------------------------------------------------------------
// timers (shared so Home and the pill can show what is running)
// ---------------------------------------------------------------------------------------

export interface TimerItem {
  id: number;
  label: string;
  duration: number;
  accumulated: number;
  runningSince: number | null;
}

export const timerElapsed = (t: TimerItem, n: number): number =>
  t.accumulated + (t.runningSince != null ? Math.max(0, n - t.runningSince) : 0);
export const timerRemaining = (t: TimerItem, n: number): number =>
  Math.max(0, t.duration - timerElapsed(t, n));
export const timerDone = (t: TimerItem, n: number): boolean => timerElapsed(t, n) >= t.duration;
export const timerProgress = (t: TimerItem, n: number): number =>
  t.duration <= 0 ? 1 : Math.min(1, timerElapsed(t, n) / t.duration);

function demoTimers(): TimerItem[] {
  if (mode() !== 'demo') return [];
  const t = Date.now();
  return [
    { id: 1, label: 'Tea', duration: 4 * 60_000, accumulated: 0, runningSince: t - 71_000 },
    { id: 2, label: 'Standup', duration: 10 * 60_000, accumulated: 0, runningSince: null },
  ];
}

export const timers = persisted<TimerItem[]>('krovi.timers', demoTimers());

export function timerAdd(label: string, minutes: number, start = false): void {
  const duration = Math.max(1, Math.round(minutes * 60_000));
  const item: TimerItem = {
    id: uid(),
    label: label.trim() || `${Math.round(minutes)} min`,
    duration,
    accumulated: 0,
    runningSince: start ? Date.now() : null,
  };
  timers.update((l) => [item, ...l]);
}
export function timerToggle(id: number): void {
  const t = Date.now();
  timers.update((l) =>
    l.map((x) => {
      if (x.id !== id) return x;
      if (x.runningSince != null) return { ...x, accumulated: timerElapsed(x, t), runningSince: null };
      if (timerDone(x, t)) return { ...x, accumulated: 0, runningSince: t };
      return { ...x, runningSince: t };
    }),
  );
}
export function timerReset(id: number): void {
  timers.update((l) => l.map((x) => (x.id === id ? { ...x, accumulated: 0, runningSince: null } : x)));
}
export function timerRemove(id: number): void {
  timers.update((l) => l.filter((x) => x.id !== id));
}

function tickTimers(n: number): void {
  const list = get(timers);
  if (!list.some((t) => t.runningSince != null && timerDone(t, n))) return;
  timers.set(
    list.map((t) => {
      if (t.runningSince != null && timerDone(t, n)) {
        pushActivity('timer', t.label, 'Timer done', 4500);
        pushToast(`${t.label} is done`);
        return { ...t, accumulated: t.duration, runningSince: null };
      }
      return t;
    }),
  );
}

// ---- stopwatch (memory only) ----
export interface Stopwatch {
  accumulated: number;
  runningSince: number | null;
  laps: number[];
}
export const stopwatch = writable<Stopwatch>({ accumulated: 0, runningSince: null, laps: [] });
export const swElapsed = (s: Stopwatch, n: number): number =>
  s.accumulated + (s.runningSince != null ? Math.max(0, n - s.runningSince) : 0);
export function swToggle(): void {
  const t = Date.now();
  stopwatch.update((s) =>
    s.runningSince != null
      ? { ...s, accumulated: swElapsed(s, t), runningSince: null }
      : { ...s, runningSince: t },
  );
}
export function swLap(): void {
  const t = Date.now();
  stopwatch.update((s) => ({ ...s, laps: [swElapsed(s, t), ...s.laps] }));
}
export function swReset(): void {
  stopwatch.set({ accumulated: 0, runningSince: null, laps: [] });
}

// ---------------------------------------------------------------------------------------
// pomodoro
// ---------------------------------------------------------------------------------------

export type PomoPhase = 'work' | 'short' | 'long';
export const POMO = { work: 25 * 60_000, short: 5 * 60_000, long: 15 * 60_000, every: 4 } as const;
export interface PomodoroState {
  phase: PomoPhase;
  duration: number;
  accumulated: number;
  runningSince: number | null;
  finished: boolean;
  completed: number;
  day: string;
}

const dayKey = (t = Date.now()): string => {
  const d = new Date(t);
  return `${d.getFullYear()}-${d.getMonth() + 1}-${d.getDate()}`;
};
const pomoFresh = (): PomodoroState => ({
  phase: 'work',
  duration: POMO.work,
  accumulated: 0,
  runningSince: null,
  finished: false,
  completed: 0,
  day: dayKey(),
});
export const pomodoro = persisted<PomodoroState>('krovi.pomodoro', pomoFresh());
// a new day starts a fresh count
pomodoro.update((p) => (p.day === dayKey() ? p : { ...p, completed: 0, day: dayKey() }));

export const pomoElapsed = (p: PomodoroState, n: number): number =>
  p.accumulated + (p.runningSince != null ? Math.max(0, n - p.runningSince) : 0);
export const pomoRemaining = (p: PomodoroState, n: number): number =>
  Math.max(0, p.duration - pomoElapsed(p, n));
export const pomoLabel = (ph: PomoPhase): string =>
  ph === 'work' ? 'Focus' : ph === 'short' ? 'Short break' : 'Long break';

export function pomodoroStart(): void {
  pomodoro.update((p) => (p.runningSince == null && !p.finished ? { ...p, runningSince: Date.now() } : p));
}
export function pomodoroPause(): void {
  const t = Date.now();
  pomodoro.update((p) =>
    p.runningSince != null ? { ...p, accumulated: pomoElapsed(p, t), runningSince: null } : p,
  );
}
export function pomodoroToggle(): void {
  if (get(pomodoro).runningSince != null) pomodoroPause();
  else pomodoroStart();
}
export function pomodoroResetPhase(): void {
  pomodoro.update((p) => ({ ...p, accumulated: 0, runningSince: null, finished: false }));
}
/** Move to the next phase and start it. */
export function pomodoroAdvance(): void {
  const t = Date.now();
  pomodoro.update((p) => {
    const nextPhase: PomoPhase =
      p.phase === 'work' ? (p.completed > 0 && p.completed % POMO.every === 0 ? 'long' : 'short') : 'work';
    return {
      ...p,
      phase: nextPhase,
      duration: POMO[nextPhase],
      accumulated: 0,
      runningSince: t,
      finished: false,
    };
  });
}
export function pomodoroResetCycle(): void {
  pomodoro.set(pomoFresh());
}

function tickPomodoro(n: number): void {
  const p = get(pomodoro);
  if (p.runningSince == null || pomoElapsed(p, n) < p.duration) return;
  const completed = p.phase === 'work' ? p.completed + 1 : p.completed;
  pomodoro.set({ ...p, accumulated: p.duration, runningSince: null, finished: true, completed });
  const text = p.phase === 'work' ? 'Focus complete' : 'Break over';
  pushActivity('timer', text, 'Pomodoro', 4500);
  pushToast(text);
}

/**
 * "Pause the timer", "resume the pomodoro", "stop the timer" (voice). Acts on whatever is running
 * (or paused, for resume) and returns a line saying what happened, including when nothing could.
 */
export function timerControl(
  action: 'pause' | 'resume' | 'stop',
  target: 'timer' | 'pomodoro',
  at = Date.now(),
): string {
  if (target === 'pomodoro') {
    const p = get(pomodoro);
    if (action === 'pause') {
      if (p.runningSince == null) return 'The pomodoro isn’t running';
      pomodoroPause();
      return 'Pomodoro paused';
    }
    if (action === 'resume') {
      if (p.runningSince != null) return 'The pomodoro is already running';
      if (p.finished) return 'That pomodoro phase is finished';
      pomodoroStart();
      return 'Pomodoro resumed';
    }
    pomodoroResetPhase();
    return 'Pomodoro stopped';
  }

  const list = get(timers);
  const running = list.filter((x) => x.runningSince != null && !timerDone(x, at));
  const paused = list.filter((x) => x.runningSince == null && x.accumulated > 0 && !timerDone(x, at));
  const name = (l: TimerItem[]) => (l.length === 1 ? l[0].label : `${l.length} timers`);
  if (action === 'pause') {
    if (!running.length) return 'No timer is running';
    running.forEach((x) => timerToggle(x.id));
    return `${name(running)} paused`;
  }
  if (action === 'resume') {
    if (!paused.length) return 'No timer is paused';
    paused.forEach((x) => timerToggle(x.id));
    return `${name(paused)} resumed`;
  }
  if (!running.length && !paused.length) return 'No timer is running';
  [...running, ...paused].forEach((x) => timerReset(x.id));
  return `${name([...running, ...paused])} stopped`;
}

/** The one running countdown the pill should surface (pomodoro first, else the soonest timer). */
export interface LiveTimer {
  label: string;
  remainingMs: number;
}
export const liveTimer: Readable<LiveTimer | null> = derived([timers, pomodoro, now], ([$t, $p, $n]) => {
  if ($p.runningSince != null && !$p.finished) {
    return { label: pomoLabel($p.phase), remainingMs: pomoRemaining($p, $n) };
  }
  const running = $t.filter((x) => x.runningSince != null && !timerDone(x, $n));
  if (!running.length) return null;
  running.sort((a, b) => timerRemaining(a, $n) - timerRemaining(b, $n));
  return { label: running[0].label, remainingMs: timerRemaining(running[0], $n) };
});

// ---------------------------------------------------------------------------------------
// notes created outside the Notes view (voice commands, dictation in the browser demo)
// ---------------------------------------------------------------------------------------

/** Bumps whenever a note was added from elsewhere, so a mounted Notes view reloads. */
export const noteTick = writable<number>(0);
const NOTES_KEY = 'krovi.notes';

/**
 * Create a note. Natively this is the `create_note` command (the Rust store is the source of
 * truth); in the browser demo it goes into the same localStorage list the Notes view uses.
 */
export async function addNote(body: string): Promise<boolean> {
  const text = body.trim();
  if (!text) return false;
  const t = Date.now();
  if (isTauri()) {
    const res = await native<unknown[]>('create_note', { body: text, now: t });
    if (!res) return false;
  } else {
    try {
      const raw = localStorage.getItem(NOTES_KEY);
      const base = raw ? (JSON.parse(raw) as unknown[]) : await bridge.loadNotes();
      localStorage.setItem(NOTES_KEY, JSON.stringify([{ id: t, body: text, created_at: t, updated_at: t }, ...base]));
    } catch {
      return false;
    }
  }
  noteTick.update((n) => n + 1);
  return true;
}

// ---------------------------------------------------------------------------------------
// data changed outside the view that shows it (voice commands)
// ---------------------------------------------------------------------------------------

/** Bumps when a calendar event was added elsewhere (voice reminder) so Calendar and Home reload. */
export const calendarTick = writable<number>(0);
/** Bumps when the file shelf changed elsewhere so a mounted File Shelf reloads. */
export const shelfTick = writable<number>(0);
const SHELF_KEY = 'krovi.shelves';

/**
 * Park file references on a shelf (the first one unless named), stored exactly the way the
 * File Shelf view stores them. References only: nothing is copied or moved.
 */
export async function addToShelf(
  entries: { name: string; path: string; size?: number }[],
  shelfName?: string,
): Promise<{ count: number; shelf: string } | null> {
  const wanted = entries.filter((e) => e.name || e.path);
  if (!wanted.length) return null;
  let shelves: StoredShelf[] = [];
  try {
    const raw = localStorage.getItem(SHELF_KEY);
    shelves = raw ? (JSON.parse(raw) as StoredShelf[]) : await bridge.loadShelves();
  } catch {
    shelves = [];
  }
  if (!shelves.length) shelves = [{ name: 'Desk', items: [] }];
  const idx = Math.max(0, shelfName ? shelves.findIndex((s) => s.name === shelfName) : 0);
  const have = new Set(shelves[idx].items.map((i) => i.stored_path));
  const t = Date.now();
  const fresh: ShelfItem[] = wanted
    .filter((e) => !have.has(e.path))
    .map((e, i) => ({ id: t + i, name: e.name, stored_path: e.path, size_bytes: e.size ?? 0, added_at: t }));
  if (!fresh.length) return { count: 0, shelf: shelves[idx].name };
  shelves = shelves.map((s, i) => (i === idx ? { ...s, items: [...fresh, ...s.items] } : s));
  try {
    localStorage.setItem(SHELF_KEY, JSON.stringify(shelves));
  } catch {
    return null;
  }
  shelfTick.update((n) => n + 1);
  return { count: fresh.length, shelf: shelves[idx].name };
}

/**
 * "Send this to my phone": set the paths and open Quick Transfer. The Transfer view (owned by
 * the transfer agent) should read `transferRequest` on mount, start a share for `paths`, and
 * then set the store back to null.
 */
export const transferRequest = writable<{ id: number; paths: string[] } | null>(null);
export function requestTransfer(paths: string[]): void {
  transferRequest.set({ id: uid(), paths });
  openTool('transfer');
}

// ---------------------------------------------------------------------------------------
// voice sheets: the rows a voice command produced, and the "What can I say?" help
// ---------------------------------------------------------------------------------------

export interface VoiceSheet {
  id: number;
  /** the line the backend sent ("Found 3 matches for ...") */
  feedback: string;
  rows: VoiceResultItem[];
}
/** The results sheet (search hits, a folder's newest items, ambiguous candidates); null = closed. */
export const voiceResults = writable<VoiceSheet | null>(null);
export const voiceHelpOpen = writable<boolean>(false);

export function showVoiceResults(feedback: string, rows: VoiceResultItem[]): void {
  voiceHelpOpen.set(false);
  voiceResults.set({ id: uid(), feedback, rows });
}
/** Close whichever voice sheet is open. Returns true if one was (so Esc can stop there). */
export function closeVoiceSheets(): boolean {
  const any = get(voiceHelpOpen) || get(voiceResults) !== null;
  voiceHelpOpen.set(false);
  voiceResults.set(null);
  return any;
}

/** Copy plain text to the OS clipboard (works in the webview and the browser demo). */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
