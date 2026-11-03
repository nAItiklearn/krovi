// Workflow runner: every line goes through the Agent planner and the Safe / Confirm / Blocked rules.
// A fake planner records exactly which steps were allowed to run, so these tests prove nothing
// bypasses a permission and that the workflow stops where it should.
import { describe, expect, it, vi } from 'vitest';

vi.mock('./stores', () => ({
  timerAdd: vi.fn(),
  addNote: vi.fn(async () => true),
  pushToast: vi.fn(),
  calendarTick: { update: vi.fn() },
}));

import type { AgentPlan, AgentStep, AgentTask, Risk } from './agent';
import { WorkflowRun, blockReason, confirmSteps, type RunApi, type RunState } from './workflowRun';

const step = (id: string, risk: Risk, summary = id): AgentStep => ({
  id,
  action: { type: 'note', body: summary },
  summary,
  risk,
});
const safe = (id: string) => step(id, { level: 'safe' });
const confirm = (id: string) => step(id, { level: 'confirm' });
const blocked = (id: string, reason: string) => step(id, { level: 'blocked', reason });

/** A scripted Agent: each line maps to a plan; it records every call and every step that ran. */
function fakeAgent(plans: Record<string, AgentStep[]>, opts: { failStep?: string; rejectPlan?: string } = {}) {
  const log: string[] = [];
  const ran: string[] = [];
  const tasks: AgentTask[] = [];
  let nextTask = 1;
  const planOf = new Map<string, AgentPlan>();
  const api: RunApi = {
    async plan(text) {
      log.push(`plan:${text}`);
      if (opts.rejectPlan === text) throw new Error('The planner is unavailable.');
      const id = `p${planOf.size + 1}`;
      const p: AgentPlan = { id, steps: plans[text] ?? [], suggestions: [] };
      planOf.set(id, p);
      return p;
    },
    async run(planId, confirmed = []) {
      log.push(`run:${planId}:${confirmed.join(',')}`);
      const p = planOf.get(planId)!;
      const touched: AgentTask[] = [];
      for (const s of p.steps) {
        if (s.risk.level === 'blocked') continue;
        const allowed = s.risk.level === 'safe' || confirmed.includes(s.id);
        let t = tasks.find((x) => x.planId === planId && x.stepId === s.id);
        if (!t) {
          t = { id: nextTask++, planId, stepId: s.id, summary: s.summary, status: { state: 'awaiting-confirmation' }, createdAt: 1, updatedAt: 1 };
          tasks.push(t);
        } else if (t.status.state !== 'awaiting-confirmation') {
          continue;
        }
        if (allowed) {
          ran.push(`${planId}/${s.id}`);
          t.status = opts.failStep === s.id ? { state: 'failed', message: 'Windows could not open it.' } : { state: 'done' };
        }
        touched.push({ ...t });
      }
      return touched;
    },
    async cancel(taskId) {
      log.push(`cancel:${taskId}`);
      const t = tasks.find((x) => x.id === taskId)!;
      t.status = { state: 'cancelled' };
      return { ...t };
    },
  };
  return { api, log, ran, tasks };
}

function runner(lines: string[], agent: ReturnType<typeof fakeAgent>) {
  const states: RunState[] = [];
  const r = new WorkflowRun(lines, agent.api, (s) => states.push(s));
  return { r, states };
}

describe('plan checks', () => {
  it('names the steps that wait for an OK', () => {
    const p: AgentPlan = { id: 'p', steps: [safe('s1'), confirm('s2'), confirm('s3')], suggestions: [] };
    expect(confirmSteps(p).map((s) => s.id)).toEqual(['s2', 's3']);
  });

  it('refuses a line that planned to nothing or to a blocked step', () => {
    expect(blockReason({ id: 'p', steps: [], suggestions: [] })).toContain('nothing to do');
    expect(blockReason({ id: 'p', steps: [safe('s1'), blocked('s2', 'I could not find an app called "x" in your Start menu.')], suggestions: [] })).toBe(
      'I could not find an app called "x" in your Start menu.',
    );
    expect(blockReason({ id: 'p', steps: [safe('s1')], suggestions: [] })).toBeNull();
    expect(blockReason({ id: 'p', steps: [confirm('s1')], suggestions: [] })).toBeNull();
  });
});

