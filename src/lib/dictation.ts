// Dictation anywhere (owner: panel-ui) - v0.3, D-020. Contract: src-tauri/src/dictation.rs.
//
// TAURI: hold (or toggle) a global shortcut, speak, and the text is typed into the app in front.
//   Whisper runs ON THIS PC from a model file the user chose to download. This module only wraps the
//   typed commands, keeps the two stores the Settings view and the pill read, and turns the backend
//   events (`dictation-state`, `dictation-result`, `voice-model-progress`, `voice-model-done`) into UI
//   state. It never touches audio and holds no business logic: validation, shortcut registration and
//   the model checks all stay in Rust, whose error text is shown as it is.
// DEMO (plain browser): there is NO dictation here. No `dictation_*` call is made, the model list is
//   shown with every model missing, and downloads, saving and the mic test are refused with one clear
//   message. Nothing is faked: no transcript, no progress, no level.
import { get, writable } from 'svelte/store';
import { invoke, isTauri, listen } from './bridge';

// ---------------------------------------------------------------------------------------------
// Types (mirror the Rust structs; settings are camelCase except `options`, which is snake_case)
// ---------------------------------------------------------------------------------------------

export type TalkMode = 'hold' | 'toggle';
export type DictationStyle = 'plain' | 'message' | 'email' | 'note' | 'code' | 'professional' | 'casual';

export interface Replacement {
  spoken: string;
  written: string;
}

export interface DictationOptions {
  style: DictationStyle;
  replacements: Replacement[];
  remove_fillers: boolean;
  auto_punctuate: boolean;
}

export interface DictationSettings {
  enabled: boolean;
  /** e.g. "Ctrl+Alt+D" */
  shortcut: string;
  mode: TalkMode;
  /** catalog id: "tiny" | "base" | "small" | "base.en" */
  model: string;
  /** "auto" or an ISO 639-1 code from `DICTATION_LANGUAGES` */
  language: string;
  /** microphone endpoint id; null = the Windows default */
  device: string | null;
  vocabulary: string[];
  options: DictationOptions;
  /** Talk to Agent Mode: a second shortcut whose words go to the planner and are never typed (D-020). */
  agentEnabled: boolean;
  /** e.g. "Ctrl+Alt+A". Follows the same hold / toggle choice as dictation. */
  agentShortcut: string;
}

export type ModelStatus = { state: 'installed'; path: string; bytes: number } | { state: 'missing' } | { state: 'partial' };

export interface ModelInfo {
  id: string;
  displayName: string;
  sizeBytes: number;
  multilingual: boolean;
  license: string;
  status: ModelStatus;
}

export interface InputDevice {
  id: string;
  name: string;
  isDefault: boolean;
}

export type DictationPhase = 'idle' | 'listening' | 'loading-model' | 'transcribing';

/** What a recording is for: typed into the app in front ("dictate") or sent to Agent Mode ("agent"). */
export type SessionKind = 'dictate' | 'agent';

/** Payload of the `dictation-state` event. */
export interface StatePayload {
  state: DictationPhase;
  level?: number;
  mode?: SessionKind;
}

/** Payload of the `dictation-result` event. */
export interface DictationResult {
  ok: boolean;
  mode?: SessionKind;
  /** Present on success, and with `ok: false` when typing was blocked (the text is then offered to copy). */
  text?: string;
  chars?: number;
  nothingHeard?: boolean;
  error?: string;
  language?: string;
}

export interface ModelProgress {
  id: string;
  downloaded: number;
  total: number;
}

export interface ModelDone {
  id: string;
  ok: boolean;
  error?: string;
}

// ---------------------------------------------------------------------------------------------
// Static choices (display only; the backend validates what it is sent)
// ---------------------------------------------------------------------------------------------

