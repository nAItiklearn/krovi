<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { flip } from 'svelte/animate';
  import Icon from '../components/Icon.svelte';
  import RollingText from '../components/RollingText.svelte';
  import State from '../components/State.svelte';
  import { bridge } from '../lib/bridge';
  import { now } from '../lib/stores';
  import { arrive, depart, pop, spotlight } from '../lib/motion';
  import type { WorldClockZone } from '../lib/types';

  const KEY = 'krovi.clocks';
  const LOCAL_TZ = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';

  let zones = $state<WorldClockZone[]>([]);
  let status = $state<'loading' | 'ready' | 'error'>('loading');
  let adding = $state(false);
  let q = $state('');
  let qEl: HTMLInputElement | undefined = $state();

  const allZones: string[] = (() => {
    try {
      return (Intl as unknown as { supportedValuesOf(k: string): string[] }).supportedValuesOf('timeZone');
    } catch {
      return ['UTC', 'Europe/London', 'America/New_York', 'Asia/Kolkata', 'Asia/Tokyo', 'Australia/Sydney'];
    }
  })();
  const city = (tz: string) => (tz.split('/').pop() ?? tz).replace(/_/g, ' ');
  const suggestions = $derived(
    allZones
      .filter((tz) => !zones.some((z) => z.tz === tz))
      .filter((tz) => {
        const n = q.trim().toLowerCase();
        return !n || tz.toLowerCase().replace(/_/g, ' ').includes(n);
      })
      .slice(0, 7),
  );

  function save() {
    try {
      localStorage.setItem(KEY, JSON.stringify(zones));
    } catch {
      /* ignore */
    }
  }
  async function load() {
    status = 'loading';
    try {
      const raw = localStorage.getItem(KEY);
      const base = raw ? (JSON.parse(raw) as WorldClockZone[]) : await bridge.loadWorldClocks();
      zones = base.map((z) => (z.label === 'Local' ? { ...z, tz: LOCAL_TZ } : z));
      status = 'ready';
    } catch {
      status = 'error';
    }
  }

  function add(tz: string) {
    zones = [...zones, { label: city(tz), tz }];
    save();
    adding = false;
    q = '';
  }
  function remove(tz: string) {
    zones = zones.filter((z) => z.tz !== tz);
    save();
  }
  async function toggleAdd() {
    adding = !adding;
    if (adding) {
      await tick();
      qEl?.focus();
    }
  }

  function wall(tz: string, n: number) {
    const f = new Intl.DateTimeFormat('en-US', {
      timeZone: tz,
      hourCycle: 'h23',
      year: 'numeric',
      month: 'numeric',
      day: 'numeric',
      hour: 'numeric',
      minute: 'numeric',
      second: 'numeric',
    });
    const p = Object.fromEntries(f.formatToParts(n).map((x) => [x.type, Number(x.value)]));
    const asUtc = Date.UTC(p.year, p.month - 1, p.day, p.hour % 24, p.minute, p.second);
    return { hour: p.hour % 24, minute: p.minute, offsetMin: Math.round((asUtc - Math.floor(n / 1000) * 1000) / 60000) };
  }

  function info(z: WorldClockZone, n: number) {
    const w = wall(z.tz, n);
    const localOff = -new Date(n).getTimezoneOffset();
    const diff = w.offsetMin - localOff;
    const time = new Intl.DateTimeFormat(undefined, { timeZone: z.tz, hour: 'numeric', minute: '2-digit' }).format(n);
    const date = new Intl.DateTimeFormat(undefined, { timeZone: z.tz, weekday: 'short', day: 'numeric', month: 'short' }).format(n);
    const sign = diff > 0 ? '+' : '-';
    const ah = Math.floor(Math.abs(diff) / 60);
    const am = Math.abs(diff) % 60;
    const rel = diff === 0 ? 'Same time' : `${sign}${ah}h${am ? ` ${am}m` : ''}`;
    return {
      time,
      date,
      rel,
      day: w.hour >= 6 && w.hour < 19,
      hourDeg: ((w.hour % 12) + w.minute / 60) * 30,
      minDeg: w.minute * 6,
    };
  }

  onMount(load);
</script>

