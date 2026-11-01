<script lang="ts">
  import type { Component } from 'svelte';
  import Rail from './Rail.svelte';
  import Header from './Header.svelte';
  import CommandPalette from './CommandPalette.svelte';
  import Toast from './Toast.svelte';
  import VoiceResults from './VoiceResults.svelte';
  import VoiceHelp from './VoiceHelp.svelte';
  import { activeTool, paletteOpen, voiceHelpOpen, voiceResults } from '../lib/stores';
  import { TOOL_ORDER } from '../lib/tools';
  import { swap } from '../lib/motion';
  import type { ToolId } from '../lib/types';

  import Home from '../views/Home.svelte';
  import Clipboard from '../views/Clipboard.svelte';
  import PasteStack from '../views/PasteStack.svelte';
  import Timers from '../views/Timers.svelte';
  import Pomodoro from '../views/Pomodoro.svelte';
  import FileShelf from '../views/FileShelf.svelte';
  import Media from '../views/Media.svelte';
  import Calendar from '../views/Calendar.svelte';
  import Weather from '../views/Weather.svelte';
  import Notes from '../views/Notes.svelte';
  import WorldClocks from '../views/WorldClocks.svelte';
  import Convert from '../views/Convert.svelte';
  import Calculator from '../views/Calculator.svelte';
  import ScreenTime from '../views/ScreenTime.svelte';
  import Settings from '../views/Settings.svelte';
  import Transfer from '../views/Transfer.svelte';
  import Explain from '../views/Explain.svelte';

  // The panel body: rail + header + the active tool. `show` flips true once the Island shape is
  // ~60% open; everything marked .enter then rises in on a stagger (and leaves first on collapse).
  let { show }: { show: boolean } = $props();

  const VIEWS: Record<ToolId, Component> = {
    home: Home,
    clipboard: Clipboard,
    paste: PasteStack,
    timers: Timers,
    pomodoro: Pomodoro,
    shelf: FileShelf,
    transfer: Transfer,
    media: Media,
    calendar: Calendar,
    weather: Weather,
    notes: Notes,
    explain: Explain,
    clocks: WorldClocks,
    convert: Convert,
    calculator: Calculator,
    screentime: ScreenTime,
    settings: Settings,
  };

  // slide direction follows the rail: going down the list slides up, and vice versa
  let dir = $state(1);
  let last: ToolId = 'home';
  $effect(() => {
    const t = $activeTool;
    dir = TOOL_ORDER.indexOf(t) >= TOOL_ORDER.indexOf(last) ? 1 : -1;
    last = t;
  });

  const View = $derived(VIEWS[$activeTool]);
</script>

<div class="panel" data-in={show}>
  <div class="body" inert={$paletteOpen || $voiceHelpOpen || $voiceResults !== null}>
    <div class="enter railwrap" style:--i="0"><Rail /></div>
    <div class="main">
      <div class="enter" style:--i="1"><Header /></div>
      <div class="views enter" style:--i="2" id="tool-view">
        {#key $activeTool}
          <div class="slot" in:swap={{ y: 12 * dir, duration: 320 }} out:swap={{ y: -8 * dir, duration: 160 }}>
            <View />
          </div>
        {/key}
      </div>
    </div>
  </div>

  {#if $paletteOpen}
    <CommandPalette />
  {/if}
  {#if $voiceResults}
    {#key $voiceResults.id}<VoiceResults sheet={$voiceResults} />{/key}
  {/if}
  {#if $voiceHelpOpen}
    <VoiceHelp />
  {/if}
  <Toast />
</div>

<style>
  .panel {
    position: relative;
    width: 900px;
    height: 620px;
    color: var(--text);
  }
  .body {
    position: absolute;
    inset: 0;
    display: grid;
    grid-template-columns: var(--rail-w) 1fr;
  }
  .main {
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .views {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  .slot {
    position: absolute;
    inset: 0;
  }
  .railwrap {
    min-height: 0;
    display: grid;
  }

  /* ---- staggered choreography (hidden until `show`, springy rise when it flips) ---- */
  .panel[data-in='false'] :global(.enter) {
    opacity: 0;
    transform: translateY(8px) scale(0.99);
    filter: blur(6px);
    transition:
      opacity 130ms var(--ease-in),
      transform 130ms var(--ease-in),
      filter 130ms var(--ease-in);
  }
  .panel[data-in='true'] :global(.enter) {
    animation: rise 700ms var(--spring) calc(var(--i, 0) * 55ms) backwards;
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(14px) scale(0.985);
      filter: blur(8px);
    }
  }
</style>
