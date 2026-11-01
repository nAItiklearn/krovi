// Voice API (owner: voice) — docs/CONTRACTS.md §3, §9 + Voice v2/v3 (docs/DECISIONS.md D-012, D-013, D-014).
//
// TAURI: recognition is 100% on-device (Windows SAPI 5). This module only wires the three backend
//   events (`voice-state`, `voice-partial`, `voice-result`) into the `voiceState` store and fans
//   finished results out to subscribers. Dictation notes are created by the Rust side
//   (`notes-changed` fires). Parsed voice COMMANDS are delivered to `onVoiceCommand` callbacks:
//   * `result.executed === true`  -> Rust already did it (reminder saved, media key sent, folder
//     opened, app launched, files searched, folder listed): only show `result.feedback` and, for
//     Voice v3, the `result.results` rows (`openVoiceResult(path)` opens one).
//   * otherwise the UI acts (open a tool, timers, pomodoro, shelf, Quick Transfer, creating the
//     note for `new-note`, collapse).
//   The supported phrasings are a CLOSED grammar: see docs/SPEC.md section 11.13.
// DEMO (plain browser): a convincing simulation — listening, words appearing one by one, then a
//   result — so the website demo shows the flow. Nothing is recorded or recognised, and the UI must
//   do every action itself (`executed` is always false).
import { get, writable } from 'svelte/store';
import type { ToolId } from './types';
import { invoke, isTauri, listen } from './bridge';

export type VoiceMode = 'note' | 'command';
export type MediaWord = 'playpause' | 'play' | 'pause' | 'next' | 'previous' | 'whats-playing';
export type VoiceCommand =
  | { kind: 'open-tool'; tool: ToolId }
  | { kind: 'start-pomodoro' }
  | { kind: 'start-timer'; seconds: number }
  | { kind: 'new-note'; body: string }
  | { kind: 'collapse' }
  /** `date` is yyyy-mm-dd, `time` is HH:MM (24h) when one was spoken. Rust saves it (executed). */
  | { kind: 'add-reminder'; date: string; time?: string; title: string }
  /** UI adds to the file shelf: `paths` (Explorer selection) or `text` (selected text). */
  | { kind: 'shelf-add'; paths: string[]; text?: string }
  /** UI routes to Quick Transfer: `paths` or `text`. */
  | { kind: 'transfer-send'; paths: string[]; text?: string }
  /** Rust already sent the transport key / answered "what's playing" (executed). */
  | { kind: 'media'; command: MediaWord }
  | { kind: 'timer-control'; action: 'pause' | 'resume' | 'stop'; target: 'timer' | 'pomodoro' }
  // ---- Voice v3: local OS actions (Rust carries them out: `executed`, `feedback`, `results`) ----
  /** "open downloads": a folder from the closed set (Known Folders, Quick Access). `path` is resolved. */
  | { kind: 'open-folder'; name: string; path: string }
  /** "launch spotify": an installed Start-menu app. `path` is its launch target. */
  | { kind: 'open-app'; name: string; path: string }
  /** "find invoice": a file-name search of the Windows Search index. */
  | { kind: 'search-files'; query: string }
  /** "what's in downloads": count, size and the newest items (up to 50 rows). */
  | { kind: 'list-folder'; name: string; path: string };

/** One row of `VoiceResult.results` (search hits, a folder's newest items, ambiguous candidates). */
export interface VoiceResultItem {
  kind: 'folder' | 'app' | 'file';
  name: string;
  /** Absolute path (folder/file) or launch target (app). */
  path: string;
  /** e.g. "12.4 MB, Sun 1 Nov 10:30 AM" */
  detail?: string;
}

/** What "this" / "it" referred to, exactly as captured when the session started. */
export type VoiceContext =
  | { kind: 'text'; value: string; app?: string }
  | { kind: 'files'; value: string[]; app?: string }
  | { kind: 'app'; value: string };

export interface VoiceStateV {
  state: 'idle' | 'listening' | 'processing' | 'error' | 'unavailable';
  mode: VoiceMode | null;
  partial: string;
  error?: string;
  /** Text of the most recent finished session ("" if nothing was recognised). */
  lastText?: string;
}

