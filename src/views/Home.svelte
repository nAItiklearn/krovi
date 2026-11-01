<script lang="ts">
  import { onMount } from 'svelte';
  import { flip } from 'svelte/animate';
  import Icon from '../components/Icon.svelte';
  import Ring from '../components/Ring.svelte';
  import AnimatedNumber from '../components/AnimatedNumber.svelte';
  import RollingText from '../components/RollingText.svelte';
  import Equalizer from '../components/Equalizer.svelte';
  import State from '../components/State.svelte';
  import { clockText, clipLabel, diskPercent, freeDisk, memPercent } from '../components/pillModel';
  import { nextEvent, relativeDay, toISO, upcomingHoliday, type AgendaItem } from '../components/agenda';
  import { clips, copyClip, media, mediaPosition, mediaTransport, stats } from '../lib/live';
  import { bridge } from '../lib/bridge';
  import { REGIONS } from '../lib/holidays';
  import {
    activeTool,
    calendarTick,
    now,
    onNative,
    pomoLabel,
    pomoRemaining,
    pomodoro,
    pomodoroStart,
    pushToast,
    region,
    timerDone,
    timerProgress,
    timerRemaining,
    timers,
  } from '../lib/stores';
  import { relativeTime } from '../lib/time';
  import { arrive, depart, spotlight } from '../lib/motion';
  import type { IconName } from '../lib/icons';
  import type { ToolId, WeatherData } from '../lib/types';

  // ---- live system gauges ----
  const ram = $derived(memPercent($stats));
  const disk = $derived(diskPercent($stats));
  const free = $derived(freeDisk($stats));
  const gb = (b: number) => (b / 1024 ** 3).toFixed(b >= 10 * 1024 ** 3 ? 0 : 1);

  // ---- now playing (position is interpolated locally between 5s resyncs) ----
  const m = $derived($media);
  const pos = $derived(mediaPosition(m, $now));
  const pct = $derived(m && m.durationSec > 0 ? Math.min(100, (pos / m.durationSec) * 100) : 0);
  const fmt = (s: number) => {
    const t = Math.max(0, Math.floor(s));
    return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`;
  };
  const appName = (s: string) => s.replace(/\.exe$/i, '').replace(/^\w/, (c) => c.toUpperCase());

  // ---- clips ----
  const recent = $derived([...$clips].sort((a, b) => b.created_at - a.created_at).slice(0, 3));
  async function recopy(id: number) {
    await copyClip(id);
    pushToast('Copied to clipboard');
  }
  const kindIcon = (k: string): IconName => (k === 'Link' ? 'link' : k === 'Image' ? 'image' : 'text');

  // ---- weather ----
  let weather = $state<WeatherData | null>(null);
  // 'none' = no city set yet (or offline with nothing cached): the widget invites you to set one
  let wState = $state<'loading' | 'ready' | 'none' | 'error'>('loading');
  async function loadWeather() {
    wState = 'loading';
    try {
      weather = await bridge.loadWeather();
      wState = weather ? 'ready' : 'none';
    } catch {
      wState = 'error';
    }
  }

  // ---- agenda ----
  let events = $state<AgendaItem[]>([]);
  const todayISO = $derived(toISO(new Date($now)));
  const upNext = $derived(nextEvent(events, $now));
  const holiday = $derived(upcomingHoliday(todayISO, $region));
  const regionName = $derived(REGIONS.find((r) => r.code === $region)?.name ?? $region);

  // ---- timers ----
  const running = $derived(
    $timers.filter((t) => t.runningSince != null && !timerDone(t, $now)).slice(0, 2),
  );
  const pomoOn = $derived($pomodoro.runningSince != null && !$pomodoro.finished);

  onMount(() => {
    void loadWeather();
    bridge
      .loadCalendar()
      .then((e) => (events = e))
      .catch(() => (events = []));
    // a reminder the backend saved (voice): the event carries the whole, ordered list
    let off: (() => void) | undefined;
    let dead = false;
    void onNative<AgendaItem[]>('calendar-changed', (l) => (events = l)).then((u) => {
      if (dead) u();
      else off = u;
    });
    return () => {
      dead = true;
      off?.();
    };
  });

  // a reminder added by voice while Home is open
  let seenTick = $calendarTick;
  $effect(() => {
    const t = $calendarTick;
    if (t === seenTick) return;
    seenTick = t;
    bridge
      .loadCalendar()
      .then((e) => (events = e))
      .catch(() => {});
  });

  const go = (id: ToolId) => activeTool.set(id);
</script>

<div class="view home">
  <!-- NOW PLAYING -->
  <div class="enter media" style:--i="3">
    <section class="card pad" use:spotlight aria-label="Now playing">
      <button class="wh" onclick={() => go('media')}>
        <span class="eyebrow"><Icon name="media" size={13} /> Now playing{m ? ` · ${appName(m.sourceApp)}` : ''}</span>
        <Icon name="chevronRight" size={14} />
      </button>
      {#if m}
        <div class="np">
          <div class="art" class:spin={m.playing}>
            {#if m.artwork}
              <img src={m.artwork} alt="" />
            {:else}
              <Icon name="disc" size={34} />
            {/if}
          </div>
          <div class="meta">
            <h2 class="ellipsis" title={m.title}>{m.title}</h2>
            <p class="ellipsis dim">{m.artist || 'Unknown artist'}</p>
            {#if m.album}<p class="ellipsis faint small">{m.album}</p>{/if}
          </div>
        </div>
        <div class="prog">
          <span class="t num faint">{fmt(pos)}</span>
          <div class="bar" role="progressbar" aria-label="Track position" aria-valuemin="0" aria-valuemax={Math.round(m.durationSec)} aria-valuenow={Math.round(pos)}>
            <i style:width="{pct}%"></i>
          </div>
          <span class="t num faint">{fmt(m.durationSec)}</span>
        </div>
        <div class="ctl">
          <button class="btn-round" aria-label="Previous track" onclick={() => mediaTransport('previous')}><Icon name="prev" size={18} /></button>
          <button class="btn-round big" aria-label={m.playing ? 'Pause' : 'Play'} onclick={() => mediaTransport('playpause')}>
            <Icon name={m.playing ? 'pause' : 'play'} size={20} />
          </button>
          <button class="btn-round" aria-label="Next track" onclick={() => mediaTransport('next')}><Icon name="next" size={18} /></button>
          {#if m.playing}<span class="eqpos"><Equalizer height={16} /></span>{/if}
        </div>
      {:else}
        <State
          compact
          icon="headphones"
          title="Nothing playing"
          text="Start music or a video in any app and it shows up here with controls." />
      {/if}
    </section>
  </div>

  <!-- SYSTEM -->
  <div class="enter system" style:--i="4">
    <section class="card pad" use:spotlight aria-label="System">
      <div class="wh static"><span class="eyebrow"><Icon name="activity" size={13} /> System</span></div>
      <div class="gauges">
        {#if $stats}
          <div class="gauge">
            <Ring value={(ram ?? 0) / 100} size={78} stroke={6} tone={(ram ?? 0) >= 85 ? 'accent' : 'default'} label="Memory used">
              <span class="gv"><AnimatedNumber value={ram ?? 0} /><small>%</small></span>
            </Ring>
            <span class="gl">Memory</span>
            <span class="gs faint">{gb($stats.memUsed)} of {gb($stats.memTotal)} GB</span>
          </div>
          <div class="gauge">
            <Ring value={(disk ?? 0) / 100} size={78} stroke={6} tone={(disk ?? 0) >= 90 ? 'accent' : 'default'} label="Disk used">
              <span class="gv"><AnimatedNumber value={disk ?? 0} /><small>%</small></span>
            </Ring>
            <span class="gl">Disk {$stats.diskLabel}</span>
            <span class="gs faint">{free?.text ?? '—'} free</span>
          </div>
          <div class="gauge">
            <Ring value={$stats.cpu / 100} size={78} stroke={6} tone={$stats.cpu >= 85 ? 'accent' : 'default'} label="Processor load">
              <span class="gv"><AnimatedNumber value={$stats.cpu} /><small>%</small></span>
            </Ring>
            <span class="gl">Processor</span>
            <span class="gs faint">Load now</span>
          </div>
        {:else}
          {#each [0, 1, 2] as i (i)}
            <div class="gauge">
              <div class="skel" style="width:78px;height:78px;border-radius:50%"></div>
              <div class="skel" style="width:54px;height:10px"></div>
              <div class="skel" style="width:72px;height:9px"></div>
            </div>
          {/each}
        {/if}
      </div>
    </section>
  </div>

  <!-- WEATHER -->
  <div class="enter weather" style:--i="5">
    {#if wState === 'error'}
      <section class="card pad" use:spotlight aria-label="Weather">
        <div class="wh static"><span class="eyebrow"><Icon name="weather" size={13} /> Weather</span></div>
        <p class="dim small err">Weather is unavailable right now.</p>
        <button class="btn sm retry" onclick={loadWeather}><Icon name="refresh" size={13} /> Try again</button>
      </section>
    {:else}
      <button class="card pad" onclick={() => go('weather')} use:spotlight>
        <div class="wh static"><span class="eyebrow"><Icon name="weather" size={13} /> Weather</span></div>
        {#if wState === 'ready' && weather}
          <div class="wx">
            <span class="wt num"><AnimatedNumber value={weather.current} />°</span>
            <span class="wi"><Icon name={weather.icon} size={26} /></span>
          </div>
          <p class="ellipsis dim small">{weather.condition}</p>
          <p class="ellipsis faint small">{weather.city} · {weather.high}° / {weather.low}°</p>
        {:else if wState === 'none'}
          <p class="wx-none">Set your city</p>
          <p class="faint small">Get the forecast right here.</p>
        {:else}
          <div class="skel" style="width:90px;height:38px;margin:6px 0 8px"></div>
          <div class="skel" style="width:110px;height:10px"></div>
        {/if}
      </button>
    {/if}
  </div>
  <!-- TIMERS -->
  <div class="enter timersw" style:--i="6">
    {#if pomoOn || running.length}
      <button class="card pad" onclick={() => go(pomoOn ? 'pomodoro' : 'timers')} use:spotlight>
        <div class="wh static"><span class="eyebrow"><Icon name="timers" size={13} /> Running</span></div>
        <ul class="tl">
          {#if pomoOn}
            <li>
              <Ring value={1 - pomoRemaining($pomodoro, $now) / $pomodoro.duration} size={30} stroke={3.5} tone="accent" />
              <span class="tn ellipsis">{pomoLabel($pomodoro.phase)}</span>
              <span class="tc"><RollingText text={clockText(pomoRemaining($pomodoro, $now))} /></span>
            </li>
          {/if}
          {#each running.slice(0, pomoOn ? 1 : 2) as t (t.id)}
            <li>
              <Ring value={timerProgress(t, $now)} size={30} stroke={3.5} tone="accent" />
              <span class="tn ellipsis">{t.label}</span>
              <span class="tc"><RollingText text={clockText(timerRemaining(t, $now))} /></span>
            </li>
          {/each}
        </ul>
      </button>
    {:else}
      <section class="card pad" use:spotlight aria-label="Timers">
        <div class="wh static"><span class="eyebrow"><Icon name="timers" size={13} /> Timers</span></div>
        <p class="dim small idle">Nothing running</p>
        <button class="btn sm retry" onclick={() => { pomodoroStart(); go('pomodoro'); }}>
          <Icon name="play" size={12} /> Focus
        </button>
      </section>
    {/if}
  </div>
  <!-- CLIPS -->
  <div class="enter clipsw" style:--i="7">
    <section class="card pad" use:spotlight aria-label="Latest clips">
      <button class="wh" onclick={() => go('clipboard')}>
        <span class="eyebrow"><Icon name="clipboard" size={13} /> Latest clips</span>
        <Icon name="chevronRight" size={14} />
      </button>
      {#if recent.length}
        <ul class="cl">
          {#each recent as c (c.id)}
            <li animate:flip={{ duration: 380 }} in:arrive out:depart>
              <button class="clip" onclick={() => recopy(c.id)} title="Click to copy again">
                <span class="ki"><Icon name={kindIcon(c.kind)} size={14} /></span>
                <span class="ct ellipsis">{clipLabel(c)}</span>
                {#if c.pinned}<Icon name="pin" size={12} />{/if}
                <span class="cw faint">{relativeTime(c.created_at, $now)}</span>
              </button>
            </li>
          {/each}
        </ul>
      {:else}
        <State compact icon="clipboard" title="Nothing copied yet" text="Copy anything, anywhere, and it lands here. Stays on this device." />
      {/if}
    </section>
  </div>

  <!-- NEXT UP -->
  <div class="enter upw" style:--i="8">
    <button class="card pad" onclick={() => go('calendar')} use:spotlight>
      <div class="wh static"><span class="eyebrow"><Icon name="calendar" size={13} /> Next up</span></div>
      <div class="up">
        <span class="ui"><Icon name="clock" size={15} /></span>
        <div class="ut">
          {#if upNext}
            <strong class="ellipsis">{upNext.title}</strong>
            <span class="faint small">{relativeDay(upNext.date, todayISO)}{upNext.time ? `, ${upNext.time}` : ''}</span>
          {:else}
            <strong class="dim">Nothing scheduled</strong>
            <span class="faint small">Add a reminder in Calendar</span>
          {/if}
        </div>
      </div>
      <div class="up">
        <span class="ui hol"><Icon name="sparkles" size={15} /></span>
        <div class="ut">
          {#if holiday}
            <strong class="ellipsis">{holiday.name}</strong>
            <span class="faint small">{relativeDay(holiday.date, todayISO)} · {regionName}</span>
          {:else}
            <strong class="dim">No upcoming holiday</strong>
            <span class="faint small">{regionName}</span>
          {/if}
        </div>
      </div>
    </button>
  </div>
</div>

<style>
  .home {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    grid-template-rows: 176px 150px 172px;
    gap: 12px;
    align-content: start;
    overflow: hidden;
  }
  .home > :global(div) {
    min-height: 0;
    min-width: 0;
  }
  .media {
    grid-column: 1 / 3;
    grid-row: 1 / 3;
  }
  .system {
    grid-column: 3 / 5;
    grid-row: 1;
  }
  .weather {
    grid-column: 3;
    grid-row: 2;
  }
  .timersw {
    grid-column: 4;
    grid-row: 2;
  }
  .clipsw {
    grid-column: 1 / 3;
    grid-row: 3;
  }
  .upw {
    grid-column: 3 / 5;
    grid-row: 3;
  }
  .home :global(.card) {
    height: 100%;
  }
  .pad {
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .small {
    font-size: var(--t-xs);
  }

  /* widget header */
  .wh {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    color: var(--text-3);
    margin-bottom: 8px;
    flex: none;
  }
  .wh :global(.ic:last-child) {
    opacity: 0;
    transform: translateX(-4px);
    transition:
      opacity var(--dur-2),
      transform 0.4s var(--spring);
  }
  .card:hover .wh :global(.ic:last-child) {
    opacity: 1;
    transform: none;
  }
  .wh.static {
    pointer-events: none;
  }
  .wh:hover {
    color: var(--text);
  }

  /* now playing */
  .np {
    display: flex;
    align-items: center;
    gap: 18px;
    margin: 6px 0 16px;
  }
  .art {
    position: relative;
    display: grid;
    place-items: center;
    flex: none;
    width: 128px;
    height: 128px;
    border-radius: 18px;
    overflow: hidden;
    color: var(--text-3);
    background:
      radial-gradient(circle at 30% 20%, var(--surface-3), transparent 70%),
      linear-gradient(145deg, var(--surface-2), var(--surface-1));
    border: 1px solid var(--line);
    box-shadow:
      var(--shadow-art),
      inset 0 1px 0 var(--card-hi);
  }
  .art img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .meta {
    min-width: 0;
    flex: 1;
  }
  .meta h2 {
    font-family: var(--font-display);
    font-size: var(--t-xl);
    font-weight: 650;
    letter-spacing: -0.02em;
    line-height: 1.2;
  }
  .meta p {
    margin-top: 2px;
    font-size: var(--t-sm);
  }
  .prog {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .t {
    font-size: var(--t-xs);
    min-width: 32px;
  }
  .t:last-child {
    text-align: right;
  }
  .bar {
    flex: 1;
    height: 4px;
    border-radius: 4px;
    background: var(--surface-3);
    overflow: hidden;
  }
  .bar i {
    display: block;
    height: 100%;
    border-radius: 4px;
    background: var(--text);
    transition: width 1s linear;
  }
  .ctl {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 14px;
    margin-top: auto;
  }
  .eqpos {
    position: absolute;
    right: 2px;
    bottom: 12px;
  }

  /* system gauges */
  .gauges {
    flex: 1;
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    align-items: center;
    justify-items: center;
  }
  .gauge {
    display: grid;
    justify-items: center;
    gap: 3px;
  }
  .gv {
    font-family: var(--font-display);
    font-size: var(--t-xl);
    font-weight: 650;
    letter-spacing: -0.03em;
    font-variant-numeric: tabular-nums;
  }
  .gv small {
    font-size: var(--t-xs);
    font-weight: 600;
    color: var(--text-3);
    margin-left: 1px;
  }
  .gl {
    margin-top: 5px;
    font-size: var(--t-sm);
    font-weight: 600;
  }
  .gs {
    font-size: var(--t-xs);
  }

  /* weather */
  .wx {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 2px 0 2px;
  }
  .wt {
    font-size: 40px;
    font-weight: 600;
    letter-spacing: -0.04em;
    line-height: 1.05;
  }
  .wi {
    color: var(--text-2);
  }
  .err {
    margin-top: 4px;
  }
  .wx-none {
    margin-top: 6px;
    font-family: var(--font-display);
    font-size: var(--t-lg);
    font-weight: 650;
    letter-spacing: -0.015em;
  }
  .retry {
    margin-top: auto;
    align-self: flex-start;
  }

  /* timers */
  .tl {
    display: grid;
    gap: 8px;
  }
  .tl li {
    display: flex;
    align-items: center;
    gap: 9px;
  }
  .tn {
    flex: 1;
    font-size: var(--t-sm);
    font-weight: 600;
  }
  .tc {
    font-size: var(--t-md);
    font-weight: 600;
  }
  .idle {
    margin-top: 2px;
  }

  /* clips */
  .cl {
    display: grid;
    gap: 2px;
    margin: 0 -8px;
  }
  .clip {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 34px;
    padding: 0 8px;
    border-radius: var(--r-2);
    text-align: left;
    color: var(--text);
    transition:
      background var(--dur-1),
      transform 0.4s var(--spring);
  }
  .clip:hover {
    background: var(--surface-2);
  }
  .clip:active {
    transform: scale(0.98);
  }
  .ki {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 7px;
    background: var(--surface-1);
    border: 1px solid var(--line);
    color: var(--text-2);
    flex: none;
  }
  .ct {
    flex: 1;
    font-size: var(--t-sm);
    font-weight: 500;
  }
  .cw {
    font-size: var(--t-xs);
    white-space: nowrap;
  }

  /* next up */
  .up {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 40px;
  }
  .ui {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 10px;
    flex: none;
    background: var(--surface-2);
    color: var(--text-2);
  }
  .ui.hol {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .ut {
    display: grid;
    min-width: 0;
  }
  .ut strong {
    font-size: var(--t-md);
    font-weight: 600;
    letter-spacing: -0.01em;
  }
</style>
