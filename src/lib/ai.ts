// Settings -> AI: bring your own key (docs/DECISIONS.md D-022, docs/product/AI_PROVIDERS.md).
// Contract: src-tauri/src/ai.rs (commands) and krovi-net's `ai` module (the rules).
//
// PRIVACY SHAPE (the UI must keep to it):
//   * A key is typed into a password field, handed to `saveKey` once, and cleared. It is never put
//     in a store, in localStorage, in a URL, in the console, or in any object this file returns.
//     Rust keeps it in Windows Credential Manager; the webview only ever learns "a key is saved"
//     and its last four characters.
//   * Krovi has no account, no server and no proxy. Requests go straight from this PC to the
//     provider you choose (Rust makes them). Local tools never need a key.
//   * Nothing here sends anything by itself: only the explicit buttons (Test connection, a feature's
//     own "ask") do.
//   * Gemini stays off until the notice is accepted; Snowflake is experimental.
//
// TAURI: thin wrappers over the commands, normalising every answer field by field (so an extra
//   field - even a key, if a bug ever sent one - cannot reach a store).
// DEMO (plain browser, no Tauri): no real calls. The providers are listed and every action says
//   "Available in the desktop app."

import { writable, type Readable } from 'svelte/store';
import { invoke, isTauri } from './bridge';
import { AiError, DESKTOP_ONLY, PROVIDER_IDS, errorText, toFailure, type AiFailure, type AiFailureKind, type ProviderId } from './circle';

export { AiError, DESKTOP_ONLY, toFailure };
export type { AiFailure, AiFailureKind, ProviderId };

// ---------------------------------------------------------------------------------------
// Wire types (src-tauri/src/ai.rs, camelCase)
// ---------------------------------------------------------------------------------------

export type Gate = 'consent-required' | 'account-required';

export interface UsageInfo {
  requests: number;
  inputTokens: number;
  outputTokens: number;
  lastError: { kind: AiFailureKind; at: number } | null;
}

export interface AiProviderState {
  id: ProviderId;
  name: string;
  /** a key is saved in Windows Credential Manager */
  hasKey: boolean;
  /** last four characters of the key, when it is long enough to show them */
  keyHint: string | null;
  /** a key is saved and nothing else is missing */
  ready: boolean;
  gate: Gate | null;
  /** the model used for text (the user's choice, or the default) */
  model: string;
  defaultModel: string;
  /** the model used for pictures */
  visionModel: string;
  supportsModelList: boolean;
  needsAccountUrl: boolean;
  /** Snowflake's account URL: an address, not a secret */
  accountUrl: string | null;
  usage: UsageInfo;
}

export interface GeminiConsentInfo {
  accepted: boolean;
  acceptedAt: number | null;
  version: number;
  currentVersion: number;
}

export interface AiStatus {
  providers: AiProviderState[];
  /** the provider features use; null when none is chosen and ready */
  selected: ProviderId | null;
  model: string | null;
  gemini: GeminiConsentInfo;
}

export interface ModelInfo {
  id: string;
  name: string | null;
}

export interface TestStep {
  /** `key`: the key exists and is accepted. `inference`: a one-token request really ran. */
  id: 'key' | 'inference';
  ok: boolean;
  kind: AiFailureKind | null;
  message: string;
}

export interface TestReport {
  ok: boolean;
  kind: AiFailureKind | null;
  message: string;
  status: number | null;
  detail: string | null;
  model: string;
  models: ModelInfo[] | null;
  steps: TestStep[];
  notes: string[];
  inputTokens: number | null;
  outputTokens: number | null;
}

export interface AiCompletion {
  text: string;
  truncated: boolean;
  provider: ProviderId;
  providerName: string;
  model: string;
  inputTokens: number | null;
  outputTokens: number | null;
  label: string;
}

// ---------------------------------------------------------------------------------------
// Words the page shows (honest labels, one place)
// ---------------------------------------------------------------------------------------

/** Shown under the provider cards. */
export const PRIVACY_LINE = 'Krovi has no account and no server. Requests go straight from this PC to the provider you choose. Local tools never need a key.';
/** Shown next to a saved key. */
export const KEY_STORED_TEXT = 'Key saved in Windows Credential Manager';
/** Every AI answer carries this label. */
export const AI_OUTPUT_LABEL = 'AI-generated, may be wrong.';
export const NO_PROVIDER_TEXT = 'Set up an AI provider in Settings → AI';

