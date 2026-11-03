<script lang="ts">
  import { onMount } from 'svelte';
  import Segmented from '../components/Segmented.svelte';
  import AskPanel from './AskPanel.svelte';
  import { isTauri, listen } from '../lib/bridge';
  import * as ci from '../lib/circle';

  // The Circle to Ask selection layer. This page is the whole content of a borderless, transparent,
  // always-on-top window that Rust puts over the monitor under the cursor when you press the shortcut.
  //
  // What happens, and what does not:
  //   * Until you release a selection, NOTHING is captured. The dim layer is only a layer.
  //   * On release this page sends one rectangle (physical pixels) to Rust. Rust hides this window,
  //     waits for Windows to stop drawing it, captures that rectangle once and reads its text.
  //   * Esc, a right-click, the shortcut again, or 90 seconds of nothing close it without capturing.
  //   * The picture never comes back to this page: only text does, and it is shown in Krovi's panel.

  type Mode = 'rect' | 'free';

  const demo = !isTauri();

  let mode = $state<Mode>('rect');
  let arm = $state<ci.ArmInfo | null>(null);
  let vw = $state(0);
  let vh = $state(0);
  let dragging = $state(false);
  let start = $state<ci.Pt | null>(null);
  let cur = $state<ci.Pt | null>(null);
  let path = $state<ci.Pt[]>([]);
  let notice = $state<string | null>(null);
  let busy = $state(false);
  let closed = $state(false); // demo only: there is no window to hide
  let demoResult = $state<ci.ReadResult | null>(null);

  const bounds = $derived({ w: vw, h: vh });
  const box = $derived.by(() => {
    if (!dragging) return null;
    return mode === 'rect' ? (start && cur ? ci.dragBox(start, cur, bounds) : null) : ci.pointsBox(path, bounds);
  });
  const trail = $derived(path.map((p) => `${p.x},${p.y}`).join(' '));
  const big = $derived(ci.bigEnough(box));

  const modes: { id: Mode; label: string }[] = [
    { id: 'rect', label: 'Rectangle' },
    { id: 'free', label: 'Freehand' },
  ];

  function reset() {
    dragging = false;
    start = null;
    cur = null;
    path = [];
  }

  /** Called when this page mounts and each time Rust arms it again. Rust shows the window. */
  async function armAgain() {
    reset();
    notice = null;
    busy = false;
    closed = false;
    demoResult = null;
    arm = await ci.overlayReady();
  }

  async function cancel() {
    reset();
    if (demo) {
      closed = true;
      return;
    }
    await ci.cancelCircle();
  }

  function pt(e: PointerEvent): ci.Pt {
    return { x: e.clientX, y: e.clientY };
  }

  function onDown(e: PointerEvent) {
    if (busy) return;
    if ((e.target as HTMLElement).closest('[data-no-drag]')) return;
    if (e.button === 2) return; // right-click cancels (contextmenu)
    if (e.button !== 0) return;
    notice = null;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragging = true;
    start = pt(e);
    cur = start;
    path = [start];
  }

  function onMove(e: PointerEvent) {
    if (!dragging) return;
    cur = pt(e);
    if (mode === 'free') {
      const last = path[path.length - 1];
      const p = pt(e);
      // keep the path short: one point per ~3 px of travel
      if (!last || Math.hypot(p.x - last.x, p.y - last.y) >= 3) path = [...path, p];
    }
  }

  async function onUp(e: PointerEvent) {
    if (!dragging) return;
    cur = pt(e);
    if (mode === 'free') path = [...path, cur];
    const picked = mode === 'rect' ? ci.dragBox(start ?? cur, cur, bounds) : ci.pointsBox(path, bounds);
    reset();
    if (!ci.bigEnough(picked)) {
      notice = 'Drag around something a little larger.';
      return;
    }
    await finish(picked);
  }

  async function finish(picked: ci.Box) {
    const rect = ci.toPhysicalRect(picked, bounds, arm, window.devicePixelRatio);
    busy = true;
    try {
      const r = await ci.captureRegion(rect);
      if (demo) demoResult = r; // in the app Rust has already hidden this window and the panel shows the result
    } catch (e) {
      // Nothing was captured and the overlay is still up: say why, let the user try again or Esc.
      notice = ci.errorText(e);
    } finally {
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      void cancel();
    }
  }

  function onContext(e: MouseEvent) {
    e.preventDefault();
    void cancel();
  }

  onMount(() => {
    void armAgain();
    let off: (() => void) | undefined;
    void listen('circle-arm', () => void armAgain()).then((f) => (off = f));
    return () => off?.();
  });
