// Circle to Ask inside the Tauri shell: the bridge is mocked, so this proves the command names and
// arguments, the event -> store wiring, that errors keep their kind, and that nothing but the
// documented commands is ever called (in particular, nothing captures by itself).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
const handlers = new Map<string, (payload: unknown) => void>();
const errors = new Map<string, unknown>();
const replies = new Map<string, unknown>();

const statusJson = (over: Record<string, unknown> = {}) => ({
  settings: { enabled: true, shortcut: 'Ctrl+Alt+C', sessionMinutes: 10 },
  providers: [
    { id: 'anthropic', name: 'Anthropic', defaultModel: 'claude-haiku-5-5', model: 'claude-haiku-5-5', hasKey: true, keyHint: 'abcd' },
    { id: 'openai', name: 'OpenAI', defaultModel: 'gpt-6-luna', model: 'gpt-6-luna', hasKey: false, keyHint: null },
    { id: 'gemini', name: 'Google Gemini', defaultModel: 'gemini-3.5-flash-lite', model: 'gemini-3.5-flash-lite', hasKey: false, keyHint: null },
    { id: 'groq', name: 'Groq', defaultModel: 'qwen/qwen3.8-27b', model: 'qwen/qwen3.8-27b', hasKey: false, keyHint: null },
    { id: 'snowflake', name: 'Snowflake Cortex', defaultModel: 'claude-sonnet-4-5', model: 'claude-sonnet-4-5', hasKey: false, keyHint: null },
  ],
  activeProvider: 'anthropic',
  disclosure: { provider: 'anthropic', providerName: 'Anthropic', model: 'claude-haiku-5-5', image: 'This sends the selected image to Anthropic using your key.', screen: 'This sends a picture of your screen to Anthropic using your key, each time you ask with the picture option on.' },
  disclosures: [{ provider: 'anthropic', providerName: 'Anthropic', model: 'claude-haiku-5-5', image: 'This sends the selected image to Anthropic using your key.', screen: 'This sends a picture of your screen to Anthropic using your key, each time you ask with the picture option on.' }],
  maxSessionMinutes: 30,
  shortcutError: null,
  ...over,
});

const readJson = (over: Record<string, unknown> = {}) => ({
  ok: true,
  source: 'circle',
  text: 'Hello\nWorld',
  lines: [
    { text: 'Hello', x: 1, y: 2, w: 30, h: 10 },
    { text: 'World', x: 1, y: 14, w: 32, h: 10 },
  ],
  width: 200,
  height: 80,
  language: 'en-US',
  ocrError: null,
  blank: false,
  error: null,
  expiresInSecs: 120,
  ...over,
});

const awareJson = (over: Record<string, unknown> = {}) => ({
  active: true,
  startedAt: 1_000,
  endsAt: 601_000,
  remainingSecs: 600,
  maxMinutes: 30,
  vision: false,
  providerName: null,
  reason: 'started',
  ...over,
});

vi.mock('./bridge', () => ({
  isTauri: () => true,
  listen: async (event: string, cb: (payload: unknown) => void) => {
    handlers.set(event, cb);
    return () => handlers.delete(event);
  },
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    calls.push({ cmd, args });
    if (errors.has(cmd)) throw errors.get(cmd);
    if (replies.has(cmd)) return replies.get(cmd);
    return undefined;
  },
}));

const emit = (event: string, payload: unknown) => handlers.get(event)?.(payload);
const cmds = () => calls.map((c) => c.cmd);

async function fresh() {
  vi.resetModules();
  return await import('./circle');
}

beforeEach(() => {
  calls.length = 0;
  handlers.clear();
  errors.clear();
  replies.clear();
  replies.set('circle_get_settings', statusJson());
});
afterEach(() => {
  vi.useRealTimers();
});

describe('tauri: nothing happens by itself', () => {
  it('importing, wiring and loading settings call no capture command', async () => {
    const c = await fresh();
    await c.wireCircle();
    await c.loadCircle();
    await c.ocrStatus();
    await c.awareStatus();
    expect(cmds()).toEqual(['circle_get_settings', 'circle_ocr_status', 'aware_status']);
    for (const bad of ['circle_start', 'circle_capture', 'aware_start', 'aware_ask', 'circle_ask_vision']) expect(cmds()).not.toContain(bad);
  });
});

