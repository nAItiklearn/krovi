// Quick Launcher: ranking of installed apps, and the guard that makes sure "open <app>" opens
// exactly the app that was picked, as a Safe Agent Mode step.
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('./stores', () => ({
  timerAdd: vi.fn(),
  addNote: vi.fn(async () => true),
  pushToast: vi.fn(),
  calendarTick: { update: vi.fn() },
}));

import { __resetDemo, type AgentPlan, type AgentStep, type AgentTask } from './agent';
import { checkLaunchPlan, launchApp, openAppCommand, rankApps, type LaunchApi } from './launcher';

const APPS = ['Calculator', 'Notepad', 'Notepad++', 'Visual Studio Code', 'Windows Terminal', 'Terminal Services', 'Paint', 'Paint Shop', 'PowerShell'];
const top = (q: string, names: readonly string[] = APPS, limit = 5) => rankApps(q, names, limit).map((h) => h.name);

describe('rankApps', () => {
  it('ranks nothing for an empty query (the palette does not list every app)', () => {
    expect(rankApps('', APPS)).toEqual([]);
    expect(rankApps('   ', APPS)).toEqual([]);
    expect(rankApps('note', APPS, 0)).toEqual([]);
  });

  it('puts an exact name first, then names that start with the text', () => {
    expect(top('notepad')).toEqual(['Notepad', 'Notepad++']);
    expect(top('NOTEPAD')).toEqual(['Notepad', 'Notepad++']);
  });

  it('ranks a name that starts with the text above a name with a word that starts with it', () => {
    expect(top('term')).toEqual(['Terminal Services', 'Windows Terminal']);
  });

  it('finds an app by a word inside its name', () => {
    expect(top('code')).toEqual(['Visual Studio Code']);
  });

  it('finds an app by the first letters of its words', () => {
    const hits = rankApps('vsc', APPS);
    expect(hits.map((h) => h.name)).toEqual(['Visual Studio Code']);
    // the highlighted letters are the initials: V, S, C
    expect(hits[0].ranges).toEqual([[0, 1], [7, 8], [14, 15]]);
    // letters in order inside one word are still a (weaker) match
    expect(top('ps')).toEqual(['Paint Shop', 'PowerShell']);
  });

  it('breaks ties with the shorter name, then alphabetically', () => {
    expect(top('pai')).toEqual(['Paint', 'Paint Shop']);
    expect(top('xx', ['Zeta app', 'Alpha app'])).toEqual([]);
    expect(top('app', ['Zeta app', 'Beta app'])).toEqual(['Beta app', 'Zeta app']);
  });

  it('returns the ranges to highlight inside the name', () => {
    expect(rankApps('note', ['Notepad'])[0].ranges).toEqual([[0, 4]]);
    expect(rankApps('pad', ['Notepad'])[0].ranges).toEqual([[4, 7]]);
  });

  it('counts an app listed twice (two Start-menu folders) once', () => {
    expect(top('notepad', ['Notepad', 'notepad ', 'NOTEPAD', ''])).toEqual(['Notepad']);
  });

  it('honours the limit and matches nothing it should not', () => {
    expect(top('p', APPS, 2)).toHaveLength(2);
    expect(top('zzz')).toEqual([]);
  });
});

const step = (over: Partial<AgentStep> = {}): AgentStep => ({
  id: 's1',
  action: { type: 'open-app', name: 'Notepad', target: 'shell:AppsFolder\\notepad' },
  summary: 'Open Notepad',
  risk: { level: 'safe' },
  ...over,
});
const plan = (steps: AgentStep[]): AgentPlan => ({ id: 'p1', steps, suggestions: [] });
const task = (state: 'done' | 'failed', message = ''): AgentTask => ({
  id: 1,
  planId: 'p1',
  stepId: 's1',
  summary: 'Open Notepad',
  status: state === 'failed' ? { state, message } : { state },
  createdAt: 1,
  updatedAt: 1,
});

describe('checkLaunchPlan', () => {
  it('accepts exactly one Safe "open <that app>" step, ignoring case', () => {
    expect(checkLaunchPlan(plan([step()]), 'Notepad')).toBeNull();
    expect(checkLaunchPlan(plan([step()]), 'notepad')).toBeNull();
  });

  it('refuses a plan that picked a different app than the one chosen', () => {
    const other = step({ action: { type: 'open-app', name: 'Notepad++', target: 'x' }, summary: 'Open Notepad++' });
    expect(checkLaunchPlan(plan([other]), 'Notepad')).toContain('could not open exactly "Notepad"');
  });

  it('refuses anything that is not a single open-app Safe step', () => {
    expect(checkLaunchPlan(plan([]), 'Notepad')).toContain('did not understand');
    expect(checkLaunchPlan(plan([step(), step({ id: 's2' })]), 'Notepad')).toContain('could not open exactly');
    expect(checkLaunchPlan(plan([step({ risk: { level: 'confirm' } })]), 'Notepad')).toContain('could not open exactly');
    const url = step({ action: { type: 'open-url', url: 'https://x.test/', site: 'x.test', known: false } });
    expect(checkLaunchPlan(plan([url]), 'Notepad')).toContain('could not open exactly');
  });

  it('passes on the planner reason when the app is blocked', () => {
    const blocked = step({ risk: { level: 'blocked', reason: 'Several apps match "Note": A, B. Say the full name.' } });
    expect(checkLaunchPlan(plan([blocked]), 'Note')).toBe('Several apps match "Note": A, B. Say the full name.');
  });
});

describe('launchApp', () => {
  function fakeApi(p: AgentPlan, tasks: AgentTask[] = [task('done')]) {
    const calls: string[] = [];
    const api: LaunchApi = {
      plan: async (text) => {
        calls.push(`plan:${text}`);
        return p;
      },
      run: async (id, confirmed) => {
        calls.push(`run:${id}:${JSON.stringify(confirmed ?? [])}`);
        return tasks;
      },
    };
    return { api, calls };
  }

  it('asks Agent Mode to "open <app>", runs the plan with nothing pre-confirmed, and says so', async () => {
    const { api, calls } = fakeApi(plan([step()]));
    expect(openAppCommand('Notepad')).toBe('open Notepad');
    await expect(launchApp('Notepad', api)).resolves.toBe('Opening Notepad');
    expect(calls).toEqual(['plan:open Notepad', 'run:p1:[]']);
  });

  it('never runs a plan that is not exactly the chosen app', async () => {
    const other = step({ action: { type: 'open-app', name: 'Notepad++', target: 'x' } });
    const { api, calls } = fakeApi(plan([other]));
    await expect(launchApp('Notepad', api)).rejects.toThrow('could not open exactly');
    expect(calls).toEqual(['plan:open Notepad']);
  });

  it('reports a launch that failed', async () => {
    const { api } = fakeApi(plan([step()]), [task('failed', 'That app is no longer in your Start menu list.')]);
    await expect(launchApp('Notepad', api)).rejects.toThrow('no longer in your Start menu');
  });

  describe('through the real Agent Mode demo planner', () => {
    beforeEach(() => __resetDemo());

    it('opens a demo app as a Safe step', async () => {
      await expect(launchApp('Spotify')).resolves.toBe('Opening Spotify');
    });

    it('refuses an app the planner does not know', async () => {
      await expect(launchApp('Definitely Not Installed')).rejects.toThrow();
    });
  });
});
