// Explain client (owner: explain) - docs/CONTRACTS.md section 10.
//
// Paste any text -> an on-device summary, and read it aloud.
//   * Tauri: thin wrappers over src-tauri/src/explain.rs (commands explain_*, event `explain-speak`).
//     The summary is computed in Rust on this PC; reading aloud uses a Windows voice (on-device)
//     or, only if the user saved their own key, ElevenLabs.
//   * Browser demo (`isTauri() === false`): the SAME summarising algorithm runs here in TypeScript
//     (a step-for-step port of krovi-explain's text.rs / summary.rs), and reading aloud uses the
//     browser's own speech synthesis. ElevenLabs is simulated and says so; nothing is sent anywhere.
//
// Keys never live in this file or in localStorage: in the app they are in Windows Credential
// Manager and the webview only ever learns "saved" and the last four characters.

import { writable, type Readable } from 'svelte/store';
import { invoke, isTauri, listen } from './bridge';

// ---------------------------------------------------------------------------------------
// Types (wire shapes: src-tauri/src/explain.rs, all camelCase)
// ---------------------------------------------------------------------------------------

/** What to produce. The wire strings of the Rust `SummaryLength` enum (kebab-case). */
export type SummaryLength = 'short' | 'detailed' | 'key-points' | 'action-items' | 'decisions' | 'dates' | 'questions';
/** The pre-0.3 names (`medium` = detailed, `points` = key-points); still accepted everywhere. */
export type LegacyLength = 'medium' | 'points';
export type VoiceEngine = 'windows' | 'elevenlabs';

