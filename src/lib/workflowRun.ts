// Running a workflow: each saved line goes through Agent Mode, one line at a time.
//
// There is nothing special about a workflow line. It is planned by the ordinary Agent planner and
// then follows the ordinary permission rules:
//   * Safe steps run straight away.
//   * A step that needs the user's OK stops the workflow at that line and shows its confirmation
//     card; the workflow carries on only after the user confirms every card of the line.
//   * A line with a Blocked step, a step that fails, a declined card, or the Stop button ends the
//     workflow. Nothing after that point runs, and nothing before it is undone.
// No step can run without passing through `api.run`, which is `agent_run`: a workflow cannot do
// anything the Agent panel cannot, and nothing here bypasses a permission.
//
// The runner is plain TypeScript over a tiny `RunApi`, so it is unit-tested with a fake planner.

import { agent, hasRunnable, type AgentPlan, type AgentStep, type AgentTask } from './agent';

export interface RunApi {
  plan(text: string): Promise<AgentPlan>;
  run(planId: string, confirmed?: string[]): Promise<AgentTask[]>;
  cancel(taskId: number): Promise<AgentTask>;
}

export type LineState =
  | { kind: 'pending' }
  | { kind: 'planning' }
  /** Some steps need the user's OK; `waiting` holds their ids. */
  | { kind: 'waiting'; plan: AgentPlan; waiting: string[] }
  | { kind: 'done'; plan: AgentPlan }
  /** The workflow ended here. */
  | { kind: 'stopped'; plan: AgentPlan | null; reason: string };

export type RunStatus = 'idle' | 'running' | 'waiting' | 'done' | 'stopped';

export interface RunState {
  status: RunStatus;
  /** The line being worked on (or the one the run ended at). */
  index: number;
  lines: LineState[];
  /** One plain sentence about how the run ended, empty while it is going. */
  message: string;
}

/** The steps of a plan that still wait for the user's OK. */
export function confirmSteps(plan: AgentPlan): AgentStep[] {
  return plan.steps.filter((s) => s.risk.level === 'confirm');
}

/** Why a freshly planned line cannot run at all, or `null` if it can start. */
export function blockReason(plan: AgentPlan): string | null {
  if (plan.steps.length === 0) return 'Agent Mode found nothing to do on this line.';
  const blocked = plan.steps.find((s) => s.risk.level === 'blocked');
  if (blocked && blocked.risk.level === 'blocked') return blocked.risk.reason || 'Agent Mode cannot do this.';
  if (!hasRunnable(plan)) return 'Agent Mode cannot do this.';
  return null;
}

const failure = (tasks: readonly AgentTask[]): string | null => {
  for (const t of tasks) if (t.status.state === 'failed') return t.status.message || 'A step failed.';
  return null;
};

export class WorkflowRun {
  private state: RunState;
  /** step id -> task id, for the cards that wait on the current line. */
  private waitingTasks = new Map<string, number>();
  private stopRequested = false;
  private stopReason = 'You stopped the workflow.';
  private started = false;

  constructor(
    readonly lines: readonly string[],
    private readonly api: RunApi = agent,
    private readonly onChange: (s: RunState) => void = () => {},
  ) {
    this.state = { status: 'idle', index: 0, lines: lines.map(() => ({ kind: 'pending' }) as LineState), message: '' };
  }

  get snapshot(): RunState {
    return this.state;
  }

  private set(patch: Partial<RunState>, line?: LineState): void {
    const lines = [...this.state.lines];
    if (line) lines[patch.index ?? this.state.index] = line;
    this.state = { ...this.state, ...patch, lines };
    this.onChange(this.state);
  }

  private stopAt(reason: string, plan: AgentPlan | null): void {
    this.waitingTasks.clear();
    this.set({ status: 'stopped', message: reason }, { kind: 'stopped', plan, reason });
  }

