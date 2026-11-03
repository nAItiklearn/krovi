// Clean My OS: the pure helpers and the browser-demo behaviour (no Tauri in a plain test run).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { invokeSpy, listenSpy } = vi.hoisted(() => ({
  invokeSpy: vi.fn(),
  listenSpy: vi.fn(async () => () => {}),
}));
vi.mock('./bridge', async (importOriginal) => {
  const real = await importOriginal<typeof import('./bridge')>();
  return { ...real, invoke: invokeSpy, listen: listenSpy };
});

import {
  DEMO_CATEGORIES,
  DEMO_ROOT,
  __test,
  allItems,
  cancel,
  categories,
  categoryState,
  confirmText,
  defaultSelection,
  deleteItems,
  formatCount,
  itemAge,
  itemDetail,
  itemTitle,
  plural,
  pruneSelection,
  recycleBinSize,
  scan,
  selectionBreakdown,
  selectionTotals,
  skipNotes,
  splitPath,
  summarizeReport,
  toggleCategory,
  toggleItem,
  type CategoryResult,
  type DeleteReport,
  type Scan,
  type ScanItem,
} from './clean';

const item = (id: string, category: string, bytes: number, over: Partial<ScanItem> = {}): ScanItem => ({
  id,
  category,
  path: `C:\\x\\${id}`,
  label: null,
  isDir: false,
  bytes,
  files: 1,
  modifiedMs: null,
  ...over,
});

const cat = (id: string, items: ScanItem[], over: Partial<CategoryResult> = {}): CategoryResult => ({
  id,
  bytes: items.reduce((n, i) => n + i.bytes, 0),
  files: items.reduce((n, i) => n + i.files, 0),
  items,
  unlistedItems: 0,
  unlistedBytes: 0,
  unlistedFiles: 0,
  skippedRecent: 0,
  skippedLinks: 0,
  skippedDeep: 0,
  unreadable: 0,
  rootsFound: 1,
  ...over,
});

const MB = 1024 * 1024;

const sample = (): Scan => ({
  scanId: 5,
  cancelled: false,
  scannedAtMs: 0,
  categories: [
    cat('user-temp', [item('user-temp#1', 'user-temp', 10 * MB, { files: 4, isDir: true }), item('user-temp#2', 'user-temp', 2 * MB)]),
    cat('installers', [item('installers#1', 'installers', 100 * MB)]),
    cat('thumbnails', []),
    cat('recycle-bin', [], { bytes: 5 * MB, files: 3 }),
  ],
});

