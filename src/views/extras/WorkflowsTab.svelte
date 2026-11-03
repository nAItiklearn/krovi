<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import Icon from '../../components/Icon.svelte';
  import State from '../../components/State.svelte';
  import AgentConfirmCard from '../../components/AgentConfirmCard.svelte';
  import { pushToast } from '../../lib/stores';
  import { startAgent, stepIcon } from '../../lib/agent';
  import { LIMITS, parseWorkflowLines, validateWorkflow, workflows, type Workflow } from '../../lib/extras';
  import { WorkflowRun, type LineState, type RunState } from '../../lib/workflowRun';
  import type { ExtrasIntent } from '../../lib/extrasNav';

  // Workflows: a named list of Agent Mode commands, for example
  //     open spotify
  //     set a timer for 25 minutes
  //     open notion.so
  // Each line is understood by Agent Mode and follows its permission rules, one line at a time:
  // safe steps run, a step that needs your OK stops the workflow at its card until you confirm, and a
  // line Agent Mode cannot do ends the workflow. A workflow can do nothing the Agent tool cannot.

  let {
    demo,
    intent = null,
    onconsumed,
  }: { demo: boolean; intent?: ExtrasIntent | null; onconsumed?: () => void } = $props();

  let list = $state<Workflow[]>([]);
  let loaded = $state(false);
  let loadError = $state('');
  let editing = $state<{ id?: string; name: string; text: string } | null>(null);
  let saveError = $state('');
  let confirmingId = $state<string | null>(null);
  let nameEl: HTMLInputElement | undefined = $state();

  let run = $state<WorkflowRun | null>(null);
  let runState = $state<RunState | null>(null);
  let runName = $state('');
  let busyStep = $state<string | null>(null);

  const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));
  const active = $derived(runState?.status === 'running' || runState?.status === 'waiting');
  const draftInput = $derived(editing ? { id: editing.id, name: editing.name, lines: parseWorkflowLines(editing.text) } : null);
  const why = $derived(draftInput ? validateWorkflow(draftInput, list) : null);
  const hint = $derived(editing && editing.name.trim() && editing.text.trim() ? why : null);

  async function load() {
    try {
      list = await workflows.list();
      loadError = '';
    } catch (e) {
      loadError = msg(e);
    } finally {
      loaded = true;
    }
  }

  async function startEdit(w: Workflow | null) {
    saveError = '';
    confirmingId = null;
    editing = w ? { id: w.id, name: w.name, text: w.lines.join('\n') } : { name: '', text: '' };
    await tick();
    nameEl?.focus();
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (!draftInput || why) return;
    saveError = '';
    try {
      list = await workflows.save(draftInput);
      editing = null;
      pushToast('Workflow saved', 1600);
    } catch (err) {
      saveError = msg(err);
    }
  }

  async function remove(w: Workflow) {
    try {
      list = await workflows.remove(w.id);
      confirmingId = null;
    } catch (e) {
      loadError = msg(e);
    }
  }

  function editKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      editing = null;
    }
  }

  // ----------------------------------------------------------------- running

  async function start(w: Workflow) {
    if (active) return;
    void startAgent(); // so a "set a timer" line starts a real timer
    runName = w.name;
    busyStep = null;
    const r = new WorkflowRun(w.lines, undefined, (s) => (runState = s));
    run = r;
    runState = r.snapshot;
    await r.start();
  }

  async function confirmStep(stepId: string) {
    if (!run || busyStep) return;
    busyStep = stepId;
    try {
      await run.confirm(stepId);
    } finally {
      busyStep = null;
    }
  }

  async function stop(reason?: string) {
    await run?.stop(reason);
  }

  function closeRun() {
    run = null;
    runState = null;
  }

  const lineLabel = (l: LineState): string => {
    switch (l.kind) {
      case 'pending':
        return 'Waiting its turn';
      case 'planning':
        return 'Working...';
      case 'waiting':
        return 'Waiting for your OK';
      case 'done':
        return 'Done';
      case 'stopped':
        return 'Stopped';
    }
  };

  // A request from the palette ("Run workflow: Focus time").
  $effect(() => {
    const i = intent;
    if (!i?.runWorkflow || !loaded) return;
    untrack(() => {
      const w = list.find((x) => x.id === i.runWorkflow);
      onconsumed?.();
      if (w) void start(w);
      else loadError = 'That workflow no longer exists.';
    });
  });

  onMount(() => {
    void load();
    return () => {
      // leaving the view ends a workflow that is waiting for an OK; nothing carries on unseen
      if (run && (runState?.status === 'waiting' || runState?.status === 'running')) void run.stop('You left the Workflows screen.');
    };
  });
