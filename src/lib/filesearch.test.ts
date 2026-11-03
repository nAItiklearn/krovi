// Local file-name search in the browser demo, plus the pure helpers (the Rust crate is the authority
// and walks scratch folders in its own tests).
import { describe, expect, it } from 'vitest';
import {
  MAX_QUERY_CHARS,
  canOpen,
  describeStop,
  openHit,
  parentDir,
  parseQuery,
  skipNotes,
  startSearch,
  type Hit,
  type Summary,
} from './filesearch';

const summary = (over: Partial<Summary> = {}): Summary => ({
  stop: 'finished',
  scanned: 10,
  found: 2,
  skippedLinks: 0,
  skippedHidden: 0,
  skippedDeep: 0,
  unreadable: 0,
  ...over,
});

describe('queries (the same rules as the backend)', () => {
  it('are split and lower-cased', () => {
    expect(parseQuery('  Tax  2024 ')).toEqual({ words: ['tax', '2024'], error: null });
    expect(parseQuery('ab').error).toBeNull();
  });

  it('are refused when too short, too long, too many words or with control characters', () => {
    expect(parseQuery('a').error).toContain('at least');
    expect(parseQuery('   ').error).toContain('at least');
    expect(parseQuery('a\u0000b').error).toContain('control');
    expect(parseQuery('x'.repeat(MAX_QUERY_CHARS + 1)).error).toContain('too long');
    expect(parseQuery('a b c d e f g').error).toContain('at most');
  });
});

describe('results', () => {
  it('show the folder a file is in', () => {
    expect(parentDir('C:\\Users\\me\\Documents\\a.txt')).toBe('C:\\Users\\me\\Documents');
    expect(parentDir('a.txt')).toBe('a.txt');
  });

  it('offer Open for documents and folders but never for programs and scripts', () => {
    expect(canOpen({ name: 'notes.txt', isDir: false })).toBe(true);
    expect(canOpen({ name: 'archive', isDir: false })).toBe(true);
    expect(canOpen({ name: 'Holidays', isDir: true })).toBe(true);
    for (const n of ['setup.exe', 'run.BAT', 'x.ps1', 'link.lnk', 'a.js', 'b.msi', 'c.hta']) {
      expect(canOpen({ name: n, isDir: false }), n).toBe(false);
    }
    // a folder that happens to be called x.exe is still just a folder
    expect(canOpen({ name: 'x.exe', isDir: true })).toBe(true);
  });

  it('explain how a search ended, and what it left alone', () => {
    expect(describeStop(summary())).toBe('');
    expect(describeStop(summary({ stop: 'cancelled' }))).toContain('Stopped');
    expect(describeStop(summary({ stop: 'result-limit', found: 200 }))).toContain('first 200');
    expect(describeStop(summary({ stop: 'time-limit' }))).toContain('20 seconds');
    expect(describeStop(summary({ stop: 'entry-limit' }))).toContain('very large number');
    expect(skipNotes(summary())).toEqual([]);
    expect(skipNotes(summary({ skippedHidden: 1, skippedLinks: 2, skippedDeep: 3, unreadable: 1 }))).toEqual([
      '1 hidden or system item skipped.',
      '2 links and cloud placeholders not followed.',
      '3 very deep folders not entered.',
      '1 item could not be read.',
    ]);
  });
});

describe('DEMO search', () => {
  const collect = async (query: string, folders: Parameters<typeof startSearch>[1] = []) => {
    const batches: Hit[][] = [];
    const h = await startSearch(query, folders, (hits) => batches.push(hits));
    const s = await h.done;
    return { batches, s, h };
  };

  it('rejects a bad query before anything starts', async () => {
    await expect(startSearch('a', [], () => {})).rejects.toThrow('at least');
  });

  it('streams matching names from the made-up list and labels the summary as a demo', async () => {
    const { batches, s } = await collect('invoice', ['downloads']);
    expect(batches.flat().map((h) => h.name)).toEqual(['invoice-march.pdf', 'invoice-april.pdf']);
    expect(batches.length).toBe(2); // streamed one at a time, not all at the end
    expect(batches.flat().map((h) => h.id)).toEqual([1, 2]);
    expect(batches.flat().every((h) => h.path.startsWith('C:\\DEMO\\'))).toBe(true);
    expect(s).toMatchObject({ stop: 'finished', found: 2, demo: true });
  });

  it('matches every word, in any order, and finds folders too', async () => {
    const { batches } = await collect('return tax');
    expect(batches.flat().map((h) => h.name)).toEqual(['tax return 2024.pdf']);
    const folders = (await collect('holidays', ['pictures'])).batches.flat();
    expect(folders.find((h) => h.isDir)).toMatchObject({ name: 'Holidays', size: null });
  });

  it('stays inside the folders asked for', async () => {
    const { batches } = await collect('invoice', ['documents']);
    expect(batches).toEqual([]);
  });

  it('can be stopped, and keeps what it found', async () => {
    const batches: Hit[][] = [];
    const h = await startSearch('ee', [], (hits) => batches.push(hits));
    await h.cancel();
    const s = await h.done;
    expect(s.stop).toBe('cancelled');
    expect(s.found).toBe(batches.flat().length);
  });

  it('opens nothing in the preview and says so', async () => {
    const hit: Hit = { id: 1, name: 'a.txt', path: 'C:\\DEMO\\a.txt', folder: 'documents', isDir: false, size: 1, modifiedMs: 1 };
    expect(await openHit(1, hit, false)).toContain('DEMO');
    expect(await openHit(1, hit, true)).toContain('File Explorer');
  });
});