<div class="view">
  <div class="toolbar">
    <p class="faint">{zones.length} {zones.length === 1 ? 'place' : 'places'} · times are computed on your device</p>
    <div class="addwrap">
      <button class="btn" aria-expanded={adding} onclick={toggleAdd}><Icon name="plus" size={14} /> Add place</button>
      {#if adding}
        <div class="pop" transition:pop|global={{ duration: 220, y: -6, from: 0.96 }}>
          <label class="find">
            <Icon name="search" size={14} />
            <input bind:this={qEl} bind:value={q} placeholder="Search a city or zone" aria-label="Search time zones" onkeydown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); adding = false; } else if (e.key === 'Enter' && suggestions[0]) add(suggestions[0]); }} />
          </label>
          <ul>
            {#each suggestions as tz (tz)}
              <li><button onclick={() => add(tz)}><span>{city(tz)}</span><span class="faint">{tz.split('/')[0].replace(/_/g, ' ')}</span></button></li>
            {:else}
              <li class="faint none">No matching zone</li>
            {/each}
          </ul>
        </div>
      {/if}
    </div>
  </div>

  {#if status === 'loading'}
    <div class="clocks">
      {#each Array(6) as _, i (i)}<div class="skel" style="height:96px;border-radius:20px"></div>{/each}
    </div>
  {:else if status === 'error'}
    <State tone="error" title={'Couldn’t load your clocks'} text="Your saved places could not be read." action={{ label: 'Try again', run: load }} />
  {:else if zones.length === 0}
    <State icon="clocks" title="No places yet" text="Add a city to keep an eye on its local time." action={{ label: 'Add a place', run: toggleAdd }} />
  {:else}
    <ul class="clocks scroll">
      {#each zones as z (z.tz)}
        {@const i = info(z, $now)}
        <li animate:flip={{ duration: 380 }} in:arrive out:depart>
          <div class="clock card" class:night={!i.day} use:spotlight>
            <svg class="face" viewBox="0 0 60 60" aria-hidden="true">
              <circle cx="30" cy="30" r="28" class="rim" />
              {#each Array(12) as _, t (t)}
                <line x1="30" y1="5" x2="30" y2={t % 3 === 0 ? 9 : 7} class="tick" transform="rotate({t * 30} 30 30)" />
              {/each}
              <line x1="30" y1="30" x2="30" y2="17" class="hand h" style:transform="rotate({i.hourDeg}deg)" />
              <line x1="30" y1="30" x2="30" y2="10" class="hand m" style:transform="rotate({i.minDeg}deg)" />
              <circle cx="30" cy="30" r="2" class="pin" />
            </svg>
            <div class="meta">
              <div class="lbl ellipsis">
                {z.label}
                <Icon name={i.day ? 'sun' : 'moon'} size={13} />
              </div>
              <div class="sub faint ellipsis">{i.date} · {z.tz === LOCAL_TZ ? 'Your time' : i.rel}</div>
            </div>
            <div class="time num"><RollingText text={i.time} /></div>
            {#if z.tz !== LOCAL_TZ}
              <button class="x" aria-label={`Remove ${z.label}`} onclick={() => remove(z.tz)}><Icon name="close" size={13} /></button>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 14px;
    font-size: var(--t-sm);
    flex: none;
  }
  .addwrap {
    position: relative;
  }
  .pop {
    position: absolute;
    z-index: 20;
    right: 0;
    top: calc(100% + 8px);
    width: 270px;
    padding: 6px;
    border-radius: var(--r-3);
    background: var(--pop-bg);
    border: 1px solid var(--line-strong);
    box-shadow: var(--shadow-pop);
    -webkit-backdrop-filter: blur(24px) saturate(1.6);
    backdrop-filter: blur(24px) saturate(1.6);
    transform-origin: 90% 0;
  }
  .find {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 10px;
    color: var(--text-3);
    border-bottom: 1px solid var(--line);
    margin-bottom: 4px;
  }
  .find input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: none;
    outline: none;
    color: var(--text);
    font-size: var(--t-sm);
    font-weight: 500;
  }
  .pop li button {
    display: flex;
    justify-content: space-between;
    width: 100%;
    height: 32px;
    align-items: center;
    padding: 0 10px;
    border-radius: var(--r-2);
    font-size: var(--t-sm);
    font-weight: 560;
    text-align: left;
  }
  .pop li button:hover,
  .pop li button:focus-visible {
    background: var(--surface-2);
  }
  .pop .faint {
    font-size: var(--t-xs);
    font-weight: 500;
  }
  .none {
    padding: 10px;
    font-size: var(--t-sm);
  }

  .clocks {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    grid-auto-rows: 96px;
    gap: 12px;
    align-content: start;
    padding-right: 4px;
  }
  .clock {
    height: 100%;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 0 16px 0 14px;
    border-radius: var(--r-4);
  }
  .clock.night {
    background: color-mix(in srgb, var(--surface-1) 55%, var(--surface-3));
  }
  .face {
    width: 58px;
    height: 58px;
    flex: none;
  }
  .face .rim {
    fill: var(--surface-1);
    stroke: var(--line-strong);
    stroke-width: 1;
  }
  .face .tick {
    stroke: var(--text-3);
    stroke-width: 1.4;
    stroke-linecap: round;
  }
  .face .hand {
    stroke: var(--text);
    stroke-linecap: round;
    transform-origin: 30px 30px;
    transition: transform 1s var(--ease-out);
  }
  .face .hand.h {
    stroke-width: 2.6;
  }
  .face .hand.m {
    stroke-width: 1.8;
  }
  .face .pin {
    fill: var(--accent);
  }
  .meta {
    flex: 1;
    min-width: 0;
  }
  .lbl {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--t-md);
    font-weight: 650;
    letter-spacing: -0.01em;
  }
  .lbl :global(.ic) {
    color: var(--text-3);
  }
  .sub {
    font-size: var(--t-xs);
    margin-top: 2px;
  }
  .time {
    font-size: 26px;
    font-weight: 600;
    letter-spacing: -0.035em;
    white-space: nowrap;
  }
  .x {
    position: absolute;
    top: 8px;
    right: 8px;
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 7px;
    color: var(--text-3);
    opacity: 0;
    transition:
      opacity var(--dur-1),
      background var(--dur-1),
      color var(--dur-1);
  }
  .clock:hover .x,
  .x:focus-visible {
    opacity: 1;
  }
  .x:hover {
    color: var(--danger);
    background: var(--surface-3);
  }
</style>
