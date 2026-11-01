<script lang="ts">
  import { onMount } from 'svelte';
  import { flip } from 'svelte/animate';
  import Icon from '../components/Icon.svelte';
  import State from '../components/State.svelte';
  import { bridge, mode } from '../lib/bridge';
  import { hold } from '../lib/shell';
  import { now, pushToast, requestTransfer, shelfTick } from '../lib/stores';
  import { formatBytes, relativeTime } from '../lib/time';
  import { arrive, depart } from '../lib/motion';
  import type { ShelfItem, StoredShelf } from '../lib/types';
  import type { IconName } from '../lib/icons';

  // Shelves are references (name, size, path), saved locally. The shelf keeps what you parked
  // within reach; nothing is uploaded and the files themselves are never moved.
  const KEY = 'krovi.shelves';

  let shelves = $state<StoredShelf[]>([]);
  let activeIdx = $state(0);
  let newShelf = $state('');
  let status = $state<'loading' | 'ready' | 'error'>('loading');
  let over = $state(false);
  let dragId = $state<number | null>(null);
  let fileInput: HTMLInputElement | undefined = $state();
  let nextId = Date.now();

  const active = $derived(shelves[activeIdx] ?? null);
  const total = $derived((active?.items ?? []).reduce((a, i) => a + i.size_bytes, 0));

  // "Send to phone" hands the shelf to Quick Transfer, which needs real paths. Files added with
  // the browser picker only carry a name, so they cannot be sent (the demo sends by name).
  const demo = mode() === 'demo';
  const isRealPath = (p: string) => /^[A-Za-z]:[\\/]|^\\\\|^\//.test(p);
  const sendable = $derived((active?.items ?? []).filter((i) => demo || isRealPath(i.stored_path)));
  function sendToPhone() {
    if (sendable.length) requestTransfer(sendable.map((i) => i.stored_path));
  }

  function save() {
    try {
      localStorage.setItem(KEY, JSON.stringify(shelves));
    } catch {
      /* ignore */
    }
  }

  async function load() {
    status = 'loading';
    try {
      const raw = localStorage.getItem(KEY);
      const saved = raw ? (JSON.parse(raw) as StoredShelf[]) : null;
      shelves = saved && saved.length ? saved : await bridge.loadShelves();
      if (!shelves.length) shelves = [{ name: 'Desk', items: [] }];
      status = 'ready';
    } catch {
      status = 'error';
    }
  }

  function iconFor(name: string): IconName {
    const ext = name.split('.').pop()?.toLowerCase() ?? '';
    if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'heic', 'bmp'].includes(ext)) return 'image';
    if (['pdf', 'doc', 'docx', 'txt', 'md', 'rtf', 'xls', 'xlsx', 'csv'].includes(ext)) return 'fileText';
    return 'file';
  }

  function addItems(entries: { name: string; size: number; path: string }[]) {
    if (!active || !entries.length) return;
    const fresh: ShelfItem[] = entries.map((e) => ({
      id: nextId++,
      name: e.name,
      stored_path: e.path,
      size_bytes: e.size,
      added_at: Date.now(),
    }));
    shelves = shelves.map((s, i) => (i === activeIdx ? { ...s, items: [...fresh, ...s.items] } : s));
    save();
    pushToast(`${fresh.length} ${fresh.length === 1 ? 'file' : 'files'} added to ${active.name}`);
  }

  function fromFiles(list: FileList | File[]) {
    addItems(Array.from(list).map((f) => ({ name: f.name, size: f.size, path: f.name })));
  }

  function removeItem(id: number) {
    shelves = shelves.map((s, i) => (i === activeIdx ? { ...s, items: s.items.filter((it) => it.id !== id) } : s));
    save();
  }
  function addShelf() {
    const name = newShelf.trim();
    if (!name || shelves.some((s) => s.name.toLowerCase() === name.toLowerCase())) return;
    shelves = [...shelves, { name, items: [] }];
    newShelf = '';
    activeIdx = shelves.length - 1;
    save();
  }
  function removeShelf(i: number) {
    if (shelves.length <= 1) return;
    shelves = shelves.filter((_, idx) => idx !== i);
    activeIdx = Math.max(0, Math.min(activeIdx, shelves.length - 1));
    save();
  }

  // ---- native file dialog (the panel must not collapse while it is open) ----
  function openDialog() {
    void hold(true);
    const release = () => setTimeout(() => void hold(false), 400);
    window.addEventListener('focus', release, { once: true });
    fileInput?.click();
  }
  function onPicked(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    if (input.files) fromFiles(input.files);
    input.value = '';
    void hold(false);
  }

  // ---- browser drops ----
  const hasFiles = (e: DragEvent) => Array.from(e.dataTransfer?.types ?? []).includes('Files');
  function onDragOver(e: DragEvent) {
    if (!hasFiles(e)) return;
    e.preventDefault();
    over = true;
  }
  function onDrop(e: DragEvent) {
    over = false;
    if (!hasFiles(e)) return;
    e.preventDefault();
    if (e.dataTransfer?.files) fromFiles(e.dataTransfer.files);
  }

  // ---- reorder within a shelf ----
  function reorder(targetId: number) {
    if (dragId == null || !active || dragId === targetId) return;
    const items = [...active.items];
    const from = items.findIndex((i) => i.id === dragId);
    const to = items.findIndex((i) => i.id === targetId);
    if (from < 0 || to < 0) return;
    const [moved] = items.splice(from, 1);
    items.splice(to, 0, moved);
    shelves = shelves.map((s, i) => (i === activeIdx ? { ...s, items } : s));
    dragId = null;
    save();
  }

  onMount(() => {
    void load();
    // Inside Tauri the webview intercepts OS drops and reports paths instead of DOM events.
    let off: (() => void) | undefined;
    let dead = false;
    if ('__TAURI_INTERNALS__' in window) {
      void import('@tauri-apps/api/webview')
        .then(({ getCurrentWebview }) =>
          getCurrentWebview().onDragDropEvent((ev) => {
            const p = ev.payload;
            if (p.type === 'enter' || p.type === 'over') over = true;
            else if (p.type === 'leave') over = false;
            else if (p.type === 'drop') {
              over = false;
              addItems(p.paths.map((path) => ({ name: path.split(/[\\/]/).pop() ?? path, size: 0, path })));
            }
          }),
        )
        .then((u) => {
          if (dead) u();
          else off = u;
        })
        .catch(() => {});
    }
    return () => {
      dead = true;
      off?.();
    };
  });

  // files parked from elsewhere ("put this on the shelf") while this view is open
  let seenTick = $shelfTick;
  $effect(() => {
    const t = $shelfTick;
    if (t === seenTick) return;
    seenTick = t;
    try {
      const raw = localStorage.getItem(KEY);
      if (raw) shelves = JSON.parse(raw) as StoredShelf[];
    } catch {
      /* keep what is shown */
    }
  });
