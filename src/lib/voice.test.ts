// Voice API in the browser demo (no Tauri): the simulated session, the store reducers, and the
// subscriber fan-out. The Tauri event wiring is covered in voice.tauri.test.ts.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import type { VoiceCommand, VoiceResult } from './voice';

async function fresh() {
  vi.resetModules();
  return await import('./voice');
}

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe('demo: command session', () => {
  it('goes listening -> words appear -> processing -> idle and delivers the command', async () => {
    const v = await fresh();
    const commands: VoiceCommand[] = [];
    v.onVoiceCommand((c) => commands.push(c));
    const states: string[] = [];
    v.voiceState.subscribe((s) => {
      if (states[states.length - 1] !== s.state) states.push(s.state);
    });

    await v.startVoice('command');
    expect(get(v.voiceState)).toMatchObject({ state: 'listening', mode: 'command', partial: '' });

    await vi.advanceTimersByTimeAsync(700);
    expect(get(v.voiceState).partial).toBe('open');
    await vi.advanceTimersByTimeAsync(300);
    expect(get(v.voiceState).partial).toBe('open timers');

    await vi.advanceTimersByTimeAsync(5000);
    expect(states).toEqual(['idle', 'listening', 'processing', 'idle']);
    expect(commands).toEqual([{ kind: 'open-tool', tool: 'timers' }]);
    expect(get(v.voiceState)).toMatchObject({ state: 'idle', mode: null, partial: '', lastText: 'open timers' });
  });

  it('cycles through different demo commands on successive presses', async () => {
    const v = await fresh();
    const commands: VoiceCommand[] = [];
    v.onVoiceCommand((c) => commands.push(c));
    for (let i = 0; i < 3; i++) {
      await v.startVoice('command');
      await vi.advanceTimersByTimeAsync(6000);
    }
    expect(commands.map((c) => c.kind)).toEqual(['open-tool', 'start-timer', 'start-pomodoro']);
    expect(commands[1]).toEqual({ kind: 'start-timer', seconds: 300 });
  });
});

