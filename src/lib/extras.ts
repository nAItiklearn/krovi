// Extras client, part 1: snippets, workflows and the Quick Launcher's app names.
//
//   * Tauri: thin wrappers over src-tauri/src/extras.rs (commands snippets_*, workflows_*,
//     extras_apps). The rules (validation, uniqueness, placeholders, the clipboard write) live in the
//     Rust crate `krovi-extras`. There is NO fixture fallback on this path, ever: an empty list is an
//     empty list.
//   * Browser demo (`isTauri() === false`): a small working copy in localStorage, seeded with a
//     couple of clearly made-up entries, and no command is ever invoked. Nothing real is read or
//     changed; copying a snippet only touches the browser's own clipboard.
//
// Honest by construction: snippets are used on purpose (Enter in the palette, or the Copy button).
// Krovi has no keyboard hook, does not watch typing and never expands anything by itself.

import { match } from '../components/search';
import { invoke, isTauri } from './bridge';

// ---------------------------------------------------------------------------------------
// Types (wire shapes: src-tauri/crates/krovi-extras, camelCase)
// ---------------------------------------------------------------------------------------

export interface Snippet {
  id: string;
  name: string;
  /** Optional keyword that finds the snippet in the palette, e.g. ";sig". */
  trigger: string;
  body: string;
}

/** What the form sends; the backend assigns the id on first save. */
export interface SnippetInput {
  id?: string;
  name: string;
  trigger: string;
  body: string;
}

export interface SnippetUse {
  /** Characters copied. */
  chars: number;
  /** Plain sentences the user should read (an empty or private clipboard, ...). */
  notes: string[];
  /** client-side only: the preview, not the real clipboard flow */
  demo?: boolean;
}

export interface Workflow {
  id: string;
  name: string;
  /** Agent Mode command lines, run in order. */
  lines: string[];
}

export interface WorkflowInput {
  id?: string;
  name: string;
  lines: string[];
}

export const LIMITS = {
  snippets: 200,
  snippetName: 60,
  snippetTrigger: 24,
  snippetBody: 8000,
  clipboardChars: 20_000,
  workflows: 30,
  workflowName: 60,
  workflowLines: 12,
  workflowLine: 300,
} as const;

/** The placeholders a snippet may use. Anything else in braces is plain text. */
export const PLACEHOLDERS = ['{date}', '{time}', '{clipboard}'] as const;

// ---------------------------------------------------------------------------------------
// Pure helpers (unit-tested; they mirror krovi-extras, which is the authority)
// ---------------------------------------------------------------------------------------

// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;
// eslint-disable-next-line no-control-regex
const BODY_CONTROL = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/;

const collapse = (s: string): string => s.split(/\s+/).filter(Boolean).join(' ');
const lower = (s: string): string => s.toLowerCase();

/** Tidy a snippet the way the backend does: name spacing, keyword trimmed, line endings `\n`. */
export function normalizeSnippet(i: SnippetInput): SnippetInput {
  return {
    ...(i.id ? { id: i.id } : {}),
    name: collapse(i.name),
    trigger: i.trigger.trim(),
    body: i.body.replace(/\r\n?/g, '\n'),
  };
}

/** The sentence the backend would answer with, or `null` if the snippet is fine. */
export function validateSnippet(input: SnippetInput, others: readonly Snippet[]): string | null {
  const s = normalizeSnippet(input);
  if (!s.name) return 'Give the snippet a name.';
  if (CONTROL.test(input.name)) return 'The name cannot contain control characters.';
  if ([...s.name].length > LIMITS.snippetName) return `That name is too long (over ${LIMITS.snippetName} characters).`;
  if ([...s.trigger].length > LIMITS.snippetTrigger) return `That keyword is too long (over ${LIMITS.snippetTrigger} characters).`;
  if (/\s/.test(s.trigger) || CONTROL.test(s.trigger)) return 'A keyword cannot contain spaces or control characters.';
  if (!s.body.trim()) return 'The snippet has no text.';
  if (BODY_CONTROL.test(s.body)) return 'The text contains a control character Krovi cannot store.';
  if ([...s.body].length > LIMITS.snippetBody) return `That text is too long (over ${LIMITS.snippetBody} characters).`;
  for (const o of others) {
    if (o.id === input.id) continue;
    if (lower(o.name) === lower(s.name)) return 'A snippet with that name already exists.';
    if (s.trigger && lower(o.trigger) === lower(s.trigger)) return 'Another snippet already uses that keyword.';
  }
  return null;
}

