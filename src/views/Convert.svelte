<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import Segmented from '../components/Segmented.svelte';
  import State from '../components/State.svelte';
  import AnimatedNumber from '../components/AnimatedNumber.svelte';
  import { bridge } from '../lib/bridge';
  import { convert, convertCurrency, roundDisplay, UNITS, type UnitCategory } from '../lib/units';
  import { copyText, now, pushToast } from '../lib/stores';
  import { relativeTime } from '../lib/time';
  import { spotlight } from '../lib/motion';
  import type { CurrencyInfo } from '../lib/types';

  type Mode = UnitCategory | 'currency';
  let mode = $state<Mode>('length');

  // units
  let value = $state(1);
  let from = $state('m');
  let to = $state('ft');
  // currency
  let info = $state<CurrencyInfo | null>(null);
  let ratesStatus = $state<'idle' | 'loading' | 'ready' | 'error'>('idle');
  const rates = $derived(info?.rates ?? {});
  let amount = $state(100);
  let cfrom = $state('USD');
  let cto = $state('EUR');

  const isCur = $derived(mode === 'currency');
  const list = $derived(isCur ? Object.keys(rates) : UNITS[mode as UnitCategory]);
  const fromU = $derived(isCur ? cfrom : from);
  const toU = $derived(isCur ? cto : to);

  $effect(() => {
    if (mode === 'currency') return;
    const l = UNITS[mode];
    if (!l.includes(from)) from = l[0];
    if (!l.includes(to)) to = l[1] ?? l[0];
  });

  const result = $derived.by(() => {
    try {
      return isCur
        ? roundDisplay(convertCurrency(amount || 0, cfrom, cto, rates), 2)
        : roundDisplay(convert(mode as UnitCategory, value || 0, from, to), 6);
    } catch {
      return NaN;
    }
  });
  const ok = $derived(Number.isFinite(result));
  const unitRate = $derived.by(() => {
    try {
      return isCur
        ? roundDisplay(convertCurrency(1, cfrom, cto, rates), 4)
        : roundDisplay(convert(mode as UnitCategory, 1, from, to), 6);
    } catch {
      return NaN;
    }
  });

  function swap() {
    if (isCur) [cfrom, cto] = [cto, cfrom];
    else [from, to] = [to, from];
  }
  async function loadRates(force = false) {
    ratesStatus = 'loading';
    try {
      const next = await bridge.loadCurrencyInfo(force);
      const codes = Object.keys(next.rates);
      if (!codes.length) {
        info = next;
        ratesStatus = 'error';
        return;
      }
      info = next;
      if (!next.rates[cfrom]) cfrom = codes[0];
      if (!next.rates[cto]) cto = codes[1] ?? codes[0];
      ratesStatus = 'ready';
    } catch {
      ratesStatus = 'error';
    }
  }
  const provenance = $derived.by(() => {
    if (!info) return '';
    const parts: string[] = [];
    if (info.date) parts.push(`Rates for ${info.date}`);
    if (info.updatedAt) parts.push(`fetched ${relativeTime(info.updatedAt, $now)}`);
    return parts.join(' · ');
  });
  $effect(() => {
    if (mode === 'currency' && ratesStatus === 'idle') void loadRates();
  });

  async function copyResult() {
    if (!ok) return;
    if (await copyText(String(result))) pushToast('Result copied');
  }

  const fmt = (n: number) => (Number.isFinite(n) ? n.toLocaleString(undefined, { maximumFractionDigits: isCur ? 2 : 6 }) : '—');
</script>

