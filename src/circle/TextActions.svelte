<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import { copyText, pushToast } from '../lib/stores';
  import { summarize } from '../lib/explain';
  import * as ci from '../lib/circle';

  // The text read from the screen, with the three things that need nothing but this PC and a click:
  // Copy, Summarise (Krovi's on-device summariser) and Search Google (opens the browser, only on click).
  // `extra` lets the parent add its own buttons to the same row (e.g. "Ask AI").
  let {
    text,
    lines = [],
    emptyNote = 'No text was found in this area.',
    extra,
  }: {
    text: string;
    /** lines to point out (for example the ones that match the question) */
    lines?: ci.CircleLine[];
    emptyNote?: string;
    extra?: Snippet;
  } = $props();

  let summary = $state<string | null>(null);
  let sumBusy = $state(false);
  let sumError = $state<string | null>(null);
  let sumFor = '';

  // A different text means the old summary no longer belongs to it.
  $effect(() => {
    if (text !== sumFor) {
      summary = null;
      sumError = null;
    }
  });

  const longText = $derived([...text].length > ci.MAX_QUERY_CHARS);

  async function copy() {
    const ok = await copyText(text);
    pushToast(ok ? 'Copied' : 'Couldn’t copy that', ok ? 1400 : 2600, ok ? undefined : 'error');
  }

  async function search() {
    try {
      await ci.searchWeb(text);
    } catch (e) {
      pushToast(ci.errorText(e), 3200, 'error');
    }
  }

  async function summarise() {
    const src = text;
    sumBusy = true;
    sumError = null;
    try {
      const s = await summarize(src, 'short');
      summary = s.text;
      sumFor = src;
    } catch (e) {
      sumError = ci.errorText(e);
    } finally {
      sumBusy = false;
    }
  }
</script>

<div class="tx">
  <div class="out scroll" tabindex="0" role="textbox" aria-readonly="true" aria-multiline="true" aria-label="Text read from the screen">
    {#if text}{text}{:else}<span class="faint">{emptyNote}</span>{/if}
  </div>

  {#if lines.length}
    <ul class="hits" aria-label="Lines that match your question">
      {#each lines as l (l.y + ':' + l.text)}
        <li><Icon name="search" size={11} /> {l.text}</li>
      {/each}
    </ul>
  {/if}

  <div class="row">
    <button class="btn sm" onclick={copy} disabled={!text}><Icon name="copy" size={13} /> Copy</button>
    <button
      class="btn sm"
      onclick={search}
      disabled={!text}
      title={longText ? `Opens your browser with the first ${ci.MAX_QUERY_CHARS} characters as a Google search` : 'Opens your browser with this text as a Google search'}>
      <Icon name="externalLink" size={13} /> Search Google
    </button>
    <button class="btn sm" onclick={summarise} disabled={!text || sumBusy} title="A short summary, worked out on this PC">
      <Icon name={sumBusy ? 'loader' : 'sparkles'} size={13} /> Summarise
    </button>
    {@render extra?.()}
  </div>

  {#if text}
    <p class="fine faint"><Icon name="lock" size={11} /> Copy and Summarise stay on this PC. Search Google opens your browser{longText ? ` (first ${ci.MAX_QUERY_CHARS} characters)` : ''}.</p>
  {/if}

  {#if sumError}
    <div class="problem" role="alert"><Icon name="alert" size={14} /> <span>{sumError}</span></div>
  {/if}
  {#if summary}
    <div class="sum">
      <h3 class="eyebrow">Summary &middot; on this PC</h3>
      <p>{summary}</p>
    </div>
  {/if}
</div>

<style>
  .tx {
    display: grid;
    gap: 10px;
    min-height: 0;
  }
  .out {
    max-height: 190px;
    overflow-y: auto;
    padding: 10px 12px;
    border-radius: var(--r-2);
    background: var(--surface-1);
    border: 1px solid var(--line);
    font-size: var(--t-sm);
    line-height: 1.55;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
    cursor: text;
  }
  .out:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .hits {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 4px;
    font-size: var(--t-xs);
    color: var(--text-2);
  }
  .hits li {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    border-radius: var(--r-1);
    background: var(--accent-soft);
    color: var(--text);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .fine {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: var(--t-xs);
  }
  .problem {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-radius: var(--r-2);
    border: 1px solid color-mix(in srgb, var(--danger) 40%, transparent);
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    color: var(--danger);
    font-size: var(--t-sm);
    font-weight: 560;
  }
  .sum {
    padding: 10px 12px;
    border-radius: var(--r-2);
    background: var(--surface-1);
    border: 1px solid var(--line);
  }
  .sum h3 {
    margin: 0 0 4px;
  }
  .sum p {
    margin: 0;
    font-size: var(--t-sm);
    line-height: 1.55;
    user-select: text;
  }
</style>
