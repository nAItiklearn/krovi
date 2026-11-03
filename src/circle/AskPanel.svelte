<script lang="ts">
  import { onDestroy, onMount, tick } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import TextActions from './TextActions.svelte';
  import { copyText, pushToast } from '../lib/stores';
  import * as ci from '../lib/circle';
  import { circleHeld, circleStatus } from '../lib/circle';

  // Circle to Ask: what Krovi read inside the area you circled, and what you can do with it.
  //   * Reading happens on this PC (Windows OCR). The picture never reaches this window.
  //   * Copy / Summarise stay on this PC. Search Google opens your browser, only when you click it.
  //   * Ask AI is the one thing that sends the picture anywhere, and only after you have read the
  //     sentence naming the provider and pressed its button. It needs a key from Settings → AI.
  //
  // Mount it where the panel shows a circled result. By default it shows the latest `circle-result`
  // (store `circleHeld`); pass `result` to show a specific one (the browser demo does).
  let {
    result = null,
    onclose,
  }: {
    result?: ci.ReadResult | null;
    onclose?: () => void;
  } = $props();

  const mountedAt = Date.now();
  const held: ci.Held | null = $derived(result ? { result, receivedAt: mountedAt } : $circleHeld);
  const r: ci.ReadResult | null = $derived(held?.result ?? null);

  // a clock for "how long is the picture still held"
  let now = $state(Date.now());
  const timer = setInterval(() => (now = Date.now()), 1000);
  onDestroy(() => {
    clearInterval(timer);
    // leaving the panel drops the picture; the text can stay
    void ci.dropPicture();
  });

  onMount(() => {
    void ci.loadCircle().catch(() => undefined);
  });

  const status = $derived($circleStatus);
  const avail = $derived(ci.askAvailability(status, held, now));
  const left = $derived(ci.heldSecsLeft(held, now));

  let askOpen = $state(false);
  let question = $state('');
  let chosen = $state<ci.ProviderId | null>(null);
  let busy = $state(false);
  let reply = $state<ci.AiReply | null>(null);
  let failure = $state<ci.AiFailure | null>(null);
  let questionEl: HTMLInputElement | undefined = $state();

  const keyed = $derived((status?.providers ?? []).filter((p) => p.hasKey));
  const provider = $derived(chosen && keyed.some((p) => p.id === chosen) ? chosen : (status?.activeProvider ?? null));
  const disclosure = $derived(status?.disclosures.find((d) => d.provider === provider) ?? status?.disclosure ?? null);

  const emptyNote = $derived(
    !r
      ? ''
      : r.ocrError
        ? r.ocrError
        : r.blank
          ? 'This area is one flat colour, so there is nothing to read. Protected video can’t be captured.'
          : 'No text was found in this area.',
  );

  async function openAsk() {
    askOpen = true;
    reply = null;
    failure = null;
    await tick();
    questionEl?.focus();
  }

  async function send() {
    if (!avail.enabled || !disclosure || busy) return;
    busy = true;
    failure = null;
    reply = null;
    try {
      // Pressing this button is the confirmation of the sentence shown above it.
      reply = await ci.askVision(question.trim(), { confirm: true, provider });
    } catch (e) {
      failure = ci.toFailure(e);
    } finally {
      busy = false;
    }
  }

  function onQuestionKey(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void send();
    }
  }

  async function copyAnswer() {
    if (!reply) return;
    const ok = await copyText(reply.answer);
    pushToast(ok ? 'Copied' : 'Couldn’t copy that', ok ? 1400 : 2600, ok ? undefined : 'error');
  }

  async function discard() {
    askOpen = false;
    reply = null;
    await ci.dropPicture();
  }

  function close() {
    void ci.discardHeld();
    onclose?.();
  }
</script>

