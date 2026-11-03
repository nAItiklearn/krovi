// Runtime glue for the idle companion's quick reactions (curious, focused, success, error).
// The rules live in components/idle/reaction.ts (pure, tested); this file owns the one timer and
// the subscriptions, and is the public surface:
//
//   react('success')         fire a reaction from anywhere (Agent / Circle steps call this)
//   reaction                 readable store: what the eyes should show right now (or null)
//   startReactions()         wire the built-in sources; refcounted, the companion calls it
//
// Built-in sources (all read-only; nothing here writes to another module's state):
//   stores.ts  timers + pomodoro    -> focused while a countdown / focus session runs
//   stores.ts  activity (timer)     -> success when a timer or pomodoro phase finishes
//   `alarm-fired`                   -> success (the native alarm, also when the webview was asleep)
//   `dictation-result`              -> success when text was typed, error when it failed
//   shell-cursor distance           -> curious when the cursor lingers near the pill (fed by the
//                                      companion, which already listens to shell-cursor)
//
// Cost: one setTimeout, only while a reaction has a pending edge; no polling, no per-frame work.

import { derived, writable, type Readable } from 'svelte/store';
import { listen } from './bridge';
import type { DictationResult } from './dictation';
import { activity, pomodoro, timers } from './stores';
import {
  current,
  initReaction,
  isFocusing,
  nextChange,
  noteCursor,
  reactionForAlarm,
  reactionForDictation,
  setFocused,
  setReduced,
  trigger,
  type AlarmOutcome,
  type Reaction,
  type ReactionKind,
  type ReactionState,
} from '../components/idle/reaction';

export type { Reaction, ReactionKind } from '../components/idle/reaction';
export { REACTION_KINDS, isReactionKind } from '../components/idle/reaction';

const reducedMotion = (): boolean =>
  typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

let state: ReactionState = initReaction(reducedMotion());
let shown: Reaction | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;
const out = writable<Reaction | null>(null);

/** What the eyes should show on top of their mood right now, or null. */
export const reaction: Readable<Reaction | null> = { subscribe: out.subscribe };

/** Publish if the visible reaction changed, then arm the single timer for the next edge. */
function sync(): void {
  if (timer !== null) {
    clearTimeout(timer);
    timer = null;
  }
  const now = Date.now();
  const r = current(state, now);
  if (r?.kind !== shown?.kind || r?.still !== shown?.still) {
    shown = r;
    out.set(r);
  }
  const at = nextChange(state, now);
  if (at !== null) timer = setTimeout(sync, Math.max(1, at - now));
}

/**
 * Make the companion react. Safe to call from anywhere, as often as you like: a kind that just
 * fired is ignored for a few seconds, and a lower-priority one never hides a higher one
 * (error > success > curious > focused). It only ever shows while the eyes are on screen.
 */
export function react(kind: ReactionKind): void {
  state = trigger(state, kind, Date.now());
  sync();
}

/** A timer / focus session started or stopped (level-triggered; `focused` lasts as long as it does). */
export function setFocusing(on: boolean): void {
  const next = setFocused(state, on);
  if (next === state) return;
  state = next;
  sync();
}

/** Where the cursor is, as logical px from the pill centre; `null` when the pill is gone. */
export function noteCursorDistance(dist: number | null): void {
  const next = noteCursor(state, dist, Date.now());
  if (next === state) return;
  state = next;
  sync();
}

// ------------------------------------------------------------------------------ wiring

let users = 0;
let teardown: (() => void) | null = null;

function wire(): () => void {
  const offs: (() => void)[] = [];
  let dead = false;

  const mq = typeof matchMedia === 'function' ? matchMedia('(prefers-reduced-motion: reduce)') : null;
  const onMotion = () => {
    state = setReduced(state, !!mq?.matches);
    sync();
  };
  mq?.addEventListener('change', onMotion);
  offs.push(() => mq?.removeEventListener('change', onMotion));
  onMotion();

  offs.push(derived([timers, pomodoro], ([$t, $p]) => isFocusing($t, $p)).subscribe(setFocusing));

  // A timer or pomodoro phase finishing pushes an activity of kind 'timer' (stores.ts). The value
  // already in the store at wiring time is old news.
  let primed = false;
  let lastId = -1;
  offs.push(
    activity.subscribe((a) => {
      if (a && a.id !== lastId) {
        lastId = a.id;
        if (primed && a.kind === 'timer') react('success');
      }
      primed = true;
    }),
  );

  const on = <T>(event: string, cb: (payload: T) => void) => {
    listen<T>(event, cb)
      .then((off) => (dead ? off() : offs.push(off)))
      .catch(() => {
        /* no backend (browser demo) or listener refused: that source just stays silent */
      });
  };
  on<AlarmOutcome>('alarm-fired', (a) => {
    const kind = reactionForAlarm(a);
    if (kind) react(kind);
  });
  on<Pick<DictationResult, 'ok' | 'nothingHeard'>>('dictation-result', (r) => {
    const kind = reactionForDictation(r);
    if (kind) react(kind);
  });

  return () => {
    dead = true;
    offs.forEach((off) => off());
    if (timer !== null) clearTimeout(timer);
    timer = null;
    state = initReaction(reducedMotion());
    shown = null;
    out.set(null);
  };
}

/**
 * Wire the built-in sources. Refcounted and idempotent per caller: the companion starts it on
 * mount and stops it on destroy, and the sources are live while any caller is.
 */
export function startReactions(): () => void {
  users++;
  if (users === 1) teardown = wire();
  let released = false;
  return () => {
    if (released) return;
    released = true;
    users--;
    if (users === 0) {
      teardown?.();
      teardown = null;
    }
  };
}
