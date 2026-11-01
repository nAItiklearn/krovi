import { describe, expect, it, vi } from 'vitest';
import { createVoiceRouter, showFeedbackOnly, type RouterDeps } from './voiceRouter';
import type { MediaState, ToolId } from '../lib/types';
import type { VoiceResult } from '../lib/voice';

const TOOLS = ['home', 'notes', 'media', 'transfer', 'timers', 'pomodoro'];

function setup(over: Partial<RouterDeps> = {}) {
  const deps = {
    demo: true,
    isToolId: (v: string): v is ToolId => TOOLS.includes(v),
    openTool: vi.fn(),
    pomodoroStart: vi.fn(),
    timerAdd: vi.fn(),
    timerControl: vi.fn(() => 'Tea paused'),
    addNote: vi.fn(async () => true),
    addToShelf: vi.fn(async (e: { name: string; path: string }[]) => ({ count: e.length, shelf: 'Desk' })),
    requestTransfer: vi.fn(),
    notify: vi.fn(),
    collapse: vi.fn(),
    addCalendarEvent: vi.fn(async () => undefined),
    calendarChanged: vi.fn(),
    media: vi.fn((): MediaState | null => null),
    mediaTransport: vi.fn(),
    today: () => '2030-05-10',
    ...over,
  } satisfies RouterDeps;
  return { deps, route: createVoiceRouter(deps) };
}

const result = (over: Partial<VoiceResult> = {}): VoiceResult => ({ mode: 'command', text: '', command: null, ...over });

describe('v1 commands', () => {
  it('opens a known tool and ignores an unknown one', async () => {
    const { deps, route } = setup();
    await route({ kind: 'open-tool', tool: 'notes' });
    await route({ kind: 'open-tool', tool: 'bogus' as ToolId });
    expect(deps.openTool).toHaveBeenCalledTimes(1);
    expect(deps.openTool).toHaveBeenCalledWith('notes');
  });

  it('starts timers and pomodoros, creates notes in the UI, collapses', async () => {
    const { deps, route } = setup();
    await route({ kind: 'start-timer', seconds: 600 });
    expect(deps.timerAdd).toHaveBeenCalledWith('Voice timer', 10, true);
    await route({ kind: 'start-pomodoro' });
    expect(deps.pomodoroStart).toHaveBeenCalled();
    await route({ kind: 'new-note', body: 'buy milk' });
    expect(deps.addNote).toHaveBeenCalledWith('buy milk');
    expect(deps.notify).toHaveBeenLastCalledWith('Note saved', undefined);
    await route({ kind: 'collapse' });
    expect(deps.collapse).toHaveBeenCalled();
  });

  it('says what "this" was when a note is made from a selection', async () => {
    const { deps, route } = setup();
    await route(
      { kind: 'new-note', body: 'revenue is up' },
      result({ context: { kind: 'text', value: 'revenue is up', app: 'chrome.exe' } }),
    );
    expect(deps.notify).toHaveBeenLastCalledWith('Note saved', {
      icon: 'text',
      label: '“revenue is up”',
      title: 'Using your selection in chrome: “revenue is up”',
    });
  });
});

describe('note creation order', () => {
  it('creates the note first and only then opens Notes', async () => {
    const order: string[] = [];
    const { route } = setup({
      addNote: vi.fn(async () => { order.push('create'); return true; }),
      openTool: vi.fn(() => { order.push('open'); }),
    });
    await route({ kind: 'new-note', body: 'x' });
    expect(order).toEqual(['create', 'open']);
  });

  it('does not open Notes when the note could not be saved', async () => {
    const { deps, route } = setup({ addNote: vi.fn(async () => false) });
    await route({ kind: 'new-note', body: 'x' });
    expect(deps.openTool).not.toHaveBeenCalled();
  });
});

describe('results without a command', () => {
  it('shows the backend line, with what it used', () => {
    const notify = vi.fn();
    showFeedbackOnly(
      result({ feedback: 'I didn’t understand that', context: { kind: 'app', value: 'notepad.exe' } }),
      notify,
    );
    expect(notify).toHaveBeenCalledWith('I didn’t understand that', {
      icon: 'monitor',
      label: 'In notepad',
      title: 'Using the foreground app: notepad',
    });
  });

  it('stays quiet without feedback, and leaves commands to the router', () => {
    const notify = vi.fn();
    showFeedbackOnly(result({}), notify);
    showFeedbackOnly(result({ feedback: 'Done', command: { kind: 'collapse' } }), notify);
    expect(notify).not.toHaveBeenCalled();
  });
});

