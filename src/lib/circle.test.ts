// Circle to Ask: pure helpers + the browser demo (no Tauri). The Tauri wiring is in circle.tauri.test.ts.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import * as c from './circle';

describe('selection geometry', () => {
  it('drag boxes are normalised whichever way the user dragged', () => {
    expect(c.dragBox({ x: 10, y: 20 }, { x: 110, y: 70 })).toEqual({ x: 10, y: 20, w: 100, h: 50 });
    expect(c.dragBox({ x: 110, y: 70 }, { x: 10, y: 20 })).toEqual({ x: 10, y: 20, w: 100, h: 50 });
    expect(c.dragBox({ x: 110, y: 20 }, { x: 10, y: 70 })).toEqual({ x: 10, y: 20, w: 100, h: 50 });
  });

  it('is kept inside the screen', () => {
    expect(c.dragBox({ x: -30, y: -5 }, { x: 5000, y: 700 }, { w: 1000, h: 600 })).toEqual({ x: 0, y: 0, w: 1000, h: 600 });
    expect(c.dragBox({ x: Number.NaN, y: 0 }, { x: 10, y: 10 })).toEqual({ x: 0, y: 0, w: 10, h: 10 });
  });

  it('a freehand circle becomes its bounding box', () => {
    const loop = [
      { x: 50, y: 10 },
      { x: 90, y: 40 },
      { x: 50, y: 90 },
      { x: 12, y: 45 },
      { x: 50, y: 12 },
    ];
    expect(c.pointsBox(loop)).toEqual({ x: 12, y: 10, w: 78, h: 80 });
    expect(c.pointsBox([])).toBeNull();
    expect(c.pointsBox([{ x: 5, y: 6 }])).toEqual({ x: 5, y: 6, w: 0, h: 0 });
    expect(c.pointsBox([{ x: -10, y: -10 }, { x: 2000, y: 2000 }], { w: 800, h: 600 })).toEqual({ x: 0, y: 0, w: 800, h: 600 });
  });

  it('a click or a twitch is not a selection', () => {
    expect(c.bigEnough(null)).toBe(false);
    expect(c.bigEnough({ x: 0, y: 0, w: 3, h: 200 })).toBe(false);
    expect(c.bigEnough({ x: 0, y: 0, w: 200, h: 7 })).toBe(false);
    expect(c.bigEnough({ x: 0, y: 0, w: 8, h: 8 })).toBe(true);
  });
});

describe('css -> physical pixels', () => {
  it('uses the monitor size, so 125% scaling is exact', () => {
    // a 1920x1080 monitor at 125%: the page is 1536x864 CSS px
    const r = c.toPhysicalRect({ x: 100, y: 50, w: 200, h: 100 }, { w: 1536, h: 864 }, { width: 1920, height: 1080 }, 1.25);
    expect(r).toEqual({ x: 125, y: 62, width: 250, height: 126 });
  });

  it('rounds outward so the whole selection is captured', () => {
    const r = c.toPhysicalRect({ x: 10.2, y: 10.2, w: 10.2, h: 10.2 }, { w: 1000, h: 1000 }, { width: 1500, height: 1500 }, 1.5);
    expect(r.x).toBe(Math.floor(10.2 * 1.5));
    expect(r.x + r.width).toBe(Math.ceil(20.4 * 1.5));
  });

  it('falls back to the device pixel ratio when the monitor is unknown', () => {
    expect(c.toPhysicalRect({ x: 10, y: 10, w: 100, h: 50 }, { w: 800, h: 600 }, null, 2)).toEqual({ x: 20, y: 20, width: 200, height: 100 });
    expect(c.toPhysicalRect({ x: 10, y: 10, w: 100, h: 50 }, { w: 800, h: 600 }, null, 0)).toEqual({ x: 10, y: 10, width: 100, height: 50 });
    expect(c.toPhysicalRect({ x: 10, y: 10, w: 100, h: 50 }, { w: 800, h: 600 }, null, Number.NaN).width).toBe(100);
  });

  it('never leaves the monitor', () => {
    const r = c.toPhysicalRect({ x: 1500, y: 800, w: 300, h: 300 }, { w: 1536, h: 864 }, { width: 1920, height: 1080 }, 1.25);
    expect(r.x + r.width).toBeLessThanOrEqual(1920);
    expect(r.y + r.height).toBeLessThanOrEqual(1080);
    expect(r.width).toBeGreaterThan(0);
  });
});

