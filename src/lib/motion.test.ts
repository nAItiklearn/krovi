import { describe, it, expect } from 'vitest';
import {
  SPRINGS,
  clamp,
  lerp,
  unlerp,
  easeOutCubic,
  springAt,
  overshootPx,
  settleSeconds,
  reduced,
} from './motion';
import { PANEL, PANEL_WIN, PILL_WIN } from './shell';

describe('math helpers', () => {
  it('clamps, lerps and un-lerps', () => {
    expect(clamp(5, 0, 3)).toBe(3);
    expect(clamp(-1, 0, 3)).toBe(0);
    expect(lerp(10, 20, 0.5)).toBe(15);
    expect(unlerp(10, 20, 15)).toBe(0.5);
    expect(unlerp(4, 4, 9)).toBe(1); // degenerate range never divides by zero
  });

  it('eases out monotonically from 0 to 1', () => {
    expect(easeOutCubic(0)).toBe(0);
    expect(easeOutCubic(1)).toBe(1);
    expect(easeOutCubic(0.5)).toBeGreaterThan(0.5);
  });

  it('reports "no reduced motion" outside a browser', () => {
    expect(reduced()).toBe(false);
  });
});

describe('springAt (closed form)', () => {
  it('starts at the release displacement and converges on the target', () => {
    const p = SPRINGS.openW;
    expect(springAt(p, 100, 0, 0).d).toBeCloseTo(100, 6);
    expect(Math.abs(springAt(p, 100, 0, 3).d)).toBeLessThan(1e-6);
  });

  it('is continuous in velocity: the numeric derivative matches v', () => {
    const p = SPRINGS.openH;
    const h = 1e-5;
    for (const t of [0.05, 0.2, 0.4]) {
      const a = springAt(p, 80, 12, t - h).d;
      const b = springAt(p, 80, 12, t + h).d;
      expect((b - a) / (2 * h)).toBeCloseTo(springAt(p, 80, 12, t).v, 2);
    }
  });

  it('critically damped springs never overshoot', () => {
    expect(overshootPx(SPRINGS.closeW, 900, 261)).toBeCloseTo(0, 6);
    expect(overshootPx(SPRINGS.closeH, 620, 36)).toBeCloseTo(0, 6);
  });
});

describe('Island morph budget', () => {
  const sideMargin = (PANEL_WIN.w - PANEL.w) / 2;
  const bottomMargin = PANEL_WIN.h - PANEL.h - 10;

  it('width overshoot stays inside the transparent window margin', () => {
    const over = overshootPx(SPRINGS.openW, 262, PANEL.w);
    expect(over).toBeGreaterThan(5); // it does visibly spring...
    expect(over / 2).toBeLessThan(sideMargin); // ...without being clipped by the window
  });

  it('height overshoot leaves room for the shadow below the panel', () => {
    const over = overshootPx(SPRINGS.openH, 36, PANEL.h);
    expect(over).toBeGreaterThan(2);
    expect(over).toBeLessThan(bottomMargin / 2);
  });

  it('opens and closes inside half a second of motion', () => {
    expect(settleSeconds(SPRINGS.openW, 262, PANEL.w, 1)).toBeLessThan(0.7);
    expect(settleSeconds(SPRINGS.openH, 36, PANEL.h, 1)).toBeLessThan(0.7);
    expect(settleSeconds(SPRINGS.closeW, PANEL.w, 262, 1)).toBeLessThan(0.6);
    expect(settleSeconds(SPRINGS.closeH, PANEL.h, 36, 1)).toBeLessThan(0.6);
  });

  it('the pill window can hold a max-width pill with its glow', () => {
    expect(PILL_WIN.w - 400).toBeGreaterThanOrEqual(40);
  });
});
