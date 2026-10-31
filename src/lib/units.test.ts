import { describe, it, expect } from 'vitest';
import { convert, convertCurrency, roundDisplay } from './units';

describe('unit conversion', () => {
  it('converts length through the metre base', () => {
    expect(convert('length', 1, 'km', 'm')).toBe(1000);
    expect(convert('length', 100, 'cm', 'm')).toBe(1);
    expect(roundDisplay(convert('length', 1, 'mi', 'km'))).toBeCloseTo(1.6093, 3);
    expect(roundDisplay(convert('length', 12, 'in', 'ft'))).toBe(1);
  });

  it('converts mass through the gram base', () => {
    expect(convert('mass', 1, 'kg', 'g')).toBe(1000);
    expect(roundDisplay(convert('mass', 1, 'lb', 'kg'))).toBeCloseTo(0.4536, 3);
    expect(roundDisplay(convert('mass', 16, 'oz', 'lb'))).toBeCloseTo(1, 3);
  });

  it('converts temperature with affine formulas', () => {
    expect(convert('temperature', 100, 'C', 'F')).toBe(212);
    expect(convert('temperature', 32, 'F', 'C')).toBe(0);
    expect(convert('temperature', 0, 'C', 'K')).toBeCloseTo(273.15);
    expect(convert('temperature', 273.15, 'K', 'C')).toBeCloseTo(0);
  });

  it('round-trips a conversion', () => {
    const there = convert('length', 5, 'ft', 'm');
    expect(roundDisplay(convert('length', there, 'm', 'ft'))).toBe(5);
  });

  it('converts currency through USD', () => {
    const rates = { USD: 1, EUR: 0.92, INR: 83 };
    expect(roundDisplay(convertCurrency(100, 'USD', 'EUR', rates))).toBe(92);
    expect(roundDisplay(convertCurrency(83, 'INR', 'USD', rates))).toBe(1);
    expect(roundDisplay(convertCurrency(10, 'EUR', 'INR', rates), 2)).toBeCloseTo((10 / 0.92) * 83, 2);
  });

  it('throws on unknown units/currencies', () => {
    expect(() => convert('length', 1, 'furlong', 'm')).toThrow();
    expect(() => convertCurrency(1, 'USD', 'XYZ', { USD: 1 })).toThrow();
  });
});
