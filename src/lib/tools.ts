// The tool registry: order in the left rail, labels, icon names, and the
// one-line descriptions used by the command palette and Home cards.

import type { ToolId } from './types';

export interface ToolDef {
  id: ToolId;
  label: string;
  icon: string;
  hint: string;
}

export const TOOLS: ToolDef[] = [
  { id: 'home', label: 'Home', icon: 'home', hint: 'Everything at a glance' },
  { id: 'clipboard', label: 'Clipboard', icon: 'clipboard', hint: 'Searchable copy history' },
  { id: 'paste', label: 'Paste Stack', icon: 'paste', hint: 'Numbered paste slots 1–9' },
  { id: 'timers', label: 'Timers', icon: 'timers', hint: 'Countdowns & stopwatch' },
  { id: 'pomodoro', label: 'Pomodoro', icon: 'pomodoro', hint: 'Work / break focus cycles' },
  { id: 'shelf', label: 'File Shelf', icon: 'shelf', hint: 'Park files within reach' },
  { id: 'media', label: 'Media', icon: 'media', hint: 'Now playing & transport' },
  { id: 'calendar', label: 'Calendar', icon: 'calendar', hint: 'Month view & reminders' },
  { id: 'weather', label: 'Weather', icon: 'weather', hint: 'Current & 5-day forecast' },
  { id: 'notes', label: 'Notes', icon: 'notes', hint: 'Fast scratch notes' },
  { id: 'clocks', label: 'World Clocks', icon: 'clocks', hint: 'Time zones at a glance' },
  { id: 'convert', label: 'Convert', icon: 'convert', hint: 'Units & currency' },
  { id: 'calculator', label: 'Calculator', icon: 'calculator', hint: 'Evaluate expressions' },
  { id: 'screentime', label: 'Screen Time', icon: 'screentime', hint: 'Top apps per day' },
  { id: 'settings', label: 'Settings', icon: 'settings', hint: 'Hotkey, theme, tools' },
];

export const TOOL_BY_ID: Record<ToolId, ToolDef> = Object.fromEntries(
  TOOLS.map((t) => [t.id, t]),
) as Record<ToolId, ToolDef>;
