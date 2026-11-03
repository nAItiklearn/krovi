// Settings -> AI: the pure helpers and the browser demo (no Tauri). The Tauri wiring is in ai.tauri.test.ts.
import { beforeEach, describe, expect, it } from 'vitest';
import { get } from 'svelte/store';
import * as ai from './ai';
import { PROVIDER_IDS } from './circle';

const report = (over: Partial<ai.TestReport> = {}): ai.TestReport => ({
  ok: false,
  kind: null,
  message: '',
  status: null,
  detail: null,
  model: 'm',
  models: null,
  steps: [],
  notes: [],
  inputTokens: null,
  outputTokens: null,
  ...over,
});

describe('the provider catalog', () => {
  it('lists the five providers in the order the backend does', () => {
    expect(ai.CATALOG.map((c) => c.id)).toEqual(['anthropic', 'openai', 'gemini', 'groq', 'snowflake']);
    expect(ai.CATALOG.map((c) => c.id)).toEqual(PROVIDER_IDS);
  });

  it('labels each card honestly', () => {
    const label = (id: ai.ProviderId) => ai.catalogEntry(id).label;
    expect(label('gemini')).toBe('Free tier available (Google AI Studio)');
    expect(label('groq')).toBe('Free tier available, rate-limited (Groq)');
    expect(label('anthropic')).toBe('Paid, pay as you go (Anthropic)');
    expect(label('openai')).toBe('Paid, pay as you go (OpenAI)');
    expect(label('snowflake')).toBe('Your Snowflake account · billed by Snowflake');
  });

  it('gates Gemini, marks Snowflake experimental, and nothing else', () => {
    expect(ai.CATALOG.filter((c) => c.gated).map((c) => c.id)).toEqual(['gemini']);
    expect(ai.CATALOG.filter((c) => c.experimental).map((c) => c.id)).toEqual(['snowflake']);
  });

  it('warns that the Groq free plan is too small for long documents', () => {
    const note = ai.catalogEntry('groq').note ?? '';
    expect(note).toContain('8K tokens/min');
    expect(note).toContain('long-document summaries');
    expect(ai.catalogEntry('groq').note).toContain('preview');
  });

  it('tells the Gemini free-tier truth on the card', () => {
    const blurb = ai.catalogEntry('gemini').blurb;
    expect(blurb).toContain('Google may use what you send');
    expect(blurb).toContain('people at Google may read it');
  });

  it('says it is the user’s own account for Snowflake and never claims it is verified', () => {
    const s = ai.catalogEntry('snowflake');
    expect(s.blurb).toMatch(/^Experimental\./);
    expect(s.blurb).toContain('your Snowflake agreement');
    expect(s.note).toContain('Not verified against a live account');
    expect(s.keyName).toBe('Programmatic access token');
  });

  it('keeps the no-account line and the stored-in-Credential-Manager line word for word', () => {
    expect(ai.PRIVACY_LINE).toBe('Krovi has no account and no server. Requests go straight from this PC to the provider you choose. Local tools never need a key.');
    expect(ai.KEY_STORED_TEXT).toBe('Key saved in Windows Credential Manager');
    expect(ai.AI_OUTPUT_LABEL).toBe('AI-generated, may be wrong.');
  });

  it('never mentions a banned model or DeepSeek anywhere, and its default models are clean', () => {
    const text = JSON.stringify([ai.CATALOG, ai.GEMINI_NOTICE]).toLowerCase();
    for (const bad of [...ai.BANNED_MODEL_MARKERS, ...ai.EXCLUDED_MODEL_MARKERS]) expect(text).not.toContain(bad);
    for (const id of PROVIDER_IDS) expect(ai.isExcludedModel(ai.DEFAULT_MODELS[id])).toBe(false);
    expect(PROVIDER_IDS as string[]).not.toContain('deepseek');
  });
});

