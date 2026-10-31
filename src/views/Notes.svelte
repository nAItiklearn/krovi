<script lang="ts">
  import { onMount } from 'svelte';
  import { now } from '../lib/stores';
  import { bridge } from '../lib/bridge';
  import { relativeTime } from '../lib/time';
  import Icon from '../components/Icon.svelte';
  import type { Note } from '../lib/types';

  let notes = $state<Note[]>([]);
  let selectedId = $state<number | null>(null);
  let nextId = 1000;
  let savedFlash = $state(false);
  let saveTimer: ReturnType<typeof setTimeout> | null = null;

  const selected = $derived(notes.find((n) => n.id === selectedId) ?? null);

  function create() {
    const n: Note = { id: nextId++, body: '', created_at: Date.now(), updated_at: Date.now() };
    notes = [n, ...notes];
    selectedId = n.id;
  }
  function remove(id: number) {
    notes = notes.filter((n) => n.id !== id);
    if (selectedId === id) selectedId = notes[0]?.id ?? null;
  }
  // Autosave: bump updated_at, float to top, flash "Saved".
  function onEdit(body: string) {
    if (!selected) return;
    const id = selected.id;
    const updated = { ...selected, body, updated_at: Date.now() };
    notes = [updated, ...notes.filter((n) => n.id !== id)];
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      savedFlash = true;
      setTimeout(() => (savedFlash = false), 900);
    }, 400);
  }

  onMount(async () => {
    notes = await bridge.loadNotes();
    selectedId = notes[0]?.id ?? null;
  });
</script>

<div class="layout">
  <aside class="list-col">
    <button class="btn accent full" onclick={create}><Icon name="plus" size={16} /> New note</button>
    <ul class="scroll">
      {#each notes as n (n.id)}
        <li>
          <button class="nrow" class:active={selectedId === n.id} onclick={() => (selectedId = n.id)}>
            <span class="nt">{n.body.split('\n')[0] || 'Untitled note'}</span>
            <span class="nw faint">{relativeTime(n.updated_at, $now)}</span>
          </button>
        </li>
      {:else}
        <li class="faint empty">No notes yet.</li>
      {/each}
    </ul>
  </aside>

  <section class="editor">
    {#if selected}
      <div class="ed-head">
        <span class="faint">Edited {relativeTime(selected.updated_at, $now)}</span>
        <span class="right">
          <span class="saved" class:show={savedFlash}><Icon name="check" size={13} /> Saved</span>
          <button class="btn sm" onclick={() => remove(selected.id)}><Icon name="trash" size={14} /> Delete</button>
        </span>
      </div>
      <textarea
        class="area"
        placeholder="Start typing… autosaves to local state"
        value={selected.body}
        oninput={(e) => onEdit((e.currentTarget as HTMLTextAreaElement).value)}></textarea>
      <div class="ed-foot faint">{selected.body.length} chars</div>
    {:else}
      <p class="faint center">Create a note to get started.</p>
    {/if}
  </section>
</div>

<style>
  .layout {
    display: grid;
    grid-template-columns: 230px 1fr;
    gap: 16px;
    height: calc(min(700px, 88vh) - 170px);
    min-height: 260px;
  }
  .list-col {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
  }
  .full {
    width: 100%;
  }
  .list-col ul {
    list-style: none;
    margin: 0;
    padding: 0;
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-height: 0;
  }
  .nrow {
    width: 100%;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
    border: 1px solid transparent;
    background: var(--surface-2);
    border-radius: 9px;
    padding: 9px 11px;
    text-align: left;
  }
  .nrow.active {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .nt {
    font-size: 13.5px;
    font-weight: 550;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }
  .nw {
    font-size: 11px;
  }
  .editor {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
  }
  .ed-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 12px;
  }
  .ed-head .right {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .saved {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--accent);
    opacity: 0;
    font-size: 12px;
  }
  .saved.show {
    opacity: 1;
  }
  .area {
    flex: 1;
    resize: none;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text);
    padding: 14px;
    font-size: 14px;
    line-height: 1.6;
    font-family: inherit;
    outline: none;
  }
  .area:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .ed-foot {
    font-size: 11.5px;
  }
  .center {
    margin: auto;
  }
  .empty {
    padding: 14px;
  }
  @media (max-width: 680px) {
    .layout {
      grid-template-columns: 1fr;
      height: auto;
    }
    .area {
      min-height: 220px;
    }
  }
</style>
