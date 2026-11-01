// United Kingdom (Scotland) — 2027 bank holidays.
//
// SOURCE (official): GOV.UK bank holidays dataset, division "scotland".
//   https://www.gov.uk/bank-holidays.json   (human page: https://www.gov.uk/bank-holidays)
// RETRIEVED: 2026-11-01.
// 2nd January (Saturday 2 Jan 2027) -> Monday 4 January; Christmas Day / Boxing Day (Sat/Sun)
// -> Monday 27 and Tuesday 28 December.
import type { YearData } from '../types';
import { govUkSource } from './gov-uk';

export const GB_SCT_2027: YearData = {
  region: 'GB-SCT',
  year: 2027,
  source: govUkSource('scotland', 2027, {
    crossChecks: [
      'Manchester Evening News (31 Oct 2026), headline only: "Full list of confirmed bank holidays in 2027 including two Christmas substitute days".',
    ],
  }),
  holidays: [
    { date: '2027-01-01', name: "New Year's Day", type: 'gazetted' },
    { date: '2027-01-04', name: '2nd January (substitute day)', type: 'gazetted' },
    { date: '2027-03-26', name: 'Good Friday', type: 'gazetted' },
    { date: '2027-05-03', name: 'Early May bank holiday', type: 'gazetted' },
    { date: '2027-05-31', name: 'Spring bank holiday', type: 'gazetted' },
    { date: '2027-08-02', name: 'Summer bank holiday', type: 'gazetted' },
    { date: '2027-11-30', name: "St Andrew's Day", type: 'gazetted' },
    { date: '2027-12-27', name: 'Christmas Day (substitute day)', type: 'gazetted' },
    { date: '2027-12-28', name: 'Boxing Day (substitute day)', type: 'gazetted' },
  ],
};
