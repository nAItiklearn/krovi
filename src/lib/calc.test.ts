import { describe, it, expect } from 'vitest';
import { evaluate, tryEvaluate, CalcError } from './calc';

describe('calculator evaluate', () => {
  it('adds and subtracts', () => {
    expect(evaluate('1 + 2 + 3')).toBe(6);
    expect(evaluate('10 - 4 - 1')).toBe(5);
  });

  it('respects operator precedence', () => {
    expect(evaluate('2 + 3 * 4')).toBe(14);
    expect(evaluate('2 * 3 + 4')).toBe(10);
  });

  it('handles parentheses', () => {
    expect(evaluate('(2 + 3) * 4')).toBe(20);
    expect(evaluate('2 * (3 + (4 - 1))')).toBe(12);
  });

  it('handles unary minus and decimals', () => {
    expect(evaluate('-5 + 2')).toBe(-3);
    expect(evaluate('3.5 * 2')).toBe(7);
    expect(evaluate('-(2 + 3)')).toBe(-5);
  });

  it('treats % as divide-by-100', () => {
    expect(evaluate('50%')).toBe(0.5);
    expect(evaluate('200 * 10%')).toBeCloseTo(20);
  });

  it('accepts × and ÷ symbols', () => {
    expect(evaluate('6 × 7')).toBe(42);
    expect(evaluate('84 ÷ 2')).toBe(42);
  });

  it('throws on division by zero', () => {
    expect(() => evaluate('1 / 0')).toThrow(CalcError);
  });

  it('throws on malformed input', () => {
    expect(() => evaluate('2 +')).toThrow();
    expect(() => evaluate('(1 + 2')).toThrow();
    expect(() => evaluate('abc')).toThrow();
    expect(() => evaluate('')).toThrow();
  });

  it('tryEvaluate reports ok/err without throwing', () => {
    expect(tryEvaluate('2+2')).toEqual({ ok: true, value: 4 });
    const bad = tryEvaluate('2 /');
    expect(bad.ok).toBe(false);
  });
});
