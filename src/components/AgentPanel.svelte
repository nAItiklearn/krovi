<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import Segmented from './Segmented.svelte';
  import State from './State.svelte';
  import AgentConfirmCard from './AgentConfirmCard.svelte';
  import { mode } from '../lib/bridge';
  import { pushToast } from '../lib/stores';
  import {
    EXAMPLES,
    agent,
    clearVoicePlan,
    describeTransport,
    draftToInput,
    hasRunnable,
    mergeTask,
    startAgent,
    statusLabel,
    stepIcon,
    taskFor,
    voicePlan,
    type AgentPlan,
    type AgentStep,
    type AgentTask,
    type Integration,
    type McpDraft,
    type McpServer,
    type VoicePlan,
  } from '../lib/agent';

  // Agent Mode: "Tell Krovi what to do". The sentence is understood on this PC (no AI service);
  // each step of the plan is Safe (runs now), needs your OK (a card), or is blocked (never runs).
  // Krovi never sends or reads email: it opens a draft in your mail app. MCP servers are a
  // registry only: nothing starts or contacts them.
  // You can also say it: the Agent voice shortcut (Settings > Dictation) plans what you said and
  // opens this panel on the plan. Its Safe steps have already run; the rest wait here as cards.

  const demo = mode() === 'demo';

  let text = $state('');
  let busy = $state(false);
  let busyStep = $state<string | null>(null);
  let error = $state('');
  let plan = $state<AgentPlan | null>(null);
  /** What Krovi heard, when the plan came from the voice shortcut. Empty for a typed sentence. */
  let heard = $state('');
  let tasks = $state<AgentTask[]>([]);
  let inputEl: HTMLInputElement | undefined = $state();

  let showIntegrations = $state(false);
  let loadedIntegrations = $state(false);
  let integrations = $state<Integration[]>([]);
  let servers = $state<McpServer[]>([]);
  let draft = $state<McpDraft>({ name: '', kind: 'stdio', command: '', args: '', url: '' });
  let mcpBusy = $state(false);
  let mcpError = $state('');

  const message = (e: unknown): string => (e instanceof Error ? e.message : String(e));
  const merge = (list: AgentTask[]) => {
    for (const t of list) tasks = mergeTask(tasks, t);
  };
  const waiting = $derived(tasks.filter((t) => t.status.state === 'awaiting-confirmation' || t.status.state === 'pending'));

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const asked = text.trim();
    if (!asked || busy) return;
    busy = true;
    error = '';
    heard = '';
    clearVoicePlan(); // a typed sentence replaces the spoken one
    try {
      const p = await agent.plan(asked);
      plan = p;
      if (hasRunnable(p)) {
        // Safe steps run now; steps that need your OK become cards and wait.
        merge(await agent.run(p.id, []));
        text = '';
      }
    } catch (err) {
      error = message(err);
    } finally {
      busy = false;
    }
  }

  async function confirm(step: AgentStep) {
    if (!plan || busyStep) return;
    busyStep = step.id;
    error = '';
    try {
      merge(await agent.run(plan.id, [step.id]));
    } catch (err) {
      error = message(err);
    } finally {
      busyStep = null;
    }
  }

  async function cancelTask(task: AgentTask) {
    error = '';
    try {
      merge([await agent.cancel(task.id)]);
    } catch (err) {
      error = message(err);
    }
  }

  function tryExample(example: string) {
    text = example;
    inputEl?.focus();
  }

  async function toggleIntegrations() {
    showIntegrations = !showIntegrations;
    if (showIntegrations && !loadedIntegrations) await loadIntegrations();
  }

  async function loadIntegrations() {
    try {
      [integrations, servers] = await Promise.all([agent.integrations(), agent.mcpList()]);
      loadedIntegrations = true;
    } catch (err) {
      error = message(err);
    }
  }

  async function saveServer(e: SubmitEvent) {
    e.preventDefault();
    if (mcpBusy) return;
    mcpBusy = true;
    mcpError = '';
    try {
      servers = await agent.mcpSave(draftToInput(draft));
      integrations = await agent.integrations();
      draft = { name: '', kind: draft.kind, command: '', args: '', url: '' };
      pushToast('Saved to the registry. Nothing was started.', 2600);
    } catch (err) {
      mcpError = message(err);
    } finally {
      mcpBusy = false;
    }
  }

  async function removeServer(server: McpServer) {
    mcpError = '';
    try {
      servers = await agent.mcpRemove(server.id);
      integrations = await agent.integrations();
    } catch (err) {
      mcpError = message(err);
    }
  }

  const statusTag = (s: Integration['status']) => (s === 'builtin' ? 'Built in' : s === 'not-connected' ? 'Not connected' : 'Not available yet');
  const timeOf = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });

  /**
   * Show a plan made from a spoken command. Rust has already run its Safe steps (and any
   * `agent-task` events may have come before this panel existed), so take the task list again.
   * The typed field is left alone: it may hold something the user is still writing.
   */
  function showVoicePlan(v: VoicePlan) {
    plan = v.plan;
    heard = v.text;
    error = v.error;
    void agent.tasks().then((t) => (tasks = t)).catch(() => {});
  }

  onMount(() => {
    void startAgent(); // so a "set a timer" step starts a real timer, and a spoken command reaches us
    agent.warm(); // read the Start-menu app list now, so "open spotify" is quick
    void agent.tasks().then((t) => (tasks = t)).catch(() => {});
    let off: (() => void) | undefined;
    void agent.onTask((t) => (tasks = mergeTask(tasks, t))).then((u) => (off = u));
    // Called at once with the latest spoken plan (if any), and again for each new one.
    const offVoice = voicePlan.subscribe((v) => {
      if (v) showVoicePlan(v);
    });
    inputEl?.focus();
    return () => {
      off?.();
      offVoice();
    };
  });
