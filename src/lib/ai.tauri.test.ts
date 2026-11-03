// Settings -> AI inside the Tauri shell: the bridge is mocked, so this proves the command names and
// arguments, that errors keep their kind, and above all that a key is handed over once and then
// found nowhere: not in a returned status, not in localStorage, not in the console.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
const errors = new Map<string, unknown>();
const replies = new Map<string, unknown>();

const provider = (id: string, over: Record<string, unknown> = {}) => ({
  id,
  name: id,
  hasKey: false,
  keyHint: null,
  ready: false,
  gate: null,
  model: 'm',
  defaultModel: 'm',
  visionModel: 'm',
  supportsModelList: true,
  needsAccountUrl: id === 'snowflake',
  accountUrl: null,
  usage: { requests: 0, inputTokens: 0, outputTokens: 0, lastError: null },
  ...over,
});

const statusJson = (over: Record<string, unknown> = {}) => ({
  providers: ['anthropic', 'openai', 'gemini', 'groq', 'snowflake'].map((id) => provider(id, id === 'openai' ? { hasKey: true, keyHint: 'CDEF', ready: true } : {})),
  selected: 'openai',
  model: 'gpt-6-luna',
  gemini: { accepted: false, acceptedAt: null, version: 0, currentVersion: 1 },
  ...over,
});

vi.mock('./bridge', () => ({
  isTauri: () => true,
  listen: async () => () => {},
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    calls.push({ cmd, args });
    if (errors.has(cmd)) throw errors.get(cmd);
    if (replies.has(cmd)) return replies.get(cmd);
    return undefined;
  },
}));

const cmds = () => calls.map((c) => c.cmd);

async function fresh() {
  vi.resetModules();
  return await import('./ai');
}

const KEY = 'sk-test-0123456789abcdefABCDEF';

