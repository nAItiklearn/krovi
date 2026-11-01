// Quick Transfer client (owner: transfer) — docs/CONTRACTS.md §6.
//
// LAN-only file transfer, desktop <-> phone. The Rust side (krovi-transfer) runs a tiny HTTP
// server only while a share is open; this module is the UI's view of it:
//   * Tauri: invoke/listen wrappers over src-tauri/src/transfer.rs (events: transfer-progress,
//     transfer-done, transfer-closed, plus transfer-reached / transfer-failed).
//   * Browser demo (`isTauri() === false`): a simulated phone, with a QR that is clearly a
//     placeholder (it encodes http://192.168.1.42:51234/s/DEMO-not-a-real-link and goes nowhere).
//
// All state transitions go through ONE pure reducer (`reduce`) so the same code path handles
// real events and the demo, and so it is unit-testable without Tauri.

import { derived, get, writable, type Readable } from 'svelte/store';
import { invoke, isTauri, listen } from './bridge';
import { hold } from './shell';

// ---------------------------------------------------------------------------------------
// Types (wire shapes: docs/CONTRACTS.md §6; fields after `port` are additive)
// ---------------------------------------------------------------------------------------

export type ShareMode = 'send' | 'receive';
export type Direction = 'send' | 'receive';
export type CloseReason = 'expired' | 'stopped' | 'quit' | 'locked';

export interface FileInfo {
  name: string;
  size: number;
}

export interface Share {
  id: string;
  url: string;
  qrSvg: string;
  files: FileInfo[];
  mode: ShareMode;
  /** epoch ms */
  expiresAt: number;
  downloads: number;
  lanIp: string;
  port: number;
  createdAt: number;
  uploads: number;
  /** a phone has made a valid request */
  reached: boolean;
  /** 4-digit PIN to show on the desktop, when required */
  pin: string | null;
  maxUploadBytes: number;
  receivedBytes: number;
  saveDir: string | null;
  /** client-side only: this share is the browser-demo simulation */
  demo?: boolean;
  /** client-side only: file names the phone has finished downloading (from transfer-done) */
  sent?: string[];
}

export interface ProgressInfo {
  id: string;
  file: string;
  bytes: number;
  total: number;
  direction: Direction;
}

export interface ReceivedFile {
  key: string;
  name: string;
  size: number;
  savedPath: string | null;
  at: number;
}

export interface TransferConfig {
  maxUploadBytes: number;
  ttlSecs: number;
  requirePin: boolean;
  receiveDir: string;
}

export interface FirewallReport {
  lanIp: string | null;
  adapter: string | null;
  ruleInstalled: boolean;
  /** "Private" | "Public" | "DomainAuthenticated" when Windows reports it */
  profile: string | null;
  exe: string;
}

export interface ShareOptions {
  pin?: boolean;
  ttlSecs?: number;
}

/** What the UI can offer to share: a real path (Tauri) or a browser File's name+size (demo). */
export type ShareInput = string | FileInfo;

export type TransferEvent =
  | { type: 'progress'; payload: ProgressInfo }
  | { type: 'done'; payload: { id: string; file: string; direction: Direction; size?: number; savedPath?: string | null } }
  | { type: 'failed'; payload: { id: string; file: string; direction: Direction; message: string } }
  | { type: 'reached'; payload: { id: string } }
  | { type: 'closed'; payload: { id: string; reason: CloseReason } };

export interface TransferState {
  shares: Share[];
  progress: Record<string, ProgressInfo>;
  received: ReceivedFile[];
  error: string | null;
}

// ---------------------------------------------------------------------------------------
// Pure helpers (unit-tested)
// ---------------------------------------------------------------------------------------

/** No request reached the share this long after creation => show the "can't connect" help. */
export const HINT_AFTER_MS = 20_000;
export const RECEIVED_CAP = 40;
export const DEFAULT_CONFIG: TransferConfig = {
  maxUploadBytes: 2 * 1024 * 1024 * 1024,
  ttlSecs: 10 * 60,
  requirePin: false,
  receiveDir: '',
};