describe('tauri: settings', () => {
  it('loads the status and normalises it', async () => {
    const c = await fresh();
    const st = await c.loadCircle();
    expect(st.activeProvider).toBe('anthropic');
    expect(st.providers.map((p) => [p.id, p.hasKey])).toEqual([
      ['anthropic', true],
      ['openai', false],
      ['gemini', false],
      ['groq', false],
      ['snowflake', false],
    ]);
    expect(st.disclosure?.image).toBe('This sends the selected image to Anthropic using your key.');
    expect(get(c.circleStatus)).toEqual(st);
  });

  it('survives a half-empty answer', async () => {
    replies.set('circle_get_settings', { settings: { sessionMinutes: 999, provider: 'muse' }, activeProvider: 'muse-spark-1.3' });
    const c = await fresh();
    const st = await c.loadCircle();
    expect(st.settings.sessionMinutes).toBe(30);
    expect(st.settings).not.toHaveProperty('provider');
    expect(st.activeProvider).toBeNull();
    expect(st.providers).toEqual([]);
  });

  it('saves the whole settings object and keeps the backend error text', async () => {
    const c = await fresh();
    replies.set('circle_set_settings', statusJson());
    await c.saveCircleSettings({ ...c.DEFAULT_SETTINGS, sessionMinutes: 5 });
    expect(calls.at(-1)).toEqual({ cmd: 'circle_set_settings', args: { settings: { enabled: true, shortcut: 'Ctrl+Alt+C', sessionMinutes: 5 } } });
    errors.set('circle_set_settings', 'Could not use Ctrl+Alt+C as the Circle to Ask shortcut: already registered');
    await expect(c.saveCircleSettings(c.DEFAULT_SETTINGS)).rejects.toThrow('already registered');
  });

  it('passes the key command through without ever returning a key', async () => {
    const c = await fresh();
    replies.set('circle_set_key', statusJson());
    const st = await c.setProviderKey('gemini', 'AIza-test-key-0123456789');
    expect(calls.at(-1)).toEqual({ cmd: 'circle_set_key', args: { provider: 'gemini', key: 'AIza-test-key-0123456789' } });
    expect(JSON.stringify(st)).not.toContain('AIza-test');
    await c.setProviderKey('gemini', null);
    expect(calls.at(-1)).toEqual({ cmd: 'circle_set_key', args: { provider: 'gemini', key: null } });
  });
});

