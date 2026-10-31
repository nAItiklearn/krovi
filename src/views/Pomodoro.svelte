<script lang="ts">
  import { now } from '../lib/stores';
  import { formatClock } from '../lib/time';
  import Icon from '../components/Icon.svelte';

  type Phase = 'Work' | 'ShortBreak' | 'LongBreak';
  const CONFIG = { work: 25 * 60_000, short: 5 * 60_000, long: 15 * 60_000, every: 4 };

  let phase = $state<Phase>('Work');
  let duration = $state(CONFIG.work);
  let accumulated = $state(0);
  let runningSince = $state<number | null>(null);
  let completed = $state(2); // seed: two focus sessions already done today

  const raw = $derived(accumulated + (runningSince != null ? Math.max(0, $now - runningSince) : 0));
  const remaining = $derived(Math.max(0, duration - Math.min(duration, raw)));
  const finished = $derived(raw >= duration);
  const progress = $derived(Math.min(1, raw / duration));

  const phaseLabel = $derived(
    phase === 'Work' ? 'Focus' : phase === 'ShortBreak' ? 'Short break' : 'Long break',
  );

  function durFor(p: Phase): number {
    return p === 'Work' ? CONFIG.work : p === 'ShortBreak' ? CONFIG.short : CONFIG.long;
  }

  function start() {
    if (runningSince == null && !finished) runningSince = $now;
  }
  function pause() {
    if (runningSince != null) {
      accumulated += Math.max(0, $now - runningSince);
      runningSince = null;
    }
  }
  function resetPhase() {
    accumulated = 0;
    runningSince = null;
  }
  function advance() {
    let next: Phase;
    if (phase === 'Work') {
      completed += 1;
      next = completed % CONFIG.every === 0 ? 'LongBreak' : 'ShortBreak';
    } else {
      next = 'Work';
    }
    phase = next;
    duration = durFor(next);
    accumulated = 0;
    runningSince = $now; // auto-start the next phase
  }
  function resetAll() {
    phase = 'Work';
    duration = CONFIG.work;
    accumulated = 0;
    runningSince = null;
    completed = 0;
  }

  const running = $derived(runningSince != null && !finished);
  const dotsInCycle = $derived(completed % CONFIG.every);
</script>

<div class="wrap">
  <div class="dial" style={`--p:${progress}`} data-phase={phase}>
    <div class="inner">
      <span class="phase">{phaseLabel}</span>
      <span class="big mono">{formatClock(remaining)}</span>
      <span class="faint">Session {completed + (phase === 'Work' ? 1 : 0)}</span>
    </div>
  </div>

  <div class="ctl">
    {#if finished}
      <button class="btn accent" onclick={advance}>
        <Icon name="chevronRight" size={16} />
        {phase === 'Work' ? 'Start break' : 'Back to focus'}
      </button>
    {:else if running}
      <button class="btn accent" onclick={pause}><Icon name="pause" size={16} /> Pause</button>
    {:else}
      <button class="btn accent" onclick={start}><Icon name="play" size={16} /> Start</button>
    {/if}
    <button class="btn" onclick={resetPhase}><Icon name="reset" size={15} /> Reset</button>
    <button class="btn ghost" onclick={advance}>Skip</button>
  </div>

  <div class="meta">
    <div class="dots" aria-label={`${dotsInCycle} of ${CONFIG.every} focus sessions this cycle`}>
      {#each Array(CONFIG.every) as _, i (i)}
        <span class:fill={i < dotsInCycle}></span>
      {/each}
    </div>
    <p class="faint sub">
      Classic 25 / 5 / 15. A long break every {CONFIG.every} focus sessions. {completed} completed today.
    </p>
    <button class="btn sm ghost" onclick={resetAll}>Reset cycle</button>
  </div>
</div>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 22px;
    padding-top: 14px;
  }
  .dial {
    --p: 0;
    width: 230px;
    height: 230px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: conic-gradient(var(--accent) calc(var(--p) * 360deg), var(--surface-3) 0);
    position: relative;
  }
  .dial[data-phase='ShortBreak'],
  .dial[data-phase='LongBreak'] {
    background: conic-gradient(var(--warn) calc(var(--p) * 360deg), var(--surface-3) 0);
  }
  .inner {
    width: 188px;
    height: 188px;
    border-radius: 50%;
    background: var(--surface);
    border: 1px solid var(--border);
    display: grid;
    place-items: center;
    gap: 2px;
    text-align: center;
  }
  .phase {
    font-size: 12px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-faint);
    font-weight: 600;
  }
  .big {
    font-size: 46px;
    font-weight: 700;
    letter-spacing: -0.02em;
  }
  .ctl {
    display: flex;
    gap: 9px;
  }
  .meta {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
  }
  .dots {
    display: flex;
    gap: 8px;
  }
  .dots span {
    width: 11px;
    height: 11px;
    border-radius: 50%;
    border: 1.5px solid var(--border);
  }
  .dots span.fill {
    background: var(--accent);
    border-color: var(--accent);
  }
  .sub {
    font-size: 12.5px;
    margin: 0;
    text-align: center;
  }
</style>