export const progressKey = (id: string, file: string): string => `${id}|${file}`;

export function percent(bytes: number, total: number): number {
  if (!(total > 0)) return 0;
  return Math.max(0, Math.min(100, Math.round((bytes / total) * 100)));
}

/** `9:42`, `0:07`, `1:02:03`. Never negative. */
export function countdown(msLeft: number): string {
  const s = Math.max(0, Math.ceil(msLeft / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const p2 = (n: number) => String(n).padStart(2, '0');
  return h > 0 ? `${h}:${p2(m)}:${p2(sec)}` : `${m}:${p2(sec)}`;
}

export function msLeft(share: Pick<Share, 'expiresAt'>, nowMs: number): number {
  return Math.max(0, share.expiresAt - nowMs);
}

/** True once a share has waited HINT_AFTER_MS without any phone reaching it. */
export function needsHint(share: Pick<Share, 'reached' | 'createdAt' | 'expiresAt'>, nowMs: number): boolean {
  if (share.reached) return false;
  const created = share.createdAt > 0 ? share.createdAt : share.expiresAt - DEFAULT_CONFIG.ttlSecs * 1000;
  return nowMs - created >= HINT_AFTER_MS;
}

export function closeMessage(reason: CloseReason): string {
  switch (reason) {
    case 'expired':
      return 'Share expired and was closed';
    case 'locked':
      return 'Closed after too many wrong PINs';
    case 'quit':
      return 'Share closed';
    default:
      return 'Sharing stopped';
  }
}

/** Human lines for the "my phone can't connect" panel, from a firewall report. */
export function diagnose(r: FirewallReport | null): string[] {
  if (!r) return [];
  const lines: string[] = [];
  if (!r.lanIp) {
    lines.push('This PC has no Wi-Fi or LAN address right now. Connect to the same Wi-Fi as your phone.');
    return lines;
  }
  lines.push(`Sharing on ${r.lanIp}${r.adapter ? ` (${r.adapter})` : ''}. Your phone must be on this same network.`);
  if (r.profile === 'Public') {
    lines.push(
      'Windows lists this network as Public, and its firewall blocks other devices from reaching apps on Public networks. In Windows Settings > Network & internet > Wi-Fi, open this network and choose Private, then try again.',
    );
  } else if (r.profile === 'Private' || r.profile === 'DomainAuthenticated') {
    lines.push(`Windows lists this network as ${r.profile === 'Private' ? 'Private' : 'a domain network'}.`);
  }
  lines.push(
    r.ruleInstalled
      ? 'The Krovi firewall rule is installed (this app only, Private networks only).'
      : 'There is no Krovi firewall rule. If Windows showed an "Allow access" prompt and it was closed or denied, use the button below.',
  );
  return lines;
}

function normalizeShare(raw: Partial<Share> & { id: string }): Share {
  return {
    id: raw.id,
    url: raw.url ?? '',
    qrSvg: raw.qrSvg ?? '',
    files: raw.files ?? [],
    mode: raw.mode ?? 'send',
    expiresAt: raw.expiresAt ?? Date.now() + DEFAULT_CONFIG.ttlSecs * 1000,
    downloads: raw.downloads ?? 0,
    lanIp: raw.lanIp ?? '',
    port: raw.port ?? 0,
    createdAt: raw.createdAt ?? 0,
    uploads: raw.uploads ?? 0,
    reached: raw.reached ?? false,
    pin: raw.pin ?? null,
    maxUploadBytes: raw.maxUploadBytes ?? DEFAULT_CONFIG.maxUploadBytes,
    receivedBytes: raw.receivedBytes ?? 0,
    saveDir: raw.saveDir ?? null,
    demo: raw.demo,
  };
}

export function emptyState(): TransferState {
  return { shares: [], progress: {}, received: [], error: null };
}

/** Merge an authoritative share list from the backend, keeping client-only fields. */
export function mergeShares(prev: Share[], incoming: Share[]): Share[] {
  return incoming.map((s) => {
    const old = prev.find((p) => p.id === s.id);
    if (!old) return s;
    return { ...s, ...(old.demo ? { demo: true } : {}), ...(old.sent ? { sent: old.sent } : {}) };
  });
}

/**
 * The one state transition function. Returns the next state and an optional toast line.
 * Pure: no timers, no IO, `at` is passed in.
 */
export function reduce(state: TransferState, ev: TransferEvent, at = Date.now()): { state: TransferState; toast?: string } {
  switch (ev.type) {
    case 'progress': {
      const p = ev.payload;
      if (!state.shares.some((s) => s.id === p.id)) return { state };
      return { state: { ...state, progress: { ...state.progress, [progressKey(p.id, p.file)]: p } } };
    }
    case 'reached': {
      return {
        state: { ...state, shares: state.shares.map((s) => (s.id === ev.payload.id ? { ...s, reached: true } : s)) },
      };
    }
    case 'done': {
      const p = ev.payload;
      const key = progressKey(p.id, p.file);
      const { [key]: _gone, ...progress } = state.progress;
      const size = p.size ?? 0;
      let received = state.received;
      const shares = state.shares.map((s) => {
        if (s.id !== p.id) return s;
        if (p.direction === 'send') {
          const sent = s.sent?.includes(p.file) ? s.sent : [...(s.sent ?? []), p.file];
          return { ...s, reached: true, downloads: s.downloads + 1, sent };
        }
        return {
          ...s,
          reached: true,
          uploads: s.uploads + 1,
          receivedBytes: s.receivedBytes + size,
          files: [...s.files, { name: p.file, size }],
        };
      });
      if (p.direction === 'receive') {
        const entry: ReceivedFile = { key: `${p.id}|${p.file}|${at}`, name: p.file, size, savedPath: p.savedPath ?? null, at };
        received = [entry, ...state.received].slice(0, RECEIVED_CAP);
      }
      return {
        state: { ...state, shares, progress, received },
        toast: p.direction === 'send' ? `${p.file} sent to your phone` : `Received ${p.file}`,
      };
    }
    case 'failed': {
      const p = ev.payload;
      const { [progressKey(p.id, p.file)]: _gone, ...progress } = state.progress;
      return { state: { ...state, progress, error: p.message }, toast: p.message };
    }
    case 'closed': {
      const { id, reason } = ev.payload;
      const progress = Object.fromEntries(Object.entries(state.progress).filter(([, v]) => v.id !== id));
      return {
        state: { ...state, shares: state.shares.filter((s) => s.id !== id), progress },
        toast: state.shares.some((s) => s.id === id) ? closeMessage(reason) : undefined,
      };
    }
  }
}

// ---------------------------------------------------------------------------------------
// Stores
// ---------------------------------------------------------------------------------------

const RECEIVED_KEY = 'krovi.transfer.received';
const CONFIG_KEY = 'krovi.transfer.config';

function readJson<T>(key: string, fallback: T): T {
  try {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem(key) : null;
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}
function writeJson(key: string, value: unknown): void {
  try {
    if (typeof localStorage !== 'undefined') localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* blocked storage: the list simply won't survive a restart */
  }
}

const state = writable<TransferState>(emptyState());
export const shares: Readable<Share[]> = derived(state, (s) => s.shares);
export const progress: Readable<Record<string, ProgressInfo>> = derived(state, (s) => s.progress);
export const received: Readable<ReceivedFile[]> = derived(state, (s) => s.received);
export const transferError: Readable<string | null> = derived(state, (s) => s.error);
export const config = writable<TransferConfig>({ ...DEFAULT_CONFIG, ...readJson<Partial<TransferConfig>>(CONFIG_KEY, {}) });

let toaster: ((text: string) => void) | null = null;
/** The view registers the app toast so this module stays free of UI imports. */
export function setToaster(fn: ((text: string) => void) | null): void {
  toaster = fn;
}

function dispatch(ev: TransferEvent): void {
  const at = Date.now();
  let toast: string | undefined;
  state.update((s) => {
    const r = reduce(s, ev, at);
    toast = r.toast;
    return r.state;
  });
  if (ev.type === 'done' && ev.payload.direction === 'receive') {
    writeJson(RECEIVED_KEY, get(state).received);
  }
  if (toast) toaster?.(toast);
}

export function clearError(): void {
  state.update((s) => ({ ...s, error: null }));
}

export function forgetReceived(): void {
  state.update((s) => ({ ...s, received: [] }));
  writeJson(RECEIVED_KEY, []);
}

// ---------------------------------------------------------------------------------------
// Tauri plumbing
// ---------------------------------------------------------------------------------------

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new Error(typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));
  }
}

