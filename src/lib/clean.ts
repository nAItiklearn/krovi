// Clean My OS client (owner: clean).
//
// Scan -> review the real paths and sizes -> confirm -> move to the Recycle Bin.
//   * Tauri: thin wrappers over src-tauri/src/clean.rs (commands clean_*; events clean-progress,
//     clean-done, clean-delete-progress). All the rules (what is scanned, never following links,
//     re-checking every path, the Recycle Bin call) live in the Rust crate `krovi-clean`. There is
//     NO fixture fallback on this path, ever.
//   * Browser demo (`isTauri() === false`): a made-up scan, clearly labelled DEMO. Every demo path
//     starts with `C:\DEMO\`, every result carries `demo: true`, and no command is ever invoked, so
//     nothing on the machine is read or changed.
//
// The webview never sends a path to the backend: it sends ids that came from a scan.

import { invoke, isTauri, listen } from './bridge';
import { formatBytes, relativeTime } from './time';

// ---------------------------------------------------------------------------------------
// Types (wire shapes: src-tauri/crates/krovi-clean/src/model.rs, all camelCase)
// ---------------------------------------------------------------------------------------

export interface CategoryInfo {
  id: string;
  title: string;
  /** Plain English: what this is. */
  description: string;
  /** Plain English: what could go wrong and what Krovi does about it. */
  riskNote: string;
  defaultSelected: boolean;
  /** e.g. "Your files - review first" */
  badge: string | null;
  /** Report only (the Recycle Bin): no checkbox, nothing is ever deleted. */
  readOnly: boolean;
}

export interface ScanItem {
  /** `<category>#<n>`, stable within one scan. */
  id: string;
  category: string;
  path: string;
  /** Friendly name for folders that are not obvious from the path. */
  label: string | null;
  isDir: boolean;
  bytes: number;
  /** Files inside (1 for a file). */
  files: number;
  /** Newest modification inside, epoch ms. */
  modifiedMs: number | null;
}

export interface CategoryResult {
  id: string;
  /** Everything that would be moved for this category (listed + unlisted). */
  bytes: number;
  files: number;
  /** Largest first. Only these can be ticked. */
  items: ScanItem[];
  unlistedItems: number;
  unlistedBytes: number;
  unlistedFiles: number;
  skippedRecent: number;
  skippedLinks: number;
  skippedDeep: number;
  unreadable: number;
  rootsFound: number;
}

export interface Scan {
  scanId: number;
  categories: CategoryResult[];
  /** Stopped early: what is listed is correct but incomplete. */
  cancelled: boolean;
  scannedAtMs: number;
  /** client-side only: this scan is the browser-demo fixture */
  demo?: boolean;
}

export interface Failure {
  path: string;
  reason: string;
}

export interface DeleteReport {
  /** Bytes now in the Recycle Bin (disk space is freed once it is emptied). */
  freedBytes: number;
  /** Items moved to the Recycle Bin. */
  deleted: number;
  deletedFiles: number;
  failed: Failure[];
  alreadyGone: number;
  cancelled: boolean;
  notAttempted: number;
  /** client-side only: nothing was really moved */
  demo?: boolean;
}

export interface ScanProgress {
  category: string;
  files: number;
  bytes: number;
}

export interface DeleteProgress {
  done: number;
  total: number;
}

export interface RecycleBinSize {
  bytes: number;
  items: number;
}

interface DoneEvent {
  ok: boolean;
  cancelled: boolean;
  error: string | null;
  scan: Scan | null;
}

// ---------------------------------------------------------------------------------------
// Pure helpers (unit-tested)
// ---------------------------------------------------------------------------------------

