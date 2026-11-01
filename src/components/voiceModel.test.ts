import { describe, expect, it } from 'vitest';
import {
  CONTEXT_MODES,
  MEDIA_FEEDBACK,
  contextChip,
  fileName,
  planResults,
  resultIcon,
  resultVerb,
  resultsHeadline,
  transportFor,
} from './voiceModel';
import type { VoiceResult } from '../lib/voice';

describe('context modes', () => {
  it('warns that the clipboard fallback may show up in Windows clipboard history', () => {
    expect(CONTEXT_MODES.find((m) => m.id === 'selection+clipboard-fallback')?.hint).toContain('Win+V');
  });

  it('matches the backend ids and gives every mode a label and a hint', () => {
    expect(CONTEXT_MODES.map((m) => m.id)).toEqual(['off', 'selection', 'selection+clipboard-fallback']);
    for (const m of CONTEXT_MODES) expect(m.label && m.hint).toBeTruthy();
  });
});

describe('contextChip', () => {
  it('shows the captured text itself and says where it came from', () => {
    const c = contextChip({ kind: 'text', value: 'Quarterly revenue is up this year', app: 'notepad.exe' });
    expect(c?.icon).toBe('text');
    expect(c?.label).toBe('“Quarterly revenue is…”');
    expect(c?.title).toBe('Using your selection in notepad: “Quarterly revenue is up this year”');
    expect(contextChip({ kind: 'text', value: 'Hello' })?.title).toBe('Using your selection: “Hello”');
  });

  it('names a file, counts several, and falls back to the foreground app', () => {
    expect(contextChip({ kind: 'files', value: ['C:\\Users\\me\\Desktop\\report.pdf'] })?.label).toBe('report.pdf');
    const many = contextChip({ kind: 'files', value: ['a.txt', 'b.txt'], app: 'explorer.exe' });
    expect(many?.label).toBe('2 files');
    expect(many?.title).toBe('Using 2 files in explorer');
    expect(contextChip({ kind: 'app', value: 'chrome.exe' })).toMatchObject({ icon: 'monitor', label: 'In chrome' });
  });

  it('hides empty captures', () => {
    expect(contextChip({ kind: 'text', value: '   ' })).toBeNull();
    expect(contextChip({ kind: 'files', value: [] })).toBeNull();
    expect(contextChip({ kind: 'app', value: '' })).toBeNull();
    expect(contextChip(null)).toBeNull();
    expect(contextChip(undefined)).toBeNull();
  });
});

describe('transportFor', () => {
  it('never toggles the wrong way', () => {
    expect(transportFor('pause', true)).toBe('playpause');
    expect(transportFor('pause', false)).toBeNull();
    expect(transportFor('play', false)).toBe('playpause');
    expect(transportFor('play', true)).toBeNull();
    expect(transportFor('whats-playing', true)).toBeNull();
    expect(transportFor('next', false)).toBe('next');
    expect(transportFor('previous', true)).toBe('previous');
    expect(transportFor('playpause', true)).toBe('playpause');
  });

  it('has a feedback line for every media word', () => {
    for (const w of ['next', 'previous', 'play', 'pause', 'playpause', 'whats-playing'] as const) {
      expect(MEDIA_FEEDBACK[w]).toBeTruthy();
    }
  });
});

describe('fileName', () => {
  it('takes the last segment of either path style', () => {
    expect(fileName('C:\\a\\b\\c.txt')).toBe('c.txt');
    expect(fileName('/x/y/z.png')).toBe('z.png');
    expect(fileName('C:\\folder\\')).toBe('folder');
  });
});

describe('planResults', () => {
  const base: VoiceResult = { mode: 'command', text: '', command: null };

  it('returns rows with the backend line, and nothing when there are none', () => {
    const rows = [{ kind: 'file' as const, name: 'a.pdf', path: 'C:\\a.pdf', detail: '1 KB' }];
    expect(planResults({ ...base, feedback: ' Found 1 match. ', results: rows })).toEqual({ feedback: 'Found 1 match.', rows });
    expect(planResults({ ...base, feedback: 'Opened Downloads' })).toBeNull();
    expect(planResults({ ...base, results: [] })).toBeNull();
  });

  it('drops malformed rows', () => {
    const bad = [{ kind: 'file' as const, name: '', path: 'x' }, { kind: 'file' as const, name: 'ok', path: 'p' }];
    expect(planResults({ ...base, results: bad })?.rows).toHaveLength(1);
  });
});

describe('result rows', () => {
  it('picks a glyph by kind and file type', () => {
    expect(resultIcon({ kind: 'folder', name: 'photos' })).toBe('folder');
    expect(resultIcon({ kind: 'app', name: 'Notepad' })).toBe('appWindow');
    expect(resultIcon({ kind: 'file', name: 'moodboard.PNG' })).toBe('image');
    expect(resultIcon({ kind: 'file', name: 'invoice.pdf' })).toBe('fileText');
    expect(resultIcon({ kind: 'file', name: 'setup.exe' })).toBe('file');
  });

  it('says what a click does, and that files are only shown, never run', () => {
    expect(resultVerb('folder')).toBe('Open folder');
    expect(resultVerb('app')).toBe('Launch');
    expect(resultVerb('file')).toBe('Show in Explorer');
  });

  it('falls back to a count when the backend sent no line', () => {
    expect(resultsHeadline('', 1)).toBe('1 result');
    expect(resultsHeadline('', 3)).toBe('3 results');
    expect(resultsHeadline('Found 3 matches.', 3)).toBe('Found 3 matches.');
  });
});