let started = false;
const unlisteners: Array<() => void> = [];

/** Idempotent. Wires backend events and loads the current shares + saved settings. */
export async function startTransfer(): Promise<void> {
  if (started) return;
  started = true;
  state.update((s) => ({ ...s, received: readJson<ReceivedFile[]>(RECEIVED_KEY, []).slice(0, RECEIVED_CAP) }));
  if (!isTauri()) return;
  const sub = async <T,>(name: string, f: (p: T) => TransferEvent) => {
    unlisteners.push(await listen<T>(name, (p) => dispatch(f(p))));
  };
  try {
    await sub<ProgressInfo>('transfer-progress', (payload) => ({ type: 'progress', payload }));
    await sub<Extract<TransferEvent, { type: 'done' }>['payload']>('transfer-done', (payload) => ({ type: 'done', payload }));
    await sub<Extract<TransferEvent, { type: 'failed' }>['payload']>('transfer-failed', (payload) => ({ type: 'failed', payload }));
    await sub<{ id: string }>('transfer-reached', (payload) => ({ type: 'reached', payload }));
    await sub<{ id: string; reason: CloseReason }>('transfer-closed', (payload) => ({ type: 'closed', payload }));
  } catch (e) {
    console.warn('[transfer] event wiring failed', e);
  }
  try {
    // re-apply the saved settings to the backend, then pick up shares that survived a reload
    const saved = await call<TransferConfig>('transfer_set_config', { config: get(config) });
    config.set({ ...get(config), ...saved });
  } catch {
    /* defaults on the Rust side are fine */
  }
  await refresh();
}

