<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import State from '../components/State.svelte';
  import { bridge } from '../lib/bridge';
  import { spotlight } from '../lib/motion';
  import type { ScreenTimeRecord } from '../lib/types';

  const DAY = 86_400_000;
  const today = Math.floor(Date.now() / DAY);

  let records = $state<ScreenTimeRecord[]>([]);
  let status = $state<'loading' | 'ready' | 'error'>('loading');
  let day = $state(today);

  async function load() {
    status = 'loading';
    try {
      records = await bridge.loadScreenTime();
      const latest = records.reduce((a, r) => Math.max(a, r.day), 0);
      day = records.some((r) => r.day === today) || !latest ? today : latest;
      status = 'ready';
    } catch {
      status = 'error';
    }
  }
  onMount(load);

  // last 7 days, oldest to newest
  const week = $derived(
    Array.from({ length: 7 }, (_, i) => today - 6 + i).map((d) => ({
      d,
      ms: records.filter((r) => r.day === d).reduce((a, r) => a + r.ms, 0),
    })),
  );
  const weekMax = $derived(Math.max(1, ...week.map((w) => w.ms)));
  const forDay = $derived(records.filter((r) => r.day === day).sort((a, b) => b.ms - a.ms));
  const total = $derived(forDay.reduce((a, r) => a + r.ms, 0));
  const max = $derived(forDay[0]?.ms ?? 1);

  const hm = (ms: number) => {
    const m = Math.round(ms / 60_000);
    const h = Math.floor(m / 60);
    return h > 0 ? `${h}h ${m % 60}m` : `${m}m`;
  };
  const dayName = (d: number, long = false) =>
    d === today
      ? 'Today'
      : d === today - 1
        ? 'Yesterday'
        : new Date(d * DAY + 12 * 3_600_000).toLocaleDateString(undefined, long ? { weekday: 'long', day: 'numeric', month: 'short' } : { weekday: 'short' });
  const appName = (s: string) => s.replace(/\.exe$/i, '');
  const wk = (d: number) => new Date(d * DAY + 12 * 3_600_000).toLocaleDateString(undefined, { weekday: 'short' });
  let pick = $state(String(today));
  $effect(() => {
    day = Number(pick);
  });
</script>

<div class="view">
  {#if status === 'loading'}
    <div class="skel" style="height:64px;width:220px;margin-bottom:22px"></div>
    {#each Array(5) as _, i (i)}<div class="skel" style="height:28px;margin-bottom:12px"></div>{/each}
  {:else if status === 'error'}
    <State tone="error" title={'Couldn’t read screen time'} text="Your usage log could not be read from this device." action={{ label: 'Try again', run: load }} />
  {:else if records.length === 0}
    <State icon="screentime" title="No activity recorded yet" text="Krovi notes which app is in front and for how long, aggregated per app per day. Window titles and URLs are never captured. Check back after some time at your PC." />
  {:else}
    <div class="head">
      <div>
        <p class="eyebrow"><Icon name="screentime" size={13} /> {dayName(day, true)}</p>
        <div class="total num">{hm(total)}</div>
      </div>
      <div class="spark card" use:spotlight role="img" aria-label="Screen time over the last seven days">
        {#each week as w (w.d)}
          <button class="col" class:on={w.d === day} aria-label={`${wk(w.d)}${w.d === today ? ' (today)' : ''}: ${hm(w.ms)}`} onclick={() => (pick = String(w.d))}>
            <span class="barwrap"><i style:height="{Math.max(4, (w.ms / weekMax) * 100)}%"></i></span>
            <span class="dl">{wk(w.d)}</span>
          </button>
        {/each}
      </div>
    </div>

    {#if forDay.length}
      <ul class="apps scroll">
        {#each forDay as r, i (r.app)}
          <li style:--i={i}>
            <span class="name ellipsis">{appName(r.app)}</span>
            <span class="track"><span class="fill" class:top={i === 0} style:width="{(r.ms / max) * 100}%"></span></span>
            <span class="dur num">{hm(r.ms)}</span>
          </li>
        {/each}
      </ul>
    {:else}
      <State compact icon="screentime" title="Nothing recorded this day" text="Pick another day above." />
    {/if}

    <p class="privacy faint">
      <Icon name="lock" size={13} /> Local only. Krovi samples the foreground app name and duration, per app per day. No window titles, no URLs, nothing leaves your device.
    </p>
  {/if}
</div>

<style>
  .head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 20px;
    margin-bottom: 18px;
    flex: none;
  }
  .total {
    font-size: var(--t-4xl);
    font-weight: 560;
    letter-spacing: -0.05em;
    line-height: 1.05;
    margin-top: 4px;
  }
  .spark {
    display: flex;
    gap: 4px;
    padding: 10px 12px 8px;
    height: 104px;
  }
  .col {
    display: grid;
    grid-template-rows: 1fr auto;
    justify-items: center;
    gap: 5px;
    width: 34px;
    border-radius: 9px;
    padding: 0 2px;
    transition: background var(--dur-1);
  }
  .col:hover {
    background: var(--surface-2);
  }
  .barwrap {
    display: flex;
    align-items: flex-end;
    width: 100%;
    min-height: 0;
  }
  .barwrap i {
    display: block;
    width: 100%;
    border-radius: 5px;
    background: var(--surface-3);
    transition:
      height 0.7s var(--ease-out),
      background var(--dur-2);
  }
  .col.on .barwrap i {
    background: var(--text);
  }
  .dl {
    font-size: var(--t-micro);
    color: var(--text-3);
    font-weight: 600;
  }
  .col.on .dl {
    color: var(--text);
  }
  .apps {
    flex: 1;
    min-height: 0;
    display: grid;
    align-content: start;
    gap: 6px;
    margin-right: -6px;
    padding-right: 6px;
  }
  .apps li {
    display: grid;
    grid-template-columns: 180px 1fr 64px;
    align-items: center;
    gap: 14px;
    height: 34px;
    animation: rowin 0.6s var(--spring) both;
    animation-delay: calc(var(--i) * 40ms);
  }
  @keyframes rowin {
    from {
      opacity: 0;
      transform: translateX(-10px);
    }
  }
  .name {
    font-size: var(--t-sm);
    font-weight: 560;
  }
  .track {
    height: 8px;
    border-radius: 8px;
    background: var(--surface-2);
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 100%;
    border-radius: 8px;
    background: var(--text-3);
    transition: width 0.8s var(--ease-out);
  }
  .fill.top {
    background: var(--text);
  }
  .dur {
    text-align: right;
    font-size: var(--t-sm);
    color: var(--text-2);
  }
  .privacy {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 14px;
    flex: none;
    font-size: var(--t-xs);
  }
</style>