/** Payload of the `voice-result` event / what `onVoiceResult` receives. */
export interface VoiceResult {
  mode: VoiceMode;
  /** Everything the recognizer heard ("" when nothing was recognised, and "" for commands when
   *  Keep transcripts is off). */
  text: string;
  /** Only for `mode: 'command'`, and only when the text parsed to a known command. */
  command: VoiceCommand | null;
  /** Exactly what context capture read when the session started (null if nothing / not asked). */
  context?: VoiceContext | null;
  /** A short plain-language line to show, e.g. "Reminder added for Fri 7 Nov, 9:00 AM: Call mom". */
  feedback?: string | null;
  /** True when the backend already carried the command out: show `feedback`, do not repeat it. */
  executed?: boolean;
  /** Voice v3: rows to show in the panel (search hits, a folder's newest items, the candidates of an
   *  ambiguous name). Absent or empty for every other command. Open one with `openVoiceResult`. */
  results?: VoiceResultItem[];
}

export interface VoiceAvailability {
  dictation: boolean;
  commands: boolean;
  engine: string;
}

interface StatePayload {
  state: 'idle' | 'listening' | 'processing' | 'error';
  mode: VoiceMode;
  error?: string;
}

export const voiceState = writable<VoiceStateV>({ state: 'idle', mode: null, partial: '' });

type CommandListener = (c: VoiceCommand, result: VoiceResult) => void;
const commandListeners = new Set<CommandListener>();
const resultListeners = new Set<(r: VoiceResult) => void>();

/**
 * Subscribe to parsed voice commands. The callback also receives the whole result, so it can
 * check `result.executed` (the backend already did it: just show `result.feedback`) and read
 * `result.context` (what "this" was). Returns an unsubscribe function.
 */
export function onVoiceCommand(cb: CommandListener): () => void {
  commandListeners.add(cb);
  return () => commandListeners.delete(cb);
}

/**
 * Subscribe to every finished session (including note dictation and "nothing recognised").
 * In the browser demo this is how a UI creates the dictated note, since there is no Rust side.
 */
export function onVoiceResult(cb: (r: VoiceResult) => void): () => void {
  resultListeners.add(cb);
  return () => resultListeners.delete(cb);
}

function patch(p: Partial<VoiceStateV>): void {
  voiceState.update((s) => ({ ...s, ...p }));
}

function deliver(result: VoiceResult): void {
  patch({ lastText: result.text });
  for (const cb of [...resultListeners]) {
    try {
      cb(result);
    } catch (e) {
      console.error('[voice] result listener failed', e);
    }
  }
  if (result.command) {
    for (const cb of [...commandListeners]) {
      try {
        cb(result.command, result);
      } catch (e) {
        console.error('[voice] command listener failed', e);
      }
    }
  }
}

// ---------------------------------------------------------------------------------------------
// Backend events -> store (pure reducers, exported for tests)
// ---------------------------------------------------------------------------------------------

export function reduceState(prev: VoiceStateV, p: StatePayload): VoiceStateV {
  switch (p.state) {
    case 'listening':
      return { ...prev, state: 'listening', mode: p.mode, partial: '', error: undefined };
    case 'processing':
      return { ...prev, state: 'processing', mode: p.mode, error: undefined };
    case 'error':
      return { ...prev, state: 'error', mode: p.mode, partial: '', error: p.error || 'Voice input failed.' };
    default:
      return { ...prev, state: 'idle', mode: null, partial: '', error: undefined };
  }
}

/** An error must not leave the pill stuck: fall back to idle after a few seconds. */
const ERROR_RESET_MS = 6000;
let errorTimer: ReturnType<typeof setTimeout> | null = null;

function onBackendState(p: StatePayload): void {
  voiceState.update((s) => reduceState(s, p));
  if (errorTimer) clearTimeout(errorTimer);
  errorTimer = null;
  if (p.state === 'error') {
    errorTimer = setTimeout(() => {
      voiceState.update((s) => (s.state === 'error' ? { ...s, state: 'idle', mode: null, partial: '' } : s));
    }, ERROR_RESET_MS);
  }
}

