import { describe, expect, it } from 'vitest';
import {
  CURSOR,
  PULSE,
  REACTION_KINDS,
  initReaction,
  isFocusing,
  isReactionKind,
  current,
  nextChange,
  noteCursor,
  reactionForAlarm,
  reactionForDictation,
  setFocused,
  setReduced,
  trigger,
  type ReactionState,
} from './reaction';

const kindAt = (st: ReactionState, t: number) => current(st, t)?.kind ?? null;

function deepFreeze<T>(o: T): T {
  if (o && typeof o === 'object') {
    Object.values(o as object).forEach(deepFreeze);
    Object.freeze(o);
  }
  return o;
}

describe('pulses (success / error / curious / focused)', () => {
  it('starts with nothing showing and nothing pending', () => {
    const st = initReaction();
    expect(current(st, 0)).toBeNull();
    expect(nextChange(st, 0)).toBeNull();
  });

  it.each(REACTION_KINDS)('%s shows for exactly its duration, then the eyes go back to normal', (kind) => {
    const st = trigger(initReaction(), kind, 10_000);
    const ms = PULSE[kind].ms;
    expect(kindAt(st, 10_000)).toBe(kind);
    expect(kindAt(st, 10_000 + ms - 1)).toBe(kind);
    expect(kindAt(st, 10_000 + ms)).toBeNull();
    expect(nextChange(st, 10_000)).toBe(10_000 + ms);
  });

  it('uses the documented durations', () => {
    expect(PULSE.success.ms).toBe(1_600);
    expect(PULSE.error.ms).toBe(2_000);
  });

  it('nothing is permanent: every fired pulse is gone once the longest one has ended', () => {
    let st = initReaction();
    for (const k of REACTION_KINDS) st = trigger(st, k, 0);
    const longest = Math.max(...REACTION_KINDS.map((k) => PULSE[k].ms));
    expect(current(st, longest)).toBeNull();
  });
});

describe('priority: error > success > curious > focused', () => {
  it('shows the highest active one', () => {
    // all four causes at once: focus running, cursor lingering, plus a success and an error pulse
    let st = setFocused(initReaction(), true);
    expect(kindAt(st, 0)).toBe('focused');
    st = noteCursor(st, 100, 0);
    expect(kindAt(st, CURSOR.lingerMs)).toBe('curious');
    st = trigger(st, 'success', CURSOR.lingerMs);
    expect(kindAt(st, CURSOR.lingerMs)).toBe('success');
    st = trigger(st, 'error', CURSOR.lingerMs + 100);
    expect(kindAt(st, CURSOR.lingerMs + 100)).toBe('error');
  });

  it('falls back down the list as each one ends', () => {
    let st = setFocused(initReaction(), true);
    st = noteCursor(st, 100, 0);
    const t0 = CURSOR.lingerMs + 50;
    st = trigger(st, 'error', t0);
    expect(kindAt(st, t0 + PULSE.error.ms - 1)).toBe('error');
    // the superseded success/curious pulses do not come back, but the causes still hold
    expect(kindAt(st, t0 + PULSE.error.ms)).toBe('curious');
    st = noteCursor(st, null, t0 + PULSE.error.ms + 10);
    expect(kindAt(st, t0 + PULSE.error.ms + 10)).toBe('focused');
    st = setFocused(st, false);
    expect(kindAt(st, t0 + PULSE.error.ms + 10)).toBeNull();
  });

  it('a lower reaction fired while a higher one shows is dropped, not queued for later', () => {
    let st = trigger(initReaction(), 'error', 0);
    const same = trigger(st, 'success', 500);
    expect(same).toBe(st); // untouched, and its cooldown was not spent
    st = same;
    expect(kindAt(st, PULSE.error.ms)).toBeNull(); // no success pops out afterwards
    expect(kindAt(trigger(st, 'success', PULSE.error.ms), PULSE.error.ms)).toBe('success');
  });

  it('a higher reaction replaces a lower pulse that is still showing', () => {
    let st = trigger(initReaction(), 'success', 0);
    st = trigger(st, 'error', 300);
    expect(kindAt(st, 300)).toBe('error');
    expect(kindAt(st, 300 + PULSE.error.ms)).toBeNull(); // the success was superseded
  });
});