export type Tier = 'free' | 'free-limited' | 'paid' | 'account';

export interface CatalogEntry {
  id: ProviderId;
  name: string;
  /** the honest one-line label under the name */
  label: string;
  tier: Tier;
  /** not called "supported" until verified against a live account */
  experimental: boolean;
  /** off until the user accepts the notice */
  gated: boolean;
  /** what the key is called */
  keyName: string;
  /** the placeholder of the key field (no real key shape is shown) */
  keyPlaceholder: string;
  /** the label of the "get a key" button */
  keyPageLabel: string;
  /** what happens to what you send, in one honest paragraph */
  blurb: string;
  /** something to know before you rely on it */
  note: string | null;
}

export const CATALOG: readonly CatalogEntry[] = [
  {
    id: 'anthropic',
    name: 'Anthropic',
    label: 'Paid, pay as you go (Anthropic)',
    tier: 'paid',
    experimental: false,
    gated: false,
    keyName: 'API key',
    keyPlaceholder: 'Paste your Anthropic API key',
    keyPageLabel: 'Get a key (console.anthropic.com)',
    blurb: 'Use an API key from console.anthropic.com, not a Claude subscription. Anthropic does not train on API data by default and deletes it within 30 days (longer if flagged for policy).',
    note: 'Set a spend limit in the Anthropic Console. Krovi limits how long each answer can be.',
  },
  {
    id: 'openai',
    name: 'OpenAI',
    label: 'Paid, pay as you go (OpenAI)',
    tier: 'paid',
    experimental: false,
    gated: false,
    keyName: 'API key',
    keyPlaceholder: 'Paste your OpenAI API key',
    keyPageLabel: 'Get a key (platform.openai.com)',
    blurb: 'Use an API key from platform.openai.com, not your ChatGPT login, and keep it to yourself. OpenAI does not train on API data by default; it may keep it for up to 30 days for abuse monitoring. Krovi turns off optional storage (store: false).',
    note: 'API use is billed to your OpenAI account. Set a spend limit in the OpenAI dashboard.',
  },
  {
    id: 'gemini',
    name: 'Google Gemini',
    label: 'Free tier available (Google AI Studio)',
    tier: 'free',
    experimental: false,
    gated: true,
    keyName: 'API key',
    keyPlaceholder: 'Paste your Google AI Studio key',
    keyPageLabel: 'Get a key (Google AI Studio)',
    blurb: 'You create your own key in Google AI Studio. On the free tier, Google may use what you send, including screenshots and answers, to improve its products, and people at Google may read it. Keys on a billing-enabled project are handled under Google’s paid terms.',
    note: 'A valid key does not guarantee access: Google decides free-tier access per project and region.',
  },
  {
    id: 'groq',
    name: 'Groq',
    label: 'Free tier available, rate-limited (Groq)',
    tier: 'free-limited',
    experimental: false,
    gated: false,
    keyName: 'API key',
    keyPlaceholder: 'Paste your Groq API key',
    keyPageLabel: 'Get a key (console.groq.com)',
    blurb: 'Krovi sends your text and the image you select straight to Groq using your own key. Groq states it does not train on your inputs or outputs and does not keep them by default (it may log them for up to 30 days to fix errors or investigate abuse). Data is stored in the United States.',
    note: 'The free plan is small (8K tokens/min): too small for long-document summaries, and about 55 image questions a day. The image model is a Groq preview model that can be withdrawn without notice.',
  },
  {
    id: 'snowflake',
    name: 'Snowflake Cortex',
    label: 'Your Snowflake account · billed by Snowflake',
    tier: 'account',
    experimental: true,
    gated: false,
    keyName: 'Programmatic access token',
    keyPlaceholder: 'Paste your Snowflake programmatic access token',
    keyPageLabel: 'How to create a token (Snowflake docs)',
    blurb: 'Experimental. Requests go to your own Snowflake account (the URL you enter), billed and governed by your Snowflake agreement. Snowflake runs the models inside its own service; if your account allows cross-region inference, a request can be processed in another region. Model availability depends on your region and that setting.',
    note: 'Not verified against a live account yet. Your Snowflake network policy must allow this PC.',
  },
];

