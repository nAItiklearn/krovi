// File-organizer client, part 2: rules -> Preview -> Confirm -> Run -> Undo.
//
//   * Tauri: thin wrappers over src-tauri/src/extras.rs (commands organizer_*). All the rules live in
//     the Rust crate `krovi-extras::organizer`: only your six known folders, moves only (never delete,
//     never overwrite: a clash becomes "name (2)"), links never followed, every file re-checked right
//     before it moves. There is NO fixture fallback on this path, ever.
//   * Browser demo (`isTauri() === false`): made-up files under `C:\DEMO\`, a small TypeScript copy
//     of the matching rules, every result carries `demo: true`, and no command is ever invoked, so
//     nothing on the machine is read or changed.
//
// The webview never sends a path to move: it sends rules, and later the id of a preview it was shown.

import { plural, splitPath } from './clean';
import { invoke, isTauri } from './bridge';
import { formatBytes } from './time';

// ---------------------------------------------------------------------------------------
// Types (wire shapes: src-tauri/crates/krovi-extras/src/organizer.rs, camelCase)
// ---------------------------------------------------------------------------------------

export type FolderId = 'downloads' | 'documents' | 'pictures' | 'desktop' | 'videos' | 'music';

export const FOLDER_IDS: readonly FolderId[] = ['downloads', 'documents', 'pictures', 'desktop', 'videos', 'music'];

export const FOLDER_LABEL: Record<FolderId, string> = {
  downloads: 'Downloads',
  documents: 'Documents',
  pictures: 'Pictures',
  desktop: 'Desktop',
  videos: 'Videos',
  music: 'Music',
};

export interface FolderInfo {
  id: FolderId;
  label: string;
  /** Where Windows says it is, or null if it is not on this PC. */
  path: string | null;
}

export interface Rule {
  id: string;
  enabled: boolean;
  folder: FolderId;
  /** Lower-case, no dot. Empty = any type. */
  extensions: string[];
  nameContains: string;
  olderThanDays: number | null;
  destFolder: FolderId;
  /** A folder (up to three levels, `\`-separated) inside `destFolder`. */
  destSubfolder: string;
}

/** What the form sends; the backend assigns the id on first save. */
export type RuleInput = Omit<Rule, 'id'> & { id?: string };

export interface PlannedMove {
  id: string;
  ruleId: string;
  srcFolder: FolderId;
  destFolder: FolderId;
  destSub: string;
  from: string;
  toDir: string;
  /** The final name: the original, or `name (2).ext` after a clash. */
  toName: string;
  size: number;
  modifiedMs: number;
}

export interface Preview {
  previewId: number;
  moves: PlannedMove[];
  /** Plain sentences about what was left alone and why. */
  notes: string[];
  truncated: boolean;
  createdMs: number;
  rulesChecked: number;
  /** client-side only: the preview is the browser-demo fixture */
  demo?: boolean;
}

export interface Moved {
  from: string;
  to: string;
  size: number;
  modifiedMs: number;
  srcFolder: FolderId;
  destFolder: FolderId;
}

export interface Failure {
  path: string;
  reason: string;
}

export interface RunReport {
  moved: Moved[];
  failed: Failure[];
  alreadyGone: number;
  /** client-side only: nothing was really moved */
  demo?: boolean;
}

export interface UndoInfo {
  runId: number;
  atMs: number;
  count: number;
}

export interface UndoReport {
  restored: number;
  failed: Failure[];
  /** client-side only: nothing was really moved */
  demo?: boolean;
}

export const LIMITS = {
  rules: 20,
  moves: 500,
  extensions: 20,
  extensionChars: 12,
  containsChars: 60,
  subfolderSegments: 3,
  segmentChars: 64,
  maxAgeDays: 3650,
} as const;

// ---------------------------------------------------------------------------------------
// Pure helpers (unit-tested; they mirror krovi-extras, which is the authority)
// ---------------------------------------------------------------------------------------

// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

