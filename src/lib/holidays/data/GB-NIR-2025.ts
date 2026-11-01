// United Kingdom (Northern Ireland) — 2025 bank holidays.
//
// SOURCE (official): GOV.UK bank holidays dataset, division "northern-ireland".
//   https://www.gov.uk/bank-holidays.json   (human page: https://www.gov.uk/bank-holidays)
// RETRIEVED: 2026-11-01.
// The Battle of the Boyne (12 July) fell on a Saturday in 2025, so the substitute day is Monday 14 July.
import type { YearData } from '../types';
import { govUkSource } from './gov-uk';

export const GB_NIR_2025: YearData = {
  region: 'GB-NIR',
  year: 2025,
  source: govUkSource('northern-ireland', 2025),
  holidays: [
    { date: '2025-01-01', name: "New Year's Day", type: 'gazetted' },
    { date: '2025-03-17', name: "St Patrick's Day", type: 'gazetted' },
    { date: '2025-04-18', name: 'Good Friday', type: 'gazetted' },
    { date: '2025-04-21', name: 'Easter Monday', type: 'gazetted' },
    { date: '2025-05-05', name: 'Early May bank holiday', type: 'gazetted' },
    { date: '2025-05-26', name: 'Spring bank holiday', type: 'gazetted' },
    { date: '2025-07-14', name: "Battle of the Boyne (Orangemen's Day) (substitute day)", type: 'gazetted' },
    { date: '2025-08-25', name: 'Summer bank holiday', type: 'gazetted' },
    { date: '2025-12-25', name: 'Christmas Day', type: 'gazetted' },
    { date: '2025-12-26', name: 'Boxing Day', type: 'gazetted' },
  ],
};