export const DICTATION_LANGUAGES: { id: string; label: string }[] = [
  { id: 'auto', label: 'Auto-detect' },
  { id: 'en', label: 'English' },
  { id: 'hi', label: 'Hindi' },
  { id: 'es', label: 'Spanish' },
  { id: 'fr', label: 'French' },
  { id: 'de', label: 'German' },
  { id: 'pt', label: 'Portuguese' },
  { id: 'it', label: 'Italian' },
  { id: 'nl', label: 'Dutch' },
  { id: 'ja', label: 'Japanese' },
  { id: 'zh', label: 'Chinese' },
  { id: 'ko', label: 'Korean' },
  { id: 'ru', label: 'Russian' },
  { id: 'ar', label: 'Arabic' },
  { id: 'bn', label: 'Bengali' },
  { id: 'ta', label: 'Tamil' },
  { id: 'te', label: 'Telugu' },
  { id: 'mr', label: 'Marathi' },
  { id: 'gu', label: 'Gujarati' },
  { id: 'ur', label: 'Urdu' },
];

export const DICTATION_STYLES: { id: DictationStyle; label: string; hint: string }[] = [
  { id: 'plain', label: 'Plain', hint: 'Tidy sentences with a full stop.' },
  { id: 'message', label: 'Message', hint: 'Like plain, but a short single sentence gets no full stop.' },
  { id: 'email', label: 'Email', hint: 'Say “new paragraph” to break. A greeting and sign-off get their own lines.' },
  { id: 'note', label: 'Note', hint: 'Say “new item” or “bullet” to start a list item.' },
  { id: 'code', label: 'Code', hint: 'Spoken symbols like “open paren”, and no capital letters.' },
  { id: 'professional', label: 'Professional', hint: 'Writes “do not” instead of “don’t”.' },
  { id: 'casual', label: 'Casual', hint: 'Writes “don’t” instead of “do not”.' },
];

/** Input limits, only so the fields can stop typing early. The backend is the authority. */
export const VOCAB_MAX_TERM = 60;
export const REPLACE_MAX_SPOKEN = 60;
export const REPLACE_MAX_WRITTEN = 200;

/** Shown wherever a browser visitor would otherwise expect dictation to work. */
export const DESKTOP_ONLY = 'Available in the desktop app.';
export const DESKTOP_ONLY_LONG = 'Dictation records and recognises speech on your PC, so it only runs in the Krovi desktop app. This browser demo can’t do either.';
export const NOTHING_HEARD = 'Didn’t catch that';
export const DOWNLOAD_NOTE = 'Downloads from Hugging Face (huggingface.co). Speech is recognised on this PC; audio never leaves it.';

export const DEFAULT_DICTATION_SETTINGS: DictationSettings = {
  enabled: false,
  shortcut: 'Ctrl+Alt+D',
  mode: 'hold',
  model: 'base',
  language: 'auto',
  device: null,
  vocabulary: [],
  options: { style: 'plain', replacements: [], remove_fillers: true, auto_punctuate: true },
  agentEnabled: false,
  agentShortcut: 'Ctrl+Alt+A',
};

const STYLE_IDS = DICTATION_STYLES.map((s) => s.id);

// ---------------------------------------------------------------------------------------------
// Normalisers (defensive: the backend always sends a full object, but a stale one must not crash the UI)
// ---------------------------------------------------------------------------------------------

function rec(v: unknown): Record<string, unknown> {
  return v && typeof v === 'object' ? (v as Record<string, unknown>) : {};
}
const str = (v: unknown, d: string) => (typeof v === 'string' ? v : d);
const bool = (v: unknown, d: boolean) => (typeof v === 'boolean' ? v : d);
const num = (v: unknown, d: number) => (typeof v === 'number' && Number.isFinite(v) ? v : d);