const E = {
  noCondition: 'Add at least one condition: a file type, a word in the name, or an age. A rule with none would move everything.',
  badExtension: `File types are letters and digits only, up to ${LIMITS.extensionChars} characters, like pdf or jpg.`,
  tooManyExtensions: `At most ${LIMITS.extensions} file types per rule.`,
  containsBad: 'The word to look for in the name cannot contain \\ / : * ? " < > | or control characters.',
  containsLong: `That word is too long (over ${LIMITS.containsChars} characters).`,
  age: `The age must be between 1 and ${LIMITS.maxAgeDays} days.`,
  subEmpty: 'Name the folder to move files into, for example PDFs. Files are never moved into the top of a folder.',
  subAbsolute: 'Give a folder name inside the destination, not a full path (no drive letter or leading slash).',
  subBad: 'A folder name cannot contain \\\\ / : * ? " < > |, end with a dot or space, be . or .., or be a reserved Windows name.',
  subDeep: `The destination can be at most ${LIMITS.subfolderSegments} folders deep.`,
  subLong: `A folder name is too long (over ${LIMITS.segmentChars} characters).`,
  tooManyRules: `At most ${LIMITS.rules} rules can be saved.`,
} as const;

/** "pdf, *.DOCX .xlsx" -> ["pdf", "docx", "xlsx"], or the reason a type is not acceptable. */
export function parseExtensions(text: string): { list: string[]; error: string | null } {
  const list: string[] = [];
  for (const tok of text.split(/[,;\s]+/)) {
    const t = tok.trim().replace(/^\*+/, '').replace(/^\.+/, '').toLowerCase();
    if (!t) continue;
    if ([...t].length > LIMITS.extensionChars || !/^[a-z0-9_+-]+$/.test(t)) return { list: [], error: E.badExtension };
    if (!list.includes(t)) list.push(t);
  }
  if (list.length > LIMITS.extensions) return { list: [], error: E.tooManyExtensions };
  return { list, error: null };
}

const RESERVED = new Set(['con', 'prn', 'aux', 'nul']);

function isReservedName(seg: string): boolean {
  const stem = seg.split('.')[0].toLowerCase();
  return RESERVED.has(stem) || /^(com|lpt)\d$/.test(stem);
}

