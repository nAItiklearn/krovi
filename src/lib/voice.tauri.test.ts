// Voice API inside the Tauri shell: bridge is mocked, so this proves the command calls and the
// event -> store -> subscriber wiring without a backend.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import type { VoiceCommand, VoiceResult } from './voice';

const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
const handlers = new Map<string, (payload: unknown) => void>();
let availability = { dictation: true, commands: true, engine: 'test engine' };
let startError: string | null = null;
let storedSettings: unknown = null;
let openError: string | null = null;
const knownTargets = {
  ready: true,
  folders: [{ name: 'Downloads', path: 'C:\\Users\\me\\Downloads' }],
  apps: ['Notepad', 'Spotify'],
};

vi.mock('./bridge', () => ({
  isTauri: () => true,
  listen: async (event: string, cb: (payload: unknown) => void) => {
    handlers.set(event, cb);
    return () => handlers.delete(event);
  },
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    calls.push({ cmd, args });
    if (cmd === 'voice_available') return availability;
    if (cmd === 'voice_start' && startError) throw startError;
    if (cmd === 'voice_get_settings') return storedSettings;
    if (cmd === 'voice_known_targets') return knownTargets;
    if (cmd === 'voice_open_result') {
      if (openError) throw openError;
      return `Opened ${String(args?.path)}.`;
    }
    if (cmd === 'voice_set_settings') {
      // the backend re-imposes the locked values, whatever the caller sent
      storedSettings = { ...(args?.settings as object), localOnly: true, storeAudio: false };
      return storedSettings;
    }
    return undefined;
  },
}));

const emit = (event: string, payload: unknown) => handlers.get(event)?.(payload);

async function fresh() {
  vi.resetModules();
  return await import('./voice');
}

beforeEach(() => {
  vi.useFakeTimers();
  calls.length = 0;
  handlers.clear();
  availability = { dictation: true, commands: true, engine: 'test engine' };
  startError = null;
  storedSettings = null;
  openError = null;
});
afterEach(() => {
  vi.useRealTimers();
});

describe('tauri: starting and stopping', () => {
  it('probes availability, subscribes to the three events, then calls voice_start with the mode', async () => {
    const v = await fresh();
    await v.startVoice('note');
    expect([...handlers.keys()].sort()).toEqual(['voice-partial', 'voice-result', 'voice-state']);
    expect(calls.map((c) => c.cmd)).toEqual(['voice_available', 'voice_start']);
    expect(calls[1].args).toEqual({ mode: 'note', captureContext: false });
    expect(get(v.voiceState)).toMatchObject({ state: 'listening', mode: 'note' });
  });

  it('captureContext is forwarded for commands only, and never for dictation', async () => {
    const v = await fresh();
    await v.startVoice('command', { captureContext: true });
    await v.startVoice('command');
    await v.startVoice('note', { captureContext: true });
    const starts = calls.filter((c) => c.cmd === 'voice_start').map((c) => c.args);
    expect(starts).toEqual([
      { mode: 'command', captureContext: true },
      { mode: 'command', captureContext: false },
      { mode: 'note', captureContext: false },
    ]);
  });

  it('only probes availability once and only wires listeners once', async () => {
    const v = await fresh();
    await v.startVoice('command');
    await v.startVoice('command');
    expect(calls.filter((c) => c.cmd === 'voice_available')).toHaveLength(1);
    expect(calls.filter((c) => c.cmd === 'voice_start')).toHaveLength(2);
  });

  it('stopVoice invokes voice_stop', async () => {
    const v = await fresh();
    await v.stopVoice();
    expect(calls.map((c) => c.cmd)).toEqual(['voice_stop']);
  });

  it('reports "unavailable" without ever starting when the engine is missing', async () => {
    availability = { dictation: false, commands: false, engine: 'none' };
    const v = await fresh();
    await v.startVoice('note');
    expect(get(v.voiceState)).toMatchObject({ state: 'unavailable' });
    expect(get(v.voiceState).error).toBeTruthy();
    expect(calls.map((c) => c.cmd)).toEqual(['voice_available']);
  });

  it('checks the capability that matches the mode', async () => {
    availability = { dictation: false, commands: true, engine: 'cmd-only' };
    const v = await fresh();
    await v.startVoice('note');
    expect(get(v.voiceState).state).toBe('unavailable');
    await v.startVoice('command');
    expect(get(v.voiceState).state).toBe('listening');
  });

  it('a rejected voice_start becomes an error state that resets itself (never stuck)', async () => {
    startError = 'No microphone was found.';
    const v = await fresh();
    await v.startVoice('command');
    expect(get(v.voiceState)).toMatchObject({ state: 'error', error: 'No microphone was found.' });
    await vi.advanceTimersByTimeAsync(6100);
    expect(get(v.voiceState).state).toBe('idle');
  });
});