export function stopTransferListeners(): void {
  while (unlisteners.length) unlisteners.pop()?.();
  started = false;
}

/** Re-read active shares from the backend (authoritative counts). */
export async function refresh(): Promise<void> {
  if (!isTauri()) return;
  try {
    const r = await call<{ active: Array<Partial<Share> & { id: string }> }>('transfer_status');
    const list = (r.active ?? []).map(normalizeShare);
    state.update((s) => ({ ...s, shares: mergeShares(s.shares, list) }));
  } catch (e) {
    console.warn('[transfer] status failed', e);
  }
}

function optsFor(c: TransferConfig, o?: ShareOptions) {
  return { pin: o?.pin ?? c.requirePin, ttlSecs: o?.ttlSecs ?? c.ttlSecs };
}

function upsert(share: Share): void {
  state.update((s) => ({ ...s, error: null, shares: [...s.shares.filter((x) => x.id !== share.id), share] }));
}

/** Share files. Tauri: absolute paths. Demo: `{name,size}` entries from the browser. */
export async function shareFiles(inputs: ShareInput[], opts?: ShareOptions): Promise<Share> {
  if (!inputs.length) throw new Error('Choose at least one file to share.');
  const c = get(config);
  if (!isTauri()) {
    const files = inputs.map((i) => (typeof i === 'string' ? { name: i.split(/[\\/]/).pop() ?? i, size: 1_500_000 } : i));
    return demoShare('send', files, c, opts);
  }
  const paths = inputs.filter((i): i is string => typeof i === 'string');
  const raw = await call<Partial<Share> & { id: string }>('transfer_share', { paths, opts: optsFor(c, opts) });
  const share = normalizeShare(raw);
  upsert(share);
  return share;
}

