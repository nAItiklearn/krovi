<script lang="ts">
  import { onMount } from 'svelte';
  import { bridge } from '../lib/bridge';
  import Icon from '../components/Icon.svelte';
  import type { WeatherData, WeatherIcon } from '../lib/types';

  let w = $state<WeatherData | null>(null);
  let unit = $state<'C' | 'F'>('C');

  function conv(c: number): number {
    return unit === 'C' ? c : Math.round((c * 9) / 5 + 32);
  }
  const iconFor: Record<WeatherIcon, string> = {
    sun: 'sun',
    cloud: 'cloud',
    partly: 'partly',
    rain: 'rain',
    snow: 'snow',
  };

  onMount(async () => {
    w = await bridge.loadWeather();
  });
</script>

{#if w}
  <div class="top">
    <div class="loc">
      <span class="tag">Weather</span>
      <strong>{w.city}</strong>
    </div>
    <div class="seg">
      <button aria-pressed={unit === 'C'} onclick={() => (unit = 'C')}>°C</button>
      <button aria-pressed={unit === 'F'} onclick={() => (unit = 'F')}>°F</button>
    </div>
  </div>

  <div class="grid">
    <section class="card now">
      <div class="nowicon"><Icon name={iconFor[w.icon]} size={56} /></div>
      <div class="temp">{conv(w.current)}°</div>
      <div class="cond">{w.condition}</div>
      <div class="hl faint">H {conv(w.high)}° · L {conv(w.low)}° · Feels {conv(w.feelsLike)}°</div>
      <div class="stats">
        <div><Icon name="drop" size={15} /><span>{w.humidity}%</span><small class="faint">Humidity</small></div>
        <div><Icon name="wind" size={15} /><span>{w.windKph} km/h</span><small class="faint">Wind</small></div>
        <div><Icon name="sun" size={15} /><span>{w.uvIndex}</span><small class="faint">UV index</small></div>
      </div>
    </section>

    <section class="card hourly">
      <p class="eyebrow">Hourly</p>
      <div class="hours">
        {#each w.hourly as h (h.time)}
          <div class="hour">
            <span class="ht faint">{h.time}</span>
            <Icon name={iconFor[h.icon]} size={20} />
            <span class="htemp">{conv(h.temp)}°</span>
          </div>
        {/each}
      </div>
    </section>

    <section class="card fc">
      <p class="eyebrow">5-day</p>
      {#each w.forecast as d (d.label)}
        <div class="frow">
          <span class="fday">{d.label}</span>
          <Icon name={iconFor[d.icon]} size={18} />
          <span class="flo faint">{conv(d.low)}°</span>
          <span class="track"><span class="range" style={`left:${((d.low - 18) / 20) * 100}%;right:${100 - ((d.high - 18) / 20) * 100}%`}></span></span>
          <span class="fhi">{conv(d.high)}°</span>
        </div>
      {/each}
    </section>
  </div>
  <p class="note faint">Location is only ever a coarse lat/long, sent to a keyless public API. Nothing else leaves your device (SPEC §7).</p>
{/if}

<style>
  .top {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 14px;
  }
  .loc {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 17px;
  }
  .grid {
    display: grid;
    grid-template-columns: 240px 1fr;
    grid-template-rows: auto auto;
    gap: 14px;
  }
  .now {
    grid-row: span 2;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 4px;
  }
  .nowicon {
    color: var(--accent);
  }
  .temp {
    font-size: 54px;
    font-weight: 700;
    letter-spacing: -0.03em;
  }
  .cond {
    font-size: 14px;
  }
  .hl {
    font-size: 12.5px;
  }
  .stats {
    display: flex;
    gap: 16px;
    margin-top: 14px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
    width: 100%;
    justify-content: center;
  }
  .stats div {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    color: var(--text-dim);
  }
  .stats span {
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
  }
  .stats small {
    font-size: 10.5px;
  }
  .hours {
    display: flex;
    justify-content: space-between;
    gap: 6px;
  }
  .hour {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    color: var(--accent);
  }
  .ht {
    font-size: 11.5px;
  }
  .htemp {
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
  }
  .fc {
    display: flex;
    flex-direction: column;
    gap: 9px;
  }
  .frow {
    display: grid;
    grid-template-columns: 42px 20px 28px 1fr 28px;
    align-items: center;
    gap: 10px;
    color: var(--accent);
  }
  .fday {
    font-size: 13px;
    color: var(--text);
    font-weight: 550;
  }
  .flo,
  .fhi {
    font-size: 13px;
  }
  .fhi {
    color: var(--text);
    text-align: right;
  }
  .track {
    position: relative;
    height: 5px;
    background: var(--surface-3);
    border-radius: 999px;
  }
  .range {
    position: absolute;
    top: 0;
    bottom: 0;
    background: linear-gradient(90deg, var(--accent), var(--warn));
    border-radius: 999px;
  }
  .note {
    font-size: 11.5px;
    margin-top: 14px;
  }
  @media (max-width: 680px) {
    .grid {
      grid-template-columns: 1fr;
    }
    .now {
      grid-row: auto;
    }
  }
</style>