let wired: Promise<void> | null = null;

function wire(): Promise<void> {
  if (!wired) {
    wired = (async () => {
      await Promise.all([
        listen<StatePayload>('voice-state', onBackendState),
        listen<{ text: string }>('voice-partial', (p) => patch({ partial: p.text })),
        listen<VoiceResult>('voice-result', deliver),
      ]);
    })();
  }
  return wired;
}

// ---------------------------------------------------------------------------------------------
// Privacy settings (persisted by the backend in the local settings store; never sent anywhere)
// ---------------------------------------------------------------------------------------------

/** How much Voice may read from the app in front when a voice command starts. */
export type ContextCapture = 'off' | 'selection' | 'selection+clipboard-fallback';

export interface VoiceSettings {
  /** Always `true` and locked: speech is recognised on this PC by Windows SAPI. Show it, do not offer a switch. */
  readonly localOnly: true;
  /** Always `false` and locked: Krovi never stores audio. Show it, do not offer a switch. */
  readonly storeAudio: false;
  /** `true` (default) keeps what was said; `false`: voice COMMANDS leave no transcript trace anywhere
   *  (no live text, no `text` in results, no `lastText`). Dictated notes are always saved as the note. */
  keepTranscripts: boolean;
  /** `off`: read nothing. `selection` (default): app name + selected text + selected Explorer items.
   *  `selection+clipboard-fallback`: also, if UI Automation finds no text, briefly copy with Ctrl+C
   *  and restore the clipboard. */
  contextCapture: ContextCapture;
  /** Tidy dictated notes locally with simple rules: fillers, stutters, capitals, punctuation. */
  cleanTranscription: boolean;
}

export const DEFAULT_VOICE_SETTINGS: VoiceSettings = {
  localOnly: true,
  storeAudio: false,
  keepTranscripts: true,
  contextCapture: 'selection',
  cleanTranscription: true,
};

/** Validate arbitrary stored/received data into settings, re-imposing the two locked values. */
export function normalizeVoiceSettings(raw: unknown): VoiceSettings {
  const r = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
  const captures: ContextCapture[] = ['off', 'selection', 'selection+clipboard-fallback'];
  return {
    localOnly: true,
    storeAudio: false,
    keepTranscripts: typeof r.keepTranscripts === 'boolean' ? r.keepTranscripts : DEFAULT_VOICE_SETTINGS.keepTranscripts,
    contextCapture: captures.includes(r.contextCapture as ContextCapture) ? (r.contextCapture as ContextCapture) : DEFAULT_VOICE_SETTINGS.contextCapture,
    cleanTranscription: typeof r.cleanTranscription === 'boolean' ? r.cleanTranscription : DEFAULT_VOICE_SETTINGS.cleanTranscription,
  };
}

/** The settings as last read or saved (a convenience for a Settings view; keep it bound with `getVoiceSettings`). */
export const voiceSettings = writable<VoiceSettings>(DEFAULT_VOICE_SETTINGS);

const DEMO_SETTINGS_KEY = 'krovi.voice.settings';

function readDemoSettings(): VoiceSettings {
  try {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem(DEMO_SETTINGS_KEY) : null;
    return normalizeVoiceSettings(raw ? JSON.parse(raw) : null);
  } catch {
    return DEFAULT_VOICE_SETTINGS;
  }
}

/** Load the saved voice settings (Tauri: `voice_get_settings`; browser demo: localStorage). */
export async function getVoiceSettings(): Promise<VoiceSettings> {
  const s = isTauri() ? normalizeVoiceSettings(await invoke<unknown>('voice_get_settings').catch(() => null)) : readDemoSettings();
  voiceSettings.set(s);
  return s;
}

/** Change some settings and persist them. Resolves to what was actually stored. */
export async function setVoiceSettings(patchIn: Partial<Pick<VoiceSettings, 'keepTranscripts' | 'contextCapture' | 'cleanTranscription'>>): Promise<VoiceSettings> {
  const next = normalizeVoiceSettings({ ...get(voiceSettings), ...patchIn });
  let saved = next;
  if (isTauri()) {
    saved = normalizeVoiceSettings(await invoke<unknown>('voice_set_settings', { settings: next }));
  } else {
    try {
      localStorage.setItem(DEMO_SETTINGS_KEY, JSON.stringify(next));
    } catch {
      /* private mode: keep it in memory only */
    }
  }
  voiceSettings.set(saved);
  return saved;
}

