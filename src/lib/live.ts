// Live data stores (owner: live-data). STUB matching docs/CONTRACTS.md §2 —
// the live-data agent replaces the internals; the exported surface is the contract.
import { writable } from 'svelte/store';
import type { ClipEntry } from './types';

export interface MediaState {
  title: string;
  artist: string;
  album: string;
  playing: boolean;
  positionSec: number;
  durationSec: number;
  sampledAt: number;
  sourceApp: string;
  artwork: string | null;
}
export interface SystemStats {
  memUsed: number;
  memTotal: number;
  diskUsed: number;
  diskTotal: number;
  diskLabel: string;
  cpu: number;
}

export const clips = writable<ClipEntry[]>([]);
export const media = writable<MediaState | null>(null);
export const stats = writable<SystemStats | null>(null);
/** Increments on every NEW clipboard entry — drives the pill's arrival glow. */
export const pulse = writable<number>(0);

export function startLive(): void {}
export async function copyClip(_id: number): Promise<void> {}
export async function togglePin(_id: number): Promise<void> {}
export async function removeClip(_id: number): Promise<void> {}
export async function clearClips(): Promise<void> {}
export async function mediaTransport(_cmd: 'playpause' | 'next' | 'previous'): Promise<void> {}
