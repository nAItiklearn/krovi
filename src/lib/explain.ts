// Explain client (owner: explain) - docs/CONTRACTS.md section 10.
//
// Paste any text -> an on-device summary, and read it aloud.
//   * Tauri: thin wrappers over src-tauri/src/explain.rs (commands explain_*, event `explain-speak`).
//     The summary is computed in Rust on this PC; reading aloud uses a Windows voice (on-device)
//     or, only if the user saved their own key, ElevenLabs.
//   * Browser demo (`isTauri() === false`): the SAME summarising algorithm runs here in TypeScript
//     (a port of krovi-explain's summary.rs), and reading aloud uses the browser's own speech
//     synthesis. ElevenLabs is simulated and says so; nothing is sent anywhere.
//
// Keys never live in this file or in localStorage: in the app they are in Windows Credential
// Manager and the webview only ever learns "saved" and the last four characters.

import { writable, type Readable } from 'svelte/store';
import { invoke, isTauri, listen } from './bridge';

// ---------------------------------------------------------------------------------------
// Types (wire shapes: src-tauri/src/explain.rs, all camelCase)
// ---------------------------------------------------------------------------------------

export type SummaryLength = 'short' | 'medium' | 'points';
export type VoiceEngine = 'windows' | 'elevenlabs';

export interface SummaryItem {
  index: number;
  text: string;
}
export interface SummaryStats {
  chars: number;
  words: number;
  sentences: number;
  summaryWords: number;
  summarySentences: number;
  readSecs: number;
  summaryReadSecs: number;
  savedSecs: number;
  ratio: number;
}
export interface Summary {
  length: SummaryLength;
  text: string;
  items: SummaryItem[];
  alreadyShort: boolean;
  note: string | null;
  stats: SummaryStats;
}

export interface ExplainSettings {
  summaryLength: SummaryLength;
  voiceEngine: VoiceEngine;
  windowsVoice: string | null;
  rate: number;
  elevenVoiceId: string | null;
  elevenVoiceName: string | null;
  elevenModel: string;
  elevenConsented: boolean;
}
export interface WinVoice {
  id: string;
  name: string;
  language: string;
  gender: string;
  natural: boolean;
}
export interface ElevenVoice {
  id: string;
  name: string;
  category: string | null;
  detail: string;
}
export interface ElevenModel {
  id: string;
  label: string;
  creditsPerChar: number;
}
export interface Engines {
  windows: { available: boolean; voices: WinVoice[]; defaultVoiceId: string | null; error: string | null };
  elevenlabs: { hasKey: boolean; keyHint: string | null; models: ElevenModel[]; keysUrl: string; error: string | null };
  settings: ExplainSettings;
}
export interface KeyResult {
  saved: boolean;
  hasKey: boolean;
  keyHint: string | null;
  voices: ElevenVoice[];
  message: string;
  tone: 'ok' | 'warning' | 'error';
  settings: ExplainSettings;
}
export interface TestResult {
  ok: boolean;
  kind: string | null;
  message: string;
  ms: number;
}
export interface SpeakInfo {
  engine: VoiceEngine;
  sentences: number;
  chars: number;
  credits: number | null;
}

export type SpeakEvent =
  | { state: 'speaking'; index: number; count: number; start: number; end: number; engine: string; voice: string }
  | { state: 'notice'; message: string }
  | { state: 'done'; ms: number }
  | { state: 'failed'; message: string }
  | { state: 'stopped' };

export const MAX_CHARS = 400_000;
export const RATE_MIN = 0.6;
export const RATE_MAX = 1.8;

export const DEFAULT_SETTINGS: ExplainSettings = {
  summaryLength: 'medium',
  voiceEngine: 'windows',
  windowsVoice: null,
  rate: 1,
  elevenVoiceId: null,
  elevenVoiceName: null,
  elevenModel: 'eleven_flash_v2_5',
  elevenConsented: false,
};

export const ELEVEN_MODELS: ElevenModel[] = [
  { id: 'eleven_flash_v2_5', label: 'Flash v2.5 - fast, half the credits', creditsPerChar: 0.5 },
  { id: 'eleven_multilingual_v2', label: 'Multilingual v2 - richest quality', creditsPerChar: 1 },
];
export const ELEVEN_KEYS_URL = 'https://elevenlabs.io/app/settings/api-keys';
/** The fixed sentence "Test voice" speaks (the backend has the same one; the user's text is never used for a test). */
export const TEST_SENTENCE_LENGTH = 'Hello from Krovi. This is how your chosen voice sounds.'.length;

export const LENGTHS: { id: SummaryLength; label: string; hint: string }[] = [
  { id: 'short', label: 'Short', hint: 'About two sentences' },
  { id: 'medium', label: 'Medium', hint: 'A short paragraph' },
  { id: 'points', label: 'Key points', hint: 'Bullet points' },
];

// ---------------------------------------------------------------------------------------
// Small pure helpers (unit-tested)
// ---------------------------------------------------------------------------------------

/** `45 s`, `2 min 10 s`, `1 h 05 min`. Never negative. */
export function formatDuration(secs: number): string {
  const s = Math.max(0, Math.round(secs));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) {
    const r = s % 60;
    return r ? `${m} min ${r} s` : `${m} min`;
  }
  const h = Math.floor(m / 60);
  return `${h} h ${String(m % 60).padStart(2, '0')} min`;
}

export function countText(text: string): { chars: number; words: number } {
  const t = text.trim();
  if (!t) return { chars: 0, words: 0 };
  return { chars: [...text].length, words: wordCount(text) };
}

export function formatNumber(n: number): string {
  return n.toLocaleString('en-US');
}