describe('formatting', () => {
  it('counts and pluralises', () => {
    expect(formatCount(1234567)).toBe('1,234,567');
    expect(plural(1, 'item')).toBe('1 item');
    expect(plural(0, 'item')).toBe('0 items');
    expect(plural(2500, 'file')).toBe('2,500 files');
    expect(plural(2, 'child', 'children')).toBe('2 children');
  });

  it('the confirm sentence is exactly "Move N items (X MB) to the Recycle Bin"', () => {
    expect(confirmText(12, 12.4 * MB)).toBe('Move 12 items (12 MB) to the Recycle Bin');
    expect(confirmText(1, 512)).toBe('Move 1 item (512 B) to the Recycle Bin');
    expect(confirmText(3, 1.5 * 1024 * MB)).toBe('Move 3 items (1.5 GB) to the Recycle Bin');
    expect(confirmText(2, 4.2 * MB)).toBe('Move 2 items (4.2 MB) to the Recycle Bin');
  });

  it('splits Windows and forward-slash paths', () => {
    expect(splitPath('C:\\Users\\me\\Temp\\a.tmp')).toEqual({ dir: 'C:\\Users\\me\\Temp\\', name: 'a.tmp' });
    expect(splitPath('/tmp/x/y')).toEqual({ dir: '/tmp/x/', name: 'y' });
    expect(splitPath('plain')).toEqual({ dir: '', name: 'plain' });
    expect(splitPath('C:\\dir\\')).toEqual({ dir: 'C:\\dir\\', name: 'C:\\dir\\' });
  });

  it('titles, details and ages of items', () => {
    const file = item('a#1', 'a', 5, { path: 'C:\\t\\log.txt' });
    const dir = item('a#2', 'a', 5, { path: 'C:\\t\\cache', isDir: true, files: 41, label: 'Chrome / Default / Cache' });
    expect(itemTitle(file)).toBe('log.txt');
    expect(itemTitle(dir)).toBe('Chrome / Default / Cache');
    expect(itemDetail(file)).toBe('1 file');
    expect(itemDetail(dir)).toBe('folder, 41 files');
    expect(itemDetail({ ...dir, files: 1 })).toBe('folder, 1 file');
    expect(itemAge({ ...file, modifiedMs: 0 }, 3 * 86_400_000)).toBe('3d ago');
    expect(itemAge(file, 1)).toBe('');
  });

  it('says plainly what a scan left alone, and nothing when it left nothing', () => {
    expect(skipNotes(cat('a', []))).toEqual([]);
    const notes = skipNotes(
      cat('a', [], { skippedRecent: 1, skippedLinks: 2, skippedDeep: 1, unreadable: 17, unlistedItems: 3, unlistedBytes: 5 * MB }),
    );
    expect(notes).toHaveLength(5);
    expect(notes[0]).toBe('1 file left alone because it changed too recently.');
    expect(notes[1]).toContain('2 links');
    expect(notes[2]).toContain('1 very deeply nested folder');
    expect(notes[3]).toContain('17 items skipped');
    expect(notes[3]).toContain('never asks for administrator rights');
    expect(notes[4]).toBe('3 smaller items (5.0 MB) not listed and will not be touched. Scan again after cleaning to see them.');
  });

  it('summarises a clean run, a partial one and an empty one', () => {
    const base: DeleteReport = { freedBytes: 0, deleted: 0, deletedFiles: 0, failed: [], alreadyGone: 0, cancelled: false, notAttempted: 0 };
    const ok = summarizeReport({ ...base, deleted: 12, freedBytes: 12 * MB });
    expect(ok.headline).toBe('Moved 12 items (12 MB) to the Recycle Bin.');
    expect(ok.clean).toBe(true);
    expect(ok.lines.join(' ')).toContain('empty it');

    const part = summarizeReport({
      ...base,
      deleted: 1,
      freedBytes: MB,
      alreadyGone: 2,
      failed: [{ path: 'C:\\a', reason: 'It is in use by another program.' }],
    });
    expect(part.clean).toBe(false);
    expect(part.lines[0]).toBe('1 item could not be moved and was left where it was.');
    expect(part.lines).toContain('2 items had already disappeared.');

    const none = summarizeReport({ ...base, failed: [{ path: 'a', reason: 'r' }, { path: 'b', reason: 'r' }] });
    expect(none.headline).toBe('Nothing was moved.');
    expect(none.lines[0]).toBe('2 items could not be moved and were left where they were.');
    expect(none.lines.join(' ')).not.toContain('Recycle Bin. Disk space');

    const stopped = summarizeReport({ ...base, cancelled: true, notAttempted: 7 });
    expect(stopped.headline).toBe('Stopped before anything was moved.');
    expect(stopped.lines).toContain('You stopped it: 7 items were not touched.');
  });
});

