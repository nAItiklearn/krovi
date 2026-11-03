// Dictation inside the Tauri shell: the bridge is mocked, so this proves the command names and
// arguments, the event -> store wiring, the mic-test polling and the cancel/notice behaviour.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
const handlers = new Map<string, (payload: unknown) => void>();
const errors = new Map<string, string>();
let levels: (number | null)[] = [];
let status: unknown;

const catalog = (installed: string[]) =>
  ['tiny', 'base', 'small', 'base.en'].map((id) => ({
    id,
    displayName: id,
    sizeBytes: 1000,
    multilingual: id !== 'base.en',
    license: 'MIT, test',
    status: installed.includes(id) ? { state: 'installed', path: `C:\\m\\${id}.bin`, bytes: 1000 } : { state: 'missing' },
  }));

const settingsJson = (over: Record<string, unknown> = {}) => ({
  enabled: false,
  shortcut: 'Ctrl+Alt+D',
  mode: 'hold',
  model: 'base',
  language: 'auto',
  device: null,
  vocabulary: [],
  options: { style: 'plain', replacements: [], remove_fillers: true, auto_punctuate: true },
  ...over,
});

const statusJson = (installed: string[] = [], over: Record<string, unknown> = {}) => ({
  settings: settingsJson(),
  recording: false,
  busy: false,
  modelLoaded: false,
  downloading: null,
  models: catalog(installed),
  modelsDir: 'C:\\Users\\me\\AppData\\Roaming\\Krovi\\models',
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
    const err = errors.get(cmd);
    if (err) throw err;
    switch (cmd) {
      case 'dictation_status':
        return status;
      case 'dictation_set_settings':
        return args?.settings;
      case 'voice_input_devices':
        return [
          { id: 'dev-1', name: 'Headset (USB)', isDefault: true },
          { id: 'dev-2', name: 'Laptop mic', isDefault: false },
        ];
      case 'mic_test_level':
        return levels.length ? levels.shift() : null;
      case 'voice_model_delete':
        return catalog([]);
      default:
        return undefined;
    }
  },
}));

const emit = (event: string, payload: unknown) => handlers.get(event)?.(payload);
const cmds = () => calls.map((c) => c.cmd);

async function fresh() {
  vi.resetModules();
  return await import('./dictation');
}

beforeEach(() => {
  vi.useFakeTimers();
  calls.length = 0;
  handlers.clear();
  errors.clear();
  levels = [];
  status = statusJson();
});
afterEach(() => {
  vi.useRealTimers();
});

