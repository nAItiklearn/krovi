<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import { bridge } from '../lib/bridge';
  import type { Place } from '../lib/types';

  // City picker: only the text typed here ever leaves the device (to the geocoder).
  let {
    onpick,
    busy = false,
    autofocus = true,
  }: { onpick: (p: Place) => void; busy?: boolean; autofocus?: boolean } = $props();

  let q = $state('');
  let results = $state<Place[]>([]);
  let state_ = $state<'idle' | 'searching' | 'empty' | 'error'>('idle');
  let sel = $state(0);
  let inputEl: HTMLInputElement | undefined = $state();
  let timer: ReturnType<typeof setTimeout> | null = null;
  let seq = 0;

  function onInput() {
    if (timer) clearTimeout(timer);
    const text = q.trim();
    if (text.length < 2) {
      results = [];
      state_ = 'idle';
      return;
    }
    state_ = 'searching';
    timer = setTimeout(async () => {
      const mine = ++seq;
      try {
        const found = await bridge.searchPlaces(text);
        if (mine !== seq) return; // a newer keystroke superseded this lookup
        results = found.slice(0, 6);
        sel = 0;
        state_ = found.length ? 'idle' : 'empty';
      } catch {
        if (mine === seq) state_ = 'error';
      }
    }, 280);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      sel = Math.min(results.length - 1, sel + 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      sel = Math.max(0, sel - 1);
    } else if (e.key === 'Enter' && results[sel]) {
      e.preventDefault();
      onpick(results[sel]);
    }
  }

  onMount(() => {
    if (autofocus) inputEl?.focus();
  });
  onDestroy(() => {
    if (timer) clearTimeout(timer);
  });
</script>

<div class="ps">
  <label class="field">
    <Icon name="search" size={15} />
    <input
      bind:this={inputEl}
      bind:value={q}
      oninput={onInput}
      onkeydown={onKey}
      placeholder="Search for a city"
      aria-label="Search for a city"
      autocomplete="off"
      spellcheck="false"
      disabled={busy} />
    {#if state_ === 'searching' || busy}<span class="spin" aria-label="Searching"></span>{/if}
  </label>
  <ul role="listbox" aria-label="Matching places">
    {#each results as p, i (p.name + p.lat)}
      <li role="option" aria-selected={i === sel}>
        <button class:sel={i === sel} onpointerenter={() => (sel = i)} onclick={() => onpick(p)} disabled={busy}>
          <Icon name="mapPin" size={14} />
          <span class="ellipsis">{p.name}</span>
        </button>
      </li>
    {/each}
    {#if state_ === 'empty'}<li class="note faint">No place matches &ldquo;{q.trim()}&rdquo;.</li>{/if}
    {#if state_ === 'error'}<li class="note err">Couldn&rsquo;t reach the place search. Check your connection.</li>{/if}
  </ul>
</div>

<style>
  .ps {
    display: grid;
    gap: 6px;
    width: 100%;
  }
  .field {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 40px;
    padding: 0 12px;
    border-radius: var(--r-3);
    border: 1px solid var(--line);
    background: var(--surface-1);
    color: var(--text-3);
    transition:
      border-color var(--dur-1),
      box-shadow var(--dur-2);
  }
  .field:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: none;
    outline: none;
    color: var(--text);
    font-size: var(--t-md);
    font-weight: 500;
  }
  input::placeholder {
    color: var(--text-3);
  }
  ul {
    display: grid;
    gap: 2px;
  }
  button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 36px;
    padding: 0 12px;
    border-radius: var(--r-2);
    text-align: left;
    color: var(--text);
    font-size: var(--t-sm);
    font-weight: 560;
    transition: background var(--dur-1);
  }
  button.sel {
    background: var(--surface-2);
  }
  button :global(.ic) {
    color: var(--text-3);
  }
  .note {
    padding: 8px 12px;
    font-size: var(--t-sm);
  }
  .note.err {
    color: var(--danger);
  }
  .spin {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid var(--surface-3);
    border-top-color: var(--text-2);
    animation: spin 0.7s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
