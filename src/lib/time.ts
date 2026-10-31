// Small, pure time-formatting helpers used across tools. No wall-clock reads
// here (callers pass `now`), matching krovi-core's injected-clock discipline.

export function pad2(n: number): string {
  return n < 10 ? `0${n}` : String(n);
}

/** Format a millisecond duration as H:MM:SS (hours dropped when zero → M:SS). */
export function formatClock(ms: number): string {
  const totalSec = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  const s = totalSec % 60;
  if (h > 0) return `${h}:${pad2(m)}:${pad2(s)}`;
  return `${m}:${pad2(s)}`;
}

/** Stopwatch format M:SS.cs (centiseconds) — never drops minutes. */
export function formatStopwatch(ms: number): string {
  const safe = Math.max(0, ms);
  const totalSec = Math.floor(safe / 1000);
  const m = Math.floor(totalSec / 60);
  const s = totalSec % 60;
  const cs = Math.floor((safe % 1000) / 10);
  return `${m}:${pad2(s)}.${pad2(cs)}`;
}

/** Human "time ago" from an epoch-ms timestamp relative to `now`. */
export function relativeTime(fromMs: number, now: number): string {
  const diff = Math.max(0, now - fromMs);
  const sec = Math.floor(diff / 1000);
  if (sec < 45) return 'just now';
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const days = Math.floor(hr / 24);
  if (days < 7) return `${days}d ago`;
  const weeks = Math.floor(days / 7);
  return `${weeks}w ago`;
}

/** Human-readable byte size. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb < 10 ? kb.toFixed(1) : Math.round(kb)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb < 10 ? mb.toFixed(1) : Math.round(mb)} MB`;
  const gb = mb / 1024;
  return `${gb.toFixed(1)} GB`;
}
