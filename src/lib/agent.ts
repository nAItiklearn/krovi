// Agent Mode: tell Krovi what to do, review the plan, confirm what needs confirming.
//
// TAURI: a typed bridge over the `agent_*` commands (src-tauri/src/agent.rs, which wraps the
//   `krovi-agent` crate). The Rust side understands the sentence, decides what is Safe / needs a
//   Confirm / is Blocked, and does the work. The only thing done here is the timer: timers live in
//   the frontend, so Rust emits `agent-timer` and `startAgent()` turns it into a running timer.
// DEMO (plain browser): there is no Rust. A small TypeScript planner understands a handful of
//   phrasings and the same safe / confirm / blocked rules apply. Timers, notes and reminders really
//   happen in the preview (they are local); nothing is ever opened or sent.
//
// Honest by construction: Krovi never sends or reads email (it opens a draft in YOUR mail app and
// you press Send), and MCP servers are a registry only: nothing starts or contacts them.

import { bridge, invoke, isTauri, listen } from './bridge';
import { addNote, calendarTick, pushToast, timerAdd } from './stores';
import type { IconName } from './icons';

// ---------------------------------------------------------------------------------------------
// Types (the JSON shapes of src-tauri/crates/krovi-agent, camelCase)
// ---------------------------------------------------------------------------------------------

export type MailProvider = 'gmail' | 'outlook' | 'default';

export type AgentAction =
  | { type: 'open-url'; url: string; site: string; known: boolean }
  | { type: 'search'; site: string; query: string; url: string }
  | { type: 'open-app'; name: string; target: string }
  | { type: 'timer'; seconds: number; label: string }
  | { type: 'reminder'; title: string; date: string; time: string }
  | { type: 'note'; body: string }
  | { type: 'compose-email'; provider: MailProvider; to: string[]; subject: string; body: string }
  | { type: 'unsupported'; text: string };

export type Risk = { level: 'safe' } | { level: 'confirm' } | { level: 'blocked'; reason: string };

export interface AgentStep {
  /** Unique within the plan: "s1", "s2", ... */
  id: string;
  action: AgentAction;
  /** One plain sentence, e.g. "Open youtube.com in your browser". */
  summary: string;
  risk: Risk;
}

export interface AgentPlan {
  id: string;
  steps: AgentStep[];
  /** Example commands to show when part of the sentence was not understood. */
  suggestions: string[];
}

export type TaskStatus =
  | { state: 'pending' | 'awaiting-confirmation' | 'running' | 'done' | 'cancelled' }
  | { state: 'failed'; message: string };

export interface AgentTask {
  id: number;
  planId: string;
  stepId: string;
  summary: string;
  status: TaskStatus;
  createdAt: number;
  updatedAt: number;
}

export type IntegrationStatus = 'builtin' | 'not-connected' | 'coming-soon';

export interface Integration {
  id: string;
  name: string;
  summary: string;
  status: IntegrationStatus;
  /** What data it touches. */
  data: string;
  /** Whether anything leaves this PC, and who sends it. */
  network: string;
}

export type McpTransport = { type: 'stdio'; command: string; args: string[] } | { type: 'http'; url: string };

export interface McpServer {
  id: string;
  name: string;
  transport: McpTransport;
  /** Off by default. Even when on, nothing runs the server yet. */
  enabled: boolean;
}

/** What the form sends; the backend assigns the id on first save. */
export interface McpServerInput {
  id?: string;
  name: string;
  transport: McpTransport;
  enabled?: boolean;
}

/** Payload of the `agent-timer` event. */
export interface TimerEvent {
  seconds: number;
  label?: string;
}

export const MAX_TASKS = 50;
export const MAX_TIMER_SECONDS = 24 * 3600;

// ---------------------------------------------------------------------------------------------
// Pure helpers (unit-tested)
// ---------------------------------------------------------------------------------------------

export function riskLevel(step: AgentStep): Risk['level'] {
  return step.risk.level;
}

