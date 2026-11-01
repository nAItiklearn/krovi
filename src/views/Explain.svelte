<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { fade, fly } from 'svelte/transition';
  import Icon from '../components/Icon.svelte';
  import Segmented from '../components/Segmented.svelte';
  import State from '../components/State.svelte';
  import Toggle from '../components/Toggle.svelte';
  import { clips } from '../lib/live';
  import { addNote, copyText, pushToast } from '../lib/stores';
  import { swap } from '../lib/motion';
  import * as ex from '../lib/explain';
  import { speaking } from '../lib/explain';

  // Explain: paste any long or confusing text, get a short summary, have it read aloud.
  //   * The summary is worked out on this PC. Nothing you paste leaves it.
  //   * Reading aloud uses a Windows voice (also on this PC) unless you opt in to ElevenLabs with
  //     your own key; that one is labelled "Sends text to ElevenLabs" everywhere it appears.

  const PLACEHOLDER = 'Paste a long paragraph, an article, terms and conditions, an email, or a phrase you want shortened...';
  const SAMPLE = 'Hello from Krovi. This is the voice that will read your summary.';

  let text = $state('');
  let length = $state<ex.SummaryLength>('medium');
  let summary = $state<ex.Summary | null>(null);
  let summarizedText = $state('');
  let busy = $state(false);
  let showSkeleton = $state(false);
  let error = $state<string | null>(null);
  let announce = $state('');

  let engines = $state<ex.Engines | null>(null);
  let settings = $state<ex.ExplainSettings>({ ...ex.DEFAULT_SETTINGS });
  let enginesError = $state<string | null>(null);

  let drawer = $state(false);
  let consentOpen = $state(false);
  let estimate = $state<{ sentences: number; chars: number; credits: number } | null>(null);

  // ElevenLabs panel
  let keyInput = $state('');
  let replacing = $state(false);
  let keyBusy = $state(false);
  let keyMsg = $state<{ tone: 'ok' | 'warning' | 'error'; text: string } | null>(null);
  let elevenVoices = $state<ex.ElevenVoice[]>([]);
  let testBusy = $state(false);
  let testMsg = $state<{ ok: boolean; text: string } | null>(null);
  let previewBusy = $state(false);

  let gearBtn: HTMLButtonElement | undefined = $state();
  let closeBtn: HTMLButtonElement | undefined = $state();
  let drawerEl: HTMLElement | undefined = $state();
  let markEl: HTMLElement | undefined = $state();
  let textEl: HTMLTextAreaElement | undefined = $state();

  const reducedMotion = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  const dur = reducedMotion ? 0 : 220;

  const counts = $derived(ex.countText(text));
  const readEst = $derived(counts.words > 0 ? ex.formatDuration((counts.words / 238) * 60) : '');
  const stale = $derived(summary !== null && text !== summarizedText);
  const lengthOptions = ex.LENGTHS.map((l) => ({ id: l.id, label: l.label }));

  const latestClip = $derived($clips.find((c) => c.kind === 'Text' && c.content.trim().length > 0) ?? null);

  const hasKey = $derived(engines?.elevenlabs.hasKey ?? false);
  const useEleven = $derived(settings.voiceEngine === 'elevenlabs');
  const canEleven = $derived(hasKey && !!settings.elevenVoiceId);
  const winVoices = $derived(engines?.windows.voices ?? []);
  const winVoiceName = $derived(ex.shortVoiceName(winVoices.find((v) => v.id === (settings.windowsVoice ?? engines?.windows.defaultVoiceId ?? ''))?.name ?? ''));
  const voiceLabel = $derived(
    useEleven ? `ElevenLabs · ${ex.shortVoiceName(settings.elevenVoiceName ?? 'your voice')}` : winVoiceName ? `Windows voice · ${winVoiceName}` : 'Windows voice',
  );
  const model = $derived(ex.ELEVEN_MODELS.find((m) => m.id === settings.elevenModel) ?? ex.ELEVEN_MODELS[0]);

  const isReading = $derived($speaking.status === 'speaking' && $speaking.target === 'summary');
  const parts = $derived(summary ? ex.highlightParts(summary.text, isReading ? $speaking.range : null) : null);
  const saved = $derived(summary && !summary.alreadyShort ? ex.savedLabel(summary.stats) : '');

  // ---------------------------------------------------------------- summarising

  async function run() {
    const src = text;
    if (!src.trim()) {
      error = 'Paste or type some text first.';
      return;
    }
    if (isReading) void ex.stopSpeaking();
    busy = true;
    error = null;
    const t = setTimeout(() => (showSkeleton = true), 160);
    try {
      const s = await ex.summarize(src, length);
      summary = s;
      summarizedText = src;
      ex.clearSpeakMessages();
      announce = s.alreadyShort ? 'That text is already short.' : `Summary ready: ${s.items.length} ${s.items.length === 1 ? 'sentence' : 'sentences'}.`;
    } catch (e) {
      summary = null;
      error = ex.errorText(e);
      announce = error;
    } finally {
      clearTimeout(t);
      busy = false;
      showSkeleton = false;
    }
  }

  function setText(next: string, auto = true) {
    text = next;
    if (auto && next.trim()) void run();
  }

  async function pasteClipboard() {
    try {
      const t = await navigator.clipboard.readText();
      if (t.trim()) return setText(t);
    } catch {
      /* permission refused or no clipboard access: fall back to the clip history below */
    }
    if (latestClip) {
      setText(latestClip.content);
      pushToast('Used your latest clip');
    } else {
      pushToast('Nothing to paste. Press Ctrl+V in the box instead.');
    }
  }

  function useLatestClip() {
    if (latestClip) setText(latestClip.content);
  }

  function clearAll() {
    if (isReading) void ex.stopSpeaking();
    text = '';
    summary = null;
    summarizedText = '';
    error = null;
    void tick().then(() => textEl?.focus());
  }

  function onPasteEvent(e: ClipboardEvent) {
    // Pasting into an empty box summarizes straight away; pasting into existing text just pastes.
    if (text.trim()) return;
    const pasted = e.clipboardData?.getData('text/plain') ?? '';
    if (pasted.trim().length > 0) {
      e.preventDefault();
      setText(pasted);
    }
  }

  function onTextKey(e: KeyboardEvent) {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void run();
    }
  }

  function onLength(v: ex.SummaryLength) {
    void patchSettings({ summaryLength: v });
    if (summary) void run();
  }

  async function copy() {
    if (!summary) return;
    pushToast((await copyText(summary.text)) ? 'Summary copied' : 'Could not copy');
  }

  async function saveNote() {
    if (!summary) return;
    pushToast((await addNote(ex.noteBody(summarizedText, summary.text))) ? 'Saved to Notes' : 'Could not save the note');
  }

  // ---------------------------------------------------------------- reading aloud

  async function toggleRead() {
    if (!summary) return;
    if (isReading || $speaking.status === 'speaking') {
      await ex.stopSpeaking();
      return;
    }
    if (useEleven && !settings.elevenConsented) {
      consentOpen = true;
      return;
    }
    await read(false);
  }

  async function read(confirmCloud: boolean) {
    if (!summary) return;
    try {
      await ex.speakText(summary.text, useEleven ? 'elevenlabs' : 'windows', { confirmCloud, rate: settings.rate, voiceId: settings.windowsVoice });
      if (confirmCloud) settings = { ...settings, elevenConsented: true };
    } catch (e) {
      const m = ex.errorText(e);
      if (m === 'consent-required') consentOpen = true;
      else pushToast(m, 3200, 'error');
    }
  }

  function confirmCloud() {
    consentOpen = false;
    void read(true);
  }

  async function preview() {
    previewBusy = true;
    try {
      await ex.speakText(SAMPLE, 'windows', { target: 'sample', rate: settings.rate, voiceId: settings.windowsVoice });
    } catch (e) {
      pushToast(ex.errorText(e), 3200, 'error');
    } finally {
      previewBusy = false;
    }
  }

  $effect(() => {
    const s = summary;
    const m = settings.elevenModel;
    if (!s || !useEleven) {
      estimate = null;
      return;
    }
    let live = true;
    void ex.estimateSpeech(s.text, m).then((e) => {
      if (live) estimate = e;
    });
    return () => {
      live = false;
    };
  });

  // keep the sentence being read in view
  $effect(() => {
    if (markEl) markEl.scrollIntoView({ block: 'nearest', behavior: reducedMotion ? 'auto' : 'smooth' });
  });

  // ---------------------------------------------------------------- settings drawer

  async function patchSettings(patch: Partial<ex.ExplainSettings>) {
    const next = { ...settings, ...patch };
    settings = next;
    try {
      settings = await ex.saveSettings(next);
    } catch (e) {
      pushToast(ex.errorText(e), 3200, 'error');
    }
  }

  async function openDrawer() {
    drawer = true;
    await tick();
    closeBtn?.focus();
  }

  async function closeDrawer() {
    drawer = false;
    keyInput = '';
    replacing = false;
    await tick();
    gearBtn?.focus();
  }

  function drawerKeys(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      void closeDrawer();
      return;
    }
    if (e.key !== 'Tab' || !drawerEl) return;
    const items = Array.from(drawerEl.querySelectorAll<HTMLElement>('button:not([disabled]), input:not([disabled]), select:not([disabled]), a[href]'));
    if (!items.length) return;
    const first = items[0];
    const last = items[items.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  function consentKeys(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      consentOpen = false;
    }
  }

  async function saveKey() {
    if (!keyInput.trim() || keyBusy) return;
    keyBusy = true;
    keyMsg = null;
    try {
      const r = await ex.setElevenKey(keyInput);
      keyMsg = { tone: r.tone, text: r.message };
      if (r.saved) {
        keyInput = ''; // the key is not kept in the page once it is saved
        replacing = false;
        settings = r.settings;
        if (r.voices.length) elevenVoices = r.voices;
        engines = engines ? { ...engines, elevenlabs: { ...engines.elevenlabs, hasKey: r.hasKey, keyHint: r.keyHint } } : engines;
      }
    } catch (e) {
      keyMsg = { tone: 'error', text: ex.errorText(e) };
    } finally {
      keyBusy = false;
    }
  }

  async function removeKey() {
    keyBusy = true;
    try {
      const r = await ex.setElevenKey(null);
      keyMsg = { tone: 'ok', text: r.message };
      settings = r.settings;
      elevenVoices = [];
      testMsg = null;
      replacing = false;
      engines = engines ? { ...engines, elevenlabs: { ...engines.elevenlabs, hasKey: false, keyHint: null } } : engines;
    } catch (e) {
      keyMsg = { tone: 'error', text: ex.errorText(e) };
    } finally {
      keyBusy = false;
    }
  }

  async function loadVoices() {
    keyBusy = true;
    try {
      const r = await ex.refreshElevenVoices();
      keyMsg = { tone: r.tone, text: r.message };
      if (r.voices.length) {
        elevenVoices = r.voices;
        if (!settings.elevenVoiceId) await patchSettings({ elevenVoiceId: r.voices[0].id, elevenVoiceName: r.voices[0].name });
      }
    } catch (e) {
      keyMsg = { tone: 'error', text: ex.errorText(e) };
    } finally {
      keyBusy = false;
    }
  }

  async function pickVoice(id: string) {
    const v = elevenVoices.find((x) => x.id === id);
    testMsg = null;
    await patchSettings({ elevenVoiceId: id, elevenVoiceName: v?.name ?? settings.elevenVoiceName });
  }

  async function toggleEleven(on: boolean) {
    await patchSettings({ voiceEngine: on ? 'elevenlabs' : 'windows' });
  }

  async function testVoice() {
    testBusy = true;
    testMsg = null;
    try {
      const r = await ex.testElevenVoice();
      testMsg = { ok: r.ok, text: r.message };
    } catch (e) {
      testMsg = { ok: false, text: ex.errorText(e) };
    } finally {
      testBusy = false;
    }
  }

  async function openKeys() {
    try {
      await ex.openKeysPage();
    } catch (e) {
      pushToast(ex.errorText(e), 3200, 'error');
    }
  }

  onMount(() => {
    void ex.startExplain();
    void ex
      .loadEngines()
      .then((e) => {
        engines = e;
        settings = e.settings;
        length = e.settings.summaryLength;
        const bad = e.windows.error ?? e.elevenlabs.error;
        enginesError = bad;
      })
      .catch((e) => (enginesError = ex.errorText(e)));
    textEl?.focus();
  });
