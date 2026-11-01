import { describe, expect, it } from 'vitest';
import { HELP_FAMILIES, HELP_LIMITS, filterTargets, targetsStatus } from './voiceHelp';
import type { VoiceTargets } from '../lib/voice';

const T: VoiceTargets = {
  ready: true,
  folders: [
    { name: 'Desktop', path: 'C:\\U\\Desktop' },
    { name: 'Downloads', path: 'C:\\U\\Downloads' },
    { name: 'Krovi downloads', path: 'C:\\U\\Downloads\\Krovi' },
  ],
  apps: ['Calculator', 'Notepad', 'Paint', 'Visual Studio Code'],
};

describe('help families', () => {
  it('covers every phrasing family of SPEC 11.13 with a template and examples', () => {
    expect(HELP_FAMILIES.map((f) => f.id)).toEqual([
      'tools', 'timers', 'notes', 'reminders', 'this', 'media', 'folders', 'apps', 'find', 'close',
    ]);
    for (const f of HELP_FAMILIES) {
      expect(f.title && f.template).toBeTruthy();
      expect(f.examples.length).toBeGreaterThan(0);
    }
    expect(new Set(HELP_FAMILIES.map((f) => f.id)).size).toBe(HELP_FAMILIES.length);
  });

  it('keeps the examples SPEC lists verbatim', () => {
    const all = HELP_FAMILIES.flatMap((f) => f.examples);
    for (const s of ['open downloads', 'launch visual studio code', 'search for tax return', 'what’s in downloads', 'start a timer for five minutes']) {
      expect(all).toContain(s);
    }
  });

  it('says what voice will not do', () => {
    expect(HELP_LIMITS).toMatch(/never guessed/);
  });
});

describe('target status', () => {
  it('is loading until an answer, partial while apps scan, then ready', () => {
    expect(targetsStatus(null)).toBe('loading');
    expect(targetsStatus({ ...T, ready: false })).toBe('partial');
    expect(targetsStatus(T)).toBe('ready');
  });
});

describe('filterTargets', () => {
  it('shows everything without a query', () => {
    const v = filterTargets(T, '');
    expect(v.folders).toHaveLength(3);
    expect(v.apps).toHaveLength(4);
    expect(v.moreApps).toBe(0);
    expect(v.empty).toBe(false);
  });

  it('filters folders and apps together, case-insensitively', () => {
    const v = filterTargets(T, 'DOWN');
    expect(v.folders.map((f) => f.name)).toEqual(['Downloads', 'Krovi downloads']);
    expect(v.apps).toEqual([]);
    expect(filterTargets(T, 'pad').apps).toEqual(['Notepad']);
  });

  it('caps long app lists and reports how many are hidden', () => {
    const many: VoiceTargets = { ready: true, folders: [], apps: Array.from({ length: 120 }, (_, i) => `App ${i}`) };
    const v = filterTargets(many, '', 48);
    expect(v.apps).toHaveLength(48);
    expect(v.moreApps).toBe(72);
  });

  it('flags a query with no match and tolerates no answer yet', () => {
    expect(filterTargets(T, 'zzz').empty).toBe(true);
    expect(filterTargets(null, '').folders).toEqual([]);
    expect(filterTargets(null, 'x').empty).toBe(true);
  });
});