describe('banned models', () => {
  it('recognises the muse families anywhere in an id, in any case', () => {
    for (const id of ['muse-spark-1.2', 'muse-spark-1.3', 'meta-models/muse-spark-open', 'MUSE-SPARK', 'x-muse-wonder-2', 'models/muse-glimmer']) {
      expect(ai.isBannedModel(id), id).toBe(true);
      expect(ai.isExcludedModel(id), id).toBe(true);
    }
    for (const id of ['gpt-6-luna', 'claude-haiku-5-5', 'qwen/qwen3.8-27b', 'muse', 'spark']) expect(ai.isBannedModel(id), id).toBe(false);
    expect(ai.isExcludedModel('deepseek-r1')).toBe(true);
    expect(ai.isBannedModel('deepseek-r1')).toBe(false);
  });

  it('drops banned, excluded, duplicate and odd entries from a model list', () => {
    const list = ai.cleanModelList([
      { id: 'openai/gpt-oss-120b', name: 'GPT OSS 120B' },
      { id: 'meta-models/muse-spark-open' },
      { id: 'muse-spark-1.3', name: 'x' },
      { id: 'muse-wonder-1' },
      { id: 'muse-glimmer' },
      { id: 'deepseek-r1' },
      { id: 'openai/gpt-oss-120b' },
      { id: '  qwen/qwen3.8-27b  ' },
      { id: '' },
      { id: 'x'.repeat(81) },
      { id: 7 },
      null,
      'str',
    ]);
    expect(list).toEqual([
      { id: 'openai/gpt-oss-120b', name: 'GPT OSS 120B' },
      { id: 'qwen/qwen3.8-27b', name: null },
    ]);
    expect(ai.cleanModelList('nope')).toEqual([]);
    expect(ai.cleanModelList(undefined)).toEqual([]);
  });

  it('never puts a banned model in the dropdown, even as the current choice or the default', () => {
    const opts = ai.modelOptions([{ id: 'muse-spark-1.3', name: null }, { id: 'claude-haiku-5-5', name: null }], 'muse-spark-1.3', 'muse-wonder-1');
    expect(opts.map((o) => o.id)).toEqual(['claude-haiku-5-5']);
  });
});

describe('the model dropdown', () => {
  it('marks the default and the provider’s display names', () => {
    const opts = ai.modelOptions([{ id: 'claude-haiku-5-5', name: 'Claude Haiku 5.5' }, { id: 'claude-sonnet-5-5', name: null }], 'claude-sonnet-5-5', 'claude-haiku-5-5');
    expect(opts).toEqual([
      { id: 'claude-haiku-5-5', label: 'Claude Haiku 5.5 · claude-haiku-5-5 (default)' },
      { id: 'claude-sonnet-5-5', label: 'claude-sonnet-5-5' },
    ]);
  });

  it('always offers the default when the list failed, and flags a current model the list lacks', () => {
    expect(ai.modelOptions([], 'gpt-6-luna', 'gpt-6-luna')).toEqual([{ id: 'gpt-6-luna', label: 'gpt-6-luna (default)' }]);
    const opts = ai.modelOptions([{ id: 'gpt-6.1-sol', name: null }], 'gpt-old', 'gpt-6-luna');
    expect(opts.map((o) => o.id)).toEqual(['gpt-6-luna', 'gpt-6.1-sol', 'gpt-old']);
    expect(opts[2].label).toBe('gpt-old (not in your list)');
  });
});

describe('key shape (the form’s pre-check)', () => {
  const KEY = 'sk-test-0123456789abcdefABCDEF';
  it('accepts a plausible key and says what is wrong with a bad one, without repeating it', () => {
    expect(ai.keyShapeProblem('openai', KEY)).toBeNull();
    expect(ai.keyShapeProblem('openai', `  "${KEY}"  `)).toBeNull();
    expect(ai.keyShapeProblem('openai', '')).toBe('Paste your key first.');
    expect(ai.keyShapeProblem('openai', 'short')).toMatch(/full key/);
    expect(ai.keyShapeProblem('openai', 'sk-has space inside it long')).toMatch(/no spaces/);
    expect(ai.keyShapeProblem('openai', 'k'.repeat(401))).toMatch(/full key/);
    expect(ai.keyShapeProblem('openai', 'sk-has space inside it long')).not.toContain('sk-has');
  });

  it('catches a key pasted into the wrong card but never requires a prefix', () => {
    expect(ai.keyShapeProblem('openai', 'sk-ant-test-0123456789abcdefABCDEF')).toContain('Anthropic');
    expect(ai.keyShapeProblem('anthropic', KEY)).toContain('OpenAI');
    expect(ai.keyShapeProblem('gemini', 'gsk_test0123456789abcdefABCDEF')).toContain('Groq');
    expect(ai.keyShapeProblem('groq', 'AIzaTest0123456789abcdefABCDEF-xyz')).toContain('Google Gemini');
    expect(ai.keyShapeProblem('gemini', 'AIzaTest0123456789abcdefABCDEF-xyz')).toBeNull();
    expect(ai.keyShapeProblem('gemini', 'AQ.Ab8RN6Kexample0123456789')).toBeNull();
    expect(ai.keyShapeProblem('groq', '0123456789abcdefghij')).toBeNull();
  });

  it('lets a Snowflake token be longer than an API key', () => {
    expect(ai.keyShapeProblem('snowflake', 'p'.repeat(900))).toBeNull();
    expect(ai.keyShapeProblem('openai', 'p'.repeat(900))).not.toBeNull();
    expect(ai.keyShapeProblem('snowflake', 'p'.repeat(2049))).not.toBeNull();
  });
});

