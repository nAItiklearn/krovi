import { describe, it, expect } from 'vitest';
import { daysBetween, fromISO, holidayMap, nextEvent, relativeDay, toISO, upcomingHoliday, whenLabel } from './agenda';

const at = (iso: string, hh = 9, mm = 0) => {
  const d = fromISO(iso);
  d.setHours(hh, mm, 0, 0);
  return d.getTime();
};

describe('agenda dates', () => {
  it('round-trips local dates', () => {
    expect(toISO(fromISO('2026-11-01'))).toBe('2026-11-01');
    expect(toISO(new Date(2026, 0, 5))).toBe('2026-01-05');
  });

  it('counts whole calendar days across month and DST boundaries', () => {
    expect(daysBetween('2026-11-01', '2026-11-08')).toBe(7);
    expect(daysBetween('2026-03-07', '2026-03-09')).toBe(2);
    expect(daysBetween('2026-11-08', '2026-11-01')).toBe(-7);
  });

  it('labels days relative to today', () => {
    expect(relativeDay('2026-11-01', '2026-11-01')).toBe('Today');
    expect(relativeDay('2026-11-02', '2026-11-01')).toBe('Tomorrow');
    expect(relativeDay('2026-11-04', '2026-11-01')).toBe('In 3 days');
    expect(whenLabel({ date: '2026-11-02', title: 'x', time: '09:30' }, '2026-11-01')).toBe('Tomorrow, 09:30');
  });
});

describe('nextEvent', () => {
  const items = [
    { date: '2026-11-01', title: 'Standup', time: '09:00' },
    { date: '2026-11-01', title: 'Design review', time: '15:00' },
    { date: '2026-11-03', title: 'Studio open day' },
  ];

  it('skips events that already finished', () => {
    expect(nextEvent(items, at('2026-11-01', 8))?.title).toBe('Standup');
    expect(nextEvent(items, at('2026-11-01', 11))?.title).toBe('Design review');
  });

  it('lets a timed event linger for an hour', () => {
    expect(nextEvent(items, at('2026-11-01', 9, 30))?.title).toBe('Standup');
  });

  it('keeps all-day events current until their day ends, and returns null when nothing is left', () => {
    expect(nextEvent(items, at('2026-11-03', 22))?.title).toBe('Studio open day');
    expect(nextEvent(items, at('2026-11-04', 0, 1))).toBeNull();
    expect(nextEvent([], Date.now())).toBeNull();
  });
});

describe('holidays on the agenda', () => {
  it('finds the next public holiday from the bundled dataset', () => {
    const h = upcomingHoliday('2026-11-01', 'IN');
    expect(h && h.date >= '2026-11-01').toBe(true);
    expect(h?.type).toBe('gazetted');
  });

  it('maps holidays by date across several years', () => {
    const map = holidayMap([2026, 2026, 2027], 'IN');
    expect(map.size).toBeGreaterThan(5);
    for (const [date, list] of map) expect(list.every((x) => x.date === date)).toBe(true);
  });
});