export function normalizeDictationSettings(raw: unknown): DictationSettings {
  const d = DEFAULT_DICTATION_SETTINGS;
  const r = rec(raw);
  const o = rec(r.options);
  const mode = r.mode === 'toggle' || r.mode === 'hold' ? r.mode : d.mode;
  const style = STYLE_IDS.includes(o.style as DictationStyle) ? (o.style as DictationStyle) : d.options.style;
  return {
    enabled: bool(r.enabled, d.enabled),
    shortcut: str(r.shortcut, d.shortcut),
    mode,
    model: str(r.model, d.model),
    language: str(r.language, d.language),
    device: typeof r.device === 'string' && r.device ? r.device : null,
    vocabulary: Array.isArray(r.vocabulary) ? r.vocabulary.filter((t): t is string => typeof t === 'string') : [],
    options: {
      style,
      replacements: Array.isArray(o.replacements)
        ? o.replacements.map(rec).map((x) => ({ spoken: str(x.spoken, ''), written: str(x.written, '') }))
        : [],
      remove_fillers: bool(o.remove_fillers, d.options.remove_fillers),
      auto_punctuate: bool(o.auto_punctuate, d.options.auto_punctuate),
    },
    agentEnabled: bool(r.agentEnabled, d.agentEnabled),
    agentShortcut: str(r.agentShortcut, d.agentShortcut),
  };
}

function normalizeStatus(raw: unknown): ModelStatus {
  const s = rec(raw);
  if (s.state === 'installed') return { state: 'installed', path: str(s.path, ''), bytes: num(s.bytes, 0) };
  if (s.state === 'partial') return { state: 'partial' };
  return { state: 'missing' };
}

export function normalizeModels(raw: unknown): ModelInfo[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .map(rec)
    .filter((m) => typeof m.id === 'string' && m.id)
    .map((m) => ({
      id: m.id as string,
      displayName: str(m.displayName, m.id as string),
      sizeBytes: num(m.sizeBytes, 0),
      multilingual: bool(m.multilingual, true),
      license: str(m.license, ''),
      status: normalizeStatus(m.status),
    }));
}

/** The model catalog as the desktop app ships it, shown (all missing) in the browser demo. Display only. */
const DEMO_LICENSE = 'MIT, OpenAI Whisper weights via ggerganov/whisper.cpp';
export const DEMO_MODELS: ModelInfo[] = [
  { id: 'tiny', displayName: 'Tiny (multilingual)', sizeBytes: 32_152_673, multilingual: true, license: DEMO_LICENSE, status: { state: 'missing' } },
  { id: 'base', displayName: 'Base (multilingual)', sizeBytes: 59_707_625, multilingual: true, license: DEMO_LICENSE, status: { state: 'missing' } },
  { id: 'small', displayName: 'Small (multilingual)', sizeBytes: 190_085_487, multilingual: true, license: DEMO_LICENSE, status: { state: 'missing' } },
  { id: 'base.en', displayName: 'Base (English only)', sizeBytes: 59_721_011, multilingual: false, license: DEMO_LICENSE, status: { state: 'missing' } },
];

// ---------------------------------------------------------------------------------------------
// Settings-side store: settings, model list, downloads
// ---------------------------------------------------------------------------------------------

export interface DictationInfo {
  /** False in the browser demo: nothing below is real and nothing can be changed. */
  available: boolean;
  /** `dictation_status` has answered at least once (or the demo state was set). */
  loaded: boolean;
  settings: DictationSettings;
  models: ModelInfo[];
  modelsDir: string | null;
  /** Catalog id being downloaded right now. */
  downloading: string | null;
  progress: ModelProgress | null;
  /** Last download failure, shown on that model's row. A cancel is not a failure. */
  modelError: { id: string; message: string } | null;
}

function initialInfo(): DictationInfo {
  return {
    available: isTauri(),
    loaded: false,
    settings: DEFAULT_DICTATION_SETTINGS,
    models: isTauri() ? [] : DEMO_MODELS,
    modelsDir: null,
    downloading: null,
    progress: null,
    modelError: null,
  };
}

