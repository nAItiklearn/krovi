// Content for the "What can I say?" sheet. Voice is a closed grammar (docs/SPEC.md 11.13): it
// listens only for these phrasings, so this list is the product's real contract, not marketing.
// Keep the templates in step with SPEC 11.13; notation: {a|b} one of, [x] optional, <n> a number,
// <text> free speech.

import type { IconName } from '../lib/icons';
import type { VoiceTargets } from '../lib/voice';

export interface HelpFamily {
  id: string;
  title: string;
  icon: IconName;
  /** exact grammar, in SPEC notation */
  template: string;
  /** phrases that work as written */
  examples: string[];
  note?: string;
}

export const HELP_FAMILIES: HelpFamily[] = [
  {
    id: 'tools',
    title: 'Open a Krovi tool',
    icon: 'home',
    template: '{open|show|show me|go to|switch to|launch|take me to|bring up|view} [the|my] <tool>',
    examples: ['open the calendar', 'show me clipboard', 'go to settings'],
    note: 'Tools: home, clipboard, paste stack, timers, pomodoro, shelf, media, calendar, weather, notes, world clocks, convert, calculator, screen time, settings. A Krovi tool wins over a folder or app of the same name: say “launch settings” for the Windows app, “open music folder” for the folder.',
  },
  {
    id: 'timers',
    title: 'Timers and pomodoro',
    icon: 'timers',
    template: '{start|set|begin} [a|the] timer for <n> {seconds|minutes|hours}',
    examples: ['start a timer for five minutes', 'set a ten minute timer', 'start a pomodoro', 'pause the timer', 'stop the pomodoro'],
    note: 'Also “timer for ten minutes”, “a timer for half an hour”, and {pause|resume|stop|cancel} the {timer|pomodoro|stopwatch}.',
  },
  {
    id: 'notes',
    title: 'Notes',
    icon: 'notes',
    template: '{new note|take a note|make a note|add a note|note to self} <text>',
    examples: ['new note buy milk', 'take a note', 'save this as a note'],
    note: 'Alone, the prefix just opens Notes. “Save this as a note” saves your selection and needs one.',
  },
  {
    id: 'reminders',
    title: 'Reminders',
    icon: 'bell',
    template: '{add a reminder|set a reminder|create a reminder|remind me} <when and what>',
    examples: [
      'set a reminder for tomorrow at nine thirty in the morning to renew the passport',
      'remind me at five pm to call mom',
    ],
    note: 'Understood: today, tonight, tomorrow, friday, “in twenty minutes”, “half past three”, “at noon”, “march 3rd”. The confirmation states the exact date and time it set.',
  },
  {
    id: 'this',
    title: 'Your selection (“this”)',
    icon: 'sparkles',
    template: '{put|copy|add|save|move|drop} this {on|to} the shelf · send this to my phone',
    examples: ['put this on the shelf', 'send this to my phone', 'save this as a note'],
    note: 'Needs something selected when you start the command. Krovi reads it only at that moment.',
  },
  {
    id: 'media',
    title: 'Media',
    icon: 'media',
    template: '{next track|previous track|pause music|play music|what’s playing}',
    examples: ['next track', 'pause music', 'what’s playing'],
  },
  {
    id: 'folders',
    title: 'Folders',
    icon: 'folder',
    template: '{open|show|go to|browse|take me to} [the|my] <folder>  ·  {what’s in|list|tell me about} [the|my] <folder>',
    examples: ['open downloads', 'show my pictures', 'open krovi downloads', 'what’s in downloads'],
    note: 'Only the folders listed below. Listing reads the folder on this PC: item count, size and the newest files.',
  },
  {
    id: 'apps',
    title: 'Apps',
    icon: 'appWindow',
    template: '{open|launch|start|run} [the] <app>',
    examples: ['open notepad', 'launch spotify', 'start firefox', 'launch visual studio code'],
    note: 'Apps from your Start menu, by the name shown there. If a name matches several apps, nothing launches and you get the candidates to pick.',
  },
  {
    id: 'find',
    title: 'Find a file',
    icon: 'search',
    template: '{find file|find the file|find me|search for|search my files for} <text>',
    examples: ['search for tax return', 'find the file budget', 'find me boarding pass'],
    note: 'Searches file names in the Windows index, newest first. A found file is shown in Explorer, never run.',
  },
  {
    id: 'close',
    title: 'Close',
    icon: 'close',
    template: '{close|hide|dismiss|collapse} [the] [panel|krovi]',
    examples: ['close', 'go away', 'never mind', 'that is all'],
  },
];

/** What voice does not do, so nobody wonders why a clever sentence did nothing. */
export const HELP_LIMITS =
  'Anything else is ignored, never guessed at or sent anywhere: open-ended questions, several commands in one sentence, subfolders, websites, or doing things to a found file.';

export type TargetsStatus = 'loading' | 'partial' | 'ready';

/** `null` = the first answer has not arrived yet; `ready:false` = folders are known, apps still scanning. */
export function targetsStatus(t: VoiceTargets | null): TargetsStatus {
  if (!t) return 'loading';
  return t.ready ? 'ready' : 'partial';
}

export interface TargetView {
  folders: { name: string; path: string }[];
  apps: string[];
  /** apps that matched but did not fit under the cap */
  moreApps: number;
  /** nothing matched the filter */
  empty: boolean;
}

/** Filter folders and apps by a typed fragment; apps are capped so hundreds of names stay readable. */
export function filterTargets(t: VoiceTargets | null, query: string, appCap = 48): TargetView {
  const q = query.trim().toLowerCase();
  const hit = (s: string) => !q || s.toLowerCase().includes(q);
  const folders = (t?.folders ?? []).filter((f) => hit(f.name));
  const allApps = (t?.apps ?? []).filter(hit);
  const apps = allApps.slice(0, appCap);
  return { folders, apps, moreApps: allApps.length - apps.length, empty: !!q && !folders.length && !allApps.length };
}
