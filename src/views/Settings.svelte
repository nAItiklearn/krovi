<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import Toggle from '../components/Toggle.svelte';
  import State from '../components/State.svelte';
  import { getConfig, setConfig, type ShellConfig } from '../lib/shell';
  import { REGIONS } from '../lib/holidays';
  import { clearClips, clips } from '../lib/live';
  import { pushToast, region, voiceHelpOpen } from '../lib/stores';
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
    const mods = modsOf(e);
    draftKeys = mods;
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) return;
    const key = keyName(e.code);
    if (!key) {
      error = 'That key can’t be used in a shortcut.';
      return;
    }
    if (!mods.length) {
      error = 'Include at least one modifier (Ctrl, Alt, Shift or Win).';
      return;
    }
    recording = false;
    await patch({ shortcut: [...mods, key].join('+') });
  }

  const shortcutParts = $derived((cfg?.shortcut ?? '').split('+').filter(Boolean));

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
            <button
              class="rec"
              class:on={recording}
              onclick={startRecording}
              onkeydown={onRecordKey}
              onblur={() => (recording = false)}
              aria-label={recording ? 'Press the new shortcut, or Escape to cancel' : `Shortcut ${cfg.shortcut}. Click to change`}>
              {#if recording}
                {#if draftKeys.length}
                  {#each draftKeys as k (k)}<kbd>{k}</kbd>{/each}<span class="faint">+ key</span>
                {:else}
                  <span class="faint">Press a shortcut</span>
                {/if}
              {:else}
                {#each shortcutParts as k (k)}<kbd>{k}</kbd>{/each}
                <Icon name="pencil" size={13} />
              {/if}
            </button>
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
      <h2>Privacy</h2>
      <div class="group card">
        <div class="field">
          <div class="lab">
            <strong>Everything stays on this device</strong>
            <span class="faint">No account, no telemetry, no ads. Only weather and exchange rates reach the network, with no key and no identifier.</span>
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
  .mode[aria-checked='true'] .rd {
    background: var(--invert-bg);
    border-color: transparent;
  }
  .mode[aria-checked='true'] .rd::after {
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
</style>