describe('tauri: privacy settings', () => {
  it('get/set go through voice_get_settings / voice_set_settings and the locks hold', async () => {
    const v = await fresh();
    expect(await v.getVoiceSettings()).toEqual(v.DEFAULT_VOICE_SETTINGS);
    const saved = await v.setVoiceSettings({ keepTranscripts: false, contextCapture: 'selection+clipboard-fallback' });
    expect(saved).toEqual({ localOnly: true, storeAudio: false, keepTranscripts: false, contextCapture: 'selection+clipboard-fallback', cleanTranscription: true });
    const set = calls.find((c) => c.cmd === 'voice_set_settings');
    expect(set?.args).toEqual({ settings: saved });
    expect(get(v.voiceSettings)).toEqual(saved);
    // a later partial change keeps what was saved
    const next = await v.setVoiceSettings({ cleanTranscription: false });
    expect(next).toMatchObject({ keepTranscripts: false, contextCapture: 'selection+clipboard-fallback', cleanTranscription: false });
  });

  it('a failing backend read falls back to the safe defaults', async () => {
    storedSettings = 'garbage';
    const v = await fresh();
    expect(await v.getVoiceSettings()).toEqual(v.DEFAULT_VOICE_SETTINGS);
  });
});

describe('tauri: Voice v3 local OS actions', () => {
  it('an executed open/search result reaches subscribers with its rows and feedback', async () => {
    const v = await fresh();
    const got: { c: VoiceCommand; r: VoiceResult }[] = [];
    v.onVoiceCommand((c, r) => got.push({ c, r }));
    await v.startVoice('command');
    const found: VoiceResult = {
      mode: 'command',
      text: 'find invoice',
      command: { kind: 'search-files', query: 'invoice' },
      context: null,
      feedback: 'Found 2 matches for "invoice".',
      executed: true,
      results: [
        { kind: 'file', name: 'invoice-march.pdf', path: 'C:\\Docs\\invoice-march.pdf', detail: '84 KB, Mon 2 Mar 10:12 AM' },
        { kind: 'folder', name: 'Invoices', path: 'C:\\Docs\\Invoices' },
      ],
    };
    emit('voice-result', found);
    expect(got).toHaveLength(1);
    expect(got[0].c).toEqual({ kind: 'search-files', query: 'invoice' });
    expect(got[0].r.executed).toBe(true);
    expect(got[0].r.results?.map((r) => r.kind)).toEqual(['file', 'folder']);
    expect(got[0].r.results?.[0].detail).toContain('84 KB');
  });

  it('an unresolved name is feedback with candidate rows and no command', async () => {
    const v = await fresh();
    const commands: VoiceCommand[] = [];
    const results: VoiceResult[] = [];
    v.onVoiceCommand((c) => commands.push(c));
    v.onVoiceResult((r) => results.push(r));
    await v.startVoice('command');
    emit('voice-result', {
      mode: 'command',
      text: 'open photo',
      command: null,
      feedback: 'More than one app matches "photo": Photo Editor Pro, Photo Viewer Plus. Say the full name.',
      executed: false,
      results: [
        { kind: 'app', name: 'Photo Editor Pro', path: 'shell:AppsFolder\\a' },
        { kind: 'app', name: 'Photo Viewer Plus', path: 'shell:AppsFolder\\b' },
      ],
    });
    expect(commands).toEqual([]);
    expect(results[0].results).toHaveLength(2);
    expect(results[0].feedback).toContain('Say the full name');
  });

  it('getVoiceTargets reads the closed folder/app lists from the backend', async () => {
    const v = await fresh();
    const t = await v.getVoiceTargets();
    expect(t.ready).toBe(true);
    expect(t.folders[0]).toEqual({ name: 'Downloads', path: 'C:\\Users\\me\\Downloads' });
    expect(t.apps).toEqual(['Notepad', 'Spotify']);
    expect(calls.map((c) => c.cmd)).toEqual(['voice_known_targets']);
  });

  it('openVoiceResult forwards the path to voice_open_result and surfaces a refusal', async () => {
    const v = await fresh();
    await expect(v.openVoiceResult('C:\\Docs\\Invoices')).resolves.toBe('Opened C:\\Docs\\Invoices.');
    expect(calls[0]).toEqual({ cmd: 'voice_open_result', args: { path: 'C:\\Docs\\Invoices' } });
    openError = 'That item is not in the latest voice results.';
    await expect(v.openVoiceResult('C:\\Windows\\System32\\cmd.exe')).rejects.toBe('That item is not in the latest voice results.');
  });
});

