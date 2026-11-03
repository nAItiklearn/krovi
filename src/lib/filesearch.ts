// Local file-name search client, part 3.
//
//   * Tauri: thin wrappers over src-tauri/src/extras.rs (commands filesearch_*; events
//     `filesearch-hits` and `filesearch-done`). Names only: nothing is ever read from inside a file,
//     there is no index and nothing runs in the background. Links are not followed, hidden and system
//     files are skipped, the walk is bounded and can be cancelled. There is NO fixture fallback on
//     this path, ever.
//   * Browser demo (`isTauri() === false`): a short made-up list under `C:\DEMO\`, results streamed
//     with small delays so the live list can be seen, and no command is ever invoked.
//
// A result is opened by id from the latest search; the webview never sends a path to open.

import { invoke, isTauri, listen } from './bridge';
import { FOLDER_IDS, FOLDER_LABEL, demoFolderPath, type FolderId } from './organizer';

// ---------------------------------------------------------------------------------------
// Types (wire shapes: src-tauri/crates/krovi-extras/src/search.rs, camelCase)
// ---------------------------------------------------------------------------------------

export interface Hit {
  /** 1, 2, 3, ... within one search. */
  id: number;
  name: string;
  path: string;
  folder: FolderId;
  isDir: boolean;
  size: number | null;
  modifiedMs: number | null;
}

export type Stop = 'finished' | 'cancelled' | 'result-limit' | 'entry-limit' | 'time-limit';

export interface Summary {
  stop: Stop;
  /** Names looked at. */
  scanned: number;
  found: number;
  skippedLinks: number;
  skippedHidden: number;
  skippedDeep: number;
  unreadable: number;
  /** client-side only: the summary of the browser-demo list */
  demo?: boolean;
}

interface HitsEvent {
  searchId: number;
  hits: Hit[];
}

interface DoneEvent {
  searchId: number;
  ok: boolean;
  error: string | null;
  summary: Summary | null;
}

export interface SearchHandle {
  id: number;
  /** Resolves with the summary when the search ends (also when stopped); rejects if it failed. */
  done: Promise<Summary>;
  /** Stop the walk. What was found so far stays. */
  cancel(): Promise<void>;
}

export const MIN_QUERY_CHARS = 2;
export const MAX_QUERY_CHARS = 100;
export const MAX_WORDS = 6;

// ---------------------------------------------------------------------------------------
// Pure helpers (unit-tested; they mirror krovi-extras, which is the authority)
// ---------------------------------------------------------------------------------------

// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

/** The words to look for (lower-case), or the sentence saying why the search cannot start. */
export function parseQuery(q: string): { words: string[]; error: string | null } {
  const t = q.trim();
  const no = (error: string) => ({ words: [] as string[], error });
  if ([...t].length > MAX_QUERY_CHARS) return no(`That search is too long (over ${MAX_QUERY_CHARS} characters).`);
  if (CONTROL.test(t)) return no('A search cannot contain control characters.');
  const words = t.split(/\s+/).filter(Boolean).map((w) => w.toLowerCase());
  if ([...t.replace(/\s+/g, '')].length < MIN_QUERY_CHARS) return no(`Type at least ${MIN_QUERY_CHARS} characters.`);
  if (words.length > MAX_WORDS) return no(`Use at most ${MAX_WORDS} words.`);
  return { words, error: null };
}

/** `C:\Users\me\Documents\a.txt` -> `C:\Users\me\Documents` */
export function parentDir(path: string): string {
  const cut = Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'));
  return cut > 0 ? path.slice(0, cut) : path;
}

/** Types Krovi shows in their folder but never opens (they run code). Mirrors `is_risky_to_open`. */
const RISKY = new Set([
  'exe', 'msi', 'msp', 'bat', 'cmd', 'com', 'scr', 'pif', 'ps1', 'psm1', 'vbs', 'vbe', 'js', 'jse', 'wsf', 'wsh', 'hta', 'jar', 'reg', 'lnk', 'url', 'cpl', 'msc', 'dll', 'sys', 'appx', 'msix', 'application', 'gadget', 'inf', 'scf',
]);

