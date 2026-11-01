// United Kingdom (England and Wales) — 2026 bank holidays.
//
// SOURCE (official): GOV.UK bank holidays dataset, division "england-and-wales".
//   https://www.gov.uk/bank-holidays.json   (human page: https://www.gov.uk/bank-holidays)
// RETRIEVED: 2026-11-01.
// Boxing Day falls on Saturday 26 December 2026, so the substitute day is Monday 28 December.
import type { YearData } from '../types';
import { govUkSource } from './gov-uk';

export const GB_2026: YearData = {
  region: 'GB',
  year: 2026,
  source: govUkSource('england-and-wales', 2026, {
    notes: ['Scotland\'s one-off 15 June 2026 World Cup bank holiday does not apply in England and Wales.'],
  }),
  holidays: [
    { date: '2026-01-01', name: "New Year's Day", type: 'gazetted' },
    { date: '2026-04-03', name: 'Good Friday', type: 'gazetted' },
    { date: '2026-04-06', name: 'Easter Monday', type: 'gazetted' },
    { date: '2026-05-04', name: 'Early May bank holiday', type: 'gazetted' },
    { date: '2026-05-25', name: 'Spring bank holiday', type: 'gazetted' },
    { date: '2026-08-31', name: 'Summer bank holiday', type: 'gazetted' },
    { date: '2026-12-25', name: 'Christmas Day', type: 'gazetted' },
    { date: '2026-12-28', name: 'Boxing Day (substitute day)', type: 'gazetted' },
  ],
};
