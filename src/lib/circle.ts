// Circle to Ask + the temporary screen-awareness session (owner: circle) - docs/DECISIONS.md D-021.
// Contract: src-tauri/src/circle.rs.
//
// PRIVACY SHAPE (the UI must keep to it):
//   * Nothing here captures anything by itself. The only capture happens in Rust, once, when the
//     user releases a selection (`captureRegion`) or asks a question during a session they started
//     (`awareAsk`).
//   * The picture never reaches the webview. `captureRegion` returns the recognised TEXT and its
//     line boxes; the image stays in Rust memory for two minutes for an optional "Ask AI".
//   * Nothing is sent anywhere unless a key is saved for a provider and the user confirms the
//     disclosure that names that provider (`askVision` / `awareAsk` with `vision`). This file never
//     sees a key: keys are entered in Settings -> AI and live in Windows Credential Manager.
//
// TAURI: thin wrappers over the commands + two events (`circle-result`, `aware-state`).
// DEMO (plain browser, no Tauri): no screen is read and nothing is sent. `captureRegion` returns a
//   clearly labelled sample result, AI is unavailable, and an awareness session is simulated locally
//   so the flow can be shown. Every demo result says so.

import { get, writable, type Readable } from 'svelte/store';
import { invoke, isTauri, listen } from './bridge';

// ---------------------------------------------------------------------------------------
// Types (wire shapes: src-tauri/src/circle.rs, all camelCase)
// ---------------------------------------------------------------------------------------

// The ids match `krovi_net::ai::Provider`. Settings -> AI (src/lib/ai.ts) owns which provider and
// model are chosen and where keys are entered; Circle only reads that choice.
export type ProviderId = 'anthropic' | 'openai' | 'gemini' | 'groq' | 'snowflake';

export interface CircleLine {
  text: string;
  /** box in the captured image's own pixels */
  x: number;
  y: number;
  w: number;
  h: number;
}

/** What was read from the screen (the `circle-result` event, and the command results). */
export interface ReadResult {
  ok: boolean;
  /** `circle`: a circled area; `aware`: a question during a session */
  source: 'circle' | 'aware';
  text: string;
  lines: CircleLine[];
  width: number;
  height: number;
  language: string | null;
  /** why no text was read although the picture was captured (e.g. no OCR language installed) */
  ocrError: string | null;
  /** the area is one flat colour (protected video, or nothing there) */
  blank: boolean;
  error: string | null;
  /** seconds the picture stays available to "Ask AI" (0 = not held) */
  expiresInSecs: number;
  /** browser demo only: this is a sample, not the screen */
  demo?: boolean;
}

export interface CircleSettings {
  enabled: boolean;
  shortcut: string;
  /** default length of an awareness session, 1..=30 */
  sessionMinutes: number;
}

export interface ProviderInfo {
  id: ProviderId;
  name: string;
  /** the provider's default model for pictures */
  defaultModel: string;
  /** the model that will read the picture (Settings -> AI's choice if it can see, else the provider's image model) */
  model: string;
  /** READY to use: a key is saved and nothing else is missing (Gemini's notice, Snowflake's account URL) */
  hasKey: boolean;
  keyHint: string | null;
}

/** The sentences to show, word for word, before anything is sent. They name the provider. */
export interface Disclosure {
  provider: ProviderId;
  providerName: string;
  model: string;
  image: string;
  screen: string;
}

export interface CircleStatus {
  settings: CircleSettings;
  providers: ProviderInfo[];
  /** the provider "Ask AI" would use now; null when no provider has a saved key */
  activeProvider: ProviderId | null;
  /** the disclosure for `activeProvider` */
  disclosure: Disclosure | null;
  /** one per provider that has a saved key (for a provider picker) */
  disclosures: Disclosure[];
  maxSessionMinutes: number;
  shortcutError: string | null;
}

export interface AiReply {
  answer: string;
  truncated: boolean;
  provider: ProviderId;
  providerName: string;
  model: string;
}