export function canOpen(hit: Pick<Hit, 'name' | 'isDir'>): boolean {
  if (hit.isDir) return true;
  const dot = hit.name.lastIndexOf('.');
  return dot < 0 || !RISKY.has(hit.name.slice(dot + 1).toLowerCase());
}

/** One plain sentence about how a search ended; empty when it simply finished. */
export function describeStop(s: Summary): string {
  switch (s.stop) {
    case 'cancelled':
      return 'Stopped. What was found so far is listed.';
    case 'result-limit':
      return `Showing the first ${s.found} matches. Make the search more specific to narrow it down.`;
    case 'entry-limit':
      return 'Stopped after looking at a very large number of names. Make the search more specific.';
    case 'time-limit':
      return 'Stopped after 20 seconds. Make the search more specific, or pick fewer folders.';
    default:
      return '';
  }
}

/** What a search left alone, as plain sentences. */
export function skipNotes(s: Summary): string[] {
  const out: string[] = [];
  const n = (k: number, one: string, many: string) => `${k.toLocaleString('en-US')} ${k === 1 ? one : many}`;
  if (s.skippedHidden > 0) out.push(`${n(s.skippedHidden, 'hidden or system item', 'hidden or system items')} skipped.`);
  if (s.skippedLinks > 0) out.push(`${n(s.skippedLinks, 'link or cloud placeholder', 'links and cloud placeholders')} not followed.`);
  if (s.skippedDeep > 0) out.push(`${n(s.skippedDeep, 'very deep folder', 'very deep folders')} not entered.`);
  if (s.unreadable > 0) out.push(`${n(s.unreadable, 'item', 'items')} could not be read.`);
  return out;
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

/**
 * Starts a name search. `onHits` receives each batch as it arrives. `folders` limits it to some
 * known folders (empty = all six). Rejects at once with a sentence if the search cannot start.
 */
export async function startSearch(query: string, folders: FolderId[], onHits: (hits: Hit[]) => void): Promise<SearchHandle> {
  const parsed = parseQuery(query);
  if (parsed.error) throw new Error(parsed.error);
  if (!isTauri()) return demoSearch(parsed.words, folders, onHits);

  // Events can arrive before the command returns the search id: hold them until it is known.
  let id: number | null = null;
  const early: { hits?: HitsEvent; done?: DoneEvent }[] = [];
  let finish!: (s: Summary) => void;
  let fail!: (e: Error) => void;
  const done = new Promise<Summary>((resolve, reject) => {
    finish = resolve;
    fail = reject;
  });
  done.catch(() => {}); // the caller may stop listening; never an unhandled rejection

  const onBatch = (e: HitsEvent) => {
    if (id === null) early.push({ hits: e });
    else if (e.searchId === id) onHits(e.hits);
  };
  const onDone = (e: DoneEvent) => {
    if (id === null) {
      early.push({ done: e });
      return;
    }
    if (e.searchId !== id) return;
    cleanup();
    if (e.ok && e.summary) finish(e.summary);
    else fail(new Error(e.error ?? 'The search failed. Try again.'));
  };
  const offHits = await listen<HitsEvent>('filesearch-hits', onBatch);
  const offDone = await listen<DoneEvent>('filesearch-done', onDone);
  const cleanup = () => {
    offHits();
    offDone();
  };

  try {
    id = await call<number>('filesearch_start', { query, folders });
  } catch (e) {
    cleanup();
    throw e;
  }
  for (const ev of early.splice(0)) {
    if (ev.hits) onBatch(ev.hits);
    if (ev.done) onDone(ev.done);
  }
  return {
    id,
    done,
    cancel: () => call<void>('filesearch_cancel'),
  };
}

/** Open a result (a file with its app, a folder in Explorer) or show it in its folder. */
export async function openHit(searchId: number, hit: Hit, reveal: boolean): Promise<string> {
  if (isTauri()) return call<string>('filesearch_open', { searchId, hitId: hit.id, reveal });
  return reveal
    ? `DEMO: Windows would show ${hit.name} in File Explorer. Nothing is opened in the preview.`
    : `DEMO: Windows would open ${hit.name}. Nothing is opened in the preview.`;
}

// ---------------------------------------------------------------------------------------
// Browser demo: a made-up list, labelled DEMO everywhere
// ---------------------------------------------------------------------------------------

interface DemoItem {
  folder: FolderId;
  rel: string;
  isDir: boolean;
  kb: number;
  ageDays: number;
}

export const DEMO_ITEMS: DemoItem[] = [
  { folder: 'documents', rel: 'Taxes\\tax return 2024.pdf', isDir: false, kb: 640, ageDays: 80 },
  { folder: 'documents', rel: 'Taxes', isDir: true, kb: 0, ageDays: 80 },
  { folder: 'documents', rel: 'Work\\project plan.docx', isDir: false, kb: 120, ageDays: 6 },
  { folder: 'documents', rel: 'Work\\meeting notes.txt', isDir: false, kb: 4, ageDays: 1 },
  { folder: 'downloads', rel: 'invoice-march.pdf', isDir: false, kb: 220, ageDays: 40 },
  { folder: 'downloads', rel: 'invoice-april.pdf', isDir: false, kb: 180, ageDays: 12 },
  { folder: 'downloads', rel: 'DemoSetup-2.1.0.exe', isDir: false, kb: 91_000, ageDays: 60 },
  { folder: 'pictures', rel: 'Holidays\\beach.jpg', isDir: false, kb: 3400, ageDays: 200 },
  { folder: 'pictures', rel: 'Holidays', isDir: true, kb: 0, ageDays: 200 },
  { folder: 'music', rel: 'Playlists\\focus.m3u', isDir: false, kb: 2, ageDays: 30 },
  { folder: 'desktop', rel: 'todo.txt', isDir: false, kb: 1, ageDays: 0 },
  { folder: 'videos', rel: 'screen recording.mp4', isDir: false, kb: 48_000, ageDays: 3 },
];

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

function demoSearch(words: string[], folders: FolderId[], onHits: (hits: Hit[]) => void): SearchHandle {
  let cancelled = false;
  const now = Date.now();
  const wanted = folders.length ? folders : [...FOLDER_IDS];
  const found: Hit[] = [];
  const done = (async (): Promise<Summary> => {
    let scanned = 0;
    for (const item of DEMO_ITEMS) {
      if (cancelled) break;
      if (!wanted.includes(item.folder)) continue;
      await sleep(60);
      scanned++;
      const name = item.rel.slice(item.rel.lastIndexOf('\\') + 1);
      const lower = name.toLowerCase();
      if (!words.every((w) => lower.includes(w))) continue;
      const hit: Hit = {
        id: found.length + 1,
        name,
        path: `${demoFolderPath(item.folder)}\\${item.rel}`,
        folder: item.folder,
        isDir: item.isDir,
        size: item.isDir ? null : item.kb * 1024,
        modifiedMs: now - item.ageDays * 86_400_000,
      };
      found.push(hit);
      onHits([hit]);
    }
    return {
      stop: cancelled ? 'cancelled' : 'finished',
      scanned,
      found: found.length,
      skippedLinks: 0,
      skippedHidden: 0,
      skippedDeep: 0,
      unreadable: 0,
      demo: true,
    };
  })();
  return {
    id: 1,
    done,
    cancel: async () => {
      cancelled = true;
    },
  };
}

export const folderLabel = (id: FolderId): string => FOLDER_LABEL[id];
