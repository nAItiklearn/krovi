// United States — 2026 federal holidays.
//
// SOURCE (official): U.S. Office of Personnel Management (OPM), "Federal Holidays",
//   2026 Holiday Schedule; legal basis 5 U.S.C. 6103.
//   https://www.opm.gov/policy-data-oversight/pay-leave/federal-holidays/
// RETRIEVED: 2026-11-01.
// Dates are OPM's published dates. 4 July 2026 is a Saturday, so OPM lists the
// preceding Friday, 3 July, as the holiday (5 U.S.C. 6103(b)).
import type { YearData } from '../types';

export const US_2026: YearData = {
  region: 'US',
  year: 2026,
  source: {
    authority: 'U.S. Office of Personnel Management (OPM)',
    title: 'Federal Holidays: 2026 Holiday Schedule',
    reference: '5 U.S.C. 6103; OPM Federal Holidays table, 2026 Holiday Schedule',
    url: 'https://www.opm.gov/policy-data-oversight/pay-leave/federal-holidays/',
    supplements: [],
    retrieved: '2026-11-01',
    crossChecks: [
      'Statute text of 5 U.S.C. 6103 (law.cornell.edu/uscode/text/5/6103): every date equals the statutory rule or its weekend-observed day; recomputed in the unit tests.',
    ],
    discrepancies: [],
    notes: [
      'Federal holidays only: states, banks and private employers may differ. The Federal Reserve and stock exchanges publish their own schedules.',
      'Independence Day falls on Saturday 4 July 2026; OPM lists Friday 3 July as the observed holiday, shown here as "Independence Day (observed)".',
      'One-off closures ordered by Executive Order (for example extra days around Christmas) are not part of the OPM table and are not included.',
      'Washington\'s Birthday is the statutory name; "Presidents\' Day" is added in brackets for recognition.',
    ],
  },
  holidays: [
    { date: '2026-01-01', name: "New Year's Day", type: 'gazetted' },
    { date: '2026-01-19', name: 'Birthday of Martin Luther King, Jr.', type: 'gazetted' },
    { date: '2026-02-16', name: "Washington's Birthday (Presidents' Day)", type: 'gazetted' },
    { date: '2026-05-25', name: 'Memorial Day', type: 'gazetted' },
    { date: '2026-06-19', name: 'Juneteenth National Independence Day', type: 'gazetted' },
    { date: '2026-07-03', name: 'Independence Day (observed)', type: 'gazetted' },
    { date: '2026-09-07', name: 'Labor Day', type: 'gazetted' },
    { date: '2026-10-12', name: 'Columbus Day', type: 'gazetted' },
    { date: '2026-11-11', name: 'Veterans Day', type: 'gazetted' },
    { date: '2026-11-26', name: 'Thanksgiving Day', type: 'gazetted' },
    { date: '2026-12-25', name: 'Christmas Day', type: 'gazetted' },
  ],
};