/** The steps grouped by what they need. */
export function splitSteps(plan: AgentPlan): { safe: AgentStep[]; confirm: AgentStep[]; blocked: AgentStep[] } {
  const out = { safe: [] as AgentStep[], confirm: [] as AgentStep[], blocked: [] as AgentStep[] };
  for (const s of plan.steps) out[s.risk.level].push(s);
  return out;
}

/** Is there anything in this plan that can run (now or after a confirmation)? */
export function hasRunnable(plan: AgentPlan): boolean {
  return plan.steps.some((s) => s.risk.level !== 'blocked');
}

export function isFinal(status: TaskStatus): boolean {
  return status.state === 'done' || status.state === 'failed' || status.state === 'cancelled';
}

/** The task made for one step of a plan, if any. */
export function taskFor(tasks: AgentTask[], planId: string, stepId: string): AgentTask | undefined {
  return tasks.find((t) => t.planId === planId && t.stepId === stepId);
}

export function statusLabel(status: TaskStatus): string {
  switch (status.state) {
    case 'pending':
      return 'Queued';
    case 'awaiting-confirmation':
      return 'Waiting for your OK';
    case 'running':
      return 'Running';
    case 'done':
      return 'Done';
    case 'cancelled':
      return 'Cancelled';
    case 'failed':
      return status.message ? `Failed: ${status.message}` : 'Failed';
  }
}

/** Insert or replace a task (by id), newest first, capped at {@link MAX_TASKS} (oldest finished go first). */
export function mergeTask(list: AgentTask[], task: AgentTask): AgentTask[] {
  const next = [task, ...list.filter((t) => t.id !== task.id)].sort((a, b) => b.id - a.id);
  while (next.length > MAX_TASKS) {
    let i = -1;
    for (let k = next.length - 1; k >= 0; k--) {
      if (isFinal(next[k].status)) {
        i = k;
        break;
      }
    }
    next.splice(i >= 0 ? i : next.length - 1, 1);
  }
  return next;
}

const clone = <T>(v: T): T => (typeof structuredClone === 'function' ? structuredClone(v) : (JSON.parse(JSON.stringify(v)) as T));

/** "10 minutes", "1 hour 30 minutes", "1 minute 30 seconds": the wording the plan summaries use. */
export function longDuration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  const unit = (n: number, name: string) => `${n} ${name}${n === 1 ? '' : 's'}`;
  const parts: string[] = [];
  if (Math.floor(s / 3600)) parts.push(unit(Math.floor(s / 3600), 'hour'));
  if (Math.floor((s % 3600) / 60)) parts.push(unit(Math.floor((s % 3600) / 60), 'minute'));
  if (s % 60 || parts.length === 0) parts.push(unit(s % 60, 'second'));
  return parts.join(' ');
}

