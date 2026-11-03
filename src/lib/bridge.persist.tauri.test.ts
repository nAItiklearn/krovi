// Native persistence of the file shelf and world clocks: the Rust store is the source of
// truth, and a list left in localStorage by an older build is migrated exactly once.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
vi.mock('@tauri-apps/api/core', () => ({
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    calls.push({ cmd, args });
    if (cmd === 'save_shelves') return args?.shelves;
    if (cmd === 'load_shelves') return [{ name: 'Desk', items: [] }];
    if (cmd === 'save_world_clocks') return args?.clocks;
    if (cmd === 'load_world_clocks') return [{ label: 'Local', tz: '' }];
    throw new Error(`unexpected ${cmd}`);
  },
}));

const mem = new Map<string, string>();
beforeEach(() => {
  calls.length = 0;
  mem.clear();
  vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
  vi.stubGlobal('localStorage', {
    getItem: (k: string) => mem.get(k) ?? null,
    setItem: (k: string, v: string) => void mem.set(k, v),
    removeItem: (k: string) => void mem.delete(k),
  });
});
afterEach(() => vi.unstubAllGlobals());

const shelf = [{ name: 'Work', items: [{ id: 1, name: 'a.pdf', stored_path: 'C:\\a.pdf', size_bytes: 3, added_at: 9 }] }];

describe('file shelf persistence (native)', () => {
  it('migrates a legacy localStorage shelf once, then reads the store', async () => {
    const { bridge, SHELVES_KEY } = await import('./bridge');
    mem.set(SHELVES_KEY, JSON.stringify(shelf));
    expect(await bridge.loadShelves()).toEqual(shelf);
    expect(calls.map((c) => c.cmd)).toEqual(['save_shelves']);
    expect(mem.has(SHELVES_KEY)).toBe(false);

    await bridge.loadShelves();
    expect(calls.map((c) => c.cmd)).toEqual(['save_shelves', 'load_shelves']);
  });

  it('saves through the backend, never localStorage', async () => {
    const { bridge, SHELVES_KEY } = await import('./bridge');
    await bridge.saveShelves(shelf);
    expect(calls).toEqual([{ cmd: 'save_shelves', args: { shelves: shelf } }]);
    expect(mem.has(SHELVES_KEY)).toBe(false);
  });
});

describe('world clocks persistence (native)', () => {
  it('stores Local with an empty zone so it follows the machine', async () => {
    const { bridge } = await import('./bridge');
    await bridge.saveWorldClocks([
      { label: 'Local', tz: 'Asia/Kolkata' },
      { label: 'Tokyo', tz: 'Asia/Tokyo' },
    ]);
    expect(calls[0].args).toEqual({
      clocks: [
        { label: 'Local', tz: '' },
        { label: 'Tokyo', tz: 'Asia/Tokyo' },
      ],
    });
  });

  it('migrates legacy clocks once and fills the local zone on read', async () => {
    const { bridge, CLOCKS_KEY } = await import('./bridge');
    mem.set(CLOCKS_KEY, JSON.stringify([{ label: 'London', tz: 'Europe/London' }]));
    expect(await bridge.loadWorldClocks()).toEqual([{ label: 'London', tz: 'Europe/London' }]);
    expect(mem.has(CLOCKS_KEY)).toBe(false);
    const zones = await bridge.loadWorldClocks();
    expect(zones[0].label).toBe('Local');
    expect(zones[0].tz).not.toBe('');
  });
});
