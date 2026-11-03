<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { listen, isTauri } from '../../lib/bridge';
  import { idleSettings, isIdle, personalityById, startIdle, styleById, type IdleStyleId, type PersonalityId } from '../../lib/idle';
  import { realEnv, runCompanion, type CursorEvent, type Runner } from './runner';
  import { stats } from '../../lib/live';
  import { reaction as reactionNow, noteCursorDistance, startReactions, type Reaction, type ReactionKind } from '../../lib/reactions';
  import { isBlockyTheme, resolvedTheme } from '../../lib/theme';
  import { initMood, nextMood, type Mood } from './mood';

  // The idle pill companion: a pair of original, animated eyes that fill the pill's content box
  // (the nearest positioned ancestor, e.g. the Island's `.shape`). docs/CONTRACTS.md §7.
  //
  //   <IdleCompanion active={phase === 'pill'} />
  //
  // `active` means "the collapsed pill is on screen". The companion adds the rest itself: it only
  // shows while `$idleSettings.enabled && $isIdle`, fades out in 90ms the moment activity is
  // marked, and does no work at all (no timer, no listener) while hidden.
  //
  // Cost bar: transform/opacity only; one setTimeout chain at human-scale intervals (runner.ts);
  // no rAF; stands down when the window is hidden or reduced motion is on.
  let {
    active,
    style: styleProp,
    personality: personalityProp,
    preview = false,
    forceReaction,
  }: {
    /** the collapsed pill is visible (false while the panel is open/morphing or the pill is hidden) */
    active: boolean;
    /** override the saved style (previews) */
    style?: IdleStyleId;
    /** override the saved personality (previews) */
    personality?: PersonalityId;
    /** ignore the idle clock and settings and just run (the preview harness) */
    preview?: boolean;
    /** previews only: hold this reaction (null = none) instead of following the live ones */
    forceReaction?: ReactionKind | null;
  } = $props();

  const style = $derived(styleProp ?? $idleSettings.style);
  const personality = $derived(personalityProp ?? $idleSettings.personality);
  const shown = $derived(active && (preview || ($idleSettings.enabled && $isIdle)));

  // Quick reactions (curious / focused / success / error, src/lib/reactions.ts): transient
  // expressions over the mood. Previews ignore the live ones so a settings well never twitches
  // because a timer rang; `forceReaction` lets the harness hold one on purpose.
  const rx = $derived<Reaction | null>(
    forceReaction !== undefined
      ? forceReaction
        ? { kind: forceReaction, still: realEnv.reducedMotion() }
        : null
      : preview
        ? null
        : $reactionNow,
  );
  let root: HTMLElement | undefined = $state();
  let runner: Runner | null = null;

  // The skin the eyes are drawn in: the live theme, or the per-theme preview board (the harness's
  // data-theme-scope) the companion sits in. Only the blocky themes (Voxel, Minecraft) change the
  // eye SHAPE (square eyes, pixel pupil, gaze snapped to a coarse grid); every other theme is
  // colour only, via the --eye-* tokens.
  let scope = $state<string | null>(null);
  onMount(() => {
    scope = root?.closest('[data-theme-scope]')?.getAttribute('data-theme-scope') ?? null;
  });
  const blocky = $derived(isBlockyTheme(scope ?? $resolvedTheme));

  // System-aware mood (mood.ts): only from REAL stats in the native app; the browser demo's
  // stats are simulated, so they never drive an expression. Folding a sample is a few
  // arithmetic ops every 2 s; the attribute only changes when the mood does.
  let moodState = initMood(Date.now());
  let mood = $state<Mood>('neutral');

  onMount(() => {
    if (!isTauri() || preview) return;
    return stats.subscribe((s) => {
      if (!s) return;
      moodState = nextMood(moodState, s, Date.now());
      if (moodState.mood !== mood) mood = moodState.mood;
    });
  });

  onMount(() => {
    if (preview) return;
    startIdle();
    return startReactions(); // the built-in reaction sources, for as long as this companion lives
  });

  $effect(() => {
    if (!shown || !root) return;
    const el = root;
    // Read the style/personality lazily on every step: a settings change applies on the next
    // saccade instead of restarting the eyes.
    const read = () => {
      const s = styleById(style);
      // Blocky eyes are square pixels: round the gaze to half-steps (the visor sweeps, so it keeps gliding).
      const traits = blocky && s.id !== 'visor' ? { ...s, quantize: Math.max(s.quantize, 2) } : s;
      return { personality: personalityById(personality), style: traits };
    };
    const r = untrack(() => runCompanion(el, read));
    runner = r;
    // Only the live companion feeds the shared reaction state (previews would fight over it).
    const feed = (e: CursorEvent) => {
      if (!preview) noteCursorDistance(e.dist ?? Math.hypot(e.dx, e.dy));
    };

    // Native: Rust emits `shell-cursor` (<=10 Hz, anywhere on screen, only in pill mode).
    // Browser demo: there is no Rust side, so follow the pointer ourselves, throttled to 10 Hz.
    let off: (() => void) | null = null;
    let dead = false;
    let onMove: ((e: PointerEvent) => void) | null = null;
    if (isTauri()) {
      void listen<CursorEvent>('shell-cursor', (e) => {
        r.cursor(e);
        feed(e);
      }).then((u) => {
        if (dead) u();
        else off = u;
      });
    } else {
      let last = 0;
      onMove = (e) => {
        if (e.timeStamp - last < 100) return;
        last = e.timeStamp;
        const b = el.getBoundingClientRect();
        const dx = e.clientX - (b.left + b.width / 2);
        const dy = e.clientY - (b.top + b.height / 2);
        const ev = { dx, dy, dist: Math.hypot(dx, dy) };
        r.cursor(ev);
        feed(ev);
      };
      window.addEventListener('pointermove', onMove, { passive: true });
    }

    return () => {
      dead = true;
      off?.();
      if (onMove) window.removeEventListener('pointermove', onMove);
      if (!preview) noteCursorDistance(null); // eyes gone: the cursor is no longer "near"
      r.stop();
      if (runner === r) runner = null;
    };
  });

  // The error shake: ONE stepped Web Animation on the face (transform only, added on top of the
  // face's own transform), started when the error reaction begins. Skipped under reduced motion,
  // which only keeps the squint. The stylesheet itself never animates (costbar.test.ts).
  const SHAKE_X = [0, -1.6, 1.6, -1.2, 1.2, -0.6, 0.6, 0]; // cqh
  $effect(() => {
    if (!shown || !root || rx?.kind !== 'error' || rx.still) return;
    const face = root.querySelector<HTMLElement>('.face');
    face?.animate?.(
      SHAKE_X.map((x, i) => ({
        transform: `translateX(${x}cqh)`,
        offset: i / (SHAKE_X.length - 1),
        easing: 'steps(1, jump-end)', // hold each position until the next keyframe
      })),
      { duration: 380, easing: 'linear', composite: 'add' },
    );
  });

  /** Point the eyes at the cursor (dx, dy = logical px from the pill centre). The native app does this via `shell-cursor`. */
  export function glance(ev: CursorEvent): void {
    runner?.cursor(ev);
  }
