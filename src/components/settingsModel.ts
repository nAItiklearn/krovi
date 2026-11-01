// Pure helpers for the Settings controls (unit-tested).

/** "5.5 s" / "12 s" / "1 min" for the idle delay. */
export function delayLabel(ms: number): string {
  if (ms >= 60_000) return `${Math.round(ms / 60_000)} min`;
  const s = ms / 1000;
  return `${Number.isInteger(s) ? s : s.toFixed(1)} s`;
}

/**
 * The idle delay spans 1.5 s to a minute, but people mostly pick a few seconds. The slider maps
 * its 0..1 position through a square root so the useful range gets most of the track.
 */
export function delayToPos(ms: number, min: number, max: number): number {
  const t = (Math.min(max, Math.max(min, ms)) - min) / (max - min);
  return Math.sqrt(t);
}

export function posToDelay(pos: number, min: number, max: number, step = 500): number {
  const p = Math.min(1, Math.max(0, pos));
  const raw = min + p * p * (max - min);
  const snapped = Math.round(raw / step) * step;
  return Math.min(max, Math.max(min, snapped));
}