export const DEMO_ROOT = 'C:\\DEMO\\';
/** Items shown per category before "Show more". */
export const PAGE = 50;

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${formatCount(n)} ${n === 1 ? one : many}`;
}

export function formatCount(n: number): string {
  return Math.round(n).toLocaleString('en-US');
}

/** The exact sentence on the confirm step: "Move 12 items (1.2 GB) to the Recycle Bin". */
export function confirmText(items: number, bytes: number): string {
  return `Move ${plural(items, 'item')} (${formatBytes(bytes)}) to the Recycle Bin`;
}

export function allItems(scan: Scan): ScanItem[] {
  return scan.categories.flatMap((c) => c.items);
}

/** Ids of every listed item in the categories that start ticked (never read-only ones). */
export function defaultSelection(scan: Scan, infos: CategoryInfo[]): Set<string> {
  const on = new Set(infos.filter((i) => i.defaultSelected && !i.readOnly).map((i) => i.id));
  const sel = new Set<string>();
  for (const c of scan.categories) {
    if (!on.has(c.id)) continue;
    for (const it of c.items) sel.add(it.id);
  }
  return sel;
}

export type TickState = 'none' | 'some' | 'all';

export function categoryState(sel: ReadonlySet<string>, cat: CategoryResult): TickState {
  if (cat.items.length === 0) return 'none';
  let n = 0;
  for (const it of cat.items) if (sel.has(it.id)) n++;
  return n === 0 ? 'none' : n === cat.items.length ? 'all' : 'some';
}

export function toggleItem(sel: ReadonlySet<string>, id: string): Set<string> {
  const next = new Set(sel);
  if (!next.delete(id)) next.add(id);
  return next;
}

/** Tick (or untick) every listed item of a category. With `on` omitted: all -> none, else all. */
export function toggleCategory(sel: ReadonlySet<string>, cat: CategoryResult, on?: boolean): Set<string> {
  const next = new Set(sel);
  const turnOn = on ?? categoryState(sel, cat) !== 'all';
  for (const it of cat.items) {
    if (turnOn) next.add(it.id);
    else next.delete(it.id);
  }
  return next;
}

/** Drop ids that are not in the scan (a rescan replaced it). */
export function pruneSelection(sel: ReadonlySet<string>, scan: Scan): Set<string> {
  const known = new Set(allItems(scan).map((i) => i.id));
  return new Set([...sel].filter((id) => known.has(id)));
}

export interface Totals {
  items: number;
  files: number;
  bytes: number;
}

export function selectionTotals(scan: Scan, sel: ReadonlySet<string>): Totals {
  const t: Totals = { items: 0, files: 0, bytes: 0 };
  for (const it of allItems(scan)) {
    if (!sel.has(it.id)) continue;
    t.items++;
    t.files += it.files;
    t.bytes += it.bytes;
  }
  return t;
}

/** Per-category totals of what is ticked, in scan order, categories with nothing ticked left out. */
export function selectionBreakdown(scan: Scan, sel: ReadonlySet<string>): Array<{ id: string } & Totals> {
  const out: Array<{ id: string } & Totals> = [];
  for (const c of scan.categories) {
    const t: Totals = { items: 0, files: 0, bytes: 0 };
    for (const it of c.items) {
      if (!sel.has(it.id)) continue;
      t.items++;
      t.files += it.files;
      t.bytes += it.bytes;
    }
    if (t.items > 0) out.push({ id: c.id, ...t });
  }
  return out;
}

/** "C:\Users\me\AppData\Local\Temp\a.tmp" -> { dir: "C:\Users\me\AppData\Local\Temp\", name: "a.tmp" } */
export function splitPath(path: string): { dir: string; name: string } {
  const cut = Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'));
  if (cut < 0) return { dir: '', name: path };
  return { dir: path.slice(0, cut + 1), name: path.slice(cut + 1) || path };
}

/** The line shown for one item: its label if it has one, else the file or folder name. */
export function itemTitle(it: ScanItem): string {
  return it.label ?? splitPath(it.path).name;
}

/** "3 files" / "a folder with 41 files" style detail for an item. */
export function itemDetail(it: ScanItem): string {
  if (!it.isDir) return '1 file';
  return it.files === 1 ? 'folder, 1 file' : `folder, ${formatCount(it.files)} files`;
}

export function itemAge(it: ScanItem, now: number): string {
  return it.modifiedMs == null ? '' : relativeTime(it.modifiedMs, now);
}

/** Plain sentences about what a scan left alone in a category. Empty when it left nothing alone. */
export function skipNotes(c: CategoryResult): string[] {
  const notes: string[] = [];
  if (c.skippedRecent > 0) {
    notes.push(`${plural(c.skippedRecent, 'file')} left alone because ${c.skippedRecent === 1 ? 'it' : 'they'} changed too recently.`);
  }
  if (c.skippedLinks > 0) {
    notes.push(`${plural(c.skippedLinks, 'link')} (shortcuts and cloud placeholders) not followed.`);
  }
  if (c.skippedDeep > 0) {
    notes.push(`${plural(c.skippedDeep, 'very deeply nested folder')} not entered.`);
  }
  if (c.unreadable > 0) {
    notes.push(`${plural(c.unreadable, 'item')} skipped because Windows did not allow Krovi to read ${c.unreadable === 1 ? 'it' : 'them'} (Krovi never asks for administrator rights).`);
  }
  if (c.unlistedItems > 0) {
    notes.push(`${plural(c.unlistedItems, 'smaller item')} (${formatBytes(c.unlistedBytes)}) not listed and will not be touched. Scan again after cleaning to see them.`);
  }
  return notes;
}

export interface ReportSummary {
  headline: string;
  lines: string[];
  /** Something was moved and nothing went wrong. */
  clean: boolean;
}

export function summarizeReport(r: DeleteReport): ReportSummary {
  const lines: string[] = [];
  const headline =
    r.deleted > 0
      ? `Moved ${plural(r.deleted, 'item')} (${formatBytes(r.freedBytes)}) to the Recycle Bin.`
      : r.cancelled
        ? 'Stopped before anything was moved.'
        : 'Nothing was moved.';
  if (r.failed.length > 0) {
    lines.push(`${plural(r.failed.length, 'item')} could not be moved and ${r.failed.length === 1 ? 'was' : 'were'} left where ${r.failed.length === 1 ? 'it was' : 'they were'}.`);
  }
  if (r.alreadyGone > 0) lines.push(`${plural(r.alreadyGone, 'item')} had already disappeared.`);
  if (r.cancelled && r.notAttempted > 0) {
    lines.push(`You stopped it: ${plural(r.notAttempted, 'item')} ${r.notAttempted === 1 ? 'was' : 'were'} not touched.`);
  }
  if (r.deleted > 0) {
    lines.push('You can restore anything from the Recycle Bin. Disk space comes back once you empty it.');
  }
  return { headline, lines, clean: r.deleted > 0 && r.failed.length === 0 && !r.cancelled };
}

function normalizeScan(raw: Scan): Scan {
  return {
    ...raw,
    categories: (raw.categories ?? []).map((c) => ({ ...c, items: c.items ?? [] })),
  };
}

function normalizeReport(raw: DeleteReport): DeleteReport {
  return { ...raw, failed: raw.failed ?? [] };
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

/** The fixed list of categories with their descriptions and risk notes. */
export async function categories(): Promise<CategoryInfo[]> {
  if (!isTauri()) return structuredCloneSafe(DEMO_CATEGORIES);
  return call<CategoryInfo[]>('clean_categories');
}

/**
 * Scans the given categories (all when empty) and resolves with the result. Only reads.
 * `onProgress` receives what has been looked at so far. Rejects if the scan fails.
 */
export async function scan(ids: string[] = [], onProgress?: (p: ScanProgress) => void): Promise<Scan> {
  if (!isTauri()) return demoScan(ids, onProgress);
  // Listen BEFORE asking, so no event can be missed.
  const offProgress = await listen<ScanProgress>('clean-progress', (p) => onProgress?.(p));
  let finish!: (s: Scan) => void;
  let fail!: (e: Error) => void;
  const finished = new Promise<Scan>((resolve, reject) => {
    finish = resolve;
    fail = reject;
  });
  const offDone = await listen<DoneEvent>('clean-done', (e) => {
    if (e.ok && e.scan) finish(normalizeScan(e.scan));
    else fail(new Error(e.error ?? 'The scan failed. Try again.'));
  });
  try {
    await call<void>('clean_scan', { categories: ids });
    return await finished;
  } finally {
    offProgress();
    offDone();
  }
}

/** Stop the scan or the clean that is running. */
export async function cancel(): Promise<void> {
  if (!isTauri()) {
    demoCancelled = true;
    return;
  }
  await call<void>('clean_cancel');
}

/**
 * Moves the chosen items to the Recycle Bin. Only ids from the scan with `scanId` are sent;
 * the backend re-checks every path against the disk before moving it.
 */
export async function deleteItems(
  scanId: number,
  choice: { itemIds: string[]; categoryIds?: string[] },
  onProgress?: (p: DeleteProgress) => void,
): Promise<DeleteReport> {
  if (!isTauri()) return demoDelete(scanId, choice, onProgress);
  const off = onProgress ? await listen<DeleteProgress>('clean-delete-progress', onProgress) : () => {};
  try {
    const report = await call<DeleteReport>('clean_delete', {
      scanId,
      itemIds: choice.itemIds,
      categoryIds: choice.categoryIds ?? [],
    });
    return normalizeReport(report);
  } finally {
    off();
  }
}

/** Size of the Recycle Bin (measured only; Krovi never empties it). `null` if unknown. */
export async function recycleBinSize(): Promise<RecycleBinSize | null> {
  if (!isTauri()) return { bytes: 1_180_000_000, items: 25 };
  return call<RecycleBinSize | null>('clean_recycle_bin_size');
}

// ---------------------------------------------------------------------------------------
// Browser demo: a made-up scan, labelled DEMO everywhere
// ---------------------------------------------------------------------------------------

function structuredCloneSafe<T>(v: T): T {
  return typeof structuredClone === 'function' ? structuredClone(v) : JSON.parse(JSON.stringify(v));
}

/** Mirrors `krovi_clean::categories::INFOS` (same ids and wording). */
export const DEMO_CATEGORIES: CategoryInfo[] = [
  {
    id: 'user-temp',
    title: 'Temporary files (your account)',
    description:
      'Leftovers that apps and installers drop in your Temp folder. Anything changed in the last 24 hours is left alone because an app may still be using it.',
    riskNote: 'Low risk. Apps recreate what they need. Files that are in use cannot be moved and are reported, not forced.',
    defaultSelected: true,
    badge: null,
    readOnly: false,
  },
  {
    id: 'windows-temp',
    title: 'Windows temporary files',
    description:
      'Temporary files from Windows and system installers (the Temp folder inside the Windows folder). Anything changed in the last 24 hours is left alone.',
    riskNote:
      'Low risk. Windows may not let Krovi remove everything here without administrator rights. Those items are skipped. Krovi never asks for elevation.',
    defaultSelected: true,
    badge: null,
    readOnly: false,
  },
  {
    id: 'browser-cache',
    title: 'Browser caches',
    description:
      'Saved copies of pages, scripts and images that Chrome, Edge, Brave and Firefox keep so sites load faster. Only the cache folders are listed. Cookies, history and logins are never touched.',
    riskNote: 'Low risk. Close the browser first, or the cache it is using cannot be moved. Sites load a little slower once while the cache fills up again.',
    defaultSelected: true,
    badge: null,
    readOnly: false,
  },
  {
    id: 'thumbnails',
    title: 'Thumbnail cache',
    description: 'Small preview pictures File Explorer saves for images, videos and documents.',
    riskNote: 'Low risk. Windows rebuilds them the next time you open a folder. Files Explorer is using right now cannot be moved and are reported.',
    defaultSelected: true,
    badge: null,
    readOnly: false,
  },
  {
    id: 'crash-reports',
    title: 'Crash dumps and error reports',
    description: 'Memory dumps and reports that Windows and apps save after something crashes.',
    riskNote: 'Low risk unless you are debugging a crash and still need them. Some system report folders need administrator rights and are skipped.',
    defaultSelected: true,
    badge: null,
    readOnly: false,
  },
  {
    id: 'installers',
    title: 'Old installers in Downloads',
    description: 'Setup programs (.exe and .msi) sitting directly in your Downloads folder that have not changed in 30 days.',
    riskNote:
      'These are your own files, not system leftovers. Look at each one before you tick it: you may want an installer again. Not selected by default.',
    defaultSelected: false,
    badge: 'Your files - review first',
    readOnly: false,
  },
  {
    id: 'recycle-bin',
    title: 'Recycle Bin',
    description: 'What is in the Recycle Bin right now. Krovi only shows its size.',
    riskNote:
      'Krovi does not empty the Recycle Bin, because that cannot be undone. Empty it yourself from the desktop when you are sure. Everything Krovi cleans is moved here, so disk space only comes back once you empty it.',
    defaultSelected: false,
    badge: null,
    readOnly: true,
  },
];

const MB = 1024 * 1024;
const DAY_MS = 24 * 3600 * 1000;

interface DemoItem {
  path: string;
  label?: string;
  isDir: boolean;
  mb: number;
  files: number;
  ageDays: number;
}

const T = `${DEMO_ROOT}Users\\demo\\AppData\\Local\\Temp\\`;
const W = `${DEMO_ROOT}Windows\\Temp\\`;
const L = `${DEMO_ROOT}Users\\demo\\AppData\\Local\\`;
const DL = `${DEMO_ROOT}Users\\demo\\Downloads\\`;

const DEMO_ITEMS: Record<string, DemoItem[]> = {
  'user-temp': [
    { path: `${T}installer-cache-9f2c`, isDir: true, mb: 312, files: 41, ageDays: 9 },
    { path: `${T}chrome_unpacker_7821`, isDir: true, mb: 48.4, files: 903, ageDays: 3 },
    { path: `${T}setup-trace.log`, isDir: false, mb: 2.1, files: 1, ageDays: 12 },
    { path: `${T}tmp4F2A.tmp`, isDir: false, mb: 0.4, files: 1, ageDays: 2 },
  ],
  'windows-temp': [
    { path: `${W}MpCmdRun.log`, isDir: false, mb: 6.3, files: 1, ageDays: 20 },
    { path: `${W}wuinstall-6d1e`, isDir: true, mb: 154, files: 12, ageDays: 31 },
  ],
  'browser-cache': [
    { path: `${L}Google\\Chrome\\User Data\\Default\\Cache\\Cache_Data`, label: 'Chrome / Default / Cache', isDir: true, mb: 842, files: 6120, ageDays: 0 },
    { path: `${L}Microsoft\\Edge\\User Data\\Default\\Cache\\Cache_Data`, label: 'Edge / Default / Cache', isDir: true, mb: 403, files: 2780, ageDays: 0 },
    { path: `${L}Google\\Chrome\\User Data\\Default\\Code Cache`, label: 'Chrome / Default / Code Cache', isDir: true, mb: 61, files: 540, ageDays: 1 },
    { path: `${L}Google\\Chrome\\User Data\\Default\\GPUCache`, label: 'Chrome / Default / GPU Cache', isDir: true, mb: 9.5, files: 8, ageDays: 1 },
  ],
  thumbnails: [
    { path: `${L}Microsoft\\Windows\\Explorer\\thumbcache_256.db`, isDir: false, mb: 41, files: 1, ageDays: 5 },
    { path: `${L}Microsoft\\Windows\\Explorer\\thumbcache_1024.db`, isDir: false, mb: 33, files: 1, ageDays: 5 },
    { path: `${L}Microsoft\\Windows\\Explorer\\thumbcache_idx.db`, isDir: false, mb: 0.9, files: 1, ageDays: 5 },
  ],
  'crash-reports': [
    { path: `${L}CrashDumps\\demo-app.exe.4120.dmp`, isDir: false, mb: 224, files: 1, ageDays: 40 },
    { path: `${DEMO_ROOT}ProgramData\\Microsoft\\Windows\\WER\\ReportArchive\\AppCrash_demo-app_1`, isDir: true, mb: 3.2, files: 4, ageDays: 40 },
  ],
  installers: [
    { path: `${DL}DemoSetup-2.1.0.exe`, isDir: false, mb: 186, files: 1, ageDays: 95 },
    { path: `${DL}demo-driver-pack.msi`, isDir: false, mb: 64, files: 1, ageDays: 61 },
    { path: `${DL}OldTool-Installer.exe`, isDir: false, mb: 12.7, files: 1, ageDays: 34 },
  ],
};

let demoSeq = 0;
let demoCancelled = false;
let demoStored: Scan | null = null;

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/** Builds one category of the demo scan. Largest first, like the backend. */
export function demoCategory(id: string, now: number): CategoryResult {
  const items: ScanItem[] = [...(DEMO_ITEMS[id] ?? [])]
    .sort((a, b) => b.mb - a.mb)
    .map((d, i) => ({
      id: `${id}#${i + 1}`,
      category: id,
      path: d.path,
      label: d.label ?? null,
      isDir: d.isDir,
      bytes: Math.round(d.mb * MB),
      files: d.files,
      modifiedMs: now - d.ageDays * DAY_MS,
    }));
  return {
    id,
    bytes: items.reduce((n, i) => n + i.bytes, 0),
    files: items.reduce((n, i) => n + i.files, 0),
    items,
    unlistedItems: 0,
    unlistedBytes: 0,
    unlistedFiles: 0,
    skippedRecent: id === 'user-temp' ? 214 : id === 'installers' ? 2 : 0,
    skippedLinks: id === 'user-temp' ? 1 : 0,
    skippedDeep: 0,
    unreadable: id === 'windows-temp' ? 3 : id === 'crash-reports' ? 17 : 0,
    rootsFound: 1,
  };
}

