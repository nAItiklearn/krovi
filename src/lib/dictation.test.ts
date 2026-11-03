// Dictation in the browser demo (no Tauri) plus the pure reducers and helpers. The Tauri command and
// event wiring is covered in dictation.tauri.test.ts.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

async function fresh() {
  vi.resetModules();
  return await import('./dictation');
}

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe('demo: honest unavailable state', () => {
  it('lists the catalog with every model missing and marks dictation unavailable', async () => {
    const d = await fresh();
    const info = await d.loadDictation();
    expect(info.available).toBe(false);
    expect(info.loaded).toBe(true);
    expect(info.models.length).toBeGreaterThan(0);
    expect(info.models.every((m) => m.status.state === 'missing')).toBe(true);
    expect(info.downloading).toBeNull();
    expect(info.progress).toBeNull();
    expect(info.settings.enabled).toBe(false);
  });

  it('refuses downloads, saving and the mic test with one clear message, and never fakes progress', async () => {
    const d = await fresh();
    await d.loadDictation();
    await expect(d.downloadModel('base')).rejects.toThrow(d.DESKTOP_ONLY);
    await expect(d.saveDictationSettings(d.DEFAULT_DICTATION_SETTINGS)).rejects.toThrow(d.DESKTOP_ONLY);
    await expect(d.deleteModel('base')).rejects.toThrow(d.DESKTOP_ONLY);
    await expect(d.startMicTest(null, () => {})).rejects.toThrow(d.DESKTOP_ONLY);
    expect(get(d.dictationInfo).downloading).toBeNull();
    expect(get(d.dictationInfo).progress).toBeNull();
    expect(get(d.dictationLive)).toEqual({ phase: 'idle', level: 0, notice: null });
  });

  it('has no microphones, and cancel / toggle / cancel-download are silent no-ops', async () => {
    const d = await fresh();
    expect(await d.listInputDevices()).toEqual([]);
    await d.toggleDictation();
    await d.cancelDictation();
    await d.cancelDownload();
    expect(get(d.dictationLive).notice).toBeNull();
  });
});

describe('settings normalisation', () => {
  it('fills every missing field with a default and keeps snake_case options', async () => {
    const d = await fresh();
    expect(d.normalizeDictationSettings(null)).toEqual(d.DEFAULT_DICTATION_SETTINGS);
    expect(d.normalizeDictationSettings('junk')).toEqual(d.DEFAULT_DICTATION_SETTINGS);
    const s = d.normalizeDictationSettings({
      enabled: true,
      mode: 'toggle',
      device: '',
      vocabulary: ['Krovi', 7, 'Whisper'],
      options: { style: 'email', remove_fillers: false, replacements: [{ spoken: 'kro vee', written: 'Krovi' }, 5] },
    });
    expect(s).toMatchObject({ enabled: true, mode: 'toggle', device: null, vocabulary: ['Krovi', 'Whisper'] });
    expect(s.options).toEqual({
      style: 'email',
      remove_fillers: false,
      auto_punctuate: true,
      replacements: [
        { spoken: 'kro vee', written: 'Krovi' },
        { spoken: '', written: '' },
      ],
    });
  });

  it('ignores an unknown mode or style', async () => {
    const d = await fresh();
    const s = d.normalizeDictationSettings({ mode: 'sideways', options: { style: 'poetry' } });
    expect(s.mode).toBe('hold');
    expect(s.options.style).toBe('plain');
  });

  it('the Agent voice shortcut is off by default, on Ctrl+Alt+A, and old saved settings without it still load', async () => {
    const d = await fresh();
    expect(d.DEFAULT_DICTATION_SETTINGS).toMatchObject({ agentEnabled: false, agentShortcut: 'Ctrl+Alt+A' });
    // a dictation.v1 saved before the Agent shortcut existed has neither field
    const old = d.normalizeDictationSettings({ enabled: true, shortcut: 'Ctrl+Alt+D', mode: 'toggle' });
    expect(old).toMatchObject({ enabled: true, agentEnabled: false, agentShortcut: 'Ctrl+Alt+A' });
    // and it never shares the dictation shortcut by default
    expect(d.DEFAULT_DICTATION_SETTINGS.agentShortcut).not.toBe(d.DEFAULT_DICTATION_SETTINGS.shortcut);
  });

  it('keeps a saved Agent shortcut and ignores junk in those two fields', async () => {
    const d = await fresh();
    expect(d.normalizeDictationSettings({ agentEnabled: true, agentShortcut: 'Ctrl+Shift+J' })).toMatchObject({
      agentEnabled: true,
      agentShortcut: 'Ctrl+Shift+J',
    });
    expect(d.normalizeDictationSettings({ agentEnabled: 'yes', agentShortcut: 5 })).toMatchObject({
      agentEnabled: false,
      agentShortcut: 'Ctrl+Alt+A',
    });
  });

  it('normalises a model list and its three statuses', async () => {
    const d = await fresh();
    const models = d.normalizeModels([
      { id: 'base', displayName: 'Base', sizeBytes: 10, multilingual: true, license: 'MIT', status: { state: 'installed', path: 'C:\\m\\base.bin', bytes: 10 } },
      { id: 'tiny', status: { state: 'partial' } },
      { id: 'small', status: { state: 'nonsense' } },
      { nope: true },
    ]);
    expect(models.map((m) => [m.id, m.status.state])).toEqual([
      ['base', 'installed'],
      ['tiny', 'partial'],
      ['small', 'missing'],
    ]);
    expect(d.normalizeModels('x')).toEqual([]);
  });
});

