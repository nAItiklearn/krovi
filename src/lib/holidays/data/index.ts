// Every bundled dataset. To add a year or a region: drop a new <REGION>-<YEAR>.ts file
// next to these (see types.ts for the shape), import it here, and add the region to
// REGIONS in ../index.ts. Never add a year without a real, citable source document.
import type { YearData } from '../types';
import { IN_2025 } from './IN-2025';
import { IN_2026 } from './IN-2026';
import { IN_2027 } from './IN-2027';
import { US_2025 } from './US-2025';
import { US_2026 } from './US-2026';
import { US_2027 } from './US-2027';
import { GB_2025 } from './GB-2025';
import { GB_2026 } from './GB-2026';
import { GB_2027 } from './GB-2027';
import { GB_SCT_2025 } from './GB-SCT-2025';
import { GB_SCT_2026 } from './GB-SCT-2026';
import { GB_SCT_2027 } from './GB-SCT-2027';
import { GB_NIR_2025 } from './GB-NIR-2025';
import { GB_NIR_2026 } from './GB-NIR-2026';
import { GB_NIR_2027 } from './GB-NIR-2027';

export const ALL_DATA: readonly YearData[] = [
  IN_2025,
  IN_2026,
  IN_2027,
  US_2025,
  US_2026,
  US_2027,
  GB_2025,
  GB_2026,
  GB_2027,
  GB_SCT_2025,
  GB_SCT_2026,
  GB_SCT_2027,
  GB_NIR_2025,
  GB_NIR_2026,
  GB_NIR_2027,
];