describe('tauri: status and settings', () => {
  it('dictation_status fills the store (settings, models, folder) and subscribes to the four events', async () => {
    status = statusJson(['base']);
    const d = await fresh();
    const info = await d.loadDictation();
    expect(cmds()).toEqual(['dictation_status']);
    expect(info).toMatchObject({ available: true, loaded: true, downloading: null, modelsDir: 'C:\\Users\\me\\AppData\\Roaming\\Krovi\\models' });
    expect(info.models.map((m) => [m.id, m.status.state])).toEqual([
      ['tiny', 'missing'],
      ['base', 'installed'],
      ['small', 'missing'],
      ['base.en', 'missing'],
    ]);
    await d.wireDictation();
    expect([...handlers.keys()].sort()).toEqual(['dictation-result', 'dictation-state', 'voice-model-done', 'voice-model-progress']);
  });

  it('wires the events only once', async () => {
    const d = await fresh();
    await d.loadDictation();
    await d.loadDictation();
    await d.wireDictation();
    expect(handlers.size).toBe(4);
  });

  it('dictation_set_settings gets the whole object (snake_case options) and the saved copy lands in the store', async () => {
    const d = await fresh();
    await d.loadDictation();
    const next = { ...d.DEFAULT_DICTATION_SETTINGS, enabled: true, mode: 'toggle' as const, options: { ...d.DEFAULT_DICTATION_SETTINGS.options, remove_fillers: false } };
    const saved = await d.saveDictationSettings(next);
    const call = calls.find((c) => c.cmd === 'dictation_set_settings');
    expect(call?.args).toEqual({ settings: next });
    expect(Object.keys((call?.args?.settings as { options: object }).options)).toEqual(['style', 'replacements', 'remove_fillers', 'auto_punctuate']);
    expect(saved.enabled).toBe(true);
    expect(get(d.dictationInfo).settings.mode).toBe('toggle');
  });

  it('the Agent shortcut settings go to the backend inside the same dictation settings object', async () => {
    const d = await fresh();
    await d.loadDictation();
    const next = { ...d.DEFAULT_DICTATION_SETTINGS, agentEnabled: true, agentShortcut: 'Ctrl+Alt+J' };
    const saved = await d.saveDictationSettings(next);
    const sent = calls.find((c) => c.cmd === 'dictation_set_settings')?.args?.settings as { agentEnabled: boolean; agentShortcut: string };
    expect(sent).toMatchObject({ agentEnabled: true, agentShortcut: 'Ctrl+Alt+J' });
    expect(saved).toMatchObject({ agentEnabled: true, agentShortcut: 'Ctrl+Alt+J' });
    expect(get(d.dictationInfo).settings.agentShortcut).toBe('Ctrl+Alt+J');
  });

  it('an Agent shortcut the backend refuses rejects with its text and keeps the stored one', async () => {
    const d = await fresh();
    await d.loadDictation();
    errors.set('dictation_set_settings', 'Dictation and the Agent shortcut need different keys.');
    await expect(d.saveDictationSettings({ ...d.DEFAULT_DICTATION_SETTINGS, enabled: true, agentEnabled: true, agentShortcut: 'Ctrl+Alt+D' })).rejects.toThrow('different keys');
    expect(get(d.dictationInfo).settings.agentEnabled).toBe(false);
  });

  it('a refused save rejects with the backend text and leaves the stored settings alone', async () => {
    const d = await fresh();
    await d.loadDictation();
    errors.set('dictation_set_settings', 'Could not use Ctrl+Alt+D as the dictation shortcut: already registered');
    await expect(d.saveDictationSettings({ ...d.DEFAULT_DICTATION_SETTINGS, enabled: true })).rejects.toThrow('already registered');
    expect(get(d.dictationInfo).settings.enabled).toBe(false);
  });

  it('a failing status read rejects with a readable message', async () => {
    errors.set('dictation_status', 'no state');
    const d = await fresh();
    await expect(d.loadDictation()).rejects.toThrow('no state');
  });

  it('toggle and cancel call their commands; cancel resets the pill at once', async () => {
    const d = await fresh();
    await d.toggleDictation();
    emit('dictation-state', { state: 'listening', level: 0.1 });
    await d.wireDictation();
    emit('dictation-state', { state: 'transcribing' });
    expect(get(d.dictationLive).phase).toBe('transcribing');
    await d.cancelDictation();
    expect(cmds()).toEqual(['dictation_toggle', 'dictation_cancel']);
    expect(get(d.dictationLive)).toMatchObject({ phase: 'idle', level: 0 });
  });
});

