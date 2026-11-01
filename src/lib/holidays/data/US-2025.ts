// United States — 2025 federal holidays.
//
// SOURCE (official): U.S. Office of Personnel Management (OPM), "Federal Holidays",
//   2025 Holiday Schedule; legal basis 5 U.S.C. 6103.
//   https://www.opm.gov/policy-data-oversight/pay-leave/federal-holidays/
// RETRIEVED: 2026-11-01.
// Dates are OPM's published dates (observed dates where a holiday falls on a weekend).
import type { YearData } from '../types';

export const US_2025: YearData = {
  region: 'US',
  year: 2025,
  source: {
    authority: 'U.S. Office of Personnel Management (OPM)',
    title: 'Federal Holidays: 2025 Holiday Schedule',
    reference: '5 U.S.C. 6103; OPM Federal Holidays table, 2025 Holiday Schedule',
    url: 'https://www.opm.gov/policy-data-oversight/pay-leave/federal-holidays/',
    supplements: [],
    retrieved: '2026-11-01',
    crossChecks: [
      'Statute text of 5 U.S.C. 6103 (law.cornell.edu/uscode/text/5/6103): every date equals the statutory rule (third Monday of January, last Monday of May, fixed dates, ...); recomputed in the unit tests.',
    ],
    discrepancies: [],
    notes: [
      'Federal holidays only: states, banks and private employers may differ. The Federal Reserve and stock exchanges publish their own schedules.',
      'Inauguration Day (20 January 2025) is a holiday only for federal employees in the Washington, DC area (5 U.S.C. 6103(c)), so it is typed "observance", not "gazetted".',
      'Washington\'s Birthday is the statutory name; "Presidents\' Day" is added in brackets for recognition.',
    ],
  },
  holidays: [
    { date: '2025-01-01', name: "New Year's Day", type: 'gazetted' },
    { date: '2025-01-20', name: 'Birthday of Martin Luther King, Jr.', type: 'gazetted' },
    { date: '2025-01-20', name: 'Inauguration Day (Washington, DC area federal employees)', type: 'observance' },
    { date: '2025-02-17', name: "Washington's Birthday (Presidents' Day)", type: 'gazetted' },
    { date: '2025-05-26', name: 'Memorial Day', type: 'gazetted' },
    { date: '2025-06-19', name: 'Juneteenth National Independence Day', type: 'gazetted' },
    { date: '2025-07-04', name: 'Independence Day', type: 'gazetted' },
    { date: '2025-09-01', name: 'Labor Day', type: 'gazetted' },
    { date: '2025-10-13', name: 'Columbus Day', type: 'gazetted' },
    { date: '2025-11-11', name: 'Veterans Day', type: 'gazetted' },
    { date: '2025-11-27', name: 'Thanksgiving Day', type: 'gazetted' },
    { date: '2025-12-25', name: 'Christmas Day', type: 'gazetted' },
  ],
};
