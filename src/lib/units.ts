// Offline unit conversion (SPEC C10) plus currency conversion from cached rates.
//
// Length and mass are factor-based (converted through a canonical base unit).
// Temperature needs affine formulas, so it is handled explicitly. Currency uses
// a rates table expressed as "amount of currency per 1 USD".

export type UnitCategory = 'length' | 'mass' | 'temperature';

/** Factors to the canonical base unit (metre for length, gram for mass). */
const FACTORS: Record<Exclude<UnitCategory, 'temperature'>, Record<string, number>> = {
  length: {
    mm: 0.001,
    cm: 0.01,
    m: 1,
    km: 1000,
    in: 0.0254,
    ft: 0.3048,
    yd: 0.9144,
    mi: 1609.344,
  },
  mass: {
    mg: 0.001,
    g: 1,
    kg: 1000,
    t: 1_000_000,
    oz: 28.349523125,
    lb: 453.59237,
  },
};

export const UNITS: Record<UnitCategory, string[]> = {
  length: Object.keys(FACTORS.length),
  mass: Object.keys(FACTORS.mass),
  temperature: ['C', 'F', 'K'],
};

function toCelsius(value: number, from: string): number {
  switch (from) {
    case 'C':
      return value;
    case 'F':
      return (value - 32) * (5 / 9);
    case 'K':
      return value - 273.15;
    default:
      throw new Error(`Unknown temperature unit "${from}"`);
  }
}

function fromCelsius(celsius: number, to: string): number {
  switch (to) {
    case 'C':
      return celsius;
    case 'F':
      return celsius * (9 / 5) + 32;
    case 'K':
      return celsius + 273.15;
    default:
      throw new Error(`Unknown temperature unit "${to}"`);
  }
}

/** Convert `value` from one unit to another within a category. */
export function convert(category: UnitCategory, value: number, from: string, to: string): number {
  if (category === 'temperature') {
    return fromCelsius(toCelsius(value, from), to);
  }
  const table = FACTORS[category];
  const f = table[from];
  const t = table[to];
  if (f === undefined || t === undefined) {
    throw new Error(`Unknown ${category} unit`);
  }
  return (value * f) / t;
}

/**
 * Convert currency using a rates table (amount of each currency per 1 USD).
 * e.g. rates = { USD: 1, EUR: 0.92, INR: 83.1 }.
 */
export function convertCurrency(value: number, from: string, to: string, rates: Record<string, number>): number {
  const rf = rates[from];
  const rt = rates[to];
  if (rf === undefined || rt === undefined) {
    throw new Error('Unknown currency');
  }
  const usd = value / rf;
  return usd * rt;
}

/** Round to a sensible number of significant-ish decimals for display. */
export function roundDisplay(n: number, places = 4): number {
  if (!Number.isFinite(n)) return n;
  const f = Math.pow(10, places);
  return Math.round(n * f) / f;
}