/** "10 min", "1 h 30 min", "90 s". */
export function formatSeconds(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s} s`;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const rest = s % 60;
  const parts: string[] = [];
  if (h) parts.push(`${h} h`);
  if (m) parts.push(`${m} min`);
  if (rest) parts.push(`${rest} s`);
  return parts.join(' ');
}

/** The timer a `agent-timer` event asks for, or `null` if it is not a sane request. */
export function timerFromEvent(p: unknown): { label: string; minutes: number } | null {
  if (!p || typeof p !== 'object') return null;
  const { seconds, label } = p as TimerEvent;
  if (typeof seconds !== 'number' || !Number.isFinite(seconds)) return null;
  if (seconds < 1 || seconds > MAX_TIMER_SECONDS) return null;
  const named = typeof label === 'string' ? label.trim() : '';
  return { label: named || formatSeconds(seconds), minutes: seconds / 60 };
}

/** One argument per line; blank lines are dropped, nothing else is changed. */
export function parseArgs(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((l) => l.replace(/\s+$/, ''))
    .filter((l) => l.trim() !== '');
}

export interface McpDraft {
  name: string;
  kind: 'stdio' | 'http';
  command: string;
  args: string;
  url: string;
}

/** Build the form's server from its fields. The backend validates everything again. */
export function draftToInput(d: McpDraft): McpServerInput {
  const transport: McpTransport =
    d.kind === 'http' ? { type: 'http', url: d.url.trim() } : { type: 'stdio', command: d.command.trim(), args: parseArgs(d.args) };
  return { name: d.name.trim(), transport };
}

export function describeTransport(t: McpTransport): string {
  return t.type === 'http' ? t.url : [t.command, ...t.args].join(' ');
}

export function stepIcon(a: AgentAction): IconName {
  switch (a.type) {
    case 'open-url':
      return 'externalLink';
    case 'search':
      return 'search';
    case 'open-app':
      return 'appWindow';
    case 'timer':
      return 'timers';
    case 'reminder':
      return 'bell';
    case 'note':
      return 'notes';
    case 'compose-email':
      return 'inbox';
    case 'unsupported':
      return 'alert';
  }
}

export const PROVIDER_LABEL: Record<MailProvider, string> = {
  gmail: 'Gmail',
  outlook: 'Outlook',
  default: 'your default mail app',
};

/** Starter commands for the empty state. */
export const EXAMPLES: readonly string[] = [
  'open youtube',
  'search google for rust lifetimes',
  'set a timer for 10 minutes',
  'remind me in 20 minutes to stretch',
  'note buy milk',
  'email sam@example.com about the trip',
];

// ---------------------------------------------------------------------------------------------
// Timers (they live here, in the frontend)
// ---------------------------------------------------------------------------------------------

function handleTimerEvent(p: unknown): void {
  const t = timerFromEvent(p);
  if (!t) return;
  timerAdd(t.label, t.minutes, true);
  pushToast(`Timer started: ${t.label}`, 2200);
}

let starting: Promise<() => void> | null = null;

/**
 * Listen for `agent-timer` so a "set a timer" step starts a real timer. Idempotent: call it from
 * the app shell once, and from the Agent panel; only the first call subscribes. Resolves to an
 * unsubscribe function.
 */
export function startAgent(): Promise<() => void> {
  if (!starting) {
    starting = (async () => {
      const off = await listen<TimerEvent>('agent-timer', handleTimerEvent);
      return () => {
        off();
        starting = null;
      };
    })();
  }
  return starting;
}

// ---------------------------------------------------------------------------------------------
// Demo mode
// ---------------------------------------------------------------------------------------------

interface DemoSite {
  name: string;
  label: string;
  url: string;
  search?: string;
}
const DEMO_SITES: Record<string, DemoSite> = {
  youtube: { name: 'YouTube', label: 'youtube.com', url: 'https://www.youtube.com/', search: 'https://www.youtube.com/results?search_query=' },
  google: { name: 'Google', label: 'google.com', url: 'https://www.google.com/', search: 'https://www.google.com/search?q=' },
  github: { name: 'GitHub', label: 'github.com', url: 'https://github.com/', search: 'https://github.com/search?q=' },
  wikipedia: { name: 'Wikipedia', label: 'wikipedia.org', url: 'https://www.wikipedia.org/', search: 'https://en.wikipedia.org/w/index.php?search=' },
  gmail: { name: 'Gmail', label: 'mail.google.com', url: 'https://mail.google.com/' },
  reddit: { name: 'Reddit', label: 'reddit.com', url: 'https://www.reddit.com/' },
  netflix: { name: 'Netflix', label: 'netflix.com', url: 'https://www.netflix.com/' },
};
const DEMO_APPS = ['Notepad', 'Calculator', 'Paint', 'Spotify'];
const DEMO_SUGGESTIONS = EXAMPLES.slice(0, 4);
const DOMAIN = /^(?:https?:\/\/)?[a-z0-9-]+(?:\.[a-z0-9-]+)+(?:[/?#]\S*)?$/i;

const pad2 = (n: number) => String(n).padStart(2, '0');
function clock12(h: number, m: number): string {
  return `${h % 12 === 0 ? 12 : h % 12}:${pad2(m)} ${h < 12 ? 'AM' : 'PM'}`;
}
const DAY = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
const MON = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

function blockedStep(id: string, text: string, summary: string, reason: string): AgentStep {
  return { id, action: { type: 'unsupported', text }, summary, risk: { level: 'blocked', reason } };
}

function demoStep(seg: string, id: string, now: Date): AgentStep {
  const t = seg.trim().replace(/[.!]+$/, '').trim();
  let m: RegExpMatchArray | null;

  if ((m = /^(?:take a note|new note|note)(?:\s*[:-]\s*|\s+)(.+)$/i.exec(t))) {
    const body = m[1].charAt(0).toUpperCase() + m[1].slice(1);
    return { id, action: { type: 'note', body }, summary: `Save a note: "${body}"`, risk: { level: 'safe' } };
  }

  if (/\btimer\b/i.test(t) && (m = /(\d+(?:\.\d+)?)\s*(seconds?|secs?|minutes?|mins?|hours?|hrs?)\b/i.exec(t))) {
    const unit = m[2].toLowerCase();
    const seconds = Math.round(Number(m[1]) * (unit.startsWith('h') ? 3600 : unit.startsWith('m') ? 60 : 1));
    const summary = `Start a timer for ${longDuration(seconds)}`;
    if (seconds < 1) return blockedStep(id, t, summary, 'A timer needs at least one second.');
    if (seconds > MAX_TIMER_SECONDS) return blockedStep(id, t, summary, 'Timers are limited to 24 hours.');
    return { id, action: { type: 'timer', seconds, label: '' }, summary, risk: { level: 'safe' } };
  }

  if ((m = /^remind me in (\d+)\s*(minutes?|mins?|hours?|hrs?)\s+to\s+(.+)$/i.exec(t))) {
    const mins = Number(m[1]) * (m[2].toLowerCase().startsWith('h') ? 60 : 1);
    const due = new Date(Math.ceil((now.getTime() + mins * 60_000) / 60_000) * 60_000);
    const title = m[3].charAt(0).toUpperCase() + m[3].slice(1);
    const date = `${due.getFullYear()}-${pad2(due.getMonth() + 1)}-${pad2(due.getDate())}`;
    const time = `${pad2(due.getHours())}:${pad2(due.getMinutes())}`;
    return {
      id,
      action: { type: 'reminder', title, date, time },
      summary: `Remind you on ${DAY[due.getDay()]} ${due.getDate()} ${MON[due.getMonth()]} at ${clock12(due.getHours(), due.getMinutes())}: "${title}"`,
      risk: { level: 'confirm' },
    };
  }

  if ((m = /^(?:email|draft an email to)\s+(\S+)(?:\s+about\s+(.+))?$/i.exec(t))) {
    const to = m[1];
    const subject = m[2] ? m[2].charAt(0).toUpperCase() + m[2].slice(1) : '';
    if (!/^[A-Za-z0-9._+'-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}$/.test(to)) {
      return blockedStep(
        id,
        to,
        `Draft an email to "${to}"`,
        `"${to}" is not an email address. Krovi doesn't read your contacts, so say the address, for example name@example.com.`,
      );
    }
    return {
      id,
      action: { type: 'compose-email', provider: 'default', to: [to], subject, body: '' },
      summary: `Open an email draft to ${to}${subject ? ` about "${subject}"` : ''} in your default mail app. Krovi does not send it: you press Send.`,
      risk: { level: 'confirm' },
    };
  }

  const search = (siteKey: string, q: string): AgentStep => {
    const site = DEMO_SITES[siteKey];
    return {
      id,
      action: { type: 'search', site: site.name, query: q, url: site.search + encodeURIComponent(q) },
      summary: `Search ${site.name} for "${q}"`,
      risk: { level: 'safe' },
    };
  };
  if ((m = /^search\s+(youtube|google|github|wikipedia)\s+for\s+(.+)$/i.exec(t))) return search(m[1].toLowerCase(), m[2].trim());
  if ((m = /^play\s+(.+?)\s+on\s+youtube$/i.exec(t))) return search('youtube', m[1].trim());
  if ((m = /^(?:google|search for|search)\s+(.+)$/i.exec(t))) return search('google', m[1].trim());

  if ((m = /^(?:open|go to|visit|launch|start)\s+(?:the\s+)?(.+?)(?:\s+website)?$/i.exec(t))) {
    const name = m[1].trim();
    const key = name.toLowerCase();
    const app = DEMO_APPS.find((a) => a.toLowerCase() === key);
    if (app) {
      return { id, action: { type: 'open-app', name: app, target: `shell:AppsFolder\\${app}` }, summary: `Open ${app}`, risk: { level: 'safe' } };
    }
    const host = key.replace(/^https?:\/\//, '').replace(/^www\./, '').replace(/\/$/, '');
    const known = DEMO_SITES[key] ?? Object.values(DEMO_SITES).find((s) => s.label === host);
    if (known) {
      return { id, action: { type: 'open-url', url: known.url, site: known.name, known: true }, summary: `Open ${known.label} in your browser`, risk: { level: 'safe' } };
    }
    if (/^[a-z][a-z0-9+.-]*:/i.test(name) && !/^https?:\/\//i.test(name) && !/^[a-z0-9.-]+:\d+/i.test(name)) {
      const scheme = name.split(':')[0].toLowerCase();
      return blockedStep(id, name, `Open "${name}"`, `Only http and https links can be opened, not ${scheme}:.`);
    }
    if (DOMAIN.test(name)) {
      const url = /^https?:\/\//i.test(name) ? name : `https://${name}`;
      return { id, action: { type: 'open-url', url, site: url.replace(/^https?:\/\//i, '').split('/')[0], known: false }, summary: `Open ${url} in your browser`, risk: { level: 'confirm' } };
    }
    return blockedStep(id, name, `Open "${name}"`, `I couldn't find an app called "${name}" in your Start menu.`);
  }

  return blockedStep(id, t, t ? `Not understood: "${t}"` : 'Nothing to do', "I didn't understand that.");
}

/** A small stand-in for the Rust planner, for the browser preview only. */
function demoPlan(text: string, planId: string, now = new Date()): AgentPlan {
  // split into steps only where the next part starts like a command
  const parts = text
    .trim()
    .split(/\s+(?:and then|then|and)\s+(?=(?:open|go to|visit|launch|search|google|play|set|timer|note|remind|email|draft)\b)/i)
    .filter((p) => p.trim());
  const steps = (parts.length ? parts : ['']).slice(0, 8).map((p, i) => demoStep(p, `s${i + 1}`, now));
  const lost = steps.some((s) => s.action.type === 'unsupported' && s.risk.level === 'blocked' && s.risk.reason === "I didn't understand that.");
  return { id: planId, steps, suggestions: lost ? DEMO_SUGGESTIONS : [] };
}

const DEMO_INTEGRATIONS: Integration[] = [
  { id: 'websites', name: 'Websites', summary: "Opens well-known sites (YouTube, GitHub, Wikipedia and more) in your browser. An address you type yourself asks for your OK first.", status: 'builtin', data: 'The site you named. Nothing else.', network: 'Your browser makes the request when the page opens. Krovi sends nothing itself.' },
  { id: 'google-search', name: 'Google search', summary: 'Opens a Google results page for the words you give.', status: 'builtin', data: 'The words you searched for, inside the link your browser opens.', network: 'Your browser sends the search to Google. Krovi sends nothing itself.' },
  { id: 'youtube', name: 'YouTube', summary: 'Opens YouTube or its search results. It does not play anything by itself.', status: 'builtin', data: 'The words you searched for, inside the link your browser opens.', network: 'Your browser sends the search to YouTube. Krovi sends nothing itself.' },
  { id: 'apps', name: 'Apps', summary: 'Launches apps from your Start menu.', status: 'builtin', data: 'The names of your Start-menu apps, read on this PC.', network: 'None. This stays on this PC.' },
  { id: 'timers', name: 'Timers', summary: "Starts a timer in Krovi's Timers tool.", status: 'builtin', data: 'The duration and an optional name, stored on this PC.', network: 'None. This stays on this PC.' },
  { id: 'reminders', name: 'Reminders', summary: "Adds a reminder to Krovi's calendar; a Windows notification rings at that time.", status: 'builtin', data: 'The reminder text, date and time, stored on this PC.', network: 'None. This stays on this PC.' },
  { id: 'notes', name: 'Notes', summary: "Saves a note in Krovi's Notes tool.", status: 'builtin', data: 'The note text, stored on this PC.', network: 'None. This stays on this PC.' },
  { id: 'gmail-compose', name: 'Gmail draft', summary: 'Opens a Gmail compose window with the recipient, subject and message filled in. You press Send in Gmail. Krovi never sends or reads email.', status: 'builtin', data: 'The recipient, subject and message you approved, inside the link your browser opens. Krovi cannot see your mailbox.', network: 'Your browser opens mail.google.com. Krovi has no access to your Gmail account.' },
  { id: 'outlook-compose', name: 'Outlook draft', summary: 'Opens an Outlook web compose window with the recipient, subject and message filled in. You press Send in Outlook. Krovi never sends or reads email.', status: 'builtin', data: 'The recipient, subject and message you approved, inside the link your browser opens. Krovi cannot see your mailbox.', network: 'Your browser opens outlook.live.com. Krovi has no access to your Outlook account.' },
  { id: 'mailto', name: 'Default mail app draft', summary: 'Opens a draft in whatever mail app Windows uses by default. You press Send there. Krovi never sends or reads email.', status: 'builtin', data: 'The recipient, subject and message you approved, handed to your mail app.', network: 'Whatever your mail app does when you send. Krovi sends nothing itself.' },
  { id: 'google-calendar', name: 'Google Calendar', summary: 'Not available yet. Krovi has no Google sign-in, so it cannot see or change your Google calendar.', status: 'coming-soon', data: 'None. Nothing is connected.', network: 'None.' },
  { id: 'mcp-servers', name: 'MCP servers', summary: 'Registry only. You can record MCP servers here, but Krovi does not start them or talk to them yet.', status: 'coming-soon', data: "A server's name and how to start or reach it, stored on this PC as plain text.", network: 'None. Krovi never contacts a registered server.' },
];

const MCP_KEY = 'krovi.agent.mcp.demo';
const MAX_MCP = 20;

const demo = {
  plans: new Map<string, AgentPlan>(),
  tasks: [] as AgentTask[],
  nextTask: 1,
  nextPlan: 1,
  mcp: null as McpServer[] | null,
  listeners: new Set<(t: AgentTask) => void>(),
};

function demoMcp(): McpServer[] {
  if (!demo.mcp) {
    try {
      const raw = typeof localStorage !== 'undefined' ? localStorage.getItem(MCP_KEY) : null;
      demo.mcp = raw ? (JSON.parse(raw) as McpServer[]) : [];
    } catch {
      demo.mcp = [];
    }
  }
  return demo.mcp;
}
function persistDemoMcp(): void {
  try {
    localStorage.setItem(MCP_KEY, JSON.stringify(demoMcp()));
  } catch {
    /* blocked storage: the preview forgets on reload */
  }
}

function demoValidateMcp(s: McpServerInput, others: McpServer[]): void {
  const name = s.name.trim().replace(/\s+/g, ' ');
  if (!name) throw new Error('Give the server a name.');
  if (name.length > 40) throw new Error('That name is too long (over 40 characters).');
  if (others.some((o) => o.name.toLowerCase() === name.toLowerCase() && o.id !== s.id)) throw new Error('A server with that name is already registered.');
  if (s.transport.type === 'stdio') {
    const c = s.transport.command.trim();
    if (!c) throw new Error('Enter the program that starts the server.');
    if (/[&|;<>^"%$`]/.test(c)) throw new Error('The command cannot contain control characters or the symbols & | ; < > ^ " % $ `.');
    if (/\s/.test(c) && !/^[A-Za-z]:[\\/]/.test(c)) throw new Error('Put only the program in Command. Arguments go in the Arguments list.');
  } else {
    const u = s.transport.url.trim();
    if (!/^https?:\/\/[^\s@/]+/i.test(u)) throw new Error('Enter a full http(s):// address.');
    if (/^http:\/\//i.test(u) && !/^http:\/\/(localhost|127\.\d+\.\d+\.\d+)(?::\d+)?(?:[/?#]|$)/i.test(u)) throw new Error('Use https:// for a remote server. Plain http is only allowed for localhost.');
  }
}

function demoEmit(task: AgentTask): void {
  for (const cb of [...demo.listeners]) cb({ ...task });
}

function demoNewTask(plan: AgentPlan, step: AgentStep, state: 'running' | 'awaiting-confirmation'): AgentTask {
  const now = Date.now();
  const task: AgentTask = { id: demo.nextTask++, planId: plan.id, stepId: step.id, summary: step.summary, status: { state }, createdAt: now, updatedAt: now };
  demo.tasks = mergeTask(demo.tasks, task);
  return task;
}

function demoSet(task: AgentTask, status: TaskStatus): void {
  task.status = status;
  task.updatedAt = Date.now();
  demoEmit(task);
}

async function demoExecute(step: AgentStep): Promise<void> {
  const a = step.action;
  if (a.type === 'timer') {
    handleTimerEvent({ seconds: a.seconds, label: a.label });
  } else if (a.type === 'note') {
    if (!(await addNote(a.body))) throw new Error('Could not save the note.');
  } else if (a.type === 'reminder') {
    await bridge.addCalendarEvent({ date: a.date, title: a.title, time: a.time });
    calendarTick.update((n) => n + 1);
  }
  // opening links, apps and mail drafts is not possible in a browser preview
}

async function demoRun(planId: string, confirmed: string[]): Promise<AgentTask[]> {
  const plan = demo.plans.get(planId);
  if (!plan) throw new Error('That plan has expired. Ask again.');
  const touched: AgentTask[] = [];
  for (const step of plan.steps) {
    if (step.risk.level === 'blocked') continue;
    const allowed = step.risk.level === 'safe' || confirmed.includes(step.id);
    let task = demo.tasks.find((t) => t.planId === planId && t.stepId === step.id);
    if (!task) {
      task = demoNewTask(plan, step, allowed ? 'running' : 'awaiting-confirmation');
      demoEmit(task);
    } else if (task.status.state === 'awaiting-confirmation' && allowed) {
      demoSet(task, { state: 'running' });
    } else {
      continue;
    }
    touched.push(task);
    if (allowed) {
      try {
        await demoExecute(step);
        demoSet(task, { state: 'done' });
      } catch (e) {
        demoSet(task, { state: 'failed', message: e instanceof Error ? e.message : String(e) });
      }
    }
  }
  return touched.map((t) => ({ ...t }));
}

/** Test hook: forget every demo plan, task and registered server. */
export function __resetDemo(): void {
  demo.plans.clear();
  demo.tasks = [];
  demo.nextTask = 1;
  demo.nextPlan = 1;
  demo.mcp = null;
  demo.listeners.clear();
  try {
    localStorage.removeItem(MCP_KEY);
  } catch {
    /* no storage */
  }
}

// ---------------------------------------------------------------------------------------------
// The bridge
// ---------------------------------------------------------------------------------------------

export const agent = {
  /** Start reading the Start-menu app list in the background so the first command is quick. */
  warm(): void {
    if (isTauri()) void invoke<void>('agent_warm').catch(() => {});
  },

  /** Understand a sentence. Nothing runs yet. */
  async plan(text: string): Promise<AgentPlan> {
    if (isTauri()) return invoke<AgentPlan>('agent_plan', { text });
    const plan = demoPlan(text, `demo-${demo.nextPlan++}`);
    demo.plans.set(plan.id, plan);
    return clone(plan);
  },

  /**
   * Run a plan: every Safe step, plus the Confirm steps whose ids are in `confirmed`. Blocked
   * steps never run and nothing runs twice. Resolves to the tasks that were created or changed.
   */
  async run(planId: string, confirmed: string[] = []): Promise<AgentTask[]> {
    if (isTauri()) return invoke<AgentTask[]>('agent_run', { planId, confirmed });
    return demoRun(planId, confirmed);
  },

  async tasks(): Promise<AgentTask[]> {
    if (isTauri()) return invoke<AgentTask[]>('agent_tasks');
    return demo.tasks.map((t) => ({ ...t }));
  },

  /** Cancel a task that has not started (a confirmation card's Cancel). */
  async cancel(taskId: number): Promise<AgentTask> {
    if (isTauri()) return invoke<AgentTask>('agent_cancel', { taskId });
    const task = demo.tasks.find((t) => t.id === taskId);
    if (!task) throw new Error('That task no longer exists.');
    if (task.status.state !== 'pending' && task.status.state !== 'awaiting-confirmation') {
      throw new Error(`A task that is ${statusLabel(task.status).toLowerCase()} cannot be cancelled.`);
    }
    demoSet(task, { state: 'cancelled' });
    return { ...task };
  },

  async integrations(): Promise<Integration[]> {
    if (isTauri()) return invoke<Integration[]>('agent_integrations');
    const servers = demoMcp().map<Integration>((s) => ({
      id: `mcp:${s.id}`,
      name: s.name,
      summary: `Registered MCP server, reached through ${s.transport.type === 'http' ? `an HTTP address (${s.transport.url})` : `a local program (${s.transport.command})`}. Not connected: Krovi does not start or call MCP servers yet.`,
      status: 'not-connected',
      data: 'Its name and how to start or reach it, stored on this PC as plain text.',
      network: 'None. Krovi never contacts a registered server.',
    }));
    return [...clone(DEMO_INTEGRATIONS), ...servers];
  },

  async mcpList(): Promise<McpServer[]> {
    return isTauri() ? invoke<McpServer[]>('agent_mcp_list') : clone(demoMcp());
  },

  /** Add a server to the registry, or update the one with the same id. Registry only. */
  async mcpSave(server: McpServerInput): Promise<McpServer[]> {
    if (isTauri()) return invoke<McpServer[]>('agent_mcp_save', { server });
    const list = demoMcp();
    demoValidateMcp(server, list);
    const existing = server.id ? list.find((s) => s.id === server.id) : undefined;
    if (!existing && list.length >= MAX_MCP) throw new Error(`At most ${MAX_MCP} servers can be registered.`);
    let id = existing?.id ?? '';
    if (!id) {
      const base = `mcp-${server.name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'server'}`;
      id = base;
      for (let n = 2; list.some((s) => s.id === id); n++) id = `${base}-${n}`;
    }
    const clean: McpServer = {
      id,
      name: server.name.trim().replace(/\s+/g, ' '),
      transport: clone(server.transport),
      enabled: server.enabled ?? false,
    };
    const at = list.findIndex((s) => s.id === id);
    if (at >= 0) list[at] = clean;
    else list.push(clean);
    persistDemoMcp();
    return clone(list);
  },

  async mcpRemove(id: string): Promise<McpServer[]> {
    if (isTauri()) return invoke<McpServer[]>('agent_mcp_remove', { id });
    demo.mcp = demoMcp().filter((s) => s.id !== id);
    persistDemoMcp();
    return clone(demo.mcp);
  },

  /** A task changed (created, confirmed, finished, cancelled). Resolves to an unsubscribe function. */
  async onTask(cb: (task: AgentTask) => void): Promise<() => void> {
    if (isTauri()) return listen<AgentTask>('agent-task', cb);
    demo.listeners.add(cb);
    return () => demo.listeners.delete(cb);
  },
};
