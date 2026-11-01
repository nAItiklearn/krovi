// Shared types for the bundled public-holiday datasets (owner: holidays).
// Kept in its own module so the per-year data files can import the types
// without importing the registry in index.ts (no import cycle).

/**
 * - `gazetted`   official statutory holiday: a day off for the region's government
 *                offices (IN: DoPT gazetted/closed; US: OPM federal; GB: bank holiday).
 * - `restricted` optional holiday an employee may choose to take (IN restricted holidays).
 * - `observance` a recognised day that is not a general day off (e.g. US Inauguration
 *                Day, which only applies to Washington DC area federal employees).
 */
export type HolidayType = 'gazetted' | 'restricted' | 'observance';

export interface Holiday {
  date: string; // yyyy-mm-dd
  name: string;
  type: HolidayType;
  region: string;
}

export interface RegionInfo {
  code: string;
  name: string;
}

/** One row of a bundled data file: a Holiday without its region (the file carries it). */
export interface HolidayRow {
  date: string; // yyyy-mm-dd
  name: string;
  type: HolidayType;
}

export interface SourceDocument {
  /** What the document is, e.g. the OM number and date. */
  reference: string;
  url: string;
}

/** Where a year of data came from and how it was verified. */
export interface HolidaySource {
  /** Issuing authority, e.g. "Government of India, Department of Personnel & Training". */
  authority: string;
  /** Human title of the primary document. */
  title: string;
  /** OM number / statute / dataset reference of the primary document. */
  reference: string;
  /** URL of the primary document (an official publication). */
  url: string;
  /** Supplementary official orders that amend or extend the primary document. */
  supplements: SourceDocument[];
  /** yyyy-mm-dd the source was retrieved and transcribed. */
  retrieved: string;
  /** Independent sources the dates were cross-checked against. */
  crossChecks: string[];
  /** Disagreements found between sources, and how each was resolved. */
  discrepancies: string[];
  /** Caveats a user of the data should know (moon-sighting shifts, scope, ...). */
  notes: string[];
}

export interface YearData {
  region: string;
  year: number;
  source: HolidaySource;
  holidays: HolidayRow[];
}
