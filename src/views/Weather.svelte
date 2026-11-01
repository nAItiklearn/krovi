<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import Segmented from '../components/Segmented.svelte';
  import State from '../components/State.svelte';
  import PlaceSearch from '../components/PlaceSearch.svelte';
  import AnimatedNumber from '../components/AnimatedNumber.svelte';
  import { bridge } from '../lib/bridge';
  import { now } from '../lib/stores';
  import { relativeTime } from '../lib/time';
  import { pop, spotlight } from '../lib/motion';
  import type { Place, WeatherReply } from '../lib/types';

  let reply = $state<WeatherReply | null>(null);
  let status = $state<'loading' | 'ready' | 'error'>('loading');
  let unit = $state<'C' | 'F'>('C');
  let changing = $state(false);
  let picking = $state(false);
  let pickError = $state('');

  const w = $derived(reply?.weather ?? null);
  const conv = (c: number) => (unit === 'C' ? Math.round(c) : Math.round((c * 9) / 5 + 32));

  async function load(force = false) {
    status = 'loading';
    try {
      reply = await bridge.loadWeatherStatus(force);
      status = 'ready';
    } catch {
      status = 'error';
    }
  }
  onMount(() => void load());

  async function pick(p: Place) {
    picking = true;
    pickError = '';
    try {
      reply = await bridge.setWeatherPlace(p);
      changing = false;
      if (reply.error && !reply.weather) pickError = reply.error;
    } catch (e) {
      pickError = e instanceof Error ? e.message : String(e);
    } finally {
      picking = false;
    }
  }

  const span = $derived(
    w ? { lo: Math.min(...w.forecast.map((d) => d.low)), hi: Math.max(...w.forecast.map((d) => d.high)) } : { lo: 0, hi: 1 },
  );
  const pos = (t: number) => ((t - span.lo) / Math.max(1, span.hi - span.lo)) * 100;
  const needsCity = $derived(status === 'ready' && (reply?.status === 'needs-city' || (!w && reply?.status === 'ok')));
</script>