describe('Snowflake account URL', () => {
  it('accepts account hosts and normalises them', () => {
    for (const [raw, want] of [
      ['https://myorg-myacct.snowflakecomputing.com', 'https://myorg-myacct.snowflakecomputing.com'],
      ['https://MyOrg-MyAcct.snowflakecomputing.com/', 'https://myorg-myacct.snowflakecomputing.com'],
      ['  myorg-myacct.snowflakecomputing.com ', 'https://myorg-myacct.snowflakecomputing.com'],
      ['https://xy12345.us-east-1.snowflakecomputing.com', 'https://xy12345.us-east-1.snowflakecomputing.com'],
      ['https://acct.privatelink.snowflakecomputing.com', 'https://acct.privatelink.snowflakecomputing.com'],
    ]) {
      expect(ai.checkAccountUrl(raw)).toEqual({ ok: true, url: want });
    }
  });

  it('refuses everything else', () => {
    for (const bad of [
      '',
      '   ',
      'http://myorg.snowflakecomputing.com',
      'ftp://myorg.snowflakecomputing.com',
      'https://snowflakecomputing.com',
      'https://evil.com',
      'https://snowflakecomputing.com.evil.com',
      'https://myorg.snowflakecomputing.com.evil.com',
      'https://evil-snowflakecomputing.com',
      'https://user@myorg.snowflakecomputing.com',
      'https://myorg.snowflakecomputing.com:443',
      'https://myorg.snowflakecomputing.com/api',
      'https://myorg.snowflakecomputing.com/?x=1',
      'https://myorg.snowflakecomputing.com#x',
      'https://my org.snowflakecomputing.com',
      'https://-bad.snowflakecomputing.com',
      'https://my_org.snowflakecomputing.com',
      'https://myorg..snowflakecomputing.com',
      'https://myorg.snowflakecomputing.cn',
      'https://localhost',
    ]) {
      expect(ai.checkAccountUrl(bad).ok, bad).toBe(false);
    }
  });

  it('never repeats what was typed in its error', () => {
    const r = ai.checkAccountUrl('pat-secret-0123456789abcdef');
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error).not.toContain('pat-secret');
  });
});