describe('cooldowns (no strobing)', () => {
  it('ignores the same kind again until its cooldown has passed', () => {
    const first = trigger(initReaction(), 'success', 0);
    expect(trigger(first, 'success', 1_000)).toBe(first);
    expect(trigger(first, 'success', PULSE.success.cooldownMs - 1)).toBe(first);
    const again = trigger(first, 'success', PULSE.success.cooldownMs);
    expect(kindAt(again, PULSE.success.cooldownMs)).toBe('success');
  });

  it('a repeat does not extend the first one', () => {
    let st = trigger(initReaction(), 'error', 0);
    st = trigger(st, 'error', 1_000);
    expect(kindAt(st, PULSE.error.ms)).toBeNull();
  });

  it('each kind cools down on its own: a success does not block an error', () => {
    let st = trigger(initReaction(), 'success', 0);
    st = trigger(st, 'error', 100);
    expect(kindAt(st, 100)).toBe('error');
  });

  it('a burst of identical events is one expression', () => {
    let st = initReaction();
    for (let t = 0; t < 1_500; t += 100) st = trigger(st, 'success', t);
    expect(kindAt(st, 1_599)).toBe('success');
    expect(kindAt(st, 1_600)).toBeNull();
  });
});

describe('focused (a cause: a timer or focus session is running)', () => {
  it('lasts exactly as long as the cause', () => {
    let st = setFocused(initReaction(), true);
    expect(kindAt(st, 0)).toBe('focused');
    expect(kindAt(st, 3_600_000)).toBe('focused');
    st = setFocused(st, false);
    expect(kindAt(st, 3_600_001)).toBeNull();
  });

  it('is interrupted by a success and then resumes', () => {
    let st = setFocused(initReaction(), true);
    st = trigger(st, 'success', 5_000);
    expect(kindAt(st, 5_000)).toBe('success');
    expect(kindAt(st, 5_000 + PULSE.success.ms)).toBe('focused');
  });

  it('setting the same value returns the same state (no churn)', () => {
    const st = setFocused(initReaction(), true);
    expect(setFocused(st, true)).toBe(st);
  });
});

