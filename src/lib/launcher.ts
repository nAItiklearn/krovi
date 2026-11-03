// Quick Launcher: installed apps in the command palette.
//
// The app names are the Start-menu list Agent Mode already reads (`extras_apps` returns the same
// cached list; there is no second scan). Typing ranks them; Enter opens the app by running an
// ordinary Agent Mode plan, "open <app>", which is a Safe step. So an app is launched by exactly the
// same code, and under exactly the same checks, as typing that sentence in the Agent panel.
//
// Pure ranking and the launch check live here and are unit-tested; the palette only renders them.

import { match } from '../components/search';
import { agent, type AgentPlan, type AgentTask } from './agent';

export interface AppHit {
  name: string;
  score: number;
  /** [start, end) ranges inside `name` to highlight */
  ranges: [number, number][];
}

const words = (s: string): { initial: string; at: number }[] => {
  const out: { initial: string; at: number }[] = [];
  for (let i = 0; i < s.length; i++) {
    if (/[A-Za-z0-9]/.test(s[i]) && (i === 0 || /[\s\-_/.()]/.test(s[i - 1]))) out.push({ initial: s[i].toLowerCase(), at: i });
  }
  return out;
};

/** "vsc" -> "Visual Studio Code": the query spells the first letters of the words. */
function acronym(query: string, name: string): [number, number][] | null {
  const q = query.toLowerCase().replace(/\s+/g, '');
  if (q.length < 2) return null;
  const w = words(name);
  if (w.length < q.length) return null;
  for (let i = 0; i < q.length; i++) if (w[i].initial !== q[i]) return null;
  return w.slice(0, q.length).map((x) => [x.at, x.at + 1] as [number, number]);
}

/**
 * The best-matching apps for what was typed, best first. An empty query ranks nothing (the palette
 * does not list every app). Order: exact name, name starts with it, a word starts with it, the first
 * letters of the words ("vsc"), a part of the name, then letters in order; ties go to the shorter
 * name, then alphabetical. Repeated names (the same app in two Start-menu folders) count once.
 */
export function rankApps(query: string, names: readonly string[], limit = 5): AppHit[] {
  const q = query.trim();
  if (!q || limit <= 0) return [];
  const seen = new Set<string>();
  const hits: AppHit[] = [];
  for (const name of names) {
    const key = name.trim().toLowerCase();
    if (!key || seen.has(key)) continue;
    seen.add(key);
    if (key === q.toLowerCase()) {
      hits.push({ name, score: 200, ranges: [[0, name.length]] });
      continue;
    }
    const m = match(q, name);
    const a = acronym(q, name);
    if (a && (!m || m.score < 70)) {
      hits.push({ name, score: 70, ranges: a });
    } else if (m) {
      hits.push({ name, score: m.score, ranges: m.ranges });
    }
  }
  hits.sort((a, b) => b.score - a.score || a.name.length - b.name.length || a.name.localeCompare(b.name));
  return hits.slice(0, limit);
}

/** The Agent Mode sentence that opens an app. */
export function openAppCommand(name: string): string {
  return `open ${name}`;
}

/**
 * Is this plan exactly "open that app" as a Safe step? If not, why not (a sentence to show). The
 * planner resolves a name against the Start menu by itself, so this is the guard against it
 * choosing a different app than the one picked ("Notepad" vs "Notepad++") or splitting the name.
 */
export function checkLaunchPlan(plan: AgentPlan, name: string): string | null {
  const step = plan.steps[0];
  if (!step) return `Krovi did not understand "${name}".`;
  if (plan.steps.length === 1 && step.risk.level === 'blocked') return step.risk.reason;
  const ok =
    plan.steps.length === 1 &&
    step.action.type === 'open-app' &&
    step.risk.level === 'safe' &&
    step.action.name.trim().toLowerCase() === name.trim().toLowerCase();
  return ok ? null : `Krovi could not open exactly "${name}" from here. Try it in the Agent tool.`;
}

export interface LaunchApi {
  plan(text: string): Promise<AgentPlan>;
  run(planId: string, confirmed?: string[]): Promise<AgentTask[]>;
}

/** Open an installed app through Agent Mode. Resolves to a short confirmation, or throws a sentence. */
export async function launchApp(name: string, api: LaunchApi = agent): Promise<string> {
  const plan = await api.plan(openAppCommand(name));
  const problem = checkLaunchPlan(plan, name);
  if (problem) throw new Error(problem);
  const tasks = await api.run(plan.id, []);
  for (const t of tasks) {
    if (t.status.state === 'failed') throw new Error(t.status.message || 'Windows could not open it.');
  }
  return `Opening ${name}`;
}