describe('the Gemini notice', () => {
  const all: ai.ConsentChecks = { adult: true, terms: true, freeTier: true, region: 'elsewhere', billing: false };

  it('needs every box and a region before it can be accepted', () => {
    expect(ai.consentComplete(ai.EMPTY_CHECKS)).toBe(false);
    expect(ai.consentComplete(all)).toBe(true);
    for (const k of ['adult', 'terms', 'freeTier'] as const) expect(ai.consentComplete({ ...all, [k]: false }), k).toBe(false);
    expect(ai.consentComplete({ ...all, region: null })).toBe(false);
  });

  it('asks the billing question only in the EEA, UK and Switzerland', () => {
    expect(ai.consentComplete({ ...all, region: 'restricted', billing: false })).toBe(false);
    expect(ai.consentComplete({ ...all, region: 'restricted', billing: true })).toBe(true);
    expect(ai.consentComplete({ ...all, region: 'elsewhere', billing: false })).toBe(true);
  });

  it('guesses the region from a locale, and leaves unknown ones to the user', () => {
    expect(ai.guessConsentRegion('en-GB')).toBe('restricted');
    expect(ai.guessConsentRegion('de-DE')).toBe('restricted');
    expect(ai.guessConsentRegion('fr_CH')).toBe('restricted');
    expect(ai.guessConsentRegion('nb-NO')).toBe('restricted');
    expect(ai.guessConsentRegion('en-US')).toBe('elsewhere');
    expect(ai.guessConsentRegion('en-IN')).toBe('elsewhere');
    expect(ai.guessConsentRegion('en')).toBeNull();
    expect(ai.guessConsentRegion('')).toBeNull();
    expect(ai.guessConsentRegion(undefined)).toBeNull();
  });

  it('carries the 9.4 copy: own key, Google’s terms, 18+, human review, sensitive data, EEA/UK/CH', () => {
    const n = ai.GEMINI_NOTICE;
    expect(n.body).toContain('Krovi never sees your key');
    expect(n.adult).toContain('18 or older');
    expect(n.adult).toContain('I created this key myself');
    expect(n.terms).toContain('Gemini API Additional Terms');
    expect(n.terms).toContain('not for consumer use');
    expect(n.freeTier).toContain('people at Google may read it');
    expect(n.freeTier).toContain('sensitive, confidential');
    expect(n.regionRestricted).toBe('EEA, UK or Switzerland');
    expect(n.billing).toContain('billing enabled');
    expect(n.perSend).toBe('If your key is on the free tier, Google may use and review this image.');
    expect(ai.GEMINI_CONSENT_VERSION).toBe(1);
  });
});

describe('distinct copy per failure', () => {
  const six: ai.AiFailureKind[] = ['auth', 'billing', 'quota', 'rate-limit', 'eligibility', 'network'];

  it('says something different for authentication, billing, quota, rate limit, eligibility and network', () => {
    const titles = six.map((k) => ai.failureCopy(k).title);
    const advice = six.map((k) => ai.failureCopy(k).advice);
    expect(new Set(titles).size).toBe(6);
    expect(new Set(advice).size).toBe(6);
  });

  it('covers every kind the backend sends', () => {
    const all: ai.AiFailureKind[] = ['auth', 'billing', 'quota', 'rate-limit', 'eligibility', 'model', 'blocked', 'unavailable', 'network', 'other', 'no-provider', 'consent-required', 'expired', 'inactive', 'not-allowed'];
    for (const k of all) {
      const c = ai.failureCopy(k);
      expect(c.title.length, k).toBeGreaterThan(5);
      expect(c.advice.length, k).toBeGreaterThan(15);
    }
  });

  it('tells a valid key with a blocked project apart from a bad key', () => {
    expect(ai.failureCopy('eligibility').advice).toContain('making more keys will not help');
    expect(ai.failureCopy('eligibility').advice).toContain('key is valid');
    expect(ai.failureCopy('billing').advice).toContain('does not retry');
    expect(ai.failureCopy('quota').advice).toContain('Waiting a minute will not help');
    expect(ai.failureCopy('rate-limit').advice).toContain('The key works');
  });
});