describe('tauri: circling an area', () => {
  it('start / cancel / ready use the overlay commands', async () => {
    const c = await fresh();
    await c.startCircle();
    await c.cancelCircle();
    replies.set('circle_overlay_ready', { token: 3, x: 1920, y: 0, width: 2560, height: 1440 });
    expect(await c.overlayReady()).toEqual({ token: 3, x: 1920, y: 0, width: 2560, height: 1440 });
    replies.set('circle_overlay_ready', null);
    expect(await c.overlayReady()).toBeNull();
    errors.set('circle_overlay_ready', 'gone');
    expect(await c.overlayReady()).toBeNull();
    expect(cmds()).toEqual(['circle_start', 'circle_cancel', 'circle_overlay_ready', 'circle_overlay_ready', 'circle_overlay_ready']);
  });

  it('capture sends the physical rect and gets text back, never a picture', async () => {
    const c = await fresh();
    replies.set('circle_capture', readJson());
    const r = await c.captureRegion({ x: 100, y: 50, width: 400, height: 200 });
    expect(calls.at(-1)).toEqual({ cmd: 'circle_capture', args: { rect: { x: 100, y: 50, width: 400, height: 200 } } });
    expect(r.text).toBe('Hello\nWorld');
    expect(r.lines).toHaveLength(2);
    expect(r.demo).toBeUndefined();
    expect(Object.keys(r)).not.toContain('image');
    expect(get(c.circleHeld)?.result.text).toBe('Hello\nWorld');
  });

  it('a refused capture rejects with the backend sentence', async () => {
    const c = await fresh();
    errors.set('circle_capture', "Circle to Ask isn't active. Press the shortcut again.");
    await expect(c.captureRegion({ x: 0, y: 0, width: 50, height: 50 })).rejects.toThrow("isn't active");
  });

  it('the circle-result event fills the store and calls the callback (including a failure)', async () => {
    const c = await fresh();
    const seen: string[] = [];
    await c.wireCircle((r) => seen.push(r.ok ? r.text : `ERR ${r.error}`));
    emit('circle-result', readJson({ text: 'from the event' }));
    expect(get(c.circleHeld)?.result.text).toBe('from the event');
    emit('circle-result', { ok: false, source: 'circle', error: 'The selected area is too small.' });
    expect(get(c.circleHeld)?.result).toMatchObject({ ok: false, error: 'The selected area is too small.', text: '', lines: [] });
    expect(seen).toEqual(['from the event', 'ERR The selected area is too small.']);
  });

  it('wires the two events once', async () => {
    const c = await fresh();
    await c.wireCircle();
    await c.wireCircle();
    expect([...handlers.keys()].sort()).toEqual(['aware-state', 'circle-result']);
  });

  it('discarding forgets the result and tells Rust to drop the picture', async () => {
    const c = await fresh();
    replies.set('circle_capture', readJson());
    await c.captureRegion({ x: 0, y: 0, width: 50, height: 50 });
    await c.discardHeld();
    expect(get(c.circleHeld)).toBeNull();
    expect(cmds()).toContain('circle_clear');
  });

  it('dropping the picture keeps the text but ends "Ask AI"', async () => {
    const c = await fresh();
    replies.set('circle_capture', readJson());
    await c.captureRegion({ x: 0, y: 0, width: 50, height: 50 });
    const st = await c.loadCircle();
    expect(c.askAvailability(st, get(c.circleHeld), Date.now()).enabled).toBe(true);
    await c.dropPicture();
    expect(cmds()).toContain('circle_clear');
    expect(get(c.circleHeld)?.result.text).toBe('Hello\nWorld');
    expect(c.askAvailability(st, get(c.circleHeld), Date.now()).enabled).toBe(false);
  });

  it('search and key pages go through commands (the address is built in Rust)', async () => {
    const c = await fresh();
    await c.searchWeb('some text');
    await c.openKeysPage('openai');
    expect(calls.slice(-2)).toEqual([
      { cmd: 'circle_search', args: { text: 'some text' } },
      { cmd: 'circle_open_keys_page', args: { provider: 'openai' } },
    ]);
  });
});

describe('tauri: Ask AI', () => {
  it('sends the question with the confirmation and the chosen provider', async () => {
    const c = await fresh();
    replies.set('circle_ask_vision', { answer: 'A cat.', truncated: false, provider: 'anthropic', providerName: 'Anthropic', model: 'claude-haiku-5-5' });
    const a = await c.askVision('What is this?', { confirm: true });
    expect(calls.at(-1)).toEqual({ cmd: 'circle_ask_vision', args: { args: { question: 'What is this?', confirm: true, provider: null } } });
    expect(a.answer).toBe('A cat.');
    await c.askVision('Again', { confirm: true, provider: 'gemini' });
    expect((calls.at(-1)?.args?.args as { provider: string }).provider).toBe('gemini');
  });

  it('keeps the kind of every failure so the UI can say what to do', async () => {
    const c = await fresh();
    for (const [kind, status] of [
      ['auth', 401],
      ['billing', 402],
      ['quota', 429],
      ['rate-limit', 429],
      ['eligibility', 403],
      ['network', null],
      ['consent-required', null],
      ['expired', null],
      ['no-provider', null],
    ] as const) {
      errors.set('circle_ask_vision', { kind, message: `m-${kind}`, detail: 'd', status });
      const e = await c.askVision('q', { confirm: true }).catch((x) => x);
      expect(e).toBeInstanceOf(c.AiError);
      expect(e.kind).toBe(kind);
      expect(e.failure).toEqual({ kind, message: `m-${kind}`, detail: 'd', status });
    }
  });

  it('a plain string error becomes kind "other"', async () => {
    const c = await fresh();
    errors.set('circle_ask_vision', 'boom');
    const e = await c.askVision('q', { confirm: true }).catch((x) => x);
    expect(e).toBeInstanceOf(c.AiError);
    expect(e.kind).toBe('other');
    expect(e.message).toBe('boom');
  });
});

