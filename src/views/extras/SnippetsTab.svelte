<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Icon from '../../components/Icon.svelte';
  import State from '../../components/State.svelte';
  import { segments } from '../../components/search';
  import { pushToast } from '../../lib/stores';
  import {
    LIMITS,
    PLACEHOLDERS,
    rankSnippets,
    snippetPreview,
    snippets,
    validateSnippet,
    type Snippet,
    type SnippetInput,
  } from '../../lib/extras';

  // Snippets: saved text you copy ON PURPOSE. Pick one and press Enter (or Copy): {date}, {time} and
  // {clipboard} are filled in and the result is copied to the clipboard. Krovi has no keyboard hook,
  // never watches what you type and never expands anything by itself. A "keyword" only helps you
  // find a snippet in the command palette or here.

  let { demo }: { demo: boolean } = $props();

  let list = $state<Snippet[]>([]);
  let loaded = $state(false);
  let loadError = $state('');
  let q = $state('');
  let sel = $state(0);
  let editing = $state<SnippetInput | null>(null);
  let saveError = $state('');
  let busy = $state(false);
  let confirmingId = $state<string | null>(null);
  let copiedId = $state<string | null>(null);
  let searchEl: HTMLInputElement | undefined = $state();
  let nameEl: HTMLInputElement | undefined = $state();
  let bodyEl: HTMLTextAreaElement | undefined = $state();

  const hits = $derived(rankSnippets(q, list));
  const why = $derived(editing ? validateSnippet(editing, list) : null);
  // Only mention a problem once there is something to be wrong about (empty fields are obvious).
  const hint = $derived(editing && editing.name.trim() && editing.body.trim() ? why : null);
  const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

  $effect(() => {
    if (sel >= hits.length) sel = Math.max(0, hits.length - 1);
  });

  async function load() {
    try {
      list = await snippets.list();
      loadError = '';
    } catch (e) {
      loadError = msg(e);
    } finally {
      loaded = true;
    }
  }

  async function copy(s: Snippet) {
    if (busy) return;
    busy = true;
    try {
      const used = await snippets.use(s.id);
      copiedId = s.id;
      setTimeout(() => {
        if (copiedId === s.id) copiedId = null;
      }, 1600);
      const extra = used.notes.length ? ` ${used.notes.join(' ')}` : '';
      pushToast(`Copied "${s.name}" (${used.chars.toLocaleString('en-US')} characters).${extra}`, used.notes.length ? 4200 : 2200);
    } catch (e) {
      pushToast(msg(e), 3200, 'error');
    } finally {
      busy = false;
    }
  }

  async function startEdit(s: Snippet | null) {
    saveError = '';
    confirmingId = null;
    editing = s ? { id: s.id, name: s.name, trigger: s.trigger, body: s.body } : { name: '', trigger: '', body: '' };
    await tick();
    nameEl?.focus();
  }

  async function closeEdit() {
    editing = null;
    saveError = '';
    await tick();
    searchEl?.focus();
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (!editing || busy || why) return;
    busy = true;
    saveError = '';
    try {
      list = await snippets.save(editing);
      pushToast('Snippet saved', 1600);
      await closeEdit();
    } catch (err) {
      saveError = msg(err);
    } finally {
      busy = false;
    }
  }

  async function remove(s: Snippet) {
    try {
      list = await snippets.remove(s.id);
      confirmingId = null;
      pushToast(`Deleted "${s.name}"`, 1600);
    } catch (e) {
      pushToast(msg(e), 3200, 'error');
    }
  }

  /** Put a placeholder into the text at the cursor. */
  async function insert(token: string) {
    if (!editing) return;
    const el = bodyEl;
    const start = el?.selectionStart ?? editing.body.length;
    const end = el?.selectionEnd ?? start;
    editing.body = editing.body.slice(0, start) + token + editing.body.slice(end);
    await tick();
    el?.focus();
    el?.setSelectionRange(start + token.length, start + token.length);
  }

  function onSearchKey(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      sel = hits.length ? (sel + 1) % hits.length : 0;
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      sel = hits.length ? (sel - 1 + hits.length) % hits.length : 0;
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const h = hits[sel];
      if (h) void copy(h.snippet);
    }
  }

  function onEditKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      void closeEdit();
    }
  }

  onMount(() => {
    void load();
  });
</script>

