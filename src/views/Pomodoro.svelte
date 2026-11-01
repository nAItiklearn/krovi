<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import Ring from '../components/Ring.svelte';
  import RollingText from '../components/RollingText.svelte';
  import { clockText } from '../components/pillModel';
  import {
    POMO,
    now,
    pomoElapsed,
    pomoLabel,
    pomoRemaining,
    pomodoro,
    pomodoroAdvance,
    pomodoroPause,
    pomodoroResetCycle,
    pomodoroResetPhase,
    pomodoroStart,
  } from '../lib/stores';

  const p = $derived($pomodoro);
  const remaining = $derived(pomoRemaining(p, $now));
  const progress = $derived(Math.min(1, pomoElapsed(p, $now) / p.duration));
  const running = $derived(p.runningSince != null && !p.finished);
  const cycle = $derived(p.completed % POMO.every);
  // after a finished focus session the dots show the cycle that just completed
  const dots = $derived(
    cycle === 0 && p.completed > 0 && (p.phase === 'long' || (p.phase === 'work' && p.finished)) ? POMO.every : cycle,
  );

  const cta = $derived(p.finished ? (p.phase === 'work' ? 'Start break' : 'Back to focus') : running ? 'Pause' : p.accumulated > 0 ? 'Resume' : 'Start focus');
  const ctaIcon = $derived(p.finished ? 'chevronRight' : running ? 'pause' : 'play');

  function primary() {
    if (p.finished) pomodoroAdvance();
    else if (running) pomodoroPause();
    else pomodoroStart();
  }
</script>

<div class="view">
  <div class="wrap">
    <div class="dial">
      <Ring value={progress} size={288} stroke={9} tone={p.phase === 'work' ? 'accent' : 'default'} label={`${pomoLabel(p.phase)} progress`}>
        <div class="inner">
          <span class="phase eyebrow">{pomoLabel(p.phase)}</span>
          <span class="big num"><RollingText text={clockText(remaining)} /></span>
          <span class="sess faint">{p.finished ? 'Complete' : running ? 'In progress' : p.accumulated > 0 ? 'Paused' : 'Ready'}</span>
        </div>
      </Ring>
    </div>

    <div class="side">
      <div class="cycle" role="img" aria-label={`${cycle} of ${POMO.every} focus sessions in this cycle`}>
        {#each Array(POMO.every) as _, i (i)}
          <span class:fill={i < dots}></span>
        {/each}
      </div>
      <p class="today">
        <strong class="num">{p.completed}</strong>
        <span class="dim">{p.completed === 1 ? 'focus session' : 'focus sessions'} today</span>
      </p>

      <div class="ctl">
        <button class="btn primary lg" onclick={primary}><Icon name={ctaIcon} size={16} />{cta}</button>
        <button class="btn" onclick={pomodoroResetPhase} disabled={!running && p.accumulated === 0 && !p.finished}>
          <Icon name="reset" size={15} /> Reset
        </button>
        <button class="btn ghost" onclick={pomodoroAdvance}>Skip</button>
      </div>

      <p class="rule faint">
        {POMO.work / 60000} min focus, {POMO.short / 60000} min break, and a {POMO.long / 60000} min break after every
        {POMO.every} sessions. It keeps counting while the panel is closed and appears in the pill.
      </p>
      <button class="btn sm ghost" onclick={pomodoroResetCycle}>Reset cycle</button>
    </div>
  </div>
</div>

<style>
  .wrap {
    flex: 1;
    display: grid;
    grid-template-columns: 300px 1fr;
    align-items: center;
    gap: 44px;
    padding: 0 24px;
  }
  .dial {
    display: grid;
    place-items: center;
  }
  .inner {
    display: grid;
    gap: 2px;
    justify-items: center;
  }
  .big {
    font-size: 62px;
    font-weight: 600;
    letter-spacing: -0.045em;
    line-height: 1.05;
  }
  .sess {
    font-size: var(--t-sm);
  }
  .side {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 14px;
  }
  .cycle {
    display: flex;
    gap: 9px;
  }
  .cycle span {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 1.5px solid var(--line-strong);
    transition:
      background var(--dur-3) var(--ease-out),
      border-color var(--dur-3),
      transform 0.5s var(--spring-bouncy);
  }
  .cycle span.fill {
    background: var(--text);
    border-color: var(--text);
    transform: scale(1.1);
  }
  .today {
    display: flex;
    align-items: baseline;
    gap: 8px;
    font-size: var(--t-md);
  }
  .today strong {
    font-size: var(--t-3xl);
    font-weight: 600;
    letter-spacing: -0.04em;
    line-height: 1;
  }
  .ctl {
    display: flex;
    gap: 8px;
    margin-top: 6px;
  }
  .btn.lg {
    height: 40px;
    padding: 0 20px;
    font-size: var(--t-md);
    border-radius: var(--r-3);
  }
  .rule {
    max-width: 38ch;
    font-size: var(--t-xs);
    line-height: 1.5;
  }
</style>
