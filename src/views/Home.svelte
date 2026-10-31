<script lang="ts">
  import { onMount } from 'svelte';
  import { activeTool, now } from '../lib/stores';
  import { bridge } from '../lib/bridge';
  import { formatClock, relativeTime } from '../lib/time';
  import { visible } from '../lib/clip';
  import Icon from '../components/Icon.svelte';
  import type {
    ClipEntry,
    MediaTrack,
    Note,
    ScreenTimeRecord,
    Task,
    WeatherData,
  } from '../lib/types';
  import { today } from '../lib/fixtures';

  let clips = $state<ClipEntry[]>([]);
  let notes = $state<Note[]>([]);
  let tasks = $state<Task[]>([]);
  let weather = $state<WeatherData | null>(null);
  let media = $state<MediaTrack | null>(null);
  let screen = $state<ScreenTimeRecord[]>([]);

  // A live focus countdown so Home feels alive (seeded mid-session).
  const focusDuration = 25 * 60_000;
  const focusStart = Date.now() - (6 * 60_000 + 23_000);
  const focusRemaining = $derived(Math.max(0, focusDuration - ($now - focusStart)));

  const topClips = $derived(visible(clips).slice(0, 2));
  const todayTotal = $derived(screen.filter((r) => r.day === today).reduce((a, r) => a + r.ms, 0));
  const topApp = $derived(
    screen.filter((r) => r.day === today).sort((a, b) => b.ms - a.ms)[0] ?? null,
  );
  const openTasks = $derived(tasks.filter((t) => !t.done).length);

  function hm(ms: number): string {
    const m = Math.round(ms / 60_000);
    return `${Math.floor(m / 60)}h ${m % 60}m`;
  }

  onMount(async () => {
    [clips, notes, tasks, weather, media, screen] = await Promise.all([
      bridge.loadClipboard(),
      bridge.loadNotes(),
      bridge.loadTasks(),
      bridge.loadWeather(),
      bridge.loadMedia(),
      bridge.loadScreenTime(),
    ]);
  });
</script>

<div class="grid">
  <button class="card tile" onclick={() => activeTool.set('screentime')}>
    <p class="eyebrow"><Icon name="screentime" size={13} /> Screen time</p>
    <div class="big">{hm(todayTotal)}</div>
    {#if topApp}
      <div class="sub muted">{topApp.app} · {hm(topApp.ms)}</div>
      <div class="bars" aria-hidden="true">
        {#each screen.filter((r) => r.day === today).slice(0, 6) as r (r.app)}
          <span style={`height:${Math.max(10, (r.ms / (topApp?.ms || 1)) * 100)}%`}></span>
        {/each}
      </div>
    {/if}
  </button>

  <button class="card tile" onclick={() => activeTool.set('pomodoro')}>
    <p class="eyebrow"><Icon name="pomodoro" size={13} /> Focus</p>
    <div class="big mono">{formatClock(focusRemaining)}</div>
    <div class="sub muted">{focusRemaining > 0 ? 'Focusing' : 'Break time'}</div>
  </button>

  {#if weather}
    <button class="card tile" onclick={() => activeTool.set('weather')}>
      <p class="eyebrow"><Icon name="weather" size={13} /> Weather</p>
      <div class="big">{weather.current}°</div>
      <div class="sub muted">{weather.condition} · {weather.city}</div>
      <div class="sub faint">H {weather.high}° L {weather.low}°</div>
    </button>
  {/if}

  <button class="card tile wide" onclick={() => activeTool.set('clipboard')}>
    <p class="eyebrow"><Icon name="clipboard" size={13} /> Clipboard</p>
    <ul class="list">
      {#each topClips as c (c.id)}
        <li>
          {#if c.pinned}<Icon name="pin" size={13} />{/if}
          <span class="txt">{c.content}</span>
          <span class="when faint">{relativeTime(c.created_at, $now)}</span>
        </li>
      {/each}
    </ul>
  </button>

  <button class="card tile" onclick={() => activeTool.set('notes')}>
    <p class="eyebrow"><Icon name="notes" size={13} /> Notes</p>
    {#if notes[0]}
      <div class="note-title">{notes[0].body.split('\n')[0]}</div>
      <div class="sub faint">Edited {relativeTime(notes[0].updated_at, $now)}</div>
    {/if}
    <div class="sub muted">{openTasks} task{openTasks === 1 ? '' : 's'} open</div>
  </button>

  {#if media}
    <button class="card tile" onclick={() => activeTool.set('media')}>
      <p class="eyebrow"><Icon name="media" size={13} /> Now playing</p>
      <div class="note-title">{media.title}</div>
      <div class="sub muted">{media.artist}</div>
      <div class="sub faint">{media.playing ? 'Playing' : 'Paused'} · {media.album}</div>
    </button>
  {/if}
</div>

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 14px;
  }
  .tile {
    text-align: left;
    color: var(--text);
    display: flex;
    flex-direction: column;
    gap: 2px;
    transition: border-color 0.14s ease, transform 0.1s ease;
  }
  .tile:hover {
    border-color: var(--accent);
  }
  .tile:active {
    transform: translateY(1px);
  }
  .wide {
    grid-column: span 2;
  }
  .big {
    font-size: 30px;
    font-weight: 680;
    letter-spacing: -0.02em;
    line-height: 1.1;
  }
  .sub {
    font-size: 12.5px;
  }
  .note-title {
    font-size: 15px;
    font-weight: 600;
    margin-top: 2px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bars {
    display: flex;
    align-items: flex-end;
    gap: 4px;
    height: 34px;
    margin-top: 10px;
  }
  .bars span {
    flex: 1;
    background: var(--accent);
    border-radius: 3px;
    opacity: 0.55;
    min-height: 4px;
  }
  .bars span:first-child {
    opacity: 1;
  }
  .list {
    list-style: none;
    margin: 6px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .list li {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-dim);
    font-size: 13px;
  }
  .list .txt {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .list .when {
    margin-left: auto;
    font-size: 11.5px;
    white-space: nowrap;
  }
  @media (max-width: 560px) {
    .wide {
      grid-column: span 1;
    }
  }
</style>
