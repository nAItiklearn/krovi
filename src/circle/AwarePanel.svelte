<script lang="ts">
  import { onDestroy, onMount, tick } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import Segmented from '../components/Segmented.svelte';
  import Toggle from '../components/Toggle.svelte';
  import TextActions from './TextActions.svelte';
  import { copyText, pushToast } from '../lib/stores';
  import * as ci from '../lib/circle';
  import { awareState, circleStatus } from '../lib/circle';

  // Screen awareness: a session YOU start, so you can ask a question about what is on your screen.
  //   * Krovi does not look at your screen otherwise. Starting a session captures nothing.
  //   * The screen is read only at the moment you press "Ask", and only the monitor Krovi's pill is on.
  //   * A session ends when you press Stop or after the time you chose (never more than 30 minutes).
  //     While it runs, the pill shows an "ON" indicator (the `aware-state` event drives it).
  //   * Nothing is saved, and nothing is sent anywhere unless you turn on the picture option, which
  //     needs a key from Settings → AI and names the provider before you start.
  let { onclose }: { onclose?: () => void } = $props();

  const DURATIONS = [5, 10, 20, 30];
  const options = DURATIONS.map((m) => ({ id: String(m), label: `${m} min` }));

  let now = $state(Date.now());
  const timer = setInterval(() => (now = Date.now()), 1000);
  onDestroy(() => clearInterval(timer));

  onMount(() => {
    void ci.wireCircle();
    void ci.loadCircle().catch(() => undefined);
    void ci.awareStatus().catch(() => undefined);
  });

  const st = $derived($awareState);
  const status = $derived($circleStatus);
  const left = $derived(ci.secsLeft(st.endsAt, now));
  const canPicture = $derived(!!status?.activeProvider);

  let minutes = $state('10');
  let pictureOn = $state(false); // agreement to send pictures during the session
  let starting = $state(false);
  let startError = $state<ci.AiFailure | null>(null);

  let question = $state('');
  let includePicture = $state(false);
  let asking = $state(false);
  let reply = $state<ci.AwareReply | null>(null);
  let askError = $state<ci.AiFailure | null>(null);
  let questionEl: HTMLInputElement | undefined = $state();

  const matches = $derived(reply && !reply.answer ? ci.matchingLines(question, reply.read.lines) : []);
  const emptyNote = $derived(
    !reply ? '' : reply.read.ocrError ? reply.read.ocrError : reply.read.blank ? 'The screen looked like one flat colour. Protected video can’t be captured.' : 'No text was found on the screen.',
  );

  // a session that ended (time or Stop) clears what was shown from it
  let wasActive = false;
  $effect(() => {
    if (wasActive && !st.active) {
      reply = null;
      askError = null;
      includePicture = false;
    }
    wasActive = st.active;
  });

  async function start() {
    starting = true;
    startError = null;
    try {
      await ci.awareStart(Number(minutes), pictureOn);
      await tick();
      questionEl?.focus();
    } catch (e) {
      startError = ci.toFailure(e);
    } finally {
      starting = false;
    }
  }

  async function stop() {
    reply = null;
    await ci.awareStop();
  }

  async function ask() {
    if (asking || !st.active) return;
    asking = true;
    askError = null;
    reply = null;
    try {
      reply = await ci.awareAsk(question.trim(), includePicture && st.vision);
    } catch (e) {
      askError = ci.toFailure(e);
    } finally {
      asking = false;
    }
  }

  function onQuestionKey(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void ask();
    }
  }

  async function copyAnswer() {
    if (!reply?.answer) return;
    const ok = await copyText(reply.answer.answer);
    pushToast(ok ? 'Copied' : 'Couldn’t copy that', ok ? 1400 : 2600, ok ? undefined : 'error');
  }

  const endedNote = $derived(!st.active && st.reason === 'expired' ? 'The session ended because its time was up.' : !st.active && st.reason === 'stopped' ? 'The session is stopped.' : '');
</script>

