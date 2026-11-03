<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import Toggle from '../components/Toggle.svelte';
  import State from '../components/State.svelte';
  import AiSettings from '../components/AiSettings.svelte';
  import { getConfig, setConfig, type ShellConfig } from '../lib/shell';
  import { REGIONS } from '../lib/holidays';
  import { clearClips, clips } from '../lib/live';
  import { get } from 'svelte/store';
  import { openTool, pushToast, region, voiceHelpOpen } from '../lib/stores';
  import { mode } from '../lib/bridge';
  import pkg from '../../package.json';
  import IdleCompanion from '../components/idle/IdleCompanion.svelte';
  import { SYSTEM_DARK, SYSTEM_LIGHT, THEMES, resolvedTheme, themeById, themeChoice, type ThemeChoice } from '../lib/theme';
  import {
    IDLE_DELAY_MAX,
    IDLE_DELAY_MIN,
    IDLE_STYLES,
    PERSONALITIES,
    idleSettings,
    type IdleSettings,
  } from '../lib/idle';
  import { PRIVACY_FACTS, getVoiceSettings, setVoiceSettings, type VoiceSettings } from '../lib/voice';
  import { CONTEXT_MODES } from '../components/voiceModel';
  import { delayLabel, delayToPos, posToDelay } from '../components/settingsModel';
  import Segmented from '../components/Segmented.svelte';
  import {
    DESKTOP_ONLY,
    DESKTOP_ONLY_LONG,
    DICTATION_LANGUAGES,
    DICTATION_STYLES,
    DOWNLOAD_NOTE,
    REPLACE_MAX_SPOKEN,
    REPLACE_MAX_WRITTEN,
    VOCAB_MAX_TERM,
    addVocabularyTerm,
    cancelDownload,
    cleanReplacements,
    deleteModel,
    dictationInfo,
    downloadModel,
    formatMB,
    listInputDevices,
    loadDictation,
    meterLevel,
    progressFraction,
    removeAt,
    saveDictationSettings,
    shortLicense,
    startMicTest,
    type DictationSettings,
    type DictationStyle,
    type InputDevice,
    type MicTest,
    type Replacement,
    type TalkMode,
  } from '../lib/dictation';
  import {
    DEFAULT_SETTINGS as DEFAULT_READ_ALOUD,
    RATE_MAX,
    RATE_MIN,
    loadEngines,
    saveSettings as saveReadAloud,
    speakText,
    speaking,
    startExplain,
    stopSpeaking,
    type Engines,
    type ExplainSettings,
  } from '../lib/explain';
  import { PREVIEW_SENTENCE, clampRate, elevenStatus, rateLabel, ratePercent, selectedVoiceId, voiceOptions } from '../components/readAloudModel';

  let cfg = $state<ShellConfig | null>(null);
  let status = $state<'loading' | 'ready' | 'error'>('loading');
  let error = $state('');
  let recording = $state(false);
  let draftKeys = $state<string[]>([]);
  let confirmClear = $state(false);

  async function load() {
    status = 'loading';
    try {
      cfg = await getConfig();
      status = 'ready';
    } catch {
      status = 'error';
    }
  }
  onMount(() => {
    void load();
    void loadVoice();
  });

  /** Apply one change; roll back and explain if the shell refuses it. */
  async function patch(change: Partial<ShellConfig>) {
    if (!cfg) return;
    const prev = cfg;
    cfg = { ...cfg, ...change };
    error = '';
    try {
      cfg = await setConfig(cfg);
    } catch (e) {
      cfg = prev;
      error = e instanceof Error ? e.message : String(e);
    }
  }

  // ---------------------------------------------------------------- shortcut recorder
  const MODS: [keyof KeyboardEvent, string][] = [
    ['ctrlKey', 'Ctrl'],
    ['altKey', 'Alt'],
    ['shiftKey', 'Shift'],
    ['metaKey', 'Win'],
  ];
  const NAMED: Record<string, string> = {
    Space: 'Space',
    Enter: 'Enter',
    Tab: 'Tab',
    Backspace: 'Backspace',
    Delete: 'Delete',
    Home: 'Home',
    End: 'End',
    PageUp: 'PageUp',
    PageDown: 'PageDown',
    ArrowUp: 'Up',
    ArrowDown: 'Down',
    ArrowLeft: 'Left',
    ArrowRight: 'Right',
    Backquote: '`',
    Minus: '-',
    Equal: '=',
    BracketLeft: '[',
    BracketRight: ']',
    Backslash: '\\',
    Semicolon: ';',
    Quote: "'",
    Comma: ',',
    Period: '.',
    Slash: '/',
  };
  function keyName(code: string): string | null {
    if (/^Key[A-Z]$/.test(code)) return code.slice(3);
    if (/^Digit\d$/.test(code)) return code.slice(5);
    if (/^F\d{1,2}$/.test(code)) return code;
    return NAMED[code] ?? null;
  }
  function modsOf(e: KeyboardEvent): string[] {
    return MODS.filter(([k]) => e[k]).map(([, n]) => n);
  }

  /** One key press while recording: the modifiers held so far, and either the finished chord or why it can't be one. */
  function chordFrom(e: KeyboardEvent): { mods: string[]; chord?: string; error?: string } {
    const mods = modsOf(e);
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) return { mods };
    const key = keyName(e.code);
    if (!key) return { mods, error: 'That key can’t be used in a shortcut.' };
    if (!mods.length) return { mods, error: 'Include at least one modifier (Ctrl, Alt, Shift or Win).' };
    return { mods, chord: [...mods, key].join('+') };
  }

  function startRecording() {
    recording = true;
    draftKeys = [];
    error = '';
  }
  async function onRecordKey(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') {
      recording = false;
      return;
    }
    const c = chordFrom(e);
    draftKeys = c.mods;
    if (c.error) error = c.error;
    if (!c.chord) return;
    recording = false;
    await patch({ shortcut: c.chord });
  }

  // ---------------------------------------------------------------- dictation (local Whisper)
  const di = $derived($dictationInfo);
  const ds = $derived(di.settings);
  /** Browser demo: every control is shown but off, and says why. Nothing is recorded or recognised. */
  const dOff = $derived(!di.available);
  type DictArea = 'top' | 'agent' | 'model' | 'mic' | 'text';
  let dStatus = $state<'loading' | 'ready' | 'error'>('loading');
  let dError = $state('');
  let dErrArea = $state<DictArea>('top');
  let dRecording = $state(false);
  let dDraft = $state<string[]>([]);
  let aRecording = $state(false);
  let aDraft = $state<string[]>([]);
  let devices = $state<InputDevice[]>([]);
  let devicesLoaded = $state(false);
  let testing = $state(false);
  let micBusy = $state(false);
  let micLevel = $state(0);
  let micError = $state('');
  let micTest: MicTest | null = null;
  let confirmDelete = $state<string | null>(null);
  let confirmTimer: ReturnType<typeof setTimeout> | null = null;
  let vocabDraft = $state('');
  let rows = $state<Replacement[]>([]);
  let rowsFrom = '';

  const errText = (e: unknown) => (e instanceof Error ? e.message : String(e));

  async function loadDict() {
    dStatus = 'loading';
    try {
      await loadDictation();
      dStatus = 'ready';
      void loadDevices();
    } catch {
      dStatus = 'error';
    }
  }
  async function loadDevices() {
    try {
      devices = await listInputDevices();
    } catch (e) {
      micError = errText(e);
    } finally {
      devicesLoaded = true;
    }
  }
  onMount(() => {
    void loadDict();
    return () => {
      if (confirmTimer) clearTimeout(confirmTimer);
      void micTest?.stop(); // the microphone must never stay open after leaving Settings
    };
  });

  // The replacement rows are editable drafts; follow the saved list only when it changes underneath us.
  $effect(() => {
    const saved = JSON.stringify(ds.options.replacements);
    if (saved !== rowsFrom) {
      rowsFrom = saved;
      rows = ds.options.replacements.map((r) => ({ ...r }));
    }
  });

  /** Save the whole settings object; roll back and show the backend's own words (under `area`) if it refuses. */
  async function saveDict(next: DictationSettings, area: DictArea = 'top') {
    const prev = ds;
    dError = '';
    dErrArea = area;
    dictationInfo.update((i) => ({ ...i, settings: next }));
    try {
      await saveDictationSettings(next);
    } catch (e) {
      dictationInfo.update((i) => ({ ...i, settings: prev }));
      dError = errText(e);
    }
  }
  const setOpt = (change: Partial<DictationSettings['options']>) =>
    saveDict({ ...ds, options: { ...ds.options, ...change } }, 'text');

  function startDictRecording() {
    dRecording = true;
    dDraft = [];
    dError = '';
  }
  async function onDictKey(e: KeyboardEvent) {
    if (!dRecording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') {
      dRecording = false;
      return;
    }
    const c = chordFrom(e);
    dDraft = c.mods;
    if (c.error) {
      dError = c.error;
      dErrArea = 'top';
    }
    if (!c.chord) return;
    dRecording = false;
    await saveDict({ ...ds, shortcut: c.chord });
  }

  // The Agent shortcut: same recorder, its own key. The words go to Agent, never into an app.
  function startAgentRecording() {
    aRecording = true;
    aDraft = [];
    dError = '';
  }
  async function onAgentKey(e: KeyboardEvent) {
    if (!aRecording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') {
      aRecording = false;
      return;
    }
    const c = chordFrom(e);
    aDraft = c.mods;
    if (c.error) {
      dError = c.error;
      dErrArea = 'agent';
    }
    if (!c.chord) return;
    aRecording = false;
    await saveDict({ ...ds, agentShortcut: c.chord }, 'agent');
  }

  // models
  const installedModels = $derived(di.models.filter((m) => m.status.state === 'installed'));
  const pickable = $derived(installedModels.some((m) => m.id === ds.model) ? ds.model : (installedModels[0]?.id ?? ''));
  const modelReady = $derived(installedModels.some((m) => m.id === ds.model));
  const chosenModel = $derived(di.models.find((m) => m.id === ds.model));
  const langHint = $derived(
    chosenModel && !chosenModel.multilingual && ds.language !== 'auto' && ds.language !== 'en'
      ? 'The selected model hears English only. Pick a multilingual model for this language.'
      : 'Auto-detect listens for the language as you speak.',
  );
  const styleHint = $derived(DICTATION_STYLES.find((s) => s.id === ds.options.style)?.hint ?? '');
  function onModelKey(e: KeyboardEvent) {
    const d = e.key === 'ArrowDown' || e.key === 'ArrowRight' ? 1 : e.key === 'ArrowUp' || e.key === 'ArrowLeft' ? -1 : 0;
    if (!d || !installedModels.length || dOff) return;
    e.preventDefault();
    const i = Math.max(0, installedModels.findIndex((m) => m.id === ds.model));
    const next = installedModels[(i + d + installedModels.length) % installedModels.length];
    void saveDict({ ...ds, model: next.id }, 'model');
    requestAnimationFrame(() => document.querySelector<HTMLElement>('.pickm[aria-checked="true"]')?.focus());
  }
  function askDelete(id: string) {
    if (confirmDelete !== id) {
      confirmDelete = id;
      if (confirmTimer) clearTimeout(confirmTimer);
      confirmTimer = setTimeout(() => (confirmDelete = null), 2800);
      return;
    }
    confirmDelete = null;
    deleteModel(id).catch((e) => {
      dError = errText(e);
      dErrArea = 'model';
    });
  }

  // microphone
  const deviceMissing = $derived(devicesLoaded && ds.device !== null && !devices.some((d) => d.id === ds.device));
  async function stopTest() {
    const t = micTest;
    micTest = null;
    testing = false;
    micLevel = 0;
    await t?.stop();
  }
  async function toggleTest() {
    if (testing) return stopTest();
    micError = '';
    micBusy = true;
    try {
      const t = await startMicTest(
        ds.device,
        (rms) => (micLevel = meterLevel(rms)),
        () => {
          micTest = null;
          testing = false;
          micLevel = 0;
        },
      );
      micTest = t;
      testing = true;
    } catch (e) {
      micError = errText(e);
    } finally {
      micBusy = false;
    }
  }
  async function pickDevice(id: string) {
    await stopTest();
    await saveDict({ ...ds, device: id || null }, 'mic');
  }

  // vocabulary and replacements
  function addTerm() {
    const next = addVocabularyTerm(ds.vocabulary, vocabDraft);
    vocabDraft = '';
    if (next !== ds.vocabulary) void saveDict({ ...ds, vocabulary: next }, 'text');
  }
  function commitRows() {
    const clean = cleanReplacements(rows);
    const json = JSON.stringify(clean);
    if (json === JSON.stringify(ds.options.replacements)) return; // only a blank draft row changed
    rowsFrom = json; // our own save: keep the draft rows the user is still typing into
    void setOpt({ replacements: clean });
  }
  function dropRow(i: number) {
    rows = removeAt(rows, i);
    commitRows();
  }

  // ---------------------------------------------------------------- read aloud
  // The same voice, speed and ElevenLabs settings the Explain tool reads with (its explain_* commands);
  // this is just a second place to change them. A Windows voice runs on this PC. ElevenLabs is optional,
  // is set up in Explain, and is only mentioned here.
  let raStatus = $state<'loading' | 'ready' | 'error'>('loading');
  let raEngines = $state<Engines | null>(null);
  /** What the controls show (the speed follows the slider while it is dragged). */
  let raSettings = $state<ExplainSettings>({ ...DEFAULT_READ_ALOUD });
  /** The last settings the backend confirmed, to go back to if a save is refused. */
  let raSaved = $state<ExplainSettings>({ ...DEFAULT_READ_ALOUD });
  let raError = $state('');
  let raBusy = $state(false);
  let raPreviewing = $state(false);

  const raOptions = $derived(voiceOptions(raEngines?.windows.voices ?? []));
  const raVoice = $derived(selectedVoiceId(raSettings.windowsVoice, raEngines?.windows));
  const raHasVoices = $derived(raEngines?.windows.available === true);
  const raEleven = $derived(elevenStatus(raEngines?.elevenlabs, raSaved));
  const raSpeaking = $derived($speaking.status === 'speaking');

  async function loadReadAloud() {
    raStatus = 'loading';
    try {
      void startExplain(); // so Stop and the speaking state follow what is really playing
      const e = await loadEngines();
      raEngines = e;
      raSettings = e.settings;
      raSaved = e.settings;
      raStatus = 'ready';
    } catch {
      raStatus = 'error';
    }
  }
  onMount(() => {
    void loadReadAloud();
    return () => {
      // a preview must not keep talking after you leave Settings
      if (get(speaking).target === 'sample') void stopSpeaking().catch(() => {});
    };
  });

  /** Save one change; go back to the last saved settings and show the reason if it is refused. */
  async function patchReadAloud(change: Partial<ExplainSettings>) {
    const next = { ...raSettings, ...change };
    raSettings = next;
    raError = '';
    raBusy = true;
    try {
      raSaved = raSettings = await saveReadAloud(next);
    } catch (e) {
      raSettings = raSaved;
      raError = errText(e);
    } finally {
      raBusy = false;
    }
  }
  async function previewVoice() {
    raError = '';
    raPreviewing = true;
    try {
      await speakText(PREVIEW_SENTENCE, 'windows', { target: 'sample', rate: raSaved.rate, voiceId: raSaved.windowsVoice });
    } catch (e) {
      raError = errText(e);
    } finally {
      raPreviewing = false;
    }
  }
  async function stopPreview() {
    try {
      await stopSpeaking();
    } catch (e) {
      raError = errText(e);
    }
  }

  // ---------------------------------------------------------------- theme picker
  const darkTheme = themeById(SYSTEM_DARK);
  const lightTheme = themeById(SYSTEM_LIGHT);
  const themeOptions: ThemeChoice[] = ['system', ...THEMES.map((t) => t.id)];
  const choice = $derived($themeChoice);
  const paintedName = $derived(themeById($resolvedTheme).name);

  function onThemeKey(e: KeyboardEvent) {
    const d = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 1 : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 0;
    if (!d) return;
    e.preventDefault();
    const i = themeOptions.indexOf(choice);
    themeChoice.set(themeOptions[(i + d + themeOptions.length) % themeOptions.length]);
    requestAnimationFrame(() => document.querySelector<HTMLElement>('.sw[aria-checked="true"]')?.focus());
  }

  // ---------------------------------------------------------------- idle companion
  const setIdle = (change: Partial<IdleSettings>) => idleSettings.update((s) => ({ ...s, ...change }));
  const idleOn = $derived($idleSettings.enabled);
  // 0..1000 slider position; a square-root curve keeps the everyday seconds easy to hit
  const delayPos = $derived(delayToPos($idleSettings.delayMs, IDLE_DELAY_MIN, IDLE_DELAY_MAX));

  // ---------------------------------------------------------------- voice & privacy
  let vs = $state<VoiceSettings | null>(null);
  let vStatus = $state<'loading' | 'ready' | 'error'>('loading');
  let vError = $state('');
  function onModeKey(e: KeyboardEvent) {
    const d = e.key === 'ArrowDown' || e.key === 'ArrowRight' ? 1 : e.key === 'ArrowUp' || e.key === 'ArrowLeft' ? -1 : 0;
    if (!d || !vs) return;
    e.preventDefault();
    const i = CONTEXT_MODES.findIndex((m) => m.id === vs?.contextCapture);
    const next = CONTEXT_MODES[(i + d + CONTEXT_MODES.length) % CONTEXT_MODES.length];
    void patchVoice({ contextCapture: next.id });
    requestAnimationFrame(() => document.querySelector<HTMLElement>('.mode[aria-checked="true"]')?.focus());
  }

  async function loadVoice() {
    vStatus = 'loading';
    try {
      vs = await getVoiceSettings();
      vStatus = 'ready';
    } catch {
      vStatus = 'error';
    }
  }
  /** Apply one change; roll back and explain if the backend refuses it. */
  async function patchVoice(change: Parameters<typeof setVoiceSettings>[0]) {
    if (!vs) return;
    const prev = vs;
    vs = { ...vs, ...change };
    vError = '';
    try {
      vs = await setVoiceSettings(change);
    } catch (e) {
      vs = prev;
      vError = e instanceof Error ? e.message : String(e);
    }
  }

  async function doClear() {
    if (!confirmClear) {
      confirmClear = true;
      setTimeout(() => (confirmClear = false), 2800);
      return;
    }
    confirmClear = false;
    await clearClips();
    pushToast('Clipboard history cleared');
  }

  const SHORTCUTS: [string[], string][] = [
    [['Ctrl', 'K'], 'Command palette'],
    [['Esc'], 'Close panel'],
    [['Ctrl', 'Q'], 'Quit Krovi'],
    [['Alt', '1–9'], 'Jump to a tool'],
  ];
