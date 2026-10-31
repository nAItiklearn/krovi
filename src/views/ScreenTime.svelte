<script lang="ts">
  import { onMount } from 'svelte';
  import { bridge } from '../lib/bridge';
  import { today } from '../lib/fixtures';
  import Icon from '../components/Icon.svelte';
  import type { ScreenTimeRecord } from '../lib/types';

  let records = $state<ScreenTimeRecord[]>([]);
  let day = $state(today);

  const days = $derived(
    [...new Set(records.map((r) => r.day))].sort((a, b) => b - a),
  );
  const forDay = $derived(
    records.filter((r) => r.day === day).sort((a, b) => b.ms - a.ms),
  );
  const total = $derived(forDay.reduce((a, r) => a + r.ms, 0));
  const max = $derived(forDay[0]?.ms ?? 1);

  function hm(ms: number): string {
    const m = Math.round(ms / 60_000);
    const h = Math.floor(m / 60);
    return h > 0 ? `${h}h ${m % 60}m` : `${m}m`;
  }
  function dayLabel(d: number): string {
    if (d === today) return 'Today';
    if (d === today - 1) return 'Yesterday';
    return new Date(d * 86_400_000).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
  }

  onMount(async () => {
    records = await bridge.loadScreenTime();
  });
</script>

<div class="head">
  <div>
    <p class="eyebrow"><Icon name="screentime" size={13} /> Screen time · {dayLabel(day)}</p>
    <div class="total">{hm(total)}</div>
  </div>
  <div class="seg">
    {#each days as d (d)}
      <button aria-pressed={day === d} onclick={() => (day = d)}>{dayLabel(d)}</button>
    {/each}
  </div>
</div>

<div class="apps">
  {#each forDay as r (r.app)}
    <div class="app">
      <span class="name">{r.app}</span>
      <span class="track"><span class="fill" style={`width:${(r.ms / max) * 100}%`}></span></span>
      <span class="dur mono">{hm(r.ms)}</span>
    </div>
  {:else}
    <p class="faint">No activity recorded for this day.</p>
  {/each}
</div>

<p class="privacy faint">
  <Icon name="drop" size={13} /> Local-only: Krovi samples the foreground process name and duration, aggregated per app per day. No window titles, no URLs, nothing leaves your device (SPEC C11).
</p>

<style>
  .head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 14px;
    margin-bottom: 18px;
    flex-wrap: wrap;
  }
  .total {
    font-size: 34px;
    font-weight: 700;
    letter-spacing: -0.02em;
  }
  .apps {
    display: flex;
    flex-direction: column;
    gap: 11px;
  }
  .app {
    display: grid;
    grid-template-columns: 170px 1fr 64px;
    align-items: center;
    gap: 12px;
  }
  .name {
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .track {
    height: 10px;
    background: var(--surface-3);
    border-radius: 999px;
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 100%;
    background: linear-gradient(90deg, var(--accent), var(--accent-strong));
    border-radius: 999px;
  }
  .dur {
    text-align: right;
    font-size: 12.5px;
    color: var(--text-dim);
  }
  .privacy {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-top: 22px;
    font-size: 12px;
  }
  @media (max-width: 560px) {
    .app {
      grid-template-columns: 110px 1fr 56px;
    }
  }
</style>