/** Open an upload page for the phone ("Receive from phone"). */
export async function receiveFromPhone(opts?: ShareOptions): Promise<Share> {
  const c = get(config);
  if (!isTauri()) return demoShare('receive', [], c, opts);
  const raw = await call<Partial<Share> & { id: string }>('transfer_receive', { opts: optsFor(c, opts) });
  const share = normalizeShare(raw);
  upsert(share);
  return share;
}

/** Stop one share, or every share when `id` is omitted. */
export async function stopShare(id?: string): Promise<void> {
  const ids = id ? [id] : get(state).shares.map((s) => s.id);
  const demoIds = ids.filter((i) => demoTimers.has(i));
  for (const d of demoIds) {
    clearDemo(d);
    dispatch({ type: 'closed', payload: { id: d, reason: 'stopped' } });
  }
  if (!isTauri()) return;
  await call<void>('transfer_stop', { id: id ?? null });
}

/** Native "choose files" dialog. The Island must not collapse while it is open. */
export async function pickFiles(): Promise<string[]> {
  if (!isTauri()) return [];
  await hold(true);
  try {
    return await call<string[]>('transfer_pick_files');
  } finally {
    // the dialog returns focus to the window a beat after closing; hold through that
    setTimeout(() => void hold(false), 350);
  }
}

export async function openReceiveFolder(): Promise<void> {
  if (!isTauri()) return;
  await call<void>('transfer_open_folder');
}

export async function revealReceived(path: string): Promise<void> {
  if (!isTauri()) return;
  await call<void>('transfer_reveal', { path });
}

export async function firewallStatus(): Promise<FirewallReport> {
  if (!isTauri()) {
    return { lanIp: '192.168.1.42', adapter: 'Wi-Fi', ruleInstalled: false, profile: 'Public', exe: 'krovi-app.exe' };
  }
  return call<FirewallReport>('transfer_firewall_status');
}

/** Explicit user action: Windows shows a UAC prompt, then one scoped rule is added. */
export async function firewallAllow(): Promise<FirewallReport> {
  if (!isTauri()) {
    return { lanIp: '192.168.1.42', adapter: 'Wi-Fi', ruleInstalled: true, profile: 'Private', exe: 'krovi-app.exe' };
  }
  return call<FirewallReport>('transfer_firewall_allow');
}

/** Persist + apply settings (expiry, PIN default, upload cap). */
export async function updateConfig(patch: Partial<TransferConfig>): Promise<void> {
  const next = { ...get(config), ...patch };
  config.set(next);
  writeJson(CONFIG_KEY, next);
  if (!isTauri()) return;
  try {
    const applied = await call<TransferConfig>('transfer_set_config', { config: next });
    config.set({ ...next, ...applied });
  } catch (e) {
    console.warn('[transfer] set config failed', e);
  }
}

/**
 * OS file drops. The Tauri webview swallows native drops and reports paths instead of DOM
 * events, so the view subscribes here. Resolves to an unsubscribe (a no-op in the demo, where
 * the view uses normal DOM drag events).
 */
export async function onFileDrop(
  onDrop: (paths: string[]) => void,
  onOver?: (over: boolean) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  try {
    const { getCurrentWebview } = await import('@tauri-apps/api/webview');
    return await getCurrentWebview().onDragDropEvent((ev) => {
      const p = ev.payload;
      if (p.type === 'enter' || p.type === 'over') onOver?.(true);
      else if (p.type === 'leave') onOver?.(false);
      else if (p.type === 'drop') {
        onOver?.(false);
        if (p.paths.length) onDrop(p.paths);
      }
    });
  } catch {
    return () => {};
  }
}

// ---------------------------------------------------------------------------------------
// Browser demo: a simulated phone
// ---------------------------------------------------------------------------------------

