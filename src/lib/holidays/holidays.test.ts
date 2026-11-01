import { describe, it, expect } from 'vitest';
import {
  REGIONS,
  availableYears,
  holidaysFor,
  nextHoliday,
  sourceFor,
  type Holiday,
  type HolidayType,
} from './index';

// ---- tiny date helpers (UTC, so the tests do not depend on the machine time zone) ----
const WEEKDAY = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
const iso = (y: number, m: number, d: number) =>
  `${y}-${String(m).padStart(2, '0')}-${String(d).padStart(2, '0')}`;
const dow = (date: string) => new Date(`${date}T00:00:00Z`).getUTCDay(); // 0 = Sunday
const dayName = (date: string) => WEEKDAY[dow(date)];
const addDays = (date: string, n: number) => {
  const d = new Date(`${date}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + n);
  return d.toISOString().slice(0, 10);
};
/** n-th (1-based) given weekday (0 = Sunday) of a month. */
const nthWeekday = (y: number, m: number, wd: number, n: number) => {
  const first = dow(iso(y, m, 1));
  return iso(y, m, 1 + ((wd - first + 7) % 7) + (n - 1) * 7);
};
const lastWeekday = (y: number, m: number, wd: number) => {
  const lastDay = new Date(Date.UTC(y, m, 0)).getUTCDate();
  const last = dow(iso(y, m, lastDay));
  return iso(y, m, lastDay - ((last - wd + 7) % 7));
};
/** Gregorian Easter Sunday (anonymous / Meeus algorithm). */
const easter = (y: number) => {
  const a = y % 19;
  const b = Math.floor(y / 100);
  const c = y % 100;
  const d = Math.floor(b / 4);
  const e = b % 4;
  const f = Math.floor((b + 8) / 25);
  const g = Math.floor((b - f + 1) / 3);
  const h = (19 * a + b - d - g + 15) % 30;
  const i = Math.floor(c / 4);
  const k = c % 4;
  const l = (32 + 2 * e + 2 * i - h - k) % 7;
  const m = Math.floor((a + 11 * h + 22 * l) / 451);
  const month = Math.floor((h + l - 7 * m + 114) / 31);
  const day = ((h + l - 7 * m + 114) % 31) + 1;
  return iso(y, month, day);
};

const YEARS = [2025, 2026, 2027];
const names = (hs: Holiday[]) => hs.map((h) => h.name);
const find = (hs: Holiday[], name: string) => hs.find((h) => h.name === name);
const ofType = (hs: Holiday[], type: HolidayType) => hs.filter((h) => h.type === type);

describe('regions and registry', () => {
  it('lists IN, US, GB, GB-SCT and GB-NIR with display names', () => {
    expect(REGIONS.map((r) => r.code)).toEqual(['IN', 'US', 'GB', 'GB-SCT', 'GB-NIR']);
    for (const r of REGIONS) expect(r.name.length).toBeGreaterThan(2);
    expect(new Set(REGIONS.map((r) => r.code)).size).toBe(REGIONS.length);
  });

  it('has 2025-2027 data for every listed region', () => {
    for (const r of REGIONS) expect(availableYears(r.code)).toEqual(YEARS);
  });

  it('defaults to India', () => {
    expect(holidaysFor(2026)).toEqual(holidaysFor(2026, 'IN'));
    expect(sourceFor(2026)).toEqual(sourceFor(2026, 'IN'));
    expect(availableYears()).toEqual(availableYears('IN'));
  });

  it('accepts region codes case-insensitively', () => {
    expect(holidaysFor(2026, 'in')).toEqual(holidaysFor(2026, 'IN'));
    expect(holidaysFor(2026, 'gb-sct')).toEqual(holidaysFor(2026, 'GB-SCT'));
  });
});

describe.each(REGIONS.map((r) => r.code))('dataset integrity: %s', (region) => {
  it.each(YEARS)('year %i is well formed, sorted and sourced', (year) => {
    const hs = holidaysFor(year, region);
    expect(hs.length).toBeGreaterThan(5);

    const seen = new Set<string>();
    for (const h of hs) {
      expect(h.date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
      // a real calendar date, inside the requested year
      expect(new Date(`${h.date}T00:00:00Z`).toISOString().slice(0, 10)).toBe(h.date);
      expect(Number(h.date.slice(0, 4))).toBe(year);
      expect(['gazetted', 'restricted', 'observance']).toContain(h.type);
      expect(h.region).toBe(region);
      expect(h.name.trim()).not.toBe('');
      const key = `${h.date}|${h.name}`;
      expect(seen.has(key), `duplicate ${key}`).toBe(false);
      seen.add(key);
    }

    const dates = hs.map((h) => h.date);
    expect(dates).toEqual([...dates].sort());

    const src = sourceFor(year, region);
    expect(src).not.toBeNull();
    expect(src!.url).toMatch(/^https:\/\//);
    expect(src!.retrieved).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(src!.reference.length).toBeGreaterThan(10);
    expect(src!.authority.length).toBeGreaterThan(3);
    for (const s of src!.supplements) expect(s.url).toMatch(/^https:\/\//);
  });
});

describe('unknown years and regions never fabricate data', () => {
  it('returns [] for years without a dataset', () => {
    for (const y of [1999, 2000, 2024, 2028, 2100]) {
      for (const r of REGIONS) expect(holidaysFor(y, r.code)).toEqual([]);
    }
  });

  it('returns [] for non-integer years', () => {
    expect(holidaysFor(Number.NaN)).toEqual([]);
    expect(holidaysFor(2026.5)).toEqual([]);
    expect(holidaysFor(Infinity)).toEqual([]);
  });

  it('returns [] / null for regions that are not registered', () => {
    expect(holidaysFor(2026, 'FR')).toEqual([]);
    expect(holidaysFor(2026, '')).toEqual([]);
    expect(sourceFor(2026, 'FR')).toBeNull();
    expect(availableYears('FR')).toEqual([]);
  });

  it('has no source for years without a dataset', () => {
    expect(sourceFor(2024)).toBeNull();
    expect(sourceFor(2028)).toBeNull();
  });

  it('returns copies, so callers cannot corrupt the bundled data', () => {
    const a = holidaysFor(2026, 'IN');
    a[0].name = 'CHANGED';
    a.pop();
    const b = holidaysFor(2026, 'IN');
    expect(b[0].name).not.toBe('CHANGED');
    expect(b.length).toBe(a.length + 1);

    const s = sourceFor(2026, 'IN')!;
    s.notes.push('x');
    expect(sourceFor(2026, 'IN')!.notes).not.toContain('x');
  });
});

describe('India (DoPT Office Memoranda)', () => {
  it('has the three national days as gazetted every year', () => {
    for (const y of YEARS) {
      const hs = holidaysFor(y, 'IN');
      expect(find(hs, 'Republic Day')).toMatchObject({ date: `${y}-01-26`, type: 'gazetted' });
      expect(find(hs, 'Independence Day')).toMatchObject({ date: `${y}-08-15`, type: 'gazetted' });
      expect(find(hs, "Mahatma Gandhi's Birthday")).toMatchObject({
        date: `${y}-10-02`,
        type: 'gazetted',
      });
    }
  });

  it('has the OM counts: 17 gazetted per Annexure-I, 34 / 34 / 32 restricted', () => {
    const AMBEDKAR = "Dr. B.R. Ambedkar's Birthday (Ambedkar Jayanti)";
    const restrictedCount: Record<number, number> = { 2025: 34, 2026: 34, 2027: 32 };
    for (const y of YEARS) {
      const hs = holidaysFor(y, 'IN');
      const gazetted = ofType(hs, 'gazetted').filter((h) => h.name !== AMBEDKAR);
      expect(gazetted).toHaveLength(17);
      expect(ofType(hs, 'restricted')).toHaveLength(restrictedCount[y]);
      expect(ofType(hs, 'observance')).toHaveLength(0);
    }
  });

  it('adds the separately declared Ambedkar Jayanti holiday for 2025 and 2026 only', () => {
    const name = "Dr. B.R. Ambedkar's Birthday (Ambedkar Jayanti)";
    expect(find(holidaysFor(2025, 'IN'), name)).toMatchObject({ date: '2025-04-14', type: 'gazetted' });
    expect(find(holidaysFor(2026, 'IN'), name)).toMatchObject({ date: '2026-04-14', type: 'gazetted' });
    expect(find(holidaysFor(2027, 'IN'), name)).toBeUndefined();
  });

  it('has the movable festivals on the dates in the OMs', () => {
    const date = (y: number, name: string) => find(holidaysFor(y, 'IN'), name)?.date;
    expect(date(2025, 'Diwali (Deepavali)')).toBe('2025-10-20');
    expect(date(2026, 'Diwali (Deepavali)')).toBe('2026-11-08');
    // The National Portal omits this one; the OM (item 15 and para 6) has it.
    expect(date(2027, 'Diwali (Deepavali)')).toBe('2027-10-29');

    expect(date(2025, 'Holi')).toBe('2025-03-14');
    expect(date(2026, 'Holi')).toBe('2026-03-04');
    expect(date(2027, 'Holi')).toBe('2027-03-23');

    expect(date(2025, "Guru Nanak's Birthday")).toBe('2025-11-05');
    expect(date(2026, "Guru Nanak's Birthday")).toBe('2026-11-24');
    expect(date(2027, "Guru Nanak's Birthday")).toBe('2027-11-14');

    expect(date(2025, 'Id-ul-Fitr (Eid)')).toBe('2025-03-31');
    expect(date(2026, 'Id-ul-Fitr (Eid)')).toBe('2026-03-21');
    expect(date(2027, 'Id-ul-Fitr (Eid)')).toBe('2027-03-10');
  });

  it('applies the 22 May 2026 DoPT order moving Id-ul-Zuha to 28 May', () => {
    const hs = holidaysFor(2026, 'IN');
    expect(find(hs, 'Id-ul-Zuha (Bakrid)')?.date).toBe('2026-05-28');
    expect(hs.some((h) => h.date === '2026-05-27' && h.type === 'gazetted')).toBe(false);
    expect(sourceFor(2026, 'IN')!.supplements.some((s) => s.reference.includes('22 May 2026'))).toBe(true);
  });

  it('keeps Independence Day and Milad-un-Nabi both on Sunday 15 Aug 2027 (OM: no substitute)', () => {
    const on = holidaysFor(2027, 'IN').filter((h) => h.date === '2027-08-15' && h.type === 'gazetted');
    expect(names(on)).toEqual(['Independence Day', 'Milad-un-Nabi (Id-e-Milad)']);
  });

  it('matches the weekday printed in each OM Annexure-I', () => {
    const om: Record<number, [string, string][]> = {
      2025: [
        ['01-26', 'Sunday'], ['02-26', 'Wednesday'], ['03-14', 'Friday'], ['03-31', 'Monday'],
        ['04-10', 'Thursday'], ['04-18', 'Friday'], ['05-12', 'Monday'], ['06-07', 'Saturday'],
        ['07-06', 'Sunday'], ['08-15', 'Friday'], ['08-16', 'Saturday'], ['09-05', 'Friday'],
        ['10-02', 'Thursday'], ['10-20', 'Monday'], ['11-05', 'Wednesday'], ['12-25', 'Thursday'],
      ],
      2026: [
        ['01-26', 'Monday'], ['03-04', 'Wednesday'], ['03-21', 'Saturday'], ['03-26', 'Thursday'],
        ['03-31', 'Tuesday'], ['04-03', 'Friday'], ['05-01', 'Friday'], ['05-28', 'Thursday'],
        ['06-26', 'Friday'], ['08-15', 'Saturday'], ['08-26', 'Wednesday'], ['09-04', 'Friday'],
        ['10-02', 'Friday'], ['10-20', 'Tuesday'], ['11-08', 'Sunday'], ['11-24', 'Tuesday'],
        ['12-25', 'Friday'],
      ],
      2027: [
        ['01-26', 'Tuesday'], ['03-10', 'Wednesday'], ['03-23', 'Tuesday'], ['03-26', 'Friday'],
        ['04-15', 'Thursday'], ['04-19', 'Monday'], ['05-17', 'Monday'], ['05-20', 'Thursday'],
        ['06-16', 'Wednesday'], ['08-15', 'Sunday'], ['08-25', 'Wednesday'], ['10-02', 'Saturday'],
        ['10-09', 'Saturday'], ['10-29', 'Friday'], ['11-14', 'Sunday'], ['12-25', 'Saturday'],
      ],
    };
    for (const y of YEARS) {
      const gazetted = new Set(ofType(holidaysFor(y, 'IN'), 'gazetted').map((h) => h.date));
      for (const [md, weekday] of om[y]) {
        const date = `${y}-${md}`;
        expect(gazetted.has(date), `${date} should be a gazetted holiday`).toBe(true);
        expect(dayName(date)).toBe(weekday);
      }
    }
  });

  it('documents its discrepancies and cross-checks', () => {
    const s27 = sourceFor(2027, 'IN')!;
    expect(s27.reference).toContain('F.No.12/2/2023-JCA dated 16 July 2026');
    expect(s27.discrepancies.join(' ')).toContain('Diwali');
    expect(sourceFor(2026, 'IN')!.reference).toContain('3 July 2025');
    expect(sourceFor(2025, 'IN')!.reference).toContain('9 July 2024');
    for (const y of YEARS) expect(sourceFor(y, 'IN')!.crossChecks.length).toBeGreaterThan(0);
  });
});

describe('sorting', () => {
  it('sorts by date and lists gazetted before restricted on the same day', () => {
    const hs = holidaysFor(2026, 'IN');
    const diwali = hs.filter((h) => h.date === '2026-11-08');
    expect(diwali.map((h) => h.type)).toEqual(['gazetted', 'restricted']);
    expect(diwali[0].name).toBe('Diwali (Deepavali)');
    expect(hs[0]).toMatchObject({ date: '2026-01-01', name: "New Year's Day", type: 'restricted' });
    expect(hs[hs.length - 1].date).toBe('2026-12-25');
  });
});

describe('nextHoliday', () => {
  it('is inclusive of the day itself', () => {
    expect(nextHoliday('2026-01-26')).toMatchObject({ date: '2026-01-26', name: 'Republic Day' });
  });

  it('finds the next gazetted holiday within the year', () => {
    expect(nextHoliday('2026-01-27')).toMatchObject({ date: '2026-03-04', name: 'Holi' });
    expect(nextHoliday('2026-11-09')).toMatchObject({ date: '2026-11-24', name: "Guru Nanak's Birthday" });
  });

  it('ignores restricted holidays by default but can include them', () => {
    expect(nextHoliday('2026-12-26', 'IN', ['gazetted', 'restricted'])).toMatchObject({
      date: '2027-01-01',
      name: "New Year's Day",
      type: 'restricted',
    });
    expect(nextHoliday('2026-01-01')).toMatchObject({ date: '2026-01-26' });
  });

  it('crosses a year boundary', () => {
    expect(nextHoliday('2025-12-26')).toMatchObject({ date: '2026-01-26', name: 'Republic Day' });
    expect(nextHoliday('2026-12-26')).toMatchObject({ date: '2027-01-26', name: 'Republic Day' });
    expect(nextHoliday('2026-12-26', 'US')).toMatchObject({ date: '2027-01-01', name: "New Year's Day" });
    expect(nextHoliday('2026-12-29', 'GB')).toMatchObject({ date: '2027-01-01', name: "New Year's Day" });
    expect(nextHoliday('2026-12-29', 'GB-SCT')).toMatchObject({ date: '2027-01-01' });
    expect(nextHoliday('2026-12-29', 'GB-NIR')).toMatchObject({ date: '2027-01-01' });
  });

  it('returns null after the last bundled holiday instead of inventing one', () => {
    expect(nextHoliday('2027-12-26')).toBeNull();
    expect(nextHoliday('2027-12-29', 'GB')).toBeNull();
    expect(nextHoliday('2027-12-25', 'US')).toBeNull();
  });

  it('returns null when the starting year has no data', () => {
    expect(nextHoliday('2024-12-01')).toBeNull();
    expect(nextHoliday('2030-01-01')).toBeNull();
  });

  it('returns null for invalid dates and unknown regions', () => {
    for (const bad of ['', 'abc', '2026-02-30', '2026-13-01', '2026-1-5', '26-01-01']) {
      expect(nextHoliday(bad)).toBeNull();
    }
    expect(nextHoliday('2026-01-01', 'FR')).toBeNull();
  });

  it('can select observances and returns a copy', () => {
    expect(nextHoliday('2025-01-19', 'US')).toMatchObject({ date: '2025-01-20', type: 'gazetted' });
    expect(nextHoliday('2025-01-19', 'US', ['observance'])).toMatchObject({
      date: '2025-01-20',
      name: 'Inauguration Day (Washington, DC area federal employees)',
    });
    const a = nextHoliday('2026-01-26')!;
    a.name = 'CHANGED';
    expect(nextHoliday('2026-01-26')!.name).toBe('Republic Day');
  });
});

describe('United States (OPM federal holidays)', () => {
  /** Statutory date of each 5 U.S.C. 6103(a) holiday, moved to the weekday OPM observes. */
  const expectedFederal = (y: number) => {
    const observe = (date: string) => {
      const d = dow(date);
      return d === 6 ? addDays(date, -1) : d === 0 ? addDays(date, 1) : date;
    };
    return [
      observe(iso(y, 1, 1)),
      nthWeekday(y, 1, 1, 3), // MLK: third Monday of January
      nthWeekday(y, 2, 1, 3), // Washington's Birthday: third Monday of February
      lastWeekday(y, 5, 1), // Memorial Day: last Monday of May
      observe(iso(y, 6, 19)),
      observe(iso(y, 7, 4)),
      nthWeekday(y, 9, 1, 1), // Labor Day: first Monday of September
      nthWeekday(y, 10, 1, 2), // Columbus Day: second Monday of October
      observe(iso(y, 11, 11)),
      nthWeekday(y, 11, 4, 4), // Thanksgiving: fourth Thursday of November
      observe(iso(y, 12, 25)),
    ].sort();
  };

  it.each(YEARS)('%i matches 5 U.S.C. 6103 including weekend observance', (y) => {
    const got = ofType(holidaysFor(y, 'US'), 'gazetted').map((h) => h.date);
    expect(got).toEqual(expectedFederal(y));
  });

  it('uses the OPM observed dates', () => {
    const d = (y: number, name: string) => find(holidaysFor(y, 'US'), name)?.date;
    expect(d(2026, 'Independence Day (observed)')).toBe('2026-07-03');
    expect(d(2027, 'Juneteenth National Independence Day (observed)')).toBe('2027-06-18');
    expect(d(2027, 'Independence Day (observed)')).toBe('2027-07-05');
    expect(d(2027, 'Christmas Day (observed)')).toBe('2027-12-24');
    expect(d(2025, 'Independence Day')).toBe('2025-07-04');
    expect(d(2026, 'Thanksgiving Day')).toBe('2026-11-26');
  });

  it('types Inauguration Day 2025 as an observance, not a general holiday', () => {
    const inauguration = holidaysFor(2025, 'US').filter((h) => h.name.startsWith('Inauguration Day'));
    expect(inauguration).toHaveLength(1);
    expect(inauguration[0]).toMatchObject({ date: '2025-01-20', type: 'observance' });
    expect(holidaysFor(2026, 'US').some((h) => h.type === 'observance')).toBe(false);
  });

  it('has 11 federal holidays a year, all on weekdays', () => {
    for (const y of YEARS) {
      const hs = ofType(holidaysFor(y, 'US'), 'gazetted');
      expect(hs).toHaveLength(11);
      for (const h of hs) expect([0, 6]).not.toContain(dow(h.date));
    }
  });
});

describe('United Kingdom (GOV.UK bank holidays)', () => {
  const all = (y: number, r: string) => holidaysFor(y, r);

  it.each(['GB', 'GB-SCT', 'GB-NIR'])('%s: every bank holiday falls on a weekday', (region) => {
    for (const y of YEARS) for (const h of all(y, region)) expect([0, 6]).not.toContain(dow(h.date));
  });

  it.each(YEARS)('%i England & Wales follows the statutory pattern', (y) => {
    const hs = all(y, 'GB');
    const gf = find(hs, 'Good Friday')!;
    expect(gf.date).toBe(addDays(easter(y), -2));
    expect(find(hs, 'Easter Monday')!.date).toBe(addDays(easter(y), 1));
    expect(find(hs, 'Early May bank holiday')!.date).toBe(nthWeekday(y, 5, 1, 1));
    expect(find(hs, 'Spring bank holiday')!.date).toBe(lastWeekday(y, 5, 1));
    expect(find(hs, 'Summer bank holiday')!.date).toBe(lastWeekday(y, 8, 1));
    expect(find(hs, "New Year's Day")!.date).toBe(`${y}-01-01`);
  });

  it.each(YEARS)('%i Northern Ireland adds St Patrick and the Boyne, like E&W otherwise', (y) => {
    const hs = all(y, 'GB-NIR');
    expect(find(hs, "St Patrick's Day")!.date).toBe(`${y}-03-17`);
    expect(find(hs, 'Easter Monday')!.date).toBe(addDays(easter(y), 1));
    expect(find(hs, 'Summer bank holiday')!.date).toBe(lastWeekday(y, 8, 1));
    const boyne = hs.find((h) => h.name.startsWith('Battle of the Boyne'))!;
    // 12 July, or the next Monday when it falls on a weekend
    const j12 = dow(`${y}-07-12`);
    expect(boyne.date).toBe(j12 === 6 ? `${y}-07-14` : j12 === 0 ? `${y}-07-13` : `${y}-07-12`);
  });

  it.each(YEARS)('%i Scotland has 2nd January, St Andrew and an August Monday, but no Easter Monday', (y) => {
    const hs = all(y, 'GB-SCT');
    expect(hs.some((h) => h.name === 'Easter Monday')).toBe(false);
    expect(find(hs, 'Good Friday')!.date).toBe(addDays(easter(y), -2));
    expect(find(hs, 'Summer bank holiday')!.date).toBe(nthWeekday(y, 8, 1, 1));
    expect(hs.some((h) => h.name.startsWith('2nd January'))).toBe(true);
    expect(hs.some((h) => h.name.startsWith("St Andrew's Day"))).toBe(true);
  });

  it('distinguishes the three divisions', () => {
    expect(find(all(2026, 'GB'), "St Andrew's Day")).toBeUndefined();
    expect(find(all(2026, 'GB'), "St Patrick's Day")).toBeUndefined();
    expect(find(all(2026, 'GB-SCT'), "St Andrew's Day")?.date).toBe('2026-11-30');
    expect(find(all(2026, 'GB-NIR'), "St Patrick's Day")?.date).toBe('2026-03-17');
  });

  it('includes the substitute days GOV.UK lists', () => {
    expect(names(all(2026, 'GB')).at(-1)).toBe('Boxing Day (substitute day)');
    expect(all(2026, 'GB').at(-1)!.date).toBe('2026-12-28');
    expect(all(2027, 'GB').slice(-2).map((h) => h.date)).toEqual(['2027-12-27', '2027-12-28']);
    expect(find(all(2027, 'GB-SCT'), '2nd January (substitute day)')?.date).toBe('2027-01-04');
    expect(find(all(2025, 'GB-SCT'), "St Andrew's Day (substitute day)")?.date).toBe('2025-12-01');
  });

  it('includes the one-off Scottish World Cup bank holiday of 15 June 2026 only', () => {
    expect(find(all(2026, 'GB-SCT'), 'World Cup bank holiday')).toMatchObject({
      date: '2026-06-15',
      type: 'gazetted',
    });
    expect(find(all(2026, 'GB'), 'World Cup bank holiday')).toBeUndefined();
    expect(find(all(2026, 'GB-NIR'), 'World Cup bank holiday')).toBeUndefined();
  });

  it('has no restricted days or observances', () => {
    for (const r of ['GB', 'GB-SCT', 'GB-NIR']) {
      for (const y of YEARS) for (const h of all(y, r)) expect(h.type).toBe('gazetted');
    }
  });
});