describe('curious from the cursor lingering near the pill', () => {
  it('needs the cursor to stay for the linger time', () => {
    const st = noteCursor(initReaction(), 120, 1_000);
    expect(kindAt(st, 1_000 + CURSOR.lingerMs - 1)).toBeNull();
    expect(kindAt(st, 1_000 + CURSOR.lingerMs)).toBe('curious');
    expect(nextChange(st, 1_000)).toBe(1_000 + CURSOR.lingerMs);
  });

  it('is capped: the eyes do not stay wide for as long as the cursor sits there', () => {
    const st = noteCursor(initReaction(), 120, 0);
    const end = CURSOR.lingerMs + CURSOR.maxMs;
    expect(kindAt(st, end - 1)).toBe('curious');
    expect(kindAt(st, end)).toBeNull();
    expect(kindAt(st, end + 60_000)).toBeNull();
    expect(nextChange(st, CURSOR.lingerMs)).toBe(end);
    expect(nextChange(st, end)).toBeNull();
  });

  it('a fly-by (gone before the linger time) shows nothing and costs no cooldown', () => {
    let st = noteCursor(initReaction(), 120, 0);
    st = noteCursor(st, 900, CURSOR.lingerMs - 100);
    expect(st.nearSince).toBeNull();
    expect(st.readyAt.curious).toBe(0);
    st = noteCursor(st, 120, CURSOR.lingerMs);
    expect(st.nearSince).toBe(CURSOR.lingerMs);
  });

  it('leaving after it showed earns a cooldown before the next visit counts', () => {
    let st = noteCursor(initReaction(), 120, 0);
    const left = CURSOR.lingerMs + 500;
    st = noteCursor(st, null, left);
    expect(kindAt(st, left)).toBeNull();
    // back almost at once: the dwell clock only starts when the cooldown is over
    st = noteCursor(st, 120, left + 100);
    expect(st.nearSince).toBe(left + CURSOR.cooldownMs);
    expect(kindAt(st, left + 100 + CURSOR.lingerMs)).toBeNull();
    expect(kindAt(st, left + CURSOR.cooldownMs + CURSOR.lingerMs)).toBe('curious');
  });

  it('has hysteresis: it takes more to leave than to arrive', () => {
    const between = (CURSOR.nearPx + CURSOR.farPx) / 2;
    expect(noteCursor(initReaction(), between, 0).nearSince).toBeNull(); // not near yet
    let st = noteCursor(initReaction(), CURSOR.nearPx, 0);
    expect(st.nearSince).toBe(0);
    st = noteCursor(st, between, 50);
    expect(st.nearSince).toBe(0); // still counts as near
    st = noteCursor(st, CURSOR.farPx + 1, 100);
    expect(st.nearSince).toBeNull();
  });

  it('treats "no pill" (null) and garbage distances as far away', () => {
    const near = noteCursor(initReaction(), 100, 0);
    expect(noteCursor(near, null, 10).nearSince).toBeNull();
    expect(noteCursor(near, Number.NaN, 10).nearSince).toBeNull();
    expect(noteCursor(initReaction(), Number.NaN, 0).nearSince).toBeNull();
    expect(noteCursor(initReaction(), 9999, 0).nearSince).toBeNull();
  });

  it('unchanged state is returned as is (the 10 Hz event stream allocates nothing)', () => {
    const near = noteCursor(initReaction(), 100, 0);
    expect(noteCursor(near, 150, 100)).toBe(near);
    const far = initReaction();
    expect(noteCursor(far, 5000, 100)).toBe(far);
  });

  it('is outranked by a success while it plays, then carries on', () => {
    let st = noteCursor(initReaction(), 100, 0);
    const t = CURSOR.lingerMs + 100;
    st = trigger(st, 'success', t);
    expect(kindAt(st, t)).toBe('success');
    expect(kindAt(st, t + PULSE.success.ms)).toBe('curious');
  });

  it('a curious pulse (react("curious")) shows briefly even with the cursor far away', () => {
    const st = trigger(initReaction(), 'curious', 0);
    expect(kindAt(st, 0)).toBe('curious');
    expect(kindAt(st, PULSE.curious.ms)).toBeNull();
  });
});

describe('reduced motion', () => {
  it('still shows the expression but reports it as still', () => {
    const st = trigger(setReduced(initReaction(), true), 'error', 0);
    expect(current(st, 0)).toEqual({ kind: 'error', still: true });
    expect(current(trigger(initReaction(), 'error', 0), 0)).toEqual({ kind: 'error', still: false });
  });

  it('applies live and is idempotent', () => {
    const st = setReduced(initReaction(), true);
    expect(setReduced(st, true)).toBe(st);
    expect(setReduced(st, false).reduced).toBe(false);
  });
});

describe('nextChange (what the single timer waits for)', () => {
  it('is the earliest future edge and ignores edges already past', () => {
    let st = trigger(initReaction(), 'focused', 0); // ends at 2500
    st = noteCursor(st, 100, 0); // linger edge at 700, cap edge at 6700
    expect(nextChange(st, 0)).toBe(CURSOR.lingerMs);
    expect(nextChange(st, CURSOR.lingerMs)).toBe(PULSE.focused.ms);
    expect(nextChange(st, PULSE.focused.ms)).toBe(CURSOR.lingerMs + CURSOR.maxMs);
  });

  it('is null when only a cause (no clock) is holding the expression', () => {
    expect(nextChange(setFocused(initReaction(), true), 0)).toBeNull();
  });
});

describe('purity', () => {
  it('never mutates the state it is given', () => {
    const st = deepFreeze(initReaction());
    expect(() => {
      let s = trigger(st, 'success', 0);
      s = trigger(s, 'error', 10);
      s = noteCursor(s, 100, 20);
      s = noteCursor(s, null, 5_000);
      s = setFocused(s, true);
      setReduced(s, true);
      current(s, 100);
      nextChange(s, 100);
    }).not.toThrow();
  });
});

