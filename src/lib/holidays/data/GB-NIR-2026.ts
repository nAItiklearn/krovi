// United Kingdom (Northern Ireland) — 2026 bank holidays.
//
// SOURCE (official): GOV.UK bank holidays dataset, division "northern-ireland".
//   https://www.gov.uk/bank-holidays.json   (human page: https://www.gov.uk/bank-holidays)
// RETRIEVED: 2026-11-01.
// The Battle of the Boyne (12 July) fell on a Sunday in 2026 -> Monday 13 July; Boxing Day
// (Saturday 26 Dec) -> Monday 28 December.
import type { YearData } from '../types';
import { govUkSource } from './gov-uk';

export const GB_NIR_2026: YearData = {
  region: 'GB-NIR',
  year: 2026,
  source: govUkSource('northern-ireland', 2026),
  holidays: [
    { date: '2026-01-01', name: "New Year's Day", type: 'gazetted' },
    { date: '2026-03-17', name: "St Patrick's Day", type: 'gazetted' },
    { date: '2026-04-03', name: 'Good Friday', type: 'gazetted' },
    { date: '2026-04-06', name: 'Easter Monday', type: 'gazetted' },
    { date: '2026-05-04', name: 'Early May bank holiday', type: 'gazetted' },
    { date: '2026-05-25', name: 'Spring bank holiday', type: 'gazetted' },
    { date: '2026-07-13', name: "Battle of the Boyne (Orangemen's Day) (substitute day)", type: 'gazetted' },
    { date: '2026-08-31', name: 'Summer bank holiday', type: 'gazetted' },
    { date: '2026-12-25', name: 'Christmas Day', type: 'gazetted' },
    { date: '2026-12-28', name: 'Boxing Day (substitute day)', type: 'gazetted' },
  ],
};
