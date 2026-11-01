// Voice commands -> the same entry points clicks and keys use. Written against injected
// dependencies so every branch is unit-tested without a DOM, a backend or a clock.
//
// The voice module marks a result `executed` when the backend already carried the command out
// (a reminder saved, a media key sent): then the UI only shows the feedback and refreshes what
// changed. In the browser demo nothing is executed, so the UI performs every action itself.

import type { MediaState, ToolId } from '../lib/types';
import type { IconName } from '../lib/icons';
import type { VoiceCommand, VoiceResult } from '../lib/voice';
import { relativeDay, toISO } from './agenda';
import { MEDIA_FEEDBACK, contextChip, fileName, transportFor } from './voiceModel';

export type Chip = { icon: IconName; label: string; title: string };

export interface RouterDeps {
  /** Browser demo: there is no Rust side, so the UI performs actions the backend would. */
  demo: boolean;
  isToolId: (v: string) => v is ToolId;
  openTool: (id: ToolId) => void;
  pomodoroStart: () => void;
  timerAdd: (label: string, minutes: number, start: boolean) => void;
  /** pause / resume / stop the running timer or the pomodoro; resolves to a line to show */
  timerControl: (action: 'pause' | 'resume' | 'stop', target: 'timer' | 'pomodoro') => string;
  addNote: (body: string) => Promise<boolean>;
  addToShelf: (entries: { name: string; path: string }[]) => Promise<{ count: number; shelf: string } | null>;
  requestTransfer: (paths: string[]) => void;
  /** show a line where the user is looking: a toast in the panel, an activity line on the pill */
  notify: (text: string, chip?: Chip) => void;
  collapse: () => void;
  addCalendarEvent: (ev: { date: string; title: string; time?: string | null }) => Promise<unknown>;
  calendarChanged: () => void;
  media: () => MediaState | null;
  mediaTransport: (cmd: 'previous' | 'playpause' | 'next') => void;
  today?: () => string;
}

function localAction(c: Extract<VoiceCommand, { kind: 'open-folder' | 'open-app' | 'search-files' | 'list-folder' }>): string {
  switch (c.kind) {
    case 'open-folder':
      return `Opened ${c.name}`;
    case 'open-app':
      return `Launched ${c.name}`;
    case 'search-files':
      return `No matches for “${c.query}”`;
    default:
      return `${c.name} is empty`;
  }
}

export type VoiceRoute = (c: VoiceCommand, result?: VoiceResult) => Promise<void>;

/** A finished session with no command: nothing to run, but the backend may have something to say. */
export function showFeedbackOnly(result: VoiceResult, notify: RouterDeps['notify']): void {
  // rows go to the results sheet, which carries the line itself
  if (result.command == null && result.feedback && !result.results?.length) notify(result.feedback, contextChip(result.context) ?? undefined);
}

export function createVoiceRouter(d: RouterDeps): VoiceRoute {
  const today = d.today ?? (() => toISO(new Date()));

  async function reminder(c: Extract<VoiceCommand, { kind: 'add-reminder' }>, r?: VoiceResult, ctx?: Chip): Promise<void> {
    if (r?.executed) {
      // Rust saved it already: show its line and let Calendar and Home reload
      d.calendarChanged();
      d.notify(r.feedback || 'Reminder added', ctx);
      return;
    }
    const title = (c.title ?? '').trim();
    const time = c.time || null;
    if (!title || !/^\d{4}-\d{2}-\d{2}$/.test(c.date ?? '')) {
      d.notify('I couldn’t work out that reminder');
      return;
    }
    try {
      await d.addCalendarEvent({ date: c.date, title, time });
      d.calendarChanged();
      d.notify(`Reminder set · ${relativeDay(c.date, today())}${time ? `, ${time}` : ''}`, ctx);
    } catch {
      d.notify('Couldn’t save that reminder');
    }
  }

  // "put this on the shelf": the shelf holds file references; plain text becomes a note instead
  async function shelve(c: Extract<VoiceCommand, { kind: 'shelf-add' }>, ctx?: Chip): Promise<void> {
    const paths = (c.paths ?? []).filter(Boolean);
    if (paths.length) {
      const res = await d.addToShelf(paths.map((p) => ({ name: fileName(p), path: p })));
      if (!res) d.notify('Couldn’t add that to the shelf');
      else if (res.count === 0) d.notify(`Already on ${res.shelf}`, ctx);
      else d.notify(`${res.count} ${res.count === 1 ? 'file' : 'files'} added to ${res.shelf}`, ctx);
      return;
    }
    const text = (c.text ?? '').trim();
    if (!text) {
      d.notify('Nothing selected to put on the shelf');
      return;
    }
    const ok = await d.addNote(text);
    d.notify(ok ? 'The shelf holds files, so I saved that as a note' : 'Couldn’t save that', ctx);
  }

  function media(c: Extract<VoiceCommand, { kind: 'media' }>, r?: VoiceResult): void {
    if (r?.executed) {
      d.notify(r.feedback || MEDIA_FEEDBACK[c.command]);
      return;
    }
    const m = d.media();
    if (c.command === 'whats-playing') {
      d.openTool('media');
      d.notify(m ? `${m.title}${m.artist ? ` · ${m.artist}` : ''}` : 'Nothing is playing');
      return;
    }
    const t = transportFor(c.command, m?.playing ?? false);
    if (t) d.mediaTransport(t);
    d.notify(MEDIA_FEEDBACK[c.command]);
  }

  return async function route(c, r) {
    const ctx = contextChip(r?.context) ?? undefined;
    switch (c.kind) {
      case 'open-tool':
        if (d.isToolId(c.tool)) d.openTool(c.tool);
        return;
      case 'start-pomodoro':
        d.pomodoroStart();
        d.openTool('pomodoro');
        return;
      case 'start-timer':
        d.timerAdd('Voice timer', c.seconds / 60, true);
        d.openTool('timers');
        d.notify('Timer started');
        return;
      case 'timer-control':
        d.notify(d.timerControl(c.action, c.target));
        return;
      case 'new-note': {
        // command mode: Rust only parsed the phrase, creating the note is ours; then show it
        const ok = await d.addNote(c.body);
        if (ok) {
          d.openTool('notes');
          d.notify('Note saved', ctx);
        }
        return;
      }
      case 'collapse':
        d.collapse();
        return;
      case 'add-reminder':
        return reminder(c, r, ctx);
      case 'shelf-add':
        return shelve(c, ctx);
      case 'transfer-send':
        if (c.paths?.length) d.requestTransfer(c.paths);
        else {
          d.openTool('transfer');
          d.notify(c.text?.trim() ? 'Quick Transfer sends files. Select a file and ask again.' : 'Nothing selected to send');
        }
        return;
      case 'media':
        media(c, r);
        return;
      case 'open-folder':
      case 'open-app':
      case 'search-files':
      case 'list-folder':
        // Rust already did it (opened, launched, searched, listed): never repeat it here. Rows, if
        // any, are on the results sheet; otherwise just say what happened.
        if (!r?.results?.length) d.notify(r?.feedback || localAction(c));
        return;
    }
  };
}
