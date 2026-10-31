<script lang="ts">
  import { onMount } from 'svelte';
  import { bridge } from '../lib/bridge';
  import { convert, convertCurrency, roundDisplay, UNITS, type UnitCategory } from '../lib/units';
  import { relativeTime } from '../lib/time';
  import { now } from '../lib/stores';
  import { currencyUpdatedAt } from '../lib/fixtures';
  import Icon from '../components/Icon.svelte';
  import type { CurrencyRates } from '../lib/types';

  let tab = $state<'units' | 'currency'>('units');

  // --- units ---
  let category = $state<UnitCategory>('length');
  let value = $state(1);
  let from = $state('m');
  let to = $state('ft');

  const unitList = $derived(UNITS[category]);
  $effect(() => {
    if (!unitList.includes(from)) from = unitList[0];
    if (!unitList.includes(to)) to = unitList[1] ?? unitList[0];
  });
  const unitResult = $derived.by(() => {
    try {
      return roundDisplay(convert(category, value || 0, from, to));
    } catch {
      return NaN;
    }
  });

  // --- currency ---
  let rates = $state<CurrencyRates>({});
  let camount = $state(100);
  let cfrom = $state('USD');
  let cto = $state('EUR');
  const currencies = $derived(Object.keys(rates));
  const cResult = $derived.by(() => {
    try {
      return roundDisplay(convertCurrency(camount || 0, cfrom, cto, rates), 2);
    } catch {
      return NaN;
    }
  });

  function swapUnits() {
    [from, to] = [to, from];
  }
  function swapCur() {
    [cfrom, cto] = [cto, cfrom];
  }

  onMount(async () => {
    rates = await bridge.loadCurrencyRates();
  });
</script>

<div class="seg modes">
  <button aria-pressed={tab === 'units'} onclick={() => (tab = 'units')}>Units</button>
  <button aria-pressed={tab === 'currency'} onclick={() => (tab = 'currency')}>Currency</button>
</div>

{#if tab === 'units'}
  <div class="cats">
    {#each ['length', 'mass', 'temperature'] as c (c)}
      <button class="chip" class:on={category === c} onclick={() => (category = c as UnitCategory)}>{c}</button>
    {/each}
  </div>
  <div class="convert">
    <div class="side">
      <input class="input big" type="number" bind:value aria-label="value" />
      <select class="input" bind:value={from} aria-label="from unit">
        {#each unitList as u (u)}<option value={u}>{u}</option>{/each}
      </select>
    </div>
    <button class="btn icon swap" title="Swap" onclick={swapUnits}><Icon name="convert" size={18} /></button>
    <div class="side">
      <div class="result mono">{Number.isFinite(unitResult) ? unitResult : '—'}</div>
      <select class="input" bind:value={to} aria-label="to unit">
        {#each unitList as u (u)}<option value={u}>{u}</option>{/each}
      </select>
    </div>
  </div>
  <p class="faint note">{value} {from} = {Number.isFinite(unitResult) ? unitResult : '—'} {to}. Conversions run fully offline.</p>
{:else}
  <div class="convert">
    <div class="side">
      <input class="input big" type="number" bind:value={camount} aria-label="amount" />
      <select class="input" bind:value={cfrom} aria-label="from currency">
        {#each currencies as c (c)}<option value={c}>{c}</option>{/each}
      </select>
    </div>
    <button class="btn icon swap" title="Swap" onclick={swapCur}><Icon name="convert" size={18} /></button>
    <div class="side">
      <div class="result mono">{Number.isFinite(cResult) ? cResult : '—'}</div>
      <select class="input" bind:value={cto} aria-label="to currency">
        {#each currencies as c (c)}<option value={c}>{c}</option>{/each}
      </select>
    </div>
  </div>
  <p class="faint note">
    Rates cached {relativeTime(currencyUpdatedAt, $now)} from a keyless public API. Offline falls back to the last cached table (SPEC C10).
  </p>
{/if}

<style>
  .modes {
    margin-bottom: 16px;
  }
  .cats {
    display: flex;
    gap: 8px;
    margin-bottom: 18px;
  }
  .cats .chip {
    cursor: pointer;
    text-transform: capitalize;
    border: 1px solid var(--border);
  }
  .cats .chip.on {
    background: var(--accent-soft);
    color: var(--accent);
    border-color: var(--accent);
  }
  .convert {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: 14px;
    max-width: 560px;
  }
  .side {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .input.big {
    font-size: 24px;
    font-weight: 650;
    padding: 12px;
  }
  .result {
    font-size: 24px;
    font-weight: 680;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: 9px;
    background: var(--accent-soft);
    color: var(--accent);
    min-height: 52px;
    display: flex;
    align-items: center;
    overflow: hidden;
  }
  .swap {
    align-self: center;
    margin-top: 0;
  }
  .note {
    margin-top: 18px;
    font-size: 12.5px;
  }
</style>