describe('demo: Voice v2 commands', () => {
  it('also demonstrates a reminder, timer control and media (never pre-executed in the demo)', async () => {
    const v = await fresh();
    const seen: { c: VoiceCommand; r: VoiceResult }[] = [];
    v.onVoiceCommand((c, r) => seen.push({ c, r }));
    for (let i = 0; i < 8; i++) {
      await v.startVoice('command');
      await vi.advanceTimersByTimeAsync(7000);
    }
    expect(seen.map((s) => s.c.kind)).toEqual(['open-tool', 'start-timer', 'start-pomodoro', 'new-note', 'open-tool', 'add-reminder', 'timer-control', 'media']);
    const reminder = seen[5].c;
    expect(reminder).toMatchObject({ kind: 'add-reminder', time: '09:00', title: 'Call mom' });
    expect((reminder as { date: string }).date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(seen[7].c).toEqual({ kind: 'media', command: 'next' });
    // the callback receives the whole result, and the demo never claims the backend did the work
    expect(seen.every((s) => s.r.executed === false)).toBe(true);
  });

  it('also shows the Voice v3 OS actions with sample rows (the browser opens nothing)', async () => {
    const v = await fresh();
    const seen: { c: VoiceCommand; r: VoiceResult }[] = [];
    v.onVoiceCommand((c, r) => seen.push({ c, r }));
    for (let i = 0; i < 12; i++) {
      await v.startVoice('command');
      await vi.advanceTimersByTimeAsync(7000);
    }
    expect(seen.slice(8).map((s) => s.c.kind)).toEqual(['search-files', 'open-folder', 'list-folder', 'open-app']);
    const search = seen[8].r;
    expect(search.executed).toBe(false);
    expect(search.results?.length).toBe(3);
    expect(search.feedback).toContain('Demo');
    expect(seen[10].r.results?.[0]).toMatchObject({ kind: 'file', name: 'setup.exe' });
    // a plain command carries no `results` key at all
    expect('results' in seen[0].r).toBe(false);
  });

  it('browser: getVoiceTargets returns the demo lists and openVoiceResult refuses politely', async () => {
    const v = await fresh();
    const t = await v.getVoiceTargets();
    expect(t.ready).toBe(true);
    expect(t.folders.map((f) => f.name)).toContain('Krovi downloads');
    await expect(v.openVoiceResult('C:\\x')).rejects.toThrow(/desktop app/);
  });

  it('with Keep transcripts off, a command leaves no transcript in the demo either', async () => {
    const store = new Map<string, string>();
    vi.stubGlobal('localStorage', { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v) });
    try {
      const v = await fresh();
      await v.setVoiceSettings({ keepTranscripts: false });
      const results: VoiceResult[] = [];
      v.onVoiceResult((r) => results.push(r));
      await v.startVoice('command');
      await vi.advanceTimersByTimeAsync(1200);
      expect(get(v.voiceState).partial).toBe('');
      await vi.advanceTimersByTimeAsync(6000);
      expect(results[0].text).toBe('');
      expect(results[0].command).toMatchObject({ kind: 'open-tool' });
      expect(get(v.voiceState).lastText).toBe('');
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe('demo: note session', () => {
  it('streams a dictated sentence and reports it as a note result (no command)', async () => {
    const v = await fresh();
    const results: VoiceResult[] = [];
    const commands: VoiceCommand[] = [];
    v.onVoiceResult((r) => results.push(r));
    v.onVoiceCommand((c) => commands.push(c));

    await v.startVoice('note');
    await vi.advanceTimersByTimeAsync(1400);
    const partial = get(v.voiceState).partial;
    expect(partial.length).toBeGreaterThan(0);
    expect('Remember to call the dentist tomorrow morning.'.startsWith(partial)).toBe(true);

    await vi.advanceTimersByTimeAsync(10_000);
    expect(results).toEqual([{ mode: 'note', text: 'Remember to call the dentist tomorrow morning.', command: null, context: null, feedback: null, executed: false }]);
    expect(commands).toEqual([]);
    expect(get(v.voiceState).state).toBe('idle');
  });
});

describe('demo: stop and restart', () => {
  it('stopVoice mid-sentence skips ahead and delivers exactly once', async () => {
    const v = await fresh();
    const results: VoiceResult[] = [];
    v.onVoiceResult((r) => results.push(r));

    await v.startVoice('note');
    await vi.advanceTimersByTimeAsync(1000);
    await v.stopVoice();
    expect(get(v.voiceState).state).toBe('processing');
    await vi.advanceTimersByTimeAsync(400);
    expect(get(v.voiceState).state).toBe('idle');
    await vi.advanceTimersByTimeAsync(20_000);
    expect(results).toHaveLength(1);
  });

  it('starting again cancels the previous run instead of double-delivering', async () => {
    const v = await fresh();
    const results: VoiceResult[] = [];
    v.onVoiceResult((r) => results.push(r));
    await v.startVoice('command');
    await vi.advanceTimersByTimeAsync(300);
    await v.startVoice('command');
    await vi.advanceTimersByTimeAsync(20_000);
    expect(results).toHaveLength(1);
    expect(results[0].text).toBe('start a timer for five minutes');
  });

  it('stopVoice with nothing running is harmless', async () => {
    const v = await fresh();
    await v.stopVoice();
    expect(get(v.voiceState).state).toBe('idle');
  });
});

describe('subscribers', () => {
  it('unsubscribe works and one failing listener does not break the others', async () => {
    const v = await fresh();
    const err = vi.spyOn(console, 'error').mockImplementation(() => {});
    const got: string[] = [];
    v.onVoiceCommand(() => {
      throw new Error('boom');
    });
    const off = v.onVoiceCommand((c) => got.push(c.kind));
    await v.startVoice('command');
    await vi.advanceTimersByTimeAsync(6000);
    expect(got).toEqual(['open-tool']);
    expect(err).toHaveBeenCalled();

    off();
    await v.startVoice('command');
    await vi.advanceTimersByTimeAsync(6000);
    expect(got).toEqual(['open-tool']);
    err.mockRestore();
  });
});

describe('privacy settings and facts', () => {
  it('defaults: locked values shown, transcripts kept, selection only, cleaning on', async () => {
    const v = await fresh();
    expect(v.DEFAULT_VOICE_SETTINGS).toEqual({ localOnly: true, storeAudio: false, keepTranscripts: true, contextCapture: 'selection', cleanTranscription: true });
    expect(await v.getVoiceSettings()).toEqual(v.DEFAULT_VOICE_SETTINGS);
  });

  it('normalize re-imposes the locks and rejects junk', async () => {
    const v = await fresh();
    expect(v.normalizeVoiceSettings({ localOnly: false, storeAudio: true, keepTranscripts: false, contextCapture: 'selection+clipboard-fallback', cleanTranscription: false })).toEqual({
      localOnly: true,
      storeAudio: false,
      keepTranscripts: false,
      contextCapture: 'selection+clipboard-fallback',
      cleanTranscription: false,
    });
    expect(v.normalizeVoiceSettings({ contextCapture: 'everything', keepTranscripts: 'yes' })).toEqual(v.DEFAULT_VOICE_SETTINGS);
    expect(v.normalizeVoiceSettings(null)).toEqual(v.DEFAULT_VOICE_SETTINGS);
    expect(v.normalizeVoiceSettings('x')).toEqual(v.DEFAULT_VOICE_SETTINGS);
  });

  it('setVoiceSettings persists partial changes (demo: localStorage) and updates the store', async () => {
    const store = new Map<string, string>();
    vi.stubGlobal('localStorage', { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v) });
    try {
      const v = await fresh();
      const saved = await v.setVoiceSettings({ contextCapture: 'off' });
      expect(saved.contextCapture).toBe('off');
      expect(saved.keepTranscripts).toBe(true);
      expect(get(v.voiceSettings)).toEqual(saved);
      const again = await fresh();
      expect(await again.getVoiceSettings()).toEqual(saved);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('PRIVACY_FACTS are plain sentences, include the headline guarantee, and promise nothing false', async () => {
    const v = await fresh();
    expect(v.PRIVACY_FACTS[0]).toBe('Speech is recognized on this PC by Windows SAPI. Nothing is sent anywhere.');
    expect(v.PRIVACY_FACTS.length).toBeGreaterThanOrEqual(8);
    for (const f of v.PRIVACY_FACTS) {
      expect(f.length).toBeGreaterThan(20);
      expect(f.endsWith('.')).toBe(true);
    }
    const all = v.PRIVACY_FACTS.join(' ');
    expect(all).toContain('never stores audio');
    expect(all).toContain('clipboard history'); // the honest caveat about the fallback
    expect(all.toLowerCase()).not.toContain('cloud');
  });
});

describe('availability and reducers', () => {
  it('the demo reports both capabilities', async () => {
    const v = await fresh();
    expect(await v.voiceAvailability()).toMatchObject({ dictation: true, commands: true });
  });

  it('reduceState maps backend states and clears stale text', async () => {
    const v = await fresh();
    const base = { state: 'idle', mode: null, partial: 'old', lastText: 'kept' } as const;
    expect(v.reduceState({ ...base }, { state: 'listening', mode: 'note' })).toMatchObject({ state: 'listening', mode: 'note', partial: '' });
    expect(v.reduceState({ ...base }, { state: 'processing', mode: 'note' })).toMatchObject({ state: 'processing', partial: 'old' });
    expect(v.reduceState({ ...base }, { state: 'error', mode: 'command', error: 'No microphone' })).toMatchObject({ state: 'error', error: 'No microphone', partial: '' });
    expect(v.reduceState({ ...base }, { state: 'error', mode: 'command' }).error).toBeTruthy();
    expect(v.reduceState({ ...base, state: 'error', error: 'x' }, { state: 'idle', mode: 'note' })).toEqual({ state: 'idle', mode: null, partial: '', error: undefined, lastText: 'kept' });
  });
});