export function catalogEntry(id: ProviderId): CatalogEntry {
  const e = CATALOG.find((c) => c.id === id);
  if (!e) throw new Error(`unknown provider ${id}`);
  return e;
}

/** The suggested model per provider when the provider cannot list its own (and when listing fails). */
export const DEFAULT_MODELS: Record<ProviderId, string> = {
  anthropic: 'claude-haiku-5-5',
  openai: 'gpt-6-luna',
  gemini: 'gemini-3.5-flash-lite',
  groq: 'openai/gpt-oss-120b',
  snowflake: 'llama3.1-8b',
};

// ---------------------------------------------------------------------------------------
// Models: banned families never appear (AGENTS.md); DeepSeek is not in v1
// ---------------------------------------------------------------------------------------

/** Model families that must never be shown, chosen or sent to. Matched anywhere in the id. */
export const BANNED_MODEL_MARKERS = ['muse-spark', 'muse-wonder', 'muse-glimmer'] as const;
export const EXCLUDED_MODEL_MARKERS = ['deepseek'] as const;

export function isBannedModel(id: string): boolean {
  const l = id.toLowerCase();
  return BANNED_MODEL_MARKERS.some((m) => l.includes(m));
}

/** Banned, or not offered in v1. */
export function isExcludedModel(id: string): boolean {
  const l = id.toLowerCase();
  return isBannedModel(l) || EXCLUDED_MODEL_MARKERS.some((m) => l.includes(m));
}

/** Defence in depth on top of the backend's filter: drop banned/excluded/odd entries, de-duplicate. */
export function cleanModelList(list: unknown): ModelInfo[] {
  if (!Array.isArray(list)) return [];
  const seen = new Set<string>();
  const out: ModelInfo[] = [];
  for (const item of list) {
    if (!item || typeof item !== 'object') continue;
    const o = item as Record<string, unknown>;
    const id = typeof o.id === 'string' ? o.id.trim() : '';
    if (!id || id.length > 80 || isExcludedModel(id) || seen.has(id)) continue;
    seen.add(id);
    out.push({ id, name: typeof o.name === 'string' && o.name.trim() ? o.name.trim() : null });
  }
  return out;
}

export interface ModelOption {
  id: string;
  label: string;
}

/**
 * What the model dropdown shows: the provider's list (or, if listing failed, just the default), with the
 * current choice always present - marked when it is not in the list - and the default marked.
 */
export function modelOptions(models: ModelInfo[], current: string, defaultModel: string): ModelOption[] {
  const list = cleanModelList(models);
  const out: ModelOption[] = list.map((m) => ({ id: m.id, label: `${m.name && m.name !== m.id ? `${m.name} · ` : ''}${m.id}${m.id === defaultModel ? ' (default)' : ''}` }));
  const has = (id: string) => out.some((o) => o.id === id);
  if (!has(defaultModel) && !isExcludedModel(defaultModel)) out.unshift({ id: defaultModel, label: `${defaultModel} (default)` });
  if (current && !has(current) && !isExcludedModel(current)) out.push({ id: current, label: `${current} (not in your list)` });
  return out;
}

// ---------------------------------------------------------------------------------------
// Keys and addresses: shape checks for the form (Rust checks again; nothing is echoed)
// ---------------------------------------------------------------------------------------

/** Which provider a key plainly belongs to, by its well-known prefix. Unknown shapes return null. */
export function keyFamily(key: string): ProviderId | null {
  if (key.startsWith('sk-ant-')) return 'anthropic';
  if (key.startsWith('gsk_')) return 'groq';
  if (key.startsWith('AIza')) return 'gemini';
  if (key.startsWith('sk-')) return 'openai';
  return null;
}