describe('voice v3 local actions', () => {
  it('never repeats what Rust already did: it only says it, or leaves rows to the sheet', async () => {
    const { deps, route } = setup();
    await route({ kind: 'open-folder', name: 'Downloads', path: 'C:\\Users\\you\\Downloads' }, result({ executed: true, feedback: 'Opened Downloads.' }));
    expect(deps.notify).toHaveBeenLastCalledWith('Opened Downloads.');
    await route({ kind: 'open-app', name: 'Notepad', path: 'shell:x' }, result({ executed: true, feedback: 'Launched Notepad.' }));
    expect(deps.notify).toHaveBeenLastCalledWith('Launched Notepad.');

    (deps.notify as ReturnType<typeof vi.fn>).mockClear();
    await route(
      { kind: 'search-files', query: 'invoice' },
      result({ executed: true, feedback: 'Found 1 match.', results: [{ kind: 'file', name: 'a.pdf', path: 'C:\\a.pdf' }] }),
    );
    await route(
      { kind: 'list-folder', name: 'Downloads', path: 'C:\\d' },
      result({ executed: true, feedback: 'Downloads: 1 item.', results: [{ kind: 'file', name: 'b', path: 'C:\\b' }] }),
    );
    expect(deps.notify).not.toHaveBeenCalled();
    // none of them touches tools, shelf, transfer, media or the calendar
    expect(deps.openTool).not.toHaveBeenCalled();
    expect(deps.addToShelf).not.toHaveBeenCalled();
    expect(deps.addCalendarEvent).not.toHaveBeenCalled();
    expect(deps.mediaTransport).not.toHaveBeenCalled();
  });

  it('has a plain line ready if the backend sent none', async () => {
    const { deps, route } = setup();
    await route({ kind: 'open-folder', name: 'Music', path: 'p' });
    expect(deps.notify).toHaveBeenLastCalledWith('Opened Music');
    await route({ kind: 'search-files', query: 'tax' });
    expect(deps.notify).toHaveBeenLastCalledWith('No matches for “tax”');
    await route({ kind: 'list-folder', name: 'Desktop', path: 'p' });
    expect(deps.notify).toHaveBeenLastCalledWith('Desktop is empty');
  });

  it('leaves a no-command result with rows (ambiguous app) to the sheet', () => {
    const notify = vi.fn();
    showFeedbackOnly(
      result({ feedback: 'Several apps match "photo".', results: [{ kind: 'app', name: 'Photo Editor Pro', path: 'shell:a' }] }),
      notify,
    );
    expect(notify).not.toHaveBeenCalled();
  });
});

describe('timer-control', () => {
  it('delegates to the timers and shows the line it returns', async () => {
    const { deps, route } = setup();
    await route({ kind: 'timer-control', action: 'pause', target: 'timer' });
    expect(deps.timerControl).toHaveBeenCalledWith('pause', 'timer');
    expect(deps.notify).toHaveBeenLastCalledWith('Tea paused');
  });
});

describe('add-reminder', () => {
  it('adds the event itself in the demo and refreshes Calendar', async () => {
    const { deps, route } = setup();
    await route({ kind: 'add-reminder', date: '2030-05-11', time: '09:00', title: 'Call mom' });
    expect(deps.addCalendarEvent).toHaveBeenCalledWith({ date: '2030-05-11', title: 'Call mom', time: '09:00' });
    expect(deps.calendarChanged).toHaveBeenCalled();
    expect(deps.notify).toHaveBeenLastCalledWith('Reminder set · Tomorrow, 09:00', undefined);
  });

  it('only shows the backend line when Rust already saved it', async () => {
    const { deps, route } = setup({ demo: false });
    await route(
      { kind: 'add-reminder', date: '2030-05-11', time: '09:00', title: 'Call mom' },
      result({ executed: true, feedback: 'Reminder added for Sat 11 May, 9:00 AM: Call mom' }),
    );
    expect(deps.addCalendarEvent).not.toHaveBeenCalled();
    expect(deps.calendarChanged).toHaveBeenCalled();
    expect(deps.notify).toHaveBeenLastCalledWith('Reminder added for Sat 11 May, 9:00 AM: Call mom', undefined);
  });

  it('falls back to adding it when the backend did not', async () => {
    const { deps, route } = setup({ demo: false });
    await route({ kind: 'add-reminder', date: '2030-05-12', title: 'Dentist' }, result({ executed: false }));
    expect(deps.addCalendarEvent).toHaveBeenCalledWith({ date: '2030-05-12', title: 'Dentist', time: null });
  });

  it('refuses a reminder it cannot understand and reports a failed save', async () => {
    const { deps, route } = setup();
    await route({ kind: 'add-reminder', date: 'tomorrow', title: 'x' });
    await route({ kind: 'add-reminder', date: '2030-05-11', title: '   ' });
    expect(deps.addCalendarEvent).not.toHaveBeenCalled();
    expect(deps.notify).toHaveBeenLastCalledWith('I couldn’t work out that reminder');

    const failing = setup({ addCalendarEvent: vi.fn(async () => { throw new Error('disk full'); }) });
    await failing.route({ kind: 'add-reminder', date: '2030-05-11', title: 'Call mom' });
    expect(failing.deps.notify).toHaveBeenLastCalledWith('Couldn’t save that reminder');
    expect(failing.deps.calendarChanged).not.toHaveBeenCalled();
  });
});