describe('status rows', () => {
  const rows = (r: ai.TestReport) => Object.fromEntries(ai.statusRows(r).map((x) => [x.id, x.state]));

  it('a passing test is all green', () => {
    const r = report({ ok: true, steps: [{ id: 'key', ok: true, kind: null, message: '' }, { id: 'inference', ok: true, kind: null, message: '' }] });
    expect(rows(r)).toEqual({ auth: 'ok', billing: 'ok', quota: 'ok', eligibility: 'ok', model: 'ok' });
  });

  it('a Gemini key that works but whose project is blocked reads as exactly that', () => {
    const r = report({ kind: 'eligibility', steps: [{ id: 'key', ok: true, kind: null, message: '' }, { id: 'inference', ok: false, kind: 'eligibility', message: '' }] });
    expect(rows(r)).toEqual({ auth: 'ok', billing: 'unknown', quota: 'unknown', eligibility: 'problem', model: 'unknown' });
  });

  it('a rejected key is an authentication problem and nothing after it was checked', () => {
    const r = report({ kind: 'auth', steps: [{ id: 'key', ok: false, kind: 'auth', message: '' }] });
    expect(rows(r)).toEqual({ auth: 'problem', billing: 'unknown', quota: 'unknown', eligibility: 'unknown', model: 'unknown' });
  });

  it('billing, quota and rate limit each light their own row only', () => {
    expect(rows(report({ kind: 'billing' }))).toEqual({ auth: 'ok', billing: 'problem', quota: 'unknown', eligibility: 'unknown', model: 'unknown' });
    expect(rows(report({ kind: 'quota' }))).toEqual({ auth: 'ok', billing: 'unknown', quota: 'problem', eligibility: 'unknown', model: 'unknown' });
    expect(rows(report({ kind: 'rate-limit' }))).toEqual({ auth: 'ok', billing: 'unknown', quota: 'problem', eligibility: 'unknown', model: 'unknown' });
    const rate = ai.statusRows(report({ kind: 'rate-limit' })).find((x) => x.id === 'quota');
    expect(rate?.text).toBe(ai.failureCopy('rate-limit').title);
  });

  it('a wrong model is a model problem on a key that works', () => {
    expect(rows(report({ kind: 'model' }))).toEqual({ auth: 'ok', billing: 'unknown', quota: 'unknown', eligibility: 'unknown', model: 'problem' });
  });

  it('no answer at all (network, provider down) leaves everything unchecked', () => {
    const all = { auth: 'unknown', billing: 'unknown', quota: 'unknown', eligibility: 'unknown', model: 'unknown' };
    expect(rows(report({ kind: 'network' }))).toEqual(all);
    expect(rows(report({ kind: 'unavailable' }))).toEqual(all);
  });

  it('Snowflake has no model-list step: a pass still shows the key accepted', () => {
    const r = report({ ok: true, steps: [{ id: 'inference', ok: true, kind: null, message: '' }] });
    expect(rows(r).auth).toBe('ok');
  });
});

describe('usage', () => {
  it('shows nothing until something was sent, then a plain line', () => {
    const zero: ai.UsageInfo = { requests: 0, inputTokens: 0, outputTokens: 0, lastError: null };
    expect(ai.usageLine(zero)).toBeNull();
    expect(ai.usageLine({ ...zero, requests: 1 })).toBe('1 request since Krovi started');
    expect(ai.usageLine({ ...zero, requests: 3, inputTokens: 1000, outputTokens: 204 })).toBe('3 requests, 1,204 tokens since Krovi started');
    expect(ai.formatTokens(-5)).toBe('0');
    expect(ai.formatTokens(Number.NaN)).toBe('0');
  });
});