/** "Saves about 38 s of reading", or '' when nothing was saved. */
export function savedLabel(stats: SummaryStats): string {
  if (stats.savedSecs < 5) return '';
  return `Saves about ${formatDuration(stats.savedSecs)} of reading`;
}

export function creditsFor(chars: number, modelId: string, models: ElevenModel[] = ELEVEN_MODELS): number {
  const per = models.find((m) => m.id === modelId)?.creditsPerChar ?? 1;
  return Math.ceil(chars * per);
}

/** The text split around the range being spoken (clamped; empty `mid` when nothing is spoken). */
export function highlightParts(text: string, range: { start: number; end: number } | null): { before: string; mid: string; after: string } {
  if (!range || range.end <= range.start) return { before: text, mid: '', after: '' };
  const a = Math.max(0, Math.min(text.length, range.start));
  const b = Math.max(a, Math.min(text.length, range.end));
  return { before: text.slice(0, a), mid: text.slice(a, b), after: text.slice(b) };
}

/** The body saved to Notes: a first line that names the source, then the summary. */
export function noteBody(source: string, summary: string): string {
  const first = source.trim().replace(/\s+/g, ' ');
  const head = first.length > 60 ? `${first.slice(0, 57).trimEnd()}...` : first;
  return `Summary of: ${head}\n\n${summary.trim()}`;
}

/** "Microsoft David - English (United States)" -> "Microsoft David"; a long name is cut with an ellipsis. */
export function shortVoiceName(name: string): string {
  const base = name.split(' - ')[0].trim();
  return base.length > 32 ? `${base.slice(0, 31).trimEnd()}…` : base;
}

export function errorText(e: unknown): string {
  return typeof e === 'string' ? e : e instanceof Error ? e.message : String(e);
}

// ---------------------------------------------------------------------------------------
// Sentence splitting + summariser (TypeScript port of krovi-explain's text.rs / summary.rs)
// ---------------------------------------------------------------------------------------

export interface Unit {
  text: string;
  /** UTF-16 offsets into the input */
  start: number;
  end: number;
  kind: 'sentence' | 'bullet' | 'heading';
  para: number;
  firstInPara: boolean;
}

interface Line {
  start: number;
  text: string;
  indent: number;
}

const NEVER = new Set(['mr', 'mrs', 'ms', 'mx', 'dr', 'prof', 'st', 'mt', 'gen', 'gov', 'sen', 'rep', 'lt', 'col', 'capt', 'sgt', 'cmdr', 'adm', 'hon', 'rev', 'mme', 'mlle', 'messrs', 'vs', 'v', 'cf', 'viz', 'ca', 'esp', 'incl', 'approx', 'est', 'dept', 'univ', 'assn', 'bros', 'ave', 'blvd', 'e.g', 'i.e', 'et', 'ph.d']);
const REFS = new Set(['no', 'nos', 'fig', 'figs', 'eq', 'eqs', 'sec', 'secs', 'art', 'arts', 'para', 'paras', 'ch', 'vol', 'vols', 'pp', 'p', 'pg', 'pt', 'ex', 'tab', 'cl', 'cls', 'ss']);
const SOFT = new Set(['inc', 'ltd', 'co', 'corp', 'llc', 'plc', 'jr', 'sr', 'al', 'u.s', 'u.k', 'a.m', 'p.m', 'b.c', 'a.d', 'd.c', 'u.n', 'e.u']);
const MONTHS = new Set(['jan', 'feb', 'mar', 'apr', 'jun', 'jul', 'aug', 'sep', 'sept', 'oct', 'nov', 'dec']);
const STARTERS = new Set(['the', 'a', 'an', 'it', 'its', 'this', 'that', 'these', 'those', 'we', 'they', 'he', 'she', 'i', 'in', 'on', 'at', 'for', 'to', 'if', 'as', 'but', 'however', 'there', 'then', 'when', 'after', 'before', 'our', 'their', 'his', 'her', 'you', 'your', 'one', 'some', 'all', 'many', 'most', 'such', 'so', 'thus', 'also', 'according', 'today', 'yesterday', 'meanwhile', 'although', 'while', 'since', 'because', 'with', 'by', 'from', 'under', 'both', 'each', 'no', 'not', 'what', 'who', 'how', 'why', 'where', 'which', 'my', 'us', 'now', 'here']);
const STOP = new Set('a about above after again against all also am an and any are arent as at be because been before being below between both but by can cannot cant could couldnt did didnt do does doesnt doing dont down during each either else etc even ever every few for from further had hadnt has hasnt have havent having he hed her here hers herself hes him himself his how however i id if ill im in into is isnt it its itself ive just let lets like may me might more most much must my myself neither no nor not now of off on once one only onto or other ought our ours ourselves out over own per rather said same say says shall she shed shes should shouldnt so some such than that thats the their theirs them themselves then there therefore theres these they theyd theyll theyre theyve this those through thus to too under until up upon us use used using very via was wasnt we wed well were weren werent weve what whats when where wheres whether which while who whom whose why will with within without wont would wouldnt yet you youd youll your youre yours yourself yourselves youve'.split(' '));

const isUpper = (c: string) => c !== c.toLowerCase();
const isLower = (c: string) => c !== c.toUpperCase();
const isAlpha = (c: string) => /\p{L}/u.test(c);
const isNumeric = (c: string) => /\p{N}/u.test(c);
const isAlnum = (c: string) => /[\p{L}\p{N}]/u.test(c);
const isSpace = (c: string) => /\s/u.test(c);
const isTerminator = (c: string) => '.!?…。！？'.includes(c);
const isCjkTerminator = (c: string) => '。！？'.includes(c);
const isCloser = (c: string) => '"\'”’)]}»」'.includes(c);
const isOpener = (c: string) => '"\'“‘([{«「'.includes(c);