export const dictationInfo = writable<DictationInfo>(initialInfo());

/** Plain text of whatever a rejected `invoke` carried (Rust sends a string). */
export function errorText(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

/** Fold a `dictation_status` answer into the store state. */
export function applyStatus(prev: DictationInfo, raw: unknown): DictationInfo {
  const r = rec(raw);
  const downloading = typeof r.downloading === 'string' && r.downloading ? r.downloading : null;
  return {
    ...prev,
    available: true,
    loaded: true,
    settings: normalizeDictationSettings(r.settings),
    models: normalizeModels(r.models),
    modelsDir: typeof r.modelsDir === 'string' ? r.modelsDir : null,
    downloading,
    progress: downloading && prev.progress?.id === downloading ? prev.progress : null,
  };
}

export function reduceProgress(prev: DictationInfo, p: ModelProgress): DictationInfo {
  return {
    ...prev,
    downloading: p.id,
    progress: { id: p.id, downloaded: p.downloaded, total: p.total },
    modelError: prev.modelError?.id === p.id ? null : prev.modelError,
  };
}

/** A finished (or cancelled, or failed) download. The caller refreshes the model list afterwards. */
export function reduceDone(prev: DictationInfo, d: ModelDone): DictationInfo {
  const cancelled = !d.ok && /cancel/i.test(d.error ?? '');
  return {
    ...prev,
    downloading: null,
    progress: null,
    modelError: d.ok || cancelled ? null : { id: d.id, message: d.error?.trim() || 'The download failed.' },
  };
}

/** 0..1 for a progress bar (0 while the total is unknown). */
export function progressFraction(p: ModelProgress | null): number {
  if (!p || !(p.total > 0)) return 0;
  return Math.min(1, Math.max(0, p.downloaded / p.total));
}

/** "57 MB". Binary megabytes, as Windows reports file sizes. */
export function formatMB(bytes: number): string {
  return `${Math.max(1, Math.round(bytes / (1024 * 1024)))} MB`;
}

/** "MIT, OpenAI Whisper weights via ..." -> "MIT" (the full text stays available as a tooltip). */
export function shortLicense(license: string): string {
  return license.split(',')[0].trim() || license;
}

// ---------------------------------------------------------------------------------------------
// Pure list helpers for the vocabulary / replacements editors (the backend re-validates everything)
// ---------------------------------------------------------------------------------------------

/** Trim and collapse blanks; ignore an empty term or one already listed (any case). */
export function addVocabularyTerm(list: string[], raw: string): string[] {
  const term = raw.replace(/\s+/g, ' ').trim();
  if (!term) return list;
  if (list.some((t) => t.toLowerCase() === term.toLowerCase())) return list;
  return [...list, term];
}

export function removeAt<T>(list: T[], index: number): T[] {
  return list.filter((_, i) => i !== index);
}

/** What gets saved from the editable rows: rows with nothing spoken are drafts and stay local. */
export function cleanReplacements(rows: Replacement[]): Replacement[] {
  return rows
    .map((r) => ({ spoken: r.spoken.replace(/\s+/g, ' ').trim(), written: r.written }))
    .filter((r) => r.spoken !== '');
}

// ---------------------------------------------------------------------------------------------
// Live store: what the pill shows
// ---------------------------------------------------------------------------------------------

export interface DictationNotice {
  id: number;
  /** info: brief, quiet. error: failed. blocked: recognised but could not be typed - `copy` holds the text. */
  kind: 'info' | 'error' | 'blocked';
  text: string;
  /** Only for `blocked`: copied to the clipboard solely when the user clicks Copy. */
  copy?: string;
  /** Present (true) only when the session was a spoken Agent command, not dictation. */
  agent?: boolean;
}

export interface DictationLive {
  phase: DictationPhase;
  /** Raw RMS 0..1 while listening; use `meterLevel` to draw it. */
  level: number;
  notice: DictationNotice | null;
  /** Present (true) only while the session is a spoken Agent command, not dictation. */
  agent?: boolean;
}

export const dictationLive = writable<DictationLive>({ phase: 'idle', level: 0, notice: null });

const clamp01 = (n: number) => (Number.isFinite(n) ? Math.min(1, Math.max(0, n)) : 0);

/** `{ agent: true }` for a spoken Agent command, nothing for dictation (so dictation's shapes stay as they were). */
const agentFlag = (mode: SessionKind | undefined): { agent?: true } => (mode === 'agent' ? { agent: true } : {});

export function reduceLive(prev: DictationLive, p: StatePayload): DictationLive {
  switch (p.state) {
    case 'listening':
      return { phase: 'listening', level: clamp01(p.level ?? 0), notice: null, ...agentFlag(p.mode) };
    case 'loading-model':
    case 'transcribing':
      return { phase: p.state, level: 0, notice: null, ...agentFlag(p.mode) };
    default:
      return { phase: 'idle', level: 0, notice: prev.notice };
  }
}

/** Map a raw RMS reading to a 0..1 meter fill on a decibel scale (silence at -54 dB, full at -6 dB). */
export function meterLevel(rms: number): number {
  if (!(rms > 0)) return 0;
  const db = 20 * Math.log10(Math.min(1, rms));
  return clamp01((db + 54) / 48);
}

/** The notice (if any) a finished dictation deserves. A clean success says nothing. */
export function noticeFor(r: DictationResult): Omit<DictationNotice, 'id'> | null {
  const agent = agentFlag(r.mode);
  if (r.ok) return r.nothingHeard ? { kind: 'info', text: NOTHING_HEARD, ...agent } : null;
  const why = (r.error ?? '').trim();
  if (r.text) return { kind: 'blocked', text: why || 'Krovi couldn’t type into that app.', copy: r.text, ...agent };
  return { kind: 'error', text: why || (r.mode === 'agent' ? 'The voice command didn’t work.' : 'Dictation didn’t work.'), ...agent };
}

export const NOTICE_MS: Record<DictationNotice['kind'], number> = { info: 2400, error: 5200, blocked: 15000 };

/** What the pill draws, or null when dictation has nothing to show. */
export type DictationFace =
  | { kind: 'listening'; label: string; level: number }
  | { kind: 'loading-model' | 'transcribing'; label: string }
  | { kind: 'notice'; tone: DictationNotice['kind']; label: string; detail: string; copy?: string };

export function pillFace(live: DictationLive): DictationFace | null {
  switch (live.phase) {
    case 'listening':
      return { kind: 'listening', label: live.agent ? 'Listening for a command' : 'Listening', level: meterLevel(live.level) };
    case 'loading-model':
      return { kind: 'loading-model', label: 'Loading speech model' };
    case 'transcribing':
      return { kind: 'transcribing', label: 'Transcribing' };
  }
  const n = live.notice;
  if (!n) return null;
  if (n.kind === 'info') return { kind: 'notice', tone: 'info', label: n.text, detail: '' };
  return {
    kind: 'notice',
    tone: n.kind,
    label: n.kind === 'blocked' ? 'Couldn’t type it' : n.agent ? 'Voice command failed' : 'Dictation failed',
    detail: n.text,
    copy: n.copy,
  };
}

let noticeSeq = 0;
let noticeTimer: ReturnType<typeof setTimeout> | null = null;

function clearNoticeTimer(): void {
  if (noticeTimer) clearTimeout(noticeTimer);
  noticeTimer = null;
}

/** Drop the notice now (and with it any text held for Copy). */
export function dismissNotice(): void {
  clearNoticeTimer();
  dictationLive.update((s) => (s.notice ? { ...s, notice: null } : s));
}

function showNotice(n: Omit<DictationNotice, 'id'>): void {
  clearNoticeTimer();
  const id = ++noticeSeq;
  dictationLive.update((s) => ({ ...s, notice: { ...n, id } }));
  noticeTimer = setTimeout(() => {
    dictationLive.update((s) => (s.notice && s.notice.id === id ? { ...s, notice: null } : s));
  }, NOTICE_MS[n.kind]);
}

type Toaster = (text: string, ms?: number, tone?: 'error') => void;
let toaster: Toaster | null = null;

/** Where error text goes when the panel is open. The pill shows the same notice either way. */
export function setDictationToaster(fn: Toaster | null): void {
  toaster = fn;
}

function onState(p: StatePayload): void {
  // A new session replaces whatever the last one left behind (reduceLive drops the notice).
  if (p.state !== 'idle') clearNoticeTimer();
  dictationLive.update((s) => reduceLive(s, p));
}

function onResult(r: DictationResult): void {
  const n = noticeFor(r);
  if (!n) return;
  showNotice(n);
  if (n.kind !== 'info') toaster?.(n.text, n.kind === 'blocked' ? 4200 : 3600, 'error');
}

function onProgress(p: ModelProgress): void {
  dictationInfo.update((i) => reduceProgress(i, p));
}

async function afterModelDone(d: ModelDone): Promise<void> {
  try {
    await loadDictation();
  } catch {
    return;
  }
  if (!d.ok) return;
  // First model the user ever downloads becomes the one in use, so enabling just works.
  const cur = get(dictationInfo);
  const chosen = cur.models.find((m) => m.id === cur.settings.model);
  const fresh = cur.models.find((m) => m.id === d.id);
  if (chosen?.status.state !== 'installed' && fresh?.status.state === 'installed') {
    try {
      await saveDictationSettings({ ...cur.settings, model: d.id });
    } catch {
      /* Settings will show the model as not selected; nothing else to do */
    }
  }
}

function onDone(d: ModelDone): void {
  dictationInfo.update((i) => reduceDone(i, d));
  void afterModelDone(d);
}

let wired: Promise<void> | null = null;

/** Subscribe to the four backend events once. Harmless in the demo (no events ever arrive). */
export function wireDictation(): Promise<void> {
  if (!wired) {
    wired = Promise.all([
      listen<StatePayload>('dictation-state', onState),
      listen<DictationResult>('dictation-result', onResult),
      listen<ModelProgress>('voice-model-progress', onProgress),
      listen<ModelDone>('voice-model-done', onDone),
    ])
      .then(() => undefined)
      .catch(() => {
        wired = null;
      });
  }
  return wired;
}

// ---------------------------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------------------------

/** Read settings + models + download state. Demo: the honest unavailable state. */
export async function loadDictation(): Promise<DictationInfo> {
  if (!isTauri()) {
    dictationInfo.update((i) => ({ ...i, available: false, loaded: true, models: DEMO_MODELS }));
    return get(dictationInfo);
  }
  void wireDictation();
  let raw: unknown;
  try {
    raw = await invoke<unknown>('dictation_status');
  } catch (e) {
    throw new Error(errorText(e));
  }
  dictationInfo.update((i) => applyStatus(i, raw));
  return get(dictationInfo);
}

/** Save settings. Rejects with the backend's readable text (bad shortcut, taken shortcut, limits). */
export async function saveDictationSettings(next: DictationSettings): Promise<DictationSettings> {
  if (!isTauri()) throw new Error(DESKTOP_ONLY);
  let raw: unknown;
  try {
    raw = await invoke<unknown>('dictation_set_settings', { settings: next });
  } catch (e) {
    throw new Error(errorText(e));
  }
  const saved = normalizeDictationSettings(raw);
  dictationInfo.update((i) => ({ ...i, settings: saved }));
  return saved;
}

/** Start or stop from the UI, same as pressing the shortcut in toggle mode. */
export async function toggleDictation(): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke<void>('dictation_toggle');
  } catch (e) {
    onResult({ ok: false, error: errorText(e) });
  }
}