<section class="aware card" aria-label="Screen awareness">
  <header class="head">
    <h2 class="eyebrow">Screen awareness</h2>
    {#if st.active}
      <span class="tag live" role="status"><span class="pulse" aria-hidden="true"></span> ON &middot; {ci.formatRemaining(left)} left</span>
    {:else}
      <span class="tag">Off</span>
    {/if}
    <span class="grow"></span>
    {#if onclose}
      <button class="btn sm icon ghost" onclick={onclose} aria-label="Close" title="Close"><Icon name="close" size={14} /></button>
    {/if}
  </header>

  {#if !st.active}
    <p class="lede">
      Krovi doesn’t look at your screen. Start a session and you can ask a question about what is on it. It is read only when you press
      <b>Ask</b>, for at most {st.maxMinutes} minutes. Screenshots are never saved; text you choose to Copy goes to your clipboard history like any copy.
    </p>

    <div class="field">
      <span class="lab" id="aw-len">Session length</span>
      <Segmented label="Session length" size="sm" {options} bind:value={minutes} />
    </div>

    <div class="field">
      <div class="pic">
        <Toggle bind:checked={pictureOn} label="Allow sending a picture of my screen to an AI provider" disabled={!canPicture} />
        <span class="pictext">
          <b>Also allow sending a picture of my screen to an AI provider</b>
          {#if canPicture && status?.disclosure}
            <small>{status.disclosure.screen}</small>
          {:else}
            <small class="why"><Icon name="lock" size={11} /> {ci.NO_PROVIDER_TEXT}</small>
          {/if}
        </span>
      </div>
    </div>

    {#if endedNote}<p class="faint small" role="status">{endedNote}</p>{/if}
    {#if startError}
      <div class="problem" role="alert"><Icon name="alert" size={14} /> <span>{startError.message}</span></div>
    {/if}

    <div class="row">
      <button class="btn primary" onclick={start} disabled={starting}>
        <Icon name={starting ? 'loader' : 'eye'} size={14} /> Start session
      </button>
      <span class="faint small">Starting captures nothing.</span>
    </div>
  {:else}
    <div class="banner" role="status">
      <Icon name="eye" size={15} />
      <span>
        <b>Screen awareness is ON</b>
        until {new Date(st.endsAt ?? now).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}.
        {#if st.vision && st.providerName}Pictures may be sent to {st.providerName} when you choose.{:else}Nothing leaves this PC.{/if}
      </span>
      <button class="btn sm" onclick={stop}><Icon name="square" size={12} /> Stop</button>
    </div>

    <label class="sr-only" for="aw-q">Your question about the screen</label>
    <div class="askrow">
      <input
        id="aw-q"
        class="input"
        bind:this={questionEl}
        bind:value={question}
        placeholder="Ask about what is on your screen…"
        maxlength="2000"
        onkeydown={onQuestionKey}
        disabled={asking} />
      <button class="btn primary" onclick={ask} disabled={asking}>
        <Icon name={asking ? 'loader' : 'search'} size={14} /> {asking ? 'Reading…' : 'Ask'}
      </button>
    </div>

    {#if st.vision}
      <label class="check">
        <input type="checkbox" bind:checked={includePicture} />
        <span>Include a picture of my screen <small>({status?.disclosure?.screen ?? `Sends a picture to ${st.providerName ?? 'your AI provider'}.`})</small></span>
      </label>
    {/if}

    {#if askError}
      <div class="problem" role="alert"><Icon name="alert" size={14} /> <span>{askError.message}{#if ci.failureAdvice(askError)} <small>{ci.failureAdvice(askError)}</small>{/if}</span></div>
    {/if}

    {#if reply}
      {#if reply.answer}
        <div class="answer" aria-live="polite">
          <div class="ahead">
            <h3 class="eyebrow">{reply.answer.providerName} &middot; {reply.answer.model}</h3>
            <span class="grow"></span>
            <button class="btn sm icon ghost" onclick={copyAnswer} aria-label="Copy the answer" title="Copy"><Icon name="copy" size={14} /></button>
          </div>
          <p>{reply.answer.answer}</p>
          {#if reply.answer.truncated}<p class="faint small"><Icon name="info" size={12} /> The answer hit the length limit and was cut off.</p>{/if}
        </div>
      {/if}
      {#if reply.aiError}
        <div class="problem" role="alert">
          <Icon name="alert" size={14} />
          <span>{reply.aiError.message}{#if ci.failureAdvice(reply.aiError)} <small>{ci.failureAdvice(reply.aiError)}</small>{/if}</span>
        </div>
      {/if}
      {#if !reply.read.ok}
        <div class="problem" role="alert"><Icon name="alert" size={14} /> <span>{reply.read.error ?? 'The screen could not be read.'}</span></div>
      {:else}
        <h3 class="eyebrow sect">Text on the screen{reply.read.language ? ` · ${reply.read.language}` : ''}</h3>
        <TextActions text={reply.read.text} lines={matches} {emptyNote} />
      {/if}
    {/if}
  {/if}
</section>

<style>
  .aware {
    display: grid;
    gap: 12px;
    padding: 14px 16px 14px;
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
  .lede {
    margin: 0;
    max-width: 62ch;
    color: var(--text-2);
    font-size: var(--t-sm);
    line-height: 1.55;
  }
  .small {
    font-size: var(--t-xs);
  }
  .field {
    display: grid;
    gap: 6px;
    justify-items: start;
  }
  .lab {
    font-size: var(--t-xs);
    color: var(--text-2);
    font-weight: 600;
  }
  .pic {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }
  .pictext {
    display: grid;
    gap: 2px;
    font-size: var(--t-sm);
  }
  .pictext small {
    color: var(--text-2);
    font-size: var(--t-xs);
    line-height: 1.45;
  }
  .why {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .row,
  .askrow {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .askrow .input {
    flex: 1;
  }
  .banner {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 12px;
    border-radius: var(--r-3);
    background: var(--accent-soft);
    border: 1px solid color-mix(in srgb, var(--accent) 45%, transparent);
    font-size: var(--t-sm);
    line-height: 1.4;
  }
  .banner span {
    flex: 1;
  }
  .tag.live {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .pulse {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 0 0 var(--accent-glow);
    animation: aw-pulse 1.8s var(--ease-out) infinite;
  }
  @keyframes aw-pulse {
    70% {
      box-shadow: 0 0 0 7px transparent;
    }
    100% {
      box-shadow: 0 0 0 0 transparent;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .pulse {
      animation: none;
    }
  }
  .check {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    font-size: var(--t-sm);
    line-height: 1.45;
    cursor: pointer;
  }
  .check input {
    margin-top: 3px;
  }
  .check small {
    color: var(--text-2);
  }
  .sect {
    margin: 2px 0 -4px;
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
</style>
