// Quick reactions for the companion's eyes: curious, focused, success, error.
//
// They are TRANSIENT expressions laid over the resting mood (mood.ts) and the micro-poses
// (engine.ts): the moment one ends the eyes go back to whatever they were doing. Pure and
// deterministic like mood.ts: state in, state out, every time passed in as `now` (ms). The runtime
// glue (stores, events, the one timer) is src/lib/reactions.ts; the CSS is in IdleCompanion.svelte.
//
//   priority    error > success > curious > focused           (the highest active one is shown)
//   success     1.6 s pulse   timer done, dictation typed, a task finished
//   error       2.0 s pulse   dictation failed, an agent step failed
//   curious     the cursor lingers near the pill (while it stays, capped), or a 1.4 s pulse
//   focused     while a timer / focus session runs (a cause, not a pulse), or a 2.5 s pulse
//   cooldowns   a kind that just fired cannot fire again for a while, so a burst of events
//               (three timers ringing together, a retry loop) is one expression, not a strobe
//   reduced     the expression still changes (shape), the movement (shake, tilt) does not: the
//               machine only reports `still`; the stylesheet decides what that removes

/** Highest priority first. */
export const REACTION_KINDS = ['error', 'success', 'curious', 'focused'] as const;
export type ReactionKind = (typeof REACTION_KINDS)[number];

export const isReactionKind = (v: unknown): v is ReactionKind =>
  typeof v === 'string' && (REACTION_KINDS as readonly string[]).includes(v);

/** How long a fired reaction shows, and how long before that kind may fire again (from the fire). */
export const PULSE: Record<ReactionKind, { ms: number; cooldownMs: number }> = {
  error: { ms: 2_000, cooldownMs: 3_500 },
  success: { ms: 1_600, cooldownMs: 3_000 },
  curious: { ms: 1_400, cooldownMs: 5_000 },
  focused: { ms: 2_500, cooldownMs: 4_000 },
};

/** The cursor-lingers-near-the-pill cause for `curious`. Distances are logical px from the pill centre. */
export const CURSOR = {
  /** within this and it counts as near */
  nearPx: 300,
  /** once near, it takes moving past this to count as gone (hysteresis, so the edge does not flicker) */
  farPx: 380,
  /** it must stay near this long before the eyes react */
  lingerMs: 700,
  /** the eyes do not stay wide for ever: this is the longest one visit shows */
  maxMs: 6_000,
  /** after a visit that showed, the next one waits this long */
  cooldownMs: 2_500,
} as const;

export interface ReactionState {
  /** prefers-reduced-motion: the shape changes, the movement does not */
  reduced: boolean;
  /** when each fired pulse stops showing (0 = not showing) */
  until: Record<ReactionKind, number>;
  /** the earliest time each kind may fire again */
  readyAt: Record<ReactionKind, number>;
  /** cause: a timer or focus session is running */
  focusOn: boolean;
  /** cause: the cursor has been near since (already pushed past any cooldown), null = not near */
  nearSince: number | null;
}

/** What to draw. `still` = the user prefers reduced motion. */
export interface Reaction {
  kind: ReactionKind;
  still: boolean;
}

const zero = (): Record<ReactionKind, number> => ({ error: 0, success: 0, curious: 0, focused: 0 });

export const initReaction = (reduced = false): ReactionState => ({
  reduced,
  until: zero(),
  readyAt: zero(),
  focusOn: false,
  nearSince: null,
});

export const setReduced = (st: ReactionState, reduced: boolean): ReactionState =>
  st.reduced === reduced ? st : { ...st, reduced };

/** Is a reaction of strictly higher priority than `kind` showing right now? */
function outranked(st: ReactionState, kind: ReactionKind, now: number): boolean {
  for (const k of REACTION_KINDS) {
    if (k === kind) return false;
    if (now < st.until[k]) return true;
  }
  return false;
}

/**
 * Fire a reaction (the public `react(kind)` ends up here). Ignored while that kind is cooling
 * down, and ignored while something that outranks it is showing (it would be invisible anyway,
 * and it must not queue up and pop out later). Firing replaces any lower-priority pulse.
 */
export function trigger(st: ReactionState, kind: ReactionKind, now: number): ReactionState {
  if (now < st.readyAt[kind] || outranked(st, kind, now)) return st;
  const until = { ...st.until, [kind]: now + PULSE[kind].ms };
  for (const k of REACTION_KINDS.slice(REACTION_KINDS.indexOf(kind) + 1)) until[k] = 0;
  return { ...st, until, readyAt: { ...st.readyAt, [kind]: now + PULSE[kind].cooldownMs } };
}