/** Throw away the current recording / transcription. Nothing is typed. */
export async function cancelDictation(): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke<void>('dictation_cancel');
  } catch {
    /* the backend's own idle event (or the reset below) is what matters */
  }
  dictationLive.update((s) => reduceLive(s, { state: 'idle' }));
}

export async function listInputDevices(): Promise<InputDevice[]> {
  if (!isTauri()) return [];
  try {
    const raw = await invoke<unknown>('voice_input_devices');
    return (Array.isArray(raw) ? raw : [])
      .map(rec)
      .filter((d) => typeof d.id === 'string' && d.id)
      .map((d) => ({ id: d.id as string, name: str(d.name, 'Microphone'), isDefault: bool(d.isDefault, false) }));
  } catch (e) {
    throw new Error(errorText(e));
  }
}

export async function downloadModel(id: string): Promise<void> {
  if (!isTauri()) throw new Error(DESKTOP_ONLY);
  const size = get(dictationInfo).models.find((m) => m.id === id)?.sizeBytes ?? 0;
  dictationInfo.update((i) => ({ ...i, downloading: id, progress: { id, downloaded: 0, total: size }, modelError: null }));
  try {
    await invoke<void>('voice_model_download', { id });
  } catch (e) {
    dictationInfo.update((i) => ({ ...i, downloading: null, progress: null, modelError: { id, message: errorText(e) } }));
    void loadDictation().catch(() => {}); // another download may really be running: resync
  }
}

