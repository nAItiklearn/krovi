<script lang="ts">
  import Icon from './Icon.svelte';
  import Menu, { type MenuItem } from './Menu.svelte';
  import { activeTool, emitIntent, now, openTool, paletteOpen, voiceHelpOpen } from '../lib/stores';
  import { TOOL_BY_ID } from '../lib/tools';
  import { quit } from '../lib/shell';
  import { onMount } from 'svelte';
  import { startVoice, stopVoice, voiceAvailability, voiceState } from '../lib/voice';
  import { swap } from '../lib/motion';

  // Title + Spotlight-style search + voice + overflow menu + a close control that is always visible.
  const dateFmt = new Intl.DateTimeFormat(undefined, { weekday: 'long', day: 'numeric', month: 'long' });

  const tool = $derived(TOOL_BY_ID[$activeTool]);
  const hour = $derived(new Date($now).getHours());
  const greeting = $derived(hour < 5 ? 'Good night' : hour < 12 ? 'Good morning' : hour < 18 ? 'Good afternoon' : 'Good evening');
  const title = $derived($activeTool === 'home' ? greeting : tool.label);
  const sub = $derived($activeTool === 'home' ? dateFmt.format(new Date($now)) : tool.hint);

  const listening = $derived($voiceState.state === 'listening' || $voiceState.state === 'processing');
  let commandsOk = $state(true);
  const voiceOff = $derived(!commandsOk || $voiceState.state === 'unavailable');
  onMount(() => {
    void voiceAvailability().then((a) => (commandsOk = a.commands));
  });

  let menuOpen = $state(false);
  let moreBtn: HTMLButtonElement | undefined = $state();

  function toggleVoice() {
    if (listening) void stopVoice();
    else void startVoice('command');
  }

  const items: MenuItem[] = [
    { id: 'palette', label: 'Command palette', icon: 'command', kbd: 'Ctrl K', run: () => paletteOpen.set(true) },
    { id: 'settings', label: 'Settings', icon: 'settings', run: () => openTool('settings') },
    { id: 'theme', label: 'Change appearance', icon: 'sun', run: () => openTool('settings') },
    { id: 'voice-help', label: 'What can I say?', icon: 'circleHelp', run: () => voiceHelpOpen.set(true) },
    { id: 'close', label: 'Close panel', icon: 'close', kbd: 'Esc', separatorBefore: true, run: () => emitIntent('collapse') },
    { id: 'quit', label: 'Quit Krovi', icon: 'power', kbd: 'Ctrl Q', danger: true, run: () => void quit() },
  ];

  function closeMenu(restore: boolean) {
    menuOpen = false;
    if (restore) moreBtn?.focus();
  }
</script>

<header class="head">
  <div class="titles">
    {#key title}
      <h1 class="title" in:swap={{ y: 6, duration: 300 }}>{title}</h1>
    {/key}
    <p class="sub ellipsis">{sub}</p>
  </div>

  <button class="search" type="button" onclick={() => paletteOpen.set(true)}>
    <Icon name="search" size={15} />
    <span class="ph">Search tools and actions</span>
    <span class="keys"><kbd>Ctrl</kbd><kbd>K</kbd></span>
  </button>

  <div class="actions">
    {#if listening}
      <button
        type="button"
        class="btn sm ghost helplink"
        in:swap={{ y: 6, duration: 240 }}
        onclick={() => voiceHelpOpen.set(true)}>
        <Icon name="circleHelp" size={14} /> What can I say?
      </button>
    {/if}
    <button
      type="button"
      class="btn icon ghost mic"
      class:live={listening}
      disabled={voiceOff}
      data-tip={voiceOff ? 'Voice isn’t available here' : listening ? 'Stop listening' : 'Voice command'}
      data-tip-pos="bottom"
      aria-label={listening ? 'Stop listening' : 'Start voice command'}
      aria-pressed={listening}
      onclick={toggleVoice}>
      <Icon name={voiceOff ? 'micOff' : 'mic'} size={17} />
    </button>

    <div class="more">
      <button
        type="button"
        class="btn icon ghost"
        class:on={menuOpen}
        bind:this={moreBtn}
        aria-label="More"
        aria-haspopup="menu"
        aria-expanded={menuOpen}
        data-tip={menuOpen ? undefined : 'More'}
        data-tip-pos="bottom"
        onclick={() => (menuOpen = !menuOpen)}>
        <Icon name="more" size={18} />
      </button>
      {#if menuOpen}
        <Menu {items} label="Krovi menu" onclose={closeMenu} />
      {/if}
    </div>

    <button
      type="button"
      class="btn icon ghost close"
      aria-label="Close panel"
      data-tip="Close  Esc"
      data-tip-pos="bottom-end"
      onclick={() => emitIntent('collapse')}>
      <Icon name="close" size={18} />
    </button>
  </div>
</header>

<style>
  .head {
    display: flex;
    align-items: center;
    gap: 14px;
    height: var(--head-h);
    padding: 0 14px 0 24px;
    border-bottom: 1px solid var(--line);
    flex: none;
  }
  .titles {
    flex: 1;
    min-width: 0;
    display: grid;
    grid-template-rows: auto auto;
  }
  .title {
    font-family: var(--font-display);
    font-size: var(--t-lg);
    font-weight: 650;
    letter-spacing: -0.015em;
    line-height: 1.2;
  }
  .sub {
    font-size: var(--t-xs);
    color: var(--text-3);
  }
  .search {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 268px;
    height: 34px;
    padding: 0 8px 0 12px;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: var(--surface-1);
    color: var(--text-3);
    font-size: var(--t-sm);
    text-align: left;
    transition:
      background var(--dur-2) var(--ease-out),
      border-color var(--dur-2),
      width 0.5s var(--spring),
      color var(--dur-1);
  }
  .search:hover {
    background: var(--surface-2);
    border-color: var(--line-strong);
    color: var(--text-2);
    width: 292px;
  }
  .ph {
    flex: 1;
  }
  .keys {
    display: inline-flex;
    gap: 3px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .more {
    position: relative;
  }
  .helplink {
    margin-right: 4px;
    color: var(--text-2);
  }
  .mic.live {
    color: var(--accent);
    background: var(--accent-soft);
    animation: mic-pulse 1.6s var(--ease-in-out) infinite;
  }
  @keyframes mic-pulse {
    50% {
      box-shadow: 0 0 0 5px transparent;
    }
    0% {
      box-shadow: 0 0 0 0 var(--accent-glow);
    }
  }
  .close:hover {
    color: var(--text);
  }
</style>