export function wordCount(text: string): number {
  return text.split(/\s+/).filter((t) => /[\p{L}\p{N}]/u.test(t)).length;
}

export function collapseWs(text: string): string {
  return text.replace(/[\s­​]+/g, ' ').trim();
}

const ROMAN = new Set(['i', 'ii', 'iii', 'iv', 'v', 'vi', 'vii', 'viii', 'ix', 'x', 'xi', 'xii']);
function isListLabel(inner: string): boolean {
  if (!inner) return false;
  if (/^\d+$/.test(inner)) return inner.length <= 3;
  return (inner.length === 1 && /^[A-Za-z]$/.test(inner)) || ROMAN.has(inner.toLowerCase());
}

/** Length of a leading list marker plus its whitespace ("- ", "* ", "12. ", "(a) "), or 0. */
export function bulletMarkerLen(s: string): number {
  let m = /^[-*+•·–—▪►→✓●◦]\s+/u.exec(s);
  if (m) return m[0].length;
  m = /^(\d{1,3})[.)]\s+/.exec(s);
  if (m) return m[0].length;
  m = /^\(([^)\s]{1,4})\)\s+/.exec(s);
  if (m && isListLabel(m[1])) return m[0].length;
  m = /^([a-z]{1,4})\)\s+/.exec(s);
  if (m && isListLabel(m[1])) return m[0].length;
  return 0;
}

function paragraphs(input: string): Line[][] {
  const paras: Line[][] = [];
  let cur: Line[] = [];
  let off = 0;
  for (const raw of input.split('\n')) {
    const lineOff = off;
    off += raw.length + 1;
    const noCr = raw.endsWith('\r') ? raw.slice(0, -1) : raw;
    const lead = noCr.length - noCr.trimStart().length;
    const text = noCr.trim();
    if (!text) {
      if (cur.length) paras.push(cur);
      cur = [];
      continue;
    }
    cur.push({ start: lineOff + lead, text, indent: lead });
  }
  if (cur.length) paras.push(cur);
  return paras;
}

function isHeadingLine(lines: Line[], k: number): boolean {
  if (k !== 0) return false;
  const t = lines[0].text;
  if ([...t].length > 70 || wordCount(t) > 10 || wordCount(t) === 0) return false;
  const first = t[0];
  if (!(isUpper(first) || /\d/.test(first))) return false;
  if ('.!?…,;:"\'”’)]'.includes(t[t.length - 1])) return false;
  const next = lines[1];
  if (!next) return true;
  return bulletMarkerLen(next.text) > 0 || isUpper(next.text[0]) || /\d/.test(next.text[0]);
}

interface Block {
  kind: Unit['kind'];
  lines: Line[];
}

function blocksOf(lines: Line[]): Block[] {
  const out: Block[] = [];
  lines.forEach((line, k) => {
    const marker = bulletMarkerLen(line.text);
    if (marker > 0) {
      const rest = line.text.slice(marker).trimStart();
      out.push({ kind: 'bullet', lines: [{ start: line.start + (line.text.length - rest.length), text: rest, indent: line.indent }] });
      return;
    }
    if (isHeadingLine(lines, k)) {
      out.push({ kind: 'heading', lines: [line] });
      return;
    }
    const prev = out[out.length - 1];
    if (prev && prev.kind === 'bullet') {
      if (line.indent > 0 || isLower(line.text[0])) prev.lines.push(line);
      else out.push({ kind: 'sentence', lines: [line] });
    } else if (prev && prev.kind === 'sentence') {
      prev.lines.push(line);
    } else {
      out.push({ kind: 'sentence', lines: [line] });
    }
  });
  return out;
}

function wordBefore(s: string, i: number): string {
  let j = i;
  while (j > 0 && (isAlnum(s[j - 1]) || s[j - 1] === '.' || s[j - 1] === "'" || s[j - 1] === '’')) j--;
  return s.slice(j, i).replace(/^\.+/, '').toLowerCase();
}
function wordAfter(s: string, m: number): string {
  let j = m;
  while (j < s.length && isAlnum(s[j])) j++;
  return s.slice(m, j).toLowerCase();
}

function isBoundary(s: string, i: number, j: number, m: number): boolean {
  const term = s[i];
  const next = s[m];
  if (term === '?' || term === '!') return !isLower(next);
  if (isLower(next)) return false;
  const opens = isUpper(next) || isNumeric(next) || isOpener(next) || isAlpha(next);
  if (j > i || term === '…') return opens;
  if (!opens) return false;
  let st = i;
  while (st > 0 && !isSpace(s[st - 1])) st--;
  const token = s.slice(st, i).toLowerCase();
  if (token.includes('/') || token.includes('@') || token.startsWith('www.')) return true;
  const w = wordBefore(s, i);
  if (!w) return true;
  if (NEVER.has(w)) return false;
  if (REFS.has(w)) return !isNumeric(next);
  if ([...w].length === 1 && isAlpha(w)) return false;
  if (MONTHS.has(w)) return false;
  if (SOFT.has(w)) return STARTERS.has(wordAfter(s, m));
  // other dotted abbreviations (U.S, Ph.D, a.m); a dotted token with digits ("v2.0.1.") is a number
  if (w.includes('.') && !/\d/.test(w)) return STARTERS.has(wordAfter(s, m));
  return true;
}