describe('tauri: events drive the live store', () => {
  it('state events move the pill through the phases', async () => {
    const d = await fresh();
    await d.wireDictation();
    emit('dictation-state', { state: 'listening' });
    emit('dictation-state', { state: 'listening', level: 0.25 });
    expect(get(d.dictationLive)).toMatchObject({ phase: 'listening', level: 0.25 });
    emit('dictation-state', { state: 'loading-model' });
    expect(get(d.dictationLive).phase).toBe('loading-model');
    emit('dictation-state', { state: 'transcribing' });
    emit('dictation-state', { state: 'idle' });
    expect(get(d.dictationLive)).toEqual({ phase: 'idle', level: 0, notice: null });
  });

  it('an Agent-shortcut session says it is a command on the pill, and failures are labelled as one', async () => {
    const d = await fresh();
    await d.wireDictation();
    emit('dictation-state', { state: 'listening', level: 0.3, mode: 'agent' });
    expect(d.pillFace(get(d.dictationLive))).toMatchObject({ kind: 'listening', label: 'Listening for a command' });
    emit('dictation-state', { state: 'transcribing', mode: 'agent' });
    expect(get(d.dictationLive)).toMatchObject({ phase: 'transcribing', agent: true });
    emit('dictation-state', { state: 'idle', mode: 'agent' });
    emit('dictation-result', { ok: false, error: 'Download a speech model first (Settings › Voice).', mode: 'agent' });
    expect(d.pillFace(get(d.dictationLive))).toMatchObject({ tone: 'error', label: 'Voice command failed' });
    // the next dictation is a plain dictation again
    emit('dictation-state', { state: 'listening', mode: 'dictate' });
    expect(d.pillFace(get(d.dictationLive))).toMatchObject({ label: 'Listening' });
  });

  it('a recognised voice command is never announced and its words are not kept in the pill store', async () => {
    const d = await fresh();
    await d.wireDictation();
    emit('dictation-state', { state: 'idle', mode: 'agent' });
    emit('dictation-result', { ok: true, text: 'a private command', chars: 16, mode: 'agent' });
    expect(get(d.dictationLive).notice).toBeNull();
    expect(JSON.stringify(get(d.dictationLive))).not.toContain('private');
  });

  it('an error result becomes a pill notice AND a toast, then expires on its own', async () => {
    const d = await fresh();
    const toasts: [string, number | undefined, string | undefined][] = [];
    d.setDictationToaster((t, ms, tone) => toasts.push([t, ms, tone]));
    await d.wireDictation();
    emit('dictation-result', { ok: false, error: 'Download a speech model first (Settings › Voice).' });
    expect(get(d.dictationLive).notice).toMatchObject({ kind: 'error', text: 'Download a speech model first (Settings › Voice).' });
    expect(toasts).toEqual([['Download a speech model first (Settings › Voice).', 3600, 'error']]);
    await vi.advanceTimersByTimeAsync(5300);
    expect(get(d.dictationLive).notice).toBeNull();
  });

  it('nothing heard shows "Didn’t catch that" in the pill only', async () => {
    const d = await fresh();
    const toasts: string[] = [];
    d.setDictationToaster((t) => toasts.push(t));
    await d.wireDictation();
    emit('dictation-result', { ok: true, nothingHeard: true });
    expect(get(d.dictationLive).notice).toMatchObject({ kind: 'info', text: 'Didn’t catch that' });
    expect(toasts).toEqual([]);
    await vi.advanceTimersByTimeAsync(2500);
    expect(get(d.dictationLive).notice).toBeNull();
  });

  it('a typed success leaves no notice and keeps no text', async () => {
    const d = await fresh();
    await d.wireDictation();
    emit('dictation-result', { ok: true, text: 'a private sentence', chars: 18, language: 'en' });
    expect(get(d.dictationLive).notice).toBeNull();
    expect(JSON.stringify(get(d.dictationLive))).not.toContain('private');
  });

  it('blocked typing keeps the text for Copy only until the notice is dismissed or expires', async () => {
    const d = await fresh();
    await d.wireDictation();
    emit('dictation-result', { ok: false, text: 'Meet at noon', error: 'That app blocks typed text.' });
    const face = d.pillFace(get(d.dictationLive));
    expect(face).toMatchObject({ kind: 'notice', tone: 'blocked', copy: 'Meet at noon' });
    d.dismissNotice();
    expect(get(d.dictationLive).notice).toBeNull();

    emit('dictation-result', { ok: false, text: 'Second', error: 'blocked' });
    expect(get(d.dictationLive).notice?.copy).toBe('Second');
    await vi.advanceTimersByTimeAsync(15100);
    expect(get(d.dictationLive).notice).toBeNull();
  });

  it('a new session replaces a standing notice and cancels its timer', async () => {
    const d = await fresh();
    await d.wireDictation();
    emit('dictation-result', { ok: false, error: 'boom' });
    emit('dictation-state', { state: 'listening' });
    expect(get(d.dictationLive).notice).toBeNull();
    await vi.advanceTimersByTimeAsync(6000);
    expect(get(d.dictationLive).phase).toBe('listening');
  });
});

