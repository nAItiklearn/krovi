// Voice API (owner: voice). STUB matching docs/CONTRACTS.md §3 —
// the voice agent replaces the internals; the exported surface is the contract.
import { writable } from 'svelte/store';
import type { ToolId } from './types';

export type VoiceMode = 'note' | 'command';
export type VoiceCommand =
  | { kind: 'open-tool'; tool: ToolId }
  | { kind: 'start-pomodoro' }
  | { kind: 'start-timer'; seconds: number }
  | { kind: 'new-note'; body: string }
  | { kind: 'collapse' };
export interface VoiceStateV {
  state: 'idle' | 'listening' | 'processing' | 'error' | 'unavailable';
  mode: VoiceMode | null;
  partial: string;
  error?: string;
}

export const voiceState = writable<VoiceStateV>({ state: 'idle', mode: null, partial: '' });

export async function startVoice(_mode: VoiceMode): Promise<void> {}
export async function stopVoice(): Promise<void> {}
export function onVoiceCommand(_cb: (c: VoiceCommand) => void): () => void {
  return () => {};
}
