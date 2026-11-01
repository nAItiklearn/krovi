// Island shell API (owner: panel-shell). STUB matching docs/CONTRACTS.md §1 —
// the panel-shell agent replaces the internals; the exported surface is the contract.
import { writable, type Readable } from 'svelte/store';
import { isTauri } from './bridge';

export type ShellMode = 'pill' | 'panel' | 'hidden';
export type ShellAction = 'expand' | 'collapse' | 'toggle';
export interface ShellConfig {
  shortcut: string;
  showPill: boolean;
  collapseOnBlur: boolean;
  launchAtLogin: boolean;
}

export const PILL_WIN = { w: 440, h: 88 };
export const PANEL_WIN = { w: 960, h: 690 };
export const PANEL = { w: 900, h: 620 };
export const ISLAND_PAD = 10;

const _mode = writable<ShellMode>('pill');
export const shellMode: Readable<ShellMode> = _mode;

const listeners = new Set<(a: ShellAction) => void>();
export function onShellRequest(cb: (a: ShellAction) => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

export function isIsland(): boolean {
  return isTauri();
}

/** UI wants the panel: grows the window first, then the UI animates. */
export async function requestExpand(): Promise<void> {
  _mode.set('panel');
}
/** UI wants the pill: the UI animates first, then calls commitMode('pill'). */
export async function requestCollapse(): Promise<void> {
  listeners.forEach((l) => l('collapse'));
}
/** Called by the UI when its animation has finished. */
export async function commitMode(mode: ShellMode): Promise<void> {
  _mode.set(mode);
}
export async function quit(): Promise<void> {}
export async function hold(_b: boolean): Promise<void> {}
export async function getConfig(): Promise<ShellConfig> {
  return { shortcut: 'Ctrl+Alt+Space', showPill: true, collapseOnBlur: true, launchAtLogin: false };
}
export async function setConfig(c: ShellConfig): Promise<ShellConfig> {
  return c;
}