</script>

<div class="view">
  <div class="scroll sc">
  <div class="body">
    {#if status === 'loading'}
      {#each Array(4) as _, i (i)}<div class="skel" style="height:52px;margin-bottom:10px;border-radius:14px"></div>{/each}
    {:else if status === 'error' || !cfg}
      <State tone="error" title={'Couldn’t read settings'} text="The shell did not answer. Try again in a moment." action={{ label: 'Try again', run: load }} />
    {:else}
      <section>
        <h2>General</h2>
        <div class="group card">
          <div class="field">
            <div class="lab"><strong>Open shortcut</strong><span class="faint">Show or hide Krovi from anywhere.</span></div>
            {@render recorder({ value: cfg.shortcut, on: recording, draft: draftKeys, start: startRecording, key: onRecordKey, stop: () => (recording = false), name: 'Shortcut' })}
          </div>
          {#if error}<p class="err" role="alert"><Icon name="alert" size={13} /> {error}</p>{/if}

          <div class="field">
            <div class="lab"><strong>Show the pill</strong><span class="faint">Keep the glanceable pill at the top of your screen.</span></div>
            <Toggle label="Show the pill" checked={cfg.showPill} onchange={(v) => patch({ showPill: v })} />
          </div>
          <div class="field">
            <div class="lab"><strong>Collapse when I click away</strong><span class="faint">Return to the pill when focus leaves the panel.</span></div>
            <Toggle label="Collapse when I click away" checked={cfg.collapseOnBlur} onchange={(v) => patch({ collapseOnBlur: v })} />
          </div>
          <div class="field">
            <div class="lab"><strong>Launch at login</strong><span class="faint">Start Krovi quietly when you sign in.</span></div>
            <Toggle label="Launch at login" checked={cfg.launchAtLogin} onchange={(v) => patch({ launchAtLogin: v })} />
          </div>
          <div class="field">
            <div class="lab"><strong>Hide in fullscreen</strong><span class="faint">Stay out of the way of games, video and slides.</span></div>
            <Toggle label="Hide in fullscreen" checked={cfg.hideInFullscreen} onchange={(v) => patch({ hideInFullscreen: v })} />
          </div>
        </div>
      </section>

    <section>
      <h2>Appearance</h2>
      <div class="group card">
        <div class="field col">
          <div class="lab">
            <strong>Theme</strong>
            <span class="faint">Every colour in Krovi comes from the theme. Follow system switches with your PC, now {paintedName}.</span>
          </div>
          <div class="swatches" role="radiogroup" aria-label="Theme" tabindex="-1" onkeydown={onThemeKey}>
            <button
              type="button"
              class="sw wide"
              role="radio"
              aria-checked={choice === 'system'}
              tabindex={choice === 'system' ? 0 : -1}
              onclick={() => themeChoice.set('system')}>
              <span class="art split">
                {@render face(darkTheme.swatch)}
                {@render face(lightTheme.swatch)}
                {#if choice === 'system'}<span class="tick"><Icon name="check" size={11} /></span>{/if}
              </span>
              <span class="nm">Follow system</span>
            </button>
            {#each THEMES as t (t.id)}
              <button
                type="button"
                class="sw"
                role="radio"
                title={t.blurb}
                aria-checked={choice === t.id}
                tabindex={choice === t.id ? 0 : -1}
                onclick={() => themeChoice.set(t.id)}>
                <span class="art">
                  {@render face(t.swatch)}
                  {#if choice === t.id}<span class="tick"><Icon name="check" size={11} /></span>{/if}
                </span>
                <span class="nm">{t.name}</span>
              </button>
            {/each}
          </div>
        </div>
      </div>
    </section>

    <section>
      <h2>Idle companion</h2>
      <div class="group card">
        <div class="field">
          <div class="lab">
            <strong>Rest as a companion</strong>
            <span class="faint">After a quiet moment the pill turns into a pair of eyes. Hover, click or any shortcut wakes it instantly.</span>
          </div>
          <Toggle label="Idle companion" checked={idleOn} onchange={(v) => setIdle({ enabled: v })} />
        </div>
        <div class="field col" class:muted={!idleOn}>
          <div class="lab"><strong>Style</strong></div>
          <div class="styles" role="radiogroup" aria-label="Idle companion style">
            {#each IDLE_STYLES as s (s.id)}
              <button
                type="button"
                class="style"
                role="radio"
                title={s.blurb}
                aria-checked={$idleSettings.style === s.id}
                disabled={!idleOn}
                onclick={() => setIdle({ style: s.id })}>
                <span class="well" aria-hidden="true">
                  <IdleCompanion active preview style={s.id} personality={$idleSettings.personality} />
                </span>
                <span class="nm">{s.name}</span>
              </button>
            {/each}
          </div>
        </div>
        <div class="field" class:muted={!idleOn}>
          <div class="lab"><strong>Personality</strong><span class="faint">How often it looks around, blinks and glances at your cursor.</span></div>
          <select
            class="input sel"
            aria-label="Idle companion personality"
            disabled={!idleOn}
            value={$idleSettings.personality}
            onchange={(e) => setIdle({ personality: e.currentTarget.value as IdleSettings['personality'] })}>
            {#each PERSONALITIES as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
          </select>
        </div>
        <div class="field" class:muted={!idleOn}>
          <div class="lab"><strong>Wake-up delay</strong><span class="faint">How long the pill sits quietly before the companion appears.</span></div>
          <div class="slider">
            <input
              class="range"
              type="range"
              min="0"
              max="1000"
              step="1"
              aria-label="Idle companion delay"
              aria-valuetext={delayLabel($idleSettings.delayMs)}
              disabled={!idleOn}
              value={Math.round(delayPos * 1000)}
              style:--p="{delayPos * 100}%"
              oninput={(e) => setIdle({ delayMs: posToDelay(Number(e.currentTarget.value) / 1000, IDLE_DELAY_MIN, IDLE_DELAY_MAX) })} />
            <output class="num">{delayLabel($idleSettings.delayMs)}</output>
          </div>
        </div>
      </div>
    </section>
    {/if}

    <section>
      <h2>Calendar</h2>
      <div class="group card">
        <div class="field">
          <div class="lab"><strong>Holiday region</strong><span class="faint">Public holidays shown in Calendar and on Home. Bundled with the app, no network.</span></div>
          <select class="input sel" value={$region} onchange={(e) => region.set(e.currentTarget.value)} aria-label="Holiday region">
            {#each REGIONS as r (r.code)}<option value={r.code}>{r.name}</option>{/each}
          </select>
        </div>
      </div>
    </section>

    <section>
      <h2>Voice &amp; privacy</h2>
      {#if vStatus === 'loading'}
        {#each Array(3) as _, i (i)}<div class="skel" style="height:52px;margin-bottom:10px;border-radius:14px"></div>{/each}
      {:else if vStatus === 'error' || !vs}
        <State tone="error" title={'Couldn’t read voice settings'} text="The voice module did not answer. Try again in a moment." action={{ label: 'Try again', run: loadVoice }} />
      {:else}
        <div class="group card">
          <div class="field">
            <div class="lab">
              <strong>What can I say?</strong>
              <span class="faint">The exact phrases Voice understands, plus the folders and apps it can open on this PC.</span>
            </div>
            <button class="btn" type="button" onclick={() => voiceHelpOpen.set(true)}><Icon name="circleHelp" size={14} /> Show phrases</button>
          </div>
          <div class="field">
            <div class="lab">
              <strong>Processed locally on this PC</strong>
              <span class="faint">Speech is recognised on this computer. Nothing is sent to a server.</span>
            </div>
            <span class="fixed" title="This is a guarantee, not a preference"><span class="always"><Icon name="lock" size={12} /> Always on</span><Toggle label="Processed locally on this PC (always on)" checked={true} locked /></span>
          </div>
          <div class="field">
            <div class="lab">
              <strong>Audio is never stored</strong>
              <span class="faint">Your voice is discarded the moment it has been recognised.</span>
            </div>
            <span class="fixed" title="This is a guarantee, not a preference"><span class="always"><Icon name="lock" size={12} /> Always on</span><Toggle label="Audio is never stored (always on)" checked={true} locked /></span>
          </div>
          <div class="field">
            <div class="lab">
              <strong>Keep transcripts</strong>
              <span class="faint">Saves what you said as the note. Off, voice commands leave no transcript anywhere.</span>
            </div>
            <Toggle label="Keep transcripts" checked={vs.keepTranscripts} onchange={(v) => patchVoice({ keepTranscripts: v })} />
          </div>
          <div class="field">
            <div class="lab">
              <strong>Clean up dictation</strong>
              <span class="faint">Removes filler words and repeats, and fixes capitals and punctuation in dictated notes.</span>
            </div>
            <Toggle label="Clean up dictation" checked={vs.cleanTranscription} onchange={(v) => patchVoice({ cleanTranscription: v })} />
          </div>
          <div class="field col">
            <div class="lab">
              <strong>What “this” means</strong>
              <span class="faint">When you say “this” or “it”, Krovi can read what you have selected. Only at the moment you speak a command, never in the background.</span>
            </div>
            <div class="modes" role="radiogroup" aria-label="Context for voice commands" tabindex="-1" onkeydown={onModeKey}>
              {#each CONTEXT_MODES as m (m.id)}
                <button
                  type="button"
                  class="mode"
                  role="radio"
                  aria-checked={vs.contextCapture === m.id}
                  tabindex={vs.contextCapture === m.id ? 0 : -1}
                  onclick={() => patchVoice({ contextCapture: m.id })}>
                  <span class="rd" aria-hidden="true"></span>
                  <span class="mt"><strong>{m.label}</strong><span class="faint">{m.hint}</span></span>
                </button>
              {/each}
            </div>
          </div>
          {#if vError}<p class="err" role="alert"><Icon name="alert" size={13} /> {vError}</p>{/if}
          <div class="facts">
            <span class="eyebrow">What Krovi does with your voice</span>
            <ul>
              {#each PRIVACY_FACTS as fact (fact)}
                <li><Icon name="check" size={13} /><span>{fact}</span></li>
              {/each}
            </ul>
          </div>
        </div>
      {/if}
    </section>

    <section>
      <h2>Dictation</h2>
      {#if dStatus === 'loading'}
        {#each Array(3) as _, i (i)}<div class="skel" style="height:52px;margin-bottom:10px;border-radius:14px"></div>{/each}
      {:else if dStatus === 'error'}
        <State tone="error" title={'Couldn’t read dictation settings'} text="The dictation module did not answer. Try again in a moment." action={{ label: 'Try again', run: loadDict }} />
      {:else}
        <div class="group card">
          {#if dOff}
            <div class="field demo">
              <div class="lab"><strong>{DESKTOP_ONLY}</strong><span class="faint">{DESKTOP_ONLY_LONG}</span></div>
              <span class="shield"><Icon name="micOff" size={18} /></span>
            </div>
          {/if}
          <div class="dgroup" class:muted={dOff} inert={dOff}>
            <div class="field">
              <div class="lab">
                <strong>Dictate anywhere</strong>
                <span class="faint">Hold a shortcut, speak, and Krovi types what you said into the app you’re using.</span>
              </div>
              <Toggle label="Dictate anywhere" checked={ds.enabled} onchange={(v) => saveDict({ ...ds, enabled: v })} />
            </div>
            {#if ds.enabled && !modelReady}
              <p class="hint"><Icon name="download" size={13} /> Download a speech model below to start dictating.</p>
            {/if}
            <div class="field">
              <div class="lab"><strong>Shortcut</strong><span class="faint">Works from any app, even while Krovi is tucked away.</span></div>
              {@render recorder({ value: ds.shortcut, on: dRecording, draft: dDraft, start: startDictRecording, key: onDictKey, stop: () => (dRecording = false), name: 'Dictation shortcut' })}
            </div>
            <div class="field">
              <div class="lab">
                <strong>Talk to Agent</strong>
                <span class="faint">Hold the Agent shortcut and say what you want, like “open YouTube” or “set a timer for ten minutes”. It isn’t typed anywhere: it becomes a plan in Agent, where safe steps run and anything else waits for your OK.</span>
              </div>
              <Toggle label="Talk to Agent" checked={ds.agentEnabled} onchange={(v) => saveDict({ ...ds, agentEnabled: v }, 'agent')} />
            </div>
            {#if ds.agentEnabled && !modelReady}
              <p class="hint"><Icon name="download" size={13} /> Download a speech model below to talk to Agent.</p>
            {/if}
            <div class="field">
              <div class="lab"><strong>Agent shortcut</strong><span class="faint">Works from any app. Dictation keeps its own shortcut, so what you dictate is never taken as a command.</span></div>
              {@render recorder({ value: ds.agentShortcut, on: aRecording, draft: aDraft, start: startAgentRecording, key: onAgentKey, stop: () => (aRecording = false), name: 'Agent shortcut' })}
            </div>
            {@render derr('agent')}
            <div class="field">
              <div class="lab">
                <strong>How it works</strong>
                <span class="faint">{ds.mode === 'hold' ? 'Hold a shortcut while you speak, then let go.' : 'Press a shortcut once to start talking, and again to finish.'} Both shortcuts work the same way.</span>
              </div>
              <Segmented
                label="How the dictation and Agent shortcuts work"
                size="sm"
                value={ds.mode}
                options={[
                  { id: 'hold', label: 'Hold' },
                  { id: 'toggle', label: 'Toggle' },
                ]}
                onchange={(v: TalkMode) => saveDict({ ...ds, mode: v })} />
            </div>
            {@render derr('top')}

            <div class="field col">
              <div class="lab">
                <strong>Speech model</strong>
                <span class="faint">Bigger models are more accurate but take more disk space and a little longer.</span>
              </div>
              <div class="models" role="radiogroup" aria-label="Speech model" tabindex="-1" onkeydown={onModelKey}>
                {#each di.models as m (m.id)}
                  {@const st = m.status.state}
                  {@const pct = Math.round(progressFraction(di.progress) * 100)}
                  <div class="model">
                    <button
                      type="button"
                      class="pickm"
                      role="radio"
                      aria-checked={ds.model === m.id}
                      tabindex={pickable === m.id ? 0 : -1}
                      disabled={st !== 'installed'}
                      title={st === 'installed' ? 'Use this model' : 'Download it to use it'}
                      onclick={() => saveDict({ ...ds, model: m.id }, 'model')}>
                      <span class="rd" aria-hidden="true"></span>
                      <span class="mt">
                        <strong>{m.displayName}</strong>
                        <span class="faint">{formatMB(m.sizeBytes)} · {m.multilingual ? 'Multilingual' : 'English only'} · <span title={m.license}>{shortLicense(m.license)} licence</span></span>
                      </span>
                    </button>
                    <div class="macts">
                      {#if di.downloading === m.id}
                        <span class="bar" role="progressbar" aria-label="Downloading {m.displayName}" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct}>
                          <i style:transform="scaleX({progressFraction(di.progress)})"></i>
                        </span>
                        <span class="pct">{pct}%</span>
                        <button class="btn sm" type="button" onclick={() => void cancelDownload()}>Cancel</button>
                      {:else if st === 'installed'}
                        <span class="tag"><Icon name="check" size={11} /> Installed</span>
                        <button
                          class="btn sm ghost icon danger"
                          class:armed={confirmDelete === m.id}
                          type="button"
                          aria-label={confirmDelete === m.id ? `Click again to delete ${m.displayName}` : `Delete ${m.displayName}`}
                          title={confirmDelete === m.id ? 'Click again to delete' : 'Delete from this PC'}
                          onclick={() => askDelete(m.id)}>
                          <Icon name="trash" size={13} />
                        </button>
                      {:else}
                        {#if st === 'partial'}<span class="tag">Incomplete</span>{/if}
                        <button class="btn sm" type="button" disabled={di.downloading !== null} onclick={() => void downloadModel(m.id)}>
                          <Icon name="download" size={13} /> {st === 'partial' ? 'Download again' : 'Download'}
                        </button>
                      {/if}
                    </div>
                    {#if di.modelError?.id === m.id}
                      <p class="err inrow" role="alert"><Icon name="alert" size={13} /> {di.modelError.message}</p>
                    {/if}
                  </div>
                {/each}
              </div>
              <p class="note faint"><Icon name="lock" size={12} /> {DOWNLOAD_NOTE}</p>
              {@render derr('model')}
            </div>

            <div class="field">
              <div class="lab"><strong>Language</strong><span class="faint">{langHint}</span></div>
              <select class="input sel" aria-label="Dictation language" value={ds.language} onchange={(e) => saveDict({ ...ds, language: e.currentTarget.value }, 'text')}>
                {#each DICTATION_LANGUAGES as l (l.id)}<option value={l.id}>{l.label}</option>{/each}
              </select>
            </div>

            <div class="field col">
              <div class="mrow">
                <div class="lab"><strong>Microphone</strong><span class="faint">It’s only on while you dictate or test it. Sound is never saved.</span></div>
                <select class="input sel mic" aria-label="Microphone" value={ds.device ?? ''} onchange={(e) => void pickDevice(e.currentTarget.value)}>
                  <option value="">Windows default</option>
                  {#if deviceMissing && ds.device}<option value={ds.device}>Saved microphone (not connected)</option>{/if}
                  {#each devices as d (d.id)}<option value={d.id}>{d.name}{d.isDefault ? ' (default)' : ''}</option>{/each}
                </select>
              </div>
              <div class="test">
                <button class="btn sm" class:on={testing} type="button" disabled={micBusy} onclick={() => void toggleTest()}>
                  <Icon name={testing ? 'close' : 'mic'} size={13} /> {testing ? 'Stop test' : 'Test microphone'}
                </button>
                <span class="level" role="meter" aria-label="Microphone level" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(micLevel * 100)}>
                  <i style:transform="scaleX({micLevel})"></i>
                </span>
                {#if testing}<span class="faint tst">Say something.</span>{/if}
              </div>
              {#if micError}<p class="err inrow" role="alert"><Icon name="alert" size={13} /> {micError}</p>{/if}
              {@render derr('mic')}
            </div>

            <div class="field">
              <div class="lab"><strong>Writing style</strong><span class="faint">{styleHint}</span></div>
              <select class="input sel" aria-label="Writing style" value={ds.options.style} onchange={(e) => setOpt({ style: e.currentTarget.value as DictationStyle })}>
                {#each DICTATION_STYLES as s (s.id)}<option value={s.id}>{s.label}</option>{/each}
              </select>
            </div>
            <div class="field">
              <div class="lab"><strong>Remove filler words</strong><span class="faint">Leaves out “um”, “uh” and the like.</span></div>
              <Toggle label="Remove filler words" checked={ds.options.remove_fillers} onchange={(v) => setOpt({ remove_fillers: v })} />
            </div>

            <div class="field col">
              <div class="lab"><strong>Vocabulary</strong><span class="faint">Names and terms to listen for: people, products, jargon.</span></div>
              {#if ds.vocabulary.length}
                <ul class="chips" aria-label="Your vocabulary">
                  {#each ds.vocabulary as t, i (`${i}:${t}`)}
                    <li class="vchip">
                      <span>{t}</span>
                      <button type="button" class="x" aria-label="Remove {t}" onclick={() => saveDict({ ...ds, vocabulary: removeAt(ds.vocabulary, i) }, 'text')}>
                        <Icon name="close" size={11} />
                      </button>
                    </li>
                  {/each}
                </ul>
              {/if}
              <form
                class="addrow"
                onsubmit={(e) => {
                  e.preventDefault();
                  addTerm();
                }}>
                <input class="input" type="text" maxlength={VOCAB_MAX_TERM} placeholder="Add a name or term" aria-label="New vocabulary term" bind:value={vocabDraft} />
                <button class="btn sm" type="submit" disabled={!vocabDraft.trim()}><Icon name="plus" size={13} /> Add</button>
              </form>
            </div>

            <div class="field col">
              <div class="lab"><strong>Replacements</strong><span class="faint">When you say the first, Krovi writes the second. Good for names, emails and short phrases.</span></div>
              {#each rows as r, i (i)}
                <div class="rrow">
                  <input class="input" type="text" maxlength={REPLACE_MAX_SPOKEN} placeholder="When I say" aria-label="Spoken phrase {i + 1}" bind:value={r.spoken} onchange={commitRows} />
                  <span class="arr faint" aria-hidden="true">→</span>
                  <input class="input" type="text" maxlength={REPLACE_MAX_WRITTEN} placeholder="Write" aria-label="Written text {i + 1}" bind:value={r.written} onchange={commitRows} />
                  <button class="btn sm ghost icon danger" type="button" aria-label="Remove replacement {i + 1}" onclick={() => dropRow(i)}><Icon name="trash" size={13} /></button>
                </div>
              {/each}
              <div><button class="btn sm" type="button" onclick={() => (rows = [...rows, { spoken: '', written: '' }])}><Icon name="plus" size={13} /> Add replacement</button></div>
              {@render derr('text')}
            </div>
          </div>
        </div>
      {/if}
    </section>

    <section>
      <h2>Read aloud</h2>
      {#if raStatus === 'loading'}
        {#each Array(3) as _, i (i)}<div class="skel" style="height:52px;margin-bottom:10px;border-radius:14px"></div>{/each}
      {:else if raStatus === 'error'}
        <State tone="error" title={'Couldn’t read the voice settings'} text="The read-aloud module did not answer. Try again in a moment." action={{ label: 'Try again', run: loadReadAloud }} />
      {:else}
        <div class="group card ra">
          <div class="field">
            <div class="lab">
              <strong>Voice</strong>
              <span class="faint">
                {#if raHasVoices}
                  The Windows voice that reads summaries out loud in Explain. It runs on this PC, with no internet.
                {:else}
                  Windows didn’t report any installed voices. Add one in Windows Settings › Time &amp; language › Speech.
                {/if}
              </span>
            </div>
            <select
              class="input sel mic"
              aria-label="Read-aloud voice"
              disabled={!raHasVoices || raBusy}
              value={raVoice}
              onchange={(e) => void patchReadAloud({ windowsVoice: e.currentTarget.value || null })}>
              {#each raOptions as o (o.id)}<option value={o.id}>{o.label}</option>{:else}<option value="">System default</option>{/each}
            </select>
          </div>
          <div class="field">
            <div class="lab"><strong>Speed</strong><span class="faint">How fast it talks. 1.0× is normal speech.</span></div>
            <div class="slider">
              <input
                class="range"
                type="range"
                min={RATE_MIN}
                max={RATE_MAX}
                step="0.1"
                aria-label="Read-aloud speed"
                aria-valuetext={rateLabel(raSettings.rate)}
                style:--p="{ratePercent(raSettings.rate)}%"
                value={raSettings.rate}
                oninput={(e) => (raSettings = { ...raSettings, rate: clampRate(Number(e.currentTarget.value)) })}
                onchange={(e) => void patchReadAloud({ rate: clampRate(Number(e.currentTarget.value)) })} />
              <output class="num">{rateLabel(raSettings.rate)}</output>
            </div>
          </div>
          <div class="field">
            <div class="lab"><strong>Hear it</strong><span class="faint">Plays one short sentence in the voice and speed you chose.</span></div>
            <div class="racts">
              <button class="btn sm" type="button" disabled={!raHasVoices || raBusy || raPreviewing || raSpeaking} onclick={() => void previewVoice()}>
                <Icon name="volume" size={13} /> Preview
              </button>
              <button class="btn sm" type="button" disabled={!raSpeaking} onclick={() => void stopPreview()}>
                <Icon name="square" size={13} /> Stop
              </button>
            </div>
          </div>
          {#if raError}<p class="err" role="alert"><Icon name="alert" size={13} /> {raError}</p>{/if}
          {#if raEngines?.windows.error}<p class="err" role="alert"><Icon name="alert" size={13} /> {raEngines.windows.error}</p>{/if}
          <div class="field">
            <div class="lab">
              <strong>ElevenLabs voice (optional)</strong>
              <span class="faint">{raEleven.detail}</span>
            </div>
            <div class="racts">
              <span class="tag" class:accent={raEleven.tone === 'on'} title={raEleven.sendsText ? 'Text is sent to ElevenLabs while it reads' : 'Nothing is sent'}>
                {#if raEleven.sendsText}<Icon name="upload" size={11} />{/if}{raEleven.label}
              </span>
              <button class="btn sm" type="button" onclick={() => openTool('explain')}>
                <Icon name="externalLink" size={13} /> {raEleven.tone === 'off' ? 'Set up in Explain' : 'Manage in Explain'}
              </button>
            </div>
          </div>
        </div>
      {/if}
    </section>

    <section>
      <h2>AI (bring your own key)</h2>
      <AiSettings />
    </section>

    <section>
      <h2>Privacy</h2>
      <div class="group card">
        <div class="field">
          <div class="lab">
            <strong>Everything stays on this device</strong>
            <span class="faint">No account, no telemetry, no ads. By themselves only weather and exchange rates reach the network, with no key and no identifier. AI features send something only if you add your own key and press their button.</span>
          </div>
          <span class="shield"><Icon name="shield" size={18} /></span>
        </div>
        <div class="field">
          <div class="lab"><strong>Clipboard history</strong><span class="faint">{$clips.length} {$clips.length === 1 ? 'item' : 'items'} stored locally. Pinned items are kept.</span></div>
          <button class="btn danger" class:armed={confirmClear} disabled={!$clips.length} onclick={doClear}>
            <Icon name="trash" size={14} />{confirmClear ? 'Sure? Click again' : 'Clear history'}
          </button>
        </div>
      </div>
    </section>

    <section>
      <h2>Keyboard</h2>
      <div class="group card">
        {#each SHORTCUTS as [keys, what] (what)}
          <div class="field slim">
            <span>{what}</span>
            <span class="keys">{#each keys as k (k)}<kbd>{k}</kbd>{/each}</span>
          </div>
        {/each}
      </div>
    </section>

    <p class="about faint">
      Krovi {pkg.version} · free and open source (MIT) · {mode() === 'demo' ? 'running in your browser' : 'running natively'}
    </p>
  </div>
  </div>
</div>

{#snippet recorder(r: { value: string; on: boolean; draft: string[]; start: () => void; key: (e: KeyboardEvent) => void; stop: () => void; name: string })}
  <button
    class="rec"
    class:on={r.on}
    type="button"
    onclick={r.start}
    onkeydown={r.key}
    onblur={r.stop}
    aria-label={r.on ? 'Press the new shortcut, or Escape to cancel' : `${r.name} ${r.value}. Click to change`}>
    {#if r.on}
      {#if r.draft.length}
        {#each r.draft as k (k)}<kbd>{k}</kbd>{/each}<span class="faint">+ key</span>
      {:else}
        <span class="faint">Press a shortcut</span>
      {/if}
    {:else}
      {#each r.value.split('+').filter(Boolean) as k (k)}<kbd>{k}</kbd>{/each}
      <Icon name="pencil" size={13} />
    {/if}
  </button>
{/snippet}

{#snippet derr(area: DictArea)}
  {#if dError && dErrArea === area}<p class="err" class:inrow={area !== 'top'} role="alert"><Icon name="alert" size={13} /> {dError}</p>{/if}
{/snippet}

{#snippet face(sw: readonly [string, string, string])}
  <span class="half" style:--sw-bg={sw[0]} style:--sw-fg={sw[1]} style:--sw-ac={sw[2]} aria-hidden="true">
    <i class="l1"></i><i class="l2"></i><i class="l3"></i><i class="dot"></i>
  </span>
{/snippet}

<style>
  .view {
    padding-right: 8px;
  }
  .sc {
    flex: 1;
    min-height: 0;
    padding-right: 14px;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 22px;
    max-width: 660px;
    margin: 0 auto;
    padding: 2px 0 10px;
  }
  h2 {
    margin: 0 0 8px 4px;
    font-size: var(--t-micro);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
    color: var(--text-3);
  }
  .group {
    border-radius: var(--r-4);
  }
  .field {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    min-height: 58px;
    padding: 10px 16px;
  }
  .field + .field {
    border-top: 1px solid var(--line);
  }
  .field.slim {
    min-height: 42px;
    font-size: var(--t-sm);
    font-weight: 560;
  }
  .lab {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .lab strong {
    font-size: var(--t-md);
    font-weight: 600;
    letter-spacing: -0.005em;
  }
  .lab span {
    font-size: var(--t-xs);
    line-height: 1.45;
    max-width: 46ch;
  }
  .keys {
    display: inline-flex;
    gap: 4px;
  }
  .rec {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-width: 132px;
    height: 34px;
    padding: 0 10px;
    justify-content: center;
    border-radius: var(--r-2);
    border: 1px solid var(--line);
    background: var(--surface-1);
    color: var(--text-3);
    font-size: var(--t-xs);
    transition:
      border-color var(--dur-1),
      background var(--dur-1),
      box-shadow var(--dur-2);
  }
  .rec:hover {
    border-color: var(--line-strong);
    background: var(--surface-2);
  }
  .rec.on {
    border-color: var(--accent);
    background: var(--accent-soft);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .err {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: -2px 16px 10px;
    font-size: var(--t-xs);
    color: var(--danger);
  }
  .sel {
    width: auto;
    min-width: 150px;
  }
  .shield {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 11px;
    background: var(--surface-2);
    color: var(--text-2);
    flex: none;
  }
  .armed {
    color: var(--danger);
    border-color: color-mix(in srgb, var(--danger) 50%, transparent);
  }
  .about {
    font-size: var(--t-xs);
    padding: 2px 4px 8px;
  }

  /* stacked field: label above a full-width control */
  .field.col {
    flex-direction: column;
    align-items: stretch;
    gap: 12px;
    padding-bottom: 16px;
  }
  .muted {
    opacity: 0.5;
    transition: opacity var(--dur-2);
  }

  /* theme swatch cards: a miniature of the theme, drawn from its own swatch data */
  .swatches {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 10px;
  }
  .sw {
    display: grid;
    gap: 7px;
    padding: 0;
    text-align: left;
    color: var(--text-2);
    transition:
      color var(--dur-1),
      transform 0.45s var(--spring);
  }
  .sw.wide {
    grid-column: span 2;
  }
  .sw:hover {
    color: var(--text);
    transform: translateY(-1px);
  }
  .sw:active {
    transform: scale(0.97);
    transition-duration: 0.08s;
  }
  .sw[aria-checked='true'] {
    color: var(--text);
  }
  .art {
    position: relative;
    display: grid;
    grid-template-columns: 1fr;
    height: 64px;
    border-radius: var(--r-3);
    border: 1px solid var(--line-strong);
    overflow: hidden;
    transition:
      box-shadow var(--dur-2) var(--ease-out),
      border-color var(--dur-2);
  }
  .art.split {
    grid-template-columns: 1fr 1fr;
  }
  .sw[aria-checked='true'] .art {
    border-color: transparent;
    box-shadow: 0 0 0 2px var(--invert-bg);
  }
  .sw:focus-visible .art {
    box-shadow: 0 0 0 3px var(--accent-soft), 0 0 0 1.5px var(--accent);
  }
  .half {
    position: relative;
    background: var(--sw-bg);
  }
  .half i {
    position: absolute;
    display: block;
    border-radius: 3px;
  }
  .l1 {
    left: 10px;
    top: 13px;
    width: 46%;
    height: 6px;
    background: var(--sw-fg);
  }
  .l2 {
    left: 10px;
    top: 26px;
    width: 68%;
    height: 5px;
    background: color-mix(in srgb, var(--sw-fg) 38%, transparent);
  }
  .l3 {
    left: 10px;
    top: 37px;
    width: 52%;
    height: 5px;
    background: color-mix(in srgb, var(--sw-fg) 38%, transparent);
  }
  .half .dot {
    right: 9px;
    bottom: 8px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--sw-ac);
  }
  .tick {
    position: absolute;
    top: 6px;
    right: 6px;
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--invert-bg);
    color: var(--invert-fg);
    box-shadow: var(--shadow-sm);
    animation: tick-in 0.5s var(--spring-bouncy) both;
  }
  @keyframes tick-in {
    from {
      transform: scale(0.4);
      opacity: 0;
    }
  }
  .nm {
    font-size: var(--t-sm);
    font-weight: 600;
    letter-spacing: -0.005em;
  }

  /* idle companion style cards: each preview sits in a little pill, like the real thing */
  .styles {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 10px;
  }
  .style {
    display: grid;
    gap: 7px;
    padding: 0;
    text-align: left;
    color: var(--text-2);
    transition:
      color var(--dur-1),
      transform 0.45s var(--spring),
      opacity var(--dur-2);
  }
  .style:hover:not(:disabled) {
    color: var(--text);
    transform: translateY(-1px);
  }
  .style[aria-checked='true'] {
    color: var(--text);
  }
  .style:disabled {
    cursor: default;
  }
  .well {
    position: relative;
    display: block;
    height: 40px;
    border-radius: 999px;
    border: 1px solid var(--line-strong);
    background: var(--pill-bg);
    color: var(--pill-fg-2);
    overflow: hidden;
    transition: box-shadow var(--dur-2) var(--ease-out);
  }
  .style[aria-checked='true'] .well {
    border-color: transparent;
    box-shadow: 0 0 0 2px var(--invert-bg);
  }
  .style:focus-visible .well {
    box-shadow: 0 0 0 3px var(--accent-soft), 0 0 0 1.5px var(--accent);
  }
  .slider {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 230px;
  }
  .slider output {
    min-width: 42px;
    text-align: right;
    font-size: var(--t-sm);
    font-weight: 600;
    color: var(--text-2);
  }

  /* voice & privacy: the three "what this means" choices, each with its own explanation */
  .modes {
    display: grid;
    gap: 8px;
  }
  .mode {
    display: grid;
    grid-template-columns: 18px 1fr;
    gap: 12px;
    align-items: start;
    padding: 11px 13px;
    text-align: left;
    border-radius: var(--r-3);
    border: 1px solid var(--line);
    background: var(--surface-1);
    color: var(--text-2);
    transition:
      background var(--dur-1),
      border-color var(--dur-1),
      color var(--dur-1),
      transform 0.45s var(--spring);
  }
  .mode:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .mode:active {
    transform: scale(0.99);
    transition-duration: 0.08s;
  }
  .mode[aria-checked='true'] {
    border-color: var(--line-strong);
    background: var(--surface-2);
    color: var(--text);
  }
  .mode:focus-visible {
    box-shadow: 0 0 0 3px var(--accent-soft), 0 0 0 1.5px var(--accent);
  }
  .rd {
    position: relative;
    width: 18px;
    height: 18px;
    margin-top: 1px;
    border-radius: 50%;
    border: 1.5px solid var(--line-strong);
    transition:
      background var(--dur-2),
      border-color var(--dur-2);
  }
  .rd::after {
    content: '';
    position: absolute;
    inset: 4px;
    border-radius: 50%;
    background: var(--invert-fg);
    transform: scale(0);
    transition: transform 0.45s var(--spring-bouncy);
  }
  .mode[aria-checked='true'] .rd,
  .pickm[aria-checked='true'] .rd {
    background: var(--invert-bg);
    border-color: transparent;
  }
  .mode[aria-checked='true'] .rd::after,
  .pickm[aria-checked='true'] .rd::after {
    transform: scale(1);
  }
  .mt {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .mt strong {
    font-size: var(--t-sm);
    font-weight: 600;
  }
  .mt span {
    font-size: var(--t-xs);
    line-height: 1.45;
  }

  /* voice & privacy */
  .fixed {
    display: inline-flex;
    align-items: center;
    gap: 12px;
  }
  .always {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: var(--t-xs);
    font-weight: 600;
    color: var(--text-3);
  }
  .facts {
    padding: 14px 16px 16px;
    border-top: 1px solid var(--line);
  }
  .facts ul {
    display: grid;
    gap: 7px;
    margin-top: 10px;
  }
  .facts li {
    display: flex;
    align-items: flex-start;
    gap: 9px;
    font-size: var(--t-xs);
    line-height: 1.5;
    color: var(--text-2);
  }
  .facts li :global(.ic) {
    flex: none;
    margin-top: 2px;
    color: var(--text);
  }

  /* dictation */
  .demo .lab span {
    max-width: 52ch;
  }
  .demo + .dgroup {
    border-top: 1px solid var(--line);
  }
  .dgroup .err + .field,
  .dgroup .hint + .field {
    border-top: 1px solid var(--line);
  }
  .hint {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: -2px 16px 10px;
    font-size: var(--t-xs);
    color: var(--text-2);
  }
  .err.inrow {
    margin: 0;
  }
  .note {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    font-size: var(--t-xs);
    line-height: 1.5;
  }
  .note :global(.ic) {
    flex: none;
    margin-top: 3px;
  }

  /* model list: one row per speech model */
  .models {
    display: grid;
    border: 1px solid var(--line);
    border-radius: var(--r-3);
    background: var(--surface-1);
    overflow: hidden;
  }
  .model {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 4px 12px;
    padding: 9px 12px;
  }
  .model + .model {
    border-top: 1px solid var(--line);
  }
  .model .err {
    grid-column: 1 / -1;
  }
  .pickm {
    display: grid;
    grid-template-columns: 18px 1fr;
    gap: 12px;
    align-items: start;
    padding: 2px 0;
    text-align: left;
    border-radius: var(--r-2);
    color: var(--text-2);
    transition: color var(--dur-1);
  }
  .pickm:hover:not(:disabled),
  .pickm[aria-checked='true'] {
    color: var(--text);
  }
  .pickm:disabled {
    cursor: default;
  }
  .pickm:disabled .rd {
    opacity: 0.45;
  }
  .pickm:focus-visible {
    box-shadow: 0 0 0 3px var(--accent-soft), 0 0 0 1.5px var(--accent);
  }
  .macts {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .bar {
    position: relative;
    width: 96px;
    height: 4px;
    border-radius: 999px;
    background: var(--surface-3);
    overflow: hidden;
  }
  .bar i {
    display: block;
    width: 100%;
    height: 100%;
    background: var(--invert-bg);
    transform-origin: left center;
    transition: transform 0.25s var(--ease-out);
  }
  .pct {
    min-width: 34px;
    text-align: right;
    font-size: var(--t-xs);
    font-weight: 600;
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }

  /* microphone + live level meter (transform only) */
  .mrow {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
  }
  .sel.mic {
    max-width: 230px;
    text-overflow: ellipsis;
  }
  .test {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .level {
    position: relative;
    flex: 1;
    max-width: 220px;
    height: 6px;
    border-radius: 999px;
    background: var(--surface-3);
    overflow: hidden;
  }
  .level i {
    display: block;
    width: 100%;
    height: 100%;
    background: var(--accent);
    transform: scaleX(0);
    transform-origin: left center;
    transition: transform 90ms linear;
  }
  .tst {
    font-size: var(--t-xs);
  }

  /* read aloud: the buttons / status beside a field, and a divider after an error line */
  .racts {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex: none;
  }
  .ra .err + .field {
    border-top: 1px solid var(--line);
  }

  /* vocabulary chips and replacement rows */
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .vchip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-width: 100%;
    height: 28px;
    padding: 0 4px 0 11px;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: var(--surface-1);
    color: var(--text-2);
    font-size: var(--t-xs);
    font-weight: 600;
  }
  .vchip span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .x {
    flex: none;
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    color: var(--text-3);
    transition:
      background var(--dur-1),
      color var(--dur-1);
  }
  .x:hover {
    background: var(--surface-3);
    color: var(--text);
  }
  .x:focus-visible {
    box-shadow: 0 0 0 3px var(--accent-soft), 0 0 0 1.5px var(--accent);
  }
  .addrow {
    display: flex;
    gap: 8px;
  }
  .addrow .input {
    flex: 1;
  }
  .rrow {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
  }
  .arr {
    font-size: var(--t-sm);
  }
</style>