export type AiFailureKind =
  | 'auth'
  | 'billing'
  | 'quota'
  | 'rate-limit'
  | 'eligibility'
  | 'model'
  | 'blocked'
  | 'unavailable'
  | 'network'
  | 'other'
  // ours
  | 'no-provider'
  | 'consent-required'
  | 'expired'
  | 'inactive'
  | 'not-allowed';

export interface AiFailure {
  kind: AiFailureKind;
  /** one plain sentence for the user */
  message: string;
  /** the provider's own message, trimmed */
  detail: string | null;
  status: number | null;
}

export class AiError extends Error {
  readonly failure: AiFailure;
  readonly kind: AiFailureKind;
  constructor(failure: AiFailure) {
    super(failure.message);
    this.name = 'AiError';
    this.failure = failure;
    this.kind = failure.kind;
  }
}

/** Payload of the `aware-state` event and the `aware_*` status results. */
export interface AwareStatus {
  active: boolean;
  /** Unix epoch ms (display only) */
  startedAt: number | null;
  endsAt: number | null;
  remainingSecs: number;
  maxMinutes: number;
  /** the user allowed pictures of the screen to go to `providerName` during this session */
  vision: boolean;
  providerName: string | null;
  /** only on events: started | stopped | expired */
  reason: 'started' | 'stopped' | 'expired' | null;
}

export interface AwareReply {
  read: ReadResult;
  answer: AiReply | null;
  aiError: AiFailure | null;
}

export interface OcrStatus {
  available: boolean;
  language: string | null;
  message: string | null;
}

/** Where the overlay window sits, physical pixels (the answer to `circle_overlay_ready`). */
export interface ArmInfo {
  token: number;
  x: number;
  y: number;
  width: number;
  height: number;
}

export const NO_PROVIDER_TEXT = 'Set up an AI provider in Settings → AI';
export const MAX_SESSION_MINUTES = 30;
export const MIN_SESSION_MINUTES = 1;
export const DEFAULT_SESSION_MINUTES = 10;
/** How long the circled picture is held for "Ask AI" (matches the Rust slot). */
export const HELD_SECS = 120;
/** Longest Google search text (matches `krovi_sys::web::MAX_QUERY_CHARS`). */
export const MAX_QUERY_CHARS = 300;
export const DESKTOP_ONLY = 'Available in the desktop app.';
export const GOOGLE_SEARCH_BASE = 'https://www.google.com/search?q=';

export const PROVIDER_IDS: ProviderId[] = ['anthropic', 'openai', 'gemini', 'groq', 'snowflake'];

export const DEFAULT_SETTINGS: CircleSettings = {
  enabled: true,
  shortcut: 'Ctrl+Alt+C',
  sessionMinutes: DEFAULT_SESSION_MINUTES,
};

export const IDLE_AWARE: AwareStatus = {
  active: false,
  startedAt: null,
  endsAt: null,
  remainingSecs: 0,
  maxMinutes: MAX_SESSION_MINUTES,
  vision: false,
  providerName: null,
  reason: null,
};

// ---------------------------------------------------------------------------------------
// Pure helpers (unit-tested)
// ---------------------------------------------------------------------------------------

export interface Pt {
  x: number;
  y: number;
}
export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}
export interface PxRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Smallest selection (CSS px, either side) that counts as a selection and not a click. */
export const MIN_SELECTION_CSS = 8;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const finite = (n: number) => (Number.isFinite(n) ? n : 0);