describe('shelf-add', () => {
  it('parks file references and says what they came from', async () => {
    const { deps, route } = setup();
    await route(
      { kind: 'shelf-add', paths: ['C:\\Users\\me\\report.pdf', 'C:\\x\\a.png'] },
      result({ context: { kind: 'files', value: ['C:\\Users\\me\\report.pdf', 'C:\\x\\a.png'] } }),
    );
    expect(deps.addToShelf).toHaveBeenCalledWith([
      { name: 'report.pdf', path: 'C:\\Users\\me\\report.pdf' },
      { name: 'a.png', path: 'C:\\x\\a.png' },
    ]);
    expect(deps.notify).toHaveBeenLastCalledWith('2 files added to Desk', { icon: 'file', label: '2 files', title: 'Using 2 files' });
  });

  it('is honest that the shelf holds files: text becomes a note', async () => {
    const { deps, route } = setup();
    await route({ kind: 'shelf-add', paths: [], text: 'a quote worth keeping' });
    expect(deps.addToShelf).not.toHaveBeenCalled();
    expect(deps.addNote).toHaveBeenCalledWith('a quote worth keeping');
    expect(deps.notify).toHaveBeenLastCalledWith('The shelf holds files, so I saved that as a note', undefined);
  });

  it('handles an empty capture and a duplicate', async () => {
    const { deps, route } = setup({ addToShelf: vi.fn(async () => ({ count: 0, shelf: 'Desk' })) });
    await route({ kind: 'shelf-add', paths: [] });
    expect(deps.notify).toHaveBeenLastCalledWith('Nothing selected to put on the shelf');
    await route({ kind: 'shelf-add', paths: ['C:\\a.txt'] });
    expect(deps.notify).toHaveBeenLastCalledWith('Already on Desk', undefined);
  });
});

describe('transfer-send', () => {
  it('hands paths to Quick Transfer and explains when it only got text or nothing', async () => {
    const { deps, route } = setup();
    await route({ kind: 'transfer-send', paths: ['C:\\a.zip'] });
    expect(deps.requestTransfer).toHaveBeenCalledWith(['C:\\a.zip']);

    await route({ kind: 'transfer-send', paths: [], text: 'hello' });
    expect(deps.requestTransfer).toHaveBeenCalledTimes(1);
    expect(deps.openTool).toHaveBeenLastCalledWith('transfer');
    expect(deps.notify).toHaveBeenLastCalledWith('Quick Transfer sends files. Select a file and ask again.');

    await route({ kind: 'transfer-send', paths: [] });
    expect(deps.notify).toHaveBeenLastCalledWith('Nothing selected to send');
  });
});

describe('media', () => {
  const playing = { title: 'Midnight Drive', artist: 'Neon Harbor', playing: true } as MediaState;

  it('presses the key itself only when the backend did not, and never the wrong way', async () => {
    const demo = setup({ media: () => playing });
    await demo.route({ kind: 'media', command: 'pause' });
    expect(demo.deps.mediaTransport).toHaveBeenCalledWith('playpause');

    const native = setup({ demo: false, media: () => playing });
    await native.route({ kind: 'media', command: 'next' }, result({ executed: true, feedback: 'Skipped to the next track' }));
    expect(native.deps.mediaTransport).not.toHaveBeenCalled();
    expect(native.deps.notify).toHaveBeenLastCalledWith('Skipped to the next track');

    const paused = setup({ media: () => ({ ...playing, playing: false }) });
    await paused.route({ kind: 'media', command: 'pause' });
    expect(paused.deps.mediaTransport).not.toHaveBeenCalled();
  });

  it('answers "what\'s playing" by opening Media and naming the track', async () => {
    const { deps, route } = setup({ media: () => playing });
    await route({ kind: 'media', command: 'whats-playing' });
    expect(deps.openTool).toHaveBeenCalledWith('media');
    expect(deps.notify).toHaveBeenLastCalledWith('Midnight Drive · Neon Harbor');
  });

  it('copes with nothing playing', async () => {
    const { deps, route } = setup();
    await route({ kind: 'media', command: 'whats-playing' });
    expect(deps.notify).toHaveBeenLastCalledWith('Nothing is playing');
  });
});
