// Agent Mode bridge in the browser demo (no Tauri): the planner stand-in, the safe / confirm /
// blocked rules, the task engine, the integration catalog and the MCP registry. Stores are mocked
// so the effects (timer, note, reminder) can be observed.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  timerAdd: vi.fn(),
  addNote: vi.fn(async (_body: string) => true),
  pushToast: vi.fn(),
  calendarTick: { update: vi.fn() },
}));
vi.mock('./stores', () => mocks);

import {
  EXAMPLES,
  MAX_TASKS,
  __resetDemo,
  agent,
  describeTransport,
  draftToInput,
  formatSeconds,
  hasRunnable,
  isFinal,
  longDuration,
  mergeTask,
  parseArgs,
  splitSteps,
  startAgent,
  statusLabel,
  stepIcon,
  taskFor,
  timerFromEvent,
  type AgentPlan,
  type AgentStep,
  type AgentTask,
} from './agent';
import { bridge, mode } from './bridge';

beforeEach(() => {
  __resetDemo();
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(new Date(2026, 10, 2, 10, 0, 30)); // Mon 2 Nov 2026, 10:00:30 local
  mocks.timerAdd.mockClear();
  mocks.addNote.mockClear();
  mocks.addNote.mockImplementation(async () => true);
  mocks.pushToast.mockClear();
  mocks.calendarTick.update.mockClear();
});
afterEach(() => {
  vi.useRealTimers();
});

const levels = (p: AgentPlan) => p.steps.map((s) => s.risk.level);

describe('the demo planner', () => {
  it('runs in demo mode in a plain environment', () => {
    expect(mode()).toBe('demo');
  });

  it('classifies steps as safe, confirm or blocked', async () => {
    for (const [text, want] of [
      ['open youtube', ['safe']],
      ['go to github.com', ['safe']],
      ['launch notepad', ['safe']],
      ['search google for rust', ['safe']],
      ['play lofi on youtube', ['safe']],
      ['set a timer for 10 minutes', ['safe']],
      ['note buy milk', ['safe']],
      ['go to example.org', ['confirm']],
      ['remind me in 20 minutes to stretch', ['confirm']],
      ['email sam@example.com about the trip', ['confirm']],
      ['open javascript:alert(1)', ['blocked']],
      ['open nonexistentapp', ['blocked']],
      ['set a timer for 25 hours', ['blocked']],
      ['email mom about dinner', ['blocked']],
      ['tell me a joke', ['blocked']],
      ['open youtube then go to example.org then open nonexistentapp', ['safe', 'confirm', 'blocked']],
    ] as const) {
      const plan = await agent.plan(text);
      expect(levels(plan), text).toEqual(want);
    }
  });

  it('writes a plain summary for each step', async () => {
    const summaries = async (t: string) => (await agent.plan(t)).steps.map((s) => s.summary);
    expect(await summaries('open youtube')).toEqual(['Open youtube.com in your browser']);
    expect(await summaries('search youtube for lofi beats')).toEqual(['Search YouTube for "lofi beats"']);
    expect(await summaries('set a timer for 90 seconds')).toEqual(['Start a timer for 1 minute 30 seconds']);
    expect(await summaries('note buy milk')).toEqual(['Save a note: "Buy milk"']);
    expect(await summaries('go to example.org')).toEqual(['Open https://example.org in your browser']);
    expect(await summaries('remind me in 20 minutes to stretch')).toEqual([
      'Remind you on Mon 2 Nov at 10:21 AM: "Stretch"',
    ]);
  });

  it('searches are percent-encoded', async () => {
    const [step] = (await agent.plan('search google for rust & "lifetimes" 100%')).steps;
    expect(step.action).toMatchObject({
      type: 'search',
      url: 'https://www.google.com/search?q=rust%20%26%20%22lifetimes%22%20100%25',
    });
  });

  it('an email draft carries the recipient, subject and no sending', async () => {
    const [step] = (await agent.plan('email sam@example.com about the trip')).steps;
    expect(step.risk.level).toBe('confirm');
    expect(step.action).toEqual({
      type: 'compose-email',
      provider: 'default',
      to: ['sam@example.com'],
      subject: 'The trip',
      body: '',
    });
    expect(step.summary).toContain('Krovi does not send it');
  });

  it('a sentence it does not understand comes back blocked with suggestions', async () => {
    const plan = await agent.plan('what is the weather like');
    expect(plan.steps).toHaveLength(1);
    const [step] = plan.steps;
    expect(step.risk).toEqual({ level: 'blocked', reason: "I didn't understand that." });
    expect(step.action.type).toBe('unsupported');
    expect(plan.suggestions.length).toBeGreaterThanOrEqual(3);
    expect(hasRunnable(plan)).toBe(false);
  });

  it('empty input is blocked, not an empty plan', async () => {
    const plan = await agent.plan('   ');
    expect(plan.steps).toHaveLength(1);
    expect(plan.steps[0].risk.level).toBe('blocked');
  });

  it('refuses dangerous addresses with the reason', async () => {
    for (const text of ['open javascript:alert(1)', 'go to file:///C:/Windows/System32/cmd.exe', 'open data:text/html,hi']) {
      const [step] = (await agent.plan(text)).steps;
      expect(step.risk.level, text).toBe('blocked');
      if (step.risk.level === 'blocked') expect(step.risk.reason).toMatch(/Only http and https/);
    }
  });

  it('each plan gets its own id and is a copy', async () => {
    const a = await agent.plan('open youtube');
    const b = await agent.plan('open youtube');
    expect(a.id).not.toBe(b.id);
    b.steps[0].summary = 'tampered';
    b.steps[0].risk = { level: 'safe' };
    // the stored plan is untouched: running it still says what it said
    const [task] = await agent.run(b.id);
    expect(task.summary).toBe('Open youtube.com in your browser');
  });
});

