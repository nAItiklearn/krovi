<script lang="ts">
  import { onMount } from 'svelte';
  import { now } from '../lib/stores';
  import { bridge } from '../lib/bridge';
  import { formatBytes, relativeTime } from '../lib/time';
  import Icon from '../components/Icon.svelte';
  import type { ShelfItem, StoredShelf } from '../lib/types';

  let shelves = $state<StoredShelf[]>([]);
  let activeIdx = $state(0);
  let newShelf = $state('');
  let dragId = $state<number | null>(null);
  let nextId = 100;

  const active = $derived(shelves[activeIdx] ?? null);

  function addShelf() {
    const name = newShelf.trim();
    if (!name || shelves.some((s) => s.name === name)) return;
    shelves = [...shelves, { name, items: [] }];
    newShelf = '';
    activeIdx = shelves.length - 1;
  }
  function removeShelf(i: number) {
    if (shelves.length <= 1) return;
    shelves = shelves.filter((_, idx) => idx !== i);
    activeIdx = Math.max(0, Math.min(activeIdx, shelves.length - 1));
  }
  function addFile() {
    if (!active) return;
    const names = ['Draft.pdf', 'Shot.png', 'Notes.md', 'Export.zip', 'Logo.svg'];
    const item: ShelfItem = {
      id: nextId++,
      name: names[Math.floor(Math.random() * names.length)],
      stored_path: `blobs/demo-${nextId}`,
      size_bytes: Math.round((20 + Math.random() * 3000) * 1024),
      added_at: Date.now(),
    };
    shelves = shelves.map((s, i) => (i === activeIdx ? { ...s, items: [item, ...s.items] } : s));
  }
  function removeItem(id: number) {
    shelves = shelves.map((s, i) =>
      i === activeIdx ? { ...s, items: s.items.filter((it) => it.id !== id) } : s,
    );
  }

  // --- drag to reorder within the active shelf ---
  function onDragStart(id: number) {
    dragId = id;
  }
  function onDrop(targetId: number) {
    if (dragId == null || !active || dragId === targetId) return;
    const items = [...active.items];
    const from = items.findIndex((i) => i.id === dragId);
    const to = items.findIndex((i) => i.id === targetId);
    if (from < 0 || to < 0) return;
    const [moved] = items.splice(from, 1);
    items.splice(to, 0, moved);
    shelves = shelves.map((s, i) => (i === activeIdx ? { ...s, items } : s));
    dragId = null;
  }

  onMount(async () => {
    shelves = await bridge.loadShelves();
  });
</script>

<div class="layout">
  <aside class="shelves">
    <p class="eyebrow">Shelves</p>
    <ul>
      {#each shelves as s, i (s.name)}
        <li>
          <button class="shelf" class:active={i === activeIdx} onclick={() => (activeIdx = i)}>
            <Icon name="shelf" size={16} />
            <span class="nm">{s.name}</span>
            <span class="ct faint">{s.items.length}</span>
          </button>
          {#if shelves.length > 1}
            <button class="x" title="Remove shelf" onclick={() => removeShelf(i)}><Icon name="close" size={13} /></button>
          {/if}
        </li>
      {/each}
    </ul>
    <div class="newshelf">
      <input class="input" placeholder="New shelf" bind:value={newShelf} onkeydown={(e) => e.key === 'Enter' && addShelf()} />
      <button class="btn icon" onclick={addShelf} title="Add shelf"><Icon name="plus" size={16} /></button>
    </div>
  </aside>

  <section class="files">
    <div class="files-head">
      <div>
        <strong>{active?.name}</strong>
        <span class="faint"> · {active?.items.length ?? 0} items · {formatBytes((active?.items ?? []).reduce((a, i) => a + i.size_bytes, 0))}</span>
      </div>
      <button class="btn sm" onclick={addFile}><Icon name="plus" size={15} /> Add file</button>
    </div>

    <ul class="rows scroll">
      {#each active?.items ?? [] as it (it.id)}
        <li
          class="row"
          class:dragging={dragId === it.id}
          draggable="true"
          ondragstart={() => onDragStart(it.id)}
          ondragover={(e) => e.preventDefault()}
          ondrop={() => onDrop(it.id)}>
          <span class="grip" title="Drag to reorder"><Icon name="grip" size={16} /></span>
          <span class="ficon"><Icon name="notes" size={16} /></span>
          <span class="fname">{it.name}</span>
          <span class="fsize faint">{formatBytes(it.size_bytes)}</span>
          <span class="fwhen faint">{relativeTime(it.added_at, $now)}</span>
          <button class="x" title="Remove" onclick={() => removeItem(it.id)}><Icon name="trash" size={14} /></button>
        </li>
      {:else}
        <li class="empty faint">This shelf is empty. Add a file or drop one in.</li>
      {/each}
    </ul>

    <div class="dropzone">
      <Icon name="shelf" size={18} />
      <span>Drop files here — drag out to use them anywhere <span class="faint">(OS drag-and-drop in the Tauri shell)</span></span>
    </div>
  </section>
</div>

<style>
  .layout {
    display: grid;
    grid-template-columns: 210px 1fr;
    gap: 16px;
    height: calc(min(700px, 88vh) - 170px);
    min-height: 260px;
  }
  .shelves {
    border-right: 1px solid var(--border);
    padding-right: 14px;
    display: flex;
    flex-direction: column;
  }
  .shelves ul {
    list-style: none;
    margin: 0;
    padding: 0;
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .shelves li {
    display: flex;
    align-items: center;
  }
  .shelf {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 9px;
    border: none;
    background: transparent;
    color: var(--text-dim);
    padding: 9px 10px;
    border-radius: 9px;
    text-align: left;
  }
  .shelf.active {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .shelf .nm {
    flex: 1;
    font-size: 13.5px;
    font-weight: 550;
  }
  .newshelf {
    display: flex;
    gap: 6px;
    margin-top: 10px;
  }
  .files {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .files-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 12px;
    font-size: 14px;
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 9px 10px;
    border: 1px solid var(--border);
    border-radius: 9px;
    background: var(--surface-2);
    cursor: grab;
  }
  .row.dragging {
    opacity: 0.5;
    border-color: var(--accent);
  }
  .grip {
    color: var(--text-faint);
    display: flex;
  }
  .ficon {
    color: var(--accent);
    display: flex;
  }
  .fname {
    flex: 1;
    font-size: 13.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fsize,
  .fwhen {
    font-size: 12px;
    white-space: nowrap;
  }
  .x {
    border: none;
    background: transparent;
    color: var(--text-faint);
    padding: 5px;
    border-radius: 7px;
  }
  .x:hover {
    color: var(--danger);
    background: var(--surface-3);
  }
  .dropzone {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 12px;
    padding: 13px;
    border: 1px dashed var(--border);
    border-radius: var(--radius-sm);
    color: var(--text-dim);
    font-size: 12.5px;
  }
  .empty {
    padding: 24px;
    text-align: center;
  }
  @media (max-width: 680px) {
    .layout {
      grid-template-columns: 1fr;
      height: auto;
    }
    .shelves {
      border-right: none;
      border-bottom: 1px solid var(--border);
      padding-right: 0;
      padding-bottom: 12px;
    }
  }
</style>
