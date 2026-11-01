<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { get } from 'svelte/store';
  import {
    ISLAND_PAD,
    PANEL,
    commitMode,
    onShellRequest,
    requestExpand,
    setHitRect,
    shellMode,
  } from '../lib/shell';
  import { mode as envMode } from '../lib/bridge';
  import { clips, media, pulse, stats } from '../lib/live';
  import { startVoice, stopVoice, voiceAvailability, voiceState } from '../lib/voice';
  import { isIdle, markActivity } from '../lib/idle';
  import IdleCompanion from './idle/IdleCompanion.svelte';
  import Icon from './Icon.svelte';
  import {
    activity,
    closeVoiceSheets,
    liveTimer,
    onIntent,
    paletteOpen,
    pushActivity,
    voiceHelpOpen,
    voiceResults,
    type Intent,
  } from '../lib/stores';
  import { SPRINGS, clamp, createSpring, reduced, sleep, unlerp } from '../lib/motion';
  import {
    PILL_BASE_H,
    clipLabel,
    freeDisk,
    latestClip,
    memPercent,
    morphRadius,
    pillMode,
    pillSize,
  } from './pillModel';
  import Pill from './Pill.svelte';
  import Panel from './Panel.svelte';

  // ---------------------------------------------------------------------------
  // The Island: ONE shape that is both the pill and the panel.
  //
  //  expand  : window grows (requestExpand) -> width/height springs overshoot and settle
  //            -> panel contents arrive staggered once the shape is ~60% open
  //  collapse: contents leave first -> shape springs home -> commitMode('pill')
  //
  // Pill and panel contents are fixed-size layers clipped by the shape, so nothing reflows
  // while it morphs. The shape is anchored top-centre, exactly like the native window.
  // ---------------------------------------------------------------------------

  type Phase = 'pill' | 'opening' | 'panel' | 'closing';
  const W = PANEL.w;
  const H = PANEL.h;
  const PANEL_RADIUS = 30;
  const demo = envMode() === 'demo';

  let phase = $state<Phase>('pill');
  let panelIn = $state(false);
  let hover = $state(false);
  let contentW = $state(150);
  let fit = $state(1);
  let haloKey = $state(0);
  let announce = $state('');
  let stageEl: HTMLElement | undefined = $state();
  let panelEl: HTMLElement | undefined = $state();

  // ---- pill model -----------------------------------------------------------
  const voice = $derived(
    $voiceState.state === 'listening' || $voiceState.state === 'processing' || $voiceState.state === 'error'
      ? $voiceState.state
      : null,
  );
  const voiceBusy = $derived(voice === 'listening' || voice === 'processing');
  let micOk = $state(true);
  let micFocus = $state(false);
  const showMic = $derived(phase === 'pill' && micOk && (hover || micFocus || voiceBusy));
  function toggleMic() {
    if (voiceBusy) void stopVoice();
    // started from the pill the previous app still has focus, so "this" can be read from it
    else void startVoice('command', { captureContext: true });
  }
  const pmode = $derived(pillMode({ voice: voice !== null, activity: $activity !== null }));
  const psize = $derived(pillSize(pmode, contentW));
  // The collapsed pill rests as a companion after a quiet spell; anything alive wakes it.
  const pillShown = $derived($shellMode === 'pill' && phase === 'pill' && pmode === 'idle');
  const idleActive = $derived($isIdle && pillShown);
  const mem = $derived(memPercent($stats));
  const free = $derived(freeDisk($stats));
  const pillLabel = $derived(
    mem === null
      ? 'Open Krovi'
      : `Open Krovi. RAM ${Math.round(mem)} percent used${free ? `, ${free.text} free on ${$stats?.diskLabel ?? 'C:'}` : ''}`,
  );

  // ---- springs --------------------------------------------------------------
  const sw = createSpring(untrack(() => psize.w), SPRINGS.pill);
  const sh = createSpring(untrack(() => psize.h), SPRINGS.pill);

  function tune(kind: 'open' | 'close' | 'pill') {
    const w = kind === 'open' ? SPRINGS.openW : kind === 'close' ? SPRINGS.closeW : SPRINGS.pill;
    const h = kind === 'open' ? SPRINGS.openH : kind === 'close' ? SPRINGS.closeH : SPRINGS.pill;
    sw.tune(w);
    sh.tune(h);
  }

  /** Resolves true when both springs settled on this target, false if retargeted/aborted. */
  async function springTo(w: number, h: number, instant = false): Promise<boolean> {
    const [a, b] = await Promise.all([sw.set(w, { instant }), sh.set(h, { instant })]);
    return a && b;
  }

  // progress 0 (pill) .. 1 (panel); overshoots slightly with the spring
  const p = $derived(clamp(unlerp(PILL_BASE_H, H, $sh), 0, 1.08));
  const pc = $derived(clamp(p, 0, 1));
  const radius = $derived(morphRadius(p, $sw, $sh, psize.h, PANEL_RADIUS));
  const pillOpacity = $derived(clamp(1 - p * 4.5, 0, 1));

  // keep the pill the right size while idle (hover, live activity, stats appearing...)
  $effect(() => {
    const { w, h } = psize;
    untrack(() => {
      if (phase !== 'pill') return;
      tune('pill');
      void springTo(w, h, reduced());
    });
  });

  // panel contents arrive once the shape is ~60% open
  $effect(() => {
    if (phase === 'opening' && p > 0.6) panelIn = true;
  });

  // ---- hit rect (click-through) -----------------------------------------------
  type Rect = { x: number; y: number; w: number; h: number };
  function measureStage(): Rect | null {
    if (!stageEl) return null;
    const r = stageEl.getBoundingClientRect();
    return { x: Math.floor(r.left) - 1, y: Math.floor(r.top) - 1, w: Math.ceil(r.width) + 2, h: Math.ceil(r.height) + 2 };
  }
  function finalPanelRect(): Rect {
    const vw = window.innerWidth;
    return {
      x: Math.floor((vw - W * fit) / 2) - 1,
      y: Math.floor(ISLAND_PAD) - 1,
      w: Math.ceil(W * fit) + 2,
      h: Math.ceil(H * fit) + 2,
    };
  }
  const union = (a: Rect, b: Rect): Rect => {
    const x = Math.min(a.x, b.x);
    const y = Math.min(a.y, b.y);
    return { x, y, w: Math.max(a.x + a.w, b.x + b.w) - x, h: Math.max(a.y + a.h, b.y + b.h) - y };
  };
  const sendHit = (r: Rect | null) => {
    if (r) void setHitRect({ x: Math.max(0, r.x), y: Math.max(0, r.y), w: r.w, h: r.h });
  };
  let hitTimer: ReturnType<typeof setTimeout> | null = null;
  function reportSoon() {
    if (phase === 'opening' || phase === 'closing') return; // morph start already reported the union
    if (hitTimer) return;
    hitTimer = setTimeout(() => {
      hitTimer = null;
      sendHit(measureStage());
    }, 50);
  }

  // ---- state machine ------------------------------------------------------------
  let run = 0; // monotonically increasing token: a newer transition cancels an older one
  let expanding = false;

  async function expand(fromShell = false) {
    if (phase === 'panel' || phase === 'opening') return;
    const token = ++run;
    expanding = true;
    hover = false;
    if (!fromShell) await requestExpand(); // the native window grows BEFORE the shape does
    if (token !== run) {
      expanding = false;
      return;
    }
    phase = 'opening';
    sendHit(union(measureStage() ?? finalPanelRect(), finalPanelRect()));
    if (reduced()) {
      tune('open');
      await springTo(W, H, true);
      panelIn = true;
    } else {
      tune('open');
      const ok = await springTo(W, H);
      if (!ok || token !== run) {
        expanding = false;
        return;
      }
    }
    expanding = false;
    phase = 'panel';
    panelIn = true;
    await tick();
    sendHit(measureStage());
    // keep focus in an already-open palette or voice sheet (a voice command can open the panel with rows)
    if (!get(paletteOpen) && !get(voiceResults) && !get(voiceHelpOpen)) panelEl?.focus({ preventScroll: true });
  }

  async function collapse() {
    if (phase !== 'panel' && phase !== 'opening') return;
    const token = ++run;
    expanding = false;
    phase = 'closing';
    panelIn = false;
    paletteOpen.set(false);
    closeVoiceSheets();
    sendHit(union(measureStage() ?? finalPanelRect(), finalPanelRect()));
    await sleep(reduced() ? 0 : 150); // contents leave first
    if (token !== run) return;
    tune('close');
    const ok = await springTo(psize.w, psize.h, reduced());
    if (!ok || token !== run) return;
    phase = 'pill';
    await commitMode('pill');
    await tick();
    sendHit(measureStage());
  }

  /** The shell disagrees with what we show (watchdog, tray, second instance): snap, don't animate. */
  function snapToPill() {
    run++;
    expanding = false;
    phase = 'pill';
    panelIn = false;
    paletteOpen.set(false);
    closeVoiceSheets();
    tune('pill');
    void springTo(psize.w, psize.h, true);
    void tick().then(() => sendHit(measureStage()));
  }

  function act(i: Intent) {
    markActivity();
    if (i === 'expand') void expand();
    else if (i === 'collapse') void collapse();
    else if (phase === 'panel' || phase === 'opening') void collapse();
    else void expand();
  }

  // ---- live activity: copy pulse ----------------------------------------------
  let lastPulse = get(pulse);
  $effect(() => {
    const n = $pulse;
    if (n <= lastPulse) {
      lastPulse = Math.min(lastPulse, n);
      return;
    }
    lastPulse = n;
    markActivity();
    // the new entry lands in the same tick as the counter; read it after the store settles
    setTimeout(() => {
      const c = latestClip(get(clips));
      pushActivity('copy', c ? clipLabel(c) : 'Clipboard updated');
      haloKey += 1;
    }, 0);
  });

  // voice starting/finishing/failing and live activity all count as signs of life
  $effect(() => {
    void $voiceState.state;
    void $activity;
    markActivity();
  });

  $effect(() => {
    const a = $activity;
    announce = a ? (a.kind === 'copy' ? `Copied ${a.text}` : `${a.text}. ${a.detail ?? ''} ${a.chip?.title ?? ''}`.trim()) : '';
  });

  // ---- lifecycle -----------------------------------------------------------------
  onMount(() => {
    void voiceAvailability().then((a) => (micOk = a.commands));
    const offIntent = onIntent(act);
    const offShell = onShellRequest((a) => act(a));
    const offMode = shellMode.subscribe((m) => {
      markActivity();
      if (m === 'panel' && phase === 'pill' && !expanding) void expand(true);
      else if ((m === 'pill' || m === 'hidden') && (phase === 'panel' || phase === 'opening')) snapToPill();
    });

    const refit = () => {
      fit = demo ? Math.min(1, (window.innerWidth - 16) / W, (window.innerHeight - 16) / (H + ISLAND_PAD)) : 1;
      reportSoon();
    };
    refit();
    window.addEventListener('resize', refit);

    const ro = stageEl ? new ResizeObserver(reportSoon) : null;
    if (stageEl) ro?.observe(stageEl);
    void tick().then(() => sendHit(measureStage()));

    return () => {
      offIntent();
      offShell();
      offMode();
      ro?.disconnect();
      window.removeEventListener('resize', refit);
    };
  });

  function onEnter(e: PointerEvent) {
    markActivity();
    if (phase === 'pill' && e.pointerType !== 'touch') hover = true;
  }
  // resting the cursor on the pill keeps it awake; throttled, markActivity itself is cheap
  let lastMove = 0;
  function onMove() {
    const t = performance.now();
    if (t - lastMove < 250) return;
    lastMove = t;
    markActivity();
  }
  function onLeave() {
    hover = false;
  }

  // exposed for tests/screenshots (no effect in production flows)
  export function snapshot() {
    return { phase, w: $sw, h: $sh, p };
  }