describe('running a plan (demo)', () => {
  it('safe steps run, confirm steps wait, blocked steps never run', async () => {
    const plan = await agent.plan('open youtube then go to example.org then open nonexistentapp');
    const tasks = await agent.run(plan.id);
    expect(tasks.map((t) => [t.stepId, t.status.state])).toEqual([
      ['s1', 'done'],
      ['s2', 'awaiting-confirmation'],
    ]);
    const all = await agent.tasks();
    expect(all).toHaveLength(2);
    expect(taskFor(all, plan.id, 's3')).toBeUndefined();
  });

  it('a confirmed step runs exactly once', async () => {
    const plan = await agent.plan('open youtube then go to example.org');
    await agent.run(plan.id);
    const confirmed = await agent.run(plan.id, ['s2']);
    expect(confirmed.map((t) => [t.stepId, t.status.state])).toEqual([['s2', 'done']]);
    expect(await agent.run(plan.id, ['s2', 's1'])).toEqual([]);
    expect(await agent.tasks()).toHaveLength(2);
  });

  it('confirming ids that are not confirm steps does nothing', async () => {
    const plan = await agent.plan('open youtube then go to example.org then open nonexistentapp');
    await agent.run(plan.id);
    expect(await agent.run(plan.id, ['s3', 's9', '', 'S2'])).toEqual([]);
    expect(taskFor(await agent.tasks(), plan.id, 's2')?.status.state).toBe('awaiting-confirmation');
  });

  it('can confirm in the first call', async () => {
    const plan = await agent.plan('open youtube then go to example.org');
    const tasks = await agent.run(plan.id, ['s2']);
    expect(tasks.map((t) => t.status.state)).toEqual(['done', 'done']);
  });

  it('a blocked-only plan creates no tasks', async () => {
    const plan = await agent.plan('tell me a joke');
    expect(await agent.run(plan.id, ['s1'])).toEqual([]);
    expect(await agent.tasks()).toEqual([]);
  });

  it('an unknown or expired plan is an error', async () => {
    await expect(agent.run('nope')).rejects.toThrow(/expired/);
  });

  it('cancelling a waiting step stops it for good', async () => {
    const plan = await agent.plan('go to example.org');
    const [waiting] = await agent.run(plan.id);
    expect(waiting.status.state).toBe('awaiting-confirmation');
    const cancelled = await agent.cancel(waiting.id);
    expect(cancelled.status).toEqual({ state: 'cancelled' });
    expect(await agent.run(plan.id, ['s1'])).toEqual([]);
    expect(taskFor(await agent.tasks(), plan.id, 's1')?.status.state).toBe('cancelled');
  });

  it('a finished task cannot be cancelled, and an unknown one is an error', async () => {
    const plan = await agent.plan('open youtube');
    const [done] = await agent.run(plan.id);
    await expect(agent.cancel(done.id)).rejects.toThrow(/cannot be cancelled/);
    await expect(agent.cancel(9999)).rejects.toThrow(/no longer exists/);
  });

  it('tasks are listed newest first and carry the step summary', async () => {
    await agent.run((await agent.plan('open youtube')).id);
    await agent.run((await agent.plan('note hello')).id);
    const [newest, oldest] = await agent.tasks();
    expect(newest.id).toBeGreaterThan(oldest.id);
    expect(newest.summary).toBe('Save a note: "Hello"');
    expect(oldest.summary).toBe('Open youtube.com in your browser');
  });

  it('tells subscribers about every status change', async () => {
    const seen: string[] = [];
    const off = await agent.onTask((t) => seen.push(`${t.stepId}:${t.status.state}`));
    const plan = await agent.plan('open youtube then go to example.org');
    await agent.run(plan.id);
    await agent.run(plan.id, ['s2']);
    off();
    await agent.run((await agent.plan('open github')).id);
    expect(seen).toEqual(['s1:running', 's1:done', 's2:awaiting-confirmation', 's2:running', 's2:done']);
  });
});

