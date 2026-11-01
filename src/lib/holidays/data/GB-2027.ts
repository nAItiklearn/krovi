// United Kingdom (England and Wales) — 2027 bank holidays.
//
// SOURCE (official): GOV.UK bank holidays dataset, division "england-and-wales".
//   https://www.gov.uk/bank-holidays.json   (human page: https://www.gov.uk/bank-holidays)
// RETRIEVED: 2026-11-01.
// Christmas Day (Saturday) and Boxing Day (Sunday) 2027 both move: Monday 27 and Tuesday 28 December.
import type { YearData } from '../types';
import { govUkSource } from './gov-uk';

export const GB_2027: YearData = {
  region: 'GB',
  year: 2027,
  source: govUkSource('england-and-wales', 2027, {
    crossChecks: [
      'Manchester Evening News (31 Oct 2026), headline only: "Full list of confirmed bank holidays in 2027 including two Christmas substitute days".',
    ],
  }),
  holidays: [
    { date: '2027-01-01', name: "New Year's Day", type: 'gazetted' },
    { date: '2027-03-26', name: 'Good Friday', type: 'gazetted' },
    { date: '2027-03-29', name: 'Easter Monday', type: 'gazetted' },
    { date: '2027-05-03', name: 'Early May bank holiday', type: 'gazetted' },
    { date: '2027-05-31', name: 'Spring bank holiday', type: 'gazetted' },
    { date: '2027-08-30', name: 'Summer bank holiday', type: 'gazetted' },
    { date: '2027-12-27', name: 'Christmas Day (substitute day)', type: 'gazetted' },
    { date: '2027-12-28', name: 'Boxing Day (substitute day)', type: 'gazetted' },
  ],
};