export interface SummaryItem {
  index: number;
  text: string;
  /** which section (0-based) the sentence belongs to */
  section: number;
  /** extraction modes: the phrase that matched ("by Friday", "decided", "?") */
  hit: string | null;
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
  /** how many sections the text was divided into (headings, or paragraph chunks) */
  sections: number;
  /** how many of them contributed at least one sentence */
  sectionsCovered: number;
  /** how many sentences were scored or scanned */
  sentencesConsidered: number;
  /** sectionsCovered / sections */
  coverage: number;
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

/** Longest input accepted (characters). Longer text is refused with an error, never cut. */
export const MAX_CHARS = 1_000_000;
export const RATE_MIN = 0.6;
export const RATE_MAX = 1.8;

export const DEFAULT_SETTINGS: ExplainSettings = {
  summaryLength: 'detailed',
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
  { id: 'short', label: 'Short', hint: 'One to three sentences' },
  { id: 'detailed', label: 'Detailed', hint: 'Grows with the text and visits every section' },
  { id: 'key-points', label: 'Key points', hint: 'Bullet points; short list items allowed' },
  { id: 'action-items', label: 'Actions', hint: 'Things somebody needs to do' },
  { id: 'decisions', label: 'Decisions', hint: 'What was decided or agreed' },
  { id: 'dates', label: 'Dates', hint: 'Dates, deadlines and times' },
  { id: 'questions', label: 'Questions', hint: 'Open questions and unresolved points' },
];

/** Modes that filter by cue (action items, decisions, dates, questions) instead of ranking. */
export function isExtractMode(l: SummaryLength): boolean {
  return l === 'action-items' || l === 'decisions' || l === 'dates' || l === 'questions';
}

/** Accepts the old wire strings (`medium`, `points`) and anything unknown (-> `detailed`). */
export function normalizeLength(l: string | null | undefined): SummaryLength {
  if (l === 'medium') return 'detailed';
  if (l === 'points') return 'key-points';
  return LENGTHS.some((x) => x.id === l) ? (l as SummaryLength) : 'detailed';
}

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

/** How much of the text the result is based on: "Covers all 8 sections · 412 sentences read". */
export function coverageLabel(s: Summary): string {
  if (s.alreadyShort || s.sentencesConsidered === 0) return '';
  const read = `${formatNumber(s.sentencesConsidered)} ${s.sentencesConsidered === 1 ? 'sentence' : 'sentences'}`;
  if (isExtractMode(s.length)) {
    if (!s.items.length) return `${read} scanned`;
    return s.sections > 1 ? `Found in ${s.sectionsCovered} of ${s.sections} sections · ${read} scanned` : `${read} scanned`;
  }
  if (s.sections <= 1) return `${read} read`;
  const part = s.sectionsCovered === s.sections ? `Covers all ${s.sections} sections` : `Covers ${s.sectionsCovered} of ${s.sections} sections`;
  return `${part} · ${read} read`;
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

/** Thrown (browser demo) or returned by the backend ("Summary cancelled.") when a summary is stopped. */
export class CancelledError extends Error {
  constructor() {
    super('Summary cancelled.');
    this.name = 'CancelledError';
  }
}

/** True for the error a stopped summary ends with, in either runtime. */
export function isCancelled(e: unknown): boolean {
  return e instanceof CancelledError || errorText(e) === 'Summary cancelled.';
}

// ---------------------------------------------------------------------------------------
// Sentence splitting (TypeScript port of krovi-explain's text.rs)
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
// Hindi (Devanagari) and Hinglish (Roman) function words, as in text.rs.
const STOP_MULTI = new Set(
  'apna apne apni aur bhi hai hain hua hui isliye jab ka kar karna karo ke ki ko kya lekin liye matlab mein nahi nhi par se tak tha thi ya yaani yeh अंदर अपना अपनी अपने अभी आदि इन इनका इस इसका इसकी इसके इसमें इसे उन उनका उनकी उनके उस उसका उसकी उसके उसमें उसे एक ऐसा ऐसी ऐसे और कभी करता करते करना का काफी कि किया किस किसी की के केवल खुद गए गया जब जहाँ जा जितना जिस जिसे जो तक तब तरह तो था थी थे दिया दूसरा दूसरी दूसरे न नहीं ना पर पहले पूरा पे फिर बहुत बाद बिल्कुल भर भी मगर में सकता सकते सब सभी साथ से हम हमारा हमारी हमारे ही हुआ हुई हुए है हैं हो होता होती होते होना होने'.split(' '),
);

const isUpper = (c: string) => c !== c.toLowerCase();
const isLower = (c: string) => c !== c.toUpperCase();
const isAlpha = (c: string) => /\p{L}/u.test(c);
const isNumeric = (c: string) => /\p{N}/u.test(c);
const isAlnum = (c: string) => /[\p{L}\p{N}]/u.test(c);
const isSpace = (c: string) => /\s/u.test(c);
const isTerminator = (c: string) => '.!?…。！？।॥'.includes(c);
const isCjkTerminator = (c: string) => '。！？'.includes(c);
const isDanda = (c: string) => '।॥'.includes(c);
const isCloser = (c: string) => '"\'”’)]}»」'.includes(c);
const isOpener = (c: string) => '"\'“‘([{«「'.includes(c);

/** 1 = Han / kana (about two characters per word), 2 = Thai / Lao / Myanmar / Khmer (about four), 0 = other. */
function spacelessClass(cp: number): 0 | 1 | 2 {
  if ((cp >= 0x3040 && cp <= 0x30ff) || (cp >= 0x3400 && cp <= 0x4dbf) || (cp >= 0x4e00 && cp <= 0x9fff) || (cp >= 0xf900 && cp <= 0xfaff) || (cp >= 0xff66 && cp <= 0xff9f)) return 1;
  if ((cp >= 0x0e00 && cp <= 0x0eff) || (cp >= 0x1000 && cp <= 0x109f) || (cp >= 0x1780 && cp <= 0x17ff)) return 2;
  return 0;
}
const SPACELESS_RE = /[\u0e00-\u0eff\u1000-\u109f\u1780-\u17ff\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\uff66-\uff9f]/;
const HAS_ALNUM_RE = /[\p{L}\p{N}]/u;

/** Words: whitespace-separated tokens with a letter or digit; space-less scripts count per ~2 (Han, kana) or ~4 (Thai...) characters. */
export function wordCount(text: string): number {
  let n = 0;
  for (const tok of text.split(/\s+/)) {
    if (!tok) continue;
    if (!SPACELESS_RE.test(tok)) {
      if (HAS_ALNUM_RE.test(tok)) n++;
      continue;
    }
    let han = 0;
    let thai = 0;
    let other = false;
    for (const ch of tok) {
      const cls = spacelessClass(ch.codePointAt(0) ?? 0);
      if (cls === 1) han++;
      else if (cls === 2) thai++;
      else if (isAlnum(ch)) other = true;
    }
    n += (other ? 1 : 0) + Math.ceil(han / 2) + Math.ceil(thai / 4);
  }
  return n;
}

export function collapseWs(text: string): string {
  return text.replace(/[\s\u00ad\u200b]+/g, ' ').trim();
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

/** Length of a Markdown ATX heading marker ("##" plus the required space), or 0. */
function atxMarkerLen(s: string): number {
  const m = /^#{1,6}\s+/.exec(s);
  return m ? m[0].length : 0;
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
    let indent = 0;
    for (const c of noCr.slice(0, lead)) indent += c === '\t' ? 4 : 1;
    cur.push({ start: lineOff + lead, text, indent });
  }
  if (cur.length) paras.push(cur);
  return paras;
}

const firstChar = (s: string) => (s ? String.fromCodePoint(s.codePointAt(0) ?? 32) : ' ');
/** A heading starts with a capital, a digit, or a Han / kana character (those have no case). */
const headingStarter = (c: string) => isUpper(c) || /[0-9]/.test(c) || spacelessClass(c.codePointAt(0) ?? 0) === 1;

function isHeadingLine(lines: Line[], k: number): boolean {
  if (k !== 0) return false;
  const t = lines[0].text;
  if ([...t].length > 70 || wordCount(t) > 10 || wordCount(t) === 0) return false;
  if (!headingStarter(firstChar(t))) return false;
  const last = [...t].pop() ?? ' ';
  if ('.!?…,;:"\'”’)]。！？，、：；'.includes(last)) return false;
  const next = lines[1];
  if (!next) return true;
  return bulletMarkerLen(next.text) > 0 || headingStarter(firstChar(next.text));
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
    // Markdown table rows are their own units; re-flowing them would glue rows into one "sentence"
    if (line.text.startsWith('|')) {
      out.push({ kind: 'sentence', lines: [line] });
      return;
    }
    // Markdown ATX headings ("## Title") are headings wherever they appear
    const atx = atxMarkerLen(line.text);
    if (atx > 0) {
      const rest = line.text.slice(atx).trimStart();
      out.push({ kind: 'heading', lines: [{ start: line.start + (line.text.length - rest.length), text: rest, indent: line.indent }] });
      return;
    }
    if (isHeadingLine(lines, k)) {
      out.push({ kind: 'heading', lines: [line] });
      return;
    }
    const prev = out[out.length - 1];
    if (prev && prev.kind === 'bullet') {
      if (line.indent > 0 || isLower(firstChar(line.text))) prev.lines.push(line);
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
    if (isCjkTerminator(s[j]) || isDanda(s[j])) {
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

/** Content terms for scoring. Runs of space-less script become character bigrams (trigrams for Thai and friends). */
export function contentTerms(text: string): string[] {
  const out: string[] = [];
  let word = '';
  let run: string[] = [];
  let runClass = 0;
  const flush = () => {
    if (word && [...word].length >= 2 && !STOP.has(word) && !STOP_MULTI.has(word)) out.push(stem(word));
    word = '';
  };
  const flushRun = () => {
    if (!run.length) return;
    const n = runClass === 2 ? 3 : 2;
    if (run.length < n) out.push(run.join(''));
    else for (let i = 0; i + n <= run.length; i++) out.push(run.slice(i, i + n).join(''));
    run = [];
  };
  for (const c of text) {
    const cls = spacelessClass(c.codePointAt(0) ?? 0);
    if (cls !== 0) {
      flush();
      if (cls !== runClass) {
        flushRun();
        runClass = cls;
      }
      run.push(c);
      continue;
    }
    flushRun();
    runClass = 0;
    if (isAlnum(c)) word += c.toLowerCase();
    else if ((c === "'" || c === '’') && word) continue;
    else flush();
  }
  flush();
  flushRun();
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

/** Markdown noise removed for display (emphasis / code markers, heading hashes, table pipes). */
export function displayClean(s: string): string {
  let t = collapseWs(s);
  const trimmed = t.trim();
  if (trimmed.startsWith('|') && /^[|\-: \t]*$/.test(trimmed) && trimmed.includes('-')) return '';
  if (trimmed.startsWith('|') && trimmed.endsWith('|') && [...trimmed].length > 1) {
    const inner = trimmed.slice(0, -1).replace(/^\|+/, '').trim();
    t = inner.split(' | ').join(' · ');
  }
  for (;;) {
    const cut = atxMarkerLen(t);
    if (!cut) break;
    t = t.slice(cut);
  }
  const chars = [...t];
  let out = '';
  chars.forEach((c, i) => {
    if (c === '`') return;
    if (c === '*' || c === '_') {
      const prev = i === 0 ? null : chars[i - 1];
      const next = i + 1 < chars.length ? chars[i + 1] : null;
      const atEdge = (x: string | null) => x === null || !isAlnum(x);
      if (!atEdge(prev) && !atEdge(next)) out += c;
      return;
    }
    out += c;
  });
  return collapseWs(out);
}

// ---------------------------------------------------------------------------------------
// Cue detection: action items, decisions, dates, open questions (port of the same section of text.rs)
// ---------------------------------------------------------------------------------------

interface Tok {
  lc: string;
  start: number;
  end: number;
  cap: boolean;
}

function tokens(s: string): Tok[] {
  const out: Tok[] = [];
  const n = s.length;
  let i = 0;
  while (i < n) {
    if (!isAlnum(s[i])) {
      i++;
      continue;
    }
    const st = i;
    let j = i + 1;
    while (j < n) {
      const c = s[j];
      if (isAlnum(c)) j++;
      else if ('-/:.\'’'.includes(c) && j + 1 < n && isAlnum(s[j + 1])) j++;
      else break;
    }
    const raw = s.slice(st, j);
    out.push({ lc: raw.toLowerCase().replace(/’/g, "'"), start: st, end: j, cap: isUpper(raw[0]) || (st > 0 && s[st - 1] === '@') });
    i = j;
  }
  return out;
}

const isSeq = (t: Tok[], i: number, words: readonly string[]) => i + words.length <= t.length && words.every((w, k) => t[i + k].lc === w);
const tokAt = (t: Tok[], i: number) => (i >= 0 && i < t.length ? t[i].lc : '');
const phraseOf = (s: string, t: Tok[], a: number, b: number) => s.slice(t[a].start, t[b - 1].end);
const allDigits = (s: string) => /^\d+$/.test(s);

/** True when the sentence ends in a question mark (closing quotes / brackets ignored). */
export function endsWithQuestion(s: string): boolean {
  const t = s.trimEnd().replace(/["'”’)\]}»」]+$/, '');
  return t.endsWith('?') || t.endsWith('？');
}

const MONTH_NUM: Record<string, number> = { january: 1, jan: 1, february: 2, feb: 2, march: 3, mar: 3, april: 4, apr: 4, may: 5, june: 6, jun: 6, july: 7, jul: 7, august: 8, aug: 8, september: 9, sept: 9, sep: 9, october: 10, oct: 10, november: 11, nov: 11, december: 12, dec: 12 };
const monthOf = (w: string): number | null => (Object.prototype.hasOwnProperty.call(MONTH_NUM, w) ? MONTH_NUM[w] : null);

function dayOf(w: string): number | null {
  const m = /^(\d{1,2})(st|nd|rd|th)?$/.exec(w);
  if (!m) return null;
  const d = Number(m[1]);
  return d >= 1 && d <= 31 ? d : null;
}

function yearOf(w: string): number | null {
  if (!/^\d{4}$/.test(w)) return null;
  const y = Number(w);
  return y >= 1900 && y <= 2100 ? y : null;
}

function isWeekday(w: string, prev: string): boolean {
  switch (w) {
    case 'monday':
    case 'tuesday':
    case 'wednesday':
    case 'thursday':
    case 'friday':
    case 'saturday':
    case 'sunday':
    case 'tue':
    case 'tues':
    case 'thu':
    case 'thur':
    case 'thurs':
    case 'fri':
      return true;
    case 'mon':
    case 'wed':
    case 'sat':
    case 'sun':
      return ['by', 'on', 'next', 'this', 'last', 'every', 'until', 'before'].includes(prev);
    default:
      return false;
  }
}

function isNumericDate(w: string): boolean {
  const parts = w.split(/[-/.]/);
  if (parts.length !== 3 || !parts.every((p) => p.length <= 4 && allDigits(p))) return false;
  const l = parts.map((p) => p.length);
  const n = parts.map((p) => Number(p));
  const month = (v: number) => v >= 1 && v <= 12;
  const day = (v: number) => v >= 1 && v <= 31;
  if (l[0] === 4 && l[1] <= 2 && l[2] <= 2) return month(n[1]) && day(n[2]);
  if (l[0] <= 2 && l[1] <= 2 && (l[2] === 4 || l[2] === 2)) return (month(n[0]) && day(n[1])) || (day(n[0]) && month(n[1]));
  return false;
}

/** `3pm`, `10:30`, `10:30am` -> [has am/pm, has a colon]; null for anything else. */
function clock(w: string): [boolean, boolean] | null {
  let body = w;
  let mer = false;
  if (w.endsWith('am') || w.endsWith('pm')) {
    body = w.slice(0, -2);
    mer = true;
  }
  const ci = body.indexOf(':');
  const h = ci < 0 ? body : body.slice(0, ci);
  const m = ci < 0 ? null : body.slice(ci + 1);
  if (h.length > 2 || !allDigits(h)) return null;
  const hv = Number(h);
  if (m !== null && (m.length !== 2 || !allDigits(m) || Number(m) > 59)) return null;
  const colon = m !== null;
  if (!mer && !colon) return null;
  const ok = mer ? hv >= 1 && hv <= 12 : hv <= 23;
  return ok ? [mer, colon] : null;
}

const isQuarter = (w: string) => /^q[1-4]$/.test(w) || /^fy(\d{2}|\d{4})$/.test(w);

const DATE_PREFIX = ['by', 'on', 'at', 'in', 'next', 'this', 'last', 'every', 'until', 'till', 'before', 'after', 'from', 'since', 'through', 'due', 'around'];
const COUNT_WORDS = ['a', 'an', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine', 'ten', 'twelve', 'fifteen', 'twenty', 'thirty', 'sixty', 'ninety'];
const TIME_UNITS = ['minute', 'minutes', 'min', 'mins', 'hour', 'hours', 'hr', 'hrs', 'day', 'days', 'week', 'weeks', 'month', 'months', 'year', 'years', 'quarter', 'quarters'];
const PERIOD_UNITS = ['week', 'weekend', 'month', 'quarter', 'year', 'day', 'morning', 'afternoon', 'evening', 'night', 'fortnight', 'sprint'];
const REL_WORDS = ['next', 'this', 'last', 'every', 'coming', 'previous', 'following', 'upcoming'];
const SINGLE_REL = ['today', 'tonight', 'tomorrow', 'yesterday', 'eod', 'eow', 'eom', 'eoq', 'eoy', 'cob', 'noon', 'midnight'];
const TIMEZONES = ['utc', 'gmt', 'est', 'edt', 'cst', 'cdt', 'mst', 'mdt', 'pst', 'pdt', 'cet', 'cest', 'ist', 'bst', 'jst', 'et', 'pt'];
const ORDINAL_Q = ['first', 'second', 'third', 'fourth', '1st', '2nd', '3rd', '4th'];
const AMBIGUOUS_MONTH_PREV = ['in', 'by', 'until', 'before', 'after', 'since', 'during', 'from', 'early', 'mid', 'late', 'end', 'of'];
const YEAR_PREV = ['in', 'since', 'by', 'until', 'before', 'after', 'during', 'from', 'through', 'of', 'fy'];

/** End (exclusive) of a date / time phrase that starts at token `i`, if one does. */
function dateSpan(t: Tok[], i: number): number | null {
  const w = t[i].lc;
  const at = (k: number) => tokAt(t, k);
  const prev = i > 0 ? t[i - 1].lc : '';
  const meridiem = (k: number) => ['am', 'pm', 'a.m', 'p.m'].includes(at(k));

  if (isNumericDate(w)) return i + 1;
  const ck = clock(w);
  if (ck) {
    let e = i + 1;
    if (!ck[0] && meridiem(e)) e++;
    if (TIMEZONES.includes(at(e))) e++;
    return e;
  }
  if (allDigits(w) && w.length <= 2 && Number(w) >= 1 && Number(w) <= 12 && meridiem(i + 1)) {
    let e = i + 2;
    if (TIMEZONES.includes(at(e))) e++;
    return e;
  }
  if (SINGLE_REL.includes(w) || isWeekday(w, prev)) return i + 1;
  if (isQuarter(w)) return i + 1 + (yearOf(at(i + 1)) !== null ? 1 : 0);
  if (ORDINAL_Q.includes(w) && at(i + 1) === 'quarter') return i + 2 + (yearOf(at(i + 2)) !== null ? 1 : 0);
  // day first: 3 Nov 2026, 3rd of November
  if (dayOf(w) !== null) {
    let e = i + 1;
    if (at(e) === 'of') e++;
    if (monthOf(at(e)) !== null) {
      e++;
      return e + (yearOf(at(e)) !== null ? 1 : 0);
    }
  }
  // month first: Nov 3, November 2026, in March ("may" and "march" are also ordinary words)
  if (monthOf(w) !== null) {
    if (dayOf(at(i + 1)) !== null) {
      const e = i + 2;
      return e + (yearOf(at(e)) !== null ? 1 : 0);
    }
    if (yearOf(at(i + 1)) !== null) return i + 2;
    const ambiguous = w === 'may' || w === 'march';
    if (!ambiguous || AMBIGUOUS_MONTH_PREV.includes(prev)) return i + 1;
    return null;
  }
  if (REL_WORDS.includes(w) && PERIOD_UNITS.includes(at(i + 1))) return i + 2;
  if (w === 'in' || w === 'within' || w === 'after') {
    const c = at(i + 1);
    if ((allDigits(c) || COUNT_WORDS.includes(c)) && TIME_UNITS.includes(at(i + 2))) return i + 3;
  }
  if ((w === 'end' || w === 'start' || w === 'beginning' || w === 'middle') && at(i + 1) === 'of') {
    let e = i + 2;
    if (at(e) === 'the') e++;
    if (at(e) === 'business' && at(e + 1) === 'day') return e + 2;
    if (['day', 'week', 'month', 'quarter', 'year', 'sprint'].includes(at(e)) || monthOf(at(e)) !== null) return e + 1;
  }
  if (w === 'deadline' || w === 'deadlines') return i + 1;
  if (w === 'due' && at(i + 1) === 'date') return i + 2;
  if (yearOf(w) !== null && YEAR_PREV.includes(prev)) return i + 1;
  return null;
}

/** Every date / deadline / time phrase in `s`, in order ("by Friday", "Q3 2026", "at 3 pm"); at most four. */
export function findDates(s: string): string[] {
  const t = tokens(s);
  const n = t.length;
  const out: string[] = [];
  let lastEnd = 0;
  let i = 0;
  while (i < n && out.length < 4) {
    const e = dateSpan(t, i);
    if (e === null) {
      i++;
      continue;
    }
    let st = i;
    if (st >= lastEnd + 3 && isSeq(t, st - 3, ['no', 'later', 'than'])) st -= 3;
    else if (st > lastEnd && DATE_PREFIX.includes(t[st - 1].lc)) st -= 1;
    else if (st >= lastEnd + 2 && t[st - 1].lc === 'the' && DATE_PREFIX.includes(t[st - 2].lc)) st -= 2;
    const p = phraseOf(s, t, st, e);
    if (!out.includes(p)) out.push(p);
    lastEnd = e;
    i = e;
  }
  return out;
}

const NON_NAME = new Set(['the', 'a', 'an', 'this', 'that', 'these', 'those', 'it', 'its', 'there', 'here', 'he', 'she', 'they', 'we', 'i', 'you', 'who', 'what', 'when', 'where', 'why', 'how', 'which', 'if', 'as', 'in', 'on', 'at', 'for', 'to', 'by', 'with', 'from', 'after', 'before', 'during', 'however', 'also', 'but', 'and', 'or', 'so', 'then', 'today', 'tomorrow', 'yesterday', 'tonight', 'next', 'last', 'every', 'all', 'some', 'many', 'most', 'our', 'their', 'my', 'your', 'his', 'her', 'no', 'not', 'once', 'while', 'although', 'because', 'since', 'please', 'note', 'todo', 'action', 'attendees', 'agenda', 'notes', 'update', 'summary', 'decision', 'question', 'open', 'owner', 'status']);

function isName(t: Tok): boolean {
  return t.cap && [...t.lc].length >= 2 && /^\p{L}+$/u.test(t.lc) && !NON_NAME.has(t.lc) && monthOf(t.lc) === null && !isWeekday(t.lc, '');
}

const IMPERATIVES = new Set(['send', 'email', 'call', 'update', 'review', 'schedule', 'prepare', 'create', 'write', 'fix', 'confirm', 'book', 'submit', 'share', 'finalize', 'finalise', 'draft', 'contact', 'follow', 'add', 'remove', 'arrange', 'complete', 'finish', 'deploy', 'investigate', 'ping', 'organize', 'organise', 'buy', 'pay', 'sign', 'reply', 'approve', 'notify', 'remind', 'clean', 'publish', 'renew', 'cancel', 'collect', 'gather', 'print', 'upload', 'download', 'install', 'ask', 'tell', 'verify', 'migrate', 'refactor', 'document', 'assign', 'escalate', 'merge']);
const IMPERATIVES_LIST = new Set(['plan', 'order', 'test', 'set', 'file', 'move', 'record', 'report', 'get', 'make', 'take', 'put', 'bring', 'pick', 'clear', 'close', 'open', 'check', 'run', 'start', 'stop', 'build', 'learn', 'practice', 'try', 'find', 'look', 'think', 'decide', 'discuss', 'list', 'track', 'wrap', 'ship', 'launch', 'polish', 'read', 'reach', 'turn', 'walk', 'water', 'feed', 'wash', 'pack', 'return', 'drop', 'grab', 'fill', 'choose', 'compare', 'research', 'negotiate', 'hire', 'interview', 'train', 'invite', 'thank', 'congratulate']);
const NOT_AFTER_IMPERATIVE = new Set(['is', 'are', 'was', 'were', 'has', 'have', 'had', 'will', 'can', 'would', 'could', 'should', 'may', 'might', 'of', 'to', 'and', 'or', 'does', 'did']);

const isVerb = (w: string, listItem: boolean) => IMPERATIVES.has(w) || (listItem && IMPERATIVES_LIST.has(w));

/** Length of a leading checkbox marker (`[ ]`, `[x]`, `☐`, `☑`) plus its whitespace, or 0. */
export function checkboxLen(s: string): number {
  for (const m of ['[ ]', '[x]', '[X]', '[]', '☐', '☑', '☒']) {
    if (s.startsWith(m)) return s.length - s.slice(m.length).trimStart().length;
  }
  return 0;
}

const REQUEST_PHRASES: string[][] = [['can', 'you'], ['could', 'you'], ['would', 'you'], ['can', 'someone'], ['could', 'someone']];
const STRONG_PHRASES: string[][] = [['todo'], ['to-do'], ['action', 'item'], ['action', 'items'], ['follow', 'up'], ['follow-up'], ['next', 'step'], ['next', 'steps'], ['need', 'to'], ['needs', 'to'], ['has', 'to'], ['have', 'to'], ['must'], ['required', 'to'], ['responsible', 'for'], ['make', 'sure'], ['remember', 'to'], ["don't", 'forget'], ['be', 'sure', 'to'], ['please'], ['kindly'], ["i'll"], ["we'll"], ["let's"]];

/** Is this sentence an action item? [strength 1..3, the phrase that matched]. `listItem`: a bare verb opens an instruction ("Buy milk"). */
export function actionCue(s: string, listItem: boolean): [number, string] | null {
  if (checkboxLen(s.trimStart()) > 0) return [3, 'checkbox'];
  const t = tokens(s);
  if (!t.length) return null;
  const n = t.length;
  const q = endsWithQuestion(s);
  for (let i = 0; i < n; i++) {
    for (const p of REQUEST_PHRASES) if (isSeq(t, i, p)) return [2, phraseOf(s, t, i, i + p.length)];
  }
  if (q) return null;
  if (isName(t[0]) && n >= 3 && t[1].lc === 'to' && isVerb(t[2].lc, true)) return [3, phraseOf(s, t, 0, 3)];
  if (isName(t[0]) && n >= 2 && s.slice(t[0].end).startsWith(':') && isVerb(t[1].lc, true)) return [3, phraseOf(s, t, 0, 2)];
  for (let i = 0; i < n; i++) {
    for (const p of STRONG_PHRASES) if (isSeq(t, i, p)) return [2, phraseOf(s, t, i, i + p.length)];
  }
  for (let i = 1; i < n; i++) {
    const w = t[i].lc;
    let span = 0;
    if (w === 'will' || w === 'shall' || w === 'should') span = 1;
    else if ((w === 'going' || w === 'ought') && tokAt(t, i + 1) === 'to') span = 2;
    if (span > 0) {
      const p = t[i - 1].lc;
      if (p === 'i' || p === 'we' || p === 'you' || isName(t[i - 1])) return [1, phraseOf(s, t, i - 1, i + span)];
    }
  }
  if (isVerb(t[0].lc, listItem) && n >= 2 && !NOT_AFTER_IMPERATIVE.has(t[1].lc) && !s.slice(t[0].end).startsWith(':')) {
    return [listItem ? 2 : 1, phraseOf(s, t, 0, 1)];
  }
  return null;
}

const OPEN_GUARDS = ['to be decided', 'yet to decide', 'yet to be decided', 'not yet decided', 'not decided', 'undecided', 'no decision', 'tbd', 'if we decide', 'whether to'];
const DECISION_WORDS = new Set(['decided', 'agreed', 'approved', 'resolved', 'settled', 'chose', 'chosen', 'voted', 'adopted', 'finalized', 'finalised', 'endorsed', 'ratified', 'rejected', 'vetoed', 'opted', 'decision', 'decisions']);
const DECISION_PHRASES: string[][] = [['signed', 'off'], ['will', 'go', 'with'], ["we'll", 'go', 'with'], ['going', 'with'], ['will', 'proceed', 'with']];

/** Is this sentence a decision? The phrase that matched ("decided", "agreed", ...). */
export function decisionCue(s: string): string | null {
  if (endsWithQuestion(s)) return null;
  const low = s.toLowerCase();
  if (OPEN_GUARDS.some((g) => low.includes(g))) return null;
  const t = tokens(s);
  for (let i = 0; i < t.length; i++) {
    const w = t[i].lc;
    let len = DECISION_WORDS.has(w) ? 1 : 0;
    if (len === 0) {
      const p = DECISION_PHRASES.find((ph) => isSeq(t, i, ph));
      if (p) len = p.length;
    }
    if (len === 0) continue;
    if ((w === 'decision' || w === 'decisions') && ['tree', 'trees', 'making', 'maker', 'makers', 'support', 'boundary'].includes(tokAt(t, i + 1))) continue;
    const from = Math.max(0, i - 3);
    if (t.slice(from, i).some((p) => ['if', 'whether', 'unless', 'once', 'until'].includes(p.lc))) continue;
    return phraseOf(s, t, i, i + len);
  }
  return null;
}

const QUESTION_PHRASES: string[][] = [['tbd'], ['tbc'], ['unclear'], ['unresolved'], ['undecided'], ['undetermined'], ['unsure'], ['open', 'question'], ['open', 'questions'], ['open', 'issue'], ['open', 'issues'], ['to', 'be', 'decided'], ['to', 'be', 'determined'], ['to', 'be', 'confirmed'], ['not', 'sure'], ['not', 'yet', 'decided'], ['need', 'to', 'decide'], ['needs', 'to', 'be', 'decided'], ['remains', 'to', 'be', 'seen'], ['up', 'in', 'the', 'air'], ['i', 'wonder'], ['wondering', 'whether'], ['wondering', 'if']];

/** Is this sentence an open question or unresolved point? "?" or the phrase that matched. */
export function questionCue(s: string): string | null {
  if (endsWithQuestion(s)) return '?';
  const t = tokens(s);
  for (let i = 0; i < t.length; i++) {
    const p = QUESTION_PHRASES.find((ph) => isSeq(t, i, ph));
    if (p) return phraseOf(s, t, i, i + p.length);
  }
  return null;
}

// ---------------------------------------------------------------------------------------
// Summariser (port of summary.rs; read its module comment for the pipeline)
// ---------------------------------------------------------------------------------------

const READ_WPM = 238;
const SHORT_INPUT_WORDS = 25;
const SEC_TARGET = 24;
const SEC_MAX = 40;
const SEC_MIN = 8;
const OVERLAP = 3;
const DUP = 0.8;
const MAX_MATCHES = 150;

const readSeconds = (words: number) => (words / READ_WPM) * 60;
const maxOf = (xs: ArrayLike<number>, init = 0) => {
  let m = init;
  for (let i = 0; i < xs.length; i++) if (xs[i] > m) m = xs[i];
  return m;
};

type Vec = Array<[number, number]>;

function dot(a: Vec, b: Vec): number {
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

function overlapShare(v: Vec, title: Set<number>): number {
  if (!title.size) return 0;
  let hit = 0;
  for (const [id] of v) if (title.has(id)) hit++;
  return Math.min(1, hit / title.size);
}

function textRank(sim: Float64Array, m: number): number[] {
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
    if (delta < 1e-9) break;
  }
  const max = maxOf(r);
  return max > 0 ? r.map((x) => x / max) : r;
}

/** A mild lead bonus: the first sentences of a section and of the document tend to carry the topic. */
function positionPrior(pSec: number, secLen: number, pDoc: number, firstInPara: boolean): number {
  const leadSec = 1 + 0.12 * Math.exp(-pSec / 2);
  const leadDoc = 1 + 0.1 * Math.exp(-pDoc / 3);
  const para = firstInPara ? 1.04 : 1;
  const last = pSec + 1 === secLen && secLen > 3 ? 1.03 : 1;
  return leadSec * leadDoc * para * last;
}

function lengthPrior(words: number, mode: SummaryLength, bullet: boolean): number {
  const shortOk = bullet && mode === 'key-points';
  const [lo, hi, hard] = mode === 'key-points' ? (shortOk ? [3, 28, 70] : [8, 28, 70]) : [9, 35, 90];
  const floor = shortOk ? 2 : 5;
  if (words < floor) return 0.5;
  if (words < lo) return 0.6 + (0.4 * (words - floor)) / (lo - floor);
  if (words <= hi) return 1;
  if (words < hard) return 1 - (0.35 * (words - hi)) / (hard - hi);
  return 0.6;
}

const BOOST = ['in conclusion', 'in summary', 'to summarize', 'to summarise', 'in short', 'overall,', 'importantly', 'the main idea', 'the main point', 'the key point', 'the key idea', 'the purpose of', 'the goal of', 'this means', 'as a result', 'therefore', ' must ', 'shall not', 'is prohibited', 'are prohibited', 'is required', 'are required', 'in no event', 'निष्कर्ष', 'सारांश', 'मुख्य बात', 'इसका मतलब', 'इसलिए', 'matlab', 'yaani', 'isliye', '总之', '综上', '结论', '因此', '重要的是'];
const DOWN = ['for example', 'for instance', 'e.g.', 'such as', 'photo:', 'image:', 'caption:', 'source:', 'जैसे'];
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

interface Cand {
  unit: number;
  words: number;
  para: number;
  firstInPara: boolean;
  bullet: boolean;
}

interface Sec {
  start: number;
  end: number;
  parentStart: number;
  parentEnd: number;
  title: number | null;
  words: number;
}

function candidatesOf(units: Unit[], clean: string[], uwords: number[], minProse: number, minBullet: number): Cand[] {
  const out: Cand[] = [];
  units.forEach((u, i) => {
    if (u.kind === 'heading' || clean[i] === '') return;
    const min = u.kind === 'bullet' ? minBullet : minProse;
    if (uwords[i] >= min) out.push({ unit: i, words: uwords[i], para: u.para, firstInPara: u.firstInPara, bullet: u.kind === 'bullet' });
  });
  return out;
}

/** Cut `a..b` into chunks of about SEC_TARGET candidates at paragraph boundaries. */
function chunkRanges(cands: Cand[], a: number, b: number): Array<[number, number]> {
  if (b - a <= SEC_MAX) return [[a, b]];
  const out: Array<[number, number]> = [];
  let start = a;
  for (let j = a + 1; j < b; j++) {
    const len = j - start;
    const boundary = cands[j].para !== cands[j - 1].para;
    if ((boundary && len >= SEC_TARGET) || len >= SEC_MAX) {
      out.push([start, j]);
      start = j;
    }
  }
  if (b - start < SEC_MIN && out.length) out[out.length - 1][1] = b;
  else out.push([start, b]);
  return out;
}

function buildSections(units: Unit[], cands: Cand[]): Sec[] {
  const groups: Array<{ start: number; end: number; title: number | null }> = [];
  let curStart = 0;
  let curTitle: number | null = null;
  let ci = 0;
  for (let ui = 0; ui < units.length; ui++) {
    if (units[ui].kind !== 'heading') continue;
    while (ci < cands.length && cands[ci].unit < ui) ci++;
    if (ci > curStart) {
      groups.push({ start: curStart, end: ci, title: curTitle });
      curStart = ci;
    }
    curTitle = ui; // nested headings with no text between them: the deepest one is the title
  }
  if (cands.length > curStart) groups.push({ start: curStart, end: cands.length, title: curTitle });
  const out: Sec[] = [];
  for (const g of groups) {
    for (const [s, e] of chunkRanges(cands, g.start, g.end)) {
      let words = 0;
      for (let c = s; c < e; c++) words += cands[c].words;
      out.push({ start: s, end: e, parentStart: g.start, parentEnd: g.end, title: g.title, words });
    }
  }
  return out;
}

function sectionOf(cands: Cand[], secs: Sec[]): number[] {
  const v = new Array<number>(cands.length).fill(0);
  secs.forEach((s, si) => {
    for (let c = s.start; c < s.end; c++) v[c] = si;
  });
  return v;
}

interface Plan {
  k: number;
  kMin: number;
  budget: number;
  wordCap: number;
  lambda: number;
  wLocal: number;
  wGlob: number;
  hardFrac: number;
}

/** 12% of the sentences up to 100, easing to 5% at 3 000 and beyond. */
function detailShare(n: number): number {
  return n <= 100 ? 0.12 : Math.max(0.05, 0.12 - (0.07 * Math.log(n / 100)) / Math.log(30));
}

function planFor(mode: SummaryLength, n: number, words: number): Plan {
  const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
  let p: Plan;
  if (mode === 'short') {
    const k = n < 5 ? 1 : n < 200 ? 2 : 3;
    p = { k, kMin: 1, budget: k === 3 ? 100 : clamp(Math.floor(words * 0.45), 15, 70), wordCap: Infinity, lambda: 0.85, wLocal: 0.4, wGlob: 0.6, hardFrac: 0 };
  } else if (mode === 'key-points') {
    p = { k: clamp(Math.round(n * detailShare(n) * 0.75), 3, 70), kMin: 3, budget: 0, wordCap: clamp(Math.floor(words * 0.2), 90, 1500), lambda: 0.65, wLocal: 0.75, wGlob: 0.25, hardFrac: 0.09 };
  } else {
    p = { k: clamp(Math.round(n * detailShare(n)), 3, 90), kMin: 3, budget: 0, wordCap: clamp(Math.floor(words * 0.3), 120, 2400), lambda: 0.7, wLocal: 0.75, wGlob: 0.25, hardFrac: 0.12 };
  }
  // tiny inputs must really shrink, and the result is always shorter than the input
  p.k = Math.max(1, Math.min(p.k, Math.max(1, Math.floor((3 * n) / 5)), Math.max(0, n - 1)));
  p.kMin = Math.min(p.kMin, p.k);
  return p;
}

interface SecStat {
  cands: number;
  words: number;
  substantial: boolean;
}

/**
 * Split `k` sentences across sections: one for every substantial section first, the rest in proportion
 * to size (largest remainder), no section giving more than ~60% of its sentences. With more substantial
 * sections than `k` the budget grows to cover them, up to `hard`.
 */
function allocate(secs: SecStat[], k: number, hard: number): number[] {
  const n = secs.length;
  const q = new Array<number>(n).fill(0);
  const cap = secs.map((s) => Math.max(1, Math.floor((s.cands * 3) / 5)));
  let sub: number[] = [];
  for (let i = 0; i < n; i++) if (secs[i].substantial) sub.push(i);
  const kEff = sub.length > k ? Math.min(sub.length, Math.max(hard, k)) : k;
  if (sub.length > kEff) {
    sub.sort((a, b) => secs[b].words - secs[a].words || a - b);
    sub = sub.slice(0, kEff);
  }
  for (const i of sub) q[i] = 1;
  let assigned = sub.length;
  for (;;) {
    const remaining = Math.max(0, kEff - assigned);
    if (remaining === 0) break;
    const open: number[] = [];
    for (let i = 0; i < n; i++) if (q[i] < cap[i]) open.push(i);
    let wsum = 0;
    for (const i of open) wsum += secs[i].words;
    if (!open.length || wsum <= 0) break;
    let gave = 0;
    const frac: Array<[number, number]> = [];
    for (const i of open) {
      const share = (remaining * secs[i].words) / wsum;
      const whole = Math.min(Math.floor(share), cap[i] - q[i]);
      q[i] += whole;
      gave += whole;
      frac.push([i, share - Math.floor(share)]);
    }
    frac.sort((a, b) => b[1] - a[1] || secs[b[0]].words - secs[a[0]].words || a[0] - b[0]);
    let left = remaining - gave;
    for (const [i] of frac) {
      if (left === 0) break;
      if (q[i] < cap[i]) {
        q[i] += 1;
        left -= 1;
        gave += 1;
      }
    }
    if (gave === 0) break;
    assigned += gave;
  }
  return q;
}

function bulletsOf(items: SummaryItem[]): string {
  return items.map((it) => `• ${it.text}`).join('\n');
}

/** One paragraph per section that contributed. */
function bySection(items: SummaryItem[]): string {
  let out = '';
  let prev: number | null = null;
  for (const it of items) {
    if (prev !== null) out += prev === it.section ? ' ' : '\n\n';
    out += it.text;
    prev = it.section;
  }
  return out;
}

interface Ctx {
  units: Unit[];
  clean: string[];
  uwords: number[];
  mode: SummaryLength;
  chars: number;
  words: number;
}

function assemble(ctx: Ctx, items: SummaryItem[], text: string, alreadyShort: boolean, note: string | null, sections: number, sentencesConsidered: number): Summary {
  const summaryWords = wordCount(text);
  const read = readSeconds(ctx.words);
  const sread = readSeconds(summaryWords);
  const covered = new Set(items.map((i) => i.section)).size;
  return {
    length: ctx.mode,
    text,
    items,
    alreadyShort,
    note: alreadyShort ? 'This is already short, so here it is as written. Paste something longer to see a summary.' : note,
    stats: {
      chars: ctx.chars,
      words: ctx.words,
      sentences: ctx.units.length,
      summaryWords,
      summarySentences: items.length,
      readSecs: read,
      summaryReadSecs: sread,
      savedSecs: Math.max(0, read - sread),
      ratio: ctx.words === 0 ? 1 : Math.min(1, summaryWords / ctx.words),
    },
    sections,
    sectionsCovered: covered,
    sentencesConsidered,
    coverage: sections === 0 ? 0 : covered / sections,
  };
}

function* extractSteps(ctx: Ctx): Generator<void, Summary, void> {
  const { units, clean, uwords, mode } = ctx;
  const cands = candidatesOf(units, clean, uwords, 2, 2);
  const secs = buildSections(units, cands);
  const secOf = sectionOf(cands, secs);

  const found: Array<{ c: number; strength: number; hit: string }> = [];
  for (const sec of secs) {
    yield;
    for (let c = sec.start; c < sec.end; c++) {
      const u = units[cands[c].unit];
      let hit: [number, string] | null = null;
      if (mode === 'action-items') hit = actionCue(u.text, cands[c].bullet);
      else if (mode === 'decisions') {
        const h = decisionCue(u.text);
        hit = h === null ? null : [2, h];
      } else if (mode === 'questions') {
        const h = questionCue(u.text);
        hit = h === null ? null : [h === '?' ? 3 : 2, h];
      } else {
        const d = findDates(u.text);
        hit = d.length ? [Math.min(d.length + 1, 3), d.join(', ')] : null;
      }
      if (hit) found.push({ c, strength: hit[0], hit: hit[1] });
    }
  }
  yield;
  const total = found.length;

  // strongest first, then document order; near-duplicates and the cap drop the rest
  const order = found.map((_, i) => i).sort((x, y) => found[y].strength - found[x].strength || x - y);
  const ids = new Map<string, number>();
  const mvec: Vec[] = found.map((f) => {
    const tf = new Map<number, number>();
    for (const t of contentTerms(units[cands[f.c].unit].text)) {
      let id = ids.get(t);
      if (id === undefined) {
        id = ids.size;
        ids.set(t, id);
      }
      tf.set(id, (tf.get(id) ?? 0) + 1);
    }
    const v: Vec = [...tf.entries()].map(([id, n]) => [id, 1 + Math.log(n)] as [number, number]).sort((a, b) => a[0] - b[0]);
    let norm = 0;
    for (const x of v) norm += x[1] * x[1];
    norm = Math.sqrt(norm);
    return norm > 0 ? v.map(([id, w]) => [id, w / norm] as [number, number]) : v;
  });
  const kept: number[] = [];
  let capped = false;
  for (const f of order) {
    if (kept.length >= MAX_MATCHES) {
      capped = true;
      break;
    }
    if (kept.some((k) => dot(mvec[k], mvec[f]) >= 0.85)) continue;
    kept.push(f);
  }
  kept.sort((a, b) => a - b);

  const items: SummaryItem[] = kept.map((f) => {
    const { c, hit } = found[f];
    const unit = cands[c].unit;
    let t = clean[unit];
    if (mode === 'action-items') t = t.slice(checkboxLen(t)).trimStart();
    return { index: unit, text: t, section: secOf[c], hit };
  });
  const text = mode === 'dates' ? items.map((it) => `• ${it.hit ?? ''}: ${it.text}`).join('\n') : bulletsOf(items);
  let note: string | null = null;
  if (!items.length) {
    note = mode === 'action-items' ? 'No action items found in this text.' : mode === 'decisions' ? 'No decisions found in this text.' : mode === 'dates' ? 'No dates or deadlines found in this text.' : 'No open questions found in this text.';
  } else if (capped) {
    note = `Showing the ${items.length} strongest of ${total} matches.`;
  }
  return assemble(ctx, items, text, false, note, secs.length, cands.length);
}

/**
 * The whole summariser as a generator: it yields between phases and sections so a caller can stop it
 * (`summarizeLocal` drives it synchronously, `summarizeLocalAsync` hands the thread back now and then).
 * Throws an Error with a plain sentence when the input is empty or too long.
 */
export function* summarizeSteps(input: string, length: SummaryLength): Generator<void, Summary, void> {
  if (!input.trim()) throw new Error('Paste some text to summarize.');
  const chars = [...input].length;
  if (chars > MAX_CHARS) throw new Error(`That is ${formatNumber(chars)} characters; Krovi summarizes up to ${formatNumber(MAX_CHARS)} at a time. Paste a smaller part.`);
  yield;

  const units = splitUnits(input);
  yield;
  const wordsTotal = wordCount(input);
  const clean = units.map((u) => (u.kind === 'heading' ? '' : displayClean(u.text)));
  const uwords = units.map((u) => wordCount(u.text));
  const ctx: Ctx = { units, clean, uwords, mode: length, chars, words: wordsTotal };

  if (isExtractMode(length)) return yield* extractSteps(ctx);

  const minBullet = length === 'key-points' ? 2 : 4;
  let cands = candidatesOf(units, clean, uwords, 4, minBullet);
  if (cands.length < 2) cands = candidatesOf(units, clean, uwords, 1, 1);
  if (wordsTotal < SHORT_INPUT_WORDS || cands.length < 2) {
    // Nothing worth cutting: hand the text back, say so.
    let chosen = units.map((_, i) => i).filter((i) => units[i].kind !== 'heading');
    if (!chosen.length) chosen = units.map((_, i) => i);
    const items: SummaryItem[] = [];
    for (const i of chosen) {
      const t = displayClean(units[i].text);
      if (t) items.push({ index: i, text: t, section: 0, hit: null });
    }
    return assemble(ctx, items, items.map((it) => it.text).join(' '), true, null, 1, items.length);
  }

  const secs = buildSections(units, cands);
  const secOf = sectionOf(cands, secs);
  const n = cands.length;
  const p = planFor(length, n, wordsTotal);

  // ---- TF-IDF vectors (IDF over every candidate sentence) ---------------------------------
  const ids = new Map<string, number>();
  const df: number[] = [];
  const counts: Vec[] = cands.map((c) => {
    const tf = new Map<number, number>();
    for (const t of contentTerms(units[c.unit].text)) {
      let id = ids.get(t);
      if (id === undefined) {
        id = ids.size;
        ids.set(t, id);
        df.push(0);
      }
      tf.set(id, (tf.get(id) ?? 0) + 1);
    }
    for (const id of tf.keys()) df[id] += 1;
    return [...tf.entries()].sort((a, b) => a[0] - b[0]);
  });
  yield;
  const idf = df.map((d) => Math.max(0, Math.log((n + 1) / (d + 0.5))) + 0.15);
  const vecs: Vec[] = counts.map((c) => {
    const v: Vec = c.map(([id, tf]) => [id, (1 + Math.log(tf)) * idf[id]] as [number, number]);
    let norm = 0;
    for (const x of v) norm += x[1] * x[1];
    norm = Math.sqrt(norm);
    return norm > 0 ? v.map(([id, w]) => [id, w / norm] as [number, number]) : v;
  });
  const vocab = ids.size;

  const titleSet = (unit: number | null): Set<number> => {
    const out = new Set<number>();
    if (unit === null) return out;
    for (const t of contentTerms(units[unit].text)) {
      const id = ids.get(t);
      if (id !== undefined) out.add(id);
    }
    return out;
  };
  const docTitle = titleSet(units[0]?.kind === 'heading' ? 0 : null);
  const secTitles = secs.map((s) => titleSet(s.title));

  // ---- document centroid ---------------------------------------------------------------
  const centroid = new Float64Array(vocab);
  for (const v of vecs) for (const [id, w] of v) centroid[id] += w;
  let cnorm = 0;
  for (let i = 0; i < vocab; i++) cnorm += centroid[i] * centroid[i];
  cnorm = Math.sqrt(cnorm);
  if (cnorm > 0) for (let i = 0; i < vocab; i++) centroid[i] /= cnorm;
  const docSim = vecs.map((v) => {
    let s = 0;
    for (const [id, w] of v) s += w * centroid[id];
    return s;
  });
  const gmax = maxOf(docSim);

  // ---- sections: how many sentences each gets -------------------------------------------------
  const sortedWords = secs.map((s) => s.words).sort((a, b) => a - b);
  const median = sortedWords[Math.floor(sortedWords.length / 2)];
  const threshold = Math.min(40, Math.max(12, Math.floor(median / 3)));
  const stats: SecStat[] = secs.map((s) => ({ cands: s.end - s.start, words: s.words, substantial: s.end - s.start >= 2 && s.words >= threshold }));
  // Covering every substantial section may need more than `k`: up to 12% (9% for key points) of all
  // sentences, and always enough for a dozen sections.
  const hard = Math.max(Math.ceil(p.hardFrac * n), 12);
  const quota = length === 'short' ? new Array<number>(secs.length).fill(0) : allocate(stats, p.k, hard);

  // ---- per-section ranking ---------------------------------------------------------------
  const raw = new Array<number>(n).fill(0);
  const orders: number[][] = [];
  const scratch = new Float64Array(vocab);
  for (let si = 0; si < secs.length; si++) {
    yield;
    const sec = secs[si];
    const a = sec.start;
    const b = sec.end;
    const ca = Math.max(Math.max(a - OVERLAP, 0), sec.parentStart);
    const cb = Math.min(b + OVERLAP, sec.parentEnd);
    const m = cb - ca;
    const off = a - ca;
    const core = b - a;

    const sim = new Float64Array(m * m);
    for (let x = 0; x < m; x++) {
      for (let y = x + 1; y < m; y++) {
        const s = dot(vecs[ca + x], vecs[ca + y]);
        sim[x * m + y] = s;
        sim[y * m + x] = s;
      }
    }
    const rank = textRank(sim, m);

    // section centroid (core plus context)
    const touched: number[] = [];
    for (let c = ca; c < cb; c++) {
      for (const [id, w] of vecs[c]) {
        if (scratch[id] === 0) touched.push(id);
        scratch[id] += w;
      }
    }
    let snorm = 0;
    for (const id of touched) snorm += scratch[id] * scratch[id];
    snorm = Math.sqrt(snorm);
    const cent: number[] = [];
    for (let t = 0; t < core; t++) {
      let s = 0;
      for (const [id, w] of vecs[a + t]) s += w * scratch[id];
      cent.push(snorm > 0 ? s / snorm : 0);
    }
    for (const id of touched) scratch[id] = 0;

    let rmax = 0;
    for (let t = 0; t < core; t++) rmax = Math.max(rmax, rank[off + t]);
    const cmax = maxOf(cent);
    for (let t = 0; t < core; t++) {
      const c = a + t;
      const rn = rmax > 0 ? rank[off + t] / rmax : 1;
      const cn = cmax > 0 ? cent[t] / cmax : 1;
      const local = 0.5 * rn + 0.5 * cn;
      const glob = gmax > 0 ? docSim[c] / gmax : 1;
      const base = p.wLocal * local + p.wGlob * glob;
      const cand = cands[c];
      const u = units[cand.unit];
      const title = 1 + 0.22 * overlapShare(vecs[c], secTitles[si]) + 0.08 * overlapShare(vecs[c], docTitle);
      raw[c] = base * positionPrior(t, core, c, cand.firstInPara) * lengthPrior(cand.words, length, cand.bullet) * cuePrior(u.text) * title;
    }
    let smax = 0;
    for (let c = a; c < b; c++) smax = Math.max(smax, raw[c]);
    const ss: number[] = [];
    for (let c = a; c < b; c++) ss.push(smax > 0 ? raw[c] / smax : 0);

    // MMR inside the section: a few more than the quota, as replacements for duplicates
    const limit = length === 'short' ? Math.min(4, core) : Math.min(quota[si] + 3, core);
    const order: number[] = [];
    const red = new Array<number>(core).fill(0);
    const taken = new Array<boolean>(core).fill(false);
    while (order.length < limit) {
      let best: [number, number] | null = null;
      for (let t = 0; t < core; t++) {
        if (taken[t] || red[t] >= DUP) continue;
        const gain = p.lambda * ss[t] - (1 - p.lambda) * red[t];
        if (!best || gain > best[1] + 1e-9) best = [t, gain];
      }
      if (!best) break;
      const t = best[0];
      taken[t] = true;
      order.push(a + t);
      for (let u = 0; u < core; u++) if (!taken[u]) red[u] = Math.max(red[u], sim[(off + t) * m + (off + u)]);
    }
    orders.push(order);
  }
  yield;

  // ---- merge the sections ------------------------------------------------------------------
  const kept: number[] = [];
  if (length === 'short') {
    // a global MMR over each section's best few
    const pool = orders.flat();
    const gmaxRaw = maxOf(raw);
    const score = (c: number) => (gmaxRaw > 0 ? raw[c] / gmaxRaw : 0);
    const maxWords = Math.floor((p.budget * 3) / 2);
    const red = new Array<number>(pool.length).fill(0);
    const taken = new Array<boolean>(pool.length).fill(false);
    let selWords = 0;
    while (kept.length < p.k) {
      let best: [number, number] | null = null;
      for (let pi = 0; pi < pool.length; pi++) {
        const c = pool[pi];
        if (taken[pi] || red[pi] >= DUP) continue;
        if (kept.length && selWords + cands[c].words > maxWords) continue;
        const gain = p.lambda * score(c) - (1 - p.lambda) * red[pi];
        if (!best || gain > best[1] + 1e-9) best = [pi, gain];
      }
      if (!best) break;
      const pi = best[0];
      taken[pi] = true;
      kept.push(pool[pi]);
      selWords += cands[pool[pi]].words;
      for (let qi = 0; qi < pool.length; qi++) if (!taken[qi]) red[qi] = Math.max(red[qi], dot(vecs[pool[pi]], vecs[pool[qi]]));
      if (kept.length >= p.kMin && selWords >= p.budget) break;
    }
  } else {
    const got = new Array<number>(secs.length).fill(0);
    for (let si = 0; si < secs.length; si++) {
      for (const c of orders[si]) {
        if (got[si] >= quota[si]) break;
        if (kept.some((k) => dot(vecs[k], vecs[c]) >= DUP)) continue; // the same point was already made in an earlier section
        kept.push(c);
        got[si] += 1;
      }
    }
    // keep the whole under the word cap, never dropping a section's only sentence
    let total = 0;
    for (const c of kept) total += cands[c].words;
    while (total > p.wordCap && kept.length > p.kMin) {
      const per = new Array<number>(secs.length).fill(0);
      for (const c of kept) per[secOf[c]] += 1;
      let victim = -1;
      for (let pos = 0; pos < kept.length; pos++) {
        const c = kept[pos];
        if (per[secOf[c]] < 2) continue;
        if (victim < 0) victim = pos;
        else {
          const o = kept[victim];
          if (raw[c] < raw[o] || (raw[c] === raw[o] && c > o)) victim = pos;
        }
      }
      if (victim < 0) break;
      total -= cands[kept[victim]].words;
      kept.splice(victim, 1);
    }
  }
  kept.sort((x, y) => x - y);

  const items: SummaryItem[] = kept.map((c) => ({ index: cands[c].unit, text: clean[cands[c].unit], section: secOf[c], hit: null }));
  const text = length === 'key-points' ? bulletsOf(items) : length === 'short' ? items.map((it) => it.text).join(' ') : bySection(items);
  return assemble(ctx, items, text, false, null, secs.length, n);
}

/**
 * Synchronous driver. `isCancelled` is asked between phases and sections; when it returns true the
 * summary stops with a `CancelledError`. Accepts the pre-0.3 names (`medium`, `points`) too.
 */
export function summarizeLocal(input: string, length: SummaryLength | LegacyLength, isCancelled?: () => boolean): Summary {
  const it = summarizeSteps(input, normalizeLength(length));
  for (;;) {
    const r = it.next();
    if (r.done) return r.value;
    if (isCancelled?.()) throw new CancelledError();
  }
}

/** Like `summarizeLocal`, but hands the thread back every few milliseconds and stops when `signal` aborts. */
export async function summarizeLocalAsync(input: string, length: SummaryLength | LegacyLength, signal?: AbortSignal): Promise<Summary> {
  const it = summarizeSteps(input, normalizeLength(length));
  let last = Date.now();
  for (;;) {
    if (signal?.aborted) throw new CancelledError();
    const r = it.next();
    if (r.done) return r.value;
    if (Date.now() - last > 12) {
      await new Promise<void>((res) => setTimeout(res, 0));
      last = Date.now();
    }
  }
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
    const s = { ...DEFAULT_SETTINGS, ...(raw ? (JSON.parse(raw) as Partial<ExplainSettings>) : {}) };
    return { ...s, summaryLength: normalizeLength(s.summaryLength) };
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

/**
 * Summarize `text`. In the app this runs on a background thread in Rust; in the browser demo the same
 * algorithm runs here and yields to the page as it goes. `signal` stops it (the promise rejects with
 * a `CancelledError` / "Summary cancelled."). Accepts the pre-0.3 names (`medium`, `points`).
 */
export async function summarize(text: string, length: SummaryLength | LegacyLength, signal?: AbortSignal): Promise<Summary> {
  const mode = normalizeLength(length);
  if (isTauri()) {
    if (signal?.aborted) throw new CancelledError();
    // best effort: the backend also stops an older summary when a newer one starts
    const onAbort = () => void call<void>('explain_cancel_summarize').catch(() => undefined);
    signal?.addEventListener('abort', onAbort, { once: true });
    try {
      return await call<Summary>('explain_summarize', { text, length: mode });
    } finally {
      signal?.removeEventListener('abort', onAbort);
    }
  }
  return summarizeLocalAsync(text, mode, signal);
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
  if (isTauri()) {
    const e = await call<Engines>('explain_engines');
    return { ...e, settings: { ...e.settings, summaryLength: normalizeLength(e.settings.summaryLength) } };
  }
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
