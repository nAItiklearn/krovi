<script lang="ts">
  import Icon from './Icon.svelte';
  import { PROVIDER_LABEL, stepIcon, type AgentStep } from '../lib/agent';

  // A step that waits for the user's OK. It shows exactly what will happen: the full address,
  // the reminder's date and time, or the whole email draft (recipient, subject, message).
  // It never takes focus by itself, so the Enter that sent the command cannot also confirm it.
  let {
    step,
    busy = false,
    onrun,
    oncancel,
  }: {
    step: AgentStep;
    busy?: boolean;
    onrun: () => void;
    oncancel: () => void;
  } = $props();

  const a = $derived(step.action);
  const runLabel = $derived(
    a.type === 'compose-email' ? 'Open the draft' : a.type === 'reminder' ? 'Add reminder' : a.type === 'open-url' ? 'Open link' : 'Run',
  );
</script>

<section class="confirm card" role="group" aria-label="Needs your confirmation: {step.summary}">
  <header>
    <span class="ico" aria-hidden="true"><Icon name={stepIcon(a)} size={16} /></span>
    <div class="head">
      <span class="tag accent">Needs your OK</span>
      <strong>{step.summary}</strong>
    </div>
  </header>

  {#if a.type === 'compose-email'}
    <dl class="fields">
      <div><dt>Mail app</dt><dd>{PROVIDER_LABEL[a.provider]}</dd></div>
      <div>
        <dt>To</dt>
        <dd>
          {#if a.to.length}{a.to.join(', ')}{:else}<span class="faint">You choose in your mail app</span>{/if}
        </dd>
      </div>
      <div>
        <dt>Subject</dt>
        <dd>{#if a.subject}{a.subject}{:else}<span class="faint">(none)</span>{/if}</dd>
      </div>
      <div>
        <dt>Message</dt>
        <dd class="msg">{#if a.body}{a.body}{:else}<span class="faint">(empty)</span>{/if}</dd>
      </div>
    </dl>
    <p class="note">
      <Icon name="shield" size={13} />
      Krovi only opens a draft in {PROVIDER_LABEL[a.provider]}. It never sends email or reads your mailbox: you review it and press Send yourself.
    </p>
  {:else if a.type === 'open-url'}
    <dl class="fields">
      <div><dt>Address</dt><dd class="mono">{a.url}</dd></div>
    </dl>
    <p class="note"><Icon name="info" size={13} /> This is not one of Krovi's known sites. Check the address before you open it.</p>
  {:else if a.type === 'reminder'}
    <dl class="fields">
      <div><dt>Reminder</dt><dd>{a.title}</dd></div>
      <div><dt>Date</dt><dd>{a.date}</dd></div>
      <div><dt>Time</dt><dd>{a.time} <span class="faint">on this PC's clock</span></dd></div>
    </dl>
  {/if}

  <footer>
    <button class="btn" type="button" onclick={oncancel} disabled={busy}>Cancel</button>
    <button class="btn primary" type="button" onclick={onrun} disabled={busy}>{busy ? 'Working...' : runLabel}</button>
  </footer>
</section>

<style>
  .confirm {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
    padding: 14px 16px;
    border-radius: var(--r-4);
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  }
  header {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }
  .ico {
    display: grid;
    place-items: center;
    flex: none;
    width: 34px;
    height: 34px;
    border-radius: 10px;
    background: var(--surface-1);
    border: 1px solid var(--line);
    color: var(--text-2);
  }
  .head {
    display: grid;
    gap: 6px;
    min-width: 0;
  }
  .head strong {
    font-size: var(--t-md);
    font-weight: 600;
    line-height: 1.4;
    letter-spacing: -0.01em;
    overflow-wrap: anywhere;
  }
  .head .tag {
    justify-self: start;
  }
  .fields {
    display: grid;
    gap: 6px;
    padding: 10px 12px;
    border-radius: var(--r-3);
    background: var(--surface-1);
    border: 1px solid var(--line);
  }
  .fields > div {
    display: grid;
    grid-template-columns: 78px 1fr;
    gap: 10px;
    align-items: start;
  }
  dt {
    color: var(--text-3);
    font-size: var(--t-xs);
    font-weight: 600;
    padding-top: 1px;
  }
  dd {
    margin: 0;
    font-size: var(--t-sm);
    overflow-wrap: anywhere;
    user-select: text;
  }
  .mono {
    font-family: var(--font-mono);
    font-size: var(--t-xs);
  }
  .msg {
    white-space: pre-wrap;
  }
  .note {
    display: flex;
    gap: 7px;
    align-items: flex-start;
    font-size: var(--t-xs);
    color: var(--text-2);
    line-height: 1.45;
  }
  .note :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