describe('live state reducer', () => {
  it('follows listening -> loading-model -> transcribing -> idle', async () => {
    const d = await fresh();
    let s: ReturnType<typeof d.reduceLive> = { phase: 'idle', level: 0, notice: null };
    s = d.reduceLive(s, { state: 'listening' });
    expect(s).toMatchObject({ phase: 'listening', level: 0 });
    s = d.reduceLive(s, { state: 'listening', level: 0.2 });
    expect(s.level).toBe(0.2);
    s = d.reduceLive(s, { state: 'loading-model' });
    expect(s).toMatchObject({ phase: 'loading-model', level: 0 });
    s = d.reduceLive(s, { state: 'transcribing' });
    expect(s.phase).toBe('transcribing');
    s = d.reduceLive(s, { state: 'idle' });
    expect(s).toEqual({ phase: 'idle', level: 0, notice: null });
  });

  it('clamps nonsense levels and drops an old notice when a new session starts', async () => {
    const d = await fresh();
    const withNotice = { phase: 'idle' as const, level: 0, notice: { id: 1, kind: 'error' as const, text: 'old' } };
    expect(d.reduceLive(withNotice, { state: 'listening', level: 7 })).toEqual({ phase: 'listening', level: 1, notice: null });
    expect(d.reduceLive(withNotice, { state: 'listening', level: -1 }).level).toBe(0);
    expect(d.reduceLive(withNotice, { state: 'listening', level: Number.NaN }).level).toBe(0);
    // going back to idle keeps the notice so the pill can still show it
    expect(d.reduceLive(withNotice, { state: 'idle' }).notice).toEqual(withNotice.notice);
  });

  it('marks a spoken Agent command, and a dictation session after it is a plain dictation again', async () => {
    const d = await fresh();
    let s: ReturnType<typeof d.reduceLive> = { phase: 'idle', level: 0, notice: null };
    s = d.reduceLive(s, { state: 'listening', level: 0.1, mode: 'agent' });
    expect(s).toEqual({ phase: 'listening', level: 0.1, notice: null, agent: true });
    s = d.reduceLive(s, { state: 'transcribing', mode: 'agent' });
    expect(s).toMatchObject({ phase: 'transcribing', agent: true });
    s = d.reduceLive(s, { state: 'idle', mode: 'agent' });
    expect(s).toEqual({ phase: 'idle', level: 0, notice: null }); // idle drops the flag
    s = d.reduceLive(s, { state: 'listening', mode: 'dictate' });
    expect(s).toEqual({ phase: 'listening', level: 0, notice: null });
    // an event from a backend that predates the field is dictation
    expect(d.reduceLive(s, { state: 'listening' })).not.toHaveProperty('agent');
  });

  it('maps RMS to a 0..1 meter on a decibel scale', async () => {
    const d = await fresh();
    expect(d.meterLevel(0)).toBe(0);
    expect(d.meterLevel(-1)).toBe(0);
    expect(d.meterLevel(0.001)).toBe(0); // room noise
    expect(d.meterLevel(1)).toBe(1);
    expect(d.meterLevel(5)).toBe(1);
    const quiet = d.meterLevel(0.01);
    const speech = d.meterLevel(0.1);
    expect(quiet).toBeGreaterThan(0);
    expect(speech).toBeGreaterThan(quiet);
    expect(speech).toBeLessThan(1);
  });
});