/** Encodes http://192.168.1.42:51234/s/DEMO-not-a-real-link (generated by krovi-transfer). */
const DEMO_URL = 'http://192.168.1.42:51234/s/DEMO-not-a-real-link';
const DEMO_QR = '<svg role="img" aria-label="QR code" viewBox="0 0 41 41" shape-rendering="crispEdges"><rect width="41" height="41" fill="#ffffff"/><path d="M4 4h7v1h-7zM13 4h1v1h-1zM16 4h1v1h-1zM19 4h1v1h-1zM23 4h1v1h-1zM25 4h4v1h-4zM30 4h7v1h-7zM4 5h1v1h-1zM10 5h1v1h-1zM13 5h2v1h-2zM16 5h1v1h-1zM19 5h1v1h-1zM22 5h1v1h-1zM26 5h1v1h-1zM28 5h1v1h-1zM30 5h1v1h-1zM36 5h1v1h-1zM4 6h1v1h-1zM6 6h3v1h-3zM10 6h1v1h-1zM12 6h2v1h-2zM15 6h1v1h-1zM18 6h2v1h-2zM27 6h1v1h-1zM30 6h1v1h-1zM32 6h3v1h-3zM36 6h1v1h-1zM4 7h1v1h-1zM6 7h3v1h-3zM10 7h1v1h-1zM12 7h4v1h-4zM22 7h1v1h-1zM25 7h2v1h-2zM30 7h1v1h-1zM32 7h3v1h-3zM36 7h1v1h-1zM4 8h1v1h-1zM6 8h3v1h-3zM10 8h1v1h-1zM12 8h1v1h-1zM16 8h5v1h-5zM24 8h1v1h-1zM28 8h1v1h-1zM30 8h1v1h-1zM32 8h3v1h-3zM36 8h1v1h-1zM4 9h1v1h-1zM10 9h1v1h-1zM12 9h1v1h-1zM15 9h1v1h-1zM18 9h1v1h-1zM22 9h2v1h-2zM25 9h1v1h-1zM28 9h1v1h-1zM30 9h1v1h-1zM36 9h1v1h-1zM4 10h7v1h-7zM12 10h1v1h-1zM14 10h1v1h-1zM16 10h1v1h-1zM18 10h1v1h-1zM20 10h1v1h-1zM22 10h1v1h-1zM24 10h1v1h-1zM26 10h1v1h-1zM28 10h1v1h-1zM30 10h7v1h-7zM12 11h1v1h-1zM15 11h1v1h-1zM17 11h2v1h-2zM20 11h6v1h-6zM27 11h1v1h-1zM4 12h1v1h-1zM6 12h5v1h-5zM14 12h5v1h-5zM21 12h1v1h-1zM23 12h1v1h-1zM30 12h5v1h-5zM4 13h2v1h-2zM8 13h1v1h-1zM13 13h1v1h-1zM16 13h2v1h-2zM19 13h5v1h-5zM25 13h1v1h-1zM27 13h5v1h-5zM33 13h2v1h-2zM36 13h1v1h-1zM4 14h1v1h-1zM8 14h1v1h-1zM10 14h3v1h-3zM16 14h3v1h-3zM21 14h1v1h-1zM32 14h1v1h-1zM34 14h2v1h-2zM5 15h1v1h-1zM8 15h1v1h-1zM11 15h1v1h-1zM13 15h1v1h-1zM17 15h1v1h-1zM19 15h5v1h-5zM28 15h1v1h-1zM30 15h1v1h-1zM32 15h3v1h-3zM4 16h4v1h-4zM9 16h3v1h-3zM15 16h1v1h-1zM18 16h1v1h-1zM20 16h1v1h-1zM23 16h1v1h-1zM25 16h2v1h-2zM29 16h3v1h-3zM33 16h1v1h-1zM35 16h1v1h-1zM6 17h1v1h-1zM11 17h2v1h-2zM14 17h4v1h-4zM19 17h1v1h-1zM22 17h1v1h-1zM26 17h1v1h-1zM29 17h1v1h-1zM34 17h3v1h-3zM9 18h2v1h-2zM16 18h7v1h-7zM26 18h1v1h-1zM28 18h1v1h-1zM32 18h4v1h-4zM4 19h4v1h-4zM9 19h1v1h-1zM12 19h1v1h-1zM14 19h1v1h-1zM16 19h4v1h-4zM22 19h1v1h-1zM25 19h1v1h-1zM27 19h1v1h-1zM29 19h1v1h-1zM33 19h2v1h-2zM4 20h3v1h-3zM8 20h5v1h-5zM18 20h3v1h-3zM22 20h1v1h-1zM24 20h1v1h-1zM31 20h2v1h-2zM34 20h1v1h-1zM36 20h1v1h-1zM4 21h1v1h-1zM6 21h2v1h-2zM11 21h1v1h-1zM13 21h1v1h-1zM17 21h4v1h-4zM22 21h2v1h-2zM25 21h7v1h-7zM33 21h2v1h-2zM36 21h1v1h-1zM4 22h2v1h-2zM10 22h1v1h-1zM12 22h1v1h-1zM15 22h2v1h-2zM18 22h2v1h-2zM22 22h1v1h-1zM26 22h1v1h-1zM28 22h2v1h-2zM32 22h1v1h-1zM34 22h2v1h-2zM4 23h3v1h-3zM8 23h2v1h-2zM11 23h1v1h-1zM14 23h1v1h-1zM16 23h1v1h-1zM19 23h1v1h-1zM23 23h1v1h-1zM27 23h1v1h-1zM30 23h1v1h-1zM32 23h3v1h-3zM7 24h1v1h-1zM10 24h1v1h-1zM14 24h1v1h-1zM16 24h2v1h-2zM19 24h1v1h-1zM22 24h1v1h-1zM24 24h5v1h-5zM32 24h2v1h-2zM35 24h1v1h-1zM4 25h2v1h-2zM8 25h1v1h-1zM11 25h1v1h-1zM13 25h1v1h-1zM15 25h1v1h-1zM17 25h1v1h-1zM19 25h3v1h-3zM23 25h1v1h-1zM25 25h1v1h-1zM27 25h3v1h-3zM32 25h1v1h-1zM34 25h1v1h-1zM36 25h1v1h-1zM4 26h1v1h-1zM6 26h9v1h-9zM16 26h1v1h-1zM18 26h1v1h-1zM21 26h3v1h-3zM25 26h1v1h-1zM30 26h3v1h-3zM34 26h2v1h-2zM4 27h1v1h-1zM6 27h1v1h-1zM11 27h1v1h-1zM13 27h3v1h-3zM20 27h2v1h-2zM23 27h5v1h-5zM29 27h1v1h-1zM33 27h1v1h-1zM4 28h1v1h-1zM6 28h1v1h-1zM8 28h4v1h-4zM14 28h3v1h-3zM21 28h2v1h-2zM24 28h1v1h-1zM28 28h6v1h-6zM35 28h1v1h-1zM12 29h1v1h-1zM14 29h1v1h-1zM16 29h2v1h-2zM19 29h1v1h-1zM21 29h3v1h-3zM25 29h1v1h-1zM28 29h1v1h-1zM32 29h1v1h-1zM34 29h2v1h-2zM4 30h7v1h-7zM17 30h1v1h-1zM19 30h3v1h-3zM26 30h3v1h-3zM30 30h1v1h-1zM32 30h1v1h-1zM34 30h1v1h-1zM4 31h1v1h-1zM10 31h1v1h-1zM12 31h3v1h-3zM18 31h5v1h-5zM24 31h1v1h-1zM28 31h1v1h-1zM32 31h1v1h-1zM34 31h3v1h-3zM4 32h1v1h-1zM6 32h3v1h-3zM10 32h1v1h-1zM12 32h1v1h-1zM14 32h1v1h-1zM17 32h2v1h-2zM20 32h1v1h-1zM25 32h1v1h-1zM28 32h6v1h-6zM35 32h1v1h-1zM4 33h1v1h-1zM6 33h3v1h-3zM10 33h1v1h-1zM12 33h2v1h-2zM15 33h5v1h-5zM22 33h3v1h-3zM26 33h1v1h-1zM31 33h5v1h-5zM4 34h1v1h-1zM6 34h3v1h-3zM10 34h1v1h-1zM12 34h1v1h-1zM14 34h1v1h-1zM16 34h8v1h-8zM26 34h1v1h-1zM28 34h4v1h-4zM4 35h1v1h-1zM10 35h1v1h-1zM14 35h1v1h-1zM16 35h1v1h-1zM18 35h2v1h-2zM23 35h3v1h-3zM27 35h2v1h-2zM31 35h3v1h-3zM4 36h7v1h-7zM12 36h2v1h-2zM17 36h5v1h-5zM24 36h1v1h-1zM27 36h1v1h-1zM30 36h3v1h-3zM35 36h1v1h-1z" fill="#000000"/></svg>';

