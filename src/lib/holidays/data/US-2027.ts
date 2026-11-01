// United States — 2027 federal holidays.
//
// SOURCE (official): U.S. Office of Personnel Management (OPM), "Federal Holidays",
//   2027 Holiday Schedule; legal basis 5 U.S.C. 6103.
//   https://www.opm.gov/policy-data-oversight/pay-leave/federal-holidays/
// RETRIEVED: 2026-11-01.
// Dates are OPM's published dates. Three holidays fall on a weekend in 2027:
//   Juneteenth (Sat 19 Jun)  -> Friday 18 June  (5 U.S.C. 6103(b))
//   Independence Day (Sun 4 Jul) -> Monday 5 July (E.O. 11582 s.3(a))
//   Christmas Day (Sat 25 Dec)   -> Friday 24 December (5 U.S.C. 6103(b))
import type { YearData } from '../types';

export const US_2027: YearData = {
  region: 'US',
  year: 2027,
  source: {
    authority: 'U.S. Office of Personnel Management (OPM)',
    title: 'Federal Holidays: 2027 Holiday Schedule',
    reference: '5 U.S.C. 6103; OPM Federal Holidays table, 2027 Holiday Schedule',
    url: 'https://www.opm.gov/policy-data-oversight/pay-leave/federal-holidays/',
    supplements: [],
    retrieved: '2026-11-01',
    crossChecks: [
      'Statute text of 5 U.S.C. 6103 (law.cornell.edu/uscode/text/5/6103): every date equals the statutory rule or its weekend-observed day; recomputed in the unit tests.',
    ],
    discrepancies: [],
    notes: [
      'Federal holidays only: states, banks and private employers may differ. The Federal Reserve and stock exchanges publish their own schedules.',
      'Observed days (OPM-published): Juneteenth Fri 18 Jun, Independence Day Mon 5 Jul, Christmas Day Fri 24 Dec; shown as "(observed)".',
      'New Year\'s Day 2028 falls on a Saturday, so under 5 U.S.C. 6103(b) the observed holiday is Friday 31 December 2027. OPM\'s 2027 table does not list it (it belongs to the 2028 schedule), so it is deliberately not included here; add it from OPM\'s 2028 table when published.',
      'Washington\'s Birthday is the statutory name; "Presidents\' Day" is added in brackets for recognition.',
    ],
  },
  holidays: [
    { date: '2027-01-01', name: "New Year's Day", type: 'gazetted' },
    { date: '2027-01-18', name: 'Birthday of Martin Luther King, Jr.', type: 'gazetted' },
    { date: '2027-02-15', name: "Washington's Birthday (Presidents' Day)", type: 'gazetted' },
    { date: '2027-05-31', name: 'Memorial Day', type: 'gazetted' },
    { date: '2027-06-18', name: 'Juneteenth National Independence Day (observed)', type: 'gazetted' },
    { date: '2027-07-05', name: 'Independence Day (observed)', type: 'gazetted' },
    { date: '2027-09-06', name: 'Labor Day', type: 'gazetted' },
    { date: '2027-10-11', name: 'Columbus Day', type: 'gazetted' },
    { date: '2027-11-11', name: 'Veterans Day', type: 'gazetted' },
    { date: '2027-11-25', name: 'Thanksgiving Day', type: 'gazetted' },
    { date: '2027-12-24', name: 'Christmas Day (observed)', type: 'gazetted' },
  ],
};
