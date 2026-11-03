// System-aware mood for the companion's eyes, from REAL measurements only (`stats-tick`
// from Rust: GlobalMemoryStatusEx, GetSystemTimes, GetSystemPowerStatus). Pure and
// deterministic: smoothing (EMA), hysteresis (separate enter/leave thresholds) and a
// minimum dwell keep it from flickering. No battery (desktops) => no battery moods.
// Decision record: D-017 amendment (docs/DECISIONS.md).

import type { SystemStats } from '../../lib/types';

export type Mood = 'neutral' | 'happy' | 'sad' | 'stressed' | 'calm';

export interface MoodState {
  mood: Mood;
  /** when the current mood started (ms) */
  since: number;
  /** smoothed CPU % and RAM fraction; null until the first sample */
  cpu: number | null;
  mem: number | null;
  /** a different mood that is currently wanted, and since when (it must settle first) */
  pending: Mood | null;
  pendingSince: number;
}

/** A newly wanted mood must keep being wanted this long before it shows. */
export const SETTLE_MS = 8_000;

/** EMA weight per 2 s sample (~10 s time constant). */
const ALPHA = 0.2;
/** A mood holds at least this long before another may replace it. */
export const MIN_DWELL_MS = 15_000;

export const T = {
  sadEnter: 15, // battery %, on battery power
  sadLeave: 20,
  happyEnter: 90, // battery %
  happyLeave: 85,
  stressEnter: 0.9, // RAM used fraction
  stressLeave: 0.85,
  calmEnter: 10, // CPU %
  calmLeave: 18,
} as const;

export const initMood = (now = 0): MoodState => ({
  mood: 'neutral',
  since: now,
  cpu: null,
  mem: null,
  pending: null,
  pendingSince: now,
});

const ema = (prev: number | null, x: number): number => (prev == null ? x : prev + ALPHA * (x - prev));

/** Which moods currently hold, given the mood we are in (hysteresis). */
function candidates(cur: Mood, cpu: number, mem: number, s: SystemStats): Mood[] {
  const out: Mood[] = [];
  const b = s.battery ?? null;
  if (b && !b.pluggedIn) {
    const thr = cur === 'sad' ? T.sadLeave : T.sadEnter;
    if (b.percent <= thr) out.push('sad');
  }
  if (mem >= (cur === 'stressed' ? T.stressLeave : T.stressEnter)) out.push('stressed');
  if (b) {
    const thr = cur === 'happy' ? T.happyLeave : T.happyEnter;
    if (b.percent >= thr) out.push('happy');
  }
  if (cpu <= (cur === 'calm' ? T.calmLeave : T.calmEnter)) out.push('calm');
  return out;
}

const PRIORITY: Mood[] = ['sad', 'stressed', 'happy', 'calm'];

/** Fold one real stats sample into the mood. */
export function nextMood(st: MoodState, s: SystemStats, now: number): MoodState {
  const memFrac = s.memTotal > 0 ? s.memUsed / s.memTotal : 0;
  const cpu = ema(st.cpu, Math.max(0, Math.min(100, s.cpu)));
  const mem = ema(st.mem, Math.max(0, Math.min(1, memFrac)));
  const holds = candidates(st.mood, cpu, mem, s);
  const want: Mood = PRIORITY.find((m) => holds.includes(m)) ?? 'neutral';
  if (want === st.mood) return { ...st, cpu, mem, pending: null, pendingSince: now };
  const pendingSince = st.pending === want ? st.pendingSince : now;
  const settled = now - pendingSince >= SETTLE_MS;
  if (!settled || now - st.since < MIN_DWELL_MS) return { ...st, cpu, mem, pending: want, pendingSince };
  return { mood: want, since: now, cpu, mem, pending: null, pendingSince: now };
}
