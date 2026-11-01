// The tool registry: order in the rail, labels, icon names, and the one-line descriptions
// used by the command palette and the header. Icons are keys of `lib/icons.ts`.

import type { ToolId } from './types';
import type { IconName } from './icons';

export interface ToolDef {
  id: ToolId;
  label: string;
  icon: IconName;
  hint: string;
  /** extra palette search terms */
  keywords: string[];
}

export const TOOLS: ToolDef[] = [
  { id: 'home', label: 'Home', icon: 'home', hint: 'Everything at a glance', keywords: ['dashboard', 'overview'] },
  { id: 'clipboard', label: 'Clipboard', icon: 'clipboard', hint: 'Searchable copy history', keywords: ['copy', 'history', 'paste'] },
  { id: 'paste', label: 'Paste Stack', icon: 'paste', hint: 'Numbered paste slots', keywords: ['queue', 'slots', 'stack'] },
  { id: 'timers', label: 'Timers', icon: 'timers', hint: 'Countdowns and stopwatch', keywords: ['countdown', 'stopwatch', 'alarm'] },
  { id: 'pomodoro', label: 'Pomodoro', icon: 'pomodoro', hint: 'Focus and break cycles', keywords: ['focus', 'work', 'break'] },
  { id: 'shelf', label: 'File Shelf', icon: 'shelf', hint: 'Park files within reach', keywords: ['files', 'drop', 'tray'] },
  { id: 'transfer', label: 'Quick Transfer', icon: 'transfer', hint: 'Send files between devices', keywords: ['send', 'phone', 'share', 'files', 'airdrop', 'qr'] },
  { id: 'media', label: 'Media', icon: 'media', hint: 'Now playing and transport', keywords: ['music', 'spotify', 'player', 'song'] },
  { id: 'calendar', label: 'Calendar', icon: 'calendar', hint: 'Month view and holidays', keywords: ['date', 'holiday', 'reminder', 'events'] },
  { id: 'weather', label: 'Weather', icon: 'weather', hint: 'Current conditions and forecast', keywords: ['forecast', 'temperature', 'rain'] },
  { id: 'notes', label: 'Notes', icon: 'notes', hint: 'Fast scratch notes', keywords: ['write', 'memo', 'dictate'] },
  { id: 'explain', label: 'Explain', icon: 'bookOpen', hint: 'Summarize & explain any text', keywords: ['summarize', 'summary', 'explain', 'read aloud', 'tldr', 'simplify', 'text'] },
  { id: 'clocks', label: 'World Clocks', icon: 'clocks', hint: 'Time zones at a glance', keywords: ['time', 'timezone'] },
  { id: 'convert', label: 'Convert', icon: 'convert', hint: 'Units and currency', keywords: ['unit', 'currency', 'exchange'] },
  { id: 'calculator', label: 'Calculator', icon: 'calculator', hint: 'Evaluate expressions', keywords: ['math', 'calc'] },
  { id: 'screentime', label: 'Screen Time', icon: 'screentime', hint: 'Where your day went', keywords: ['apps', 'usage', 'focus'] },
  { id: 'settings', label: 'Settings', icon: 'settings', hint: 'Shortcut, theme, behaviour', keywords: ['preferences', 'options', 'hotkey'] },
];

export const TOOL_BY_ID: Record<ToolId, ToolDef> = Object.fromEntries(
  TOOLS.map((t) => [t.id, t]),
) as Record<ToolId, ToolDef>;

export const TOOL_ORDER: ToolId[] = TOOLS.map((t) => t.id);

export function isToolId(v: unknown): v is ToolId {
  return typeof v === 'string' && v in TOOL_BY_ID;
}