describe('session length and clock', () => {
  it('is clamped to 1..30 minutes like the backend', () => {
    expect(c.clampMinutes(null)).toBe(10);
    expect(c.clampMinutes(undefined, 5)).toBe(5);
    expect(c.clampMinutes(0)).toBe(1);
    expect(c.clampMinutes(7.9)).toBe(7);
    expect(c.clampMinutes(30)).toBe(30);
    expect(c.clampMinutes(31)).toBe(30);
    expect(c.clampMinutes(1e9)).toBe(30);
    expect(c.clampMinutes(Number.NaN)).toBe(10);
  });

  it('formats the time left', () => {
    expect(c.formatRemaining(545)).toBe('9:05');
    expect(c.formatRemaining(12)).toBe('0:12');
    expect(c.formatRemaining(0)).toBe('0:00');
    expect(c.formatRemaining(-5)).toBe('0:00');
    expect(c.formatRemaining(3723)).toBe('1:02:03');
    expect(c.formatRemaining(Number.NaN)).toBe('0:00');
  });

  it('counts seconds left from the end time', () => {
    expect(c.secsLeft(10_000, 4_500)).toBe(6);
    expect(c.secsLeft(10_000, 10_000)).toBe(0);
    expect(c.secsLeft(10_000, 99_999)).toBe(0);
    expect(c.secsLeft(null, 0)).toBe(0);
  });
});

describe('web search address (mirrors krovi_sys::web)', () => {
  it('cleans and encodes the text', () => {
    expect(c.googleSearchUrl('  Rust   lifetimes\nexplained \t')).toBe('https://www.google.com/search?q=Rust%20lifetimes%20explained');
    expect(c.googleSearchUrl('a&b=c?d#e')).toBe('https://www.google.com/search?q=a%26b%3Dc%3Fd%23e');
    expect(c.googleSearchUrl('café')).toBe('https://www.google.com/search?q=caf%C3%A9');
    expect(c.googleSearchUrl("it's (fine)!")).toBe('https://www.google.com/search?q=it%27s%20%28fine%29%21');
  });

  it('has no search for blank text and cuts long text', () => {
    expect(c.googleSearchUrl('')).toBeNull();
    expect(c.googleSearchUrl(' \n\t ')).toBeNull();
    expect(c.googleSearchUrl('\u0000\u0007')).toBeNull();
    expect([...(c.searchQuery('é'.repeat(1000)) ?? '')]).toHaveLength(c.MAX_QUERY_CHARS);
    const q = c.searchQuery('word '.repeat(200)) ?? '';
    expect([...q].length).toBeLessThanOrEqual(c.MAX_QUERY_CHARS);
    expect(q.endsWith(' ')).toBe(false);
  });
});

describe('errors', () => {
  it('reads text out of whatever invoke rejected with', () => {
    expect(c.errorText('plain')).toBe('plain');
    expect(c.errorText(new Error('boom'))).toBe('boom');
    expect(c.errorText({ kind: 'auth', message: 'bad key' })).toBe('bad key');
    expect(c.errorText(42)).toBe('42');
  });

  it('turns it into a typed failure', () => {
    expect(c.toFailure({ kind: 'rate-limit', message: 'slow down', detail: 'x', status: 429 })).toEqual({ kind: 'rate-limit', message: 'slow down', detail: 'x', status: 429 });
    expect(c.toFailure({ kind: 'martian', message: 'what' }).kind).toBe('other');
    expect(c.toFailure('some string')).toEqual({ kind: 'other', message: 'some string', detail: null, status: null });
    const e = new c.AiError({ kind: 'billing', message: 'no credit', detail: null, status: 402 });
    expect(c.toFailure(e).kind).toBe('billing');
    expect(e.kind).toBe('billing');
    expect(e.message).toBe('no credit');
  });

  it('gives advice for the failures the user can fix', () => {
    const f = (kind: c.AiFailureKind) => ({ kind, message: 'm', detail: null, status: null });
    expect(c.failureAdvice(f('auth'))).toMatch(/Settings → AI/);
    expect(c.failureAdvice(f('expired'))).toMatch(/again/);
    expect(c.failureAdvice(f('network'))).toBe('');
  });
});

