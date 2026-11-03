// Agent Mode inside the Tauri shell: bridge is mocked, so this proves the command names and
// argument shapes, and the agent-timer -> timer wiring, without a backend.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
const handlers = new Map<string, (payload: unknown) => void>();
let subscriptions = 0;
let failing: string | null = null;

const mocks = vi.hoisted(() => ({
  timerAdd: vi.fn(),
  addNote: vi.fn(async (_body: string) => true),
  pushToast: vi.fn(),
  calendarTick: { update: vi.fn() },
}));
vi.mock('./stores', () => mocks);

vi.mock('./bridge', () => ({
  isTauri: () => true,
  listen: async (event: string, cb: (payload: unknown) => void) => {
    handlers.set(event, cb);
    subscriptions++;
    return () => {
      handlers.delete(event);
      subscriptions--;
    };
  },
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    calls.push({ cmd, args });
    if (failing === cmd) throw 'boom';
    switch (cmd) {
      case 'agent_plan':
        return { id: 'p1', steps: [], suggestions: [] };
      case 'agent_run':
      case 'agent_tasks':
      case 'agent_integrations':
      case 'agent_mcp_list':
      case 'agent_mcp_save':
      case 'agent_mcp_remove':
        return [];
      case 'agent_cancel':
        return { id: 7, status: { state: 'cancelled' } };
      default:
        return undefined;
    }
  },
}));

const emit = (event: string, payload: unknown) => handlers.get(event)?.(payload);

async function fresh() {
  vi.resetModules();
  return await import('./agent');
}

beforeEach(() => {
  calls.length = 0;
  handlers.clear();
  subscriptions = 0;
  failing = null;
  mocks.timerAdd.mockClear();
  mocks.pushToast.mockClear();
});
afterEach(() => {
  vi.restoreAllMocks();
});

describe('tauri: commands', () => {
  it('plan, run, tasks and cancel use the documented commands and argument names', async () => {
    const { agent } = await fresh();
    await agent.plan('open youtube');
    await agent.run('p1', ['s2']);
    await agent.run('p1');
    await agent.tasks();
    await agent.cancel(7);
    expect(calls).toEqual([
      { cmd: 'agent_plan', args: { text: 'open youtube' } },
      { cmd: 'agent_run', args: { planId: 'p1', confirmed: ['s2'] } },
      { cmd: 'agent_run', args: { planId: 'p1', confirmed: [] } },
      { cmd: 'agent_tasks', args: undefined },
      { cmd: 'agent_cancel', args: { taskId: 7 } },
    ]);
  });

  it('the registry commands pass the server and the id', async () => {
    const { agent } = await fresh();
    const server = { name: 'Files', transport: { type: 'stdio' as const, command: 'npx', args: ['-y'] } };
    await agent.integrations();
    await agent.mcpList();
    await agent.mcpSave(server);
    await agent.mcpRemove('mcp-files');
    expect(calls).toEqual([
      { cmd: 'agent_integrations', args: undefined },
      { cmd: 'agent_mcp_list', args: undefined },
      { cmd: 'agent_mcp_save', args: { server } },
      { cmd: 'agent_mcp_remove', args: { id: 'mcp-files' } },
    ]);
  });

  it('warm is fire-and-forget and never throws', async () => {
    const { agent } = await fresh();
    failing = 'agent_warm';
    expect(() => agent.warm()).not.toThrow();
    await Promise.resolve();
    expect(calls.map((c) => c.cmd)).toEqual(['agent_warm']);
  });

  it('a backend error rejects with the backend text', async () => {
    const { agent } = await fresh();
    failing = 'agent_run';
    await expect(agent.run('p1')).rejects.toBe('boom');
  });

  it('never touches the demo engine: the plan comes from Rust, not from a local guess', async () => {
    const { agent } = await fresh();
    const plan = await agent.plan('open youtube');
    expect(plan).toEqual({ id: 'p1', steps: [], suggestions: [] });
  });
});

describe('tauri: events', () => {
  it('onTask listens to agent-task', async () => {
    const { agent } = await fresh();
    const seen: unknown[] = [];
    const off = await agent.onTask((t) => seen.push(t));
    emit('agent-task', { id: 1, status: { state: 'running' } });
    expect(seen).toEqual([{ id: 1, status: { state: 'running' } }]);
    off();
    expect(handlers.has('agent-task')).toBe(false);
  });

  it('agent-timer starts a timer in the app', async () => {
    const { startAgent } = await fresh();
    await startAgent();
    emit('agent-timer', { seconds: 600, label: 'tea' });
    expect(mocks.timerAdd).toHaveBeenCalledWith('tea', 10, true);
    expect(mocks.pushToast).toHaveBeenCalledWith('Timer started: tea', 2200);

    emit('agent-timer', { seconds: 90, label: '' });
    expect(mocks.timerAdd).toHaveBeenLastCalledWith('1 min 30 s', 1.5, true);
  });

  it('ignores a malformed or out-of-range timer request', async () => {
    const { startAgent } = await fresh();
    await startAgent();
    for (const bad of [null, {}, { seconds: 0 }, { seconds: 90_000 }, { seconds: 'ten' }, 'x']) emit('agent-timer', bad);
    expect(mocks.timerAdd).not.toHaveBeenCalled();
    expect(mocks.pushToast).not.toHaveBeenCalled();
  });

  it('startAgent subscribes once however many times it is called', async () => {
    const { startAgent } = await fresh();
    const [a, b, c] = [startAgent(), startAgent(), startAgent()];
    await Promise.all([a, b, c]);
    expect(subscriptions).toBe(1);
    emit('agent-timer', { seconds: 60 });
    expect(mocks.timerAdd).toHaveBeenCalledTimes(1);
  });

  it('unsubscribing lets it be started again', async () => {
    const { startAgent } = await fresh();
    const off = await startAgent();
    off();
    expect(subscriptions).toBe(0);
    await startAgent();
    expect(subscriptions).toBe(1);
  });
});
