import { describe, expect, it } from 'vitest';

// The idle companion's hard cost bar, enforced against the source so it cannot regress silently:
//   - only transform/opacity are ever animated or transitioned (visibility only as a zero-length
//     switch that fires after an opacity fade)
//   - no requestAnimationFrame / setInterval loop anywhere in the companion's runtime files
//   - at most the two named timers exist (idle countdown + the engine chain)

const nodeFs = 'node:fs';
const fs = await import(/* @vite-ignore */ nodeFs);
const dir = new URL('./', import.meta.url);
const read = (rel: string): string => fs.readFileSync(new URL(rel, dir), 'utf8');

const svelte = read('IdleCompanion.svelte');
const css = svelte.slice(svelte.indexOf('<style>'));

const ANIMATABLE = new Set(['transform', 'opacity']);

/** Split on top-level commas (cubic-bezier(...) has commas inside parentheses). */
function topLevelCommas(s: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let cur = '';
  for (const ch of s) {
    if (ch === '(') depth++;
    if (ch === ')') depth--;
    if (ch === ',' && depth === 0) {
      out.push(cur);
      cur = '';
    } else cur += ch;
  }
  out.push(cur);
  return out.map((x) => x.trim()).filter(Boolean);
}

describe('idle companion cost bar', () => {
  it('only transitions transform and opacity (plus a delayed visibility switch)', () => {
    const decls = [...css.matchAll(/(?<![-\w])transition\s*:\s*([^;]+);/g)].map((m) => m[1]);
    expect(decls.length).toBeGreaterThan(5);
    for (const decl of decls) {
      for (const part of topLevelCommas(decl)) {
        const prop = part.split(/\s+/)[0];
        if (prop === 'visibility') {
          // must be the zero-duration, delayed form: `visibility 0s linear 90ms`
          expect(part, `visibility transition: ${part}`).toMatch(/^visibility 0s /);
        } else {
          expect(ANIMATABLE.has(prop), `transition of "${prop}" in "${decl}"`).toBe(true);
        }
      }
    }
  });

  it('never uses transition-property, and will-change only names transform/opacity', () => {
    expect(css).not.toMatch(/transition-property/);
    const wc = [...css.matchAll(/will-change\s*:\s*([^;]+);/g)].map((m) => m[1]);
    expect(wc.length).toBeGreaterThan(0);
    for (const decl of wc) {
      for (const prop of topLevelCommas(decl)) expect(ANIMATABLE.has(prop), `will-change: ${prop}`).toBe(true);
    }
  });

  it('only animates transform in any css keyframes (the blink/wake use Web Animations)', () => {
    const blocks = [...css.matchAll(/@keyframes\s+[\w-]+\s*\{([\s\S]*?\})\s*\}/g)].map((m) => m[1]);
    for (const body of blocks) {
      const props = [...body.matchAll(/([a-z-]+)\s*:/g)].map((m) => m[1]);
      for (const p of props) expect(ANIMATABLE.has(p), `@keyframes animates "${p}"`).toBe(true);
    }
    // no css animation at all on the eye lids: only WAAPI touches them
    expect(css).not.toMatch(/\banimation\s*:/);
  });

  it('every Web Animation the runner starts animates transform only', () => {
    const src = read('runner.ts');
    const frames = [...src.matchAll(/\{\s*transform:/g)].length;
    expect(frames).toBeGreaterThan(5);
    // no keyframe object that starts with any other property
    expect(src).not.toMatch(/\{\s*(opacity|filter|width|height|top|left|right|bottom|background|boxShadow)\s*:/);
  });

  it('has no infinite css animation (a frame stream even when nothing moves)', () => {
    expect(css).not.toMatch(/infinite/);
  });

  it.each(['engine.ts', 'runner.ts', 'IdleCompanion.svelte', '../../lib/idle.ts'])(
    '%s has no frame loop and no polling interval',
    (file) => {
      const src = read(file);
      expect(src).not.toMatch(/requestAnimationFrame/);
      expect(src).not.toMatch(/setInterval/);
    },
  );

  it('creates timers in exactly two places: the idle countdown and the engine chain', () => {
    // the real call sites only: `=> setTimeout(` (`env.setTimeout(` is the runner re-arming
    // through its injected env, counted separately below)
    const count = (file: string) => (read(file).match(/=>\s*setTimeout\s*\(/g) ?? []).length;
    // idle.ts: the countdown (inside the TimerApi it hands the machine)
    expect(count('../../lib/idle.ts')).toBe(1);
    // runner.ts: the env adapter + the one the chain re-arms through env.setTimeout
    expect(count('runner.ts')).toBe(1);
    expect((read('runner.ts').match(/env\.setTimeout\s*\(/g) ?? []).length).toBe(2); // first arm + re-arm
    expect(count('IdleCompanion.svelte')).toBe(0);
  });

  it('listens to the cursor only while the eyes are shown', () => {
    // the subscription lives inside the `shown` effect, so it is torn down with the eyes
    const effect = svelte.slice(svelte.indexOf('$effect('), svelte.indexOf('export function glance'));
    expect(effect).toContain('if (!shown || !root) return;');
    expect(effect).toContain("listen<CursorEvent>('shell-cursor'");
    expect(effect).toContain('r.stop()');
  });
});