function status(over: Partial<c.CircleStatus> = {}): c.CircleStatus {
  return {
    settings: { ...c.DEFAULT_SETTINGS },
    providers: [],
    activeProvider: 'gemini',
    disclosure: null,
    disclosures: [],
    maxSessionMinutes: 30,
    shortcutError: null,
    ...over,
  };
}
function held(over: Partial<c.ReadResult> = {}, receivedAt = 1000): { result: c.ReadResult; receivedAt: number } {
  return {
    receivedAt,
    result: { ok: true, source: 'circle', text: 't', lines: [], width: 10, height: 10, language: null, ocrError: null, blank: false, error: null, expiresInSecs: 120, ...over },
  };
}

describe('"Ask AI" availability', () => {
  it('is disabled with the Settings sentence until a key exists', () => {
    expect(c.askAvailability(status({ activeProvider: null }), held(), 2000)).toEqual({ enabled: false, reason: 'Set up an AI provider in Settings → AI' });
    expect(c.NO_PROVIDER_TEXT).toBe('Set up an AI provider in Settings → AI');
  });

  it('waits for the settings to load', () => {
    expect(c.askAvailability(null, held(), 2000).enabled).toBe(false);
  });

  it('is enabled with a key and a held picture', () => {
    expect(c.askAvailability(status(), held(), 2000)).toEqual({ enabled: true, reason: null });
  });

  it('ends when the two minutes are up, and never exists for a demo or an unheld result', () => {
    expect(c.askAvailability(status(), held({}, 0), 119_999).enabled).toBe(true);
    expect(c.askAvailability(status(), held({}, 0), 120_000).enabled).toBe(false);
    expect(c.askAvailability(status(), held({ demo: true }), 2000).enabled).toBe(false);
    expect(c.askAvailability(status(), held({ expiresInSecs: 0 }), 2000).enabled).toBe(false);
    expect(c.askAvailability(status(), null, 2000).enabled).toBe(false);
  });

  it('counts the seconds the picture is still held', () => {
    expect(c.heldSecsLeft(held({}, 0), 30_000)).toBe(90);
    expect(c.heldSecsLeft(held({}, 0), 500_000)).toBe(0);
    expect(c.heldSecsLeft(null, 0)).toBe(0);
  });
});

describe('matching the question against the read lines', () => {
  const lines: c.CircleLine[] = [
    { text: 'Invoice total: $420.00', x: 0, y: 0, w: 1, h: 1 },
    { text: 'Due date 12 December', x: 0, y: 20, w: 1, h: 1 },
    { text: 'Thank you for your business', x: 0, y: 40, w: 1, h: 1 },
  ];
  it('finds lines that share a real word', () => {
    expect(c.matchingLines('What is the invoice total?', lines).map((l) => l.text)).toEqual(['Invoice total: $420.00']);
    expect(c.matchingLines('due date?', lines).map((l) => l.text)).toEqual(['Due date 12 December']);
  });
  it('ignores filler words and short words', () => {
    expect(c.matchingLines('what is the', lines)).toEqual([]);
    expect(c.matchingLines('', lines)).toEqual([]);
    expect(c.matchingLines('is it ok', lines)).toEqual([]);
  });
});