/** The rectangle between two corners, whichever way it was dragged, kept inside `bounds`. */
export function dragBox(a: Pt, b: Pt, bounds?: { w: number; h: number }): Box {
  let x0 = Math.min(finite(a.x), finite(b.x));
  let y0 = Math.min(finite(a.y), finite(b.y));
  let x1 = Math.max(finite(a.x), finite(b.x));
  let y1 = Math.max(finite(a.y), finite(b.y));
  if (bounds) {
    x0 = clamp(x0, 0, bounds.w);
    x1 = clamp(x1, 0, bounds.w);
    y0 = clamp(y0, 0, bounds.h);
    y1 = clamp(y1, 0, bounds.h);
  }
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

/** Bounding box of a freehand path (a circle drawn with the mouse). `null` for an empty path. */
export function pointsBox(points: Pt[], bounds?: { w: number; h: number }): Box | null {
  if (!points.length) return null;
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  for (const p of points) {
    x0 = Math.min(x0, finite(p.x));
    y0 = Math.min(y0, finite(p.y));
    x1 = Math.max(x1, finite(p.x));
    y1 = Math.max(y1, finite(p.y));
  }
  return dragBox({ x: x0, y: y0 }, { x: x1, y: y1 }, bounds);
}

export function bigEnough(b: Box | null, min = MIN_SELECTION_CSS): b is Box {
  return !!b && b.w >= min && b.h >= min;
}

/**
 * CSS-pixel box in the overlay -> physical pixels relative to the overlay's top-left (= the monitor's).
 * The scale is `monitor.width / viewport.w` when the monitor size is known (exact even at 125% or 150%
 * where CSS pixels do not divide evenly), otherwise the device pixel ratio. The result is rounded
 * outward (floor the near edge, ceil the far edge) and kept inside the monitor.
 */
export function toPhysicalRect(css: Box, viewport: { w: number; h: number }, monitor: { width: number; height: number } | null, dpr: number): PxRect {
  const ratio = Number.isFinite(dpr) && dpr > 0 ? dpr : 1;
  const sx = monitor && viewport.w > 0 ? monitor.width / viewport.w : ratio;
  const sy = monitor && viewport.h > 0 ? monitor.height / viewport.h : ratio;
  const maxW = monitor ? monitor.width : Infinity;
  const maxH = monitor ? monitor.height : Infinity;
  const x0 = clamp(Math.floor(css.x * sx), 0, maxW);
  const y0 = clamp(Math.floor(css.y * sy), 0, maxH);
  const x1 = clamp(Math.ceil((css.x + css.w) * sx), 0, maxW);
  const y1 = clamp(Math.ceil((css.y + css.h) * sy), 0, maxH);
  return { x: x0, y: y0, width: Math.max(0, x1 - x0), height: Math.max(0, y1 - y0) };
}

/** Same length rule as the backend: 1..=30, whole minutes, missing = `fallback`. */
export function clampMinutes(requested: number | null | undefined, fallback = DEFAULT_SESSION_MINUTES): number {
  const v = requested == null || !Number.isFinite(requested) ? fallback : Math.floor(requested);
  return clamp(v, MIN_SESSION_MINUTES, MAX_SESSION_MINUTES);
}

/** `9:05` for 545 seconds, `0:12` for 12, `1:02:03` past an hour. Never negative. */
export function formatRemaining(secs: number): string {
  const s = Math.max(0, Math.floor(Number.isFinite(secs) ? secs : 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = s % 60;
  const two = (n: number) => String(n).padStart(2, '0');
  return h > 0 ? `${h}:${two(m)}:${two(r)}` : `${m}:${two(r)}`;
}

/** Seconds left until `endsAt` (epoch ms), as of `now` (epoch ms). */
export function secsLeft(endsAt: number | null, now: number): number {
  if (endsAt == null) return 0;
  return Math.max(0, Math.ceil((endsAt - now) / 1000));
}

/** The text to search for: control characters dropped, whitespace collapsed, cut to 300 characters. */
export function searchQuery(text: string): string | null {
  let out = '';
  let pendingSpace = false;
  let count = 0;
  for (const c of text) {
    if (/\s/u.test(c)) {
      pendingSpace = out.length > 0;
      continue;
    }
    if (/\p{Cc}/u.test(c)) continue;
    if (count >= MAX_QUERY_CHARS) break;
    if (pendingSpace) {
      out += ' ';
      count++;
      pendingSpace = false;
      if (count >= MAX_QUERY_CHARS) break;
    }
    out += c;
    count++;
  }
  out = out.trimEnd();
  return out ? out : null;
}

/** RFC 3986 percent-encoding of the UTF-8 bytes; only `A-Z a-z 0-9 - . _ ~` stay (same as the backend). */
export function percentEncode(s: string): string {
  let out = '';
  for (const b of new TextEncoder().encode(s)) {
    const ch = String.fromCharCode(b);
    out += /[A-Za-z0-9\-._~]/.test(ch) ? ch : `%${b.toString(16).toUpperCase().padStart(2, '0')}`;
  }
  return out;
}

export function googleSearchUrl(text: string): string | null {
  const q = searchQuery(text);
  return q ? `${GOOGLE_SEARCH_BASE}${percentEncode(q)}` : null;
}

/** Plain text of whatever a rejected `invoke` carried (Rust sends a string, or an object for AI errors). */
export function errorText(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  if (e && typeof e === 'object' && typeof (e as { message?: unknown }).message === 'string') return (e as { message: string }).message;
  return String(e);
}

const FAILURE_KINDS: AiFailureKind[] = ['auth', 'billing', 'quota', 'rate-limit', 'eligibility', 'model', 'blocked', 'unavailable', 'network', 'other', 'no-provider', 'consent-required', 'expired', 'inactive', 'not-allowed'];

/** Whatever was thrown -> an `AiFailure` (a plain string becomes kind `other`). */
export function toFailure(e: unknown): AiFailure {
  if (e instanceof AiError) return e.failure;
  if (e && typeof e === 'object') {
    const o = e as Record<string, unknown>;
    if (typeof o.message === 'string') {
      const kind = FAILURE_KINDS.includes(o.kind as AiFailureKind) ? (o.kind as AiFailureKind) : 'other';
      return {
        kind,
        message: o.message,
        detail: typeof o.detail === 'string' ? o.detail : null,
        status: typeof o.status === 'number' ? o.status : null,
      };
    }
  }
  return { kind: 'other', message: errorText(e), detail: null, status: null };
}

/** What to do about a failure, shown under its message. Empty when the message says it all. */
export function failureAdvice(f: AiFailure): string {
  switch (f.kind) {
    case 'auth':
    case 'no-provider':
      return 'Open Settings → AI to check your key.';
    case 'billing':
    case 'quota':
    case 'eligibility':
    case 'model':
      return 'This is about your provider account, not Krovi. You can change the model in Settings → AI.';
    case 'expired':
      return 'Press the shortcut and circle the area again.';
    default:
      return '';
  }
}

/** Can "Ask AI" be pressed, and if not, the one line that says why (shown in place of the button). */
export function askAvailability(status: CircleStatus | null, held: { receivedAt: number; result: ReadResult } | null, now: number): { enabled: boolean; reason: string | null } {
  if (!status) return { enabled: false, reason: 'Checking your AI settings…' };
  if (!status.activeProvider) return { enabled: false, reason: NO_PROVIDER_TEXT };
  if (!held || held.result.demo || held.result.expiresInSecs <= 0) return { enabled: false, reason: 'There is no selected image to send.' };
  if (now - held.receivedAt >= held.result.expiresInSecs * 1000) return { enabled: false, reason: 'The selected area is no longer held (two minutes). Circle it again.' };
  return { enabled: true, reason: null };
}

/** Whole seconds the picture is still held for, as of `now`. */
export function heldSecsLeft(held: { receivedAt: number; result: ReadResult } | null, now: number): number {
  if (!held) return 0;
  return Math.max(0, Math.ceil((held.result.expiresInSecs * 1000 - (now - held.receivedAt)) / 1000));
}

/**
 * The OCR lines that share a word (3+ letters) with the question, best first. Lets the
 * "no AI" screen answer show where on the screen the words of the question appear.
 */
export function matchingLines(question: string, lines: CircleLine[], limit = 5): CircleLine[] {
  const words = new Set(
    (question.toLowerCase().match(/[\p{L}\p{N}]{3,}/gu) ?? []).filter((w) => !STOP.has(w)),
  );
  if (!words.size) return [];
  return lines
    .map((l) => ({ l, n: (l.text.toLowerCase().match(/[\p{L}\p{N}]{3,}/gu) ?? []).filter((w) => words.has(w)).length }))
    .filter((x) => x.n > 0)
    .sort((a, b) => b.n - a.n)
    .slice(0, limit)
    .map((x) => x.l);
}

const STOP = new Set(['the', 'and', 'what', 'who', 'how', 'why', 'when', 'where', 'which', 'this', 'that', 'with', 'for', 'are', 'was', 'does', 'did', 'can', 'you', 'your', 'about', 'from', 'there', 'tell', 'show', 'explain']);

// ---------------------------------------------------------------------------------------
// Stores
// ---------------------------------------------------------------------------------------

/** The latest read area and when the webview received it (the picture is held for `expiresInSecs`). */
export interface Held {
  result: ReadResult;
  receivedAt: number;
}

const heldStore = writable<Held | null>(null);
/** The most recent `circle-result` (also set by `captureRegion`). The panel's AskPanel reads it. */
export const circleHeld: Readable<Held | null> = { subscribe: heldStore.subscribe };

const awareStore = writable<AwareStatus>({ ...IDLE_AWARE });
/** Screen-awareness state: `active` drives the pill's always-visible indicator. */
export const awareState: Readable<AwareStatus> = { subscribe: awareStore.subscribe };

const statusStore = writable<CircleStatus | null>(null);
/** Settings + providers + disclosure, as of the last `loadCircle` / `saveCircleSettings`. */
export const circleStatus: Readable<CircleStatus | null> = { subscribe: statusStore.subscribe };

function setHeld(result: ReadResult): void {
  heldStore.set({ result, receivedAt: Date.now() });
}

/** Forget the shown result (the panel closed). Also tells Rust to drop the held picture. */
export async function discardHeld(): Promise<void> {
  heldStore.set(null);
  if (isTauri()) {
    try {
      await invoke<void>('circle_clear');
    } catch {
      /* nothing to drop */
    }
  }
}

// ---------------------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------------------

function normalizeRead(raw: unknown): ReadResult {
  const r = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
  const num = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) ? v : 0);
  const lines = Array.isArray(r.lines)
    ? r.lines
        .filter((l): l is Record<string, unknown> => !!l && typeof l === 'object')
        .map((l) => ({ text: String(l.text ?? ''), x: num(l.x), y: num(l.y), w: num(l.w), h: num(l.h) }))
    : [];
  return {
    ok: r.ok === true,
    source: r.source === 'aware' ? 'aware' : 'circle',
    text: typeof r.text === 'string' ? r.text : '',
    lines,
    width: num(r.width),
    height: num(r.height),
    language: typeof r.language === 'string' ? r.language : null,
    ocrError: typeof r.ocrError === 'string' ? r.ocrError : null,
    blank: r.blank === true,
    error: typeof r.error === 'string' ? r.error : null,
    expiresInSecs: num(r.expiresInSecs),
  };
}

export function normalizeAware(raw: unknown): AwareStatus {
  const r = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
  const num = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) ? v : null);
  const reason = r.reason === 'started' || r.reason === 'stopped' || r.reason === 'expired' ? r.reason : null;
  return {
    active: r.active === true,
    startedAt: num(r.startedAt),
    endsAt: num(r.endsAt),
    remainingSecs: num(r.remainingSecs) ?? 0,
    maxMinutes: num(r.maxMinutes) ?? MAX_SESSION_MINUTES,
    vision: r.vision === true,
    providerName: typeof r.providerName === 'string' ? r.providerName : null,
    reason,
  };
}

