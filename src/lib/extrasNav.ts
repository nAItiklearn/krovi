// How the palette (and anything else) sends the user to a section of the Extras tool.
//
// The Extras tool has four sections. `openExtras('workflows', { runWorkflow: id })` switches to the
// tool and hands the view a one-time intent; the view takes it when it mounts (or right away if it is
// already open). Running a workflow always happens inside the Workflows section, where its steps and
// any confirmation cards are visible, never silently from the palette.

import { get, writable } from 'svelte/store';
import { openTool } from './stores';
import { isToolId } from './tools';
import type { ToolId } from './types';

export type ExtrasTab = 'snippets' | 'organize' | 'workflows' | 'search';

/** The id the Extras tool is registered under in `tools.ts` / `types.ts` / `Panel.svelte`. */
export const EXTRAS_TOOL = 'extras' as string as ToolId;

/**
 * Has the Extras tool been registered? The palette only offers app, snippet and workflow results
 * (and asks the backend for them) once it has, so nothing points at a view that does not exist.
 */
export function extrasAvailable(): boolean {
  return isToolId(EXTRAS_TOOL);
}

export interface ExtrasIntent {
  tab: ExtrasTab;
  /** Start this workflow (by id) as soon as the Workflows section opens. */
  runWorkflow?: string;
  /** Put this in the search box. */
  query?: string;
}

export const extrasIntent = writable<ExtrasIntent | null>(null);

export function openExtras(tab: ExtrasTab, more: Omit<ExtrasIntent, 'tab'> = {}): void {
  extrasIntent.set({ tab, ...more });
  openTool(EXTRAS_TOOL);
}

/** Read the pending intent and clear it, so it is applied exactly once. */
export function takeExtrasIntent(): ExtrasIntent | null {
  const i = get(extrasIntent);
  if (i) extrasIntent.set(null);
  return i;
}
