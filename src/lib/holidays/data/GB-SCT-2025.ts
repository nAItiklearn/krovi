// United Kingdom (Scotland) — 2025 bank holidays.
//
// SOURCE (official): GOV.UK bank holidays dataset, division "scotland".
//   https://www.gov.uk/bank-holidays.json   (human page: https://www.gov.uk/bank-holidays)
// RETRIEVED: 2026-11-01.
// St Andrew's Day (30 Nov) fell on a Sunday in 2025, so the substitute day is Monday 1 December.
import type { YearData } from '../types';
import { govUkSource } from './gov-uk';

export const GB_SCT_2025: YearData = {
  region: 'GB-SCT',
  year: 2025,
  source: govUkSource('scotland', 2025),
  holidays: [
    { date: '2025-01-01', name: "New Year's Day", type: 'gazetted' },
    { date: '2025-01-02', name: '2nd January', type: 'gazetted' },
    { date: '2025-04-18', name: 'Good Friday', type: 'gazetted' },
    { date: '2025-05-05', name: 'Early May bank holiday', type: 'gazetted' },
    { date: '2025-05-26', name: 'Spring bank holiday', type: 'gazetted' },
    { date: '2025-08-04', name: 'Summer bank holiday', type: 'gazetted' },
    { date: '2025-12-01', name: "St Andrew's Day (substitute day)", type: 'gazetted' },
    { date: '2025-12-25', name: 'Christmas Day', type: 'gazetted' },
    { date: '2025-12-26', name: 'Boxing Day', type: 'gazetted' },
  ],
};
