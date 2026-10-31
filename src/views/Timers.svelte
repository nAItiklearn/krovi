<script lang="ts">
  import { now } from '../lib/stores';
  import { formatClock, formatStopwatch } from '../lib/time';
  import Icon from '../components/Icon.svelte';

  interface T {
    id: number;
    label: string;
    duration: number;
    accumulated: number;
    runningSince: number | null;
  }

  let nextId = 4;
  let timers = $state<T[]>([
    { id: 1, label: 'Tea steep', duration: 3 * 60_000, accumulated: 0, runningSince: Date.now() - 48_000 },
    { id: 2, label: 'Standup', duration: 10 * 60_000, accumulated: 0, runningSince: null },
    { id: 3, label: 'Oven', duration: 20 * 60_000, accumulated: 7 * 60_000, runningSince: null },
  ]);
  let newLabel = $state('');
  let newMin = $state(5);

  function raw(t: T, n: number): number {
    return t.accumulated + (t.runningSince != null ? Math.max(0, n - t.runningSince) : 0);
  }
  function remaining(t: T, n: number): number {
    return Math.max(0, t.duration - Math.min(t.duration, raw(t, n)));
  }
  function finished(t: T, n: number): boolean {
    return raw(t, n) >= t.duration;
  }
  function progress(t: T, n: number): number {
    return Math.min(1, raw(t, n) / t.duration);
  }

  function start(t: T) {
    if (t.runningSince == null && !finished(t, $now)) t.runningSince = $now;
    timers = [...timers];
  }
  function pause(t: T) {
    if (t.runningSince != null) {
      t.accumulated += Math.max(0, $now - t.runningSince);
      t.runningSince = null;
    }
    timers = [...timers];
  }
  function reset(t: T) {
    t.accumulated = 0;
    t.runningSince = null;
    timers = [...timers];
  }
  function remove(id: number) {
    timers = timers.filter((t) => t.id !== id);
  }
  function add() {
    const mins = Math.max(1, Math.round(newMin || 0));
    timers = [
      ...timers,
      { id: nextId++, label: newLabel.trim() || `${mins} min`, duration: mins * 60_000, accumulated: 0, runningSince: null },
    ];
    newLabel = '';
  }

  // ---- stopwatch ----
  let sw = $state<{ accumulated: number; runningSince: number | null; laps: number[] }>({
    accumulated: 0,
    runningSince: null,
    laps: [],
  });
  const swElapsed = $derived(sw.accumulated + (sw.runningSince != null ? Math.max(0, $now - sw.runningSince) : 0));
  function swStart() {
    if (sw.runningSince == null) sw.runningSince = $now;
  }
  function swPause() {
    if (sw.runningSince != null) {
      sw.accumulated += Math.max(0, $now - sw.runningSince);
      sw.runningSince = null;
    }
  }
  function swLap() {
    sw.laps = [swElapsed, ...sw.laps];
  }
  function swReset() {
    sw = { accumulated: 0, runningSince: null, laps: [] };
  }
</script>

<div class="cols">
  <section>
    <div class="adder">
      <input class="input" placeholder="Timer name" bind:value={newLabel} />
      <input class="input mins" type="number" min="1" max="600" bind:value={newMin} aria-label="minutes" />
      <button class="btn accent" onclick={add}><Icon name="plus" size={16} /> Add</button>
    </div>

    <div class="timers">
      {#each timers as t (t.id)}
        {@const done = finished(t, $now)}
        {@const running = t.runningSince != null && !done}
        <div class="timer" class:done>
          <div class="ring" style={`--p:${progress(t, $now)}`}>
            <span class="time mono">{formatClock(remaining(t, $now))}</span>
          </div>
          <div class="info">
            <div class="label">{t.label}</div>
            <div class="state faint">
              {done ? 'Finished' : running ? 'Running' : t.accumulated > 0 ? 'Paused' : 'Ready'}
            </div>
          </div>
          <div class="ctl">
            {#if running}
              <button class="mini" title="Pause" onclick={() => pause(t)}><Icon name="pause" size={15} /></button>
            {:else}
              <button class="mini" title="Start" onclick={() => start(t)} disabled={done}>
                <Icon name="play" size={15} />
              </button>
            {/if}
            <button class="mini" title="Reset" onclick={() => reset(t)}><Icon name="reset" size={15} /></button>
            <button class="mini" title="Remove" onclick={() => remove(t.id)}><Icon name="trash" size={15} /></button>
          </div>
        </div>
      {/each}
    </div>
  </section>

  <section class="sw card">
    <p class="eyebrow"><Icon name="lap" size={13} /> Stopwatch</p>
    <div class="sw-time mono">{formatStopwatch(swElapsed)}</div>
    <div class="sw-ctl">
      {#if sw.runningSince != null}
        <button class="btn" onclick={swPause}><Icon name="pause" size={15} /> Pause</button>
        <button class="btn" onclick={swLap}><Icon name="flag" size={15} /> Lap</button>
      {:else}
        <button class="btn accent" onclick={swStart}><Icon name="play" size={15} /> Start</button>
        <button class="btn" onclick={swReset} disabled={swElapsed === 0}><Icon name="reset" size={15} /> Reset</button>
      {/if}
    </div>
    {#if sw.laps.length}
      <ul class="laps scroll">
        {#each sw.laps as lap, i (sw.laps.length - i)}
          <li><span class="faint">Lap {sw.laps.length - i}</span><span class="mono">{formatStopwatch(lap)}</span></li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

<style>
  .cols {
    display: grid;
    grid-template-columns: 1fr 280px;
    gap: 16px;
  }
  .adder {
    display: flex;
    gap: 8px;
    margin-bottom: 14px;
  }
  .adder .input {
    flex: 1;
  }
  .mins {
    width: 78px;
    flex: none;
  }
  .timers {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .timer {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
  }
  .timer.done {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .ring {
    --p: 0;
    width: 62px;
    height: 62px;
    flex: none;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: conic-gradient(var(--accent) calc(var(--p) * 360deg), var(--surface-3) 0);
  }
  .ring::before {
    content: '';
    position: absolute;
    width: 50px;
    height: 50px;
    border-radius: 50%;
    background: var(--surface-2);
  }
  .timer.done .ring::before {
    background: var(--surface);
  }
  .time {
    position: relative;
    font-size: 13px;
    font-weight: 600;
  }
  .info {
    flex: 1;
    min-width: 0;
  }
  .label {
    font-weight: 600;
    font-size: 14px;
  }
  .state {
    font-size: 12px;
  }
  .ctl {
    display: flex;
    gap: 2px;
  }
  .mini {
    border: none;
    background: transparent;
    color: var(--text-dim);
    padding: 7px;
    border-radius: 8px;
  }
  .mini:hover {
    background: var(--surface-3);
    color: var(--text);
  }
  .mini:disabled {
    opacity: 0.4;
  }
  .sw {
    display: flex;
    flex-direction: column;
    gap: 12px;
    align-self: start;
  }
  .sw-time {
    font-size: 34px;
    font-weight: 680;
    letter-spacing: -0.02em;
  }
  .sw-ctl {
    display: flex;
    gap: 8px;
  }
  .laps {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 160px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .laps li {
    display: flex;
    justify-content: space-between;
    padding: 6px 2px;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
  }
  @media (max-width: 680px) {
    .cols {
      grid-template-columns: 1fr;
    }
  }
</style>