let wired: Promise<void> | null = null;

/**
 * Subscribe to `circle-result` and `aware-state` once. Call it from the main window at start-up
 * (App). `onResult` runs for each circled area, so the app can open the panel on the Ask view.
 * Harmless in the demo (no events ever arrive).
 */
export function wireCircle(onResult?: (r: ReadResult) => void): Promise<void> {
  if (!wired) {
    wired = Promise.all([
      listen<unknown>('circle-result', (raw) => {
        const r = normalizeRead(raw);
        setHeld(r);
        onResult?.(r);
      }),
      listen<unknown>('aware-state', (raw) => awareStore.set(normalizeAware(raw))),
    ])
      .then(() => undefined)
      .catch(() => {
        wired = null;
      });
  }
  return wired;
}

// ---------------------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------------------

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new Error(errorText(e));
  }
}

async function callAi<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new AiError(toFailure(e));
  }
}

function demoStatus(settings: CircleSettings): CircleStatus {
  const none = (id: ProviderId, name: string, model: string): ProviderInfo => ({ id, name, defaultModel: model, model, hasKey: false, keyHint: null });
  return {
    settings,
    providers: [
      none('anthropic', 'Anthropic', 'claude-haiku-5-5'),
      none('openai', 'OpenAI', 'gpt-6-luna'),
      none('gemini', 'Google Gemini', 'gemini-3.5-flash-lite'),
      none('groq', 'Groq', 'qwen/qwen3.8-27b'),
      none('snowflake', 'Snowflake Cortex', 'claude-sonnet-4-5'),
    ],
    activeProvider: null,
    disclosure: null,
    disclosures: [],
    maxSessionMinutes: MAX_SESSION_MINUTES,
    shortcutError: null,
  };
}