</script>

<div class="view">
  {#if status === 'loading'}
    <div class="skel" style="height:44px;margin-bottom:10px"></div>
    <div class="skel" style="height:44px;margin-bottom:10px"></div>
    <div class="skel" style="height:44px"></div>
  {:else if status === 'error'}
    <State tone="error" title={'Couldn’t open your shelves'} text="Your saved shelves could not be read." action={{ label: 'Try again', run: load }} />
  {:else}
    <div class="layout">
      <aside class="shelves" aria-label="Shelves">
        <p class="eyebrow">Shelves</p>
        <ul>
          {#each shelves as s, i (s.name)}
            <li>
              <button class="shelf" class:on={i === activeIdx} aria-current={i === activeIdx} onclick={() => (activeIdx = i)}>
                <Icon name={i === activeIdx ? 'shelf' : 'folder'} size={15} />
                <span class="nm ellipsis">{s.name}</span>
                <span class="ct faint">{s.items.length}</span>
              </button>
              {#if shelves.length > 1}
                <button class="x" aria-label={`Remove shelf ${s.name}`} onclick={() => removeShelf(i)}><Icon name="close" size={12} /></button>
              {/if}
            </li>
          {/each}
        </ul>
        <form class="newshelf" onsubmit={(e) => { e.preventDefault(); addShelf(); }}>
          <input class="input" placeholder="New shelf" bind:value={newShelf} aria-label="New shelf name" maxlength="24" />
          <button class="btn icon" type="submit" aria-label="Add shelf" disabled={!newShelf.trim()}><Icon name="plus" size={15} /></button>
        </form>
      </aside>

      <section
        class="files"
        class:over
        ondragover={onDragOver}
        ondragleave={() => (over = false)}
        ondrop={onDrop}
        aria-label={`Files on ${active?.name ?? 'shelf'}`}>
        <div class="fh">
          <div>
            <h2>{active?.name}</h2>
            <p class="faint">
              {active?.items.length ?? 0} {active?.items.length === 1 ? 'item' : 'items'}{total > 0 ? ` · ${formatBytes(total)}` : ''}
            </p>
          </div>
          <div class="acts">
            <button
              class="btn"
              onclick={sendToPhone}
              disabled={!sendable.length}
              title={sendable.length
                ? `Send ${sendable.length === 1 ? 'this file' : `these ${sendable.length} files`} to your phone over Wi-Fi`
                : 'Only files dragged in from Explorer can be sent: the picker does not share their location'}>
              <Icon name="smartphone" size={14} /> Send to phone
            </button>
            <button class="btn" onclick={openDialog}><Icon name="plus" size={14} /> Add files</button>
          </div>
          <input bind:this={fileInput} type="file" multiple hidden onchange={onPicked} />
        </div>

        {#if active && active.items.length}
          <ul class="rows scroll">
            {#each active.items as it (it.id)}
              <li
                class="frow"
                class:dragging={dragId === it.id}
                draggable="true"
                ondragstart={() => (dragId = it.id)}
                ondragend={() => (dragId = null)}
                ondragover={(e) => { if (dragId != null) e.preventDefault(); }}
                ondrop={() => reorder(it.id)}
                animate:flip={{ duration: 340 }}
                in:arrive
                out:depart>
                <span class="grip" title="Drag to reorder"><Icon name="grip" size={14} /></span>
                <span class="fi"><Icon name={iconFor(it.name)} size={15} /></span>
                <span class="fn ellipsis" title={it.stored_path}>{it.name}</span>
                <span class="fs faint">{it.size_bytes > 0 ? formatBytes(it.size_bytes) : '—'}</span>
                <span class="fw faint">{relativeTime(it.added_at, $now)}</span>
                <button class="x" aria-label={`Remove ${it.name}`} onclick={() => removeItem(it.id)}><Icon name="trash" size={13} /></button>
              </li>
            {/each}
          </ul>
        {:else}
          <div class="drop" class:over>
            <div class="glyph"><Icon name="upload" size={20} /></div>
            <h2>Drop files here</h2>
            <p>Park files you keep reaching for. Drag them in from Explorer, or choose them with Add files.</p>
          </div>
        {/if}

        {#if over}
          <div class="veil" aria-hidden="true"><Icon name="upload" size={22} /> Drop to add to {active?.name}</div>
        {/if}
      </section>
    </div>
  {/if}
</div>

<style>
  .layout {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 188px 1fr;
    gap: 18px;
  }
  .shelves {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-height: 0;
    padding-right: 16px;
    border-right: 1px solid var(--line);
  }
  .shelves ul {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: grid;
    align-content: start;
    gap: 2px;
  }
  .shelves li {
    position: relative;
    display: flex;
    align-items: center;
  }
  .shelf {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 9px;
    height: 36px;
    padding: 0 10px;
    border-radius: var(--r-2);
    color: var(--text-2);
    text-align: left;
    transition:
      background var(--dur-1),
      color var(--dur-1);
  }
  .shelf:hover {
    background: var(--surface-1);
    color: var(--text);
  }
  .shelf.on {
    background: var(--surface-2);
    color: var(--text);
  }
  .nm {
    flex: 1;
    font-size: var(--t-sm);
    font-weight: 600;
  }
  .ct {
    font-size: var(--t-xs);
    font-variant-numeric: tabular-nums;
  }
  .shelves li .x {
    position: absolute;
    right: 30px;
    opacity: 0;
  }
  .shelves li:hover .x,
  .shelves li:focus-within .x {
    opacity: 1;
  }
  .newshelf {
    display: flex;
    gap: 6px;
  }
  .files {
    position: relative;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-radius: var(--r-4);
  }
  .acts {
    display: flex;
    gap: 8px;
  }
  .fh {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 12px;
    flex: none;
  }
  .fh h2 {
    font-family: var(--font-display);
    font-size: var(--t-xl);
    font-weight: 650;
    letter-spacing: -0.02em;
  }
  .fh p {
    font-size: var(--t-sm);
    margin-top: 2px;
  }
  .rows {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-right: -6px;
    padding-right: 6px;
  }
  .frow {
    display: flex;
    align-items: center;
    gap: 11px;
    min-height: 44px;
    padding: 0 8px 0 6px;
    border-radius: var(--r-3);
    border: 1px solid var(--line);
    background: var(--surface-1);
    cursor: grab;
    transition:
      background var(--dur-1),
      border-color var(--dur-1),
      opacity var(--dur-1);
  }
  .frow:hover {
    background: var(--surface-2);
    border-color: var(--line-strong);
  }
  .frow.dragging {
    opacity: 0.4;
  }
  .grip {
    color: var(--text-3);
    display: flex;
  }
  .fi {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 9px;
    background: var(--surface-2);
    color: var(--text-2);
    flex: none;
  }
  .fn {
    flex: 1;
    font-size: var(--t-sm);
    font-weight: 560;
  }
  .fs,
  .fw {
    font-size: var(--t-xs);
    white-space: nowrap;
  }
  .fs {
    min-width: 54px;
    text-align: right;
  }
  .x {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 8px;
    color: var(--text-3);
    transition:
      color var(--dur-1),
      background var(--dur-1),
      opacity var(--dur-1);
  }
  .x:hover {
    color: var(--danger);
    background: var(--surface-3);
  }
  .drop {
    flex: 1;
    display: grid;
    place-items: center;
    align-content: center;
    gap: 6px;
    text-align: center;
    padding: 24px;
    border: 1.5px dashed var(--line-strong);
    border-radius: var(--r-4);
    color: var(--text-2);
    transition:
      border-color var(--dur-2),
      background var(--dur-2);
  }
  .drop.over {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .drop .glyph {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: var(--r-3);
    background: var(--surface-1);
    border: 1px solid var(--line);
    margin-bottom: 6px;
  }
  .drop h2 {
    font-size: var(--t-lg);
    font-weight: 600;
    color: var(--text);
  }
  .drop p {
    max-width: 34ch;
    font-size: var(--t-sm);
    line-height: 1.5;
  }
  .veil {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    border-radius: var(--r-4);
    border: 1.5px dashed var(--accent);
    background: color-mix(in srgb, var(--accent-soft) 70%, transparent);
    -webkit-backdrop-filter: blur(4px);
    backdrop-filter: blur(4px);
    font-weight: 650;
    pointer-events: none;
  }
</style>