const pad2 = (n: number): string => String(n).padStart(2, '0');

/** `2026-11-03` and `14:05` for this PC's local time. */
export function localDateTime(now: Date): { date: string; time: string } {
  return {
    date: `${String(now.getFullYear()).padStart(4, '0')}-${pad2(now.getMonth() + 1)}-${pad2(now.getDate())}`,
    time: `${pad2(now.getHours())}:${pad2(now.getMinutes())}`,
  };
}

export function usesClipboard(body: string): boolean {
  return body.includes('{clipboard}');
}

export interface Expanded {
  text: string;
  /** `{clipboard}` was used but there was no text on the clipboard. */
  clipboardEmpty: boolean;
  clipboardTruncated: boolean;
}

/**
 * Fill in `{date}`, `{time}` and `{clipboard}`. One pass: a substituted value is not scanned
 * again, so a clipboard that contains `{date}` stays as it is. Same result as the Rust `expand`.
 */
export function expandSnippet(body: string, ctx: { now: Date; clipboard: string | null }): Expanded {
  const { date, time } = localDateTime(ctx.now);
  let clipboardEmpty = false;
  let clipboardTruncated = false;
  const text = body.replace(/\{(date|time|clipboard)\}/g, (_all, key: string) => {
    if (key === 'date') return date;
    if (key === 'time') return time;
    const clip = ctx.clipboard;
    if (!clip) {
      clipboardEmpty = true;
      return '';
    }
    const chars = [...clip];
    if (chars.length > LIMITS.clipboardChars) {
      clipboardTruncated = true;
      return chars.slice(0, LIMITS.clipboardChars).join('');
    }
    return clip;
  });
  return { text, clipboardEmpty, clipboardTruncated };
}

/** One tidy line of a snippet's text, for lists. */
export function snippetPreview(body: string, max = 90): string {
  const one = body.replace(/\s+/g, ' ').trim();
  const chars = [...one];
  return chars.length > max ? `${chars.slice(0, max - 1).join('')}…` : one;
}

export interface SnippetHit {
  snippet: Snippet;
  score: number;
  /** [start, end) ranges inside `snippet.name` to highlight */
  ranges: [number, number][];
}

/**
 * Search snippets. With nothing typed, every snippet in the order given. Otherwise: the exact
 * keyword first, then a keyword that starts with the text, then the name (best match first), then
 * words found anywhere in the text. Ties keep the order given.
 */
export function rankSnippets(query: string, list: readonly Snippet[]): SnippetHit[] {
  const q = query.trim();
  if (!q) return list.map((snippet) => ({ snippet, score: 0, ranges: [] }));
  const ql = lower(q);
  const out: SnippetHit[] = [];
  for (const snippet of list) {
    const trig = lower(snippet.trigger);
    if (trig && trig === ql) {
      out.push({ snippet, score: 300, ranges: [] });
      continue;
    }
    if (trig && trig.startsWith(ql)) {
      out.push({ snippet, score: 200, ranges: [] });
      continue;
    }
    const m = match(q, snippet.name, [snippet.trigger, snippet.body]);
    if (m) out.push({ snippet, score: m.score, ranges: m.ranges });
  }
  return out
    .map((h, i) => ({ h, i }))
    .sort((a, b) => b.h.score - a.h.score || a.i - b.i)
    .map((x) => x.h);
}

/** A textarea's lines as workflow lines: trimmed, inner spacing collapsed, blank lines dropped. */
export function parseWorkflowLines(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map(collapse)
    .filter((l) => l !== '');
}

