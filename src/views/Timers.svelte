<script lang="ts">
  import { flip } from 'svelte/animate';
  import Icon from '../components/Icon.svelte';
  import Ring from '../components/Ring.svelte';
  import RollingText from '../components/RollingText.svelte';
  import State from '../components/State.svelte';
  import { clockText } from '../components/pillModel';
  import {
    now,
    stopwatch,
    swElapsed,
    swLap,
    swReset,
    swToggle,
    timerAdd,
    timerDone,
    timerProgress,
    timerRemaining,
    timerRemove,
    timerReset,
    timerToggle,
    timers,
  } from '../lib/stores';
  import { formatStopwatch } from '../lib/time';
  import { arrive, depart, spotlight } from '../lib/motion';

  let label = $state('');
  let minutes = $state(5);

  function add(start: boolean, mins = minutes, name = label) {
    timerAdd(name, Math.max(1, Math.min(600, Math.round(mins || 1))), start);
    label = '';
  }

  // stopwatch needs sub-second ticks: drive it from rAF only while it runs
  let sw = $state(Date.now());
  $effect(() => {
    if ($stopwatch.runningSince == null) {
      sw = Date.now();
      return;
    }
    let id = 0;
    const loop = () => {
      sw = Date.now();
      id = requestAnimationFrame(loop);
    };
    id = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(id);
  });
  const swTime = $derived(swElapsed($stopwatch, sw));
  const swRunning = $derived($stopwatch.runningSince != null);

  const statusOf = (t: (typeof $timers)[number]) =>
    timerDone(t, $now) ? 'Done' : t.runningSince != null ? 'Running' : t.accumulated > 0 ? 'Paused' : 'Ready';
</script>

<div class="view">
  <div class="cols">
    <section class="left">
      <form class="adder" onsubmit={(e) => { e.preventDefault(); add(true); }}>
        <input class="input" placeholder="Name (optional)" bind:value={label} aria-label="Timer name" maxlength="40" />
        <label class="mins">
          <input class="input" type="number" min="1" max="600" bind:value={minutes} aria-label="Minutes" />
          <span class="faint">min</span>
        </label>
        <button class="btn primary" type="submit"><Icon name="plus" size={15} /> Start</button>
      </form>
      <div class="chips" aria-label="Quick timers">
        {#each [1, 5, 10, 25, 45] as m (m)}
          <button class="chip" type="button" onclick={() => add(true, m, '')}>{m} min</button>
        {/each}
      </div>

      {#if $timers.length}
        <ul class="tl scroll" aria-label="Timers">
          {#each $timers as t (t.id)}
            {@const done = timerDone(t, $now)}
            {@const running = t.runningSince != null && !done}
            <li animate:flip={{ duration: 380 }} in:arrive out:depart>
              <div class="timer card" class:done use:spotlight>
                <Ring value={timerProgress(t, $now)} size={54} stroke={4.5} tone={running || done ? 'accent' : 'default'} label={`${t.label} progress`}>
                  <Icon name={done ? 'check' : t.accumulated > 0 && !running ? 'pause' : 'timers'} size={15} />
                </Ring>
                <div class="info">
                  <div class="nm ellipsis">{t.label}</div>
                  <div class="st faint">{statusOf(t)} · {Math.round(t.duration / 60000)} min</div>
                </div>
                <div class="time" aria-live="off"><RollingText text={clockText(timerRemaining(t, $now))} /></div>
                <div class="ctl">
                  <button class="btn icon sm" aria-label={running ? `Pause ${t.label}` : done ? `Restart ${t.label}` : `Start ${t.label}`} onclick={() => timerToggle(t.id)}>
                    <Icon name={running ? 'pause' : done ? 'reset' : 'play'} size={14} />
                  </button>
                  <button class="btn icon sm ghost" aria-label={`Reset ${t.label}`} onclick={() => timerReset(t.id)}><Icon name="reset" size={14} /></button>
                  <button class="btn icon sm ghost danger" aria-label={`Delete ${t.label}`} onclick={() => timerRemove(t.id)}><Icon name="trash" size={14} /></button>
                </div>
              </div>
            </li>
          {/each}
        </ul>
      {:else}
        <div class="grow"><State icon="timers" title="No timers yet" text="Start a quick one above, or name your own. Timers keep running while the panel is closed and show in the pill." /></div>
      {/if}
    </section>

    <aside class="sw card" use:spotlight aria-label="Stopwatch">
      <span class="eyebrow"><Icon name="lap" size={13} /> Stopwatch</span>
      <div class="swt num">{formatStopwatch(swTime)}</div>
      <div class="swc">
        <button class="btn primary" onclick={swToggle}>
          <Icon name={swRunning ? 'pause' : 'play'} size={14} />{swRunning ? 'Pause' : swTime > 0 ? 'Resume' : 'Start'}
        </button>
        {#if swRunning}
          <button class="btn" onclick={swLap}><Icon name="flag" size={14} /> Lap</button>
        {:else}
          <button class="btn" onclick={swReset} disabled={swTime === 0}><Icon name="reset" size={14} /> Reset</button>
        {/if}
      </div>
      {#if $stopwatch.laps.length}
        <ul class="laps scroll" aria-label="Laps">
          {#each $stopwatch.laps as lap, i (i)}
            <li in:arrive><span class="faint">Lap {$stopwatch.laps.length - i}</span><span class="num">{formatStopwatch(lap)}</span></li>
          {/each}
        </ul>
      {/if}
    </aside>
  </div>
</div>

<style>
  .cols {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 1fr 252px;
    gap: 16px;
  }
  .left {
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .adder {
    display: flex;
    gap: 8px;
    flex: none;
  }
  .adder .input {
    flex: 1;
  }
  .mins {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 96px;
    flex: none;
    font-size: var(--t-xs);
  }
  .mins .input {
    width: 64px;
    flex: none;
    padding: 0 8px 0 12px;
  }
  .chips {
    display: flex;
    gap: 6px;
    margin: 10px 0 14px;
    flex: none;
  }
  .tl {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-right: -6px;
    padding-right: 6px;
  }
  .grow {
    flex: 1;
    display: grid;
    place-items: center;
  }
  .timer {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 11px 12px 11px 14px;
    border-radius: var(--r-4);
  }
  .timer.done {
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 18%, transparent);
  }
  .info {
    flex: 1;
    min-width: 0;
  }
  .nm {
    font-weight: 600;
    font-size: var(--t-md);
    letter-spacing: -0.01em;
  }
  .st {
    font-size: var(--t-xs);
  }
  .time {
    font-size: 26px;
    font-weight: 600;
    min-width: 76px;
    text-align: right;
  }
  .ctl {
    display: flex;
    gap: 2px;
  }

  .sw {
    align-self: start;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    min-height: 0;
    max-height: 100%;
  }
  .swt {
    font-size: 38px;
    font-weight: 600;
    letter-spacing: -0.035em;
    line-height: 1;
  }
  .swc {
    display: flex;
    gap: 8px;
  }
  .laps {
    max-height: 168px;
    display: grid;
    gap: 1px;
    margin-right: -8px;
    padding-right: 8px;
  }
  .laps li {
    display: flex;
    justify-content: space-between;
    padding: 7px 2px;
    border-top: 1px solid var(--line);
    font-size: var(--t-sm);
  }
</style>