function splitRanges(s: string): Array<[number, number]> {
  const n = s.length;
  const out: Array<[number, number]> = [];
  const push = (a: number, bIncl: number) => {
    if (a > bIncl || a >= n) return;
    const slice = s.slice(a, bIncl + 1);
    const lead = slice.length - slice.trimStart().length;
    const t = slice.trim();
    if (t) out.push([a + lead, a + lead + t.length]);
  };
  let start = 0;
  let i = 0;
  while (i < n) {
    if (!isTerminator(s[i])) {
      i++;
      continue;
    }
    let j = i;
    while (j + 1 < n && isTerminator(s[j + 1])) j++;
    let k = j;
    while (k + 1 < n && isCloser(s[k + 1])) k++;
    if (k + 1 >= n) {
      push(start, k);
      start = n;
      break;
    }
    if (isCjkTerminator(s[j])) {
      let m = k + 1;
      while (m < n && isSpace(s[m])) m++;
      push(start, k);
      start = m;
      i = m;
      continue;
    }
    if (isSpace(s[k + 1])) {
      let m = k + 1;
      while (m < n && isSpace(s[m])) m++;
      if (m >= n) {
        push(start, k);
        start = n;
        break;
      }
      if (isBoundary(s, i, j, m)) {
        push(start, k);
        start = m;
        i = m;
        continue;
      }
    }
    i = k + 1;
  }
  if (start < n) push(start, n - 1);
  return out;
}

/** Sentences, bullets and headings of `input`, with offsets back into it. */
export function splitUnits(input: string): Unit[] {
  const units: Unit[] = [];
  paragraphs(input).forEach((lines, pi) => {
    let first = true;
    for (const block of blocksOf(lines)) {
      // re-flow the lines of the block, remembering where each sits in the joined string
      let joined = '';
      const segs: Array<{ j: number; o: number; len: number }> = [];
      block.lines.forEach((l, idx) => {
        if (idx > 0) {
          const wrapped = joined.endsWith('-') && /\p{L}/u.test(joined[joined.length - 2] ?? '') && isLower(l.text[0]);
          if (!wrapped) joined += ' ';
        }
        segs.push({ j: joined.length, o: l.start, len: l.text.length });
        joined += l.text;
      });
      const toStart = (x: number) => {
        for (const g of segs) {
          if (x >= g.j && x < g.j + g.len) return g.o + (x - g.j);
          if (x < g.j) return g.o;
        }
        const last = segs[segs.length - 1];
        return last.o + last.len;
      };
      const toEnd = (x: number) => {
        for (const g of segs) {
          if (x > g.j && x <= g.j + g.len) return g.o + (x - g.j);
          if (x <= g.j) return g.o;
        }
        const last = segs[segs.length - 1];
        return last.o + last.len;
      };
      const ranges: Array<[number, number]> = block.kind === 'heading' ? [[0, joined.length]] : splitRanges(joined);
      for (const [a, b] of ranges) {
        units.push({ text: joined.slice(a, b), start: toStart(a), end: toEnd(b), kind: block.kind, para: pi, firstInPara: first });
        first = false;
      }
    }
  });
  return units;
}

function stem(w: string): string {
  const n = [...w].length;
  if (n <= 3 || /\d/.test(w)) return w;
  const undouble = (s: string) => {
    const a = s[s.length - 2];
    const c = s[s.length - 1];
    return a === c && !'lszaeiou'.includes(a) ? s.slice(0, -1) : s;
  };
  if (w.endsWith('ies') && n > 4) return `${w.slice(0, -3)}y`;
  if (w.endsWith('sses')) return w.slice(0, -2);
  if (w.endsWith('ing') && n > 5) return undouble(w.slice(0, -3));
  if (w.endsWith('ed') && n > 4) return undouble(w.slice(0, -2));
  if (w.endsWith('ly') && n > 5) return w.slice(0, -2);
  if (w.endsWith('s') && !w.endsWith('ss') && !w.endsWith('us') && !w.endsWith('is')) return w.slice(0, -1);
  return w;
}

export function contentTerms(text: string): string[] {
  const out: string[] = [];
  let word = '';
  const flush = () => {
    if (word && [...word].length >= 2 && !STOP.has(word)) out.push(stem(word));
    word = '';
  };
  for (const c of text) {
    if (isAlnum(c)) word += c.toLowerCase();
    else if ((c === "'" || c === '’') && word) continue;
    else flush();
  }
  flush();
  return out;
}