/** The sentence the backend would answer with, or `null` if the workflow is fine. */
export function validateWorkflow(input: WorkflowInput, others: readonly Workflow[]): string | null {
  const name = collapse(input.name);
  if (!name) return 'Give the workflow a name.';
  if (CONTROL.test(input.name)) return 'The name cannot contain control characters.';
  if ([...name].length > LIMITS.workflowName) return `That name is too long (over ${LIMITS.workflowName} characters).`;
  const lines = input.lines.map(collapse).filter((l) => l !== '');
  if (input.lines.some((l) => /[\u0000-\u0008\u000e-\u001f\u007f-\u009f]/.test(l))) return 'A line contains a control character.';
  if (lines.length === 0) return 'Add at least one command line.';
  if (lines.length > LIMITS.workflowLines) return `A workflow can have at most ${LIMITS.workflowLines} lines.`;
  if (lines.some((l) => [...l].length > LIMITS.workflowLine)) return `A line is too long (over ${LIMITS.workflowLine} characters).`;
  for (const o of others) {
    if (o.id !== input.id && lower(o.name) === lower(name)) return 'A workflow with that name already exists.';
  }
  return null;
}

/** Palette-sized id for a stored thing, same scheme as the backend (`sn-`/`wf-` + slug). */
function slug(name: string, fallback: string): string {
  const s = lower(name)
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 32)
    .replace(/-+$/g, '');
  return s || fallback;
}

