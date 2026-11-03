<script lang="ts">
  import { onDestroy, onMount, untrack } from 'svelte';
  import Icon from '../../components/Icon.svelte';
  import { pushToast } from '../../lib/stores';
  import { formatBytes, relativeTime } from '../../lib/time';
  import { FOLDER_IDS, FOLDER_LABEL, type FolderId } from '../../lib/organizer';
  import {
    canOpen,
    describeStop,
    openHit,
    parentDir,
    parseQuery,
    skipNotes,
    startSearch,
    type Hit,
    type SearchHandle,
    type Summary,
  } from '../../lib/filesearch';
  import type { ExtrasIntent } from '../../lib/extrasNav';

  // File search: looks at the NAMES of files and folders inside your Downloads, Documents, Pictures,
  // Desktop, Videos and Music folders. It reads nothing inside a file, keeps no index and does
  // nothing in the background: it searches when you press Search, shows results as they are found,
  // and can be stopped at any moment. Links, hidden and system items are skipped.

  let {
    demo,
    intent = null,
    onconsumed,
  }: { demo: boolean; intent?: ExtrasIntent | null; onconsumed?: () => void } = $props();

  let q = $state('');
  let picked = $state<FolderId[]>([]);
  let hits = $state<Hit[]>([]);
  let summary = $state<Summary | null>(null);
  let running = $state(false);
  let error = $state('');
  let handle: SearchHandle | null = null;
  let inputEl: HTMLInputElement | undefined = $state();

  const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));
  const queryError = $derived(q.trim() ? parseQuery(q).error : null);
  // "Type at least 2 characters" would nag after the first keystroke; every other problem is shown.
  const liveError = $derived(queryError && !queryError.startsWith('Type at least') ? queryError : '');
  const stopNote = $derived(summary ? describeStop(summary) : '');
  const skipped = $derived(summary ? skipNotes(summary) : []);

  function toggleFolder(id: FolderId) {
    picked = picked.includes(id) ? picked.filter((x) => x !== id) : [...picked, id];
  }

  async function search() {
    if (running) return;
    const parsed = parseQuery(q);
    if (parsed.error) {
      error = parsed.error;
      return;
    }
    error = '';
    hits = [];
    summary = null;
    running = true;
    try {
      // startSearch only passes on batches of THIS search, in the order they were found
      const mine = await startSearch(q, picked, (batch) => {
        hits = [...hits, ...batch];
      });
      handle = mine;
      summary = await mine.done;
    } catch (e) {
      error = msg(e);
    } finally {
      running = false;
    }
  }

  async function stop() {
    try {
      await handle?.cancel();
    } catch (e) {
      error = msg(e);
    }
  }

  async function open(hit: Hit, reveal: boolean) {
    if (!handle) return;
    try {
      pushToast(await openHit(handle.id, hit, reveal), demo ? 3200 : 2000);
    } catch (e) {
      pushToast(msg(e), 3600, 'error');
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      void search();
    }
  }

  // A request from the palette ("Search files for ...").
  $effect(() => {
    const i = intent;
    if (!i?.query) return;
    untrack(() => {
      q = i.query ?? '';
      onconsumed?.();
      void search();
    });
  });

  onMount(() => {
    inputEl?.focus();
  });

  onDestroy(() => {
    // leaving the screen ends a search that is still walking folders
    void handle?.cancel().catch(() => {});
  });
</script>