</script>

<div
  class="idle"
  class:on={shown}
  data-style={style}
  data-skin={blocky ? 'blocky' : undefined}
  data-mood={rx || mood === 'neutral' ? undefined : mood}
  data-react={rx?.kind}
  data-react-still={rx?.still ? '' : undefined}
  bind:this={root}
  aria-hidden="true"
>
  <div class="face">
    {#each ['l', 'r'] as side (side)}
      <div class="eye {side}">
        <i class="cheek"></i>
        <div class="gaze">
          <i class="glow"></i>
          <div class="lid"><i class="shape"></i></div>
          <i class="arc"></i>
        </div>
      </div>
    {/each}
  </div>
</div>

<style>
  /* ---- the stage. Everything is sized in cqh so the eyes scale with the pill's height. ---- */
  .idle {
    --gap: 60cqh;
    --ew: 40cqh;
    --eh: 40cqh;
    --er: 50%;
    --ax: 11cqh; /* how far a full-scale gaze moves the eye */
    --ay: 6cqh;
    --arc-w: 4.4cqh;
    --arc-h: 20cqh;
    --ease: var(--ease-out, cubic-bezier(0.22, 1, 0.36, 1));
    /* Every move is short and STEPPED, so the compositor draws one frame per step instead of one
       per vsync. That is what keeps the companion near zero cost. */
    --look: steps(var(--look-steps, 1), jump-start); /* gaze shift; --look-ms/--look-steps set per personality */
    --pose-t: 100ms steps(3, jump-end); /* a pose moving in or out */
    --fade-t: 60ms steps(2, jump-end); /* cross-fades (eye <-> arc, cheeks) */

    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    border-radius: inherit;
    overflow: hidden;
    pointer-events: none;
    container-type: size;
    opacity: 0;
    visibility: hidden;
    transition:
      opacity 90ms linear,
      visibility 0s linear 90ms;
  }
  .idle.on {
    opacity: 1;
    visibility: visible;
    transition: opacity 340ms var(--ease);
  }

  .face {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--gap);
    transform: translateY(var(--face-y, 0px)) rotate(var(--face-rot, 0deg)) scale(var(--face-s, 1));
    transition: transform var(--pose-t);
  }

  .eye {
    position: relative;
    flex: none;
    width: var(--ew);
    height: var(--eh);
  }
  .gaze {
    position: absolute;
    inset: 0;
    transform: translate(calc(var(--gx, 0) * var(--ax)), calc(var(--gy, 0) * var(--ay)));
    transition: transform var(--look-ms, 80ms) var(--look);
  }
  .glow {
    position: absolute;
    inset: -55%;
    border-radius: 50%;
    background: radial-gradient(closest-side, var(--eye-glow, transparent), transparent);
  }
  .lid {
    position: absolute;
    inset: 0;
    transform-origin: 50% 70%;
  }
  /* While the eyes are on screen the moving parts keep their compositor layers (a handful of
     tiny ones) instead of paying to create and destroy them on every move. */
  .idle.on .face,
  .idle.on .gaze,
  .idle.on .lid {
    will-change: transform;
  }
  .idle.on .shape,
  .idle.on .arc {
    will-change: transform, opacity;
  }
  .shape {
    position: absolute;
    inset: 0;
    background: var(--eye-fill);
    border-radius: var(--er);
    transform-origin: 50% 100%;
    transition:
      transform var(--pose-t),
      opacity var(--fade-t);
  }
  /* happy / dozing eyes are drawn as arcs that the round eye cross-fades into */
  .arc {
    position: absolute;
    left: 6%;
    right: 6%;
    top: 26%;
    height: var(--arc-h);
    border: var(--arc-w) solid var(--eye-fill);
    border-bottom: 0;
    border-radius: 999px 999px 0 0;
    opacity: 0;
    transform: translateY(14%) scale(0.92);
    transition:
      opacity var(--fade-t),
      transform var(--pose-t);
  }
  .cheek {
    position: absolute;
    left: 8%;
    right: 8%;
    top: 82%;
    height: 36%;
    border-radius: 50%;
    background: var(--eye-blush, transparent);
    opacity: 0;
    transition: opacity var(--fade-t);
  }

  /* ---- moods (mood.ts, from real system stats). A resting expression under the poses: a pose
     (blink-free transforms on .shape/.face) wins while it plays. Transforms and opacity only. ---- */
  .idle[data-mood='sad']:not([data-pose]) .shape {
    transform: scaleY(0.62); /* heavy lids */
  }
  .idle[data-mood='sad']:not([data-pose]) .face {
    --face-y: 0.9cqh;
  }
  .idle[data-mood='stressed']:not([data-pose]) .eye.l .shape {
    transform: scaleY(0.6) rotate(10deg); /* narrowed, inner corners down */
  }
  .idle[data-mood='stressed']:not([data-pose]) .eye.r .shape {
    transform: scaleY(0.6) rotate(-10deg);
  }
  .idle[data-mood='calm']:not([data-pose]) .shape {
    transform: scaleY(0.86);
  }
  .idle[data-mood='calm']:not([data-pose]) .face {
    --face-y: 0.3cqh;
  }
  .idle[data-mood='happy']:not([data-pose]):not([data-style='visor']) .shape {
    opacity: 0;
  }
  .idle[data-mood='happy']:not([data-pose]):not([data-style='visor']) .arc {
    opacity: 1;
    transform: none;
  }
  .idle[data-mood='happy']:not([data-pose]) .cheek {
    opacity: 1;
  }

  /* ---- poses. Set as `data-pose` on the root by runner.ts; transforms and opacity only. ---- */
  .idle:global([data-pose='droop']) .shape {
    transform: scaleY(0.5);
  }
  .idle:global([data-pose='droop']) .face {
    --face-y: 0.8cqh;
  }
  .idle:global([data-pose='tilt']) .face {
    --face-rot: calc(var(--side, 1) * -6deg);
  }
  .idle:global([data-pose='tilt']) .eye:last-child .shape {
    transform: scaleY(calc(1 + 0.13 * var(--side, 1)));
  }
  .idle:global([data-pose='tilt']) .eye:first-child .shape {
    transform: scaleY(calc(1 - 0.13 * var(--side, 1)));
  }
  .idle:global([data-pose='perk']) .face {
    --face-s: 1.13;
    --face-y: -0.8cqh;
    transition: transform 90ms steps(3, jump-end);
  }
  .idle:global([data-pose='happy']):not([data-style='visor']) .shape,
  .idle:global([data-pose='doze']):not([data-style='visor']) .shape {
    opacity: 0;
  }
  .idle:global([data-pose='happy']):not([data-style='visor']) .arc,
  .idle:global([data-pose='doze']):not([data-style='visor']) .arc {
    opacity: 1;
    transform: none;
  }
  .idle:global([data-pose='doze']):not([data-style='visor']) .arc {
    transform: translateY(26%) rotate(180deg); /* ∪ : a closed, sleeping eye */
  }
  .idle:global([data-pose='happy']) .cheek,
  .idle:global([data-pose='doze']) .cheek {
    opacity: 1;
  }
  .idle:global([data-pose='happy']) .face {
    --face-y: -0.6cqh;
  }
  .idle:global([data-pose='doze']) .face {
    --face-y: 1cqh;
  }

  /* ---- quick reactions (lib/reactions.ts): transient expressions over mood and pose. `[data-react]`
     is doubled in every selector on purpose: it lifts them above the mood and pose rules (which carry
     :not([data-pose]) and extra attributes), so a reaction always wins while it lasts and the eyes fall
     back to their mood the moment it ends. Shape and transform only; the shake uses the individual
     `translate` property so it composes with the face's own transform. ---- */

  /* success: happy arcs, cheeks, a small lift */
  .idle[data-react='success'][data-react]:not([data-style='visor']) .shape {
    opacity: 0;
  }
  .idle[data-react='success'][data-react]:not([data-style='visor']) .arc {
    opacity: 1;
    transform: none;
  }
  .idle[data-react='success'][data-react] .cheek {
    opacity: 1;
  }
  .idle[data-react='success'][data-react] .face {
    --face-y: -0.7cqh;
    --face-s: 1.05;
    --face-rot: 0deg;
  }
  .idle[data-react='success'][data-react][data-style='visor'] {
    --core-s: 2.7;
  }

  /* error: a "> <" squint (two chevrons; ring eyes just slant). The brief shake is one Web
     Animation started from the script (like the blink): the stylesheet never animates. */
  .idle[data-react='error'][data-react] .face {
    --face-y: 0.4cqh;
    --face-s: 1;
    --face-rot: 0deg;
  }
  .idle[data-react='error'][data-react]:not([data-style='visor']):not([data-style='ring']) .eye.l .shape {
    clip-path: polygon(0 0, 30% 0, 100% 50%, 30% 100%, 0 100%, 70% 50%);
  }
  .idle[data-react='error'][data-react]:not([data-style='visor']):not([data-style='ring']) .eye.r .shape {
    clip-path: polygon(100% 0, 70% 0, 0 50%, 70% 100%, 100% 100%, 30% 50%);
  }
  .idle[data-react='error'][data-react][data-style='ring'] .eye.l .shape {
    transform: scaleY(0.6) rotate(16deg);
  }
  .idle[data-react='error'][data-react][data-style='ring'] .eye.r .shape {
    transform: scaleY(0.6) rotate(-16deg);
  }
  .idle[data-react='error'][data-react][data-style='visor'] {
    --core-s: 0.5;
  }
  /* curious: wider eyes and a small head tilt */
  .idle[data-react='curious'][data-react]:not([data-style='visor']) .shape {
    transform: scale(1.16);
  }
  .idle[data-react='curious'][data-react] .face {
    --face-y: -0.5cqh;
    --face-rot: -5deg;
  }
  .idle[data-react='curious'][data-react][data-style='visor'] {
    --core-s: 1.5;
  }

  /* focused: eyes narrowed to level slits */
  .idle[data-react='focused'][data-react]:not([data-style='visor']) .shape {
    transform: scaleY(0.52);
  }
  .idle[data-react='focused'][data-react] .face {
    --face-y: 0.2cqh;
    --face-rot: 0deg;
  }
  .idle[data-react='focused'][data-react][data-style='visor'] {
    --core-s: 0.5;
  }

  /* reduced motion: the expression still changes (that is a shape, not movement), but nothing
     shakes (the script skips the Web Animation), lifts or tilts, and nothing eases. Must stay
     after the reaction rules above (same specificity, later wins). */
  .idle[data-react-still][data-react] .face {
    --face-y: 0cqh;
    --face-s: 1;
    --face-rot: 0deg;
    transition-duration: 0s;
  }
  .idle[data-react-still][data-react] .shape,
  .idle[data-react-still][data-react] .arc,
  .idle[data-react-still][data-react] .cheek {
    transition-duration: 0s;
  }

  /* ================= styles: each one only sets sizes, shapes and the odd pseudo-element ================= */

  /* Dot: round eyes with a glint */
  .idle[data-style='dot'] .shape::after {
    content: '';
    position: absolute;
    top: 14%;
    right: 18%;
    width: 24%;
    height: 24%;
    border-radius: 50%;
    background: var(--eye-accent);
    opacity: 0.9;
  }

  /* Capsule: tall rounded bars */
  .idle[data-style='capsule'] {
    --ew: 24cqh;
    --eh: 52cqh;
    --er: 999px;
    --gap: 44cqh;
    --ax: 9cqh;
    --ay: 4cqh;
    --arc-h: 16cqh;
    --arc-w: 4cqh;
  }

  /* Sleepy: flat-lidded, a little tired, outer corners low */
  .idle[data-style='sleepy'] {
    --ew: 46cqh;
    --eh: 30cqh;
    --er: 12% 12% 50% 50% / 20% 20% 80% 80%;
    --gap: 50cqh;
    --ax: 8cqh;
    --ay: 3cqh;
    --arc-h: 15cqh;
    --arc-w: 4cqh;
  }
  .idle[data-style='sleepy'] .eye:first-child {
    transform: rotate(6deg);
  }
  .idle[data-style='sleepy'] .eye:last-child {
    transform: rotate(-6deg);
  }

  /* Pixel: a 4x5 cell eye with notched corners; the gaze snaps to the grid (see IDLE_STYLES) */
  .idle[data-style='pixel'] {
    --cell: 4px;
    --ew: calc(var(--cell) * 4);
    --eh: calc(var(--cell) * 5);
    --er: 0;
    --gap: calc(var(--cell) * 6);
    --ax: calc(var(--cell) * 2);
    --ay: calc(var(--cell) * 2);
    --arc-w: var(--cell);
    --arc-h: calc(var(--cell) * 3);
  }
  .idle[data-style='pixel'] .shape {
    clip-path: polygon(
      25% 0,
      75% 0,
      75% 20%,
      100% 20%,
      100% 80%,
      75% 80%,
      75% 100%,
      25% 100%,
      25% 80%,
      0 80%,
      0 20%,
      25% 20%
    );
  }
  .idle[data-style='pixel'] .shape::after {
    content: '';
    position: absolute;
    left: 50%;
    top: 20%;
    width: 25%;
    height: 20%;
    background: var(--eye-accent);
  }
  .idle[data-style='pixel'] .arc {
    border-radius: 0;
  }
  .idle[data-style='pixel'] .cheek {
    border-radius: 0;
    top: 86%;
    height: 22%;
  }

  /* Visor: one band; a bright core sweeps along a dim track */
  .idle[data-style='visor'] {
    --ew: min(46cqw, 250cqh);
    --eh: 22cqh;
    --er: 999px;
    --gap: 0px;
    --ax: 0px;
    --ay: 2cqh;
    --core-s: 1;
  }
  .idle[data-style='visor'] .eye:last-child,
  .idle[data-style='visor'] .arc,
  .idle[data-style='visor'] .cheek {
    display: none;
  }
  .idle[data-style='visor'] .shape {
    background: var(--eye-fill-2);
    overflow: hidden;
  }
  .idle[data-style='visor'] .shape::after {
    content: '';
    position: absolute;
    top: 0;
    bottom: 0;
    left: 33%;
    width: 34%;
    border-radius: 999px;
    background: var(--eye-fill);
    transform: translateX(calc(var(--gx, 0) * 94%)) scaleX(var(--core-s));
    transition: transform var(--look-ms, 80ms) var(--look);
  }
  .idle[data-style='visor']:global([data-pose='happy']) {
    --core-s: 2.7;
  }
  .idle[data-style='visor']:global([data-pose='perk']) {
    --core-s: 1.5;
  }
  .idle[data-style='visor']:global([data-pose='droop']) {
    --core-s: 0.5;
  }
  .idle[data-style='visor']:global([data-pose='doze']) {
    --core-s: 0.28;
  }

  /* Ring: hollow rings, the pupil roams inside */
  .idle[data-style='ring'] {
    --ew: 42cqh;
    --eh: 42cqh;
    --gap: 56cqh;
    --ax: 3cqh;
    --ay: 2cqh;
    --arc-h: 21cqh;
  }
  .idle[data-style='ring'] .shape {
    background: transparent;
    border: 3.4cqh solid var(--eye-fill);
  }
  .idle[data-style='ring'] .shape::after {
    content: '';
    position: absolute;
    left: 50%;
    top: 50%;
    width: 34%;
    height: 34%;
    margin: -17% 0 0 -17%;
    border-radius: 50%;
    background: var(--eye-accent);
    transform: translate(calc(var(--gx, 0) * 62%), calc(var(--gy, 0) * 62%));
    transition: transform var(--look-ms, 80ms) var(--look);
  }

  /* ================= Blocky skin (Voxel and Minecraft themes): square eyes, pixel pupil =================
     `data-skin='blocky'` is stamped by the script from the live theme, or from the preview board's
     data-theme-scope, so a board shows its own theme's eyes whatever the page theme is. It sits on top
     of whichever style is chosen (Pixel is already blocky and the Visor is a bar, so they keep their
     shapes, in the theme's colours). The gaze is the existing mechanism: the pupil rides --gx/--gy, and
     the script rounds the gaze to half-steps under this skin so it hops from pixel to pixel. */
  .idle[data-skin='blocky']:not([data-style='visor']):not([data-style='pixel']) {
    --er: 0px;
  }
  .idle[data-skin='blocky']:not([data-style='visor']):not([data-style='pixel']) .arc,
  .idle[data-skin='blocky']:not([data-style='visor']):not([data-style='pixel']) .cheek {
    border-radius: 0;
  }
  /* the pupil: a dark square (the pill colour) that slides about inside the pale square */
  .idle[data-skin='blocky']:not([data-style='visor']):not([data-style='pixel']):not([data-style='ring']) .shape::after {
    content: '';
    position: absolute;
    left: 50%;
    top: 50%;
    right: auto;
    bottom: auto;
    width: 38%;
    height: auto;
    aspect-ratio: 1;
    margin: -19% 0 0 -19%;
    border-radius: 0;
    background: var(--pill-bg);
    opacity: 1;
    transform: translate(calc(var(--gx, 0) * 62%), calc(var(--gy, 0) * 62%));
    transition: transform var(--look-ms, 80ms) var(--look);
  }
  /* ring eyes keep their (accent-coloured) pupil, squared off */
  .idle[data-skin='blocky'][data-style='ring'] .shape::after {
    border-radius: 0;
  }
  /* the "> <" error squint is clipped to chevrons: the pupil would be cropped, so it steps aside */
  .idle[data-skin='blocky'][data-react='error'][data-react]:not([data-style='visor']):not([data-style='pixel']):not([data-style='ring']) .shape::after {
    opacity: 0;
  }
</style>