describe('effects that are local to the app (demo)', () => {
  it('a timer step starts a real timer in the app', async () => {
    const plan = await agent.plan('set a timer for 10 minutes');
    await agent.run(plan.id);
    expect(mocks.timerAdd).toHaveBeenCalledWith('10 min', 10, true);
    expect(mocks.pushToast).toHaveBeenCalledWith('Timer started: 10 min', 2200);
  });

  it('a note step saves a note; a failure is reported on the task', async () => {
    await agent.run((await agent.plan('note buy milk')).id);
    expect(mocks.addNote).toHaveBeenCalledWith('Buy milk');

    mocks.addNote.mockImplementation(async () => false);
    const [task] = await agent.run((await agent.plan('note second')).id);
    expect(task.status).toEqual({ state: 'failed', message: 'Could not save the note.' });
  });

  it('a reminder is added only after it is confirmed', async () => {
    const before = (await bridge.loadCalendar()).length;
    const plan = await agent.plan('remind me in 20 minutes to stretch');
    await agent.run(plan.id);
    expect((await bridge.loadCalendar()).length).toBe(before);
    expect(mocks.calendarTick.update).not.toHaveBeenCalled();

    await agent.run(plan.id, ['s1']);
    const list = await bridge.loadCalendar();
    expect(list).toHaveLength(before + 1);
    expect(list.find((e) => e.title === 'Stretch')).toMatchObject({ date: '2026-11-02', time: '10:21' });
    expect(mocks.calendarTick.update).toHaveBeenCalledTimes(1);
  });

  it('opening a site, app or mail draft changes nothing in a browser preview', async () => {
    await agent.run((await agent.plan('open youtube then launch notepad')).id);
    expect(mocks.timerAdd).not.toHaveBeenCalled();
    expect(mocks.addNote).not.toHaveBeenCalled();
    expect((await agent.tasks()).every((t) => t.status.state === 'done')).toBe(true);
  });
});