</script>

<div class="view agent">
  <div class="view-head">
    <div>
      <h2>Agent</h2>
      <p>Tell Krovi what to do. Safe steps run straight away; anything that needs your OK waits for it.</p>
    </div>
  </div>

  <form class="ask" onsubmit={submit}>
    <input
      class="input"
      type="text"
      placeholder="Tell Krovi what to do"
      aria-label="Tell Krovi what to do"
      maxlength="500"
      autocomplete="off"
      spellcheck="false"
      bind:value={text}
      bind:this={inputEl} />
    <button class="btn primary" type="submit" disabled={busy || !text.trim()}>
      <Icon name={busy ? 'loader' : 'enter'} size={14} />{busy ? 'Working...' : 'Go'}
    </button>
  </form>

  <div class="body scroll">
    {#if demo}
      <p class="demo faint"><Icon name="info" size={13} /> Preview: links, apps and email drafts are not opened here. Timers, notes and reminders do work.</p>
    {/if}
    {#if error}
      <p class="error" role="alert"><Icon name="alert" size={14} /> {error}</p>
    {/if}

    {#if plan}
      <section aria-label="Plan">
        <h3 class="eyebrow"><Icon name="listChecks" size={13} /> Plan</h3>
        {#if heard}
          <p class="heard"><Icon name="mic" size={13} /><span>Heard: “{heard}”</span></p>
        {/if}
        <ul class="steps">
          {#each plan.steps as step (step.id)}
            {@const task = taskFor(tasks, plan.id, step.id)}
            <li>
              {#if step.risk.level === 'confirm' && task?.status.state === 'awaiting-confirmation'}
                <AgentConfirmCard {step} busy={busyStep === step.id} onrun={() => confirm(step)} oncancel={() => cancelTask(task)} />
              {:else}
                <div class="step card" class:blocked={step.risk.level === 'blocked'}>
                  <span class="ico" aria-hidden="true"><Icon name={stepIcon(step.action)} size={16} /></span>
                  <div class="tx">
                    <span class="nm">{step.summary}</span>
                    {#if step.risk.level === 'blocked'}<span class="why">{step.risk.reason}</span>{/if}
                  </div>
                  <span class="st" class:bad={task?.status.state === 'failed'}>
                    {#if step.risk.level === 'blocked'}Not run{:else if task}{statusLabel(task.status)}{:else}Preparing...{/if}
                  </span>
                </div>
              {/if}
            </li>
          {/each}
        </ul>
        {#if plan.suggestions.length}
          <div class="try">
            <span class="faint">Try:</span>
            {#each plan.suggestions as s (s)}
              <button class="chip" type="button" onclick={() => tryExample(s)}>{s}</button>
            {/each}
          </div>
        {/if}
      </section>
    {:else}
      <section aria-label="Examples">
        <State icon="sparkles" title="What should Krovi do?" text="Open a site or app, search, set a timer or reminder, take a note, or prepare an email draft. You can chain steps with 'then'." compact>
          <div class="try center">
            {#each EXAMPLES as example (example)}
              <button class="chip" type="button" onclick={() => tryExample(example)}>{example}</button>
            {/each}
          </div>
        </State>
      </section>
    {/if}

    <section aria-label="Tasks">
      <h3 class="eyebrow"><Icon name="activity" size={13} /> Tasks {#if waiting.length}<span class="tag accent">{waiting.length} waiting</span>{/if}</h3>
      {#if tasks.length}
        <ul class="tasks">
          {#each tasks as task (task.id)}
            <li class="task">
              <span class="dot {task.status.state}" aria-hidden="true"></span>
              <span class="nm ellipsis" title={task.summary}>{task.summary}</span>
              <span class="st" class:bad={task.status.state === 'failed'}>{statusLabel(task.status)}</span>
              <span class="when faint">{timeOf(task.updatedAt)}</span>
              {#if task.status.state === 'awaiting-confirmation' || task.status.state === 'pending'}
                <button class="btn sm ghost" type="button" onclick={() => cancelTask(task)} aria-label="Cancel: {task.summary}">Cancel</button>
              {/if}
            </li>
          {/each}
        </ul>
      {:else}
        <p class="faint small">Nothing has run yet.</p>
      {/if}
    </section>

    <section aria-label="Integrations and MCP">
      <button class="head-toggle" type="button" onclick={toggleIntegrations} aria-expanded={showIntegrations}>
        <span class="eyebrow tog"><Icon name="link" size={13} /> Integrations and MCP</span>
        <Icon name={showIntegrations ? 'chevronUp' : 'chevronDown'} size={14} />
      </button>

      {#if showIntegrations}
        <ul class="integrations">
          {#each integrations as item (item.id)}
            <li class="card integ">
              <div class="row">
                <strong>{item.name}</strong>
                <span class="tag" class:accent={item.status === 'builtin'}>{statusTag(item.status)}</span>
              </div>
              <p>{item.summary}</p>
              <p class="faint small"><b>Touches:</b> {item.data}</p>
              <p class="faint small"><b>Network:</b> {item.network}</p>
            </li>
          {/each}
        </ul>

        <div class="mcp card">
          <h4>MCP servers</h4>
          <p class="registry">
            <Icon name="info" size={14} />
            <span>
              <b>Registry only: Krovi doesn't run MCP servers yet.</b>
              What you add here is saved on this PC as plain text and is never started or contacted. Servers are saved switched off, because there is nothing to switch on yet. Don't put passwords or API keys in the command, arguments or address.
            </span>
          </p>

          {#if servers.length}
            <ul class="servers">
              {#each servers as server (server.id)}
                <li>
                  <div class="tx">
                    <span class="nm">{server.name}</span>
                    <span class="mono faint ellipsis" title={describeTransport(server.transport)}>{describeTransport(server.transport)}</span>
                  </div>
                  <span class="tag">Not connected</span>
                  <button class="btn icon sm ghost danger" type="button" aria-label="Remove {server.name}" onclick={() => removeServer(server)}>
                    <Icon name="trash" size={14} />
                  </button>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="faint small">No servers registered.</p>
          {/if}

          <form class="mcp-form" onsubmit={saveServer}>
            <input class="input" type="text" placeholder="Name" aria-label="Server name" maxlength="40" bind:value={draft.name} />
            <Segmented
              size="sm"
              label="Server type"
              options={[
                { id: 'stdio', label: 'Local command' },
                { id: 'http', label: 'Web address' },
              ]}
              bind:value={draft.kind} />
            {#if draft.kind === 'stdio'}
              <input class="input mono" type="text" placeholder="Command, e.g. npx" aria-label="Command" bind:value={draft.command} />
              <textarea class="input area mono" rows="3" placeholder={'Arguments, one per line'} aria-label="Arguments, one per line" bind:value={draft.args}></textarea>
            {:else}
              <input class="input mono" type="text" placeholder="https://..." aria-label="Server address" bind:value={draft.url} />
            {/if}
            {#if mcpError}<p class="error" role="alert"><Icon name="alert" size={14} /> {mcpError}</p>{/if}
            <div class="actions">
              <button class="btn" type="submit" disabled={mcpBusy || !draft.name.trim()}>
                <Icon name="plus" size={14} /> Add to registry
              </button>
            </div>
          </form>
        </div>
      {/if}
    </section>
  </div>
</div>

<style>
  .agent {
    gap: 0;
  }
  .ask {
    display: flex;
    gap: 8px;
    flex: none;
    margin-bottom: 14px;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    align-content: start;
    gap: 18px;
    margin-right: -8px;
    padding-right: 8px;
  }
  section {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 10px;
    min-width: 0;
  }
  h3.eyebrow,
  .eyebrow.tog {
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }
  .demo {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: var(--t-xs);
  }
  .error {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    color: var(--danger);
    font-size: var(--t-sm);
    overflow-wrap: anywhere;
  }
  .error :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .heard {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 0;
    font-size: var(--t-sm);
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .heard :global(svg) {
    flex: none;
    margin-top: 3px;
  }
  .small {
    font-size: var(--t-xs);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .steps {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 8px;
  }
  .step {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    border-radius: var(--r-3);
  }
  .step.blocked {
    border-style: dashed;
    opacity: 0.85;
  }
  .ico {
    display: grid;
    place-items: center;
    flex: none;
    width: 32px;
    height: 32px;
    border-radius: 10px;
    background: var(--surface-1);
    border: 1px solid var(--line);
    color: var(--text-2);
  }
  .tx {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 2px;
  }
  .nm {
    font-size: var(--t-sm);
    font-weight: 560;
    overflow-wrap: anywhere;
  }
  .why {
    font-size: var(--t-xs);
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .st {
    flex: none;
    font-size: var(--t-xs);
    font-weight: 600;
    color: var(--text-2);
    max-width: 220px;
    overflow-wrap: anywhere;
    text-align: right;
  }
  .st.bad {
    color: var(--danger);
  }
  .try {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .try.center {
    justify-content: center;
    margin-top: 6px;
  }

  .tasks {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 2px;
  }
  .task {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 34px;
    padding: 4px 6px;
    border-radius: var(--r-2);
    border-top: 1px solid var(--line);
  }
  .task .nm {
    flex: 1;
    min-width: 0;
    font-weight: 500;
  }
  .when {
    flex: none;
    font-size: var(--t-micro);
    font-variant-numeric: tabular-nums;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-3);
  }
  .dot.done {
    background: var(--accent);
  }
  .dot.running,
  .dot.awaiting-confirmation {
    background: var(--accent);
    opacity: 0.55;
  }
  .dot.failed {
    background: var(--danger);
  }
  .dot.cancelled {
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--text-3);
  }

  .head-toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 4px 0;
    color: var(--text-2);
    text-align: left;
  }
  .head-toggle:hover {
    color: var(--text);
  }
  .integrations {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 8px;
  }
  .integ {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 4px;
    padding: 10px 14px;
    border-radius: var(--r-3);
  }
  .integ .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .integ p {
    margin: 0;
    font-size: var(--t-sm);
    line-height: 1.45;
  }
  .integ p.small {
    font-size: var(--t-xs);
  }

  .mcp {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 10px;
    padding: 14px 16px;
    border-radius: var(--r-4);
  }
  .mcp h4 {
    margin: 0;
    font-size: var(--t-md);
    font-weight: 620;
  }
  .registry {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    margin: 0;
    padding: 10px 12px;
    border-radius: var(--r-3);
    background: var(--accent-soft);
    font-size: var(--t-xs);
    line-height: 1.5;
  }
  .registry :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .servers {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 6px;
  }
  .servers li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border-radius: var(--r-2);
    background: var(--surface-1);
    border: 1px solid var(--line);
  }
  .mono {
    font-family: var(--font-mono);
    font-size: var(--t-xs);
  }
  .mcp-form {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 8px;
  }
  .area {
    height: auto;
    padding: 8px 12px;
    resize: vertical;
    line-height: 1.4;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
</style>