<div class="tab">
  <div class="bar">
    <div class="search">
      <Icon name="search" size={15} />
      <input
        bind:this={inputEl}
        bind:value={q}
        class="bare"
        type="text"
        placeholder="Part of a file or folder name"
        aria-label="Search file and folder names"
        maxlength="100"
        autocomplete="off"
        spellcheck="false"
        onkeydown={onKey} />
    </div>
    {#if running}
      <button class="btn" type="button" onclick={stop}><Icon name="square" size={13} /> Stop</button>
    {:else}
      <button class="btn primary" type="button" onclick={search} disabled={!q.trim() || !!queryError}><Icon name="search" size={14} /> Search</button>
    {/if}
  </div>

  <div class="where" role="group" aria-label="Folders to search">
    <span class="faint">In:</span>
    <button class="chip" class:on={picked.length === 0} type="button" aria-pressed={picked.length === 0} onclick={() => (picked = [])} disabled={running}>All</button>
    {#each FOLDER_IDS as id (id)}
      <button class="chip" class:on={picked.includes(id)} type="button" aria-pressed={picked.includes(id)} onclick={() => toggleFolder(id)} disabled={running}>
        {FOLDER_LABEL[id]}
      </button>
    {/each}
  </div>

  {#if error || liveError}
    <p class="error" role="alert"><Icon name="alert" size={14} /> {error || liveError}</p>
  {/if}

  <div class="body scroll" aria-live="polite">
    {#if hits.length > 0}
      <ul class="results">
        {#each hits as h (h.id)}
          <li class="hit">
            <span class="ico" aria-hidden="true"><Icon name={h.isDir ? 'folder' : 'fileText'} size={16} /></span>
            <div class="tx">
              <span class="nm" title={h.name}>{h.name}</span>
              <span class="where-path ellipsis" title={h.path}>{parentDir(h.path)}</span>
            </div>
            <span class="meta faint">
              {#if h.size !== null}{formatBytes(h.size)}{/if}
              {#if h.modifiedMs !== null}<span>{relativeTime(h.modifiedMs, Date.now())}</span>{/if}
            </span>
            <div class="acts">
              {#if canOpen(h)}
                <button class="btn sm" type="button" onclick={() => open(h, false)}>Open</button>
              {/if}
              <button class="btn sm ghost" type="button" onclick={() => open(h, true)}>
                <Icon name="folder" size={12} /> Show in folder
              </button>
            </div>
          </li>
        {/each}
      </ul>
    {:else if running}
      <p class="faint status"><Icon name="loader" size={14} /> Looking through file names...</p>
    {:else if summary}
      <p class="faint status">No file or folder name matches &ldquo;{q.trim()}&rdquo;.</p>
    {/if}

    {#if summary}
      <div class="foot">
        <p>
          <b>{summary.found}</b>
          {summary.found === 1 ? 'match' : 'matches'}, {summary.scanned.toLocaleString('en-US')} names looked at.
          {#if summary.demo}<span class="tag accent">DEMO</span>{/if}
        </p>
        {#if stopNote}<p>{stopNote}</p>{/if}
        {#each skipped as note (note)}<p class="faint">{note}</p>{/each}
      </div>
    {/if}
  </div>

  <p class="note">
    <Icon name="shield" size={13} />
    <span>
      Searches file and folder <b>names</b> only. Krovi reads nothing inside your files, keeps no index and searches nothing until you
      press Search. It looks at most six folders deep and never follows links.
    </span>
  </p>
</div>

<style>
  .tab {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .bar {
    flex: none;
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .search {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 12px;
    border-radius: var(--r-2);
    border: 1px solid var(--line);
    background: var(--surface-1);
    color: var(--text-2);
  }
  .search:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .bare {
    flex: 1;
    min-width: 0;
    height: 100%;
    border: 0;
    background: transparent;
    color: var(--text);
    font-size: var(--t-sm);
    font-weight: 500;
    outline: none;
  }
  .where {
    flex: none;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    font-size: var(--t-xs);
  }
  .error {
    flex: none;
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 0;
    color: var(--danger);
    font-size: var(--t-sm);
    overflow-wrap: anywhere;
  }
  .error :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    align-content: start;
    gap: 10px;
    padding-right: 4px;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 6px 0;
    font-size: var(--t-sm);
  }
  .results {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 2px;
  }
  .hit {
    display: grid;
    grid-template-columns: 30px minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border-radius: var(--r-2);
    border-top: 1px solid var(--line);
  }
  .hit:hover {
    background: var(--surface-1);
  }
  .ico {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 9px;
    background: var(--surface-1);
    border: 1px solid var(--line);
    color: var(--text-2);
  }
  .tx {
    display: grid;
    gap: 1px;
    min-width: 0;
  }
  .nm {
    font-size: var(--t-sm);
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .where-path {
    font-size: var(--t-micro);
    color: var(--text-3);
  }
  .meta {
    display: grid;
    justify-items: end;
    font-size: var(--t-micro);
    font-variant-numeric: tabular-nums;
  }
  .acts {
    display: flex;
    gap: 4px;
  }
  .foot {
    display: grid;
    gap: 2px;
    padding-top: 6px;
    font-size: var(--t-xs);
    color: var(--text-2);
  }
  .foot p {
    margin: 0;
  }
  .note {
    flex: none;
    display: flex;
    gap: 7px;
    align-items: flex-start;
    margin: 0;
    font-size: var(--t-xs);
    color: var(--text-2);
    line-height: 1.45;
  }
  .note :global(svg) {
    flex: none;
    margin-top: 2px;
  }
</style>