let demoSettings: CircleSettings = { ...DEFAULT_SETTINGS };

function normalizeStatus(raw: unknown): CircleStatus {
  const r = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
  const s = (r.settings && typeof r.settings === 'object' ? r.settings : {}) as Record<string, unknown>;
  const providers = Array.isArray(r.providers) ? (r.providers as ProviderInfo[]) : [];
  const active = PROVIDER_IDS.includes(r.activeProvider as ProviderId) ? (r.activeProvider as ProviderId) : null;
  return {
    settings: {
      enabled: s.enabled !== false,
      shortcut: typeof s.shortcut === 'string' ? s.shortcut : DEFAULT_SETTINGS.shortcut,
      sessionMinutes: clampMinutes(typeof s.sessionMinutes === 'number' ? s.sessionMinutes : null),
    },
    providers,
    activeProvider: active,
    disclosure: r.disclosure && typeof r.disclosure === 'object' ? (r.disclosure as Disclosure) : null,
    disclosures: Array.isArray(r.disclosures) ? (r.disclosures as Disclosure[]) : [],
    maxSessionMinutes: typeof r.maxSessionMinutes === 'number' ? r.maxSessionMinutes : MAX_SESSION_MINUTES,
    shortcutError: typeof r.shortcutError === 'string' ? r.shortcutError : null,
  };
}

