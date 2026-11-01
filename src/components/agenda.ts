// Calendar helpers shared by Home ("next up") and the Calendar view. Pure and unit-tested.

import { holidaysFor, nextHoliday, type Holiday } from '../lib/holidays';

export interface AgendaItem {
  /** yyyy-mm-dd */
  date: string;
  title: string;
  /** HH:MM; absent for all-day */
  time?: string;
}

export function toISO(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

/** Local-midnight Date for a yyyy-mm-dd string. */
export function fromISO(iso: string): Date {
  const [y, m, d] = iso.split('-').map(Number);
  return new Date(y, (m || 1) - 1, d || 1);
}

/** Whole calendar days from a to b (negative when b is earlier). */
export function daysBetween(aISO: string, bISO: string): number {
  const a = fromISO(aISO);
  const b = fromISO(bISO);
  const utcA = Date.UTC(a.getFullYear(), a.getMonth(), a.getDate());
  const utcB = Date.UTC(b.getFullYear(), b.getMonth(), b.getDate());
  return Math.round((utcB - utcA) / 86_400_000);
}

function startMs(it: AgendaItem): number {
  const d = fromISO(it.date);
  if (it.time) {
    const [h, m] = it.time.split(':').map(Number);
    d.setHours(h || 0, m || 0, 0, 0);
  }
  return d.getTime();
}

/** The soonest item that has not passed (all-day items stay "next" until their day ends). */
export function nextEvent(items: AgendaItem[], nowMs: number): AgendaItem | null {
  let best: AgendaItem | null = null;
  let bestAt = Infinity;
  for (const it of items) {
    const at = startMs(it);
    const end = it.time ? at + 60 * 60_000 : at + 86_400_000; // timed events linger for an hour
    if (end <= nowMs) continue;
    if (at < bestAt) {
      best = it;
      bestAt = at;
    }
  }
  return best;
}

/** Next public holiday on or after `fromISOStr` (bundled data, no network). */
export function upcomingHoliday(fromISOStr: string, region: string): Holiday | null {
  return nextHoliday(fromISOStr, region);
}

/** "Today", "Tomorrow", "In 5 days" / "Mon 3 Nov". */
export function relativeDay(dateISO: string, todayISO: string): string {
  const n = daysBetween(todayISO, dateISO);
  if (n === 0) return 'Today';
  if (n === 1) return 'Tomorrow';
  if (n === -1) return 'Yesterday';
  if (n > 1 && n < 7) return `In ${n} days`;
  return fromISO(dateISO).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
}

export function whenLabel(it: AgendaItem, todayISO: string): string {
  const day = relativeDay(it.date, todayISO);
  return it.time ? `${day}, ${it.time}` : day;
}

/** Holidays for a 6-week grid that may straddle two calendar years. */
export function holidayMap(years: number[], region: string): Map<string, Holiday[]> {
  const map = new Map<string, Holiday[]>();
  for (const y of new Set(years)) {
    for (const h of holidaysFor(y, region)) {
      const list = map.get(h.date) ?? [];
      list.push(h);
      map.set(h.date, list);
    }
  }
  return map;
}