/** Why `raw` cannot be a key for `provider` (one plain sentence, never repeating the key), or null. */
export function keyShapeProblem(provider: ProviderId, raw: string): string | null {
  const key = raw.trim().replace(/^["']+|["']+$/g, '');
  if (!key) return 'Paste your key first.';
  if (/[^\x21-\x7e]/.test(key)) return 'A key has no spaces or line breaks. Copy the whole key again.';
  const max = provider === 'snowflake' ? 2048 : 400;
  if (key.length < 16 || key.length > max) return 'That doesn’t look like a full key. Copy the whole key and paste it again.';
  const fam = keyFamily(key);
  if (fam && fam !== provider) return `That looks like a ${catalogEntry(fam).name} key. Paste it into the ${catalogEntry(fam).name} card instead.`;
  return null;
}

export const SNOWFLAKE_SUFFIX = '.snowflakecomputing.com';

/** Is this a Snowflake account host (lowercase DNS labels, then .snowflakecomputing.com)? */
export function snowflakeHostOk(host: string): boolean {
  if (!host.endsWith(SNOWFLAKE_SUFFIX) || host.length > 253) return false;
  const prefix = host.slice(0, -SNOWFLAKE_SUFFIX.length);
  if (!prefix) return false;
  return prefix.split('.').every((l) => l.length > 0 && l.length <= 63 && !l.startsWith('-') && !l.endsWith('-') && /^[a-z0-9-]+$/.test(l));
}

export type AccountUrlCheck = { ok: true; url: string } | { ok: false; error: string };

/**
 * The same rules as the backend: https only, host ends in .snowflakecomputing.com, no path, query,
 * port or user info. A bare host gets https://. The error never repeats the input (it might be a token).
 */
export function checkAccountUrl(raw: string): AccountUrlCheck {
  const help = 'Enter your Snowflake account URL, like https://myorg-myaccount.snowflakecomputing.com';
  const s = raw.trim().replace(/\/+$/, '').toLowerCase();
  if (!s || s.length > 270 || /[\s\u0000-\u001f\u007f]/.test(s)) return { ok: false, error: help };
  let host = s;
  if (s.startsWith('https://')) host = s.slice('https://'.length);
  else if (s.includes('://')) return { ok: false, error: 'The Snowflake account URL must start with https://' };
  if (/[/?#@:\\]/.test(host)) return { ok: false, error: help };
  if (!snowflakeHostOk(host)) return { ok: false, error: 'That isn’t a Snowflake account URL. It must end in .snowflakecomputing.com.' };
  return { ok: true, url: `https://${host}` };
}

// ---------------------------------------------------------------------------------------
// The Gemini notice (docs/product/AI_PROVIDERS.md 9.4). Changing this wording means bumping
// GEMINI_CONSENT_VERSION here and in krovi-net's ai_settings, which asks everyone again.
// ---------------------------------------------------------------------------------------

export const GEMINI_CONSENT_VERSION = 1;

export const GEMINI_NOTICE = {
  title: 'Use your own Gemini key',
  body: 'Krovi does not provide Gemini access. You create your own key in Google AI Studio, and your use of the Gemini API is an agreement between you and Google. Krovi never sees your key: it stays in Windows Credential Manager on this PC, and requests go straight from this PC to Google.',
  adult: 'I am 18 or older, and I created this key myself with my own Google account.',
  terms: 'I have read the Gemini API Additional Terms. Google says the Gemini API is “for developers building with Google AI models for professional or business purposes, not for consumer use”. I am responsible for using my key in line with Google’s terms, and Google may restrict my key or project.',
  freeTier: 'If my key is on the free tier, Google may use what I send, including screenshots and answers, to improve its products, and people at Google may read it. I will not send sensitive, confidential or other people’s personal content with a free key. Keys on a billing-enabled project are handled under Google’s paid terms.',
  regionLabel: 'Where I use Krovi',
  regionRestricted: 'EEA, UK or Switzerland',
  regionElsewhere: 'Elsewhere',
  billing: 'My key’s Google project has billing enabled. (Google’s terms say only paid services may be used when making API clients available to users in these regions.)',
  footnote: 'A valid key does not guarantee access. Google decides free-tier access per project and region. If Krovi reports a problem, fix it in AI Studio; creating more keys will not help.',
  perSend: 'If your key is on the free tier, Google may use and review this image.',
} as const;

export type ConsentRegion = 'restricted' | 'elsewhere';

export interface ConsentChecks {
  adult: boolean;
  terms: boolean;
  freeTier: boolean;
  region: ConsentRegion | null;
  billing: boolean;
}

export const EMPTY_CHECKS: ConsentChecks = { adult: false, terms: false, freeTier: false, region: null, billing: false };

/** The notice can be accepted only when every box that applies is ticked and the region is chosen. */
export function consentComplete(c: ConsentChecks): boolean {
  if (!c.adult || !c.terms || !c.freeTier || c.region === null) return false;
  return c.region === 'elsewhere' || c.billing;
}

const EEA_UK_CH = new Set(['AT', 'BE', 'BG', 'HR', 'CY', 'CZ', 'DK', 'EE', 'FI', 'FR', 'DE', 'GR', 'HU', 'IE', 'IT', 'LV', 'LT', 'LU', 'MT', 'NL', 'PL', 'PT', 'RO', 'SK', 'SI', 'ES', 'SE', 'IS', 'LI', 'NO', 'GB', 'CH']);

/** A starting guess for the region question from a locale such as `en-GB` (the user can change it). */
export function guessConsentRegion(locale: string | null | undefined): ConsentRegion | null {
  const m = /^[a-z]{2,3}[-_]([A-Za-z]{2})(?:[-_]|$)/.exec((locale ?? '').trim());
  if (!m) return null;
  return EEA_UK_CH.has(m[1].toUpperCase()) ? 'restricted' : 'elsewhere';
}

// ---------------------------------------------------------------------------------------
// Test connection: separate statuses, distinct copy per kind
// ---------------------------------------------------------------------------------------

export interface FailureCopy {
  title: string;
  advice: string;
}

/** Distinct words for each way a request can fail; authentication, billing, quota, rate limit, eligibility and network all differ. */
export function failureCopy(kind: AiFailureKind): FailureCopy {
  switch (kind) {
    case 'auth':
      return { title: 'The key was not accepted', advice: 'Check that you pasted the whole key, or create a new one with the provider, then save it again.' };
    case 'billing':
      return { title: 'Billing problem', advice: 'Add credit or fix the payment method (or the spending limit) with the provider. Krovi does not retry this.' };
    case 'quota':
      return { title: 'Quota used up', advice: 'A daily, monthly or free-tier limit is used up. Waiting a minute will not help: wait for the reset or upgrade the plan.' };
    case 'rate-limit':
      return { title: 'Too many requests too fast', advice: 'The key works. Wait a minute and test again.' };
    case 'eligibility':
      return { title: 'This account, project or region can’t use that', advice: 'The key is valid, but the provider does not allow this here. That is decided by the provider, not Krovi, and making more keys will not help.' };
    case 'model':
      return { title: 'That model isn’t available', advice: 'Pick another model below.' };
    case 'blocked':
      return { title: 'The provider declined this one', advice: 'Its safety filter stepped in. Try different wording.' };
    case 'unavailable':
      return { title: 'The provider is having trouble', advice: 'This is on their side. Try again in a minute.' };
    case 'network':
      return { title: 'Can’t reach the provider', advice: 'Check your internet connection, firewall or VPN, then test again.' };
    case 'consent-required':
      return { title: 'Needs your OK first', advice: 'Read and accept the notice on this page.' };
    case 'no-provider':
      return { title: 'Not set up yet', advice: 'Save a key (and any other detail this provider needs) first.' };
    default:
      return { title: 'Something else went wrong', advice: 'Try again. If it keeps happening, the provider’s own message below may say why.' };
  }
}

export type RowState = 'ok' | 'problem' | 'unknown';

export interface StatusRow {
  id: 'auth' | 'billing' | 'quota' | 'eligibility' | 'model';
  label: string;
  state: RowState;
  text: string;
}

const PAST_AUTH: AiFailureKind[] = ['billing', 'quota', 'rate-limit', 'eligibility', 'model', 'blocked'];

/**
 * The result of a test as separate statuses - Authentication, Billing, Quota/rate, Eligibility, Model - so a key
 * that works but whose project is blocked reads as exactly that. A request that never got an answer (network,
 * the provider down) leaves everything "not checked".
 */
export function statusRows(r: TestReport): StatusRow[] {
  const kind = r.kind;
  const keyStep = r.steps.find((s) => s.id === 'key');
  // the provider recognised the key if the list loaded, the test ran, or it complained about something else
  const keyKnown = r.ok || keyStep?.ok === true || (kind !== null && PAST_AUTH.includes(kind));
  const row = (id: StatusRow['id'], label: string, bad: AiFailureKind[], okText: string, badText: string): StatusRow => {
    if (kind !== null && bad.includes(kind)) return { id, label, state: 'problem', text: badText };
    if (r.ok) return { id, label, state: 'ok', text: okText };
    return { id, label, state: 'unknown', text: 'Not checked' };
  };
  const auth: StatusRow =
    kind === 'auth'
      ? { id: 'auth', label: 'Authentication', state: 'problem', text: failureCopy('auth').title }
      : keyKnown
        ? { id: 'auth', label: 'Authentication', state: 'ok', text: 'Key accepted' }
        : { id: 'auth', label: 'Authentication', state: 'unknown', text: 'Not checked' };
  return [
    auth,
    row('billing', 'Billing', ['billing'], 'No billing problem found', failureCopy('billing').title),
    row('quota', 'Quota and rate', ['quota', 'rate-limit'], 'Within limits', failureCopy(kind === 'rate-limit' ? 'rate-limit' : 'quota').title),
    row('eligibility', 'Eligibility (plan, region, policy)', ['eligibility'], 'Allowed', failureCopy('eligibility').title),
    row('model', 'Model', ['model'], 'Available', failureCopy('model').title),
  ];
}

export function formatTokens(n: number): string {
  return new Intl.NumberFormat('en-US').format(Math.max(0, Math.round(Number.isFinite(n) ? n : 0)));
}

/** "3 requests, 1,204 tokens since Krovi started" - or null when nothing was sent yet. */
export function usageLine(u: UsageInfo): string | null {
  if (u.requests <= 0) return null;
  const tokens = u.inputTokens + u.outputTokens;
  return `${u.requests} ${u.requests === 1 ? 'request' : 'requests'}${tokens > 0 ? `, ${formatTokens(tokens)} tokens` : ''} since Krovi started`;
}

// ---------------------------------------------------------------------------------------
// Normalising what Rust sends (field by field: nothing unknown gets through)
// ---------------------------------------------------------------------------------------

const FAILURE_KINDS: AiFailureKind[] = ['auth', 'billing', 'quota', 'rate-limit', 'eligibility', 'model', 'blocked', 'unavailable', 'network', 'other', 'no-provider', 'consent-required', 'expired', 'inactive', 'not-allowed'];

const num = (v: unknown, fallback = 0): number => (typeof v === 'number' && Number.isFinite(v) ? v : fallback);
const numOrNull = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null);
const strOr = (v: unknown, fallback: string): string => (typeof v === 'string' ? v : fallback);
const strOrNull = (v: unknown): string | null => (typeof v === 'string' ? v : null);
const kindOrNull = (v: unknown): AiFailureKind | null => (FAILURE_KINDS.includes(v as AiFailureKind) ? (v as AiFailureKind) : null);
const isProviderId = (v: unknown): v is ProviderId => PROVIDER_IDS.includes(v as ProviderId);

function obj(v: unknown): Record<string, unknown> {
  return v && typeof v === 'object' ? (v as Record<string, unknown>) : {};
}

/** The last four characters of a key are the most a provider card may show; anything longer is dropped. */
function hint(v: unknown): string | null {
  return typeof v === 'string' && v.length > 0 && v.length <= 4 ? v : null;
}

function normalizeUsage(raw: unknown): UsageInfo {
  const u = obj(raw);
  const le = obj(u.lastError);
  const kind = kindOrNull(le.kind);
  return {
    requests: num(u.requests),
    inputTokens: num(u.inputTokens),
    outputTokens: num(u.outputTokens),
    lastError: kind ? { kind, at: num(le.at) } : null,
  };
}

function normalizeProvider(raw: unknown): AiProviderState | null {
  const p = obj(raw);
  const id = p.id;
  if (!isProviderId(id)) return null;
  const gate: Gate | null = p.gate === 'consent-required' || p.gate === 'account-required' ? p.gate : null;
  const def = DEFAULT_MODELS[id];
  return {
    id,
    name: strOr(p.name, catalogEntry(id).name),
    hasKey: p.hasKey === true,
    keyHint: hint(p.keyHint),
    ready: p.ready === true,
    gate,
    model: strOr(p.model, def),
    defaultModel: strOr(p.defaultModel, def),
    visionModel: strOr(p.visionModel, def),
    supportsModelList: p.supportsModelList !== false,
    needsAccountUrl: p.needsAccountUrl === true,
    accountUrl: strOrNull(p.accountUrl),
    usage: normalizeUsage(p.usage),
  };
}

export function normalizeStatus(raw: unknown): AiStatus {
  const r = obj(raw);
  const g = obj(r.gemini);
  const providers: AiProviderState[] = [];
  for (const item of Array.isArray(r.providers) ? r.providers : []) {
    const p = normalizeProvider(item);
    if (p) providers.push(p);
  }
  const sel = r.selected;
  const selected: ProviderId | null = isProviderId(sel) && providers.some((p) => p.id === sel) ? sel : null;
  return {
    providers,
    selected,
    model: selected && typeof r.model === 'string' ? r.model : null,
    gemini: {
      accepted: g.accepted === true,
      acceptedAt: numOrNull(g.acceptedAt),
      version: num(g.version),
      currentVersion: num(g.currentVersion, GEMINI_CONSENT_VERSION),
    },
  };
}

function normalizeSteps(raw: unknown): TestStep[] {
  if (!Array.isArray(raw)) return [];
  const out: TestStep[] = [];
  for (const item of raw) {
    const s = obj(item);
    const id = s.id;
    if (id !== 'key' && id !== 'inference') continue;
    out.push({ id, ok: s.ok === true, kind: kindOrNull(s.kind), message: strOr(s.message, '') });
  }
  return out;
}

export function normalizeReport(raw: unknown): TestReport {
  const r = obj(raw);
  return {
    ok: r.ok === true,
    kind: kindOrNull(r.kind),
    message: strOr(r.message, ''),
    status: numOrNull(r.status),
    detail: strOrNull(r.detail),
    model: strOr(r.model, ''),
    models: Array.isArray(r.models) ? cleanModelList(r.models) : null,
    steps: normalizeSteps(r.steps),
    notes: Array.isArray(r.notes) ? r.notes.filter((n): n is string => typeof n === 'string') : [],
    inputTokens: numOrNull(r.inputTokens),
    outputTokens: numOrNull(r.outputTokens),
  };
}

// ---------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------

/** The browser demo: the providers are listed, nothing has a key, nothing can be done. */
export function demoStatus(): AiStatus {
  return {
    providers: CATALOG.map((c) => ({
      id: c.id,
      name: c.name,
      hasKey: false,
      keyHint: null,
      ready: false,
      gate: null,
      model: DEFAULT_MODELS[c.id],
      defaultModel: DEFAULT_MODELS[c.id],
      visionModel: DEFAULT_MODELS[c.id],
      supportsModelList: c.id !== 'snowflake',
      needsAccountUrl: c.id === 'snowflake',
      accountUrl: null,
      usage: { requests: 0, inputTokens: 0, outputTokens: 0, lastError: null },
    })),
    selected: null,
    model: null,
    gemini: { accepted: false, acceptedAt: null, version: 0, currentVersion: GEMINI_CONSENT_VERSION },
  };
}

const statusStore = writable<AiStatus | null>(null);
/** Providers, the choice and the Gemini notice, as of the last call. Never holds a key. */
export const aiStatus: Readable<AiStatus | null> = { subscribe: statusStore.subscribe };

function publish(st: AiStatus): AiStatus {
  statusStore.set(st);
  return st;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    // The message is Rust's plain sentence (it never contains a key); `errorText` just unwraps it.
    throw new AiError(toFailure(e));
  }
}

function requireDesktop(): void {
  if (!isTauri()) throw new Error(DESKTOP_ONLY);
}

// ---------------------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------------------

/** Providers (whether a key is saved, never the key), the choice, and the Gemini notice. Sends nothing. */
export async function loadAi(): Promise<AiStatus> {
  if (!isTauri()) return publish(demoStatus());
  return publish(normalizeStatus(await call<unknown>('ai_status')));
}

/**
 * Save a key. Rust trims and checks it, then stores it in Windows Credential Manager. The caller clears its
 * input afterwards; this function keeps nothing and returns only the new status.
 */
export async function saveKey(provider: ProviderId, key: string): Promise<AiStatus> {
  requireDesktop();
  const problem = keyShapeProblem(provider, key);
  if (problem) throw new AiError({ kind: 'other', message: problem, detail: null, status: null });
  return publish(normalizeStatus(await call<unknown>('ai_set_key', { provider, key })));
}

export async function removeKey(provider: ProviderId): Promise<AiStatus> {
  requireDesktop();
  return publish(normalizeStatus(await call<unknown>('ai_remove_key', { provider })));
}

/** Save (or clear, with null) the account URL of a provider that has one (Snowflake). */
export async function setAccountUrl(provider: ProviderId, url: string | null): Promise<AiStatus> {
  requireDesktop();
  return publish(normalizeStatus(await call<unknown>('ai_set_account', { provider, url })));
}

/** Test the saved key: it is accepted, then a one-token request runs. Sends one or two tiny requests. */
export async function testConnection(provider: ProviderId): Promise<TestReport> {
  requireDesktop();
  return normalizeReport(await call<unknown>('ai_test', { provider }));
}

export interface ModelList {
  models: ModelInfo[];
  /** true when the provider gave this list; false when it is only the default */
  listed: boolean;
  error: AiFailure | null;
}

/** The models the key can use. If listing fails (or the demo), just the provider's default - never nothing. */
export async function loadModels(provider: ProviderId): Promise<ModelList> {
  const fallback: ModelInfo[] = [{ id: DEFAULT_MODELS[provider], name: null }];
  if (!isTauri()) return { models: fallback, listed: false, error: null };
  try {
    const list = cleanModelList(await call<unknown>('ai_models', { provider }));
    return list.length ? { models: list, listed: true, error: null } : { models: fallback, listed: false, error: null };
  } catch (e) {
    return { models: fallback, listed: false, error: toFailure(e) };
  }
}

/** Choose the provider features use, and optionally its model (undefined keeps it, '' resets it). */
export async function selectProvider(provider: ProviderId, model?: string): Promise<AiStatus> {
  requireDesktop();
  if (model && isExcludedModel(model)) throw new AiError({ kind: 'model', message: 'That model isn’t available in Krovi.', detail: null, status: null });
  return publish(normalizeStatus(await call<unknown>('ai_select', { provider, model: model ?? null })));
}

/** Record that the user accepted (or withdrew) the Gemini notice. */
export async function setGeminiConsent(accepted: boolean): Promise<AiStatus> {
  requireDesktop();
  return publish(normalizeStatus(await call<unknown>('ai_set_gemini_consent', { accepted })));
}

/** Open the page where the user creates a key, in their browser (Rust has a fixed list of pages). */
export async function openKeyPage(provider: ProviderId): Promise<void> {
  requireDesktop();
  try {
    await invoke<void>('ai_open_keys_page', { provider });
  } catch (e) {
    throw new Error(errorText(e));
  }
}

/**
 * Send a text prompt to the chosen provider (for features such as summaries; the feature's own screen must
 * have shown the user what is sent and to whom). Rejects with an `AiError`.
 */
export async function complete(prompt: string, opts: { system?: string; maxTokens?: number } = {}): Promise<AiCompletion> {
  if (!isTauri()) throw new AiError({ kind: 'no-provider', message: `${NO_PROVIDER_TEXT} (the desktop app).`, detail: null, status: null });
  const raw = obj(await call<unknown>('ai_complete', { prompt, system: opts.system ?? null, maxTokens: opts.maxTokens ?? null }));
  const pid = raw.provider;
  return {
    text: strOr(raw.text, ''),
    truncated: raw.truncated === true,
    provider: isProviderId(pid) ? pid : 'anthropic',
    providerName: strOr(raw.providerName, ''),
    model: strOr(raw.model, ''),
    inputTokens: numOrNull(raw.inputTokens),
    outputTokens: numOrNull(raw.outputTokens),
    label: strOr(raw.label, AI_OUTPUT_LABEL),
  };
}

/** Test hook. */
export const __test = {
  reset(): void {
    statusStore.set(null);
  },
};
