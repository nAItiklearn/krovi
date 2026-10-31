<script lang="ts">
  import { onMount } from 'svelte';
  import { now } from '../lib/stores';
  import { bridge } from '../lib/bridge';
  import Icon from '../components/Icon.svelte';
  import type { MediaTrack } from '../lib/types';

  interface Queued extends MediaTrack {}

  const queue: Queued[] = [
    { title: 'Midnight Drive', artist: 'Neon Harbor', album: 'After Hours', playing: true, positionSec: 72, durationSec: 214 },
    { title: 'Paper Planes', artist: 'Lantern', album: 'Field Notes', playing: false, positionSec: 0, durationSec: 198 },
    { title: 'Slow Tide', artist: 'Marigold', album: 'Coastlines', playing: false, positionSec: 0, durationSec: 243 },
  ];

  let idx = $state(0);
  let playing = $state(true);
  let position = $state(72); // seconds
  let lastTick = Date.now();

  const track = $derived(queue[idx]);

  // Advance the playhead using the shared clock while playing.
  $effect(() => {
    const t = $now;
    const dt = (t - lastTick) / 1000;
    lastTick = t;
    if (playing && dt > 0) {
      position = Math.min(track.durationSec, position + dt);
      if (position >= track.durationSec) next();
    }
  });

  const pct = $derived(Math.min(100, (position / track.durationSec) * 100));

  function fmt(sec: number): string {
    const s = Math.max(0, Math.floor(sec));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  }
  function play(i: number) {
    idx = i;
    position = 0;
    playing = true;
  }
  function next() {
    idx = (idx + 1) % queue.length;
    position = 0;
  }
  function prev() {
    if (position > 3) {
      position = 0;
    } else {
      idx = (idx - 1 + queue.length) % queue.length;
      position = 0;
    }
  }
  function seek(e: MouseEvent) {
    const el = e.currentTarget as HTMLElement;
    const r = el.getBoundingClientRect();
    position = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)) * track.durationSec;
  }

  onMount(async () => {
    const t = await bridge.loadMedia();
    const i = queue.findIndex((q) => q.title === t.title);
    if (i >= 0) idx = i;
    playing = t.playing;
    position = t.positionSec;
  });
</script>

<div class="player card">
  <div class="art" aria-hidden="true"><Icon name="media" size={40} /></div>
  <div class="meta">
    <div class="title">{track.title}</div>
    <div class="artist muted">{track.artist}</div>
    <div class="album faint">{track.album}</div>
  </div>
  <button class="bar" onclick={seek} title="Seek" aria-label="Seek track position">
    <span class="fill" style={`width:${pct}%`}></span>
  </button>
  <div class="times faint mono">
    <span>{fmt(position)}</span>
    <span>{fmt(track.durationSec)}</span>
  </div>
  <div class="transport">
    <button class="tbtn" title="Previous" onclick={prev}><Icon name="prev" size={20} /></button>
    <button class="tbtn big" title={playing ? 'Pause' : 'Play'} onclick={() => (playing = !playing)}>
      <Icon name={playing ? 'pause' : 'play'} size={24} />
    </button>
    <button class="tbtn" title="Next" onclick={next}><Icon name="next" size={20} /></button>
  </div>
</div>

<div class="queue">
  <p class="eyebrow">Up next</p>
  <ul>
    {#each queue as q, i (q.title)}
      <li>
        <button class="qrow" class:active={i === idx} onclick={() => play(i)}>
          <Icon name={i === idx && playing ? 'pause' : 'play'} size={14} />
          <span class="qt">{q.title}</span>
          <span class="qa faint">{q.artist}</span>
          <span class="qd faint mono">{fmt(q.durationSec)}</span>
        </button>
      </li>
    {/each}
  </ul>
</div>

<style>
  .player {
    max-width: 440px;
    margin: 0 auto 18px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    text-align: center;
  }
  .art {
    width: 120px;
    height: 120px;
    border-radius: 16px;
    background: linear-gradient(140deg, var(--accent-soft), var(--surface-3));
    display: grid;
    place-items: center;
    color: var(--accent);
  }
  .title {
    font-size: 19px;
    font-weight: 680;
  }
  .artist {
    font-size: 14px;
  }
  .album {
    font-size: 12px;
  }
  .bar {
    width: 100%;
    height: 6px;
    border: none;
    padding: 0;
    border-radius: 999px;
    background: var(--surface-3);
    cursor: pointer;
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .times {
    width: 100%;
    display: flex;
    justify-content: space-between;
    font-size: 11.5px;
  }
  .transport {
    display: flex;
    align-items: center;
    gap: 18px;
    margin-top: 2px;
  }
  .tbtn {
    border: none;
    background: transparent;
    color: var(--text);
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: 50%;
  }
  .tbtn:hover {
    background: var(--surface-3);
  }
  .tbtn.big {
    background: var(--accent);
    color: var(--accent-contrast);
    width: 54px;
    height: 54px;
  }
  .tbtn.big:hover {
    background: var(--accent-strong);
  }
  .queue {
    max-width: 440px;
    margin: 0 auto;
  }
  .queue ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .qrow {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 11px;
    border: none;
    background: transparent;
    color: var(--text-dim);
    padding: 9px 10px;
    border-radius: 9px;
    text-align: left;
  }
  .qrow:hover {
    background: var(--surface-2);
  }
  .qrow.active {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .qt {
    font-weight: 550;
    font-size: 13.5px;
    color: var(--text);
  }
  .qrow.active .qt {
    color: var(--accent);
  }
  .qa {
    flex: 1;
    font-size: 12px;
  }
  .qd {
    font-size: 12px;
  }
</style>