describe('WorkflowRun', () => {
  it('runs Safe lines one after another and finishes', async () => {
    const a = fakeAgent({ 'open spotify': [safe('s1')], 'set a timer for 25 minutes': [safe('s1')] });
    const { r, states } = runner(['open spotify', 'set a timer for 25 minutes'], a);
    await r.start();
    expect(r.snapshot.status).toBe('done');
    expect(r.snapshot.lines.map((l) => l.kind)).toEqual(['done', 'done']);
    // each line is planned only after the one before it finished
    expect(a.log).toEqual(['plan:open spotify', 'run:p1:', 'plan:set a timer for 25 minutes', 'run:p2:']);
    expect(a.ran).toEqual(['p1/s1', 'p2/s1']);
    expect(states.at(-1)!.message).toBe('The workflow finished.');
  });

  it('stops at a line that needs an OK and does not plan the next line until it is given', async () => {
    const a = fakeAgent({ 'open spotify': [safe('s1')], 'open notion.so': [confirm('s1')], 'note done': [safe('s1')] });
    const { r } = runner(['open spotify', 'open notion.so', 'note done'], a);
    await r.start();
    expect(r.snapshot.status).toBe('waiting');
    expect(r.snapshot.index).toBe(1);
    const line = r.snapshot.lines[1];
    expect(line.kind === 'waiting' && line.waiting).toEqual(['s1']);
    expect(a.ran).toEqual(['p1/s1']); // the Confirm step has NOT run
    expect(a.log).not.toContain('plan:note done');
    expect(r.snapshot.lines[2].kind).toBe('pending');

    await r.confirm('s1');
    expect(a.ran).toEqual(['p1/s1', 'p2/s1', 'p3/s1']);
    expect(a.log.filter((l) => l.startsWith('run:p2'))).toEqual(['run:p2:', 'run:p2:s1']);
    expect(r.snapshot.status).toBe('done');
  });

  it('only runs the confirm step that was confirmed', async () => {
    const a = fakeAgent({ 'two cards': [confirm('s1'), confirm('s2')] });
    const { r } = runner(['two cards'], a);
    await r.start();
    expect(r.snapshot.status).toBe('waiting');
    await r.confirm('s2');
    expect(a.ran).toEqual(['p1/s2']);
    expect(r.snapshot.status).toBe('waiting');
    const line = r.snapshot.lines[0];
    expect(line.kind === 'waiting' && line.waiting).toEqual(['s1']);
    await r.confirm('s1');
    expect(r.snapshot.status).toBe('done');
  });

  it('ignores a confirmation for a step that is not waiting', async () => {
    const a = fakeAgent({ 'one card': [confirm('s1')] });
    const { r } = runner(['one card'], a);
    await r.start();
    await r.confirm('s9');
    expect(a.ran).toEqual([]);
    expect(r.snapshot.status).toBe('waiting');
  });

  it('stops at a blocked line and runs nothing from it or after it', async () => {
    const a = fakeAgent({
      'open spotify': [safe('s1')],
      'open nothing': [safe('s1'), blocked('s2', 'I could not find an app called "nothing" in your Start menu.')],
      'note after': [safe('s1')],
    });
    const { r } = runner(['open spotify', 'open nothing', 'note after'], a);
    await r.start();
    expect(r.snapshot.status).toBe('stopped');
    expect(r.snapshot.message).toBe('I could not find an app called "nothing" in your Start menu.');
    expect(r.snapshot.lines.map((l) => l.kind)).toEqual(['done', 'stopped', 'pending']);
    expect(a.ran).toEqual(['p1/s1']); // not even the safe half of the blocked line
    expect(a.log).not.toContain('plan:note after');
  });

  it('stops when a step fails', async () => {
    const a = fakeAgent({ 'open spotify': [safe('s1')], 'note after': [safe('s1')] }, { failStep: 's1' });
    const { r } = runner(['open spotify', 'note after'], a);
    await r.start();
    expect(r.snapshot.status).toBe('stopped');
    expect(r.snapshot.message).toBe('Windows could not open it.');
    expect(a.log).not.toContain('plan:note after');
  });

  it('stops when the planner itself fails', async () => {
    const a = fakeAgent({ 'open spotify': [safe('s1')] }, { rejectPlan: 'open spotify' });
    const { r } = runner(['open spotify'], a);
    await r.start();
    expect(r.snapshot).toMatchObject({ status: 'stopped', message: 'The planner is unavailable.' });
  });

  it('cancels the waiting cards and ends when the user declines', async () => {
    const a = fakeAgent({ 'open notion.so': [confirm('s1'), confirm('s2')], 'note after': [safe('s1')] });
    const { r } = runner(['open notion.so', 'note after'], a);
    await r.start();
    await r.stop('You declined a step, so the workflow stopped.');
    expect(r.snapshot).toMatchObject({ status: 'stopped', message: 'You declined a step, so the workflow stopped.' });
    expect(a.log.filter((l) => l.startsWith('cancel'))).toEqual(['cancel:1', 'cancel:2']);
    expect(a.tasks.every((t) => t.status.state === 'cancelled')).toBe(true);
    expect(a.ran).toEqual([]); // nothing the user did not approve ever ran
    expect(a.log).not.toContain('plan:note after');
  });

  it('runs a workflow only once', async () => {
    const a = fakeAgent({ 'note one': [safe('s1')] });
    const { r } = runner(['note one'], a);
    await r.start();
    await r.start();
    expect(a.ran).toEqual(['p1/s1']);
  });

  it('has nothing to do for an empty workflow', async () => {
    const a = fakeAgent({});
    const { r } = runner([], a);
    await r.start();
    expect(r.snapshot).toMatchObject({ status: 'done', message: 'This workflow has no lines.' });
    expect(a.log).toEqual([]);
  });

  it('stopping a finished workflow changes nothing', async () => {
    const a = fakeAgent({ 'note one': [safe('s1')] });
    const { r } = runner(['note one'], a);
    await r.start();
    await r.stop();
    expect(r.snapshot.status).toBe('done');
  });
});
