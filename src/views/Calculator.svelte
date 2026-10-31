<script lang="ts">
  import { tryEvaluate } from '../lib/calc';
  import { roundDisplay } from '../lib/units';
  import Icon from '../components/Icon.svelte';

  let expr = $state('');
  let history = $state<{ expr: string; value: number }[]>([]);

  const preview = $derived.by(() => {
    if (!expr.trim()) return '';
    const r = tryEvaluate(expr);
    return r.ok ? String(roundDisplay(r.value, 8)) : '';
  });
  const invalid = $derived(expr.trim() !== '' && preview === '');

  function press(s: string) {
    expr += s;
  }
  function clear() {
    expr = '';
  }
  function back() {
    expr = expr.slice(0, -1);
  }
  function equals() {
    const r = tryEvaluate(expr);
    if (r.ok) {
      history = [{ expr, value: roundDisplay(r.value, 8) }, ...history].slice(0, 8);
      expr = String(roundDisplay(r.value, 8));
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter' || e.key === '=') {
      e.preventDefault();
      equals();
    }
  }

  const keys = [
    ['(', ')', '%', 'C'],
    ['7', '8', '9', '/'],
    ['4', '5', '6', '*'],
    ['1', '2', '3', '-'],
    ['0', '.', '=', '+'],
  ];
</script>

<div class="calc">
  <div class="screen">
    <input
      class="expr mono"
      bind:value={expr}
      placeholder="0"
      aria-label="expression"
      spellcheck="false"
      onkeydown={onKey} />
    <div class="preview mono" class:err={invalid}>
      {invalid ? 'invalid expression' : preview ? `= ${preview}` : ''}
    </div>
  </div>

  <div class="pad">
    {#each keys as row, r (r)}
      {#each row as k (k)}
        <button
          class="key"
          class:op={'+-*/%'.includes(k) && k.length === 1}
          class:eq={k === '='}
          class:fn={k === 'C'}
          onclick={() => (k === '=' ? equals() : k === 'C' ? clear() : press(k))}>
          {k === '*' ? '×' : k === '/' ? '÷' : k}
        </button>
      {/each}
    {/each}
    <button class="key wide fn" onclick={back}><Icon name="chevronLeft" size={16} /> Backspace</button>
  </div>

  {#if history.length}
    <div class="history">
      <p class="eyebrow">Recent</p>
      <ul>
        {#each history as h, i (i)}
          <li>
            <button class="hrow" onclick={() => (expr = String(h.value))}>
              <span class="he faint mono">{h.expr}</span>
              <span class="hv mono">= {h.value}</span>
            </button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</div>

<style>
  .calc {
    max-width: 340px;
    margin: 0 auto;
  }
  .screen {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 14px;
    margin-bottom: 12px;
  }
  .expr {
    width: 100%;
    border: none;
    background: transparent;
    color: var(--text);
    font-size: 26px;
    text-align: right;
    outline: none;
  }
  .preview {
    text-align: right;
    color: var(--accent);
    font-size: 15px;
    min-height: 20px;
    margin-top: 4px;
  }
  .preview.err {
    color: var(--danger);
    font-size: 12.5px;
  }
  .pad {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
  }
  .key {
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text);
    border-radius: 11px;
    padding: 15px 0;
    font-size: 17px;
    font-weight: 550;
    transition: background 0.12s ease, transform 0.08s ease;
  }
  .key:hover {
    background: var(--surface-3);
  }
  .key:active {
    transform: translateY(1px);
  }
  .key.op {
    color: var(--accent);
  }
  .key.fn {
    color: var(--text-dim);
  }
  .key.eq {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-contrast);
  }
  .key.eq:hover {
    background: var(--accent-strong);
  }
  .key.wide {
    grid-column: span 4;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    font-size: 13px;
  }
  .history {
    margin-top: 16px;
  }
  .history ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .hrow {
    width: 100%;
    display: flex;
    justify-content: space-between;
    gap: 12px;
    border: none;
    background: transparent;
    padding: 7px 9px;
    border-radius: 8px;
    color: var(--text);
  }
  .hrow:hover {
    background: var(--surface-2);
  }
  .he {
    font-size: 12.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hv {
    font-size: 13px;
    font-weight: 600;
    white-space: nowrap;
  }
</style>
