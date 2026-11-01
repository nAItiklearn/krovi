// United Kingdom (England and Wales) — 2025 bank holidays.
//
// SOURCE (official): GOV.UK bank holidays dataset, division "england-and-wales".
//   https://www.gov.uk/bank-holidays.json   (human page: https://www.gov.uk/bank-holidays)
// RETRIEVED: 2026-11-01.
import type { YearData } from '../types';
import { govUkSource } from './gov-uk';

export const GB_2025: YearData = {
  region: 'GB',
  year: 2025,
  source: govUkSource('england-and-wales', 2025),
  holidays: [
    { date: '2025-01-01', name: "New Year's Day", type: 'gazetted' },
    { date: '2025-04-18', name: 'Good Friday', type: 'gazetted' },
    { date: '2025-04-21', name: 'Easter Monday', type: 'gazetted' },
    { date: '2025-05-05', name: 'Early May bank holiday', type: 'gazetted' },
    { date: '2025-05-26', name: 'Spring bank holiday', type: 'gazetted' },
    { date: '2025-08-25', name: 'Summer bank holiday', type: 'gazetted' },
    { date: '2025-12-25', name: 'Christmas Day', type: 'gazetted' },
    { date: '2025-12-26', name: 'Boxing Day', type: 'gazetted' },
  ],
};