const demoTimers = new Map<string, Array<ReturnType<typeof setTimeout> | ReturnType<typeof setInterval>>>();
let demoSeq = 0;

function clearDemo(id: string): void {
  for (const t of demoTimers.get(id) ?? []) {
    clearTimeout(t as ReturnType<typeof setTimeout>);
    clearInterval(t as ReturnType<typeof setInterval>);
  }
  demoTimers.delete(id);
}

function demoShare(mode: ShareMode, files: FileInfo[], c: TransferConfig, opts?: ShareOptions): Share {
  const id = `demo-${++demoSeq}`;
  const created = Date.now();
  const ttl = (opts?.ttlSecs ?? c.ttlSecs) * 1000;
  const share = normalizeShare({
    id,
    url: DEMO_URL,
    qrSvg: DEMO_QR,
    files: mode === 'send' ? files : [],
    mode,
    expiresAt: created + ttl,
    createdAt: created,
    lanIp: '192.168.1.42',
    port: 51234,
    pin: (opts?.pin ?? c.requirePin) ? '4821' : null,
    maxUploadBytes: c.maxUploadBytes,
    saveDir: mode === 'receive' ? 'C:\\Users\\you\\Downloads\\Krovi' : null,
    demo: true,
  });
  upsert(share);
  const timers: Array<ReturnType<typeof setTimeout> | ReturnType<typeof setInterval>> = [];
  demoTimers.set(id, timers);

  // the "phone" scans the code a moment later...
  timers.push(setTimeout(() => dispatch({ type: 'reached', payload: { id } }), 2800));
  // ...and transfers the files one after another
  const queue: FileInfo[] = mode === 'send' ? files : [
    { name: 'IMG_2041.jpg', size: 3_400_000 },
    { name: 'Voice note.m4a', size: 880_000 },
    { name: 'Screenshot_2026-11-01.png', size: 1_250_000 },
  ];
  let at = 3400;
  for (const f of queue) {
    const dur = Math.max(1800, Math.min(5200, f.size / 900));
    timers.push(setTimeout(() => runDemoTransfer(id, f, mode, dur, timers), at));
    at += dur + 500;
  }
  timers.push(setTimeout(() => {
    clearDemo(id);
    dispatch({ type: 'closed', payload: { id, reason: 'expired' } });
  }, ttl));
  return share;
}

function runDemoTransfer(id: string, f: FileInfo, mode: ShareMode, dur: number, timers: Array<ReturnType<typeof setTimeout> | ReturnType<typeof setInterval>>): void {
  const direction: Direction = mode === 'send' ? 'send' : 'receive';
  const started = Date.now();
  const tick = setInterval(() => {
    const t = Math.min(1, (Date.now() - started) / dur);
    if (t < 1) {
      dispatch({ type: 'progress', payload: { id, file: f.name, bytes: Math.round(f.size * t), total: f.size, direction } });
    } else {
      clearInterval(tick);
      dispatch({
        type: 'done',
        payload: { id, file: f.name, direction, size: f.size, savedPath: direction === 'receive' ? `C:\\Users\\you\\Downloads\\Krovi\\${f.name}` : null },
      });
    }
  }, 100);
  timers.push(tick);
}

/** Test hooks. */
export const __test = {
  reset(): void {
    for (const id of [...demoTimers.keys()]) clearDemo(id);
    state.set(emptyState());
    config.set({ ...DEFAULT_CONFIG });
    started = false;
    demoSeq = 0;
  },
  state,
  dispatch,
};