describe('selection', () => {
  const infos = DEMO_CATEGORIES;

  it('starts with the default categories ticked and never the installers or the Recycle Bin', () => {
    const sel = defaultSelection(sample(), infos);
    expect([...sel].sort()).toEqual(['user-temp#1', 'user-temp#2']);
    expect(sel.has('installers#1')).toBe(false);
  });

  it('the demo categories keep the spec defaults', () => {
    const by = Object.fromEntries(infos.map((i) => [i.id, i]));
    expect(by['installers'].defaultSelected).toBe(false);
    expect(by['installers'].badge).toMatch(/review/i);
    expect(by['recycle-bin'].readOnly).toBe(true);
    expect(by['user-temp'].defaultSelected && by['windows-temp'].defaultSelected && by['browser-cache'].defaultSelected).toBe(true);
    expect(by['browser-cache'].riskNote).toMatch(/close the browser/i);
    expect(infos.map((i) => i.id)).toEqual(['user-temp', 'windows-temp', 'browser-cache', 'thumbnails', 'crash-reports', 'installers', 'recycle-bin']);
  });

  it('computes none / some / all per category, empty categories are always none', () => {
    const s = sample();
    const [temp, , empty] = s.categories;
    expect(categoryState(new Set(), temp)).toBe('none');
    expect(categoryState(new Set(['user-temp#1']), temp)).toBe('some');
    expect(categoryState(new Set(['user-temp#1', 'user-temp#2']), temp)).toBe('all');
    expect(categoryState(new Set(['x']), empty)).toBe('none');
  });

  it('toggles single items and whole categories without mutating the input', () => {
    const s = sample();
    const temp = s.categories[0];
    const base: ReadonlySet<string> = new Set(['user-temp#1']);
    const added = toggleItem(base, 'user-temp#2');
    expect([...base]).toEqual(['user-temp#1']);
    expect(added.has('user-temp#2')).toBe(true);
    expect(toggleItem(added, 'user-temp#2').has('user-temp#2')).toBe(false);

    expect(categoryState(toggleCategory(base, temp), temp)).toBe('all'); // some -> all
    expect(categoryState(toggleCategory(new Set(['user-temp#1', 'user-temp#2']), temp), temp)).toBe('none'); // all -> none
    expect(toggleCategory(new Set(['user-temp#1']), temp, false).size).toBe(0);
    expect(toggleCategory(new Set(), temp, true).size).toBe(2);
  });

  it('totals only what is ticked, and breaks it down by category', () => {
    const s = sample();
    const sel = new Set(['user-temp#1', 'installers#1', 'ghost#9']);
    expect(selectionTotals(s, sel)).toEqual({ items: 2, files: 5, bytes: 110 * MB });
    expect(selectionBreakdown(s, sel)).toEqual([
      { id: 'user-temp', items: 1, files: 4, bytes: 10 * MB },
      { id: 'installers', items: 1, files: 1, bytes: 100 * MB },
    ]);
    expect(selectionTotals(s, new Set())).toEqual({ items: 0, files: 0, bytes: 0 });
  });

  it('drops ticked ids that a newer scan no longer has', () => {
    expect([...pruneSelection(new Set(['user-temp#1', 'gone#1']), sample())]).toEqual(['user-temp#1']);
  });

  it('lists every item of a scan', () => {
    expect(allItems(sample()).map((i) => i.id)).toEqual(['user-temp#1', 'user-temp#2', 'installers#1']);
  });
});

