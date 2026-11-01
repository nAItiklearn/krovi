// Shared source description for the UK bank-holiday datasets (GB, GB-SCT, GB-NIR).
// All three come from the same official GOV.UK dataset, one "division" each.
import type { HolidaySource } from '../types';

export type UkDivision = 'england-and-wales' | 'scotland' | 'northern-ireland';

export const GOV_UK_JSON = 'https://www.gov.uk/bank-holidays.json';
export const GOV_UK_PAGE = 'https://www.gov.uk/bank-holidays';
export const GOV_UK_RETRIEVED = '2026-11-01';

const DIVISION_LABEL: Record<UkDivision, string> = {
  'england-and-wales': 'England and Wales',
  scotland: 'Scotland',
  'northern-ireland': 'Northern Ireland',
};

export function govUkSource(
  division: UkDivision,
  year: number,
  extra: { crossChecks?: string[]; notes?: string[] } = {},
): HolidaySource {
  return {
    authority: 'UK Government (GOV.UK), bank holidays',
    title: `UK bank holidays ${year}: ${DIVISION_LABEL[division]}`,
    reference: `GOV.UK bank holidays dataset (${GOV_UK_JSON}), division "${division}", events dated ${year}`,
    url: GOV_UK_JSON,
    supplements: [],
    retrieved: GOV_UK_RETRIEVED,
    crossChecks: [
      `The GOV.UK bank holidays web page (${GOV_UK_PAGE}), ${DIVISION_LABEL[division]} tab, lists the same dates (retrieved ${GOV_UK_RETRIEVED}).`,
      'The statutory pattern (Banking and Financial Dealings Act 1971 and later proclamations: Easter Monday = Good Friday + 3 days, early-May and spring/summer Mondays, weekend substitution) is recomputed in the unit tests and agrees.',
      ...(extra.crossChecks ?? []),
    ],
    discrepancies: [],
    notes: [
      'Bank holidays are statutory days off for banks and many employers; whether an individual employer gives the day off is a matter of contract.',
      'Substitute days are shown as "(substitute day)" on the date GOV.UK gives, not on the weekend date they replace.',
      ...(extra.notes ?? []),
    ],
  };
}