</script>

<div class="tab">
  <div class="bar">
    <p class="lede">
      A workflow is a saved list of Agent commands, run one line at a time. Safe steps run; anything that needs your OK waits for it;
      anything Agent Mode cannot do stops the workflow.
    </p>
    <button class="btn primary" type="button" onclick={() => startEdit(null)} disabled={!!editing || active || list.length >= LIMITS.workflows}>
      <Icon name="plus" size={14} /> New workflow
    </button>
  </div>

  <div class="body scroll">
    {#if editing}
      <!-- Escape closes the editor from any field in it -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <form class="card editor" onsubmit={save} onkeydown={editKey} aria-label={editing.id ? 'Edit workflow' : 'New workflow'}>
        <label>
          <span class="lbl">Name</span>
          <input class="input" bind:this={nameEl} bind:value={editing.name} maxlength={LIMITS.workflowName} placeholder="Focus time" autocomplete="off" />
        </label>
        <label>
          <span class="lbl">Commands <span class="faint">(one per line, up to {LIMITS.workflowLines})</span></span>
          <textarea
            class="input area"
            bind:value={editing.text}
            rows="6"
            spellcheck="false"
            placeholder={'open spotify\nset a timer for 25 minutes\nopen notion.so'}></textarea>
        </label>
        {#if hint || saveError}
          <p class="error" role="alert"><Icon name="alert" size={14} /> {saveError || hint}</p>
        {/if}
        <div class="actions">
          <button class="btn" type="button" onclick={() => (editing = null)}>Cancel</button>
          <button class="btn primary" type="submit" disabled={!!why}><Icon name="check" size={14} /> Save</button>
        </div>
      </form>
    {/if}

    {#if run && runState}
      <section class="card runner" aria-label="Running {runName}">
        <header>
          <div class="rtitle">
            <b>{runName}</b>
            <span class="tag" class:accent={active}>
              {runState.status === 'running' ? 'Running' : runState.status === 'waiting' ? 'Waiting for your OK' : runState.status === 'done' ? 'Finished' : runState.status === 'stopped' ? 'Stopped' : 'Starting'}
            </span>
            {#if demo}<span class="tag accent">DEMO</span>{/if}
          </div>
          {#if active}
            <button class="btn sm" type="button" onclick={() => stop()}><Icon name="square" size={12} /> Stop</button>
          {:else}
            <button class="btn sm" type="button" onclick={closeRun}>Close</button>
          {/if}
        </header>

        <ol class="lines">
          {#each run.lines as text, i (i)}
            {@const l = runState.lines[i]}
            <li class="line" class:now={i === runState.index && active}>
              <span class="mark" aria-hidden="true">
                {#if l.kind === 'done'}<Icon name="circleCheck" size={15} />
                {:else if l.kind === 'planning'}<span class="spin"><Icon name="loader" size={15} /></span>
                {:else if l.kind === 'waiting'}<Icon name="bell" size={15} />
                {:else if l.kind === 'stopped'}<Icon name="alert" size={15} />
                {:else}<Icon name="square" size={15} />{/if}
              </span>
              <div class="ltx">
                <span class="cmd">{text}</span>
                <span class="state" class:bad={l.kind === 'stopped'}>{lineLabel(l)}</span>
                {#if l.kind === 'done'}
                  {#each l.plan.steps as s (s.id)}<span class="step"><Icon name={stepIcon(s.action)} size={12} /> {s.summary}</span>{/each}
                {:else if l.kind === 'stopped'}
                  <span class="why">{l.reason}</span>
                  {#if l.plan}
                    {#each l.plan.steps as s (s.id)}<span class="step"><Icon name={stepIcon(s.action)} size={12} /> {s.summary}</span>{/each}
                  {/if}
                {:else if l.kind === 'waiting'}
                  <div class="cards">
                    {#each l.plan.steps as s (s.id)}
                      {#if l.waiting.includes(s.id)}
                        <AgentConfirmCard step={s} busy={busyStep === s.id} onrun={() => confirmStep(s.id)} oncancel={() => stop('You declined a step, so the workflow stopped.')} />
                      {:else}
                        <span class="step"><Icon name={stepIcon(s.action)} size={12} /> {s.summary}</span>
                      {/if}
                    {/each}
                  </div>
                {/if}
              </div>
            </li>
          {/each}
        </ol>
        {#if runState.message}<p class="end" class:bad={runState.status === 'stopped'}>{runState.message}</p>{/if}
      </section>
    {/if}

    {#if loadError}
      <p class="error" role="alert"><Icon name="alert" size={14} /> {loadError}</p>
    {:else if !loaded}
      <p class="faint">Loading...</p>
    {:else if list.length === 0 && !editing}
      <State
        icon="listChecks"
        title="No workflows yet"
        text="Save a few Agent commands as one workflow, for example: open spotify, set a timer for 25 minutes, open notion.so."
        action={{ label: 'New workflow', run: () => startEdit(null) }} />
    {:else}
      <ul class="list">
        {#each list as w (w.id)}
          <li class="row card">
            <div class="main">
              <span class="nm">{w.name}</span>
              <span class="pv ellipsis">{w.lines.length} {w.lines.length === 1 ? 'step' : 'steps'}: {w.lines.join(', then ')}</span>
            </div>
            <div class="acts">
              {#if confirmingId === w.id}
                <span class="faint sure">Delete?</span>
                <button class="btn sm danger" type="button" onclick={() => remove(w)}>Delete</button>
                <button class="btn sm ghost" type="button" onclick={() => (confirmingId = null)}>Keep</button>
              {:else}
                <button class="btn sm primary" type="button" onclick={() => start(w)} disabled={active || !!editing}>
                  <Icon name="play" size={12} /> Run
                </button>
                <button class="btn sm icon ghost" type="button" aria-label="Edit {w.name}" onclick={() => startEdit(w)} disabled={!!editing || active}>
                  <Icon name="pencil" size={13} />
                </button>
                <button class="btn sm icon ghost danger" type="button" aria-label="Delete {w.name}" onclick={() => (confirmingId = w.id)} disabled={!!editing || active}>
                  <Icon name="trash" size={13} />
                </button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .tab {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .bar {
    flex: none;
    display: flex;
    gap: 14px;
    align-items: center;
    justify-content: space-between;
  }
  .lede {
    margin: 0;
    max-width: 58ch;
    color: var(--text-2);
    font-size: var(--t-xs);
    line-height: 1.5;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    align-content: start;
    gap: 12px;
    padding-right: 4px;
  }
  .editor,
  .runner {
    display: grid;
    gap: 10px;
    padding: 14px 16px;
    border-radius: var(--r-4);
  }
  label {
    display: grid;
    gap: 4px;
    min-width: 0;
  }
  .lbl {
    font-size: var(--t-xs);
    font-weight: 600;
    color: var(--text-2);
  }
  .area {
    height: auto;
    padding: 8px 12px;
    resize: vertical;
    line-height: 1.45;
    font-family: var(--font-mono);
    font-size: var(--t-xs);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .error {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 0;
    color: var(--danger);
    font-size: var(--t-sm);
    overflow-wrap: anywhere;
  }
  .error :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .runner {
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  }
  .runner header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .rtitle {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .lines {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 8px;
  }
  .line {
    display: grid;
    grid-template-columns: 22px minmax(0, 1fr);
    gap: 8px;
    align-items: start;
    padding: 4px 2px;
    color: var(--text-2);
  }
  .line.now {
    color: var(--text);
  }
  .mark {
    display: grid;
    place-items: center;
    padding-top: 1px;
  }
  .spin :global(svg) {
    animation: wf-spin 1.1s linear infinite;
  }
  @keyframes wf-spin {
    to {
      transform: rotate(360deg);
    }
  }
  .ltx {
    display: grid;
    gap: 3px;
    min-width: 0;
  }
  .cmd {
    font-family: var(--font-mono);
    font-size: var(--t-xs);
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .state {
    font-size: var(--t-micro);
    font-weight: 600;
    letter-spacing: 0.02em;
  }
  .state.bad,
  .end.bad {
    color: var(--danger);
  }
  .why {
    font-size: var(--t-xs);
    color: var(--danger);
    overflow-wrap: anywhere;
  }
  .step {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--t-xs);
    color: var(--text-2);
  }
  .cards {
    display: grid;
    gap: 8px;
    margin-top: 4px;
  }
  .end {
    margin: 0;
    font-size: var(--t-sm);
    font-weight: 560;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border-radius: var(--r-3);
  }
  .main {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 3px;
  }
  .nm {
    font-size: var(--t-md);
    font-weight: 600;
  }
  .pv {
    font-size: var(--t-xs);
    color: var(--text-3);
    font-family: var(--font-mono);
  }
  .acts {
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .sure {
    font-size: var(--t-xs);
    margin-right: 2px;
  }
</style>
