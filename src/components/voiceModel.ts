// Pure helpers for voice v2 in the UI (unit-tested). The API itself (settings, commands,
// context, PRIVACY_FACTS) lives in lib/voice.ts, owned by the voice agent.

import type { IconName } from '../lib/icons';
import type { ContextCapture, MediaWord, VoiceContext, VoiceResult, VoiceResultItem } from '../lib/voice';

/** Labels for the "what does this mean" setting (the ids are the backend's). */
export const CONTEXT_MODES: { id: ContextCapture; label: string; hint: string }[] = [
  { id: 'off', label: 'Off', hint: '“This” and “it” are not resolved.' },
  { id: 'selection', label: 'Selection only', hint: 'Reads your selection at the moment you speak.' },
  {
    id: 'selection+clipboard-fallback',
    label: 'Selection, with clipboard fallback',
    hint: 'If an app hides its selection, Krovi copies it briefly, then restores your clipboard. The copy may appear in Windows clipboard history (Win+V).',
  },
];

/** "name.ext" from a path of either style. */
export function fileName(path: string): string {
  return path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || path;
}

const clip = (s: string, n: number) => {
  const flat = s.replace(/\s+/g, ' ').trim();
  return flat.length <= n ? flat : flat.slice(0, Math.max(1, n - 1)).trimEnd() + '…';
};

const appName = (a: string | undefined) => (a ?? '').replace(/\.exe$/i, '').trim();

/**
 * The chip the pill shows after a voice command, so nothing is read invisibly. The label is the
 * captured text itself (or the file name / count), the title says it in a sentence:
 * "Using your selection in notepad", "Using 2 files in explorer". Null when nothing was captured.
 */
export function contextChip(ctx: VoiceContext | null | undefined): { icon: IconName; label: string; title: string } | null {
  if (!ctx) return null;
  if (ctx.kind === 'text') {
    const v = String(ctx.value ?? '').trim();
    if (!v) return null;
    const app = appName(ctx.app);
    return {
      icon: 'text',
      label: `“${clip(v, 22)}”`,
      title: `Using your selection${app ? ` in ${app}` : ''}: “${clip(v, 120)}”`,
    };
  }
  if (ctx.kind === 'files') {
    const list = (Array.isArray(ctx.value) ? ctx.value : [ctx.value]).filter(Boolean);
    if (!list.length) return null;
    const app = appName(ctx.app);
    return {
      icon: 'file',
      label: list.length === 1 ? clip(fileName(list[0]), 24) : `${list.length} files`,
      title: `Using ${list.length === 1 ? fileName(list[0]) : `${list.length} files`}${app ? ` in ${app}` : ''}`,
    };
  }
  const app = appName(ctx.value);
  return app ? { icon: 'monitor', label: `In ${clip(app, 20)}`, title: `Using the foreground app: ${app}` } : null;
}

// ---- voice v3: rows (search hits, a folder's newest items, ambiguous candidates) ----

/** The rows a finished session wants on screen, with the line to show above them; null when none. */
export function planResults(r: VoiceResult): { feedback: string; rows: VoiceResultItem[] } | null {
  const rows = (r.results ?? []).filter((x) => x && x.name && x.path);
  return rows.length ? { feedback: (r.feedback ?? '').trim(), rows } : null;
}

const IMAGE = /\.(png|jpe?g|gif|webp|svg|heic|bmp|avif|ico)$/i;
const DOC = /\.(pdf|docx?|txt|md|rtf|xlsx?|csv|pptx?|odt)$/i;

/** One glyph per row: folder, app, or by file type. */
export function resultIcon(row: Pick<VoiceResultItem, 'kind' | 'name'>): IconName {
  if (row.kind === 'folder') return 'folder';
  if (row.kind === 'app') return 'appWindow';
  if (IMAGE.test(row.name)) return 'image';
  if (DOC.test(row.name)) return 'fileText';
  return 'file';
}

/** What the sheet says it will do with a row, so a click is never a surprise (files are never run). */
export function resultVerb(kind: VoiceResultItem['kind']): string {
  return kind === 'folder' ? 'Open folder' : kind === 'app' ? 'Launch' : 'Show in Explorer';
}

/** A line for the sheet header when the backend sent none. */
export function resultsHeadline(feedback: string, count: number): string {
  return feedback || `${count} ${count === 1 ? 'result' : 'results'}`;
}

/** Transport key to press for a media word, or null when it would do the wrong thing. */
export function transportFor(word: MediaWord, playing: boolean): 'next' | 'previous' | 'playpause' | null {
  switch (word) {
    case 'next':
      return 'next';
    case 'previous':
      return 'previous';
    case 'playpause':
      return 'playpause';
    case 'play':
      return playing ? null : 'playpause';
    case 'pause':
      return playing ? 'playpause' : null;
    default:
      return null;
  }
}

export const MEDIA_FEEDBACK: Record<MediaWord, string> = {
  next: 'Next track',
  previous: 'Previous track',
  play: 'Playing',
  pause: 'Paused',
  playpause: 'Play / pause',
  'whats-playing': 'Now playing',
};