describe('results become notices', () => {
  it('a clean success says nothing, and its text is never kept', async () => {
    const d = await fresh();
    expect(d.noticeFor({ ok: true, text: 'hello there', chars: 11, language: 'en' })).toBeNull();
    expect(d.noticeFor({ ok: true })).toBeNull();
  });

  it('nothing heard is a quiet info line', async () => {
    const d = await fresh();
    expect(d.noticeFor({ ok: true, nothingHeard: true })).toEqual({ kind: 'info', text: 'Didn’t catch that' });
  });

  it('an error without text is an error with the backend reason', async () => {
    const d = await fresh();
    expect(d.noticeFor({ ok: false, error: 'No microphone was found.' })).toEqual({ kind: 'error', text: 'No microphone was found.' });
    expect(d.noticeFor({ ok: false })?.text).toBeTruthy();
  });

  it('a voice command that failed or heard nothing says so as a command, and never offers Copy', async () => {
    const d = await fresh();
    expect(d.noticeFor({ ok: true, nothingHeard: true, mode: 'agent' })).toEqual({ kind: 'info', text: 'Didn’t catch that', agent: true });
    expect(d.noticeFor({ ok: false, error: 'Still working on the last voice command.', mode: 'agent' })).toEqual({
      kind: 'error',
      text: 'Still working on the last voice command.',
      agent: true,
    });
    expect(d.noticeFor({ ok: false, mode: 'agent' })?.text).toBe('The voice command didn’t work.');
    // a recognised command was handed to the Agent: nothing to announce
    expect(d.noticeFor({ ok: true, mode: 'agent' })).toBeNull();
    // dictation results carry no agent flag at all
    expect(d.noticeFor({ ok: false, error: 'boom', mode: 'dictate' })).toEqual({ kind: 'error', text: 'boom' });
  });

  it('typing blocked with text offers it for Copy', async () => {
    const d = await fresh();
    expect(d.noticeFor({ ok: false, text: 'Send the report', error: 'That window blocks typing.' })).toEqual({
      kind: 'blocked',
      text: 'That window blocks typing.',
      copy: 'Send the report',
    });
  });
});

describe('pill face', () => {
  it('is null when idle with no notice', async () => {
    const d = await fresh();
    expect(d.pillFace({ phase: 'idle', level: 0, notice: null })).toBeNull();
  });

  it('shows the live phases with a level only while listening', async () => {
    const d = await fresh();
    expect(d.pillFace({ phase: 'listening', level: 0.1, notice: null })).toMatchObject({ kind: 'listening', label: 'Listening' });
    expect(d.pillFace({ phase: 'loading-model', level: 0, notice: null })?.kind).toBe('loading-model');
    expect(d.pillFace({ phase: 'transcribing', level: 0, notice: null })?.kind).toBe('transcribing');
  });

  it('says "Listening for a command" for the Agent shortcut and plain "Listening" for dictation', async () => {
    const d = await fresh();
    expect(d.pillFace({ phase: 'listening', level: 0.1, notice: null, agent: true })).toMatchObject({ kind: 'listening', label: 'Listening for a command' });
    expect(d.pillFace({ phase: 'listening', level: 0.1, notice: null })).toMatchObject({ label: 'Listening' });
  });

  it('labels a failed voice command as one', async () => {
    const d = await fresh();
    const err = d.pillFace({ phase: 'idle', level: 0, notice: { id: 1, kind: 'error', text: 'No microphone was found.', agent: true } });
    expect(err).toMatchObject({ kind: 'notice', tone: 'error', label: 'Voice command failed', detail: 'No microphone was found.' });
    const dict = d.pillFace({ phase: 'idle', level: 0, notice: { id: 2, kind: 'error', text: 'x' } });
    expect(dict).toMatchObject({ label: 'Dictation failed' });
  });

  it('a live phase wins over a leftover notice', async () => {
    const d = await fresh();
    const live = { phase: 'transcribing' as const, level: 0, notice: { id: 1, kind: 'error' as const, text: 'x' } };
    expect(d.pillFace(live)?.kind).toBe('transcribing');
  });

  it('shows notices; only a blocked one carries the text to copy', async () => {
    const d = await fresh();
    expect(d.pillFace({ phase: 'idle', level: 0, notice: { id: 1, kind: 'info', text: 'Didn’t catch that' } })).toEqual({
      kind: 'notice',
      tone: 'info',
      label: 'Didn’t catch that',
      detail: '',
    });
    const blocked = d.pillFace({ phase: 'idle', level: 0, notice: { id: 2, kind: 'blocked', text: 'Blocked.', copy: 'hi' } });
    expect(blocked).toMatchObject({ kind: 'notice', tone: 'blocked', copy: 'hi' });
    const err = d.pillFace({ phase: 'idle', level: 0, notice: { id: 3, kind: 'error', text: 'Boom' } });
    expect(err).toMatchObject({ tone: 'error', detail: 'Boom' });
    expect(err && 'copy' in err ? err.copy : undefined).toBeUndefined();
  });
});