/** A destination folder as clean segments: relative only, no `.`/`..`, nothing Windows treats specially. */
export function cleanSubfolder(raw: string): { segments: string[]; error: string | null } {
  const bad = (error: string) => ({ segments: [] as string[], error });
  const t = raw.trim();
  if (t.startsWith('\\') || t.startsWith('/')) return bad(E.subAbsolute);
  const u = t.replace(/[\\/]+$/, '').trim();
  if (!u) return bad(E.subEmpty);
  if (u.includes(':')) return bad(E.subAbsolute);
  const segments = u
    .replace(/\//g, '\\')
    .split('\\')
    .map((s) => s.trim());
  if (segments.length > LIMITS.subfolderSegments) return bad(E.subDeep);
  for (const s of segments) {
    if (!s || s === '.' || s === '..' || s.endsWith('.') || CONTROL.test(s) || /[<>:"|?*]/.test(s) || isReservedName(s)) return bad(E.subBad);
    if ([...s].length > LIMITS.segmentChars) return bad(E.subLong);
  }
  return { segments, error: null };
}

/** The sentence the backend would answer with, or `null` if the rule is fine. */
export function validateRuleInput(input: RuleInput): string | null {
  const ext = parseExtensions(input.extensions.join(','));
  if (ext.error) return ext.error;
  const contains = input.nameContains.trim();
  if ([...contains].length > LIMITS.containsChars) return E.containsLong;
  if (CONTROL.test(contains) || /[\\/:*?"<>|]/.test(contains)) return E.containsBad;
  const age = input.olderThanDays;
  if (age != null && (!Number.isInteger(age) || age < 0 || age > LIMITS.maxAgeDays)) return E.age;
  if (ext.list.length === 0 && !contains && !age) return E.noCondition;
  return cleanSubfolder(input.destSubfolder).error;
}

/** A rule as normalised by the backend (extensions parsed, age 0 -> none, folder path tidied). */
export function normalizeRule(input: RuleInput): RuleInput {
  return {
    ...(input.id ? { id: input.id } : {}),
    enabled: input.enabled,
    folder: input.folder,
    extensions: parseExtensions(input.extensions.join(',')).list,
    nameContains: input.nameContains.trim(),
    olderThanDays: input.olderThanDays && input.olderThanDays > 0 ? input.olderThanDays : null,
    destFolder: input.destFolder,
    destSubfolder: cleanSubfolder(input.destSubfolder).segments.join('\\'),
  };
}

/** "In Downloads, move .pdf files older than 7 days to Documents\PDFs" */
export function describeRule(r: RuleInput): string {
  const what = r.extensions.length ? `${r.extensions.map((e) => `.${e}`).join(', ')} files` : 'files';
  let s = `In ${FOLDER_LABEL[r.folder]}, move ${what}`;
  if (r.nameContains) s += ` with "${r.nameContains}" in the name`;
  if (r.olderThanDays) s += ` older than ${r.olderThanDays} day${r.olderThanDays === 1 ? '' : 's'}`;
  return `${s} to ${FOLDER_LABEL[r.destFolder]}\\${r.destSubfolder}`;
}

export function emptyRule(): RuleInput {
  return { enabled: true, folder: 'downloads', extensions: [], nameContains: '', olderThanDays: null, destFolder: 'documents', destSubfolder: '' };
}

/** Does a file name satisfy a rule's type and name conditions? (Age is checked separately.) */
export function nameMatches(r: Pick<RuleInput, 'extensions' | 'nameContains'>, name: string): boolean {
  if (r.extensions.length) {
    const dot = name.lastIndexOf('.');
    if (dot <= 0 || !r.extensions.includes(name.slice(dot + 1).toLowerCase())) return false;
  }
  return !r.nameContains || name.toLowerCase().includes(r.nameContains.toLowerCase());
}

export function ageMatches(r: Pick<RuleInput, 'olderThanDays'>, modifiedMs: number | null, now: number): boolean {
  if (!r.olderThanDays) return true;
  return modifiedMs != null && now - modifiedMs >= r.olderThanDays * 86_400_000;
}

export function fileName(path: string): string {
  return splitPath(path).name;
}

export interface Summary {
  headline: string;
  lines: string[];
  /** Everything listed was moved and nothing went wrong. */
  clean: boolean;
}

export function summarizeRun(r: RunReport): Summary {
  const lines: string[] = [];
  const headline = r.moved.length > 0 ? `Moved ${plural(r.moved.length, 'file')}.` : 'Nothing was moved.';
  if (r.failed.length > 0) {
    lines.push(`${plural(r.failed.length, 'file')} could not be moved and ${r.failed.length === 1 ? 'was' : 'were'} left where ${r.failed.length === 1 ? 'it was' : 'they were'}.`);
  }
  if (r.alreadyGone > 0) lines.push(`${plural(r.alreadyGone, 'file')} had already disappeared.`);
  if (r.moved.length > 0) lines.push('Nothing was deleted or replaced. You can undo this run below.');
  return { headline, lines, clean: r.moved.length > 0 && r.failed.length === 0 };
}

export function summarizeUndo(r: UndoReport): Summary {
  const lines: string[] = [];
  const headline = r.restored > 0 ? `Put back ${plural(r.restored, 'file')}.` : 'Nothing was put back.';
  if (r.failed.length > 0) {
    lines.push(`${plural(r.failed.length, 'file')} ${r.failed.length === 1 ? 'was' : 'were'} left where ${r.failed.length === 1 ? 'it is' : 'they are'} because ${r.failed.length === 1 ? 'it was' : 'they were'} changed or the old name is taken.`);
  }
  if (r.restored > 0) lines.push('Folders that were created for the move are left in place.');
  return { headline, lines, clean: r.restored > 0 && r.failed.length === 0 };
}

export function totalBytes(moves: readonly PlannedMove[]): number {
  return moves.reduce((n, m) => n + m.size, 0);
}

/** The exact sentence on the confirm button: "Move 12 files (1.2 MB)". */
export function confirmText(moves: readonly PlannedMove[]): string {
  return `Move ${plural(moves.length, 'file')} (${formatBytes(totalBytes(moves))})`;
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

const clone = <T>(v: T): T => (typeof structuredClone === 'function' ? structuredClone(v) : (JSON.parse(JSON.stringify(v)) as T));

/** The six known folders and where they are on this PC. */
export async function folders(): Promise<FolderInfo[]> {
  if (!isTauri()) return FOLDER_IDS.map((id) => ({ id, label: FOLDER_LABEL[id], path: demoFolderPath(id) }));
  return call<FolderInfo[]>('organizer_folders');
}

export async function rules(): Promise<Rule[]> {
  if (!isTauri()) return clone(demoRules());
  return call<Rule[]>('organizer_rules');
}

/** Add a rule, or update the one with the same id. Resolves to the whole list. */
export async function saveRule(input: RuleInput): Promise<Rule[]> {
  if (isTauri()) return call<Rule[]>('organizer_save_rule', { rule: normalizeRule(input) });
  const why = validateRuleInput(input);
  if (why) throw new Error(why);
  const list = demoRules();
  const clean = normalizeRule(input);
  const at = clean.id ? list.findIndex((r) => r.id === clean.id) : -1;
  if (at >= 0) {
    list[at] = { ...clean, id: list[at].id } as Rule;
  } else {
    if (list.length >= LIMITS.rules) throw new Error(E.tooManyRules);
    let n = 1;
    while (list.some((r) => r.id === `rule-${n}`)) n++;
    list.push({ ...clean, id: `rule-${n}` } as Rule);
  }
  writeLocal(RULES_DEMO_KEY, list);
  return clone(list);
}

export async function removeRule(id: string): Promise<Rule[]> {
  if (isTauri()) return call<Rule[]>('organizer_remove_rule', { id });
  demoRuleList = demoRules().filter((r) => r.id !== id);
  writeLocal(RULES_DEMO_KEY, demoRuleList);
  return clone(demoRuleList);
}

/** Step 1. Lists the exact moves (empty `ruleIds` = every enabled rule). Only reads. */
export async function preview(ruleIds: string[] = []): Promise<Preview> {
  if (isTauri()) return call<Preview>('organizer_preview', { ruleIds });
  const chosen = demoRules().filter((r) => ruleIds.length === 0 || ruleIds.includes(r.id));
  const p = demoPlan(chosen, demoFileList(), Date.now(), ++demoSeq);
  demoStored = p;
  return clone({ ...p, demo: true });
}

/**
 * Step 3, after the user confirmed. Only the id of the preview that was shown is sent; the backend
 * re-checks every file before moving it. The preview is spent by this call.
 */
export async function run(previewId: number): Promise<RunReport> {
  if (isTauri()) {
    const r = await call<RunReport>('organizer_run', { previewId });
    return { ...r, moved: r.moved ?? [], failed: r.failed ?? [] };
  }
  return demoRun(previewId);
}

export async function undoInfo(): Promise<UndoInfo | null> {
  if (isTauri()) return call<UndoInfo | null>('organizer_undo_info');
  return demoUndoInfo();
}

/** Put back the last run's files, but only the ones nothing has touched since. */
export async function undo(): Promise<UndoReport> {
  if (isTauri()) {
    const r = await call<UndoReport>('organizer_undo');
    return { ...r, failed: r.failed ?? [] };
  }
  return demoUndo();
}

// ---------------------------------------------------------------------------------------
// Browser demo: made-up files, labelled DEMO everywhere
// ---------------------------------------------------------------------------------------

export const DEMO_ROOT = 'C:\\DEMO\\Users\\demo\\';
const RULES_DEMO_KEY = 'krovi.extras.rules.demo';
const DAY_MS = 86_400_000;
const MIN_QUIET_MS = 120_000;

export const demoFolderPath = (id: FolderId): string => `${DEMO_ROOT}${FOLDER_LABEL[id]}`;

interface DemoFile {
  name: string;
  folder: FolderId;
  /** '' = the top of the folder */
  sub: string;
  bytes: number;
  modifiedMs: number;
}

const demoPath = (f: { folder: FolderId; sub: string }, name: string): string =>
  `${demoFolderPath(f.folder)}${f.sub ? `\\${f.sub}` : ''}\\${name}`;

/** Made-up starter rules for the preview. The real app starts with none. */
export const DEMO_RULES: Rule[] = [
  { id: 'rule-1', enabled: true, folder: 'downloads', extensions: ['pdf'], nameContains: '', olderThanDays: 7, destFolder: 'documents', destSubfolder: 'PDFs' },
  { id: 'rule-2', enabled: true, folder: 'downloads', extensions: ['jpg', 'png'], nameContains: '', olderThanDays: 14, destFolder: 'pictures', destSubfolder: 'Downloaded' },
];

function seedFiles(now: number): DemoFile[] {
  const d = (folder: FolderId, sub: string, name: string, kb: number, ageDays: number): DemoFile => ({
    name,
    folder,
    sub,
    bytes: Math.round(kb * 1024),
    modifiedMs: now - ageDays * DAY_MS,
  });
  return [
    d('downloads', '', 'invoice-march.pdf', 220, 40),
    d('downloads', '', 'invoice-april.pdf', 180, 12),
    d('downloads', '', 'receipt-store.pdf', 90, 3),
    d('downloads', '', 'holiday-photo.jpg', 3400, 30),
    d('downloads', '', 'screenshot.png', 410, 5),
    d('downloads', '', 'DemoSetup-2.1.0.exe', 91_000, 60),
    d('downloads', '', 'notes-2024.docx', 55, 20),
    d('downloads', '', 'movie.mkv.crdownload', 120_000, 30),
    // already in the destination, so "invoice-march.pdf" shows the "name (2)" rule
    d('documents', 'PDFs', 'invoice-march.pdf', 200, 90),
    d('documents', '', 'thesis-draft.docx', 310, 2),
  ];
}

let demoRuleList: Rule[] | null = null;
let demoFiles: DemoFile[] | null = null;
let demoSeq = 0;
let demoStored: Preview | null = null;
let demoJournal: { runId: number; atMs: number; moves: Moved[] } | null = null;

function readLocal<T>(key: string): T | null {
  try {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem(key) : null;
    return raw ? (JSON.parse(raw) as T) : null;
  } catch {
    return null;
  }
}

function writeLocal(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* blocked storage: the preview forgets on reload */
  }
}

function demoRules(): Rule[] {
  if (!demoRuleList) demoRuleList = readLocal<Rule[]>(RULES_DEMO_KEY) ?? clone(DEMO_RULES);
  return demoRuleList;
}

function demoFileList(): DemoFile[] {
  if (!demoFiles) demoFiles = seedFiles(Date.now());
  return demoFiles;
}

const lc = (s: string): string => s.toLowerCase();

/** `name`, `name (2).ext`, ... : the first one that is neither on the demo disk nor already promised. */
function freeName(files: readonly DemoFile[], folder: FolderId, sub: string, name: string, reserved: Set<string>): string | null {
  for (let n = 1; n <= 99; n++) {
    const dot = name.lastIndexOf('.');
    const cand = n === 1 ? name : dot > 0 ? `${name.slice(0, dot)} (${n})${name.slice(dot)}` : `${name} (${n})`;
    const key = lc(demoPath({ folder, sub }, cand));
    if (reserved.has(key)) continue;
    if (files.some((f) => f.folder === folder && f.sub === sub && lc(f.name) === lc(cand))) continue;
    return cand;
  }
  return null;
}

/** A TypeScript copy of `plan_moves` over the made-up files: same conditions, same safety skips. */
export function demoPlan(rulesIn: readonly Rule[], files: readonly DemoFile[], now: number, previewId: number): Preview {
  const moves: PlannedMove[] = [];
  const notes: string[] = [];
  const claimed = new Set<DemoFile>();
  const reserved = new Set<string>();
  let recent = 0;
  let checked = 0;
  for (const rule of rulesIn.filter((r) => r.enabled)) {
    checked++;
    const segs = cleanSubfolder(rule.destSubfolder).segments;
    if (!segs.length) continue;
    const sub = segs.join('\\');
    const here = files.filter((f) => f.folder === rule.folder && f.sub === '').sort((a, b) => lc(a.name).localeCompare(lc(b.name)));
    for (const f of here) {
      if (!nameMatches(rule, f.name) || !ageMatches(rule, f.modifiedMs, now)) continue;
      if (/\.(crdownload|part|partial|download|tmp|opdownload)$/i.test(f.name) || now - f.modifiedMs < MIN_QUIET_MS) {
        recent++;
        continue;
      }
      if (claimed.has(f)) continue;
      const toName = freeName(files, rule.destFolder, sub, f.name, reserved);
      if (!toName) continue;
      if (moves.length >= LIMITS.moves) break;
      claimed.add(f);
      reserved.add(lc(demoPath({ folder: rule.destFolder, sub }, toName)));
      moves.push({
        id: `m${moves.length + 1}`,
        ruleId: rule.id,
        srcFolder: rule.folder,
        destFolder: rule.destFolder,
        destSub: sub,
        from: demoPath(f, f.name),
        toDir: `${demoFolderPath(rule.destFolder)}\\${sub}`,
        toName,
        size: f.bytes,
        modifiedMs: f.modifiedMs,
      });
    }
  }
  if (recent > 0) {
    notes.push(`${plural(recent, 'matching file')} changed in the last two minutes or still ${recent === 1 ? 'looks' : 'look'} like unfinished downloads, so ${recent === 1 ? 'it was' : 'they were'} left alone.`);
  }
  return { previewId, moves, notes, truncated: false, createdMs: now, rulesChecked: checked, demo: true };
}

function demoRun(previewId: number): RunReport {
  const p = demoStored;
  if (!p) throw new Error('There is no preview to run. Preview first.');
  if (p.previewId !== previewId) throw new Error('This preview is out of date. Preview again before running.');
  demoStored = null; // spent
  const files = demoFileList();
  const report: RunReport = { moved: [], failed: [], alreadyGone: 0, demo: true };
  for (const m of p.moves) {
    const src = files.find((f) => f.folder === m.srcFolder && f.sub === '' && demoPath(f, f.name) === m.from);
    if (!src) {
      report.alreadyGone++;
      continue;
    }
    // the same "never replace" rule as the real thing, checked at the moment of the move
    const taken = files.some((f) => f !== src && f.folder === m.destFolder && f.sub === m.destSub && lc(f.name) === lc(m.toName));
    const finalName = taken ? freeName(files, m.destFolder, m.destSub, src.name, new Set()) : m.toName;
    if (!finalName) {
      report.failed.push({ path: m.from, reason: 'A file with that name already exists.' });
      continue;
    }
    src.folder = m.destFolder;
    src.sub = m.destSub;
    src.name = finalName;
    report.moved.push({
      from: m.from,
      to: demoPath(src, finalName),
      size: m.size,
      modifiedMs: m.modifiedMs,
      srcFolder: m.srcFolder,
      destFolder: m.destFolder,
    });
  }
  demoJournal = report.moved.length ? { runId: previewId, atMs: Date.now(), moves: report.moved.map((x) => ({ ...x })) } : demoJournal;
  return clone(report);
}

function demoUndoInfo(): UndoInfo | null {
  return demoJournal ? { runId: demoJournal.runId, atMs: demoJournal.atMs, count: demoJournal.moves.length } : null;
}

function demoUndo(): UndoReport {
  const j = demoJournal;
  if (!j) throw new Error('There is nothing to undo.');
  const files = demoFileList();
  const report: UndoReport = { restored: 0, failed: [], demo: true };
  for (const m of [...j.moves].reverse()) {
    const f = files.find((x) => demoPath(x, x.name) === m.to);
    if (!f) {
      report.failed.push({ path: m.to, reason: 'It is no longer where Krovi put it, so it was left alone.' });
      continue;
    }
    const origin = splitPath(m.from).name;
    if (files.some((x) => x !== f && x.folder === m.srcFolder && x.sub === '' && lc(x.name) === lc(origin))) {
      report.failed.push({ path: m.to, reason: 'Something with the original name is there now, so it was not moved back.' });
      continue;
    }
    f.folder = m.srcFolder;
    f.sub = '';
    f.name = origin;
    report.restored++;
  }
  demoJournal = null;
  return clone(report);
}

/** Test hook: forget the demo rules, files, preview and undo list. */
export const __test = {
  reset(): void {
    demoRuleList = null;
    demoFiles = null;
    demoSeq = 0;
    demoStored = null;
    demoJournal = null;
    try {
      localStorage.removeItem(RULES_DEMO_KEY);
    } catch {
      /* no storage */
    }
  },
  /** Replace the demo files (tests). */
  setFiles(files: DemoFile[]): void {
    demoFiles = files;
  },
};

export type { DemoFile };