export async function cancelDownload(): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke<void>('voice_model_cancel_download');
  } catch {
    /* the done event reports how it ended */
  }
}

export async function deleteModel(id: string): Promise<void> {
  if (!isTauri()) throw new Error(DESKTOP_ONLY);
  let raw: unknown;
  try {
    raw = await invoke<unknown>('voice_model_delete', { id });
  } catch (e) {
    throw new Error(errorText(e));
  }
  dictationInfo.update((i) => ({
    ...i,
    models: normalizeModels(raw),
    modelError: i.modelError?.id === id ? null : i.modelError,
  }));
}

// ---------------------------------------------------------------------------------------------
// Microphone test
// ---------------------------------------------------------------------------------------------

export const MIC_POLL_MS = 100;

export interface MicTest {
  /** Close the microphone. Safe to call more than once. */
  stop(): Promise<void>;
}

/**
 * Open the chosen microphone and poll its level ~10 times a second until stopped (the backend also
 * ends the test by itself after 20 s). The audio is discarded in Rust; only a number comes back.
 * `onEnd` runs when the backend ended the test, not when `stop()` did.
 */
export async function startMicTest(device: string | null, onLevel: (rms: number) => void, onEnd?: () => void): Promise<MicTest> {
  if (!isTauri()) throw new Error(DESKTOP_ONLY);
  try {
    await invoke<void>('mic_test_start', { device });
  } catch (e) {
    throw new Error(errorText(e));
  }
  let alive = true;
  let stopped = false;
  let polling = false;
  const timer = setInterval(async () => {
    if (!alive || polling) return;
    polling = true;
    try {
      const level = await invoke<number | null>('mic_test_level');
      if (!alive) return;
      if (typeof level === 'number') {
        onLevel(level);
      } else {
        alive = false;
        clearInterval(timer);
        onEnd?.();
      }
    } catch {
      if (alive) {
        alive = false;
        clearInterval(timer);
        void invoke<void>('mic_test_stop').catch(() => {});
        onEnd?.();
      }
    } finally {
      polling = false;
    }
  }, MIC_POLL_MS);
  return {
    async stop() {
      if (stopped) return;
      stopped = true;
      alive = false;
      clearInterval(timer);
      try {
        await invoke<void>('mic_test_stop');
      } catch {
        /* nothing left to close */
      }
    },
  };
}