describe('integrations and the MCP registry (demo)', () => {
  it('lists what is real, what is not available yet, and says so plainly', async () => {
    const all = await agent.integrations();
    const byId = (id: string) => all.find((i) => i.id === id)!;
    expect(byId('google-calendar').status).toBe('coming-soon');
    expect(byId('google-calendar').summary).toContain('Not available yet');
    expect(byId('mcp-servers').summary).toContain('Registry only');
    expect(byId('gmail-compose').status).toBe('builtin');
    for (const id of ['gmail-compose', 'outlook-compose', 'mailto']) {
      expect(byId(id).summary).toContain('never sends or reads email');
    }
    for (const i of all) expect(i.data && i.network && i.summary && i.name).toBeTruthy();
    expect(new Set(all.map((i) => i.id)).size).toBe(all.length);
  });

  it('starts empty and saves a server disabled', async () => {
    expect(await agent.mcpList()).toEqual([]);
    const list = await agent.mcpSave({ name: ' My  Files ', transport: { type: 'stdio', command: 'npx', args: ['-y', 'srv'] } });
    expect(list).toEqual([
      { id: 'mcp-my-files', name: 'My Files', transport: { type: 'stdio', command: 'npx', args: ['-y', 'srv'] }, enabled: false },
    ]);
    expect(await agent.mcpList()).toEqual(list);
  });

  it('updates by id, rejects bad input and duplicate names, and removes', async () => {
    const [first] = await agent.mcpSave({ name: 'Docs', transport: { type: 'http', url: 'https://mcp.example.com' } });
    const renamed = await agent.mcpSave({ ...first, name: 'Docs 2' });
    expect(renamed).toHaveLength(1);
    expect(renamed[0].name).toBe('Docs 2');
    expect(renamed[0].id).toBe(first.id);

    await expect(agent.mcpSave({ name: 'docs 2', transport: { type: 'http', url: 'https://x.example.com' } })).rejects.toThrow(/already registered/);
    await expect(agent.mcpSave({ name: '', transport: { type: 'http', url: 'https://x.example.com' } })).rejects.toThrow(/name/);
    await expect(agent.mcpSave({ name: 'a', transport: { type: 'stdio', command: 'npx -y srv', args: [] } })).rejects.toThrow(/Arguments/);
    await expect(agent.mcpSave({ name: 'b', transport: { type: 'stdio', command: 'a && b', args: [] } })).rejects.toThrow(/symbols/);
    await expect(agent.mcpSave({ name: 'c', transport: { type: 'http', url: 'http://example.com' } })).rejects.toThrow(/https/);
    await expect(agent.mcpSave({ name: 'd', transport: { type: 'http', url: 'javascript:alert(1)' } })).rejects.toThrow();
    expect(await agent.mcpList()).toHaveLength(1);
    expect((await agent.mcpSave({ name: 'local', transport: { type: 'http', url: 'http://localhost:8080/mcp' } }))).toHaveLength(2);

    expect(await agent.mcpRemove(first.id)).toHaveLength(1);
    expect(await agent.mcpRemove('nope')).toHaveLength(1);
  });

  it('a registered server shows up as not connected, never as running', async () => {
    await agent.mcpSave({ name: 'Files', transport: { type: 'stdio', command: 'npx', args: [] } });
    const all = await agent.integrations();
    const entry = all.find((i) => i.id === 'mcp:mcp-files')!;
    expect(entry.status).toBe('not-connected');
    expect(entry.summary).toContain('does not start or call MCP servers yet');
    expect(entry.network).toContain('never contacts');
  });

  it('caps the registry', async () => {
    for (let i = 0; i < 20; i++) await agent.mcpSave({ name: `server ${i}`, transport: { type: 'stdio', command: 'x', args: [] } });
    await expect(agent.mcpSave({ name: 'one too many', transport: { type: 'stdio', command: 'x', args: [] } })).rejects.toThrow(/At most 20/);
  });

  it('what comes back is a copy', async () => {
    const list = await agent.mcpSave({ name: 'Files', transport: { type: 'stdio', command: 'x', args: ['a'] } });
    list[0].name = 'tampered';
    (list[0].transport as { args: string[] }).args.push('b');
    const again = await agent.mcpList();
    expect(again[0].name).toBe('Files');
    expect(again[0].transport).toEqual({ type: 'stdio', command: 'x', args: ['a'] });
  });
});