describe('browser demo (no Tauri)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    __test.reset();
    invokeSpy.mockReset();
    listenSpy.mockClear();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  async function runScan(ids: string[] = [], onProgress?: Parameters<typeof scan>[1]) {
    const p = scan(ids, onProgress);
    await vi.runAllTimersAsync();
    return p;
  }

  it('serves the category list without touching the backend', async () => {
    const list = await categories();
    expect(list).toHaveLength(7);
    expect(list).toEqual(DEMO_CATEGORIES);
    list[0].title = 'changed';
    expect((await categories())[0].title).not.toBe('changed'); // a copy
    expect(invokeSpy).not.toHaveBeenCalled();
  });

  it('a demo scan is labelled demo, and every path says DEMO', async () => {
    const s = await runScan();
    expect(s.demo).toBe(true);
    expect(s.cancelled).toBe(false);
    expect(s.categories.map((c) => c.id)).toEqual(DEMO_CATEGORIES.map((c) => c.id));
    const items = allItems(s);
    expect(items.length).toBeGreaterThan(10);
    for (const it of items) {
      expect(it.path.startsWith(DEMO_ROOT)).toBe(true);
      expect(it.id).toBe(`${it.category}#${s.categories.find((c) => c.id === it.category)!.items.indexOf(it) + 1}`);
    }
    expect(invokeSpy).not.toHaveBeenCalled();
    expect(listenSpy).not.toHaveBeenCalled();
  });

  it('demo items are largest first with consistent totals; the Recycle Bin is size only', async () => {
    const s = await runScan();
    for (const c of s.categories) {
      const sizes = c.items.map((i) => i.bytes);
      expect(sizes).toEqual([...sizes].sort((a, b) => b - a));
      if (c.id !== 'recycle-bin') expect(c.bytes).toBe(sizes.reduce((n, b) => n + b, 0));
    }
    const bin = s.categories.find((c) => c.id === 'recycle-bin')!;
    expect(bin.items).toEqual([]);
    expect(bin.bytes).toBeGreaterThan(0);
  });

  it('can scan just some categories, reports progress, and ids restart from 1', async () => {
    const seen: string[] = [];
    const s = await runScan(['thumbnails', 'installers'], (p) => seen.push(p.category));
    expect(s.categories.map((c) => c.id)).toEqual(['thumbnails', 'installers']);
    expect(new Set(seen)).toEqual(new Set(['thumbnails', 'installers']));
    expect(s.categories[0].items[0].id).toBe('thumbnails#1');
  });

  it('stopping a demo scan returns what was found so far, flagged as cancelled', async () => {
    const p = scan();
    await vi.advanceTimersByTimeAsync(300);
    await cancel();
    await vi.runAllTimersAsync();
    const s = await p;
    expect(s.cancelled).toBe(true);
    expect(s.categories.length).toBeGreaterThan(0);
    expect(s.categories.length).toBeLessThan(DEMO_CATEGORIES.length);
  });

  it('a demo delete moves the ticked items and says it is a demo; nothing is invoked', async () => {
    const s = await runScan();
    const sel = defaultSelection(s, DEMO_CATEGORIES);
    const totals = selectionTotals(s, sel);
    const progress: number[] = [];
    const p = deleteItems(s.scanId, { itemIds: [...sel] }, (d) => progress.push(d.done));
    await vi.runAllTimersAsync();
    const r = await p;
    expect(r.demo).toBe(true);
    expect(r.deleted + r.failed.length).toBe(totals.items);
    expect(r.failed.length).toBeGreaterThan(0); // the demo shows one failure on purpose
    expect(r.failed[0].reason).toContain('in use');
    expect(r.freedBytes).toBeLessThan(totals.bytes);
    expect(progress[progress.length - 1]).toBe(totals.items);
    expect(invokeSpy).not.toHaveBeenCalled();
  });

  it('a demo delete refuses an old scan id and unknown ids, like the backend', async () => {
    const s = await runScan();
    await expect(deleteItems(s.scanId + 1, { itemIds: [] })).rejects.toThrow(/out of date/);
    const p = deleteItems(s.scanId, { itemIds: ['nope#1'] });
    await vi.runAllTimersAsync();
    const r = await p;
    expect(r.deleted).toBe(0);
    // the scan is spent after a delete
    await expect(deleteItems(s.scanId, { itemIds: [] })).rejects.toThrow(/out of date/);
  });

  it('a demo delete by category takes every item of that category', async () => {
    const s = await runScan();
    const n = s.categories.find((c) => c.id === 'thumbnails')!.items.length;
    const p = deleteItems(s.scanId, { itemIds: [], categoryIds: ['thumbnails'] });
    await vi.runAllTimersAsync();
    expect((await p).deleted).toBe(n);
  });

  it('the Recycle Bin size is a made-up number in the demo', async () => {
    const size = await recycleBinSize();
    expect(size!.bytes).toBeGreaterThan(0);
    expect(invokeSpy).not.toHaveBeenCalled();
  });
});