/**
 * Plain-language, TRUE statements about what Voice does with your data. The Settings view can
 * show them verbatim. Each one is backed by the implementation (see docs/DECISIONS.md D-012/D-013).
 */
export const PRIVACY_FACTS: readonly string[] = [
  'Speech is recognized on this PC by Windows SAPI. Nothing is sent anywhere.',
  'Krovi never stores audio. Your voice becomes text in memory and the sound is discarded.',
  'The microphone is only on while you dictate or give a voice command, and it switches off by itself after a few seconds of silence.',
  'Krovi looks at your selection only at the moment you start a voice command, never in the background, and only reads the selected text or files and the name of the app (not its window title).',
  'Password fields that Windows marks as protected are never read.',
  'Nothing is saved unless the action itself saves it: a note, a reminder, or a shelf item.',
  'The optional clipboard fallback briefly copies your selection and then puts your previous clipboard back. If Windows clipboard history is on, Windows may record that copy. It is off unless you turn it on.',
  "Krovi turns off the recognizer's learning, so what you say is not added to your Windows speech profile.",
  'With Keep transcripts off, the words of a voice command are not kept anywhere; only what the command did remains. Dictated notes are always saved as the note you asked for.',
  'Tidying a dictated note uses simple rules on this PC, not an AI model.',
  "Saying \"open\", \"launch\", \"search for\" or \"what's in\" works entirely on this PC: the folders and apps come from Windows' own lists, finding files uses the Windows Search index, and nothing is sent anywhere.",
  'Voice only opens folders and launches apps from those lists. It never guesses a path, and a file it finds is shown in its folder, never run.',
] as const;

// ---------------------------------------------------------------------------------------------
// Availability
// ---------------------------------------------------------------------------------------------

let availability: Promise<VoiceAvailability> | null = null;

/** What this device can do. Cached; in the demo both are "available". */
export function voiceAvailability(): Promise<VoiceAvailability> {
  if (!availability) {
    availability = isTauri()
      ? invoke<VoiceAvailability>('voice_available').catch(() => ({ dictation: false, commands: false, engine: 'unavailable' }))
      : Promise.resolve({ dictation: true, commands: true, engine: 'demo (simulated)' });
  }
  return availability;
}

// ---------------------------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------------------------

export interface StartOptions {
  /**
   * Command mode only. Read what "this" / "it" refers to (selected text, selected Explorer items,
   * the foreground app's name) at the moment the session starts, as far as the Voice settings
   * allow. Pass `true` ONLY when voice is started from the pill or the global shortcut, while the
   * other app still has focus: the panel steals focus, and then there is nothing to read.
   */
  captureContext?: boolean;
}

export async function startVoice(mode: VoiceMode, opts: StartOptions = {}): Promise<void> {
  if (!isTauri()) {
    runDemo(mode);
    return;
  }

  await wire();
  const a = await voiceAvailability();
  if (mode === 'note' ? !a.dictation : !a.commands) {
    patch({ state: 'unavailable', mode: null, partial: '', error: 'Speech recognition is not available on this device.' });
    return;
  }
  // Optimistic: show "listening" at once; the backend confirms, or reports an error, within ~100 ms.
  patch({ state: 'listening', mode, partial: '', error: undefined });
  try {
    await invoke<void>('voice_start', { mode, captureContext: mode === 'command' && opts.captureContext === true });
  } catch (e) {
    // The backend also emits a voice-state error with the same reason; this covers the case where
    // it could not even be reached.
    onBackendState({ state: 'error', mode, error: String(e) });
  }
}

/** The folders and apps "open ..." / "what's in ..." can name. Names only - for a "what can I say?" list. */
export interface VoiceTargets {
  /** False while the first full scan of the Start menu is still running (Known Folders only until then). */
  ready: boolean;
  folders: { name: string; path: string }[];
  apps: string[];
}

