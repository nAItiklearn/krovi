<script lang="ts">
  import { activeTool, now, paletteOpen, pinned, themePref } from '../lib/stores';
  import { TOOL_BY_ID } from '../lib/tools';
  import Icon from './Icon.svelte';

  let { onclose }: { onclose: () => void } = $props();

  const dateFmt = new Intl.DateTimeFormat(undefined, { weekday: 'long', day: 'numeric', month: 'short' });
  const timeFmt = new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit' });

  const tool = $derived(TOOL_BY_ID[$activeTool]);
  const dateLabel = $derived(dateFmt.format(new Date($now)));
  const timeLabel = $derived(timeFmt.format(new Date($now)));

  function cycleTheme() {
    themePref.update((t) => (t === 'light' ? 'dark' : t === 'dark' ? 'auto' : 'light'));
  }
  const themeIcon = $derived($themePref === 'dark' ? 'moon' : $themePref === 'light' ? 'sun' : 'settings');
</script>

<header class="header">
  <div class="title">
    <Icon name={tool.icon} size={18} />
    <h1>{tool.label}</h1>
    <span class="sep" aria-hidden="true"></span>
    <span class="date muted">{dateLabel}</span>
  </div>

  <div class="right">
    <span class="clock mono" aria-label="current time">{timeLabel}</span>
    <button class="btn icon ghost" title="Command palette (Ctrl/Cmd+K)" onclick={() => paletteOpen.set(true)}>
      <Icon name="search" size={18} />
    </button>
    <button class="btn icon ghost" title={`Theme: ${$themePref}`} onclick={cycleTheme}>
      <Icon name={themeIcon} size={18} />
    </button>
    <button
      class="btn icon ghost"
      class:on={$pinned}
      title={$pinned ? 'Unpin (allow dismiss)' : 'Pin (stay open)'}
      aria-pressed={$pinned}
      onclick={() => pinned.update((p) => !p)}>
      <Icon name="pin" size={18} />
    </button>
    <button class="btn icon ghost" title="Dismiss panel" onclick={onclose}>
      <Icon name="close" size={18} />
    </button>
  </div>
</header>

<style>
  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 11px 14px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }
  .title {
    display: flex;
    align-items: center;
    gap: 9px;
    color: var(--accent);
    min-width: 0;
  }
  h1 {
    font-size: 15px;
    font-weight: 650;
    margin: 0;
    color: var(--text);
    white-space: nowrap;
  }
  .sep {
    width: 1px;
    height: 16px;
    background: var(--border);
  }
  .date {
    font-size: 12.5px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .right {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .clock {
    font-size: 13px;
    color: var(--text-dim);
    padding-right: 6px;
  }
  .btn.icon.on {
    color: var(--accent);
    background: var(--accent-soft);
  }
</style>