<div class="tab">
  <div class="bar">
    <div class="search">
      <Icon name="search" size={15} />
      <input
        bind:this={searchEl}
        bind:value={q}
        class="bare"
        type="text"
        placeholder="Search snippets by name, keyword or text"
        aria-label="Search snippets"
        autocomplete="off"
        spellcheck="false"
        onkeydown={onSearchKey} />
    </div>
    <button class="btn primary" type="button" onclick={() => startEdit(null)} disabled={!!editing || list.length >= LIMITS.snippets}>
      <Icon name="plus" size={14} /> New snippet
    </button>
  </div>

  <div class="body scroll">
    {#if editing}
      <!-- Escape closes the editor from any field in it -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <form class="card editor" onsubmit={save} onkeydown={onEditKey} aria-label={editing.id ? 'Edit snippet' : 'New snippet'}>
        <div class="two">
          <label>
            <span class="lbl">Name</span>
            <input class="input" bind:this={nameEl} bind:value={editing.name} maxlength={LIMITS.snippetName} placeholder="Email sign-off" autocomplete="off" />
          </label>
          <label>
            <span class="lbl">Keyword <span class="faint">(optional)</span></span>
            <input class="input" bind:value={editing.trigger} maxlength={LIMITS.snippetTrigger} placeholder=";sig" autocomplete="off" spellcheck="false" />
          </label>
        </div>
        <label>
          <span class="lbl">Text</span>
          <textarea class="input area" bind:this={bodyEl} bind:value={editing.body} rows="5" maxlength={LIMITS.snippetBody} placeholder="Thanks,&#10;Alex"></textarea>
        </label>
        <div class="holders">
          <span class="faint">Fill in at the moment you use it:</span>
          {#each PLACEHOLDERS as token (token)}
            <button class="chip" type="button" onclick={() => insert(token)} title="Insert {token}"><code>{token}</code></button>
          {/each}
        </div>
        {#if hint || saveError}
          <p class="error" role="alert"><Icon name="alert" size={14} /> {saveError || hint}</p>
        {/if}
        <div class="actions">
          <button class="btn" type="button" onclick={closeEdit}>Cancel</button>
          <button class="btn primary" type="submit" disabled={busy || !!why}><Icon name="check" size={14} /> Save</button>
        </div>
      </form>
    {/if}

    {#if loadError}
      <p class="error" role="alert"><Icon name="alert" size={14} /> {loadError}</p>
    {:else if !loaded}
      <p class="faint">Loading...</p>
    {:else if list.length === 0 && !editing}
      <State
        icon="text"
        title="No snippets yet"
        text="Save text you type again and again. Copy it with one keypress from here or from the command palette."
        action={{ label: 'New snippet', run: () => startEdit(null) }} />
    {:else if hits.length === 0 && list.length > 0}
      <p class="faint none">Nothing matches &ldquo;{q}&rdquo;.</p>
    {:else}
      <ul class="list" role="listbox" aria-label="Snippets">
        {#each hits as h, i (h.snippet.id)}
          {@const s = h.snippet}
          <li class="row card" class:sel={i === sel} role="option" aria-selected={i === sel} onpointerenter={() => (sel = i)}>
            <button class="main" type="button" onclick={() => copy(s)} aria-label="Copy {s.name}">
              <span class="top">
                <span class="nm">
                  {#each segments(s.name, h.ranges) as part, k (k)}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}
                </span>
                {#if s.trigger}<kbd class="kw">{s.trigger}</kbd>{/if}
              </span>
              <span class="pv ellipsis">{snippetPreview(s.body)}</span>
            </button>
            <div class="acts">
              {#if confirmingId === s.id}
                <span class="faint sure">Delete?</span>
                <button class="btn sm danger" type="button" onclick={() => remove(s)}>Delete</button>
                <button class="btn sm ghost" type="button" onclick={() => (confirmingId = null)}>Keep</button>
              {:else}
                <button class="btn sm" type="button" onclick={() => copy(s)} disabled={busy}>
                  <Icon name={copiedId === s.id ? 'check' : 'copy'} size={13} /> {copiedId === s.id ? 'Copied' : 'Copy'}
                </button>
                <button class="btn sm icon ghost" type="button" aria-label="Edit {s.name}" onclick={() => startEdit(s)} disabled={!!editing}>
                  <Icon name="pencil" size={13} />
                </button>
                <button class="btn sm icon ghost danger" type="button" aria-label="Delete {s.name}" onclick={() => (confirmingId = s.id)} disabled={!!editing}>
                  <Icon name="trash" size={13} />
                </button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  <p class="note">
    <Icon name="shield" size={13} />
    <span>
      Pick a snippet and press <kbd>Enter</kbd> to copy it. Krovi has no keyboard hook: it never watches what you type and never
      expands anything by itself. {demo ? 'In this preview the copy goes to your browser clipboard.' : 'Copies are not added to Clipboard history.'}
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
  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    align-content: start;
    gap: 10px;
    padding-right: 4px;
  }
  .editor {
    display: grid;
    gap: 10px;
    padding: 14px 16px;
    border-radius: var(--r-4);
  }
  .two {
    display: grid;
    grid-template-columns: 1fr 160px;
    gap: 10px;
  }
  label {
    display: grid;
    gap: 4px;
    min-width: 0;
  }
  .lbl {
    font-size: var(--t-xs);
    font-weight: 600;
    color: var(--text-2);
  }
  .area {
    height: auto;
    padding: 8px 12px;
    resize: vertical;
    line-height: 1.45;
    font-family: var(--font-mono);
    font-size: var(--t-xs);
  }
  .holders {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    font-size: var(--t-xs);
  }
  .holders code {
    font-family: var(--font-mono);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .error {
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
  .none {
    text-align: center;
    padding: 24px 0;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px 8px 6px;
    border-radius: var(--r-3);
  }
  .row.sel {
    border-color: var(--line-strong);
    background: var(--surface-2);
  }
  .main {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 3px;
    padding: 4px 8px;
    text-align: left;
    border-radius: var(--r-2);
    color: var(--text);
  }
  .top {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .nm {
    font-size: var(--t-md);
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  mark {
    background: none;
    color: var(--text);
    font-weight: 700;
    text-decoration: underline;
    text-decoration-color: var(--accent);
    text-underline-offset: 3px;
    text-decoration-thickness: 2px;
  }
  .pv {
    font-size: var(--t-xs);
    color: var(--text-3);
  }
  .acts {
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .sure {
    font-size: var(--t-xs);
    margin-right: 2px;
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