async function demoScan(ids: string[], onProgress?: (p: ScanProgress) => void): Promise<Scan> {
  demoCancelled = false;
  const wanted = ids.length ? ids : DEMO_CATEGORIES.map((c) => c.id);
  const now = Date.now();
  const out: CategoryResult[] = [];
  let cancelled = false;
  for (const info of DEMO_CATEGORIES) {
    if (!wanted.includes(info.id)) continue;
    if (demoCancelled) {
      cancelled = true;
      break;
    }
    if (info.readOnly) {
      out.push({ ...demoCategory(info.id, now), bytes: 1_180_000_000, files: 25 });
      continue;
    }
    const cat = demoCategory(info.id, now);
    const steps = 3;
    for (let s = 1; s <= steps; s++) {
      await sleep(70);
      onProgress?.({ category: info.id, files: Math.round((cat.files * s) / steps), bytes: Math.round((cat.bytes * s) / steps) });
    }
    out.push(cat);
  }
  const result: Scan = { scanId: ++demoSeq, categories: out, cancelled, scannedAtMs: now, demo: true };
  demoStored = result;
  return structuredCloneSafe(result);
}

async function demoDelete(
  scanId: number,
  choice: { itemIds: string[]; categoryIds?: string[] },
  onProgress?: (p: DeleteProgress) => void,
): Promise<DeleteReport> {
  demoCancelled = false;
  if (!demoStored || demoStored.scanId !== scanId) {
    throw new Error('This scan is out of date. Scan again before cleaning.');
  }
  const wantedCats = new Set(choice.categoryIds ?? []);
  const wantedIds = new Set(choice.itemIds);
  const chosen = allItems(demoStored).filter((i) => wantedIds.has(i.id) || wantedCats.has(i.category));
  const report: DeleteReport = {
    freedBytes: 0,
    deleted: 0,
    deletedFiles: 0,
    failed: [],
    alreadyGone: 0,
    cancelled: false,
    notAttempted: 0,
    demo: true,
  };
  onProgress?.({ done: 0, total: chosen.length });
  for (let n = 0; n < chosen.length; n++) {
    if (demoCancelled) {
      report.cancelled = true;
      report.notAttempted = chosen.length - n;
      break;
    }
    await sleep(40);
    const it = chosen[n];
    // one believable failure so the "could not be moved" part of the UI can be seen
    if (it.category === 'browser-cache' && it.label?.startsWith('Edge')) {
      report.failed.push({ path: it.path, reason: 'It is in use by another program.' });
    } else {
      report.deleted++;
      report.deletedFiles += it.files;
      report.freedBytes += it.bytes;
    }
    onProgress?.({ done: n + 1, total: chosen.length });
  }
  demoStored = null;
  return report;
}

/** Test hooks. */
export const __test = {
  reset(): void {
    demoSeq = 0;
    demoCancelled = false;
    demoStored = null;
  },
};