describe('download state reducers', () => {
  const base = () => ({
    available: true,
    loaded: true,
    settings: {} as never,
    models: [],
    modelsDir: null,
    downloading: null,
    progress: null,
    modelError: null,
  });

  it('progress marks the model as downloading and clears its old error', async () => {
    const d = await fresh();
    const prev = { ...base(), modelError: { id: 'base', message: 'old' } } as never;
    const next = d.reduceProgress(prev, { id: 'base', downloaded: 10, total: 40 });
    expect(next.downloading).toBe('base');
    expect(next.progress).toEqual({ id: 'base', downloaded: 10, total: 40 });
    expect(next.modelError).toBeNull();
    expect(d.progressFraction(next.progress)).toBe(0.25);
  });

  it('done clears the download; a failure keeps its text; a user cancel is not an error', async () => {
    const d = await fresh();
    const prev = d.reduceProgress(base() as never, { id: 'base', downloaded: 1, total: 2 });
    expect(d.reduceDone(prev, { id: 'base', ok: true })).toMatchObject({ downloading: null, progress: null, modelError: null });
    expect(d.reduceDone(prev, { id: 'base', ok: false, error: 'network unreachable' }).modelError).toEqual({ id: 'base', message: 'network unreachable' });
    expect(d.reduceDone(prev, { id: 'base', ok: false, error: 'download cancelled' }).modelError).toBeNull();
    expect(d.reduceDone(prev, { id: 'base', ok: false }).modelError?.message).toBeTruthy();
  });

  it('progress fraction is safe with no total', async () => {
    const d = await fresh();
    expect(d.progressFraction(null)).toBe(0);
    expect(d.progressFraction({ id: 'x', downloaded: 5, total: 0 })).toBe(0);
    expect(d.progressFraction({ id: 'x', downloaded: 99, total: 50 })).toBe(1);
  });
});

describe('formatting and list helpers', () => {
  it('formats sizes as binary megabytes and shortens the licence', async () => {
    const d = await fresh();
    expect(d.formatMB(59_707_625)).toBe('57 MB');
    expect(d.formatMB(190_085_487)).toBe('181 MB');
    expect(d.formatMB(10)).toBe('1 MB');
    expect(d.shortLicense('MIT, OpenAI Whisper weights via ggerganov/whisper.cpp')).toBe('MIT');
    expect(d.shortLicense('Apache-2.0')).toBe('Apache-2.0');
  });

  it('adds vocabulary terms trimmed, once, in any case', async () => {
    const d = await fresh();
    let v: string[] = [];
    v = d.addVocabularyTerm(v, '  Krovi  ');
    v = d.addVocabularyTerm(v, 'krovi');
    v = d.addVocabularyTerm(v, '   ');
    v = d.addVocabularyTerm(v, 'Whisper   cpp');
    expect(v).toEqual(['Krovi', 'Whisper cpp']);
    expect(d.removeAt(v, 0)).toEqual(['Whisper cpp']);
  });

  it('drops draft replacement rows with nothing spoken and trims the spoken side only', async () => {
    const d = await fresh();
    expect(
      d.cleanReplacements([
        { spoken: '  kro vee ', written: ' Krovi ' },
        { spoken: '   ', written: 'ignored' },
        { spoken: 'um', written: '' },
      ]),
    ).toEqual([
      { spoken: 'kro vee', written: ' Krovi ' },
      { spoken: 'um', written: '' },
    ]);
  });

  it('offers Auto-detect first among the languages', async () => {
    const d = await fresh();
    expect(d.DICTATION_LANGUAGES[0]).toEqual({ id: 'auto', label: 'Auto-detect' });
    expect(new Set(d.DICTATION_LANGUAGES.map((l) => l.id)).size).toBe(d.DICTATION_LANGUAGES.length);
  });
});