/** Settings, providers (key saved or not, never the key) and the disclosure text. */
export async function loadCircle(): Promise<CircleStatus> {
  const st = isTauri() ? normalizeStatus(await call<unknown>('circle_get_settings')) : demoStatus(demoSettings);
  statusStore.set(st);
  return st;
}

/** Save settings. Rejects with the backend's readable text (bad or taken shortcut). */
export async function saveCircleSettings(settings: CircleSettings): Promise<CircleStatus> {
  if (!isTauri()) {
    demoSettings = { ...settings, sessionMinutes: clampMinutes(settings.sessionMinutes) };
    const st = demoStatus(demoSettings);
    statusStore.set(st);
    return st;
  }
  const st = normalizeStatus(await call<unknown>('circle_set_settings', { settings }));
  statusStore.set(st);
  return st;
}

/**
 * Save (string) or remove (null) the key for a provider in Windows Credential Manager. Thin: the
 * Settings -> AI page owns key entry; this exists so any feature can reuse the same command.
 */
export async function setProviderKey(provider: ProviderId, key: string | null): Promise<CircleStatus> {
  if (!isTauri()) throw new Error(DESKTOP_ONLY);
  const st = normalizeStatus(await call<unknown>('circle_set_key', { provider, key }));
  statusStore.set(st);
  return st;
}