function freshId(prefix: string, name: string, fallback: string, taken: readonly { id: string }[]): string {
  const base = `${prefix}-${slug(name, fallback)}`;
  let id = base;
  for (let n = 2; taken.some((t) => t.id === id); n++) id = `${base}-${n}`;
  return id;
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

// ---------------------------------------------------------------------------------------
// Browser demo: a working copy in localStorage, clearly made up
// ---------------------------------------------------------------------------------------

const SNIPPETS_DEMO_KEY = 'krovi.extras.snippets.demo';
const WORKFLOWS_DEMO_KEY = 'krovi.extras.workflows.demo';

/** Made-up starter entries for the preview. The real app starts empty. */
export const DEMO_SNIPPETS: Snippet[] = [
  { id: 'sn-email-sign-off', name: 'Email sign-off', trigger: ';sig', body: 'Thanks,\nAlex Demo' },
  { id: 'sn-meeting-notes', name: 'Meeting notes header', trigger: ';mtg', body: 'Meeting notes: {date} at {time}\nAttendees:\nDecisions:\nNext steps:' },
  { id: 'sn-quote-clipboard', name: 'Quote what I copied', trigger: ';quote', body: '> {clipboard}\n\n(copied on {date})' },
];

export const DEMO_WORKFLOWS: Workflow[] = [
  { id: 'wf-focus-time', name: 'Focus time', lines: ['open spotify', 'set a timer for 25 minutes', 'open notion.so'] },
];

export const DEMO_APPS = ['Notepad', 'Calculator', 'Paint', 'Spotify'];

let demoSnippetList: Snippet[] | null = null;
let demoWorkflowList: Workflow[] | null = null;

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

function demoSnippets(): Snippet[] {
  if (!demoSnippetList) demoSnippetList = readLocal<Snippet[]>(SNIPPETS_DEMO_KEY) ?? clone(DEMO_SNIPPETS);
  return demoSnippetList;
}

function demoWorkflows(): Workflow[] {
  if (!demoWorkflowList) demoWorkflowList = readLocal<Workflow[]>(WORKFLOWS_DEMO_KEY) ?? clone(DEMO_WORKFLOWS);
  return demoWorkflowList;
}

/** Test hook: forget the demo working copies. */
export function __resetDemo(): void {
  demoSnippetList = null;
  demoWorkflowList = null;
  try {
    localStorage.removeItem(SNIPPETS_DEMO_KEY);
    localStorage.removeItem(WORKFLOWS_DEMO_KEY);
  } catch {
    /* no storage */
  }
}

// ---------------------------------------------------------------------------------------
// The bridge
// ---------------------------------------------------------------------------------------

/** The Start-menu app names for the palette (Agent Mode's own list; names only). */
export async function launcherApps(): Promise<string[]> {
  if (isTauri()) return call<string[]>('extras_apps');
  return [...DEMO_APPS];
}

export const snippets = {
  async list(): Promise<Snippet[]> {
    if (isTauri()) return call<Snippet[]>('snippets_list');
    return clone(demoSnippets());
  },

  /** Add a snippet, or update the one with the same id. Resolves to the whole list. */
  async save(input: SnippetInput): Promise<Snippet[]> {
    if (isTauri()) return call<Snippet[]>('snippets_save', { snippet: input });
    const list = demoSnippets();
    const why = validateSnippet(input, list);
    if (why) throw new Error(why);
    const clean = normalizeSnippet(input);
    const at = clean.id ? list.findIndex((s) => s.id === clean.id) : -1;
    if (at >= 0) {
      list[at] = { id: list[at].id, name: clean.name, trigger: clean.trigger, body: clean.body };
    } else {
      if (list.length >= LIMITS.snippets) throw new Error(`At most ${LIMITS.snippets} snippets can be saved.`);
      list.push({ id: freshId('sn', clean.name, 'snippet', list), name: clean.name, trigger: clean.trigger, body: clean.body });
    }
    writeLocal(SNIPPETS_DEMO_KEY, list);
    return clone(list);
  },

  async remove(id: string): Promise<Snippet[]> {
    if (isTauri()) return call<Snippet[]>('snippets_remove', { id });
    demoSnippetList = demoSnippets().filter((s) => s.id !== id);
    writeLocal(SNIPPETS_DEMO_KEY, demoSnippetList);
    return clone(demoSnippetList);
  },

  /** Fill in the placeholders and copy the snippet to the clipboard. Nothing else happens. */
  async use(id: string): Promise<SnippetUse> {
    if (isTauri()) return call<SnippetUse>('snippets_use', { id });
    const s = demoSnippets().find((x) => x.id === id);
    if (!s) throw new Error('That snippet no longer exists.');
    // The preview never reads the clipboard (the browser would ask permission for it).
    const out = expandSnippet(s.body, { now: new Date(), clipboard: null });
    const notes: string[] = usesClipboard(s.body) ? ['DEMO: the preview does not read your clipboard, so {clipboard} became empty.'] : [];
    try {
      await navigator.clipboard.writeText(out.text);
    } catch {
      notes.push('Your browser did not allow copying.');
    }
    return { chars: [...out.text].length, notes, demo: true };
  },
};

export const workflows = {
  async list(): Promise<Workflow[]> {
    if (isTauri()) return call<Workflow[]>('workflows_list');
    return clone(demoWorkflows());
  },

  async save(input: WorkflowInput): Promise<Workflow[]> {
    if (isTauri()) return call<Workflow[]>('workflows_save', { workflow: input });
    const list = demoWorkflows();
    const why = validateWorkflow(input, list);
    if (why) throw new Error(why);
    const name = collapse(input.name);
    const lines = input.lines.map(collapse).filter((l) => l !== '');
    const at = input.id ? list.findIndex((w) => w.id === input.id) : -1;
    if (at >= 0) {
      list[at] = { id: list[at].id, name, lines };
    } else {
      if (list.length >= LIMITS.workflows) throw new Error(`At most ${LIMITS.workflows} workflows can be saved.`);
      list.push({ id: freshId('wf', name, 'workflow', list), name, lines });
    }
    writeLocal(WORKFLOWS_DEMO_KEY, list);
    return clone(list);
  },

  async remove(id: string): Promise<Workflow[]> {
    if (isTauri()) return call<Workflow[]>('workflows_remove', { id });
    demoWorkflowList = demoWorkflows().filter((w) => w.id !== id);
    writeLocal(WORKFLOWS_DEMO_KEY, demoWorkflowList);
    return clone(demoWorkflowList);
  },
};
