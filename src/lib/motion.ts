// Motion language for Krovi: spring presets for the Island morph, small math helpers,
// Svelte transitions and the card-spotlight action. Pure TS (no runes) so it is testable.

export const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));
export const lerp = (a: number, b: number, t: number): number => a + (b - a) * t;
/** Normalised position of v between a and b (unclamped). */
export const unlerp = (a: number, b: number, v: number): number => (b === a ? 1 : (v - a) / (b - a));
export const easeOutCubic = (t: number): number => 1 - Math.pow(1 - t, 3);
export const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

let mq: MediaQueryList | null = null;
/** True when the user asked the OS for reduced motion. Evaluated lazily, never cached. */
export function reduced(): boolean {
  if (typeof matchMedia !== 'function') return false;
  mq ??= matchMedia('(prefers-reduced-motion: reduce)');
  return mq.matches;
}

/**
 * Physical spring parameters: natural angular frequency `omega` (rad/s) and damping ratio
 * `zeta` (1 = critically damped, < 1 overshoots). The Island morph uses *different* springs for
 * width and height so the shape grows organically (wide first, then tall) instead of scaling.
 */
export interface SpringPhysics {
  omega: number;
  zeta: number;
}
export const SPRINGS = {
  /** pill -> panel width: one visible overshoot (~3%) that still fits the 30px window margin */
  openW: { omega: 17, zeta: 0.74 },
  /** pill -> panel height: a touch slower and more damped, so the drop lands softly */
  openH: { omega: 14.5, zeta: 0.8 },
  /** panel -> pill: critically damped, it settles like "returning home" and never pinches */
  closeW: { omega: 21, zeta: 1 },
  closeH: { omega: 19, zeta: 1 },
  /** pill <-> pill resizes (hover widen, live activity) */
  pill: { omega: 24, zeta: 0.75 },
} as const satisfies Record<string, SpringPhysics>;

/**
 * Closed-form damped oscillator. Position and velocity `t` seconds after being released from
 * displacement `d0` (relative to the target) with velocity `v0`. Exact at any `t`, so the motion
 * is identical at 30, 60 or 144 fps (explicit Euler integrators are not).
 */
export function springAt(p: SpringPhysics, d0: number, v0: number, t: number): { d: number; v: number } {
  const { omega: w, zeta: z } = p;
  if (z >= 1) {
    const e = Math.exp(-w * t);
    const b = v0 + w * d0;
    return { d: (d0 + b * t) * e, v: (v0 - w * b * t) * e };
  }
  const wd = w * Math.sqrt(1 - z * z);
  const e = Math.exp(-z * w * t);
  const c = Math.cos(wd * t);
  const s = Math.sin(wd * t);
  const b = (v0 + z * w * d0) / wd;
  return { d: e * (d0 * c + b * s), v: e * ((b * wd - z * w * d0) * c - (d0 * wd + z * w * b) * s) };
}

/** Peak excursion past the target (in the direction of travel) of a release from `from` to `to`. */
export function overshootPx(p: SpringPhysics, from: number, to: number): number {
  const dir = Math.sign(to - from) || 1;
  let worst = 0;
  for (let t = 0; t < 2; t += 1 / 240) worst = Math.max(worst, springAt(p, from - to, 0, t).d * dir);
  return worst;
}

/** Seconds until the motion stays within `eps` of the target. */
export function settleSeconds(p: SpringPhysics, from: number, to: number, eps = 0.5): number {
  let last = 0;
  for (let t = 0; t < 4; t += 1 / 240) if (Math.abs(springAt(p, from - to, 0, t).d) > eps) last = t;
  return last;
}

export interface SpringHandle {
  subscribe(run: (v: number) => void): () => void;
  /** Animate to `target`. Resolves true when it settled there, false if retargeted meanwhile. */
  set(target: number, opts?: { instant?: boolean }): Promise<boolean>;
  tune(p: SpringPhysics): void;
  readonly value: number;
}

/**
 * A spring-driven number as a Svelte-compatible store (`$sw` in templates). Retargeting keeps
 * the current position AND velocity, which is what makes mid-flight reversals feel physical.
 */
export function createSpring(initial: number, physics: SpringPhysics, precision = 0.2): SpringHandle {
  let p = physics;
  let x = initial;
  let target = initial;
  let d0 = 0;
  let v0 = 0;
  let t0 = 0;
  let raf = 0;
  let resolve: ((settled: boolean) => void) | null = null;
  const subs = new Set<(v: number) => void>();
  const emit = () => subs.forEach((fn) => fn(x));
  const now = () => performance.now() / 1000;

  function frame() {
    const t = now() - t0;
    const { d, v } = springAt(p, d0, v0, t);
    x = target + d;
    if (Math.abs(d) < precision && Math.abs(v) < precision * 12) {
      x = target;
      raf = 0;
      emit();
      const r = resolve;
      resolve = null;
      r?.(true);
      return;
    }
    emit();
    raf = requestAnimationFrame(frame);
  }

  return {
    subscribe(run) {
      subs.add(run);
      run(x);
      return () => subs.delete(run);
    },
    set(next, opts) {
      resolve?.(false); // whoever was waiting on the old target is released as "interrupted"
      resolve = null;
      if (opts?.instant || typeof requestAnimationFrame !== 'function') {
        if (raf) cancelAnimationFrame(raf);
        raf = 0;
        x = target = next;
        emit();
        return Promise.resolve(true);
      }
      // carry the live velocity into the new release
      let v = 0;
      if (raf) v = springAt(p, d0, v0, now() - t0).v;
      target = next;
      d0 = x - next;
      v0 = v;
      t0 = now();
      return new Promise<boolean>((res) => {
        resolve = res;
        if (!raf) raf = requestAnimationFrame(frame);
      });
    },
    tune(next) {
      p = next;
    },
    get value() {
      return x;
    },
  };
}