describe('normalising what Rust sends', () => {
  const provider = (over: Record<string, unknown> = {}) => ({
    id: 'anthropic',
    name: 'Anthropic',
    hasKey: true,
    keyHint: 'abcd',
    ready: true,
    gate: null,
    model: 'claude-haiku-5-5',
    defaultModel: 'claude-haiku-5-5',
    visionModel: 'claude-haiku-5-5',
    supportsModelList: true,
    needsAccountUrl: false,
    accountUrl: null,
    usage: { requests: 2, inputTokens: 10, outputTokens: 5, lastError: { kind: 'quota', at: 123 } },
    ...over,
  });

  it('keeps only the known fields, so a key could never ride along', () => {
    const SECRET = 'sk-ant-leaked-0123456789abcdef';
    const st = ai.normalizeStatus({
      providers: [provider({ key: SECRET, apiKey: SECRET, secret: SECRET, usage: { requests: 2, inputTokens: 10, outputTokens: 5, key: SECRET, lastError: { kind: 'quota', at: 123, key: SECRET } } })],
      selected: 'anthropic',
      model: 'claude-haiku-5-5',
      gemini: { accepted: false, key: SECRET },
      key: SECRET,
    });
    expect(JSON.stringify(st)).not.toContain(SECRET);
    expect(st.providers).toHaveLength(1);
    expect(st.selected).toBe('anthropic');
    expect(st.providers[0].usage.lastError).toEqual({ kind: 'quota', at: 123 });
  });

  it('shows at most the last four characters of a key', () => {
    expect(ai.normalizeStatus({ providers: [provider({ keyHint: 'wxyz' })] }).providers[0].keyHint).toBe('wxyz');
    expect(ai.normalizeStatus({ providers: [provider({ keyHint: 'sk-ant-the-whole-key' })] }).providers[0].keyHint).toBeNull();
  });

  it('drops unknown providers (a banned or excluded one never renders) and a selection that does not exist', () => {
    const st = ai.normalizeStatus({
      providers: [provider(), provider({ id: 'deepseek' }), provider({ id: 'muse-spark-1.3' }), provider({ id: 'gemini', name: 'Google Gemini' })],
      selected: 'muse-spark-1.3',
      model: 'x',
    });
    expect(st.providers.map((p) => p.id)).toEqual(['anthropic', 'gemini']);
    expect(st.selected).toBeNull();
    expect(st.model).toBeNull();
    expect(ai.normalizeStatus({ selected: 'groq', providers: [provider()] }).selected).toBeNull();
  });

  it('survives junk', () => {
    for (const junk of [null, undefined, 7, 'x', [], {}]) {
      const st = ai.normalizeStatus(junk);
      expect(st.providers).toEqual([]);
      expect(st.selected).toBeNull();
      expect(st.gemini.accepted).toBe(false);
      expect(st.gemini.currentVersion).toBe(ai.GEMINI_CONSENT_VERSION);
    }
  });

  it('normalises a test report and drops banned models from its list', () => {
    const r = ai.normalizeReport({
      ok: false,
      kind: 'eligibility',
      message: 'm',
      status: 403,
      detail: 'd',
      model: 'gemini-3.5-flash-lite',
      models: [{ id: 'gemini-3.5-flash-lite', name: null }, { id: 'muse-spark-1.3' }],
      steps: [{ id: 'key', ok: true, kind: null, message: 'ok' }, { id: 'inference', ok: false, kind: 'eligibility', message: 'no' }, { id: 'weird', ok: true }],
      notes: ['a', 7],
      inputTokens: null,
      outputTokens: 3,
      extra: 'x',
    });
    expect(r.kind).toBe('eligibility');
    expect(r.models).toEqual([{ id: 'gemini-3.5-flash-lite', name: null }]);
    expect(r.steps.map((s) => s.id)).toEqual(['key', 'inference']);
    expect(r.notes).toEqual(['a']);
    expect(r).not.toHaveProperty('extra');
    expect(ai.normalizeReport(null).ok).toBe(false);
    expect(ai.normalizeReport({ kind: 'weird' }).kind).toBeNull();
  });
});

describe('demo mode (no Tauri)', () => {
  beforeEach(() => ai.__test.reset());

  it('lists the providers with no keys and nothing selected', async () => {
    const st = await ai.loadAi();
    expect(st.providers.map((p) => p.id)).toEqual(['anthropic', 'openai', 'gemini', 'groq', 'snowflake']);
    expect(st.providers.every((p) => !p.hasKey && !p.ready && p.keyHint === null)).toBe(true);
    expect(st.selected).toBeNull();
    expect(st.gemini.accepted).toBe(false);
    expect(get(ai.aiStatus)).toEqual(st);
  });

  it('refuses every action with the desktop-only sentence and makes no call', async () => {
    expect(ai.DESKTOP_ONLY).toBe('Available in the desktop app.');
    await expect(ai.saveKey('openai', 'sk-test-0123456789abcdefABCDEF')).rejects.toThrow(ai.DESKTOP_ONLY);
    await expect(ai.removeKey('openai')).rejects.toThrow(ai.DESKTOP_ONLY);
    await expect(ai.setAccountUrl('snowflake', 'https://a.snowflakecomputing.com')).rejects.toThrow(ai.DESKTOP_ONLY);
    await expect(ai.testConnection('openai')).rejects.toThrow(ai.DESKTOP_ONLY);
    await expect(ai.selectProvider('openai')).rejects.toThrow(ai.DESKTOP_ONLY);
    await expect(ai.setGeminiConsent(true)).rejects.toThrow(ai.DESKTOP_ONLY);
    await expect(ai.openKeyPage('openai')).rejects.toThrow(ai.DESKTOP_ONLY);
    await expect(ai.complete('hi')).rejects.toMatchObject({ kind: 'no-provider' });
  });

  it('still offers the default model in the dropdown', async () => {
    const m = await ai.loadModels('groq');
    expect(m.listed).toBe(false);
    expect(m.models).toEqual([{ id: 'openai/gpt-oss-120b', name: null }]);
    expect(m.error).toBeNull();
  });
});