/** Cause: is a timer / focus session running? Level-triggered, so it lasts exactly as long as it does. */
export const setFocused = (st: ReactionState, on: boolean): ReactionState =>
  st.focusOn === on ? st : { ...st, focusOn: on };

/** Has the current cursor visit lingered long enough to show, and not for too long? */
function cursorShowing(st: ReactionState, now: number): boolean {
  if (st.nearSince == null) return false;
  const from = st.nearSince + CURSOR.lingerMs;
  return now >= from && now < from + CURSOR.maxMs;
}

/**
 * Cause: where the cursor is, as a distance (logical px) from the pill centre, or `null` for
 * "gone" (the pill is hidden). Cheap enough for every `shell-cursor` event (<= 10 Hz).
 * A visit starts counting only after the cooldown of the previous one.
 */
export function noteCursor(st: ReactionState, dist: number | null, now: number): ReactionState {
  const wasNear = st.nearSince != null;
  const near = dist != null && Number.isFinite(dist) && dist <= (wasNear ? CURSOR.farPx : CURSOR.nearPx);
  if (near) return wasNear ? st : { ...st, nearSince: Math.max(now, st.readyAt.curious) };
  if (!wasNear) return st;
  // The cursor left. A visit that got as far as showing earns a cooldown; a fly-by costs nothing.
  const showed = now >= (st.nearSince as number) + CURSOR.lingerMs;
  return {
    ...st,
    nearSince: null,
    readyAt: showed ? { ...st.readyAt, curious: Math.max(st.readyAt.curious, now + CURSOR.cooldownMs) } : st.readyAt,
  };
}

/** The reaction to draw at `now`, or null for none (the eyes just carry on). */
export function current(st: ReactionState, now: number): Reaction | null {
  const on: Record<ReactionKind, boolean> = {
    error: now < st.until.error,
    success: now < st.until.success,
    curious: now < st.until.curious || cursorShowing(st, now),
    focused: now < st.until.focused || st.focusOn,
  };
  const kind = REACTION_KINDS.find((k) => on[k]);
  return kind ? { kind, still: st.reduced } : null;
}

/**
 * The next moment `current()` can change by itself (a pulse ends, the cursor has lingered long
 * enough, a visit hits its cap), or null when nothing is pending. The runtime arms ONE timer for it.
 */
export function nextChange(st: ReactionState, now: number): number | null {
  let next = Infinity;
  const consider = (t: number) => {
    if (t > now && t < next) next = t;
  };
  for (const k of REACTION_KINDS) consider(st.until[k]);
  if (st.nearSince != null) {
    consider(st.nearSince + CURSOR.lingerMs);
    consider(st.nearSince + CURSOR.lingerMs + CURSOR.maxMs);
  }
  return Number.isFinite(next) ? next : null;
}

// ------------------------------------------------------------------------------ event mapping
// Pure translations from the app's events to a reaction, so the glue stays a thin subscription
// layer and the rules are tested without a Tauri.

/** The slice of `dictation-result` this needs (the full type is DictationResult in lib/dictation). */
export interface DictationOutcome {
  ok: boolean;
  nothingHeard?: boolean;
}

/** typed → success; failed, or recognised but could not be typed → error; heard nothing → nothing. */
export function reactionForDictation(r: DictationOutcome): ReactionKind | null {
  if (!r.ok) return 'error';
  return r.nothingHeard ? null : 'success';
}

/** The slice of `alarm-fired` this needs. A late (missed) alarm is old news, not a celebration. */
export interface AlarmOutcome {
  missed?: boolean;
}

export const reactionForAlarm = (a: AlarmOutcome): ReactionKind | null => (a.missed ? null : 'success');

/** The slices of the timer and pomodoro stores this needs. */
export interface TimerLike {
  runningSince: number | null;
}
export interface PomodoroLike {
  runningSince: number | null;
  finished: boolean;
  phase: string;
}

/** Focus = a running countdown timer, or a pomodoro that is running its WORK phase (a break is rest). */
export function isFocusing(timers: readonly TimerLike[], pomodoro: PomodoroLike): boolean {
  if (pomodoro.runningSince != null && !pomodoro.finished && pomodoro.phase === 'work') return true;
  return timers.some((t) => t.runningSince != null);
}
