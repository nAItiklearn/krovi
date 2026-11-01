// Public holidays (owner: holidays) — see docs/CONTRACTS.md §4.
//
// Bundled, offline datasets transcribed from official publications (no runtime network
// call, nothing computed or guessed). Years or regions without a dataset return [] / null.
//
//   IN      India, central government holidays (DoPT Office Memoranda), 2025-2027
//   US      United States federal holidays (OPM), 2025-2027
//   GB      England & Wales bank holidays (GOV.UK), 2025-2027
//   GB-SCT  Scotland bank holidays (GOV.UK), 2025-2027
//   GB-NIR  Northern Ireland bank holidays (GOV.UK), 2025-2027
//
// Each data file in ./data carries its source URL, document reference and retrieval date,
// also available at runtime through sourceFor().
import { ALL_DATA } from './data';
import type { Holiday, HolidaySource, HolidayType, RegionInfo, YearData } from './types';

export type { Holiday, HolidaySource, HolidayType, RegionInfo, SourceDocument } from './types';

export const REGIONS: RegionInfo[] = [
  { code: 'IN', name: 'India' },
  { code: 'US', name: 'United States' },
  { code: 'GB', name: 'United Kingdom (England & Wales)' },
  { code: 'GB-SCT', name: 'United Kingdom (Scotland)' },
  { code: 'GB-NIR', name: 'United Kingdom (Northern Ireland)' },
];

const TYPE_RANK: Record<HolidayType, number> = { gazetted: 0, restricted: 1, observance: 2 };

interface Entry {
  data: YearData;
  /** Holidays sorted by date, then gazetted before restricted before observance. */
  sorted: readonly Holiday[];
}

const registry = new Map<string, Map<number, Entry>>();

for (const data of ALL_DATA) {
  const sorted = data.holidays
    .map((h, i) => ({ h, i }))
    .sort(
      (a, b) =>
        a.h.date.localeCompare(b.h.date) || TYPE_RANK[a.h.type] - TYPE_RANK[b.h.type] || a.i - b.i,
    )
    .map(({ h }): Holiday => ({ date: h.date, name: h.name, type: h.type, region: data.region }));
  let years = registry.get(data.region);
  if (!years) registry.set(data.region, (years = new Map()));
  years.set(data.year, { data, sorted });
}

function normRegion(region: string): string {
  return String(region).trim().toUpperCase();
}

function entryFor(year: number, region: string): Entry | undefined {
  if (!Number.isInteger(year)) return undefined;
  return registry.get(normRegion(region))?.get(year);
}

const ISO_RE = /^(\d{4})-(\d{2})-(\d{2})$/;

/** True for a real calendar date written as yyyy-mm-dd. */
function isIsoDate(s: string): boolean {
  const m = ISO_RE.exec(s);
  if (!m) return false;
  const y = Number(m[1]);
  const mo = Number(m[2]);
  const d = Number(m[3]);
  const dt = new Date(Date.UTC(y, mo - 1, d));
  return dt.getUTCFullYear() === y && dt.getUTCMonth() === mo - 1 && dt.getUTCDate() === d;
}

/**
 * Holidays for `year` in `region`, sorted by date (gazetted before restricted on the same
 * day). Returns `[]` for a year or region with no bundled dataset; dates are never invented.
 * The result is a fresh copy and safe to mutate.
 */
export function holidaysFor(year: number, region: string = 'IN'): Holiday[] {
  const entry = entryFor(year, region);
  return entry ? entry.sorted.map((h) => ({ ...h })) : [];
}

/** Years that have a bundled dataset for `region`, ascending. */
export function availableYears(region: string = 'IN'): number[] {
  const years = registry.get(normRegion(region));
  return years ? [...years.keys()].sort((a, b) => a - b) : [];
}

/**
 * The first holiday on or after `fromISO` (yyyy-mm-dd, local date), searching forward
 * across year boundaries for as long as consecutive years have data.
 *
 * `types` selects which holidays count; the default is `['gazetted']`, i.e. the days
 * that are really days off (pass `['gazetted', 'restricted']` to include optional days).
 *
 * Returns `null` for an invalid date, an unknown region, or when no later holiday exists
 * in the bundled data (it never skips over a year that has no data).
 */
export function nextHoliday(
  fromISO: string,
  region: string = 'IN',
  types: readonly HolidayType[] = ['gazetted'],
): Holiday | null {
  if (!isIsoDate(fromISO)) return null;
  for (let year = Number(fromISO.slice(0, 4)); ; year++) {
    const entry = entryFor(year, region);
    if (!entry) return null;
    const hit = entry.sorted.find((h) => h.date >= fromISO && types.includes(h.type));
    if (hit) return { ...hit };
  }
}

/** Provenance of the dataset for `year`/`region`, or `null` when there is none. */
export function sourceFor(year: number, region: string = 'IN'): HolidaySource | null {
  const entry = entryFor(year, region);
  if (!entry) return null;
  const s = entry.data.source;
  return {
    ...s,
    supplements: s.supplements.map((x) => ({ ...x })),
    crossChecks: [...s.crossChecks],
    discrepancies: [...s.discrepancies],
    notes: [...s.notes],
  };
}