/** Is a Windows OCR language installed? */
export async function ocrStatus(): Promise<OcrStatus> {
  if (!isTauri()) return { available: false, language: null, message: DESKTOP_ONLY };
  return call<OcrStatus>('circle_ocr_status');
}

/** Show the selection overlay (same as the shortcut). */
export async function startCircle(): Promise<void> {
  if (!isTauri()) throw new Error('Circle to Ask reads your screen, so it only runs in the Krovi desktop app.');
  await call<void>('circle_start');
}

/** Close the overlay without capturing anything. */
export async function cancelCircle(): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke<void>('circle_cancel');
  } catch {
    /* already closed */
  }
}

/** The overlay page is ready: Rust shows the window and says where it sits. `null` = nothing armed. */
export async function overlayReady(): Promise<ArmInfo | null> {
  if (!isTauri()) return null;
  try {
    const raw = await invoke<ArmInfo | null>('circle_overlay_ready');
    return raw && typeof raw.width === 'number' ? raw : null;
  } catch {
    return null;
  }
}

export const DEMO_TEXT = 'Demo: in the desktop app, Krovi reads the text inside the area you circle, on this PC.\nNothing here was captured from your screen.';

/**
 * The user released a selection. `rect` is physical pixels relative to the overlay's top-left. Rust
 * hides the overlay, captures that rectangle once and reads its text. Resolves with the result (the
 * `circle-result` event carries the same to the main window). Rejects only when nothing was captured.
 */
export async function captureRegion(rect: PxRect): Promise<ReadResult> {
  if (!isTauri()) {
    const r: ReadResult = {
      ok: true,
      source: 'circle',
      text: DEMO_TEXT,
      lines: [{ text: DEMO_TEXT.split('\n')[0], x: 0, y: 0, w: rect.width, h: 20 }],
      width: Math.max(1, Math.round(rect.width)),
      height: Math.max(1, Math.round(rect.height)),
      language: null,
      ocrError: null,
      blank: false,
      error: null,
      expiresInSecs: 0,
      demo: true,
    };
    setHeld(r);
    return r;
  }
  const r = normalizeRead(await call<unknown>('circle_capture', { rect }));
  setHeld(r); // the event does this too; both are idempotent and cover a missed event
  return r;
}

/**
 * Send the held circled picture and a question to the provider the user set up. `confirm` must be
 * true and means the user just read `disclosure.image`. Rejects with an `AiError` (`.kind`).
 */
export async function askVision(question: string, opts: { confirm: boolean; provider?: ProviderId | null }): Promise<AiReply> {
  if (!isTauri()) throw new AiError({ kind: 'no-provider', message: `${NO_PROVIDER_TEXT} (the desktop app).`, detail: null, status: null });
  return callAi<AiReply>('circle_ask_vision', { args: { question, confirm: opts.confirm, provider: opts.provider ?? null } });
}

/** Drop the held picture now (the text read from it stays on screen). "Ask AI" is then unavailable. */
export async function dropPicture(): Promise<void> {
  heldStore.update((h) => (h ? { ...h, result: { ...h.result, expiresInSecs: 0 } } : h));
  if (isTauri()) {
    try {
      await invoke<void>('circle_clear');
    } catch {
      /* nothing to drop */
    }
  }
}

/** Open a Google search for `text` in the browser (the backend builds the address). */
export async function searchWeb(text: string): Promise<void> {
  if (!isTauri()) {
    const url = googleSearchUrl(text);
    if (!url) throw new Error('There is no text to search for.');
    window.open(url, '_blank', 'noopener,noreferrer');
    return;
  }
  await call<void>('circle_search', { text });
}