<div class="view conv">
  <div class="top">
    <Segmented
      label="Conversion type"
      bind:value={mode}
      options={[
        { id: 'length', label: 'Length' },
        { id: 'mass', label: 'Mass' },
        { id: 'temperature', label: 'Temperature' },
        { id: 'currency', label: 'Currency' },
      ]} />
  </div>

  {#if isCur && ratesStatus === 'loading'}
    <div class="pair">
      <div class="skel" style="height:150px;border-radius:20px"></div>
      <div></div>
      <div class="skel" style="height:150px;border-radius:20px"></div>
    </div>
  {:else if isCur && ratesStatus === 'error'}
    <State
      tone="error"
      title={'Couldn’t load exchange rates'}
      text={info?.error ?? 'Rates come from a keyless public API and are cached for offline use. Check your connection.'}
      action={{ label: 'Try again', run: () => loadRates(true) }} />
  {:else}
    <div class="pair">
      <section class="card box" use:spotlight>
        <span class="eyebrow">From</span>
        {#if isCur}
          <input class="amount num" type="number" bind:value={amount} aria-label="Amount" />
        {:else}
          <input class="amount num" type="number" bind:value aria-label="Value" />
        {/if}
        <select class="input" value={fromU} onchange={(e) => (isCur ? (cfrom = e.currentTarget.value) : (from = e.currentTarget.value))} aria-label="From unit">
          {#each list as u (u)}<option value={u}>{u}</option>{/each}
        </select>
      </section>

      <button class="swap btn icon" aria-label="Swap units" onclick={swap}><Icon name="convert" size={17} /></button>

      <section class="card box res" use:spotlight>
        <span class="eyebrow">To</span>
        <div class="amount num out" aria-live="polite">
          {#if ok}<AnimatedNumber value={result} duration={420} format={(n) => fmt(n)} />{:else}&mdash;{/if}
        </div>
        <select class="input" value={toU} onchange={(e) => (isCur ? (cto = e.currentTarget.value) : (to = e.currentTarget.value))} aria-label="To unit">
          {#each list as u (u)}<option value={u}>{u}</option>{/each}
        </select>
      </section>
    </div>

    <div class="foot">
      <p class="faint line num">
        {#if ok}1 {fromU} = {fmt(unitRate)} {toU}{:else}Choose two units to compare{/if}
      </p>
      <button class="btn sm" onclick={copyResult} disabled={!ok}><Icon name="copy" size={13} /> Copy result</button>
    </div>
    {#if isCur && info}
      <div class="prov faint">
        <span class="tag" class:warn={info.stale}>
          {info.stale ? 'Cached rates' : 'Live rates'}
        </span>
        <span>{provenance || 'From a keyless public API'}</span>
        {#if info.stale && info.error}<span>· {info.error}</span>{/if}
        <button class="btn sm ghost refresh" onclick={() => loadRates(true)} aria-label="Refresh exchange rates">
          <Icon name="refresh" size={13} /> Refresh
        </button>
      </div>
    {:else}
      <p class="faint note">Unit conversions run entirely on your device.</p>
    {/if}
  {/if}
</div>

<style>
  .conv {
    align-items: center;
    justify-content: center;
    padding-bottom: 54px;
  }
  .conv > :global(*) {
    width: 100%;
    max-width: 740px;
  }
  .top {
    display: flex;
    justify-content: center;
    margin-bottom: 26px;
    flex: none;
  }
  .pair {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
    align-items: center;
    gap: 14px;
  }
  .box {
    display: grid;
    gap: 12px;
    padding: 20px 22px 22px;
    border-radius: var(--r-4);
  }
  .amount {
    width: 100%;
    min-height: 50px;
    padding: 0;
    border: 0;
    background: none;
    outline: none;
    color: var(--text);
    font-size: 44px;
    font-weight: 560;
    letter-spacing: -0.04em;
    line-height: 1.2;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .amount:focus-visible {
    outline: none;
  }
  .out {
    display: flex;
    align-items: center;
  }
  .res {
    background: var(--surface-2);
  }
  .swap {
    width: 38px;
    height: 38px;
    border-radius: 50%;
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: 16px;
  }
  .line {
    font-size: var(--t-sm);
  }
  .prov {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 20px;
    font-size: var(--t-xs);
    flex-wrap: wrap;
  }
  .prov .tag.warn {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .refresh {
    margin-left: auto;
  }
  .note {
    margin-top: 22px;
    font-size: var(--t-xs);
  }
</style>