beforeEach(() => {
  calls.length = 0;
  errors.clear();
  replies.clear();
  replies.set('ai_status', statusJson());
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('tauri: nothing happens by itself', () => {
  it('importing and loading the status calls ai_status and nothing else', async () => {
    const ai = await fresh();
    await ai.loadAi();
    expect(cmds()).toEqual(['ai_status']);
  });

  it('loads and normalises the status', async () => {
    const ai = await fresh();
    const st = await ai.loadAi();
    expect(st.providers.map((p) => [p.id, p.hasKey])).toEqual([
      ['anthropic', false],
      ['openai', true],
      ['gemini', false],
      ['groq', false],
      ['snowflake', false],
    ]);
    expect(st.selected).toBe('openai');
    expect(get(ai.aiStatus)).toEqual(st);
  });
});

describe('tauri: keys', () => {
  it('passes the key to Rust once and never shows it again', async () => {
    const ai = await fresh();
    const stored: string[] = [];
    vi.stubGlobal('localStorage', { setItem: (_k: string, v: string) => stored.push(v), getItem: () => null, removeItem: () => {} });
    const spies = (['log', 'info', 'warn', 'error', 'debug'] as const).map((m) => vi.spyOn(console, m).mockImplementation(() => {}));

    // even if a buggy backend echoed the key back, it must not reach the store
    replies.set('ai_set_key', statusJson({ providers: [provider('openai', { hasKey: true, ready: true, keyHint: 'CDEF', key: KEY })], key: KEY }));
    const st = await ai.saveKey('openai', `  ${KEY}  `);

    expect(calls.at(-1)).toEqual({ cmd: 'ai_set_key', args: { provider: 'openai', key: `  ${KEY}  ` } });
    expect(JSON.stringify(st)).not.toContain(KEY);
    expect(JSON.stringify(get(ai.aiStatus))).not.toContain(KEY);
    expect(stored).toEqual([]);
    for (const s of spies) expect(s).not.toHaveBeenCalled();
  });

  it('does not even ask Rust to save a key that is plainly wrong', async () => {
    const ai = await fresh();
    await expect(ai.saveKey('openai', 'short')).rejects.toMatchObject({ kind: 'other' });
    await expect(ai.saveKey('openai', 'sk-ant-test-0123456789abcdefABCDEF')).rejects.toThrow(/Anthropic/);
    await expect(ai.saveKey('openai', '')).rejects.toThrow('Paste your key first.');
    expect(cmds()).toEqual([]);
  });

  it('an error from Rust keeps its kind and never contains the key', async () => {
    const ai = await fresh();
    errors.set('ai_set_key', { kind: 'consent-required', message: 'Gemini is off until you accept the notice in Settings → AI.', detail: null, status: null });
    const e = await ai.saveKey('gemini', 'AIzaTest0123456789abcdefABCDEF-xyz').catch((x: unknown) => x);
    expect(e).toBeInstanceOf(ai.AiError);
    expect((e as InstanceType<typeof ai.AiError>).kind).toBe('consent-required');
    expect(JSON.stringify((e as Error).message)).not.toContain('AIza');
  });

  it('removes a key and saves the Snowflake account URL', async () => {
    const ai = await fresh();
    await ai.removeKey('openai');
    expect(calls.at(-1)).toEqual({ cmd: 'ai_remove_key', args: { provider: 'openai' } });
    await ai.setAccountUrl('snowflake', 'https://myorg-myacct.snowflakecomputing.com');
    expect(calls.at(-1)).toEqual({ cmd: 'ai_set_account', args: { provider: 'snowflake', url: 'https://myorg-myacct.snowflakecomputing.com' } });
    await ai.setAccountUrl('snowflake', null);
    expect(calls.at(-1)).toEqual({ cmd: 'ai_set_account', args: { provider: 'snowflake', url: null } });
  });
});

describe('tauri: test connection', () => {
  const eligibility = {
    ok: false,
    kind: 'eligibility',
    message: "This key's Google project can't use the Gemini API here (free tier not available for this project or region). Enable billing on the project in Google AI Studio, or try another provider.",
    status: 403,
    detail: 'Your project has been denied access.',
    model: 'gemini-3.5-flash-lite',
    models: [{ id: 'gemini-3.5-flash-lite', name: 'Gemini 3.5 Flash-Lite' }, { id: 'muse-spark-1.3', name: null }],
    steps: [
      { id: 'key', ok: true, kind: null, message: 'Google Gemini accepted the key.' },
      { id: 'inference', ok: false, kind: 'eligibility', message: 'x' },
    ],
    notes: [],
    inputTokens: null,
    outputTokens: null,
  };

  it('runs ai_test and keeps the two steps apart', async () => {
    const ai = await fresh();
    replies.set('ai_test', eligibility);
    const r = await ai.testConnection('gemini');
    expect(calls.at(-1)).toEqual({ cmd: 'ai_test', args: { provider: 'gemini' } });
    expect(r.ok).toBe(false);
    expect(r.kind).toBe('eligibility');
    expect(r.steps.map((s) => [s.id, s.ok])).toEqual([['key', true], ['inference', false]]);
    expect(r.models?.map((m) => m.id)).toEqual(['gemini-3.5-flash-lite']);
    const rows = Object.fromEntries(ai.statusRows(r).map((x) => [x.id, x.state]));
    expect(rows.auth).toBe('ok');
    expect(rows.eligibility).toBe('problem');
  });

  it('turns a Rust error into an AiError with the right kind', async () => {
    const ai = await fresh();
    errors.set('ai_test', { kind: 'consent-required', message: 'Gemini is off until you accept the notice in Settings → AI.', detail: null, status: null });
    await expect(ai.testConnection('gemini')).rejects.toMatchObject({ kind: 'consent-required' });
    errors.set('ai_test', 'a plain string');
    await expect(ai.testConnection('gemini')).rejects.toMatchObject({ kind: 'other', message: 'a plain string' });
  });
});

describe('tauri: models and choosing', () => {
  it('lists models without the banned families', async () => {
    const ai = await fresh();
    replies.set('ai_models', [{ id: 'openai/gpt-oss-120b', name: null }, { id: 'meta-models/muse-spark-open', name: null }, { id: 'qwen/qwen3.8-27b', name: null }]);
    const m = await ai.loadModels('groq');
    expect(calls.at(-1)).toEqual({ cmd: 'ai_models', args: { provider: 'groq' } });
    expect(m.listed).toBe(true);
    expect(m.models.map((x) => x.id)).toEqual(['openai/gpt-oss-120b', 'qwen/qwen3.8-27b']);
  });

  it('falls back to the default model when listing fails or comes back empty', async () => {
    const ai = await fresh();
    errors.set('ai_models', { kind: 'network', message: "Couldn't reach Groq.", detail: null, status: null });
    const failed = await ai.loadModels('groq');
    expect(failed.listed).toBe(false);
    expect(failed.models).toEqual([{ id: 'openai/gpt-oss-120b', name: null }]);
    expect(failed.error?.kind).toBe('network');

    errors.clear();
    replies.set('ai_models', []);
    const empty = await ai.loadModels('openai');
    expect(empty.models).toEqual([{ id: 'gpt-6-luna', name: null }]);
    replies.set('ai_models', [{ id: 'muse-spark-1.3' }]);
    expect((await ai.loadModels('groq')).models).toEqual([{ id: 'openai/gpt-oss-120b', name: null }]);
  });

  it('selects a provider and model, and refuses a banned one without asking Rust', async () => {
    const ai = await fresh();
    await ai.selectProvider('groq', 'openai/gpt-oss-20b');
    expect(calls.at(-1)).toEqual({ cmd: 'ai_select', args: { provider: 'groq', model: 'openai/gpt-oss-20b' } });
    await ai.selectProvider('openai');
    expect(calls.at(-1)).toEqual({ cmd: 'ai_select', args: { provider: 'openai', model: null } });
    const before = calls.length;
    await expect(ai.selectProvider('groq', 'meta-models/muse-spark-open')).rejects.toMatchObject({ kind: 'model' });
    await expect(ai.selectProvider('groq', 'deepseek-r1')).rejects.toMatchObject({ kind: 'model' });
    expect(calls.length).toBe(before);
  });

  it('records the Gemini notice', async () => {
    const ai = await fresh();
    replies.set('ai_set_gemini_consent', statusJson({ gemini: { accepted: true, acceptedAt: 1_700_000_000_000, version: 1, currentVersion: 1 } }));
    const st = await ai.setGeminiConsent(true);
    expect(calls.at(-1)).toEqual({ cmd: 'ai_set_gemini_consent', args: { accepted: true } });
    expect(st.gemini).toEqual({ accepted: true, acceptedAt: 1_700_000_000_000, version: 1, currentVersion: 1 });
  });
});

describe('tauri: opening the key page and asking', () => {
  it('opens a key page through Rust only', async () => {
    const ai = await fresh();
    await ai.openKeyPage('gemini');
    expect(calls.at(-1)).toEqual({ cmd: 'ai_open_keys_page', args: { provider: 'gemini' } });
    errors.set('ai_open_keys_page', 'Windows could not open your browser.');
    await expect(ai.openKeyPage('gemini')).rejects.toThrow('Windows could not open your browser.');
  });

  it('sends a text prompt and labels the answer', async () => {
    const ai = await fresh();
    replies.set('ai_complete', { text: 'A summary.', truncated: false, provider: 'openai', providerName: 'OpenAI', model: 'gpt-6-luna', inputTokens: 9, outputTokens: 3, label: 'AI-generated, may be wrong.', extra: 1 });
    const a = await ai.complete('Summarise this', { system: 'Be brief.', maxTokens: 256 });
    expect(calls.at(-1)).toEqual({ cmd: 'ai_complete', args: { prompt: 'Summarise this', system: 'Be brief.', maxTokens: 256 } });
    expect(a).toEqual({ text: 'A summary.', truncated: false, provider: 'openai', providerName: 'OpenAI', model: 'gpt-6-luna', inputTokens: 9, outputTokens: 3, label: ai.AI_OUTPUT_LABEL });
    await ai.complete('Plain');
    expect(calls.at(-1)).toEqual({ cmd: 'ai_complete', args: { prompt: 'Plain', system: null, maxTokens: null } });
  });
});
