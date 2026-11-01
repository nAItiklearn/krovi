<script lang="ts">
  import { onMount } from 'svelte';
  import { flip } from 'svelte/animate';
  import Icon from '../components/Icon.svelte';
  import { tryEvaluate } from '../lib/calc';
  import { roundDisplay } from '../lib/units';
  import { copyText, pushToast } from '../lib/stores';
  import { arrive } from '../lib/motion';

  let expr = $state('');
  let history = $state<{ expr: string; value: number }[]>([]);
  let inputEl: HTMLInputElement | undefined = $state();

  const preview = $derived.by(() => {
    if (!expr.trim()) return '';
    const r = tryEvaluate(expr);
    return r.ok ? String(roundDisplay(r.value, 8)) : '';
  });
  const invalid = $derived(expr.trim() !== '' && preview === '');

  function press(s: string) {
    expr += s;
    inputEl?.focus();
  }
  function equals() {
    const r = tryEvaluate(expr);
    if (!r.ok) return;
    const v = roundDisplay(r.value, 8);
    history = [{ expr, value: v }, ...history].slice(0, 12);
    expr = String(v);
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter' || e.key === '=') {
      e.preventDefault();
      equals();
    } else if (e.key === 'Escape' && expr) {
      e.stopPropagation(); // clear first, collapse on the next Esc
      expr = '';
    }
  }
  async function copy(v: string) {
    if (await copyText(v)) pushToast('Copied');
  }

  type Key = { k: string; kind?: 'op' | 'fn' | 'eq'; label?: string };
  const keys: Key[] = [
    { k: 'C', kind: 'fn' }, { k: '(', kind: 'fn' }, { k: ')', kind: 'fn' }, { k: '/', kind: 'op', label: '÷' },
    { k: '7' }, { k: '8' }, { k: '9' }, { k: '*', kind: 'op', label: '×' },
    { k: '4' }, { k: '5' }, { k: '6' }, { k: '-', kind: 'op', label: '−' },
    { k: '1' }, { k: '2' }, { k: '3' }, { k: '+', kind: 'op' },
    { k: '0' }, { k: '.' }, { k: '%', kind: 'fn' }, { k: '=', kind: 'eq' },
  ];

  onMount(() => inputEl?.focus());
</script>

<div class="view">
  <div class="calc">
    <section class="pad">
      <div class="screen card">
        <input
          bind:this={inputEl}
          bind:value={expr}
          class="expr num"
          placeholder="0"
          aria-label="Expression"
          spellcheck="false"
          autocomplete="off"
          onkeydown={onKey} />
        <button class="out num" class:err={invalid} class:has={!!preview} onclick={() => preview && copy(preview)} disabled={!preview} aria-label={preview ? `Result ${preview}, click to copy` : 'No result'}>
          {invalid ? 'Check the expression' : preview ? `= ${preview}` : ' '}
        </button>
      </div>
      <div class="keys">
        {#each keys as key (key.k)}
          <button
            class="key"
            class:op={key.kind === 'op'}
            class:fn={key.kind === 'fn'}
            class:eq={key.kind === 'eq'}
            aria-label={key.k === 'C' ? 'Clear' : key.k === '*' ? 'times' : key.k === '/' ? 'divide' : key.k === '-' ? 'minus' : key.k}
            onclick={() => (key.k === '=' ? equals() : key.k === 'C' ? ((expr = ''), inputEl?.focus()) : press(key.k))}>
            {key.label ?? key.k}
          </button>
        {/each}
      </div>
    </section>

    <aside class="hist" aria-label="History">
      <div class="hh">
        <p class="eyebrow"><Icon name="clock" size={13} /> History</p>
        {#if history.length}<button class="btn sm ghost" onclick={() => (history = [])}>Clear</button>{/if}
      </div>
      {#if history.length}
        <ul class="scroll">
          {#each history as h (h)}
            <li animate:flip={{ duration: 340 }} in:arrive>
              <button onclick={() => (expr = String(h.value))} title="Use this result">
                <span class="he faint num ellipsis">{h.expr}</span>
                <span class="hv num">= {h.value}</span>
              </button>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="faint none">Results you calculate appear here. Type on your keyboard or tap the keys. Enter evaluates, Esc clears.</p>
      {/if}
    </aside>
  </div>
</div>

<style>
  .calc {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 330px 1fr;
    gap: 34px;
    justify-content: center;
    padding: 0 10px;
  }
  .screen {
    display: grid;
    gap: 2px;
    padding: 12px 16px 10px;
    margin-bottom: 12px;
    border-radius: var(--r-4);
  }
  .expr {
    width: 100%;
    border: 0;
    background: none;
    outline: none;
    text-align: right;
    color: var(--text);
    font-size: 34px;
    font-weight: 500;
    letter-spacing: -0.04em;
    line-height: 1.2;
  }
  .expr::placeholder {
    color: var(--text-3);
  }
  .out {
    text-align: right;
    min-height: 22px;
    font-size: var(--t-lg);
    color: var(--text-3);
    font-weight: 500;
  }
  .out.has {
    color: var(--text-2);
    cursor: copy;
  }
  .out.err {
    color: var(--danger);
    font-size: var(--t-sm);
  }
  .keys {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
  }
  .key {
    height: 52px;
    border-radius: var(--r-3);
    background: var(--surface-2);
    border: 1px solid var(--line);
    box-shadow: inset 0 1px 0 var(--card-hi);
    color: var(--text);
    font-size: 19px;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    transition:
      background var(--dur-1) var(--ease-out),
      transform 0.45s var(--spring);
  }
  .key:hover {
    background: var(--surface-3);
  }
  .key:active {
    transform: scale(0.93);
    transition-duration: 0.05s;
  }
  .key.fn {
    background: var(--surface-1);
    color: var(--text-2);
  }
  .key.op {
    background: var(--surface-3);
  }
  .key.eq {
    background: var(--invert-bg);
    color: var(--invert-fg);
    border-color: transparent;
  }
  .key.eq:hover {
    opacity: 0.9;
  }

  .hist {
    display: flex;
    flex-direction: column;
    min-height: 0;
    gap: 6px;
    padding-left: 30px;
    border-left: 1px solid var(--line);
  }
  .hh {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 28px;
  }
  .hist ul {
    flex: 1;
    min-height: 0;
    display: grid;
    align-content: start;
    gap: 2px;
  }
  .hist li button {
    display: grid;
    gap: 1px;
    width: 100%;
    padding: 8px 10px;
    border-radius: var(--r-2);
    text-align: right;
    transition: background var(--dur-1);
  }
  .hist li button:hover {
    background: var(--surface-2);
  }
  .he {
    font-size: var(--t-xs);
  }
  .hv {
    font-size: var(--t-lg);
    font-weight: 600;
  }
  .none {
    font-size: var(--t-sm);
    line-height: 1.5;
    max-width: 32ch;
    padding-top: 6px;
  }
</style>
