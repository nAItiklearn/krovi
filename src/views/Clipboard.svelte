<script lang="ts">
  import { flip } from 'svelte/animate';
  import Icon from '../components/Icon.svelte';
  import Segmented from '../components/Segmented.svelte';
  import State from '../components/State.svelte';
  import { clearClips, clips, copyClip, removeClip, togglePin } from '../lib/live';
  import { counts, query } from '../lib/clip';
  import { relativeTime } from '../lib/time';
  import { now, pushToast } from '../lib/stores';
  import { arrive, depart, spotlight } from '../lib/motion';
  import type { ClipEntry, ClipKind } from '../lib/types';
  import type { IconName } from '../lib/icons';

  type Filter = 'all' | ClipKind;
  let filter = $state<Filter>('all');
  let needle = $state('');
  let selectedId = $state<number | null>(null);
  let copiedId = $state<number | null>(null);
  let confirmClear = $state(false);

  const shown = $derived(query($clips, filter === 'all' ? null : filter, needle));
  const c = $derived(counts($clips));
  const selected = $derived($clips.find((e) => e.id === selectedId) ?? shown[0] ?? null);

  const options = $derived([
    { id: 'all' as Filter, label: 'All', badge: c.all },
    { id: 'Text' as Filter, label: 'Text', badge: c.Text },
    { id: 'Link' as Filter, label: 'Links', badge: c.Link },
    { id: 'Image' as Filter, label: 'Images', badge: c.Image },
  ]);

  const kindIcon = (k: ClipKind): IconName => (k === 'Link' ? 'link' : k === 'Image' ? 'image' : 'text');
  const isDataImage = (e: ClipEntry) => e.kind === 'Image' && e.content.startsWith('data:image/');

  let flashTimer: ReturnType<typeof setTimeout> | null = null;
  async function copy(e: ClipEntry) {
    selectedId = e.id;
    await copyClip(e.id);
    copiedId = e.id;
    pushToast('Copied to clipboard');
    if (flashTimer) clearTimeout(flashTimer);
    flashTimer = setTimeout(() => (copiedId = null), 1400);
  }

  function nextSelection(id: number) {
    const i = shown.findIndex((e) => e.id === id);
    return (shown[i + 1] ?? shown[i - 1])?.id ?? null;
  }
  async function remove(e: ClipEntry) {
    const next = nextSelection(e.id);
    await removeClip(e.id);
    if (selectedId === e.id) selectedId = next;
  }

  function onListKey(ev: KeyboardEvent) {
    const rows = Array.from((ev.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>('.rowbtn'));
    const i = rows.findIndex((r) => r === document.activeElement);
    if (ev.key === 'ArrowDown' || ev.key === 'ArrowUp') {
      ev.preventDefault();
      const n = ev.key === 'ArrowDown' ? Math.min(rows.length - 1, i + 1) : Math.max(0, i - 1);
      rows[n]?.focus();
    } else if ((ev.key === 'Delete' || ev.key === 'Backspace') && i >= 0) {
      ev.preventDefault();
      const e = shown[i];
      if (e) void remove(e);
    } else if (ev.key.toLowerCase() === 'p' && i >= 0 && !ev.ctrlKey && !ev.metaKey) {
      const e = shown[i];
      if (e) void togglePin(e.id);
    }
  }

  async function doClear() {
    if (!confirmClear) {
      confirmClear = true;
      setTimeout(() => (confirmClear = false), 2600);
      return;
    }
    confirmClear = false;
    await clearClips();
    pushToast('Cleared (pinned clips kept)');
  }

  const words = (s: string) => (s.trim() ? s.trim().split(/\s+/).length : 0);
  const lines = (s: string) => s.split('\n').length;
</script>

<div class="view">
  <div class="bar">
    <Segmented label="Filter by type" {options} bind:value={filter} />
    <label class="search">
      <Icon name="search" size={15} />
      <input placeholder="Search copies" bind:value={needle} aria-label="Search clipboard history" spellcheck="false" />
      {#if needle}
        <button class="x" aria-label="Clear search" onclick={() => (needle = '')}><Icon name="close" size={13} /></button>
      {/if}
    </label>
    <button class="btn danger" class:armed={confirmClear} onclick={doClear} disabled={!$clips.length} aria-live="polite">
      <Icon name="trash" size={14} />{confirmClear ? 'Sure? Click again' : 'Clear'}
    </button>
  </div>

  {#if !$clips.length}
    <div class="fill">
      <State
        icon="clipboard"
        title="Nothing copied yet"
        text="Copy something anywhere on your PC and it shows up here instantly. History stays on this device, and password-manager copies are never stored." />
    </div>
  {:else}
    <div class="split">
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <ul class="list scroll" aria-label="Clipboard history" onkeydown={onListKey}>
        {#each shown as e (e.id)}
          <li animate:flip={{ duration: 380 }} in:arrive out:depart>
            <div class="row" class:sel={selected?.id === e.id}>
              <button
                class="rowbtn"
                onfocus={() => (selectedId = e.id)}
                onclick={() => copy(e)}
                aria-label={`Copy ${e.kind.toLowerCase()}: ${e.content.slice(0, 60)}`}>
                <span class="ki" class:ok={copiedId === e.id}>
                  <Icon name={copiedId === e.id ? 'check' : kindIcon(e.kind)} size={14} />
                </span>
                <span class="ct ellipsis">{e.kind === 'Image' ? 'Image' : e.content}</span>
                <span class="cw faint">{relativeTime(e.created_at, $now)}</span>
              </button>
              <button
                class="mini"
                class:on={e.pinned}
                aria-pressed={e.pinned}
                aria-label={e.pinned ? 'Unpin' : 'Pin'}
                onclick={() => togglePin(e.id)}>
                <Icon name="pin" size={14} />
              </button>
            </div>
          </li>
        {:else}
          <li class="none"><State compact icon="search" title="No matches" text="Try a different search or filter." /></li>
        {/each}
      </ul>

      <aside class="detail card" use:spotlight aria-label="Preview">
        {#if selected}
          {#key selected.id}
            <div class="dh">
              <span class="tag">{selected.kind === 'Link' ? 'Link' : selected.kind} · {relativeTime(selected.created_at, $now)}</span>
              <span class="grow"></span>
              <button class="btn sm icon ghost" aria-label={selected.pinned ? 'Unpin' : 'Pin'} aria-pressed={selected.pinned} onclick={() => togglePin(selected.id)}>
                <Icon name="pin" size={14} />
              </button>
              <button class="btn sm icon ghost danger" aria-label="Delete" onclick={() => remove(selected)}>
                <Icon name="trash" size={14} />
              </button>
              <button class="btn sm primary" onclick={() => copy(selected)}>
                <Icon name={copiedId === selected.id ? 'check' : 'copy'} size={14} />
                {copiedId === selected.id ? 'Copied' : 'Copy'}
              </button>
            </div>
            <div class="db scroll">
              {#if isDataImage(selected)}
                <img class="img" src={selected.content} alt="Copied" />
              {:else if selected.kind === 'Image'}
                <div class="imgph"><Icon name="image" size={30} /><span class="faint small">Image on the clipboard</span></div>
              {:else}
                <p class="txt" class:link={selected.kind === 'Link'}>{selected.content}</p>
              {/if}
            </div>
            {#if selected.kind !== 'Image'}
              <div class="df faint">
                {selected.content.length} characters · {words(selected.content)} {words(selected.content) === 1 ? 'word' : 'words'}{lines(selected.content) > 1 ? ` · ${lines(selected.content)} lines` : ''}
              </div>
            {/if}
          {/key}
        {/if}
      </aside>
    </div>
  {/if}
</div>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 14px;
    flex: none;
  }
  .search {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 10px 0 12px;
    border-radius: var(--r-2);
    border: 1px solid var(--line);
    background: var(--surface-1);
    color: var(--text-3);
    transition:
      border-color var(--dur-1),
      background var(--dur-1);
  }
  .search:hover {
    border-color: var(--line-strong);
  }
  .search:focus-within {
    background: var(--surface-2);
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: none;
    color: var(--text);
    font-size: var(--t-sm);
    font-weight: 500;
    outline: none;
  }
  .search input::placeholder {
    color: var(--text-3);
  }
  .x {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: var(--surface-3);
    color: var(--text-2);
  }
  .armed {
    color: var(--danger);
    border-color: color-mix(in srgb, var(--danger) 50%, transparent);
  }
  .fill {
    flex: 1;
    display: grid;
    place-items: center;
  }
  .split {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1.12fr) minmax(0, 1fr);
    gap: 14px;
  }
  .list {
    min-height: 0;
    margin: 0 -6px 0 -6px;
    padding: 0 6px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .row {
    display: flex;
    align-items: center;
    border-radius: var(--r-3);
    border: 1px solid transparent;
    transition:
      background var(--dur-1) var(--ease-out),
      border-color var(--dur-1);
  }
  .row:hover {
    background: var(--surface-1);
  }
  .row.sel {
    background: var(--surface-2);
    border-color: var(--line);
  }
  .rowbtn {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 11px;
    height: 44px;
    padding: 0 6px 0 10px;
    text-align: left;
    border-radius: var(--r-3);
  }
  .ki {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 8px;
    flex: none;
    background: var(--surface-1);
    border: 1px solid var(--line);
    color: var(--text-2);
    transition:
      background var(--dur-2),
      color var(--dur-2),
      transform 0.5s var(--spring-bouncy);
  }
  .ki.ok {
    background: var(--invert-bg);
    color: var(--invert-fg);
    transform: scale(1.1);
  }
  .ct {
    flex: 1;
    font-size: var(--t-sm);
    font-weight: 500;
  }
  .cw {
    font-size: var(--t-xs);
    white-space: nowrap;
  }
  .mini {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    margin-right: 6px;
    border-radius: 9px;
    color: var(--text-3);
    opacity: 0;
    transition:
      opacity var(--dur-1),
      background var(--dur-1),
      color var(--dur-1);
  }
  .row:hover .mini,
  .row:focus-within .mini,
  .mini.on {
    opacity: 1;
  }
  .mini:hover {
    background: var(--surface-3);
    color: var(--text);
  }
  .mini.on {
    color: var(--accent);
  }
  .none {
    margin-top: 20px;
  }

  .detail {
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 12px 14px 12px 16px;
    gap: 10px;
  }
  .dh {
    display: flex;
    align-items: center;
    gap: 4px;
    flex: none;
  }
  .grow {
    flex: 1;
  }
  .db {
    flex: 1;
    min-height: 0;
  }
  .txt {
    font-size: var(--t-md);
    line-height: 1.55;
    white-space: pre-wrap;
    word-break: break-word;
    user-select: text;
    padding-right: 6px;
  }
  .txt.link {
    color: var(--text);
    text-decoration: underline;
    text-decoration-color: var(--line-strong);
    text-underline-offset: 3px;
    word-break: break-all;
  }
  .img {
    max-width: 100%;
    border-radius: var(--r-2);
    border: 1px solid var(--line);
  }
  .imgph {
    display: grid;
    place-items: center;
    gap: 8px;
    height: 100%;
    color: var(--text-3);
    border: 1px dashed var(--line-strong);
    border-radius: var(--r-3);
  }
  .small {
    font-size: var(--t-xs);
  }
  .df {
    flex: none;
    padding-top: 9px;
    border-top: 1px solid var(--line);
    font-size: var(--t-xs);
  }
</style>
