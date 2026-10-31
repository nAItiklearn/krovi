// Clipboard display + query helpers (SPEC C2), mirroring the ordering and
// filter semantics of krovi-core's `ClipboardHistory` so the demo and a future
// Tauri-backed build behave identically.
//
// Ordering: pinned entries first (in given order), then unpinned (in given
// order); callers keep each group newest-first. Query filters by optional kind
// and an optional case-insensitive substring; an empty/undefined needle matches
// everything.

import type { ClipEntry, ClipKind } from './types';

/** Pinned-first, then unpinned, preserving the order within each group. */
export function visible(entries: ClipEntry[]): ClipEntry[] {
  const pinned = entries.filter((e) => e.pinned);
  const rest = entries.filter((e) => !e.pinned);
  return [...pinned, ...rest];
}

/** Filter by optional kind and optional case-insensitive substring, in display order. */
export function query(entries: ClipEntry[], kind: ClipKind | null, needle: string | null): ClipEntry[] {
  const n = needle ? needle.toLowerCase() : '';
  return visible(entries)
    .filter((e) => (kind ? e.kind === kind : true))
    .filter((e) => (n ? e.content.toLowerCase().includes(n) : true));
}

/** Count entries of each kind, for the filter-tab badges. */
export function counts(entries: ClipEntry[]): { all: number; Text: number; Link: number; Image: number } {
  return {
    all: entries.length,
    Text: entries.filter((e) => e.kind === 'Text').length,
    Link: entries.filter((e) => e.kind === 'Link').length,
    Image: entries.filter((e) => e.kind === 'Image').length,
  };
}