describe('pure helpers', () => {
  const step = (id: string, level: 'safe' | 'confirm' | 'blocked'): AgentStep => ({
    id,
    action: { type: 'note', body: id },
    summary: id,
    risk: level === 'blocked' ? { level, reason: 'no' } : { level },
  });
  const task = (id: number, state: AgentTask['status']['state'] = 'done'): AgentTask => ({
    id,
    planId: 'p',
    stepId: `s${id}`,
    summary: `t${id}`,
    status: state === 'failed' ? { state, message: 'x' } : { state },
    createdAt: id,
    updatedAt: id,
  });

  it('splitSteps and hasRunnable', () => {
    const plan: AgentPlan = { id: 'p', steps: [step('a', 'safe'), step('b', 'confirm'), step('c', 'blocked'), step('d', 'safe')], suggestions: [] };
    const s = splitSteps(plan);
    expect(s.safe.map((x) => x.id)).toEqual(['a', 'd']);
    expect(s.confirm.map((x) => x.id)).toEqual(['b']);
    expect(s.blocked.map((x) => x.id)).toEqual(['c']);
    expect(hasRunnable(plan)).toBe(true);
    expect(hasRunnable({ ...plan, steps: [step('c', 'blocked')] })).toBe(false);
  });

  it('statusLabel and isFinal', () => {
    expect(statusLabel({ state: 'pending' })).toBe('Queued');
    expect(statusLabel({ state: 'awaiting-confirmation' })).toBe('Waiting for your OK');
    expect(statusLabel({ state: 'running' })).toBe('Running');
    expect(statusLabel({ state: 'done' })).toBe('Done');
    expect(statusLabel({ state: 'cancelled' })).toBe('Cancelled');
    expect(statusLabel({ state: 'failed', message: 'Windows could not open it.' })).toBe('Failed: Windows could not open it.');
    expect(statusLabel({ state: 'failed', message: '' })).toBe('Failed');
    expect(isFinal({ state: 'done' }) && isFinal({ state: 'cancelled' }) && isFinal({ state: 'failed', message: '' })).toBe(true);
    expect(isFinal({ state: 'running' }) || isFinal({ state: 'pending' }) || isFinal({ state: 'awaiting-confirmation' })).toBe(false);
  });

  it('mergeTask replaces by id, sorts newest first and caps the history', () => {
    let list: AgentTask[] = [];
    list = mergeTask(list, task(1, 'running'));
    list = mergeTask(list, task(2));
    list = mergeTask(list, { ...task(1), status: { state: 'done' } });
    expect(list.map((t) => [t.id, t.status.state])).toEqual([[2, 'done'], [1, 'done']]);

    let big: AgentTask[] = [];
    big = mergeTask(big, task(1, 'awaiting-confirmation'));
    for (let i = 2; i <= MAX_TASKS + 20; i++) big = mergeTask(big, task(i));
    expect(big).toHaveLength(MAX_TASKS);
    // the waiting one is never the one dropped, finished ones go oldest first
    expect(big.some((t) => t.id === 1)).toBe(true);
    expect(big[0].id).toBe(MAX_TASKS + 20);
    expect(big.some((t) => t.id === 2)).toBe(false);
  });

  it('durations read naturally', () => {
    expect(formatSeconds(45)).toBe('45 s');
    expect(formatSeconds(600)).toBe('10 min');
    expect(formatSeconds(5400)).toBe('1 h 30 min');
    expect(formatSeconds(3661)).toBe('1 h 1 min 1 s');
    expect(longDuration(600)).toBe('10 minutes');
    expect(longDuration(60)).toBe('1 minute');
    expect(longDuration(90)).toBe('1 minute 30 seconds');
    expect(longDuration(3600)).toBe('1 hour');
    expect(longDuration(86400)).toBe('24 hours');
    expect(longDuration(0)).toBe('0 seconds');
  });

  it('timerFromEvent accepts sane requests only', () => {
    expect(timerFromEvent({ seconds: 600, label: ' tea ' })).toEqual({ label: 'tea', minutes: 10 });
    expect(timerFromEvent({ seconds: 90, label: '' })).toEqual({ label: '1 min 30 s', minutes: 1.5 });
    expect(timerFromEvent({ seconds: 5 })).toEqual({ label: '5 s', minutes: 5 / 60 });
    for (const bad of [null, undefined, 'x', 5, {}, { seconds: 0 }, { seconds: -3 }, { seconds: 86401 }, { seconds: NaN }, { seconds: Infinity }, { seconds: '60' }]) {
      expect(timerFromEvent(bad), JSON.stringify(bad)).toBeNull();
    }
    expect(timerFromEvent({ seconds: 86400 })).not.toBeNull();
  });

  it('parseArgs: one per line, blanks dropped, text untouched', () => {
    expect(parseArgs('-y\n@scope/server\n\n  \nC:\\Users\\me\r\n--flag=a b')).toEqual(['-y', '@scope/server', 'C:\\Users\\me', '--flag=a b']);
    expect(parseArgs('')).toEqual([]);
    expect(parseArgs('  leading space kept')).toEqual(['  leading space kept']);
  });

  it('draftToInput and describeTransport', () => {
    expect(draftToInput({ name: ' Files ', kind: 'stdio', command: ' npx ', args: '-y\nsrv', url: 'ignored' })).toEqual({
      name: 'Files',
      transport: { type: 'stdio', command: 'npx', args: ['-y', 'srv'] },
    });
    expect(draftToInput({ name: 'Docs', kind: 'http', command: 'ignored', args: 'ignored', url: ' https://mcp.example.com ' })).toEqual({
      name: 'Docs',
      transport: { type: 'http', url: 'https://mcp.example.com' },
    });
    expect(describeTransport({ type: 'stdio', command: 'npx', args: ['-y', 'srv'] })).toBe('npx -y srv');
    expect(describeTransport({ type: 'http', url: 'https://x.example.com' })).toBe('https://x.example.com');
  });

  it('every action has an icon', () => {
    const actions: AgentStep['action'][] = [
      { type: 'open-url', url: 'u', site: 's', known: true },
      { type: 'search', site: 's', query: 'q', url: 'u' },
      { type: 'open-app', name: 'n', target: 't' },
      { type: 'timer', seconds: 1, label: '' },
      { type: 'reminder', title: 't', date: 'd', time: 't' },
      { type: 'note', body: 'b' },
      { type: 'compose-email', provider: 'gmail', to: [], subject: '', body: '' },
      { type: 'unsupported', text: '' },
    ];
    expect(new Set(actions.map(stepIcon)).size).toBe(actions.length);
  });

  it('the starter examples all plan to something runnable', async () => {
    for (const text of EXAMPLES) {
      const plan = await agent.plan(text);
      expect(hasRunnable(plan), text).toBe(true);
    }
  });

  it('startAgent resolves to an unsubscribe in the demo and is idempotent', async () => {
    const a = startAgent();
    const b = startAgent();
    expect(a).toBe(b);
    const off = await a;
    expect(typeof off).toBe('function');
    off();
    expect(startAgent()).not.toBe(a);
  });
});
