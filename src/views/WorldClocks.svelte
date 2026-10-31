<script lang="ts">
  import { onMount } from 'svelte';
  import { now } from '../lib/stores';
  import { bridge } from '../lib/bridge';
  import Icon from '../components/Icon.svelte';
  import type { WorldClockZone } from '../lib/types';

  let zones = $state<WorldClockZone[]>([]);
  let adding = $state(false);

  const presets: WorldClockZone[] = [
    { label: 'San Francisco', tz: 'America/Los_Angeles' },
    { label: 'New York', tz: 'America/New_York' },
    { label: 'London', tz: 'Europe/London' },
    { label: 'Berlin', tz: 'Europe/Berlin' },
    { label: 'Mumbai', tz: 'Asia/Kolkata' },
    { label: 'Singapore', tz: 'Asia/Singapore' },
    { label: 'Tokyo', tz: 'Asia/Tokyo' },
    { label: 'Sydney', tz: 'Australia/Sydney' },
  ];

  const available = $derived(presets.filter((p) => !zones.some((z) => z.tz === p.tz)));

  function parts(tz: string, n: number) {
    const time = new Intl.DateTimeFormat('en-GB', {
      timeZone: tz,
      hour: '2-digit',
      minute: '2-digit',
    }).format(n);
    const date = new Intl.DateTimeFormat(undefined, {
      timeZone: tz,
      weekday: 'short',
      day: 'numeric',
      month: 'short',
    }).format(n);
    const hour = Number(
      new Intl.DateTimeFormat('en-GB', { timeZone: tz, hour: '2-digit', hour12: false }).format(n),
    );
    const day = hour >= 6 && hour < 19;
    // Offset vs local, in hours.
    const localH = new Date(n).getHours();
    let diff = hour - localH;
    if (diff > 12) diff -= 24;
    if (diff < -12) diff += 24;
    return { time, date, day, diff };
  }

  function remove(tz: string) {
    zones = zones.filter((z) => z.tz !== tz);
  }
  function add(z: WorldClockZone) {
    zones = [...zones, z];
    adding = false;
  }

  onMount(async () => {
    zones = await bridge.loadWorldClocks();
  });
</script>

<div class="head">
  <p class="eyebrow"><Icon name="clocks" size={13} /> World clocks</p>
  <button class="btn sm" onclick={() => (adding = !adding)} disabled={available.length === 0}>
    <Icon name="plus" size={15} /> Add zone
  </button>
</div>

{#if adding}
  <div class="picker card">
    {#each available as p (p.tz)}
      <button class="chip" onclick={() => add(p)}><Icon name="plus" size={13} /> {p.label}</button>
    {/each}
  </div>
{/if}

<div class="clocks">
  {#each zones as z (z.tz)}
    {@const p = parts(z.tz, $now)}
    <div class="clock card">
      <div class="meta">
        <div class="label">
          {z.label}
          <Icon name={p.day ? 'sun' : 'moon'} size={14} />
        </div>
        <div class="date faint">{p.date} · {z.label === 'Local' ? 'your time' : p.diff === 0 ? 'same' : `${p.diff > 0 ? '+' : ''}${p.diff}h`}</div>
      </div>
      <div class="time mono">{p.time}</div>
      {#if z.label !== 'Local'}
        <button class="x" title="Remove" onclick={() => remove(z.tz)}><Icon name="close" size={14} /></button>
      {/if}
    </div>
  {/each}
</div>

<style>
  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 12px;
  }
  .head .eyebrow {
    margin: 0;
  }
  .picker {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 14px;
  }
  .picker .chip {
    border: 1px solid var(--border);
    cursor: pointer;
  }
  .picker .chip:hover {
    border-color: var(--accent);
    color: var(--accent);
  }
  .clocks {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 12px;
  }
  .clock {
    display: flex;
    align-items: center;
    gap: 12px;
    position: relative;
  }
  .meta {
    flex: 1;
    min-width: 0;
  }
  .label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 14px;
    font-weight: 600;
    color: var(--accent);
  }
  .date {
    font-size: 11.5px;
    margin-top: 2px;
  }
  .time {
    font-size: 30px;
    font-weight: 680;
    letter-spacing: -0.02em;
  }
  .x {
    position: absolute;
    top: 8px;
    right: 8px;
    border: none;
    background: transparent;
    color: var(--text-faint);
    padding: 4px;
    border-radius: 6px;
  }
  .x:hover {
    color: var(--danger);
    background: var(--surface-3);
  }
</style>
