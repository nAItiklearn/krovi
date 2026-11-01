<script lang="ts">
  import { activeTool } from '../lib/stores';
  import { TOOLS } from '../lib/tools';
  import type { ToolId } from '../lib/types';
  import Icon from './Icon.svelte';

  // Slim icon rail. One highlight "thumb" glides between tools on a spring instead of each
  // button toggling its own background, which is what makes the switch feel physical.
  let railEl: HTMLElement | undefined = $state();

  const main = TOOLS.filter((t) => t.id !== 'settings');
  const settings = TOOLS.find((t) => t.id === 'settings')!;
  const ITEM = 30;
  const GAP = 1;

  const mainIndex = $derived(main.findIndex((t) => t.id === $activeTool));

  function select(id: ToolId) {
    activeTool.set(id);
  }

  function onKey(e: KeyboardEvent) {
    const buttons = Array.from(railEl?.querySelectorAll<HTMLButtonElement>('button[role="tab"]') ?? []);
    const cur = buttons.findIndex((b) => b === document.activeElement);
    let next = -1;
    if (e.key === 'ArrowDown' || e.key === 'ArrowRight') next = (cur + 1) % buttons.length;
    else if (e.key === 'ArrowUp' || e.key === 'ArrowLeft') next = (cur - 1 + buttons.length) % buttons.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = buttons.length - 1;
    if (next >= 0) {
      e.preventDefault();
      buttons[next].focus();
      buttons[next].click();
    }
  }
</script>

<nav class="rail" bind:this={railEl} aria-label="Krovi tools">
  <div class="brand" aria-hidden="true">
    <svg width="26" height="26" viewBox="0 0 26 26" fill="none">
      <rect x="2.5" y="9" width="21" height="8" rx="4" fill="currentColor" opacity="0.92" />
      <circle cx="18.2" cy="13" r="1.7" fill="var(--panel-bg)" />
    </svg>
  </div>

  <div class="tabs" role="tablist" aria-orientation="vertical" aria-label="Tools" onkeydown={onKey} tabindex="-1">
    <span
      class="thumb"
      class:off={mainIndex < 0}
      style:--i={Math.max(0, mainIndex)}
      style:--step="{ITEM + GAP}px"
      aria-hidden="true"></span>
    {#each main as tool (tool.id)}
      <button
        type="button"
        role="tab"
        class="item"
        class:active={$activeTool === tool.id}
        aria-selected={$activeTool === tool.id}
        aria-label={tool.label}
        tabindex={$activeTool === tool.id ? 0 : -1}
        data-tip={tool.label}
        onclick={() => select(tool.id)}>
        <Icon name={tool.icon} size={18} />
      </button>
    {/each}
  </div>

  <div class="foot" role="tablist" aria-orientation="vertical" aria-label="Preferences" onkeydown={onKey} tabindex="-1">
    <button
      type="button"
      role="tab"
      class="item solo"
      class:active={$activeTool === settings.id}
      aria-selected={$activeTool === settings.id}
      aria-label={settings.label}
      tabindex={$activeTool === settings.id ? 0 : -1}
      data-tip={settings.label}
      onclick={() => select(settings.id)}>
      <Icon name={settings.icon} size={18} />
    </button>
  </div>
</nav>

<style>
  .rail {
    position: relative;
    display: grid;
    grid-template-rows: auto 1fr auto;
    justify-items: center;
    padding: 14px 0 12px;
    border-right: 1px solid var(--line);
    background: linear-gradient(90deg, var(--surface-1), transparent);
    min-height: 0;
  }
  .brand {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    margin-bottom: 8px;
    color: var(--text);
  }
  .tabs {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-height: 0;
  }
  .thumb {
    position: absolute;
    top: 0;
    left: 0;
    width: 30px;
    height: 30px;
    border-radius: 9px;
    background: var(--surface-3);
    box-shadow: inset 0 1px 0 var(--card-hi);
    transform: translateY(calc(var(--i) * var(--step)));
    transition:
      transform 0.6s var(--spring),
      opacity var(--dur-2);
    pointer-events: none;
  }
  .thumb::before {
    content: '';
    position: absolute;
    left: -14px;
    top: 8px;
    width: 3px;
    height: 14px;
    border-radius: 0 3px 3px 0;
    background: var(--accent);
  }
  .thumb.off {
    opacity: 0;
  }
  .item {
    position: relative;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 9px;
    color: var(--text-3);
    transition:
      color var(--dur-1),
      background var(--dur-1),
      transform 0.5s var(--spring);
  }
  .item :global(svg) {
    transition: transform 0.5s var(--spring-bouncy);
  }
  .item:hover {
    color: var(--text);
  }
  .item:hover :global(svg) {
    transform: scale(1.14);
  }
  .item:active :global(svg) {
    transform: scale(0.9);
  }
  .item.active {
    color: var(--text);
  }
  .item.solo.active {
    background: var(--surface-3);
    box-shadow: inset 0 1px 0 var(--card-hi);
  }
  .item.solo.active::before {
    content: '';
    position: absolute;
    left: -15px;
    top: 8px;
    width: 3px;
    height: 14px;
    border-radius: 0 3px 3px 0;
    background: var(--accent);
  }
  .foot {
    display: grid;
  }
</style>