<div class="view">
  {#if status === 'loading' && !reply}
    <div class="grid">
      <div class="skel" style="height:100%;min-height:300px;border-radius:20px"></div>
      <div class="col">
        <div class="skel" style="height:132px;border-radius:20px"></div>
        <div class="skel" style="flex:1;border-radius:20px"></div>
      </div>
    </div>
  {:else if status === 'error'}
    <State
      tone="error"
      title={'Couldn’t get the weather'}
      text="The weather service did not answer. Check your connection and try again."
      action={{ label: 'Try again', run: () => load(true) }} />
  {:else if needsCity}
    <div class="setup">
      <div class="glyph"><Icon name="mapPin" size={22} /></div>
      <h2>Where are you?</h2>
      <p>
        Krovi doesn&rsquo;t track your location. Type your city once; only that text goes to the geocoder, and the
        forecast request uses a rounded position.
      </p>
      <div class="picker"><PlaceSearch onpick={pick} busy={picking} /></div>
      {#if pickError}<p class="perr" role="alert">{pickError}</p>{/if}
    </div>
  {:else if reply && w}
    <div class="top">
      <div class="placewrap">
        <button class="place" onclick={() => (changing = !changing)} aria-expanded={changing} aria-label={`Weather for ${w.city}. Change city`}>
          <Icon name="mapPin" size={14} />
          <span class="ellipsis">{w.city}</span>
          <Icon name="chevronDown" size={13} />
        </button>
        {#if changing}
          <div class="pop" transition:pop|global={{ duration: 220, y: -6, from: 0.96 }}>
            <PlaceSearch onpick={pick} busy={picking} />
            {#if pickError}<p class="perr" role="alert">{pickError}</p>{/if}
          </div>
        {/if}
      </div>
      <div class="right">
        {#if reply.status === 'offline' || reply.stale}
          <span class="tag" title={reply.error ?? 'Showing the last forecast we have'}>
            <Icon name="refresh" size={11} />
            {reply.status === 'offline' ? 'Offline' : 'Stale'}{reply.fetchedAt ? ` · ${relativeTime(reply.fetchedAt, $now)}` : ''}
          </span>
        {/if}
        <button class="btn icon sm ghost" aria-label="Refresh weather" onclick={() => load(true)} disabled={status === 'loading'}>
          <Icon name="refresh" size={14} />
        </button>
        <Segmented label="Temperature unit" size="sm" value={unit} onchange={(v) => (unit = v)} options={[{ id: 'C', label: '°C' }, { id: 'F', label: '°F' }]} />
      </div>
    </div>

    <div class="grid">
      <section class="card now" use:spotlight aria-label="Current conditions">
        <div class="big"><Icon name={w.icon} size={54} /></div>
        <div class="temp num"><AnimatedNumber value={conv(w.current)} /><span>°</span></div>
        <div class="cond">{w.condition}</div>
        <div class="hl faint">H {conv(w.high)}° · L {conv(w.low)}° · Feels {conv(w.feelsLike)}°</div>
        <div class="stats">
          <div><Icon name="drop" size={15} /><strong>{w.humidity}%</strong><small>Humidity</small></div>
          <div><Icon name="wind" size={15} /><strong>{w.windKph} km/h</strong><small>Wind</small></div>
          <div><Icon name="sun" size={15} /><strong>{w.uvIndex}</strong><small>UV index</small></div>
        </div>
      </section>

      <div class="col">
        <section class="card pad" use:spotlight aria-label="Hourly">
          <p class="eyebrow">Hourly</p>
          <div class="hours">
            {#each w.hourly as h (h.time)}
              <div class="hour">
                <span class="faint ht">{h.time}</span>
                <Icon name={h.icon} size={20} />
                <strong class="num">{conv(h.temp)}°</strong>
              </div>
            {/each}
          </div>
        </section>

        <section class="card pad fc" use:spotlight aria-label="Forecast">
          <p class="eyebrow">{w.forecast.length}-day forecast</p>
          <ul>
            {#each w.forecast as d (d.label)}
              <li>
                <span class="fday">{d.label}</span>
                <Icon name={d.icon} size={18} />
                <span class="flo faint num">{conv(d.low)}°</span>
                <span class="track"><span class="range" style:left="{pos(d.low)}%" style:right="{100 - pos(d.high)}%"></span></span>
                <span class="fhi num">{conv(d.high)}°</span>
              </li>
            {/each}
          </ul>
        </section>
      </div>
    </div>
    <p class="note faint">Only the city you typed and a rounded position are sent to keyless weather services. Nothing else leaves this device.</p>
  {:else}
    <State
      tone="error"
      title={reply?.status === 'offline' ? 'You are offline' : 'No forecast yet'}
      text={reply?.error ?? 'There is no saved forecast to show. Connect once and it will be cached.'}
      action={{ label: 'Try again', run: () => load(true) }} />
  {/if}
</div>

<style>
  .setup {
    flex: 1;
    display: grid;
    place-items: center;
    align-content: center;
    gap: 8px;
    text-align: center;
    padding-bottom: 40px;
  }
  .setup .glyph {
    display: grid;
    place-items: center;
    width: 52px;
    height: 52px;
    margin-bottom: 6px;
    border-radius: var(--r-3);
    background: var(--surface-1);
    border: 1px solid var(--line);
    color: var(--text-2);
  }
  .setup h2 {
    font-size: var(--t-xl);
    font-weight: 650;
    letter-spacing: -0.02em;
  }
  .setup p {
    max-width: 44ch;
    color: var(--text-2);
    font-size: var(--t-sm);
    line-height: 1.5;
  }
  .picker {
    width: min(420px, 100%);
    margin-top: 12px;
    text-align: left;
  }
  .perr {
    color: var(--danger) !important;
  }
  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 14px;
    flex: none;
  }
  .placewrap {
    position: relative;
  }
  .place {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    max-width: 320px;
    height: 32px;
    padding: 0 12px 0 10px;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: var(--surface-1);
    color: var(--text);
    font-size: var(--t-sm);
    font-weight: 650;
    transition:
      background var(--dur-1),
      border-color var(--dur-1);
  }
  .place:hover {
    background: var(--surface-2);
    border-color: var(--line-strong);
  }
  .place :global(.ic) {
    color: var(--text-3);
  }
  .pop {
    position: absolute;
    z-index: 20;
    left: 0;
    top: calc(100% + 8px);
    width: 340px;
    padding: 8px;
    border-radius: var(--r-3);
    background: var(--pop-bg);
    border: 1px solid var(--line-strong);
    box-shadow: var(--shadow-pop);
    transform-origin: 10% 0;
  }
  .right {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .grid {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 264px 1fr;
    gap: 14px;
  }
  .col {
    display: flex;
    flex-direction: column;
    gap: 14px;
    min-height: 0;
  }
  .pad {
    padding: 14px 16px;
  }
  .now {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 20px 16px;
  }
  .big {
    color: var(--text-2);
    margin-bottom: 6px;
  }
  .temp {
    font-size: 76px;
    font-weight: 500;
    letter-spacing: -0.06em;
    line-height: 1;
    display: flex;
  }
  .temp span {
    color: var(--text-3);
    margin-left: 2px;
  }
  .cond {
    margin-top: 8px;
    font-size: var(--t-lg);
    font-weight: 600;
  }
  .hl {
    font-size: var(--t-xs);
    margin-top: 3px;
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    width: 100%;
    margin-top: 20px;
    padding-top: 16px;
    border-top: 1px solid var(--line);
  }
  .stats div {
    display: grid;
    justify-items: center;
    gap: 3px;
    color: var(--text-2);
  }
  .stats strong {
    font-size: var(--t-sm);
    color: var(--text);
    font-weight: 650;
  }
  .stats small {
    font-size: var(--t-micro);
    color: var(--text-3);
  }
  .hours {
    display: flex;
    justify-content: space-between;
    margin-top: 12px;
  }
  .hour {
    display: grid;
    justify-items: center;
    gap: 8px;
    color: var(--text-2);
  }
  .ht {
    font-size: var(--t-xs);
  }
  .hour strong {
    font-size: var(--t-md);
    color: var(--text);
    font-weight: 600;
  }
  .fc {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .fc ul {
    flex: 1;
    display: grid;
    align-content: space-evenly;
    margin-top: 6px;
  }
  .fc li {
    display: grid;
    grid-template-columns: 52px 20px 34px 1fr 34px;
    align-items: center;
    gap: 12px;
    color: var(--text-2);
  }
  .fday {
    font-size: var(--t-sm);
    font-weight: 600;
    color: var(--text);
  }
  .flo,
  .fhi {
    font-size: var(--t-sm);
  }
  .flo {
    text-align: right;
  }
  .fhi {
    color: var(--text);
    font-weight: 600;
  }
  .track {
    position: relative;
    height: 5px;
    border-radius: 5px;
    background: var(--surface-3);
  }
  .range {
    position: absolute;
    top: 0;
    bottom: 0;
    border-radius: 5px;
    background: linear-gradient(90deg, var(--text-3), var(--text));
  }
  .note {
    margin-top: 10px;
    font-size: var(--t-xs);
    flex: none;
  }
</style>