describe('tauri: screen awareness', () => {
  it('start passes minutes + the picture option and publishes the state', async () => {
    const c = await fresh();
    replies.set('aware_start', awareJson());
    const st = await c.awareStart(10, false);
    expect(calls.at(-1)).toEqual({ cmd: 'aware_start', args: { minutes: 10, vision: false } });
    expect(st).toMatchObject({ active: true, endsAt: 601_000, maxMinutes: 30, vision: false });
    expect(get(c.awareState).active).toBe(true);
  });

  it('the aware-state event drives the indicator on and off', async () => {
    const c = await fresh();
    await c.wireCircle();
    emit('aware-state', awareJson({ vision: true, providerName: 'Anthropic' }));
    expect(get(c.awareState)).toMatchObject({ active: true, vision: true, providerName: 'Anthropic', reason: 'started' });
    emit('aware-state', { active: false, startedAt: null, endsAt: null, remainingSecs: 0, maxMinutes: 30, vision: false, providerName: null, reason: 'expired' });
    expect(get(c.awareState)).toMatchObject({ active: false, reason: 'expired', endsAt: null });
  });

  it('stop calls the command and drops the indicator at once', async () => {
    const c = await fresh();
    await c.wireCircle();
    emit('aware-state', awareJson());
    await c.awareStop();
    expect(cmds()).toContain('aware_stop');
    expect(get(c.awareState)).toMatchObject({ active: false, reason: 'stopped' });
  });

  it('stop drops the indicator even when the command fails', async () => {
    const c = await fresh();
    await c.wireCircle();
    emit('aware-state', awareJson());
    errors.set('aware_stop', 'oops');
    await expect(c.awareStop()).rejects.toBe('oops');
    expect(get(c.awareState).active).toBe(false);
  });

  it('a refused start keeps its kind', async () => {
    const c = await fresh();
    errors.set('aware_start', { kind: 'no-provider', message: 'Set up an AI provider in Settings → AI.', detail: null, status: null });
    await expect(c.awareStart(5, true)).rejects.toMatchObject({ kind: 'no-provider' });
    expect(get(c.awareState).active).toBe(false);
  });

  it('ask sends the question and the picture flag, and returns text + an optional answer', async () => {
    const c = await fresh();
    replies.set('aware_ask', {
      read: readJson({ source: 'aware', expiresInSecs: 0 }),
      answer: { answer: 'It is a spreadsheet.', truncated: false, provider: 'anthropic', providerName: 'Anthropic', model: 'claude-haiku-5-5' },
      aiError: null,
    });
    const r = await c.awareAsk('what is open?', true);
    expect(calls.at(-1)).toEqual({ cmd: 'aware_ask', args: { question: 'what is open?', vision: true } });
    expect(r.read).toMatchObject({ source: 'aware', text: 'Hello\nWorld', expiresInSecs: 0 });
    expect(r.answer?.answer).toBe('It is a spreadsheet.');
    expect(r.aiError).toBeNull();
  });

  it('an AI failure inside a reply is reported without losing the text', async () => {
    const c = await fresh();
    replies.set('aware_ask', {
      read: readJson({ source: 'aware', expiresInSecs: 0 }),
      answer: null,
      aiError: { kind: 'quota', message: 'used up', detail: null, status: 429 },
    });
    const r = await c.awareAsk('q', true);
    expect(r.read.text).toBe('Hello\nWorld');
    expect(r.aiError).toEqual({ kind: 'quota', message: 'used up', detail: null, status: 429 });
  });

  it('asking without a session is refused with kind "inactive"', async () => {
    const c = await fresh();
    errors.set('aware_ask', { kind: 'inactive', message: 'Start a screen-awareness session first.', detail: null, status: null });
    await expect(c.awareAsk('q', false)).rejects.toMatchObject({ kind: 'inactive' });
  });
});
