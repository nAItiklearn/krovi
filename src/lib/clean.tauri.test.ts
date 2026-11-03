// Clean My OS inside the Tauri shell: the bridge is mocked, so this proves the command names, the
// arguments (ids only, never paths) and the event flow without a backend.
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Scan } from './clean';

const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
const handlers = new Map<string, (payload: unknown) => void>();
let respond: (cmd: string, args?: Record<string, unknown>) => unknown = () => undefined;

vi.mock('./bridge', () => ({
  isTauri: () => true,
  listen: async (event: string, cb: (payload: unknown) => void) => {
    handlers.set(event, cb);
    return () => handlers.delete(event);
  },
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    calls.push({ cmd, args });
    const r = respond(cmd, args);
    if (r instanceof Error) throw r;
    return r;
  },
}));

const emit = (event: string, payload: unknown) => handlers.get(event)?.(payload);

const backendScan = (): Scan => ({
  scanId: 3,
  cancelled: false,
  scannedAtMs: 1,
  categories: [
    {
      id: 'user-temp',
      bytes: 10,
      files: 2,
      items: [{ id: 'user-temp#1', category: 'user-temp', path: 'C:\\Users\\me\\AppData\\Local\\Temp\\a.tmp', label: null, isDir: false, bytes: 10, files: 1, modifiedMs: 5 }],
      unlistedItems: 0,
      unlistedBytes: 0,
      unlistedFiles: 0,
      skippedRecent: 0,
      skippedLinks: 0,
      skippedDeep: 0,
      unreadable: 0,
      rootsFound: 1,
    },
  ],
});

async function fresh() {
  vi.resetModules();
  return await import('./clean');
}

beforeEach(() => {
  calls.length = 0;
  handlers.clear();
  respond = () => undefined;
});

describe('tauri: commands', () => {
  it('categories() asks the backend and never falls back to demo data', async () => {
    const c = await fresh();
    respond = () => [{ id: 'user-temp' }];
    expect(await c.categories()).toEqual([{ id: 'user-temp' }]);
    expect(calls).toEqual([{ cmd: 'clean_categories', args: undefined }]);
  });

  it('scan() listens first, starts the scan with the category ids, and resolves on clean-done', async () => {
    const c = await fresh();
    const progress: string[] = [];
    const p = c.scan(['user-temp', 'thumbnails'], (x) => progress.push(`${x.category}:${x.files}`));
    // let the listeners register and the command go out
    await vi.waitFor(() => expect(calls.length).toBe(1));
    expect(handlers.has('clean-progress') && handlers.has('clean-done')).toBe(true);
    expect(calls[0]).toEqual({ cmd: 'clean_scan', args: { categories: ['user-temp', 'thumbnails'] } });

    emit('clean-progress', { category: 'user-temp', files: 7, bytes: 70 });
    emit('clean-done', { ok: true, cancelled: false, error: null, scan: backendScan() });
    const s = await p;
    expect(s.scanId).toBe(3);
    expect(s.demo).toBeUndefined();
    expect(progress).toEqual(['user-temp:7']);
    // listeners are removed afterwards
    expect(handlers.size).toBe(0);
  });

  it('scan() rejects with the backend message when the scan fails', async () => {
    const c = await fresh();
    const p = c.scan();
    p.catch(() => {});
    await vi.waitFor(() => expect(calls.length).toBe(1));
    emit('clean-done', { ok: false, cancelled: false, error: 'The scan stopped unexpectedly. Try again.', scan: null });
    await expect(p).rejects.toThrow('The scan stopped unexpectedly. Try again.');
    expect(handlers.size).toBe(0);
  });

  it('scan() rejects and cleans up when the command itself is refused (already running)', async () => {
    const c = await fresh();
    respond = (cmd) => (cmd === 'clean_scan' ? new Error('A scan or clean is already running. Wait for it to finish.') : undefined);
    await expect(c.scan()).rejects.toThrow('already running');
    expect(handlers.size).toBe(0);
  });

  it('scan() with no ids sends an empty list, which the backend reads as "all"', async () => {
    const c = await fresh();
    const p = c.scan();
    await vi.waitFor(() => expect(calls.length).toBe(1));
    expect(calls[0].args).toEqual({ categories: [] });
    emit('clean-done', { ok: true, cancelled: true, error: null, scan: { ...backendScan(), cancelled: true } });
    expect((await p).cancelled).toBe(true);
  });

  it('cancel() sends clean_cancel', async () => {
    const c = await fresh();
    await c.cancel();
    expect(calls).toEqual([{ cmd: 'clean_cancel', args: undefined }]);
  });

  it('deleteItems() sends the scan id and ids only, and passes progress through', async () => {
    const c = await fresh();
    // the backend takes a moment: progress arrives before the command returns
    let finish!: () => void;
    respond = () =>
      new Promise((resolve) => {
        finish = () =>
          resolve({ freedBytes: 10, deleted: 1, deletedFiles: 1, failed: undefined, alreadyGone: 0, cancelled: false, notAttempted: 0 });
      });
    const seen: number[] = [];
    const p = c.deleteItems(3, { itemIds: ['user-temp#1'], categoryIds: ['thumbnails'] }, (d) => seen.push(d.done));
    await vi.waitFor(() => expect(calls.length).toBe(1));
    emit('clean-delete-progress', { done: 1, total: 1 });
    finish();
    const r = await p;
    expect(calls[0]).toEqual({
      cmd: 'clean_delete',
      args: { scanId: 3, itemIds: ['user-temp#1'], categoryIds: ['thumbnails'] },
    });
    // no key anywhere in the arguments carries a path
    expect(Object.keys(calls[0].args!).sort()).toEqual(['categoryIds', 'itemIds', 'scanId']);
    expect(r.deleted).toBe(1);
    expect(r.failed).toEqual([]); // a missing list is normalised
    expect(r.demo).toBeUndefined();
    expect(seen).toEqual([1]);
    expect(handlers.size).toBe(0);
  });

  it('deleteItems() surfaces the backend refusal text', async () => {
    const c = await fresh();
    respond = () => new Error('This scan is out of date. Scan again before cleaning.');
    await expect(c.deleteItems(1, { itemIds: ['x'] })).rejects.toThrow('out of date');
  });

  it('recycleBinSize() asks the backend', async () => {
    const c = await fresh();
    respond = () => ({ bytes: 5, items: 2 });
    expect(await c.recycleBinSize()).toEqual({ bytes: 5, items: 2 });
    expect(calls[0].cmd).toBe('clean_recycle_bin_size');
  });
});