describe('tauri: models', () => {
  it('download asks for the id, shows progress from events, and refreshes the list when done', async () => {
    const d = await fresh();
    await d.loadDictation();
    await d.downloadModel('small');
    expect(calls.find((c) => c.cmd === 'voice_model_download')?.args).toEqual({ id: 'small' });
    expect(get(d.dictationInfo)).toMatchObject({ downloading: 'small', progress: { id: 'small', downloaded: 0, total: 1000 } });

    emit('voice-model-progress', { id: 'small', downloaded: 400, total: 1000 });
    expect(d.progressFraction(get(d.dictationInfo).progress)).toBe(0.4);

    status = statusJson(['small']);
    emit('voice-model-done', { id: 'small', ok: true });
    await vi.advanceTimersByTimeAsync(0);
    const info = get(d.dictationInfo);
    expect(info.downloading).toBeNull();
    expect(info.progress).toBeNull();
    expect(info.models.find((m) => m.id === 'small')?.status.state).toBe('installed');
  });

  it('the first model downloaded becomes the selected one when the chosen model is not installed', async () => {
    const d = await fresh();
    await d.loadDictation(); // default model "base", nothing installed
    status = statusJson(['tiny']);
    emit('voice-model-done', { id: 'tiny', ok: true });
    await vi.advanceTimersByTimeAsync(0);
    const save = calls.filter((c) => c.cmd === 'dictation_set_settings');
    expect(save).toHaveLength(1);
    expect((save[0].args?.settings as { model: string }).model).toBe('tiny');
    expect(get(d.dictationInfo).settings.model).toBe('tiny');
  });

  it('does not change the selection when the chosen model is already installed', async () => {
    const d = await fresh();
    status = statusJson(['base']);
    await d.loadDictation();
    status = statusJson(['base', 'tiny']);
    emit('voice-model-done', { id: 'tiny', ok: true });
    await vi.advanceTimersByTimeAsync(0);
    expect(cmds()).not.toContain('dictation_set_settings');
  });

  it('a failed download shows its error on that model; a cancel does not', async () => {
    const d = await fresh();
    await d.loadDictation();
    await d.downloadModel('base');
    emit('voice-model-done', { id: 'base', ok: false, error: 'network error: timed out' });
    expect(get(d.dictationInfo).modelError).toEqual({ id: 'base', message: 'network error: timed out' });

    await d.downloadModel('base');
    expect(get(d.dictationInfo).modelError).toBeNull();
    await d.cancelDownload();
    expect(cmds()).toContain('voice_model_cancel_download');
    emit('voice-model-done', { id: 'base', ok: false, error: 'download cancelled' });
    expect(get(d.dictationInfo).modelError).toBeNull();
    expect(get(d.dictationInfo).downloading).toBeNull();
  });

  it('a download the backend refuses surfaces its text on the row', async () => {
    const d = await fresh();
    await d.loadDictation();
    errors.set('voice_model_download', 'A model is already downloading.');
    await d.downloadModel('small');
    expect(get(d.dictationInfo).modelError).toEqual({ id: 'small', message: 'A model is already downloading.' });
    expect(get(d.dictationInfo).downloading).toBeNull();
  });

  it('delete passes the id and takes the fresh list from the backend', async () => {
    status = statusJson(['base']);
    const d = await fresh();
    await d.loadDictation();
    await d.deleteModel('base');
    expect(calls.find((c) => c.cmd === 'voice_model_delete')?.args).toEqual({ id: 'base' });
    expect(get(d.dictationInfo).models.every((m) => m.status.state === 'missing')).toBe(true);
  });

  it('delete rejects with the backend text', async () => {
    const d = await fresh();
    await d.loadDictation();
    errors.set('voice_model_delete', 'no data folder');
    await expect(d.deleteModel('base')).rejects.toThrow('no data folder');
  });
});

describe('tauri: microphones and the mic test', () => {
  it('lists devices as {id, name, isDefault}', async () => {
    const d = await fresh();
    expect(await d.listInputDevices()).toEqual([
      { id: 'dev-1', name: 'Headset (USB)', isDefault: true },
      { id: 'dev-2', name: 'Laptop mic', isDefault: false },
    ]);
  });

  it('starts with the device (null = Windows default), polls about ten times a second, then stops', async () => {
    levels = [0.02, 0.2, 0.05];
    const d = await fresh();
    const got: number[] = [];
    const test = await d.startMicTest(null, (l) => got.push(l));
    expect(calls[0]).toEqual({ cmd: 'mic_test_start', args: { device: null } });
    await vi.advanceTimersByTimeAsync(d.MIC_POLL_MS * 3 + 5);
    expect(got).toEqual([0.02, 0.2, 0.05]);
    expect(calls.filter((c) => c.cmd === 'mic_test_level')).toHaveLength(3);
    await test.stop();
    await test.stop(); // idempotent
    expect(calls.filter((c) => c.cmd === 'mic_test_stop')).toHaveLength(1);
    const before = calls.length;
    await vi.advanceTimersByTimeAsync(1000);
    expect(calls.length).toBe(before); // no polling after stop
  });

  it('passes a chosen device id through', async () => {
    const d = await fresh();
    const test = await d.startMicTest('dev-2', () => {});
    expect(calls[0].args).toEqual({ device: 'dev-2' });
    await test.stop();
  });

  it('calls onEnd and stops polling when the backend ended the test by itself (null level)', async () => {
    levels = [0.1];
    const d = await fresh();
    const got: number[] = [];
    let ended = 0;
    await d.startMicTest(null, (l) => got.push(l), () => ended++);
    await vi.advanceTimersByTimeAsync(d.MIC_POLL_MS * 5);
    expect(got).toEqual([0.1]);
    expect(ended).toBe(1);
    const polls = calls.filter((c) => c.cmd === 'mic_test_level').length;
    await vi.advanceTimersByTimeAsync(1000);
    expect(calls.filter((c) => c.cmd === 'mic_test_level').length).toBe(polls);
  });

  it('a microphone that cannot be opened rejects with the reason and starts no polling', async () => {
    errors.set('mic_test_start', 'Microphone access is turned off in Windows.');
    const d = await fresh();
    await expect(d.startMicTest(null, () => {})).rejects.toThrow('Microphone access is turned off');
    await vi.advanceTimersByTimeAsync(1000);
    expect(cmds()).toEqual(['mic_test_start']);
  });
});