</script>

<div class="view" role="presentation">
  <header class="view-head" inert={drawer || consentOpen}>
    <p class="lede">Paste something long or confusing. Krovi shortens it on this PC and can read it out loud.</p>
    <button class="btn sm" bind:this={gearBtn} onclick={openDrawer} aria-haspopup="dialog" aria-expanded={drawer}>
      <Icon name="headphones" size={14} /> Voice settings
    </button>
  </header>

  <div class="body" inert={drawer || consentOpen}>
    <!-- ============================ the paste box ============================ -->
    <section class="card pastecard" aria-label="Text to summarize">
      <label class="sr-only" for="ex-text">Text to summarize</label>
      <textarea
        id="ex-text"
        bind:this={textEl}
        bind:value={text}
        class="paste scroll"
        class:compact={summary !== null}
        placeholder={PLACEHOLDER}
        spellcheck="false"
        aria-describedby="ex-count"
        onkeydown={onTextKey}
        onpaste={onPasteEvent}></textarea>
      <div class="foot">
        <span id="ex-count" class="count faint" aria-live="off">
          {#if counts.chars}
            {ex.formatNumber(counts.chars)} characters &middot; {ex.formatNumber(counts.words)} words &middot; about {readEst} to read
          {:else}
            Nothing pasted yet
          {/if}
        </span>
        <span class="grow"></span>
        <button class="btn sm" onclick={pasteClipboard} title="Paste what is on your clipboard">
          <Icon name="clipboard" size={13} /> Paste
        </button>
        <button class="btn sm" onclick={useLatestClip} disabled={!latestClip} title={latestClip ? 'Use the newest text in your clipboard history' : 'No text in your clipboard history yet'}>
          <Icon name="copy" size={13} /> Use latest clip
        </button>
        {#if text}
          <button class="btn sm ghost" onclick={clearAll}>Clear</button>
        {/if}
        <button class="btn primary" onclick={run} disabled={!text.trim() || busy} title="Summarize (Ctrl+Enter)">
          <Icon name={busy ? 'loader' : 'sparkles'} size={14} /> Summarize
        </button>
      </div>
    </section>

    <!-- ============================ length + stats ============================ -->
    <div class="controls">
      <Segmented label="Summary length" options={lengthOptions} bind:value={length} onchange={onLength} />
      <span class="grow"></span>
      {#if summary && !summary.alreadyShort}
        <span class="tag" title="Words before and after">{ex.formatNumber(summary.stats.words)} &rarr; {ex.formatNumber(summary.stats.summaryWords)} words</span>
        {#if saved}
          <span class="tag accent"><Icon name="clock" size={11} /> {saved}</span>
        {/if}
      {/if}
    </div>

    <!-- ============================ the result ============================ -->
    <section class="card result" aria-labelledby="ex-res" aria-busy={busy}>
      <div class="rhead">
        <h2 id="ex-res" class="eyebrow">Summary</h2>
        {#if stale}
          <span class="tag" role="status">Text changed. Press Summarize to update.</span>
        {/if}
        <span class="grow"></span>
        {#if summary}
          <button class="btn sm icon ghost" onclick={copy} aria-label="Copy the summary" title="Copy"><Icon name="copy" size={14} /></button>
          <button class="btn sm" onclick={saveNote}><Icon name="notes" size={13} /> Save as note</button>
        {/if}
      </div>

      <div class="rbody scroll">
        {#if showSkeleton}
          <div class="skeletons" aria-hidden="true">
            <div class="skel" style="height:14px;width:96%"></div>
            <div class="skel" style="height:14px;width:88%"></div>
            <div class="skel" style="height:14px;width:62%"></div>
          </div>
        {:else if error}
          <div class="problem" role="alert"><Icon name="alert" size={15} /> <span>{error}</span></div>
        {:else if summary && parts}
          <div in:swap>
            <p class="out" class:points={summary.length === 'points' && !summary.alreadyShort}>{parts.before}{#if parts.mid}{#key parts.before.length}<mark bind:this={markEl}>{parts.mid}</mark>{/key}{/if}{parts.after}</p>
            {#if summary.note}
              <p class="note faint"><Icon name="info" size={13} /> {summary.note}</p>
            {/if}
          </div>
        {:else}
          <State compact icon="bookOpen" title="Your summary shows up here" text="Paste or type something above, then press Summarize. It is worked out on this PC, so nothing you paste leaves it." />
        {/if}
      </div>

      {#if summary}
        <footer class="rfoot">
          <button
            class="speak"
            class:cloud={useEleven}
            class:on={$speaking.status === 'speaking'}
            onclick={toggleRead}
            aria-label={$speaking.status === 'speaking' ? 'Stop reading aloud' : useEleven ? `Read aloud with ElevenLabs. Sends text to ElevenLabs.` : `Read aloud with ${voiceLabel}`}>
            <span class="sico"><Icon name={$speaking.status === 'speaking' ? 'square' : 'volume'} size={16} /></span>
            <span class="stack">
              <b>{$speaking.status === 'speaking' ? 'Stop' : 'Read aloud'}</b>
              {#if $speaking.status === 'speaking'}
                <small>{$speaking.voice || voiceLabel}{$speaking.count > 1 ? ` · ${Math.min($speaking.index + 1, $speaking.count)} of ${$speaking.count}` : ''}</small>
              {:else if useEleven}
                <small class="warnline"><Icon name="upload" size={11} /> Sends text to ElevenLabs{estimate ? ` · about ${ex.formatNumber(estimate.credits)} credits` : ''}</small>
              {:else}
                <small><Icon name="lock" size={11} /> {voiceLabel} &middot; stays on this PC</small>
              {/if}
            </span>
          </button>
          <div class="status" role="status">
            {#if $speaking.error}
              <span class="bad"><Icon name="alert" size={13} /> {$speaking.error}</span>
            {:else if $speaking.notice}
              <span class="warn"><Icon name="info" size={13} /> {$speaking.notice}</span>
            {/if}
          </div>
        </footer>
      {/if}
    </section>
  </div>

  <p class="sr-only" role="status" aria-live="polite">{announce}</p>

  <!-- ============================ voice settings drawer ============================ -->
  {#if drawer}
    <div class="scrim" role="presentation" onclick={closeDrawer} transition:fade={{ duration: dur }}></div>
    <div
      class="drawer"
      role="dialog"
      aria-modal="true"
      aria-labelledby="ex-dr"
      tabindex="-1"
      bind:this={drawerEl}
      onkeydown={drawerKeys}
      transition:fly={{ x: 360, duration: dur }}>
      <header class="dhead">
        <h2 id="ex-dr">Voice settings</h2>
        <button class="btn icon sm ghost" bind:this={closeBtn} onclick={closeDrawer} aria-label="Close voice settings"><Icon name="close" size={14} /></button>
      </header>

      <div class="dbody scroll">
        {#if enginesError}
          <p class="problem" role="alert"><Icon name="alert" size={14} /> <span>{enginesError}</span></p>
        {/if}

        <!-- Windows voice -->
        <section class="dsec" aria-labelledby="ex-win">
          <div class="sech">
            <h3 id="ex-win">Windows voice</h3>
            <span class="tag"><Icon name="lock" size={11} /> On this PC, no internet</span>
          </div>
          {#if engines && !engines.windows.available}
            <p class="hint faint">Windows did not report any installed voices. Add one in Windows Settings &rsaquo; Time &amp; language &rsaquo; Speech.</p>
          {:else}
            <label class="fld">
              <span>Voice</span>
              <select
                class="input"
                value={settings.windowsVoice ?? engines?.windows.defaultVoiceId ?? ''}
                onchange={(e) => patchSettings({ windowsVoice: e.currentTarget.value || null })}>
                {#each winVoices as v (v.id)}
                  <option value={v.id}>{ex.shortVoiceName(v.name)}{v.natural ? ' (natural)' : ''} &middot; {v.language}</option>
                {:else}
                  <option value="">System default</option>
                {/each}
              </select>
            </label>
          {/if}
          <label class="fld">
            <span>Speed <b class="num">{settings.rate.toFixed(1)}&times;</b></span>
            <input
              class="range"
              type="range"
              min={ex.RATE_MIN}
              max={ex.RATE_MAX}
              step="0.1"
              style:--p="{((settings.rate - ex.RATE_MIN) / (ex.RATE_MAX - ex.RATE_MIN)) * 100}%"
              value={settings.rate}
              aria-label="Speaking speed"
              oninput={(e) => (settings = { ...settings, rate: Number(e.currentTarget.value) })}
              onchange={(e) => patchSettings({ rate: Number(e.currentTarget.value) })} />
          </label>
          <button class="btn sm" onclick={preview} disabled={previewBusy || $speaking.status === 'speaking'}>
            <Icon name="volume" size={13} /> Hear this voice
          </button>
        </section>

        <!-- ElevenLabs -->
        <section class="dsec eleven" aria-labelledby="ex-el">
          <div class="sech">
            <h3 id="ex-el">Use ElevenLabs voice <span class="opt">(optional)</span></h3>
            <span title={canEleven ? '' : 'Save a key and choose a voice first'}>
              <Toggle checked={useEleven} label="Use ElevenLabs voice" disabled={!canEleven} onchange={toggleEleven} />
            </span>
          </div>
          <p class="explain">
            ElevenLabs makes more natural, human-sounding voices. It is entirely optional and works with <b>your own</b> ElevenLabs
            account: Krovi has no account, key or server of its own.
          </p>
          <p class="cloudnote">
            <span class="tag accent"><Icon name="upload" size={11} /> Sends text to ElevenLabs</span>
            <span class="faint">
              While this voice is on, the sentences being read are sent to ElevenLabs over the internet, and nothing else. With it off,
              or with no key, nothing is ever sent.
            </span>
          </p>

          <ol class="steps">
            <li>
              <div class="step"><span class="n" aria-hidden="true">1</span><b>Get a key</b></div>
              <p class="faint">Make a free ElevenLabs account, open Settings &rsaquo; API keys and create a key. The free plan includes some credits.</p>
              <button class="btn sm" onclick={openKeys}><Icon name="externalLink" size={13} /> Open elevenlabs.io</button>
            </li>

            <li>
              <div class="step"><span class="n" aria-hidden="true">2</span><b>Paste it here</b></div>
              {#if hasKey && !replacing}
                <div class="saved">
                  <Icon name="circleCheck" size={16} />
                  <span class="savedtxt">Key saved{#if engines?.elevenlabs.keyHint} <small class="faint">&middot; ends in {engines.elevenlabs.keyHint}</small>{/if}</span>
                  <span class="grow"></span>
                  <button class="btn sm ghost" onclick={() => (replacing = true)}>Replace</button>
                  <button class="btn sm danger" onclick={removeKey} disabled={keyBusy}><Icon name="trash" size={13} /> Remove key</button>
                </div>
              {:else}
                <form class="keyrow" onsubmit={(e) => { e.preventDefault(); void saveKey(); }}>
                  <label class="sr-only" for="ex-key">ElevenLabs API key</label>
                  <input
                    id="ex-key"
                    class="input"
                    type="password"
                    autocomplete="off"
                    autocapitalize="off"
                    spellcheck="false"
                    placeholder="Paste your ElevenLabs API key"
                    bind:value={keyInput} />
                  <button class="btn primary" type="submit" disabled={!keyInput.trim() || keyBusy}>
                    <Icon name={keyBusy ? 'loader' : 'check'} size={14} /> Save
                  </button>
                  {#if replacing}<button class="btn sm ghost" type="button" onclick={() => { replacing = false; keyInput = ''; }}>Cancel</button>{/if}
                </form>
              {/if}
              <p class="faint tiny"><Icon name="shield" size={12} /> Kept in Windows Credential Manager on this PC. Never in a file, and never shown again.</p>
              {#if keyMsg}
                <p class="msg {keyMsg.tone}" role="status"><Icon name={keyMsg.tone === 'ok' ? 'circleCheck' : 'alert'} size={14} /> <span>{keyMsg.text}</span></p>
              {/if}
            </li>

            {#if hasKey}
              <li>
                <div class="step"><span class="n" aria-hidden="true">3</span><b>Choose a voice and test it</b></div>
                <label class="fld">
                  <span>Voice</span>
                  <select class="input" value={settings.elevenVoiceId ?? ''} onchange={(e) => pickVoice(e.currentTarget.value)} disabled={!elevenVoices.length && !settings.elevenVoiceId}>
                    {#if elevenVoices.length}
                      {#each elevenVoices as v (v.id)}
                        <option value={v.id}>{v.name}{v.detail ? ` · ${v.detail}` : ''}</option>
                      {/each}
                    {:else if settings.elevenVoiceId}
                      <option value={settings.elevenVoiceId}>{settings.elevenVoiceName ?? 'Your saved voice'}</option>
                    {:else}
                      <option value="">Load your voices first</option>
                    {/if}
                  </select>
                </label>
                <div class="row">
                  <button class="btn sm" onclick={loadVoices} disabled={keyBusy}><Icon name="refresh" size={13} /> {elevenVoices.length ? 'Refresh voices' : 'Load my voices'}</button>
                </div>
                <label class="fld">
                  <span>Quality</span>
                  <select class="input" value={settings.elevenModel} onchange={(e) => patchSettings({ elevenModel: e.currentTarget.value })}>
                    {#each ex.ELEVEN_MODELS as m (m.id)}<option value={m.id}>{m.label}</option>{/each}
                  </select>
                </label>
                <div class="row">
                  <button class="btn" onclick={testVoice} disabled={testBusy || !settings.elevenVoiceId}>
                    <Icon name={testBusy ? 'loader' : 'volume'} size={14} /> Test voice
                  </button>
                  <span class="faint tiny">Speaks one short sentence (about {ex.creditsFor(ex.TEST_SENTENCE_LENGTH, settings.elevenModel)} credits).</span>
                </div>
                {#if testMsg}
                  <p class="msg {testMsg.ok ? 'ok' : 'error'}" role="status"><Icon name={testMsg.ok ? 'circleCheck' : 'alert'} size={14} /> <span>{testMsg.text}</span></p>
                {/if}
                <button class="btn" class:primary={!useEleven} onclick={() => toggleEleven(!useEleven)} disabled={!canEleven}>
                  <Icon name={useEleven ? 'lock' : 'headphones'} size={14} />
                  {useEleven ? 'Using ElevenLabs. Switch back to the Windows voice' : 'Use this voice for reading aloud'}
                </button>
                <p class="faint tiny">One credit is about one character ({model.creditsPerChar === 0.5 ? 'Flash uses half' : 'full price'}). Reading aloud shows the cost before it sends anything.</p>
              </li>
            {/if}
          </ol>
        </section>
      </div>
    </div>
  {/if}

  <!-- ============================ one-time cloud confirmation ============================ -->
  {#if consentOpen}
    <div class="scrim front" role="presentation" transition:fade={{ duration: dur }}></div>
    <div
      class="dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="ex-c-h"
      aria-describedby="ex-c-d"
      tabindex="-1"
      onkeydown={consentKeys}
      transition:fly={{ y: 12, duration: dur }}>
      <h2 id="ex-c-h"><Icon name="upload" size={16} /> Send this text to ElevenLabs?</h2>
      <p id="ex-c-d">
        To read with your ElevenLabs voice, Krovi will send the {ex.formatNumber(estimate?.chars ?? summary?.text.length ?? 0)} characters of this summary
        to ElevenLabs over the internet, using your key. Nothing else is sent. It uses about {ex.formatNumber(estimate?.credits ?? 0)} of your credits.
        You can switch back to the Windows voice, which stays on this PC, any time in Voice settings.
      </p>
      <div class="actions">
        <button class="btn" onclick={() => (consentOpen = false)}>Cancel</button>
        <!-- svelte-ignore a11y_autofocus -->
        <button class="btn primary" onclick={confirmCloud} autofocus>Send and read aloud</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .view-head {
    align-items: center;
  }
  .lede {
    margin: 0;
    max-width: 56ch;
    color: var(--text-2);
    font-size: var(--t-sm);
    line-height: 1.5;
  }
  .grow {
    flex: 1;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  /* ---- paste box ---- */
  .pastecard {
    flex: none;
    display: flex;
    flex-direction: column;
    padding: 10px 12px 10px 14px;
    border-radius: var(--r-4);
  }
  .pastecard:focus-within {
    border-color: var(--line-strong);
  }
  textarea.paste {
    display: block;
    width: 100%;
    height: 148px;
    padding: 4px 8px 4px 0;
    margin: 0;
    resize: none;
    border: 0;
    outline: none;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--t-md);
    line-height: 1.55;
    transition: height 0.45s var(--spring);
  }
  textarea.paste.compact {
    /* exactly three lines, so no line is ever cut in half */
    height: calc(var(--t-md) * 1.55 * 3 + 8px);
  }
  textarea.paste::placeholder {
    color: var(--text-3);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-top: 8px;
    border-top: 1px solid var(--line);
    flex-wrap: wrap;
  }
  .count {
    font-size: var(--t-xs);
    font-variant-numeric: tabular-nums;
  }

  /* ---- controls ---- */
  .controls {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
  }

  /* ---- result ---- */
  .result {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-radius: var(--r-4);
  }
  .rhead {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px 0 16px;
  }
  .rhead .eyebrow {
    margin: 0;
  }
  .rbody {
    flex: 1;
    min-height: 0;
    padding: 6px 16px 10px;
    overflow-y: auto;
  }
  .out {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-size: var(--t-md);
    line-height: 1.65;
    color: var(--text);
    user-select: text;
  }
  .out.points {
    line-height: 1.6;
    /* hanging indent: a wrapped bullet lines up under its text, not under the bullet */
    padding-left: 1.1em;
    text-indent: -1.1em;
  }
  mark {
    background: var(--accent-soft);
    color: var(--text);
    border-radius: 5px;
    box-shadow: 0 0 0 3px var(--accent-soft);
    -webkit-box-decoration-break: clone;
    box-decoration-break: clone;
    transition: background var(--dur-2);
  }
  .note {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 10px 0 0;
    font-size: var(--t-xs);
  }
  .skeletons {
    display: grid;
    gap: 10px;
    padding-top: 6px;
  }
  .problem {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 6px 0;
    padding: 9px 12px;
    border-radius: var(--r-2);
    border: 1px solid color-mix(in srgb, var(--danger) 40%, transparent);
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    color: var(--danger);
    font-size: var(--t-sm);
    font-weight: 560;
  }
  .rfoot {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px 12px 16px;
    border-top: 1px solid var(--line);
  }
  .speak {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    padding: 6px 16px 6px 8px;
    border-radius: var(--r-3);
    border: 1px solid var(--line);
    background: var(--surface-1);
    color: var(--text);
    text-align: left;
    transition:
      background var(--dur-1),
      border-color var(--dur-1),
      transform 0.4s var(--spring);
  }
  .speak:hover {
    background: var(--surface-2);
    border-color: var(--line-strong);
  }
  .speak:active {
    transform: scale(0.98);
  }
  .speak.cloud {
    border-color: color-mix(in srgb, var(--accent) 45%, var(--line));
  }
  .speak.on {
    background: var(--accent-soft);
    border-color: color-mix(in srgb, var(--accent) 55%, transparent);
  }
  .sico {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: var(--invert-bg);
    color: var(--invert-fg);
    flex: none;
  }
  .stack {
    display: grid;
    gap: 1px;
    min-width: 0;
  }
  .stack b {
    font-size: var(--t-sm);
    font-weight: 650;
  }
  .stack small {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: var(--t-micro);
    color: var(--text-2);
    max-width: 340px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .stack small.warnline {
    color: var(--accent);
    font-weight: 600;
  }
  .status {
    min-width: 0;
    font-size: var(--t-xs);
  }
  .status span {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .warn {
    color: var(--text-2);
  }
  .bad {
    color: var(--danger);
  }

  /* ---- drawer ---- */
  .scrim {
    position: absolute;
    inset: 0;
    z-index: 20;
    background: var(--scrim);
    border-radius: inherit;
  }
  .scrim.front {
    z-index: 40;
  }
  .drawer {
    position: absolute;
    z-index: 30;
    top: 8px;
    right: 8px;
    bottom: 8px;
    width: min(380px, calc(100% - 16px));
    display: flex;
    flex-direction: column;
    border-radius: var(--r-4);
    border: 1px solid var(--line-strong);
    /* --pop-bg is slightly translucent; stack it twice (and blur what is behind) so the page
       underneath never shows through the text */
    background:
      linear-gradient(var(--pop-bg), var(--pop-bg)),
      var(--pop-bg);
    -webkit-backdrop-filter: blur(20px);
    backdrop-filter: blur(20px);
    box-shadow: var(--shadow-pop);
    overflow: hidden;
  }
  .dhead {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 12px 8px 16px;
  }
  .dhead h2 {
    font-family: var(--font-display);
    font-size: var(--t-lg);
    font-weight: 650;
    letter-spacing: -0.01em;
  }
  .dbody {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 14px 16px 16px;
    display: grid;
    gap: 14px;
    align-content: start;
  }
  .dsec {
    display: grid;
    gap: 10px;
    padding: 12px 13px;
    border-radius: var(--r-3);
    border: 1px solid var(--line);
    background: var(--surface-1);
  }
  .sech {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .sech h3 {
    font-size: var(--t-md);
    font-weight: 650;
  }
  .opt {
    font-weight: 500;
    color: var(--text-3);
  }
  .fld {
    display: grid;
    gap: 5px;
    font-size: var(--t-xs);
    color: var(--text-2);
    font-weight: 600;
  }
  .fld .num {
    color: var(--text);
    margin-left: 4px;
    font-variant-numeric: tabular-nums;
  }
  .hint,
  .tiny {
    font-size: var(--t-xs);
    line-height: 1.5;
    margin: 0;
  }
  .tiny {
    display: flex;
    align-items: flex-start;
    gap: 6px;
  }
  .explain {
    margin: 0;
    font-size: var(--t-sm);
    line-height: 1.55;
    color: var(--text-2);
  }
  .cloudnote {
    display: grid;
    gap: 6px;
    justify-items: start;
    margin: 0;
    font-size: var(--t-xs);
    line-height: 1.5;
  }
  .steps {
    list-style: none;
    margin: 2px 0 0;
    padding: 0;
    display: grid;
    gap: 14px;
  }
  .steps li {
    display: grid;
    gap: 8px;
    justify-items: start;
  }
  .steps li > * {
    max-width: 100%;
  }
  .steps p {
    margin: 0;
    font-size: var(--t-xs);
    line-height: 1.5;
  }
  .step {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--t-sm);
  }
  .n {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: var(--invert-bg);
    color: var(--invert-fg);
    font-size: var(--t-micro);
    font-weight: 700;
  }
  .keyrow {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
  }
  .keyrow .input {
    flex: 1;
    min-width: 0;
  }
  .saved {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 8px 7px 11px;
    border-radius: var(--r-2);
    background: var(--surface-2);
    font-size: var(--t-sm);
    font-weight: 560;
  }
  .savedtxt {
    white-space: nowrap;
  }
  .savedtxt small {
    font-size: var(--t-xs);
    font-weight: 500;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .fld .input,
  .steps .fld {
    width: 100%;
  }
  .msg {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    width: 100%;
    margin: 0;
    padding: 8px 10px;
    border-radius: var(--r-2);
    font-size: var(--t-xs);
    line-height: 1.45;
    font-weight: 560;
    background: var(--surface-2);
    color: var(--text);
  }
  .msg :global(svg) {
    flex: none;
    margin-top: 1px;
  }
  .msg.error {
    color: var(--danger);
    background: color-mix(in srgb, var(--danger) 11%, transparent);
  }
  .msg.warning {
    color: var(--text);
    background: var(--accent-soft);
  }
  .msg.ok {
    background: var(--surface-2);
  }

  /* ---- confirmation dialog ---- */
  .dialog {
    position: absolute;
    z-index: 50;
    left: 50%;
    top: 50%;
    translate: -50% -50%;
    width: min(440px, calc(100% - 32px));
    display: grid;
    gap: 12px;
    padding: 18px 20px;
    border-radius: var(--r-4);
    border: 1px solid var(--line-strong);
    background:
      linear-gradient(var(--pop-bg), var(--pop-bg)),
      var(--pop-bg);
    -webkit-backdrop-filter: blur(20px);
    backdrop-filter: blur(20px);
    box-shadow: var(--shadow-pop);
  }
  .dialog h2 {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--t-lg);
    font-weight: 650;
  }
  .dialog p {
    margin: 0;
    font-size: var(--t-sm);
    line-height: 1.55;
    color: var(--text-2);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  @media (prefers-reduced-motion: reduce) {
    textarea.paste {
      transition: none;
    }
  }
</style>
