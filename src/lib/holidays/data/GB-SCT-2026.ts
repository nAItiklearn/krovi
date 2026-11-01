// United Kingdom (Scotland) — 2026 bank holidays.
//
// SOURCE (official): GOV.UK bank holidays dataset, division "scotland".
//   https://www.gov.uk/bank-holidays.json   (human page: https://www.gov.uk/bank-holidays)
// RETRIEVED: 2026-11-01.
// Includes the one-off "World Cup bank holiday" on Monday 15 June 2026 that GOV.UK lists
// for Scotland. Boxing Day (Saturday 26 Dec) has its substitute day on Monday 28 December.
import type { YearData } from '../types';
import { govUkSource } from './gov-uk';

export const GB_SCT_2026: YearData = {
  region: 'GB-SCT',
  year: 2026,
  source: govUkSource('scotland', 2026, {
    crossChecks: [
      'mygov.scot "Scotland bank holidays" page lists the same ten 2026 dates, including "15 June: World Cup" (retrieved 2026-11-01).',
      'The Guardian (5 Jan 2026) and The Independent (10 Jun 2026) report the extra World Cup bank holiday for Scotland.',
    ],
    notes: [
      'The 15 June 2026 World Cup bank holiday is a one-off; reporting (BBC, The Scotsman, June 2026) notes that many councils and employers did not observe it as a day off.',
    ],
  }),
  holidays: [
    { date: '2026-01-01', name: "New Year's Day", type: 'gazetted' },
    { date: '2026-01-02', name: '2nd January', type: 'gazetted' },
    { date: '2026-04-03', name: 'Good Friday', type: 'gazetted' },
    { date: '2026-05-04', name: 'Early May bank holiday', type: 'gazetted' },
    { date: '2026-05-25', name: 'Spring bank holiday', type: 'gazetted' },
    { date: '2026-06-15', name: 'World Cup bank holiday', type: 'gazetted' },
    { date: '2026-08-03', name: 'Summer bank holiday', type: 'gazetted' },
    { date: '2026-11-30', name: "St Andrew's Day", type: 'gazetted' },
    { date: '2026-12-25', name: 'Christmas Day', type: 'gazetted' },
    { date: '2026-12-28', name: 'Boxing Day (substitute day)', type: 'gazetted' },
  ],
};