</script>

<svelte:window bind:innerWidth={vw} bind:innerHeight={vh} onkeydown={onKey} />

{#if !closed}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="stage"
    class:holed={!!box}
    class:busy
    onpointerdown={onDown}
    onpointermove={onMove}
    onpointerup={onUp}
    onpointercancel={() => reset()}
    oncontextmenu={onContext}
    role="application"
    aria-label="Select an area of the screen to ask about. Press Escape to cancel.">
    {#if box}
      <div class="hole" class:ok={big} style:left="{box.x}px" style:top="{box.y}px" style:width="{box.w}px" style:height="{box.h}px"></div>
      <span class="size" style:left="{box.x}px" style:top="{Math.min(box.y + box.h + 8, vh - 28)}px">{Math.round(box.w)} × {Math.round(box.h)}</span>
    {/if}
    {#if mode === 'free' && dragging && path.length > 1}
      <svg class="trail" width={vw} height={vh} aria-hidden="true"><polyline points={trail} /></svg>
    {/if}

    <div class="hint" data-no-drag>
      <span class="msg">
        {#if notice}
          <b class="warn">{notice}</b>
        {:else if busy}
          Reading…
        {:else if mode === 'rect'}
          Drag around anything
        {:else}
          Draw a loop around anything
        {/if}
        <span class="dot">·</span> <kbd>Esc</kbd> to cancel
      </span>
      <Segmented label="Selection style" size="sm" options={modes} bind:value={mode} />
    </div>

    {#if demo && !demoResult}
      <p class="demo" data-no-drag>Demo: this page can’t see your screen. In the desktop app, the area you circle is read on your PC.</p>
    {/if}
  </div>
{/if}

{#if demo && demoResult}
  <div class="result" data-no-drag>
    <AskPanel result={demoResult} onclose={() => (closed = true)} />
  </div>
{/if}

<style>
  :global(html),
  :global(body) {
    background: transparent;
  }
  .stage {
    --veil: color-mix(in srgb, var(--page) 58%, transparent);
    position: fixed;
    inset: 0;
    overflow: hidden;
    cursor: crosshair;
    background: var(--veil);
    touch-action: none;
    user-select: none;
  }
  /* While a selection exists the hole is the only undimmed part: its shadow is the dim layer. */
  .stage.holed {
    background: transparent;
  }
  .stage.busy {
    cursor: progress;
  }
  .hole {
    position: absolute;
    box-sizing: border-box;
    border: 2px solid var(--line-strong);
    border-radius: 6px;
    box-shadow: 0 0 0 100vmax var(--veil);
    pointer-events: none;
  }
  .hole.ok {
    border-color: var(--accent);
  }
  .size {
    position: absolute;
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--pop-bg);
    color: var(--text);
    font-size: var(--t-xs);
    font-variant-numeric: tabular-nums;
    box-shadow: var(--shadow-pop);
    pointer-events: none;
  }
  .trail {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .trail polyline {
    fill: none;
    stroke: var(--accent);
    stroke-width: 3;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .hint {
    position: absolute;
    top: 22px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 8px 10px 8px 16px;
    border-radius: 999px;
    background: var(--pop-bg);
    color: var(--text);
    box-shadow: var(--shadow-pop);
    font-size: var(--t-sm);
    cursor: default;
    white-space: nowrap;
  }
  .msg {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .dot {
    color: var(--text-3);
  }
  .warn {
    color: var(--danger);
    font-weight: 600;
  }
  .demo {
    position: absolute;
    bottom: 28px;
    left: 50%;
    transform: translateX(-50%);
    max-width: 60ch;
    margin: 0;
    padding: 8px 14px;
    border-radius: var(--r-3);
    background: var(--pop-bg);
    color: var(--text-2);
    font-size: var(--t-xs);
    text-align: center;
    cursor: default;
  }
  .result {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 24px;
    pointer-events: none;
  }
  .result :global(.ask) {
    pointer-events: auto;
    width: min(560px, 100%);
    max-height: 100%;
    background: var(--pop-bg);
    box-shadow: var(--shadow-pop);
  }
</style>