// ------------------------------------------------------------------ transitions

interface TransitionConfig {
  delay?: number;
  duration?: number;
  easing?: (t: number) => number;
  css?: (t: number, u: number) => string;
}

/** Tool switch: cross-fade with a small vertical slide and a blur that clears. */
export function swap(
  _node: Element,
  { y = 10, duration = 280, delay = 0 }: { y?: number; duration?: number; delay?: number } = {},
): TransitionConfig {
  if (reduced()) return { duration: 120, css: (t) => `opacity:${t}` };
  return {
    delay,
    duration,
    easing: easeOutCubic,
    css: (t, u) => `opacity:${t};transform:translateY(${(u * y).toFixed(2)}px);filter:blur(${(u * 5).toFixed(2)}px)`,
  };
}

/** New list rows (e.g. a fresh clip) drop in from above with a soft scale. */
export function arrive(_node: Element, { duration = 460 }: { duration?: number } = {}): TransitionConfig {
  if (reduced()) return { duration: 100, css: (t) => `opacity:${t}` };
  return {
    duration,
    easing: easeOutCubic,
    css: (t, u) =>
      `opacity:${t};transform:translateY(${(-u * 16).toFixed(2)}px) scale(${(0.96 + 0.04 * t).toFixed(4)});filter:blur(${(u * 4).toFixed(2)}px)`,
  };
}

/** Rows leaving: fade and collapse their height so neighbours glide up. */
export function depart(node: Element, { duration = 260 }: { duration?: number } = {}): TransitionConfig {
  const h = (node as HTMLElement).offsetHeight || 0;
  if (reduced()) return { duration: 80, css: (t) => `opacity:${t}` };
  return {
    duration,
    easing: easeOutCubic,
    css: (t) => `opacity:${t};height:${(h * t).toFixed(1)}px;overflow:hidden;transform:translateX(${((1 - t) * -14).toFixed(1)}px)`,
  };
}

/** Pop-in for popovers / the command palette. */
export function pop(
  _node: Element,
  { duration = 240, from = 0.96, y = -6 }: { duration?: number; from?: number; y?: number } = {},
): TransitionConfig {
  if (reduced()) return { duration: 100, css: (t) => `opacity:${t}` };
  return {
    duration,
    easing: easeOutCubic,
    css: (t, u) =>
      `opacity:${t};transform:translateY(${(u * y).toFixed(2)}px) scale(${(from + (1 - from) * t).toFixed(4)});filter:blur(${(u * 3).toFixed(2)}px)`,
  };
}

/** Digit roll (clocks, timers): the new glyph rises in, the old one lifts away. */
export function rollIn(_node: Element, { duration = 340 }: { duration?: number } = {}): TransitionConfig {
  if (reduced()) return { duration: 60, css: (t) => `opacity:${t}` };
  return {
    duration,
    easing: easeOutCubic,
    css: (t, u) => `opacity:${t};transform:translateY(${(u * 55).toFixed(1)}%);filter:blur(${(u * 3).toFixed(2)}px)`,
  };
}
export function rollOut(_node: Element, { duration = 260 }: { duration?: number } = {}): TransitionConfig {
  if (reduced()) return { duration: 60, css: (t) => `opacity:${t}` };
  return {
    duration,
    easing: easeOutCubic,
    css: (t, u) => `opacity:${t};transform:translateY(${(-u * 55).toFixed(1)}%);filter:blur(${(u * 3).toFixed(2)}px)`,
  };
}
// ------------------------------------------------------------------ actions

/** Cursor-following spotlight for glass cards: feeds --mx/--my consumed by `.card::before`. */
export function spotlight(node: HTMLElement): { destroy(): void } {
  node.classList.add('live');
  const move = (e: PointerEvent) => {
    const r = node.getBoundingClientRect();
    const k = node.offsetWidth ? r.width / node.offsetWidth : 1; // undo any ancestor scale()
    node.style.setProperty('--mx', `${(e.clientX - r.left) / (k || 1)}px`);
    node.style.setProperty('--my', `${(e.clientY - r.top) / (k || 1)}px`);
  };
  node.addEventListener('pointermove', move, { passive: true });
  return {
    destroy() {
      node.removeEventListener('pointermove', move);
    },
  };
}