const DEMO_TARGETS: VoiceTargets = {
  ready: true,
  folders: [
    { name: 'Desktop', path: 'C:\\Users\\you\\Desktop' },
    { name: 'Documents', path: 'C:\\Users\\you\\Documents' },
    { name: 'Downloads', path: 'C:\\Users\\you\\Downloads' },
    { name: 'Pictures', path: 'C:\\Users\\you\\Pictures' },
    { name: 'Music', path: 'C:\\Users\\you\\Music' },
    { name: 'Videos', path: 'C:\\Users\\you\\Videos' },
    { name: 'Krovi downloads', path: 'C:\\Users\\you\\Downloads\\Krovi' },
  ],
  apps: ['Calculator', 'Notepad', 'Paint', 'Settings'],
};

/** The closed sets of folder and app names voice understands (the real lists, enumerated from Windows). */
export async function getVoiceTargets(): Promise<VoiceTargets> {
  if (!isTauri()) return DEMO_TARGETS;
  try {
    return await invoke<VoiceTargets>('voice_known_targets');
  } catch {
    return { ready: false, folders: [], apps: [] };
  }
}

/**
 * Open one row of the latest voice result: a folder opens in Explorer, a file is shown selected in
 * its folder (never run), an app launches. Only rows the latest result showed are accepted. Resolves
 * to a short confirmation, rejects with the reason.
 */
export async function openVoiceResult(path: string): Promise<string> {
  if (!isTauri()) throw new Error('Opening items works in the desktop app only.');
  return await invoke<string>('voice_open_result', { path });
}

export async function stopVoice(): Promise<void> {
  if (!isTauri()) {
    stopDemo();
    return;
  }
  try {
    await invoke<void>('voice_stop');
  } catch {
    patch({ state: 'idle', mode: null, partial: '' });
  }
}

// ---------------------------------------------------------------------------------------------
// Browser demo: a simulated session
// ---------------------------------------------------------------------------------------------

const DEMO_NOTE = 'Remember to call the dentist tomorrow morning.';