</script>

<div class="island" style:--fit={fit}>
  {#if phase !== 'pill'}
    <div class="scrim" aria-hidden="true" onpointerdown={() => void collapse()}></div>
  {/if}

  <div class="fitbox">
    <div
      class="stage"
      bind:this={stageEl}
      data-phase={phase}
      style:width="{$sw}px"
      style:height="{$sh}px"
      style:--r="{radius}px"
      style:--pc={pc}
      onpointerenter={onEnter}
      onpointermove={onMove}
      onpointerdown={markActivity}
      onpointerleave={onLeave}
      role="presentation">
      {#key haloKey}
        {#if haloKey > 0}<span class="halo" aria-hidden="true"></span>{/if}
      {/key}

      <div class="shape">
        <div class="bg bg-pill"></div>
        <div class="bg bg-panel"><i class="grain"></i></div>
        <div class="rim"></div>

        <div
          class="pill-layer"
          style:opacity={pillOpacity}
          style:height="{psize.h}px"
          style:filter={pillOpacity < 1 ? `blur(${((1 - pillOpacity) * 5).toFixed(1)}px)` : undefined}
          inert={phase !== 'pill'}>
          <div class="pill-fade" class:away={idleActive}>
          <Pill
            mode={pmode}
            stats={$stats}
            playing={$media?.playing ?? false}
            {hover}
            activity={$activity}
            timer={$liveTimer}
            {voice}
            partial={$voiceState.partial}
            voiceError={$voiceState.error ?? ''}
            {showMic}
            onmeasure={(w) => (contentW = w)} />
          </div>
        </div>
        <!-- decorative: fills the pill's content box, never takes focus or pointer events -->
        <div class="idle-layer" aria-hidden="true" inert style:height="{psize.h}px" style:opacity={pillOpacity}>
          <IdleCompanion active={pillShown} />
        </div>
        {#if phase === 'pill'}
          <button
            class="pill-hit"
            type="button"
            aria-label={pillLabel}
            onfocus={markActivity}
            onclick={() => void expand()}></button>
          {#if micOk}
            <button
              class="pill-mic"
              class:show={showMic}
              class:live={voiceBusy}
              type="button"
              style:top="{(psize.h - 26) / 2}px"
              aria-label={voiceBusy ? 'Stop listening' : 'Voice command'}
              aria-pressed={voiceBusy}
              onfocus={() => (micFocus = true)}
              onblur={() => (micFocus = false)}
              onclick={toggleMic}>
              <Icon name="mic" size={14} />
            </button>
          {/if}
        {/if}

        <div
          class="panel-layer"
          class:hidden={!panelIn && p < 0.04}
          bind:this={panelEl}
          tabindex="-1"
          role="region"
          aria-label="Krovi panel"
          inert={phase === 'pill' || phase === 'closing'}>
          <Panel show={panelIn} />
        </div>
      </div>
    </div>
  </div>

  <div class="sr-only" aria-live="polite" role="status">{announce}</div>
</div>

<style>
  .island {
    position: fixed;
    inset: 0;
    overflow: hidden;
    pointer-events: none;
  }
  .scrim {
    position: absolute;
    inset: 0;
    pointer-events: auto;
  }
  .fitbox {
    position: absolute;
    top: var(--island-pad);
    left: 50%;
    transform: translateX(-50%) scale(var(--fit, 1));
    transform-origin: 50% 0;
  }
  .stage {
    position: relative;
    pointer-events: auto;
    border-radius: var(--r);
    contain: layout style;
    will-change: width, height;
    transition: transform 0.5s var(--spring);
  }
  /* two stacked shadows crossfade with progress so the lift grows with the shape */
  .stage::before,
  .stage::after {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: inherit;
    pointer-events: none;
  }
  .stage::before {
    box-shadow: var(--shadow-pill);
    opacity: calc(1 - var(--pc));
  }
  .stage::after {
    box-shadow: var(--shadow-panel);
    opacity: var(--pc);
  }
  .shape {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    overflow: hidden;
    -webkit-backdrop-filter: blur(24px) saturate(1.5);
    backdrop-filter: blur(24px) saturate(1.5);
    transform: translateZ(0);
  }
  :global(:root[data-env='tauri']) .shape {
    -webkit-backdrop-filter: none;
    backdrop-filter: none;
  }
  .bg {
    position: absolute;
    inset: 0;
  }
  .bg-pill {
    background: var(--pill-bg);
    opacity: calc(1 - var(--pc));
  }
  .bg-panel {
    background: var(--panel-bg);
    opacity: var(--pc);
  }
  .grain {
    position: absolute;
    inset: 0;
    opacity: var(--grain-opacity);
    background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='160' height='160'><filter id='n'><feTurbulence type='fractalNoise' baseFrequency='.85' numOctaves='2' stitchTiles='stitch'/><feColorMatrix values='0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 .6 0'/></filter><rect width='100%' height='100%' filter='url(%23n)'/></svg>");
    pointer-events: none;
  }
  :global(:root[data-scheme='light']) .grain,
  :global(:root[data-scheme='light']) .grain {
    filter: invert(1);
  }
  /* 1px inner highlight: brighter on top, fading away, like light catching a bevel */
  .rim {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    padding: 1px;
    background: linear-gradient(180deg, var(--rim-a), var(--rim-b) 38%, var(--rim-b) 70%, var(--rim-a) 160%);
    -webkit-mask:
      linear-gradient(#000 0 0) content-box,
      linear-gradient(#000 0 0);
    -webkit-mask-composite: xor;
    mask:
      linear-gradient(#000 0 0) content-box exclude,
      linear-gradient(#000 0 0);
    mask-composite: exclude;
    pointer-events: none;
  }
  .pill-layer {
    position: absolute;
    top: 0;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    pointer-events: none;
    will-change: opacity;
  }
  /* readouts fade out gently as the eyes open, and snap back quickly when something wakes the pill */
  .pill-fade {
    display: flex;
    align-items: center;
    height: 100%;
    transition:
      opacity 140ms var(--ease-out),
      filter 140ms var(--ease-out);
  }
  .pill-fade.away {
    opacity: 0;
    filter: blur(3px);
    transition-duration: var(--dur-3);
  }
  /* the companion sits exactly over the pill and fades in as the readouts fade out */
  .idle-layer {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 1;
    pointer-events: none;
    overflow: hidden;
    border-radius: inherit;
  }
  .pill-hit {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    z-index: 3;
    cursor: pointer;
    transition: transform 0.45s var(--spring);
  }
  .pill-hit:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 3px;
    border-radius: 999px;
  }
  .pill-mic {
    position: absolute;
    right: 8px;
    z-index: 4;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    color: var(--pill-fg);
    background: color-mix(in srgb, var(--pill-fg) 14%, transparent);
    opacity: 0;
    transform: scale(0.8);
    pointer-events: none;
    transition:
      opacity var(--dur-2) var(--ease-out),
      transform 0.5s var(--spring-bouncy),
      background var(--dur-1);
  }
  .pill-mic.show,
  .pill-mic:focus-visible {
    opacity: 1;
    transform: none;
    pointer-events: auto;
  }
  .pill-mic:hover {
    background: color-mix(in srgb, var(--pill-fg) 24%, transparent);
  }
  .pill-mic.live {
    color: var(--accent-ink);
    background: var(--accent);
  }
  .stage[data-phase='pill']:active {
    transform: scale(0.97);
    transition: transform 0.08s;
  }
  .panel-layer {
    position: absolute;
    top: 0;
    left: 50%;
    width: 900px;
    height: 620px;
    margin-left: -450px;
    outline: none;
  }
  .panel-layer.hidden {
    visibility: hidden;
  }

  /* accent arrival glow when something new is copied */
  .halo {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    pointer-events: none;
    animation: halo 1.05s var(--ease-out) both;
  }
  @keyframes halo {
    0% {
      box-shadow:
        0 0 0 0 var(--accent-glow),
        0 0 14px 1px var(--accent-glow);
      opacity: 1;
    }
    100% {
      box-shadow:
        0 0 0 9px transparent,
        0 0 22px 6px transparent;
      opacity: 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .halo {
      animation: halo-flash 0.6s linear both;
    }
    @keyframes halo-flash {
      0% {
        box-shadow: 0 0 0 2px var(--accent-glow);
      }
      100% {
        box-shadow: 0 0 0 2px transparent;
      }
    }
  }
</style>
