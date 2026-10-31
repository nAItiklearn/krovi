<script lang="ts">
  import { onMount } from 'svelte';
  import { now } from '../lib/stores';
  import { bridge } from '../lib/bridge';
  import { query, counts } from '../lib/clip';
  import { relativeTime } from '../lib/time';
  import Icon from '../components/Icon.svelte';
  import type { ClipEntry, ClipKind } from '../lib/types';

  let entries = $state<ClipEntry[]>([]);
  let kind = $state<ClipKind | null>(null);
  let needle = $state('');
  let selectedId = $state<number | null>(null);
  let copiedId = $state<number | null>(null);

  const shown = $derived(query(entries, kind, needle));
  const c = $derived(counts(entries));
  const selected = $derived(entries.find((e) => e.id === selectedId) ?? shown[0] ?? null);

  const tabs: { id: ClipKind | null; label: string; n: () => number }[] = [
    { id: null, label: 'All', n: () => c.all },
    { id: 'Text', label: 'Text', n: () => c.Text },
    { id: 'Link', label: 'Links', n: () => c.Link },
    { id: 'Image', label: 'Images', n: () => c.Image },
  ];

  function togglePin(e: ClipEntry) {
    e.pinned = !e.pinned;
    entries = [...entries];
  }

  async function copy(e: ClipEntry) {
    try {
      await navigator.clipboard?.writeText(e.content);
    } catch {
      /* demo: clipboard API may be blocked; the visual feedback still fires */
    }
    copiedId = e.id;
    setTimeout(() => (copiedId === e.id ? (copiedId = null) : null), 1200);
  }

  onMount(async () => {
    entries = await bridge.loadClipboard();
  });
</script>

<div class="toolbar">
  <div class="seg" aria-label="Filter by kind">
    {#each tabs as t (t.label)}
      <button aria-pressed={kind === t.id} onclick={() => (kind = t.id)}>
        {t.label} <span class="faint">{t.n()}</span>
      </button>
    {/each}
  </div>
  <label class="search">
    <Icon name="search" size={16} />
    <input class="bare" placeholder="Search copies…" bind:value={needle} aria-label="Search clipboard" />
  </label>
</div>

<div class="split">
  <ul class="list scroll" aria-label="Clipboard entries">
    {#each shown as e (e.id)}
      <li class:active={selected?.id === e.id}>
        <button class="row" onclick={() => (selectedId = e.id)}>
          <span class="kind kind-{e.kind.toLowerCase()}">{e.kind[0]}</span>
          <span class="content">{e.content}</span>
          <span class="meta faint">{relativeTime(e.created_at, $now)}</span>
        </button>
        <button
          class="pinbtn"
          class:on={e.pinned}
          title={e.pinned ? 'Unpin' : 'Pin'}
          aria-pressed={e.pinned}
          onclick={() => togglePin(e)}>
          <Icon name="pin" size={15} />
        </button>
      </li>
    {:else}
      <li class="empty faint">Nothing matches that filter.</li>
    {/each}
  </ul>

  <div class="detail">
    {#if selected}
      <div class="detail-head">
        <span class="tag">{selected.kind} · {relativeTime(selected.created_at, $now)}</span>
        <button class="btn sm" onclick={() => copy(selected)}>
          <Icon name={copiedId === selected.id ? 'check' : 'copy'} size={15} />
          {copiedId === selected.id ? 'Copied' : 'Copy'}
        </button>
      </div>
      {#if selected.kind === 'Image'}
        <div class="imgbox">
          <Icon name="weather" size={34} />
          <span class="faint mono">{selected.content}</span>
        </div>
      {:else}
        <p class="body" class:link={selected.kind === 'Link'}>{selected.content}</p>
      {/if}
      <div class="detail-foot faint">
        {selected.content.length} chars · {selected.content.trim().split(/\s+/).length} words
      </div>
    {:else}
      <p class="faint center">Select an entry.</p>
    {/if}
  </div>
</div>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 14px;
    flex-wrap: wrap;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 7px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 9px;
    padding: 7px 11px;
    color: var(--text-dim);
    flex: 1;
    min-width: 170px;
  }
  .bare {
    border: none;
    background: transparent;
    color: var(--text);
    font-size: 13px;
    outline: none;
    width: 100%;
    font-family: inherit;
  }
  .split {
    display: grid;
    grid-template-columns: minmax(0, 1.1fr) minmax(0, 1fr);
    gap: 14px;
    height: calc(min(700px, 88vh) - 230px);
    min-height: 240px;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .list li {
    display: flex;
    align-items: stretch;
    border-radius: 9px;
    border: 1px solid transparent;
  }
  .list li.active {
    background: var(--accent-soft);
    border-color: var(--accent);
  }
  .row {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 10px;
    background: transparent;
    border: none;
    padding: 9px 10px;
    text-align: left;
    color: var(--text);
    min-width: 0;
  }
  .kind {
    width: 22px;
    height: 22px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 6px;
    font-size: 11px;
    font-weight: 700;
    background: var(--surface-3);
    color: var(--text-dim);
  }
  .kind-link {
    color: var(--accent);
    background: var(--accent-soft);
  }
  .kind-image {
    color: var(--warn);
  }
  .content {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
  }
  .meta {
    font-size: 11px;
    white-space: nowrap;
  }
  .pinbtn {
    border: none;
    background: transparent;
    color: var(--text-faint);
    padding: 0 10px;
    border-radius: 9px;
  }
  .pinbtn.on {
    color: var(--accent);
  }
  .pinbtn:hover {
    color: var(--text);
  }
  .detail {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .detail-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .body {
    margin: 0;
    font-size: 14px;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
    flex: 1;
    overflow: auto;
  }
  .body.link {
    color: var(--accent);
    font-family: var(--mono);
    font-size: 13px;
  }
  .imgbox {
    flex: 1;
    display: grid;
    place-items: center;
    gap: 10px;
    border: 1px dashed var(--border);
    border-radius: 9px;
    color: var(--text-faint);
  }
  .detail-foot {
    font-size: 11.5px;
    border-top: 1px solid var(--border);
    padding-top: 9px;
  }
  .center {
    margin: auto;
  }
  .empty {
    padding: 20px;
    text-align: center;
  }
  @media (max-width: 680px) {
    .split {
      grid-template-columns: 1fr;
      height: auto;
    }
  }
</style>