<section class="ask card" aria-label="Circle to Ask">
  <header class="head">
    <h2 class="eyebrow">Circle to Ask</h2>
    {#if r?.demo}<span class="tag accent">Demo</span>{/if}
    {#if r?.ok && r.language}<span class="tag" title="The language used to read the text">{r.language}</span>{/if}
    {#if r?.ok && r.width}<span class="tag" title="Size of the area, in pixels">{r.width} × {r.height}</span>{/if}
    <span class="grow"></span>
    {#if onclose}
      <button class="btn sm icon ghost" onclick={close} aria-label="Close" title="Close"><Icon name="close" size={14} /></button>
    {/if}
  </header>

  {#if !r}
    <p class="empty faint">Nothing circled yet. Press <kbd>{status?.settings.shortcut ?? 'Ctrl+Alt+C'}</kbd> and drag around anything on your screen.</p>
  {:else if !r.ok}
    <div class="problem" role="alert"><Icon name="alert" size={14} /> <span>{r.error ?? 'That area could not be read.'}</span></div>
  {:else}
    {#if r.demo}
      <p class="note faint"><Icon name="info" size={12} /> This is a sample. In the desktop app Krovi reads the text in the area you circle, on your PC.</p>
    {/if}

    <TextActions text={r.text} {emptyNote}>
      {#snippet extra()}
        <button
          class="btn sm primary"
          onclick={openAsk}
          disabled={!avail.enabled || askOpen}
          aria-describedby="ask-why"
          title={avail.enabled ? 'Ask an AI about the picture (you will see what is sent first)' : (avail.reason ?? '')}>
          <Icon name="sparkle" size={13} /> Ask AI
        </button>
      {/snippet}
    </TextActions>

    {#if !avail.enabled}
      <p id="ask-why" class="why"><Icon name="lock" size={12} /> {avail.reason}</p>
    {:else}
      <p id="ask-why" class="sr-only">Ask AI sends the selected picture to {disclosure?.providerName ?? 'your AI provider'} after you confirm.</p>
    {/if}

    {#if askOpen && avail.enabled}
      <div class="askbox" role="group" aria-label="Ask an AI about this image">
        {#if keyed.length > 1}
          <div class="chips" role="radiogroup" aria-label="AI provider">
            {#each keyed as p (p.id)}
              <button class="chip" class:on={p.id === provider} role="radio" aria-checked={p.id === provider} onclick={() => (chosen = p.id)}>{p.name}</button>
            {/each}
          </div>
        {/if}

        <label class="sr-only" for="ca-q">Your question</label>
        <input
          id="ca-q"
          class="input"
          bind:this={questionEl}
          bind:value={question}
          placeholder="What is this?  (or ask anything about it)"
          maxlength="2000"
          onkeydown={onQuestionKey}
          disabled={busy} />

        {#if disclosure}
          <p class="disclose" role="note"><Icon name="upload" size={13} /> <b>{disclosure.image}</b> <span class="faint">Model: {disclosure.model}. Nothing is sent until you press the button.</span></p>
        {/if}

        <div class="row">
          <button class="btn primary" onclick={send} disabled={busy || !disclosure}>
            <Icon name={busy ? 'loader' : 'upload'} size={14} /> {busy ? 'Asking…' : `Send to ${disclosure?.providerName ?? 'AI'}`}
          </button>
          <button class="btn ghost" onclick={() => (askOpen = false)} disabled={busy}>Cancel</button>
          <span class="grow"></span>
          <span class="faint small" title="The picture is dropped from memory after this">Picture held {ci.formatRemaining(left)} more</span>
        </div>

        {#if failure}
          <div class="problem" role="alert">
            <Icon name="alert" size={14} />
            <span>
              {failure.message}
              {#if ci.failureAdvice(failure)}<br /><small>{ci.failureAdvice(failure)}</small>{/if}
              {#if failure.detail}<br /><small class="detail">{failure.detail}</small>{/if}
            </span>
          </div>
        {/if}

        {#if reply}
          <div class="answer" aria-live="polite">
            <div class="ahead">
              <h3 class="eyebrow">{reply.providerName} &middot; {reply.model}</h3>
              <span class="grow"></span>
              <button class="btn sm icon ghost" onclick={copyAnswer} aria-label="Copy the answer" title="Copy"><Icon name="copy" size={14} /></button>
            </div>
            <p>{reply.answer}</p>
            {#if reply.truncated}<p class="faint small"><Icon name="info" size={12} /> The answer hit the length limit and was cut off.</p>{/if}
          </div>
        {/if}
      </div>
    {/if}

    <footer class="foot faint">
      <Icon name="shield" size={12} />
      <span>Read on this PC. {left > 0 ? `The picture is kept in memory for ${ci.formatRemaining(left)} more, then dropped. Never saved to disk.` : 'The picture is no longer held.'}</span>
      <span class="grow"></span>
      {#if left > 0}<button class="linkbtn" onclick={discard}>Drop the picture now</button>{/if}
    </footer>
  {/if}
</section>

<style>
  .ask {
    display: grid;
    gap: 10px;
    padding: 14px 16px 12px;
    border-radius: var(--r-4);
    min-height: 0;
    overflow-y: auto;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .head .eyebrow {
    margin: 0;
  }
  .grow {
    flex: 1;
  }
  .empty,
  .note {
    margin: 0;
    font-size: var(--t-sm);
    line-height: 1.5;
  }
  .note {
    display: flex;
    gap: 6px;
    align-items: flex-start;
    font-size: var(--t-xs);
  }
  .why {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: -2px 0 0;
    font-size: var(--t-xs);
    color: var(--text-2);
  }
  .askbox {
    display: grid;
    gap: 10px;
    padding: 12px;
    border-radius: var(--r-3);
    background: var(--surface-1);
    border: 1px solid var(--line);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .disclose {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin: 0;
    padding: 8px 10px;
    border-radius: var(--r-2);
    background: var(--accent-soft);
    font-size: var(--t-sm);
    line-height: 1.45;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .small {
    font-size: var(--t-xs);
  }
  .problem {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 8px 12px;
    border-radius: var(--r-2);
    border: 1px solid color-mix(in srgb, var(--danger) 40%, transparent);
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    color: var(--danger);
    font-size: var(--t-sm);
    font-weight: 560;
    line-height: 1.45;
  }
  .problem small {
    font-weight: 450;
  }
  .detail {
    opacity: 0.85;
    user-select: text;
  }
  .answer {
    display: grid;
    gap: 4px;
    padding: 10px 12px;
    border-radius: var(--r-2);
    background: var(--surface-2);
  }
  .ahead {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .ahead .eyebrow {
    margin: 0;
  }
  .answer p {
    margin: 0;
    font-size: var(--t-sm);
    line-height: 1.6;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--t-xs);
    line-height: 1.4;
  }
  .linkbtn {
    color: var(--text-2);
    text-decoration: underline;
    font-size: var(--t-xs);
  }
  .linkbtn:hover {
    color: var(--text);
  }
</style>