describe('tauri: backend events drive the store', () => {
  it('state, partial and result events update the store and reach subscribers', async () => {
    const v = await fresh();
    const commands: VoiceCommand[] = [];
    const results: VoiceResult[] = [];
    v.onVoiceCommand((c) => commands.push(c));
    v.onVoiceResult((r) => results.push(r));
    await v.startVoice('command');

    emit('voice-state', { state: 'listening', mode: 'command' });
    emit('voice-partial', { text: 'start a timer' });
    expect(get(v.voiceState)).toMatchObject({ state: 'listening', partial: 'start a timer' });
    emit('voice-partial', { text: 'start a timer for five minutes' });
    emit('voice-state', { state: 'processing', mode: 'command' });
    const result: VoiceResult = { mode: 'command', text: 'start a timer for five minutes', command: { kind: 'start-timer', seconds: 300 } };
    emit('voice-result', result);
    emit('voice-state', { state: 'idle', mode: 'command' });

    expect(commands).toEqual([{ kind: 'start-timer', seconds: 300 }]);
    expect(results).toEqual([result]);
    expect(get(v.voiceState)).toMatchObject({ state: 'idle', mode: null, partial: '', lastText: 'start a timer for five minutes' });
  });

  it('a command callback receives the whole result: executed flag, feedback and the captured context', async () => {
    const v = await fresh();
    const got: { c: VoiceCommand; r: VoiceResult }[] = [];
    v.onVoiceCommand((c, r) => got.push({ c, r }));
    await v.startVoice('command', { captureContext: true });
    const reminder: VoiceResult = {
      mode: 'command',
      text: 'add a reminder for tomorrow at nine to call mom',
      command: { kind: 'add-reminder', date: '2026-11-02', time: '09:00', title: 'Call mom' },
      context: { kind: 'app', value: 'notepad' },
      feedback: 'Reminder set for tomorrow, Mon 2 Nov, 9:00 AM: Call mom',
      executed: true,
    };
    emit('voice-result', reminder);
    expect(got).toHaveLength(1);
    expect(got[0].c).toEqual(reminder.command);
    expect(got[0].r.executed).toBe(true);
    expect(got[0].r.feedback).toContain('Reminder set for tomorrow, Mon 2 Nov, 9:00 AM');
    expect(got[0].r.context).toEqual({ kind: 'app', value: 'notepad' });
  });

  it('a result that needs a selection nobody made carries feedback but no command', async () => {
    const v = await fresh();
    const commands: VoiceCommand[] = [];
    const results: VoiceResult[] = [];
    v.onVoiceCommand((c) => commands.push(c));
    v.onVoiceResult((r) => results.push(r));
    await v.startVoice('command', { captureContext: true });
    emit('voice-result', { mode: 'command', text: 'send this to my phone', command: null, context: null, feedback: 'Nothing is selected to send to your phone. Select it first, then try again.', executed: false });
    expect(commands).toEqual([]);
    expect(results[0].feedback).toContain('Nothing is selected');
  });

  it('a note result reaches result subscribers but never command subscribers', async () => {
    const v = await fresh();
    const commands: VoiceCommand[] = [];
    const results: VoiceResult[] = [];
    v.onVoiceCommand((c) => commands.push(c));
    v.onVoiceResult((r) => results.push(r));
    await v.startVoice('note');
    emit('voice-result', { mode: 'note', text: 'Buy milk', command: null });
    expect(commands).toEqual([]);
    expect(results).toHaveLength(1);
  });

  it('a backend error shows the reason, then falls back to idle', async () => {
    const v = await fresh();
    await v.startVoice('note');
    emit('voice-state', { state: 'error', mode: 'note', error: 'Microphone access is turned off.' });
    expect(get(v.voiceState)).toMatchObject({ state: 'error', error: 'Microphone access is turned off.' });
    await vi.advanceTimersByTimeAsync(6100);
    expect(get(v.voiceState).state).toBe('idle');
  });

  it('a later state event cancels the pending error reset', async () => {
    const v = await fresh();
    await v.startVoice('note');
    emit('voice-state', { state: 'error', mode: 'note', error: 'boom' });
    emit('voice-state', { state: 'listening', mode: 'note' });
    await vi.advanceTimersByTimeAsync(7000);
    expect(get(v.voiceState).state).toBe('listening');
  });
});