describe('demo mode (no Tauri)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    c.__test.reset();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('loads a status with no keys and no active provider', async () => {
    const st = await c.loadCircle();
    expect(st.activeProvider).toBeNull();
    expect(st.providers.map((p) => p.id)).toEqual(['openai', 'anthropic', 'gemini']);
    expect(st.providers.every((p) => !p.hasKey)).toBe(true);
    expect(st.settings.shortcut).toBe('Ctrl+Alt+C');
    expect(get(c.circleStatus)).toEqual(st);
    expect(c.askAvailability(st, null, 0).reason).toBe(c.NO_PROVIDER_TEXT);
  });

  it('saves settings in memory only, clamping the session length', async () => {
    const st = await c.saveCircleSettings({ ...c.DEFAULT_SETTINGS, sessionMinutes: 99, models: { gemini: 'gemini-2.5-pro' } });
    expect(st.settings.sessionMinutes).toBe(30);
    expect(st.providers.find((p) => p.id === 'gemini')?.model).toBe('gemini-2.5-pro');
  });

  it('refuses to start the overlay and to touch keys', async () => {
    await expect(c.startCircle()).rejects.toThrow(/desktop app/);
    await expect(c.setProviderKey('openai', 'sk-x')).rejects.toThrow(c.DESKTOP_ONLY);
    await expect(c.openKeysPage('openai')).rejects.toThrow(c.DESKTOP_ONLY);
    expect(await c.overlayReady()).toBeNull();
    expect((await c.ocrStatus()).available).toBe(false);
  });

  it('returns a labelled sample for a selection, and never claims it was the screen', async () => {
    const r = await c.captureRegion({ x: 10, y: 10, width: 300, height: 120 });
    expect(r.demo).toBe(true);
    expect(r.text).toMatch(/^Demo:/);
    expect(r.expiresInSecs).toBe(0);
    expect(get(c.circleHeld)?.result).toBe(r);
    expect(c.askAvailability(await c.loadCircle(), get(c.circleHeld), Date.now()).enabled).toBe(false);
  });

  it('cannot ask an AI anything', async () => {
    await expect(c.askVision('what is this?', { confirm: true })).rejects.toMatchObject({ kind: 'no-provider' });
    await expect(c.askVision('x', { confirm: true, provider: 'openai' })).rejects.toBeInstanceOf(c.AiError);
  });

  it('simulates a session: starts, counts down, ends by itself at the limit', async () => {
    const st = await c.awareStart(2, false);
    expect(st.active).toBe(true);
    expect(st.maxMinutes).toBe(30);
    expect(st.endsAt! - st.startedAt!).toBe(2 * 60_000);
    expect(get(c.awareState).active).toBe(true);
    await vi.advanceTimersByTimeAsync(119_000);
    expect(get(c.awareState).active).toBe(true);
    await vi.advanceTimersByTimeAsync(2_000);
    expect(get(c.awareState)).toMatchObject({ active: false, reason: 'expired' });
  });

  it('caps a session at 30 minutes even if asked for more', async () => {
    const st = await c.awareStart(500, false);
    expect(st.endsAt! - st.startedAt!).toBe(30 * 60_000);
    await c.awareStop();
  });

  it('stops with one call, and a second start replaces the first timer', async () => {
    await c.awareStart(5, false);
    await c.awareStop();
    expect(get(c.awareState)).toMatchObject({ active: false, reason: 'stopped' });
    await c.awareStart(1, false);
    await c.awareStart(3, false);
    await vi.advanceTimersByTimeAsync(61_000);
    expect(get(c.awareState).active).toBe(true); // the 1-minute timer was cancelled
    await c.awareStop();
  });

  it('only answers during a session, and never sends a picture', async () => {
    await expect(c.awareAsk('what is on screen?', false)).rejects.toMatchObject({ kind: 'inactive' });
    await expect(c.awareStart(5, true)).rejects.toMatchObject({ kind: 'no-provider' });
    await c.awareStart(5, false);
    const r = await c.awareAsk('what is on screen?', false);
    expect(r.read.demo).toBe(true);
    expect(r.answer).toBeNull();
    await c.awareStop();
  });

  it('wiring events is harmless', async () => {
    await expect(c.wireCircle()).resolves.toBeUndefined();
  });
});