/** Local yyyy-mm-dd, `offsetDays` from today (the demo's "tomorrow"). */
function isoDay(offsetDays: number): string {
  const d = new Date();
  d.setDate(d.getDate() + offsetDays);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

type DemoExtra = Pick<VoiceResult, 'feedback' | 'results'>;

const DEMO_COMMANDS: { say: string; command: () => VoiceCommand; extra?: () => DemoExtra }[] = [
  { say: 'open timers', command: () => ({ kind: 'open-tool', tool: 'timers' }) },
  { say: 'start a timer for five minutes', command: () => ({ kind: 'start-timer', seconds: 300 }) },
  { say: 'start pomodoro', command: () => ({ kind: 'start-pomodoro' }) },
  { say: 'new note buy milk', command: () => ({ kind: 'new-note', body: 'Buy milk' }) },
  { say: 'open notes', command: () => ({ kind: 'open-tool', tool: 'notes' }) },
  { say: 'add a reminder for tomorrow at nine to call mom', command: () => ({ kind: 'add-reminder', date: isoDay(1), time: '09:00', title: 'Call mom' }) },
  { say: 'pause the timer', command: () => ({ kind: 'timer-control', action: 'pause', target: 'timer' }) },
  { say: 'next track', command: () => ({ kind: 'media', command: 'next' }) },
  // Voice v3 (the browser cannot open folders or apps: these show what the panel would list)
  {
    say: 'search for invoice',
    command: () => ({ kind: 'search-files', query: 'invoice' }),
    extra: () => ({
      feedback: 'Found 3 matches for "invoice". (Demo: nothing was searched.)',
      results: [
        { kind: 'file', name: 'invoice-march.pdf', path: 'C:\\Users\\you\\Documents\\invoice-march.pdf', detail: '84 KB, Mon 2 Mar 10:12 AM' },
        { kind: 'file', name: 'invoice-april.pdf', path: 'C:\\Users\\you\\Documents\\invoice-april.pdf', detail: '79 KB, Thu 2 Apr 9:40 AM' },
        { kind: 'folder', name: 'Invoices', path: 'C:\\Users\\you\\Documents\\Invoices', detail: 'folder, Fri 3 Apr 4:05 PM' },
      ],
    }),
  },
  { say: 'open downloads', command: () => ({ kind: 'open-folder', name: 'Downloads', path: 'C:\\Users\\you\\Downloads' }), extra: () => ({ feedback: 'Opened Downloads. (Demo: nothing was opened.)' }) },
  { say: "what's in downloads", command: () => ({ kind: 'list-folder', name: 'Downloads', path: 'C:\\Users\\you\\Downloads' }),
    extra: () => ({
      feedback: 'Downloads: 3 items (2 files, 1 folder), 1.2 GB of files. Newest: setup.exe, photos, report.pdf. (Demo)',
      results: [
        { kind: 'file', name: 'setup.exe', path: 'C:\\Users\\you\\Downloads\\setup.exe', detail: '1.1 GB, Sun 1 Nov 9:00 AM' },
        { kind: 'folder', name: 'photos', path: 'C:\\Users\\you\\Downloads\\photos', detail: 'folder, Sat 31 Oct 6:30 PM' },
        { kind: 'file', name: 'report.pdf', path: 'C:\\Users\\you\\Downloads\\report.pdf', detail: '102 MB, Fri 30 Oct 2:15 PM' },
      ],
    }),
  },
  { say: 'launch notepad', command: () => ({ kind: 'open-app', name: 'Notepad', path: 'shell:AppsFolder\\Microsoft.WindowsNotepad_8wekyb3d8bbwe!App' }), extra: () => ({ feedback: 'Launched Notepad. (Demo: nothing was launched.)' }) },
];

let demoRun = 0;
let demoIndex = 0;
let demoTimers: ReturnType<typeof setTimeout>[] = [];
let demoFinish: (() => void) | null = null;

function clearDemoTimers(): void {
  demoTimers.forEach(clearTimeout);
  demoTimers = [];
}

function after(run: number, ms: number, fn: () => void): void {
  demoTimers.push(
    setTimeout(() => {
      if (run === demoRun) fn();
    }, ms),
  );
}

function runDemo(mode: VoiceMode): void {
  clearDemoTimers();
  const run = ++demoRun;
  const pick = mode === 'command' ? DEMO_COMMANDS[demoIndex++ % DEMO_COMMANDS.length] : null;
  const text = pick ? pick.say : DEMO_NOTE;
  const words = text.split(' ');
  const step = mode === 'note' ? 330 : 250;
  // Same rule as the real thing: with Keep transcripts off, a command leaves no transcript at all.
  const showText = mode === 'note' || readDemoSettings().keepTranscripts;

  patch({ state: 'listening', mode, partial: '', error: undefined });

  // A beat of silence, then the words appear one at a time.
  const lead = 650;
  words.forEach((_, i) => after(run, lead + i * step, () => patch({ partial: showText ? words.slice(0, i + 1).join(' ') : '' })));

  let done = false;
  const finish = () => {
    if (done || run !== demoRun) return;
    done = true;
    clearDemoTimers();
    demoFinish = null;
    patch({ state: 'idle', mode: null, partial: '' });
    const extra = pick?.extra?.();
    deliver({
      mode,
      text: showText ? text : '',
      command: pick ? pick.command() : null,
      context: null,
      feedback: extra?.feedback ?? null,
      executed: false,
      // only present when there are rows (a plain command result carries none)
      ...(extra?.results ? { results: extra.results } : {}),
    });
  };
  // A manual stop skips ahead: show the full phrase as "processing", then deliver.
  demoFinish = () => {
    demoFinish = null;
    clearDemoTimers();
    patch({ state: 'processing', mode, partial: showText ? text : '' });
    after(run, 250, finish);
  };

  const spoken = lead + words.length * step;
  after(run, spoken + (mode === 'note' ? 900 : 350), () => patch({ state: 'processing', partial: showText ? text : '' }));
  after(run, spoken + (mode === 'note' ? 1300 : 700), finish);
}

function stopDemo(): void {
  if (demoFinish) {
    demoFinish();
  } else {
    patch({ state: 'idle', mode: null, partial: '' });
  }
}