/** A sentence made speakable: list markers, markdown noise and bare URLs removed. */
export function speakable(text: string): string {
  let t = collapseWs(text);
  for (;;) {
    const cut = bulletMarkerLen(t);
    if (!cut) break;
    t = t.slice(cut).trimStart();
  }
  t = t.replace(/[*_`#]/g, '');
  return t
    .split(' ')
    .map((w) => {
      const bare = w.replace(/^[()[\]<>,;"]+|[()[\]<>,;"]+$/g, '').toLowerCase();
      if (bare.startsWith('http://') || bare.startsWith('https://') || bare.startsWith('www.')) {
        const trail = /[.,);!?]*$/.exec(w)?.[0] ?? '';
        return `link${trail}`;
      }
      return w;
    })
    .join(' ');
}

const READ_WPM = 238;
const SHORT_INPUT_WORDS = 45;
const MAX_CANDIDATES = 1200;

const readSeconds = (words: number) => (words / READ_WPM) * 60;

function dot(a: Array<[number, number]>, b: Array<[number, number]>): number {
  let i = 0;
  let j = 0;
  let s = 0;
  while (i < a.length && j < b.length) {
    if (a[i][0] < b[j][0]) i++;
    else if (a[i][0] > b[j][0]) j++;
    else {
      s += a[i][1] * b[j][1];
      i++;
      j++;
    }
  }
  return s;
}

function textRank(sim: Float32Array, m: number): number[] {
  const D = 0.85;
  if (m === 0) return [];
  const adj: Array<Array<[number, number]>> = Array.from({ length: m }, () => []);
  const outSum = new Array<number>(m).fill(0);
  for (let a = 0; a < m; a++) {
    for (let b = 0; b < m; b++) {
      const s = sim[a * m + b];
      if (a !== b && s >= 0.05) {
        adj[a].push([b, s]);
        outSum[a] += s;
      }
    }
  }
  let r = new Array<number>(m).fill(1 / m);
  for (let it = 0; it < 80; it++) {
    let dangling = 0;
    for (let j = 0; j < m; j++) if (outSum[j] === 0) dangling += r[j];
    const next = new Array<number>(m).fill((1 - D) / m + (D * dangling) / m);
    for (let a = 0; a < m; a++) {
      let acc = 0;
      for (const [j, w] of adj[a]) acc += (w / outSum[j]) * r[j];
      next[a] += D * acc;
    }
    let delta = 0;
    for (let a = 0; a < m; a++) delta += Math.abs(next[a] - r[a]);
    r = next;
    if (delta < 1e-6) break;
  }
  const max = Math.max(...r);
  return max > 0 ? r.map((x) => x / max) : r;
}

function positionPrior(p: number, total: number, firstInPara: boolean): number {
  const lead = 1 + 0.5 * Math.exp(-p / 2.5);
  return lead * (firstInPara ? 1.08 : 1) * (p + 1 === total && total > 3 ? 1.05 : 1);
}

function lengthPrior(words: number, length: SummaryLength): number {
  const [lo, hi, hard] = length === 'points' ? [8, 28, 70] : [9, 35, 90];
  if (words < 5) return 0.5;
  if (words < lo) return 0.6 + (0.4 * (words - 5)) / (lo - 5);
  if (words <= hi) return 1;
  if (words < hard) return 1 - (0.35 * (words - hi)) / (hard - hi);
  return 0.6;
}

const BOOST = ['in conclusion', 'in summary', 'to summarize', 'to summarise', 'in short', 'overall,', 'importantly', 'the main idea', 'the main point', 'the key point', 'the key idea', 'the purpose of', 'the goal of', 'this means', 'as a result', 'therefore', ' must ', 'shall not', 'is prohibited', 'are prohibited', 'is required', 'are required', 'in no event'];
const DOWN = ['for example', 'for instance', 'e.g.', 'such as', 'photo:', 'image:', 'caption:', 'source:'];
const JUNK = ['subscribe', 'click here', 'read more', 'sign up', 'advertisement', 'all rights reserved', 'share this', 'follow us'];

function cuePrior(sentence: string): number {
  const l = sentence.toLowerCase();
  let f = 1;
  if (BOOST.some((c) => l.includes(c))) f *= 1.1;
  if (DOWN.some((c) => l.startsWith(c) || l.includes(`, ${c}`))) f *= 0.88;
  if (JUNK.some((c) => l.includes(c))) f *= 0.4;
  if (l.trimEnd().endsWith('?')) f *= 0.85;
  if (l.startsWith('"') || l.startsWith('“')) f *= 0.92;
  if (/\d/.test(l)) {
    f *= 1.05;
    if (l.includes('$') || l.includes('%') || l.includes('percent') || l.includes('million') || l.includes('billion')) f *= 1.03;
  }
  return f;
}

function plan(length: SummaryLength, prose: number, words: number, cands: number): [number, number, number, number] {
  const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
  let k: number;
  let kMin: number;
  let budget: number;
  let lambda: number;
  if (length === 'short') {
    k = prose >= 5 ? 2 : 1;
    kMin = 1;
    budget = 70;
    lambda = 0.85;
  } else if (length === 'medium') {
    k = clamp(Math.round(prose * 0.2), 3, 7);
    kMin = 3;
    budget = clamp(Math.floor(words * 0.2), 90, 240);
    lambda = 0.7;
  } else {
    k = clamp(Math.round(prose * 0.15), 3, 8);
    kMin = 3;
    budget = clamp(Math.floor(words * 0.15), 70, 200);
    lambda = 0.65;
  }
  k = Math.max(1, Math.min(k, Math.max(0, cands - 1)));
  return [k, Math.min(kMin, k), budget, lambda];
}

function finish(length: SummaryLength, units: Unit[], chosen: number[], alreadyShort: boolean, chars: number, words: number, sentences: number): Summary {
  const items = chosen.map((i) => ({ index: i, text: collapseWs(units[i].text) }));
  const text = length === 'points' && !alreadyShort ? items.map((it) => `• ${it.text}`).join('\n') : items.map((it) => it.text).join(' ');
  const summaryWords = wordCount(text);
  const read = readSeconds(words);
  const sread = readSeconds(summaryWords);
  return {
    length,
    text,
    items,
    alreadyShort,
    note: alreadyShort ? 'This is already short, so here it is as written. Paste something longer to see a summary.' : null,
    stats: {
      chars,
      words,
      sentences,
      summaryWords,
      summarySentences: items.length,
      readSecs: read,
      summaryReadSecs: sread,
      savedSecs: Math.max(0, read - sread),
      ratio: words === 0 ? 1 : Math.min(1, summaryWords / words),
    },
  };
}

/** Throws an Error with a plain sentence when the input is empty or too long. */
export function summarizeLocal(input: string, length: SummaryLength): Summary {
  if (!input.trim()) throw new Error('Paste some text to summarize.');
  if (input.length > MAX_CHARS) throw new Error(`That is ${formatNumber(input.length)} characters; Krovi summarizes up to ${formatNumber(MAX_CHARS)} at a time. Paste a smaller part.`);

  const units = splitUnits(input);
  const wordsTotal = wordCount(input);
  const chars = [...input].length;
  let prose = units.map((u, i) => (u.kind !== 'heading' ? i : -1)).filter((i) => i >= 0);
  if (!prose.length) prose = units.map((_, i) => i);
  let cands = prose.filter((i) => wordCount(units[i].text) >= 4);
  if (cands.length < 3) cands = prose;
  if (wordsTotal < SHORT_INPUT_WORDS || cands.length < 3) return finish(length, units, prose, true, chars, wordsTotal, units.length);

  const pTotal = prose.length;
  const posOf = new Map(prose.map((u, p) => [u, p]));

  // TF-IDF
  const ids = new Map<string, number>();
  const df: number[] = [];
  const counts: Array<Array<[number, number]>> = cands.map((ui) => {
    const c = new Map<number, number>();
    for (const t of contentTerms(units[ui].text)) {
      let id = ids.get(t);
      if (id === undefined) {
        id = ids.size;
        ids.set(t, id);
        df.push(0);
      }
      c.set(id, (c.get(id) ?? 0) + 1);
    }
    for (const id of c.keys()) df[id] += 1;
    return [...c.entries()].sort((a, b) => a[0] - b[0]);
  });
  const n = cands.length;
  const idf = df.map((d) => Math.max(0, Math.log((n + 1) / (d + 0.5))) + 0.15);
  const vecs: Array<Array<[number, number]>> = counts.map((c) => {
    const v: Array<[number, number]> = c.map(([id, tf]) => [id, (1 + Math.log(tf)) * idf[id]]);
    const norm = Math.sqrt(v.reduce((s, x) => s + x[1] * x[1], 0));
    return norm > 0 ? v.map(([id, w]) => [id, w / norm] as [number, number]) : v;
  });

  const titleIds = new Set<number>();
  if (units[0]?.kind === 'heading') for (const t of contentTerms(units[0].text)) {
    const id = ids.get(t);
    if (id !== undefined) titleIds.add(id);
  }

  const centroid = new Float32Array(ids.size);
  for (const v of vecs) for (const [id, w] of v) centroid[id] += w;
  const cnorm = Math.sqrt(centroid.reduce((s, x) => s + x * x, 0));
  if (cnorm > 0) for (let i = 0; i < centroid.length; i++) centroid[i] /= cnorm;
  const centroidSim = vecs.map((v) => v.reduce((s, [id, w]) => s + w * centroid[id], 0));

  let keep = cands.map((_, i) => i);
  if (keep.length > MAX_CANDIDATES) {
    keep = [...keep].sort((a, b) => centroidSim[b] - centroidSim[a] || a - b).slice(0, MAX_CANDIDATES).sort((a, b) => a - b);
  }
  const m = keep.length;
  const sim = new Float32Array(m * m);
  for (let a = 0; a < m; a++) {
    for (let b = a + 1; b < m; b++) {
      const s = dot(vecs[keep[a]], vecs[keep[b]]);
      sim[a * m + b] = s;
      sim[b * m + a] = s;
    }
  }
  const rank = textRank(sim, m);
  const cmax = Math.max(0, ...keep.map((k) => centroidSim[k]));

  const score = keep.map((k, a) => {
    const u = units[cands[k]];
    const wc = wordCount(u.text);
    const base = cmax > 0 ? 0.5 * rank[a] + (0.5 * centroidSim[k]) / cmax : 1;
    const p = posOf.get(cands[k]) ?? 0;
    let title = 1;
    if (titleIds.size) {
      const hit = vecs[k].filter(([id]) => titleIds.has(id)).length;
      title = 1 + 0.3 * Math.min(1, hit / titleIds.size);
    }
    return base * positionPrior(p, pTotal, u.firstInPara) * lengthPrior(wc, length) * cuePrior(u.text) * title;
  });
  const smax = Math.max(0, ...score);
  if (smax > 0) for (let i = 0; i < score.length; i++) score[i] /= smax;

  const [kTarget, kMin, budget, lambda] = plan(length, Math.min(pTotal, cands.length), wordsTotal, m);
  const maxWords = Math.floor((budget * 3) / 2);
  const wordsOf = keep.map((k) => wordCount(units[cands[k]].text));
  const selected: number[] = [];
  let selWords = 0;
  while (selected.length < kTarget) {
    let best: [number, number] | null = null;
    for (let a = 0; a < m; a++) {
      if (selected.includes(a)) continue;
      let red = 0;
      for (const s of selected) red = Math.max(red, sim[a * m + s]);
      if (red >= 0.8) continue;
      if (selected.length && selWords + wordsOf[a] > maxWords) continue;
      const gain = lambda * score[a] - (1 - lambda) * red;
      if (!best || gain > best[1] + 1e-6) best = [a, gain];
    }
    if (!best) break;
    selected.push(best[0]);
    selWords += wordsOf[best[0]];
    if (selected.length >= kMin && selWords >= budget) break;
  }
  selected.sort((a, b) => a - b);
  return finish(length, units, selected.map((a) => cands[keep[a]]), false, chars, wordsTotal, units.length);
}

// ---------------------------------------------------------------------------------------
// Speaking state (one reducer for the real events and the demo)
// ---------------------------------------------------------------------------------------

export interface SpeakState {
  status: 'idle' | 'speaking';
  /** what is being read: the summary on screen, or a voice preview / test */
  target: 'summary' | 'sample' | null;
  range: { start: number; end: number } | null;
  engine: VoiceEngine | null;
  voice: string;
  index: number;
  count: number;
  /** a one-line heads-up (e.g. ElevenLabs failed, so the Windows voice took over) */
  notice: string | null;
  error: string | null;
}

export const idleSpeak = (): SpeakState => ({ status: 'idle', target: null, range: null, engine: null, voice: '', index: 0, count: 0, notice: null, error: null });

export function reduceSpeak(s: SpeakState, ev: SpeakEvent): SpeakState {
  switch (ev.state) {
    case 'speaking':
      return {
        ...s,
        status: 'speaking',
        range: ev.end > ev.start ? { start: ev.start, end: ev.end } : null,
        engine: ev.engine === 'elevenlabs' ? 'elevenlabs' : 'windows',
        voice: ev.voice,
        index: ev.index,
        count: ev.count,
      };
    case 'notice':
      return { ...s, notice: ev.message };
    case 'failed':
      return { ...idleSpeak(), error: ev.message, notice: s.notice };
    case 'done':
    case 'stopped':
      return { ...idleSpeak(), notice: s.notice };
  }
}

const speakStore = writable<SpeakState>(idleSpeak());
export const speaking: Readable<SpeakState> = { subscribe: speakStore.subscribe };

function dispatchSpeak(ev: SpeakEvent): void {
  speakStore.update((s) => reduceSpeak(s, ev));
}

export function clearSpeakMessages(): void {
  speakStore.update((s) => ({ ...s, notice: null, error: null }));
}

let started = false;
let unlisten: (() => void) | null = null;

/** Idempotent. Listens for read-aloud progress so the state survives switching tools. */
export async function startExplain(): Promise<void> {
  if (started) return;
  started = true;
  if (!isTauri()) return;
  try {
    unlisten = await listen<SpeakEvent>('explain-speak', dispatchSpeak);
  } catch (e) {
    console.warn('[explain] event wiring failed', e);
  }
}

export function stopExplainListeners(): void {
  unlisten?.();
  unlisten = null;
  started = false;
}

// ---------------------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------------------

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new Error(errorText(e));
  }
}

const SETTINGS_KEY = 'krovi.explain.settings';

function readDemoSettings(): ExplainSettings {
  try {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem(SETTINGS_KEY) : null;
    return { ...DEFAULT_SETTINGS, ...(raw ? (JSON.parse(raw) as Partial<ExplainSettings>) : {}) };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}
function writeDemoSettings(s: ExplainSettings): void {
  try {
    if (typeof localStorage !== 'undefined') localStorage.setItem(SETTINGS_KEY, JSON.stringify(s));
  } catch {
    /* blocked storage: settings just won't survive a reload */
  }
}

export async function summarize(text: string, length: SummaryLength): Promise<Summary> {
  if (isTauri()) return call<Summary>('explain_summarize', { text, length });
  return summarizeLocal(text, length);
}

// ---- demo-only state (browser): a pretend key so the flow can be shown; nothing is sent -----

let demoKey = false;
const DEMO_VOICES: ElevenVoice[] = [
  { id: 'demoVoiceRachel0001', name: 'Rachel (demo)', category: 'premade', detail: 'female · young · american' },
  { id: 'demoVoiceAdam000002', name: 'Adam (demo)', category: 'premade', detail: 'male · middle aged · american' },
];

function browserVoices(): WinVoice[] {
  if (typeof speechSynthesis === 'undefined') return [];
  return speechSynthesis
    .getVoices()
    .filter((v) => v.localService)
    .map((v) => ({ id: v.voiceURI, name: v.name, language: v.lang, gender: '', natural: /natural|neural/i.test(v.name) }));
}

export async function loadEngines(): Promise<Engines> {
  if (isTauri()) return call<Engines>('explain_engines');
  const voices = browserVoices();
  return {
    windows: { available: typeof speechSynthesis !== 'undefined', voices, defaultVoiceId: voices[0]?.id ?? null, error: null },
    elevenlabs: { hasKey: demoKey, keyHint: null, models: ELEVEN_MODELS, keysUrl: ELEVEN_KEYS_URL, error: null },
    settings: readDemoSettings(),
  };
}

export async function saveSettings(s: ExplainSettings): Promise<ExplainSettings> {
  if (isTauri()) return call<ExplainSettings>('explain_set_settings', { settings: s });
  const next = { ...s, voiceEngine: demoKey && s.elevenVoiceId ? s.voiceEngine : 'windows' } as ExplainSettings;
  writeDemoSettings(next);
  return next;
}

/** Save (string) or remove (null) the user's ElevenLabs key. */
export async function setElevenKey(key: string | null): Promise<KeyResult> {
  if (isTauri()) return call<KeyResult>('explain_set_key', { provider: 'elevenlabs', key });
  const s = readDemoSettings();
  if (key === null) {
    demoKey = false;
    const next = { ...s, voiceEngine: 'windows', elevenVoiceId: null, elevenVoiceName: null, elevenConsented: false } as ExplainSettings;
    writeDemoSettings(next);
    return { saved: false, hasKey: false, keyHint: null, voices: [], message: 'Your ElevenLabs key was removed. Reading aloud uses the Windows voice.', tone: 'ok', settings: next };
  }
  if (key.trim().length < 16) {
    return { saved: false, hasKey: demoKey, keyHint: null, voices: [], message: "That doesn't look like a full ElevenLabs API key. Copy the whole key and paste it again.", tone: 'error', settings: s };
  }
  demoKey = true;
  const next = { ...s, elevenVoiceId: s.elevenVoiceId ?? DEMO_VOICES[0].id, elevenVoiceName: s.elevenVoiceName ?? DEMO_VOICES[0].name } as ExplainSettings;
  writeDemoSettings(next);
  return { saved: true, hasKey: true, keyHint: null, voices: DEMO_VOICES, message: 'Demo: this is a simulation. In the app your key goes into Windows Credential Manager. Nothing was sent.', tone: 'ok', settings: next };
}

export async function refreshElevenVoices(): Promise<KeyResult> {
  if (isTauri()) return call<KeyResult>('explain_eleven_voices');
  return { saved: demoKey, hasKey: demoKey, keyHint: null, voices: demoKey ? DEMO_VOICES : [], message: demoKey ? `${DEMO_VOICES.length} demo voices.` : 'Paste your ElevenLabs key first.', tone: demoKey ? 'ok' : 'error', settings: readDemoSettings() };
}

export async function testElevenVoice(): Promise<TestResult> {
  if (isTauri()) return call<TestResult>('explain_test_voice');
  if (!demoKey) return { ok: false, kind: 'invalid-key', message: 'Paste and save your ElevenLabs key first.', ms: 0 };
  await new Promise((r) => setTimeout(r, 450));
  return { ok: true, kind: null, message: `Demo: pretending the voice works. In the app this plays a short sample (about ${creditsFor(TEST_SENTENCE_LENGTH, readDemoSettings().elevenModel)} credits).`, ms: 450 };
}

export async function openKeysPage(): Promise<void> {
  if (isTauri()) {
    await call<void>('explain_open_keys_page');
    return;
  }
  window.open(ELEVEN_KEYS_URL, '_blank', 'noopener,noreferrer');
}

// ---- reading aloud ----------------------------------------------------------------------

let demoQueue: SpeechSynthesisUtterance[] = [];

function demoSpeak(text: string, rate: number, voiceId: string | null): SpeakInfo {
  if (typeof speechSynthesis === 'undefined') throw new Error("This browser can't read aloud.");
  const units = splitUnits(text).filter((u) => u.kind !== 'heading' && /[\p{L}\p{N}]/u.test(speakable(u.text)));
  if (!units.length) throw new Error('There is nothing to read aloud.');
  speechSynthesis.cancel();
  const voice = speechSynthesis.getVoices().find((v) => v.voiceURI === voiceId) ?? speechSynthesis.getVoices().find((v) => v.localService) ?? null;
  const label = voice?.name ?? 'Browser voice';
  demoQueue = units.map((u, i) => {
    const ut = new SpeechSynthesisUtterance(speakable(u.text));
    ut.rate = Math.min(2, Math.max(0.5, rate));
    if (voice) ut.voice = voice;
    ut.onstart = () => dispatchSpeak({ state: 'speaking', index: i, count: units.length, start: u.start, end: u.end, engine: 'windows', voice: label });
    ut.onend = () => {
      if (i === units.length - 1) dispatchSpeak({ state: 'done', ms: 0 });
    };
    ut.onerror = (e) => {
      if (e.error === 'canceled' || e.error === 'interrupted') return;
      dispatchSpeak({ state: 'failed', message: "The browser couldn't read this aloud." });
    };
    return ut;
  });
  for (const ut of demoQueue) speechSynthesis.speak(ut);
  return { engine: 'windows', sentences: units.length, chars: units.reduce((n, u) => n + speakable(u.text).length + 1, 0), credits: null };
}

/**
 * Read `text` aloud. `engine` must be what the Read button said it would use. For ElevenLabs the
 * first call needs `confirmCloud` (the one-time "send this text to ElevenLabs?" answer); the
 * backend refuses with `consent-required` otherwise.
 */
export async function speakText(text: string, engine: VoiceEngine, opts: { confirmCloud?: boolean; target?: 'summary' | 'sample'; rate?: number; voiceId?: string | null } = {}): Promise<SpeakInfo> {
  clearSpeakMessages();
  speakStore.update((s) => ({ ...s, target: opts.target ?? 'summary' }));
  try {
    if (!isTauri()) {
      if (engine === 'elevenlabs') {
        dispatchSpeak({ state: 'notice', message: 'Demo: ElevenLabs is simulated, so your browser voice reads this.' });
      }
      return demoSpeak(text, opts.rate ?? 1, opts.voiceId ?? null);
    }
    return await call<SpeakInfo>('explain_speak', { args: { text, engine, confirmCloud: opts.confirmCloud ?? false } });
  } catch (e) {
    speakStore.update((s) => ({ ...s, target: null }));
    throw e;
  }
}

export async function stopSpeaking(): Promise<void> {
  if (!isTauri()) {
    if (typeof speechSynthesis !== 'undefined') speechSynthesis.cancel();
    demoQueue = [];
    dispatchSpeak({ state: 'stopped' });
    return;
  }
  await call<void>('explain_stop_speaking');
}

/** Characters (as spoken) and ElevenLabs credits for reading `text`. */
export async function estimateSpeech(text: string, modelId: string): Promise<{ sentences: number; chars: number; credits: number }> {
  if (isTauri()) {
    try {
      return await call<{ sentences: number; chars: number; credits: number }>('explain_estimate', { text });
    } catch {
      /* fall through to the local estimate */
    }
  }
  const units = splitUnits(text).filter((u) => /[\p{L}\p{N}]/u.test(speakable(u.text)));
  const chars = Math.max(0, units.reduce((n, u) => n + speakable(u.text).length + 1, 0) - 1);
  return { sentences: units.length, chars, credits: creditsFor(chars, modelId) };
}

/** Test hooks. */
export const __test = {
  reset(): void {
    speakStore.set(idleSpeak());
    demoKey = false;
    started = false;
  },
  setDemoKey(v: boolean): void {
    demoKey = v;
  },
};