export async function openKeysPage(provider: ProviderId): Promise<void> {
  if (!isTauri()) throw new Error(DESKTOP_ONLY);
  await call<void>('circle_open_keys_page', { provider });
}

// ---------------------------------------------------------------------------------------
// Screen awareness
// ---------------------------------------------------------------------------------------

let demoTimer: ReturnType<typeof setTimeout> | null = null;

function demoEnd(reason: 'stopped' | 'expired'): void {
  if (demoTimer) clearTimeout(demoTimer);
  demoTimer = null;
  awareStore.set({ ...IDLE_AWARE, reason });
}

/**
 * Start a session (1..=30 minutes). Nothing is captured by starting. `vision` records that the user
 * read `disclosure.screen` and agreed that pictures of the screen may go to their AI provider while
 * the session lasts. Rejects with an `AiError`.
 */
export async function awareStart(minutes: number | null, vision: boolean): Promise<AwareStatus> {
  if (!isTauri()) {
    if (vision) throw new AiError({ kind: 'no-provider', message: `${NO_PROVIDER_TEXT} (the desktop app).`, detail: null, status: null });
    const mins = clampMinutes(minutes, demoSettings.sessionMinutes);
    const now = Date.now();
    if (demoTimer) clearTimeout(demoTimer);
    const st: AwareStatus = { active: true, startedAt: now, endsAt: now + mins * 60_000, remainingSecs: mins * 60, maxMinutes: MAX_SESSION_MINUTES, vision: false, providerName: null, reason: 'started' };
    awareStore.set(st);
    demoTimer = setTimeout(() => demoEnd('expired'), mins * 60_000);
    return st;
  }
  const st = normalizeAware(await callAi<unknown>('aware_start', { minutes, vision }));
  awareStore.set(st);
  return st;
}

/** One click: the session ends now. */
export async function awareStop(): Promise<void> {
  if (!isTauri()) {
    demoEnd('stopped');
    return;
  }
  try {
    await invoke<void>('aware_stop');
  } finally {
    // The event also arrives; this makes the indicator drop even if it was missed.
    awareStore.set({ ...IDLE_AWARE, reason: 'stopped' });
  }
}

export async function awareStatus(): Promise<AwareStatus> {
  if (!isTauri()) return get(awareStore);
  const st = normalizeAware(await call<unknown>('aware_status'));
  awareStore.set(st);
  return st;
}

/**
 * Ask a question about the screen during a session: captures the primary monitor once, right now,
 * reads its text and, with `vision` (only if the session was started with it), asks the AI provider
 * about the picture. Rejects with an `AiError` (`inactive` when no session is running).
 */
export async function awareAsk(question: string, vision: boolean): Promise<AwareReply> {
  if (!isTauri()) {
    const r: ReadResult = { ok: true, source: 'aware', text: DEMO_TEXT, lines: [], width: 1, height: 1, language: null, ocrError: null, blank: false, error: null, expiresInSecs: 0, demo: true };
    if (!get(awareStore).active) throw new AiError({ kind: 'inactive', message: 'Start a screen-awareness session first. Krovi doesn’t look at the screen otherwise.', detail: null, status: null });
    return { read: r, answer: null, aiError: null };
  }
  const raw = await callAi<{ read: unknown; answer: AiReply | null; aiError: unknown }>('aware_ask', { question, vision });
  return { read: normalizeRead(raw.read), answer: raw.answer ?? null, aiError: raw.aiError ? toFailure(raw.aiError) : null };
}

/** Test hooks. */
export const __test = {
  reset(): void {
    heldStore.set(null);
    awareStore.set({ ...IDLE_AWARE });
    statusStore.set(null);
    if (demoTimer) clearTimeout(demoTimer);
    demoTimer = null;
    demoSettings = { ...DEFAULT_SETTINGS };
    wired = null;
  },
};
