// The Extras features inside the Tauri shell: the bridge is mocked, so this proves the command
// names, the arguments (ids and rules only, never a path to move or open), the event flow, and that
// nothing falls back to demo data.
import { beforeEach, describe, expect, it, vi } from 'vitest';

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

async function fresh() {
  vi.resetModules();
  const [extras, organizer, search] = await Promise.all([import('./extras'), import('./organizer'), import('./filesearch')]);
  return { extras, organizer, search };
}

beforeEach(() => {
  calls.length = 0;
  handlers.clear();
  respond = () => undefined;
});

describe('tauri: launcher, snippets, workflows', () => {
  it('asks the backend for the Start-menu names and never falls back to the demo apps', async () => {
    const { extras } = await fresh();
    respond = () => ['Calc', 'Notes'];
    expect(await extras.launcherApps()).toEqual(['Calc', 'Notes']);
    expect(calls).toEqual([{ cmd: 'extras_apps', args: undefined }]);
  });

  it('lists, saves, removes and uses snippets through the backend', async () => {
    const { extras } = await fresh();
    respond = () => [];
    expect(await extras.snippets.list()).toEqual([]);
    const input = { name: 'Hi', trigger: ';hi', body: 'Hello {date}' };
    await extras.snippets.save(input);
    await extras.snippets.remove('sn-hi');
    respond = () => ({ chars: 16, notes: [] });
    expect(await extras.snippets.use('sn-hi')).toEqual({ chars: 16, notes: [] });
    expect(calls.map((c) => c.cmd)).toEqual(['snippets_list', 'snippets_save', 'snippets_remove', 'snippets_use']);
    expect(calls[1].args).toEqual({ snippet: input });
    expect(calls[2].args).toEqual({ id: 'sn-hi' });
    expect(calls[3].args).toEqual({ id: 'sn-hi' }); // the id, never the text
  });

  it('shows the backend refusal sentence', async () => {
    const { extras } = await fresh();
    respond = () => new Error('A snippet with that name already exists.');
    await expect(extras.snippets.save({ name: 'x', trigger: '', body: 'x' })).rejects.toThrow('already exists');
  });

  it('lists, saves and removes workflows through the backend', async () => {
    const { extras } = await fresh();
    respond = () => [];
    await extras.workflows.list();
    await extras.workflows.save({ name: 'Focus', lines: ['open spotify'] });
    await extras.workflows.remove('wf-focus');
    expect(calls.map((c) => c.cmd)).toEqual(['workflows_list', 'workflows_save', 'workflows_remove']);
    expect(calls[1].args).toEqual({ workflow: { name: 'Focus', lines: ['open spotify'] } });
  });
});

describe('tauri: organizer', () => {
  const rule = { enabled: true, folder: 'downloads', extensions: ['*.PDF'], nameContains: '', olderThanDays: 7, destFolder: 'documents', destSubfolder: ' PDFs ' } as const;

  it('sends the rule normalised and nothing else', async () => {
    const { organizer } = await fresh();
    respond = () => [];
    await organizer.saveRule({ ...rule, extensions: [...rule.extensions] });
    expect(calls[0]).toEqual({
      cmd: 'organizer_save_rule',
      args: {
        rule: { enabled: true, folder: 'downloads', extensions: ['pdf'], nameContains: '', olderThanDays: 7, destFolder: 'documents', destSubfolder: 'PDFs' },
      },
    });
  });

  it('previews with rule ids only, and runs with the preview id only: never a path', async () => {
    const { organizer } = await fresh();
    respond = () => ({ previewId: 4, moves: [{ id: 'm1', from: 'C:\\Users\\me\\Downloads\\a.pdf' }], notes: [], truncated: false, createdMs: 1, rulesChecked: 1 });
    const p = await organizer.preview(['rule-1']);
    expect(p.demo).toBeUndefined();
    respond = () => ({ moved: undefined, failed: undefined, alreadyGone: 0 });
    const r = await organizer.run(p.previewId);
    expect(r).toMatchObject({ moved: [], failed: [] }); // missing lists are normalised
    expect(r.demo).toBeUndefined();
    expect(calls).toEqual([
      { cmd: 'organizer_preview', args: { ruleIds: ['rule-1'] } },
      { cmd: 'organizer_run', args: { previewId: 4 } },
    ]);
  });

  it('asks for and performs undo through the backend', async () => {
    const { organizer } = await fresh();
    respond = () => ({ runId: 4, atMs: 1, count: 2 });
    expect(await organizer.undoInfo()).toEqual({ runId: 4, atMs: 1, count: 2 });
    respond = () => ({ restored: 2, failed: undefined });
    expect(await organizer.undo()).toMatchObject({ restored: 2, failed: [] });
    expect(calls.map((c) => c.cmd)).toEqual(['organizer_undo_info', 'organizer_undo']);
  });

  it('reads folders and rules from the backend, never from demo data', async () => {
    const { organizer } = await fresh();
    respond = () => [];
    expect(await organizer.rules()).toEqual([]);
    expect(await organizer.folders()).toEqual([]);
    await organizer.removeRule('rule-1');
    expect(calls.map((c) => c.cmd)).toEqual(['organizer_rules', 'organizer_folders', 'organizer_remove_rule']);
  });

  it('surfaces the backend refusal text', async () => {
    const { organizer } = await fresh();
    respond = () => new Error('This preview is out of date. Preview again before running.');
    await expect(organizer.run(1)).rejects.toThrow('out of date');
  });
});

