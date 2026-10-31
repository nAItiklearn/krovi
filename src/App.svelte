<script lang="ts">
  import { onMount } from 'svelte';
  import { activeTool, paletteOpen, startClock, applyTheme, themePref } from './lib/stores';
  import { mode } from './lib/bridge';
  import { TOOL_BY_ID } from './lib/tools';
  import Rail from './components/Rail.svelte';
  import Header from './components/Header.svelte';
  import CommandPalette from './components/CommandPalette.svelte';

  import Home from './views/Home.svelte';
  import Clipboard from './views/Clipboard.svelte';
  import PasteStack from './views/PasteStack.svelte';
  import Timers from './views/Timers.svelte';
  import Pomodoro from './views/Pomodoro.svelte';
  import FileShelf from './views/FileShelf.svelte';
  import Media from './views/Media.svelte';
  import Calendar from './views/Calendar.svelte';
  import Weather from './views/Weather.svelte';
  import Notes from './views/Notes.svelte';
  import WorldClocks from './views/WorldClocks.svelte';
  import Convert from './views/Convert.svelte';
  import Calculator from './views/Calculator.svelte';
  import ScreenTime from './views/ScreenTime.svelte';
  import Settings from './views/Settings.svelte';

  let dismissed = $state(false);

  onMount(() => {
    startClock();
    let pref: typeof $themePref = 'auto';
    const unsub = themePref.subscribe((v) => (pref = v));
    applyTheme(pref);
    const mq =
      typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)') : null;
    const onOS = () => applyTheme(pref);
    mq?.addEventListener?.('change', onOS);
    return () => {
      unsub();
      mq?.removeEventListener?.('change', onOS);
    };
  });

  function onKey(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      paletteOpen.update((v) => !v);
    } else if (e.key === 'Escape') {
      if ($paletteOpen) paletteOpen.set(false);
    }
  }

  function dismiss() {
    dismissed = true;
    paletteOpen.set(false);
  }
</script>

<svelte:window onkeydown={onKey} />

{#if dismissed}
  <div class="summon">
    <button class="summon-btn" onclick={() => (dismissed = false)}>
      <span class="logo" aria-hidden="true"></span>
      <span>
        <strong>Krovi is hidden</strong>
        <small>Click to summon · the Tauri shell binds a global hotkey (Ctrl+Alt+Space)</small>
      </span>
    </button>
  </div>
{:else}
  <main class="panel" aria-label="Krovi panel">
    <Rail />
    <div class="body">
      <Header onclose={dismiss} />
      <div class="content scroll" id="content" tabindex="-1">
        {#if $activeTool === 'home'}
          <Home />
        {:else if $activeTool === 'clipboard'}
          <Clipboard />
        {:else if $activeTool === 'paste'}
          <PasteStack />
        {:else if $activeTool === 'timers'}
          <Timers />
        {:else if $activeTool === 'pomodoro'}
          <Pomodoro />
        {:else if $activeTool === 'shelf'}
          <FileShelf />
        {:else if $activeTool === 'media'}
          <Media />
        {:else if $activeTool === 'calendar'}
          <Calendar />
        {:else if $activeTool === 'weather'}
          <Weather />
        {:else if $activeTool === 'notes'}
          <Notes />
        {:else if $activeTool === 'clocks'}
          <WorldClocks />
        {:else if $activeTool === 'convert'}
          <Convert />
        {:else if $activeTool === 'calculator'}
          <Calculator />
        {:else if $activeTool === 'screentime'}
          <ScreenTime />
        {:else if $activeTool === 'settings'}
          <Settings />
        {/if}
      </div>
      <footer class="statusbar">
        <span class="dot" class:demo={mode() === 'demo'}></span>
        <span class="faint"
          >{mode() === 'demo'
            ? 'Demo mode — running from in-memory fixtures, no backend'
            : 'Connected to the Krovi core'}</span>
        <span class="spacer"></span>
        <span class="faint">{TOOL_BY_ID[$activeTool].hint}</span>
      </footer>
    </div>
  </main>

  {#if $paletteOpen}
    <CommandPalette />
  {/if}
{/if}

<style>
  .panel {
    width: min(1080px, 95vw);
    height: min(700px, 88vh);
    display: grid;
    grid-template-columns: var(--rail-w) 1fr;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  .body {
    display: grid;
    grid-template-rows: auto 1fr auto;
    min-width: 0;
  }
  .content {
    padding: 20px 22px;
    min-height: 0;
  }
  .statusbar {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 7px 16px;
    border-top: 1px solid var(--border);
    background: var(--surface-2);
    font-size: 11.5px;
  }
  .statusbar .spacer {
    flex: 1;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-faint);
  }
  .dot.demo {
    background: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }

  .summon {
    display: grid;
    place-items: center;
  }
  .summon-btn {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 18px 22px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--surface);
    box-shadow: var(--shadow);
    color: var(--text);
    text-align: left;
  }
  .summon-btn small {
    display: block;
    color: var(--text-dim);
    font-size: 12px;
    margin-top: 3px;
  }
  .summon-btn .logo {
    width: 40px;
    height: 40px;
    border-radius: 11px;
    background: linear-gradient(140deg, var(--accent), var(--accent-strong));
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.2);
  }

  @media (max-width: 560px) {
    .panel {
      grid-template-columns: 1fr;
      grid-template-rows: auto auto 1fr auto;
    }
  }
</style>