  /** Run from the first line until a card needs the user, or the workflow ends. */
  async start(): Promise<void> {
    if (this.started) return;
    this.started = true;
    if (this.lines.length === 0) {
      this.set({ status: 'done', message: 'This workflow has no lines.' });
      return;
    }
    this.set({ status: 'running', index: 0 });
    await this.advance();
  }

  private async advance(): Promise<void> {
    while (this.state.index < this.lines.length) {
      if (this.stopRequested) {
        this.stopAt(this.stopReason, null);
        return;
      }
      const index = this.state.index;
      this.set({ status: 'running', index }, { kind: 'planning' });
      let plan: AgentPlan;
      try {
        plan = await this.api.plan(this.lines[index]);
      } catch (e) {
        this.stopAt(e instanceof Error ? e.message : String(e), null);
        return;
      }
      if (this.stopRequested) {
        this.stopAt(this.stopReason, plan);
        return;
      }
      const blocked = blockReason(plan);
      if (blocked) {
        this.stopAt(blocked, plan);
        return;
      }
      let tasks: AgentTask[];
      try {
        // Safe steps run now; steps that need the user's OK become cards and wait.
        tasks = await this.api.run(plan.id, []);
      } catch (e) {
        this.stopAt(e instanceof Error ? e.message : String(e), plan);
        return;
      }
      const failed = failure(tasks);
      if (failed) {
        this.stopAt(failed, plan);
        return;
      }
      const cards = confirmSteps(plan);
      if (cards.length > 0) {
        this.waitingTasks.clear();
        for (const c of cards) {
          const t = tasks.find((x) => x.planId === plan.id && x.stepId === c.id);
          if (t) this.waitingTasks.set(c.id, t.id);
        }
        this.set({ status: 'waiting' }, { kind: 'waiting', plan, waiting: cards.map((c) => c.id) });
        return;
      }
      this.set({ index }, { kind: 'done', plan });
      this.set({ index: index + 1 });
    }
    const last = Math.max(0, this.lines.length - 1);
    this.set({ status: 'done', index: last, message: 'The workflow finished.' });
  }

  /** The user pressed the card's button: run that one step, then carry on when the line is done. */
  async confirm(stepId: string): Promise<void> {
    const line = this.state.lines[this.state.index];
    if (this.state.status !== 'waiting' || line?.kind !== 'waiting' || !line.waiting.includes(stepId)) return;
    let tasks: AgentTask[];
    try {
      tasks = await this.api.run(line.plan.id, [stepId]);
    } catch (e) {
      this.stopAt(e instanceof Error ? e.message : String(e), line.plan);
      return;
    }
    const failed = failure(tasks);
    if (failed) {
      this.stopAt(failed, line.plan);
      return;
    }
    this.waitingTasks.delete(stepId);
    const waiting = line.waiting.filter((id) => id !== stepId);
    if (waiting.length > 0) {
      this.set({}, { kind: 'waiting', plan: line.plan, waiting });
      return;
    }
    const index = this.state.index;
    this.set({ status: 'running', index }, { kind: 'done', plan: line.plan });
    this.set({ index: index + 1 });
    await this.advance();
  }

  /** The user pressed a card's Cancel, or Stop: cancel what waits and end the workflow. */
  async stop(reason = 'You stopped the workflow.'): Promise<void> {
    if (this.state.status === 'done' || this.state.status === 'stopped') return;
    this.stopRequested = true;
    this.stopReason = reason;
    const line = this.state.lines[this.state.index];
    if (this.state.status === 'waiting' && line?.kind === 'waiting') {
      for (const taskId of this.waitingTasks.values()) {
        try {
          await this.api.cancel(taskId);
        } catch {
          /* already started or gone: nothing more to cancel */
        }
      }
      this.stopAt(reason, line.plan);
    } else if (this.state.status === 'running') {
      // a line is being planned or run; the loop stops before the next one
      this.set({ message: reason });
    }
  }
}