describe('tauri: file-name search', () => {
  const hit = (id: number) => ({ id, name: `a${id}.txt`, path: `C:\\Users\\me\\Documents\\a${id}.txt`, folder: 'documents' as const, isDir: false, size: 1, modifiedMs: 1 });
  const summary = { stop: 'finished', scanned: 3, found: 2, skippedLinks: 0, skippedHidden: 0, skippedDeep: 0, unreadable: 0 };

  it('listens first, starts with the query and folders, streams hits and resolves on done', async () => {
    const { search } = await fresh();
    respond = () => 7;
    const got: number[][] = [];
    const h = await search.startSearch('  invoice ', ['downloads'], (hits) => got.push(hits.map((x) => x.id)));
    expect(h.id).toBe(7);
    expect(calls).toEqual([{ cmd: 'filesearch_start', args: { query: '  invoice ', folders: ['downloads'] } }]);
    emit('filesearch-hits', { searchId: 7, hits: [hit(1)] });
    emit('filesearch-hits', { searchId: 6, hits: [hit(99)] }); // an older search: ignored
    emit('filesearch-hits', { searchId: 7, hits: [hit(2)] });
    emit('filesearch-done', { searchId: 7, ok: true, error: null, summary });
    expect(await h.done).toEqual(summary);
    expect(got).toEqual([[1], [2]]);
    expect(handlers.size).toBe(0); // listeners are removed
  });

  it('does not lose events that arrive before the command returns the search id', async () => {
    const { search } = await fresh();
    respond = () => {
      // the backend thread is quicker than the command's reply
      emit('filesearch-hits', { searchId: 3, hits: [hit(1)] });
      emit('filesearch-done', { searchId: 3, ok: true, error: null, summary });
      return 3;
    };
    const got: number[] = [];
    const h = await search.startSearch('invoice', [], (hits) => got.push(...hits.map((x) => x.id)));
    expect(await h.done).toEqual(summary);
    expect(got).toEqual([1]);
  });

  it('rejects with the backend message when the search fails, and cleans up', async () => {
    const { search } = await fresh();
    respond = () => 5;
    const h = await search.startSearch('invoice', [], () => {});
    h.done.catch(() => {});
    emit('filesearch-done', { searchId: 5, ok: false, error: 'The search stopped unexpectedly. Try again.', summary: null });
    await expect(h.done).rejects.toThrow('stopped unexpectedly');
    expect(handlers.size).toBe(0);
  });

  it('cleans up when the command itself is refused', async () => {
    const { search } = await fresh();
    respond = () => new Error('could not start the search: out of threads');
    await expect(search.startSearch('invoice', [], () => {})).rejects.toThrow('could not start');
    expect(handlers.size).toBe(0);
  });

  it('refuses a bad query without calling the backend', async () => {
    const { search } = await fresh();
    await expect(search.startSearch('a', [], () => {})).rejects.toThrow('at least');
    expect(calls).toEqual([]);
  });

  it('cancels with filesearch_cancel', async () => {
    const { search } = await fresh();
    respond = () => 2;
    const h = await search.startSearch('invoice', [], () => {});
    h.done.catch(() => {});
    await h.cancel();
    expect(calls.map((c) => c.cmd)).toEqual(['filesearch_start', 'filesearch_cancel']);
  });

  it('opens a result by search id and hit id, never by path', async () => {
    const { search } = await fresh();
    respond = () => 'Showing a1.txt in File Explorer.';
    const msg = await search.openHit(7, hit(1), true);
    expect(msg).toContain('File Explorer');
    expect(calls).toEqual([{ cmd: 'filesearch_open', args: { searchId: 7, hitId: 1, reveal: true } }]);
    expect(Object.keys(calls[0].args!).sort()).toEqual(['hitId', 'reveal', 'searchId']);
  });
});