describe('stylesheet and cost bar (source guards)', () => {
  const nodeFs = 'node:fs';
  const read = async (rel: string): Promise<string> => {
    const fs = await import(/* @vite-ignore */ nodeFs);
    return fs.readFileSync(new URL(rel, import.meta.url), 'utf8');
  };

  it('the companion styles every reaction kind, and every [data-react=...] selector is doubled', async () => {
    const svelte = await read('IdleCompanion.svelte');
    const css = svelte.slice(svelte.indexOf('<style>'));
    for (const k of REACTION_KINDS) expect(css, k).toContain(`[data-react='${k}'][data-react]`);
    // an un-doubled one would lose to the mood/pose rules, which carry :not([data-pose]) + attributes
    expect(css).not.toMatch(/\[data-react='\w+'\](?!\[data-react\])/);
    expect(css).toContain('[data-react-still]');
    expect(svelte).toContain('data-react={rx?.kind}');
    // the blocky eyes hang off a skin the script derives from the live theme OR the preview board's scope
    expect(css).toContain("[data-skin='blocky']");
    expect(svelte).toContain('data-skin={blocky');
    expect(svelte).toContain("closest('[data-theme-scope]')");
  });

  it('the reaction glue stays cheap: no frame loop, no interval, exactly one timer', async () => {
    const src = await read('../../lib/reactions.ts');
    expect(src).not.toMatch(/requestAnimationFrame|setInterval/);
    expect((src.match(/setTimeout\s*\(/g) ?? []).length).toBe(1);
  });

  it('the error shake is a Web Animation on transform only (the stylesheet never animates)', async () => {
    const svelte = await read('IdleCompanion.svelte');
    expect(svelte).toMatch(/face\?\.animate\?\.\(/);
    expect(svelte).toContain("composite: 'add'");
    const css = svelte.slice(svelte.indexOf('<style>'));
    expect(css).not.toMatch(/\banimation\s*:|@keyframes/);
  });
});

describe('event mapping', () => {
  it('dictation: typed = success; failed or could not type = error; heard nothing = nothing', () => {
    expect(reactionForDictation({ ok: true })).toBe('success');
    expect(reactionForDictation({ ok: true, nothingHeard: true })).toBeNull();
    expect(reactionForDictation({ ok: false })).toBe('error');
    expect(reactionForDictation({ ok: false, nothingHeard: false })).toBe('error');
  });

  it('alarms: a fresh one is a success, a missed one is not', () => {
    expect(reactionForAlarm({})).toBe('success');
    expect(reactionForAlarm({ missed: false })).toBe('success');
    expect(reactionForAlarm({ missed: true })).toBeNull();
  });

  it('focus: a running timer, or a running pomodoro WORK phase; not a break, not a finished one', () => {
    const pomo = (o: Partial<{ runningSince: number | null; finished: boolean; phase: string }> = {}) => ({
      runningSince: null as number | null,
      finished: false,
      phase: 'work',
      ...o,
    });
    expect(isFocusing([], pomo())).toBe(false);
    expect(isFocusing([{ runningSince: 5 }], pomo())).toBe(true);
    expect(isFocusing([{ runningSince: null }], pomo())).toBe(false);
    expect(isFocusing([], pomo({ runningSince: 5 }))).toBe(true);
    expect(isFocusing([], pomo({ runningSince: 5, phase: 'short' }))).toBe(false);
    expect(isFocusing([], pomo({ runningSince: 5, phase: 'long' }))).toBe(false);
    expect(isFocusing([], pomo({ runningSince: 5, finished: true }))).toBe(false);
  });

  it('recognises its own kinds', () => {
    for (const k of REACTION_KINDS) expect(isReactionKind(k)).toBe(true);
    expect(isReactionKind('happy')).toBe(false);
    expect(isReactionKind(null)).toBe(false);
  });
});
