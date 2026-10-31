<script lang="ts">
  import { activeTool } from '../lib/stores';
  import { TOOLS } from '../lib/tools';
  import type { ToolId } from '../lib/types';
  import Icon from './Icon.svelte';

  let railEl: HTMLElement;

  function select(id: ToolId) {
    activeTool.set(id);
  }

  // Roving focus with arrow keys for keyboard navigation.
  function onKey(e: KeyboardEvent, index: number) {
    const buttons = Array.from(railEl.querySelectorAll<HTMLButtonElement>('button[role="tab"]'));
    let next = -1;
    if (e.key === 'ArrowDown' || e.key === 'ArrowRight') next = (index + 1) % buttons.length;
    else if (e.key === 'ArrowUp' || e.key === 'ArrowLeft')
      next = (index - 1 + buttons.length) % buttons.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = buttons.length - 1;
    if (next >= 0) {
      e.preventDefault();
      buttons[next].focus();
    }
  }

  // Keep Settings visually separated at the bottom.
  const main = TOOLS.filter((t) => t.id !== 'settings');
  const settings = TOOLS.find((t) => t.id === 'settings')!;
</script>

<div class="rail" bind:this={railEl} role="tablist" aria-label="Krovi tools" aria-orientation="vertical">
  <div class="brand" aria-hidden="true"></div>
  <div class="group scroll">
    {#each main as tool, i (tool.id)}
      <button
        role="tab"
        class="item"
        class:active={$activeTool === tool.id}
        aria-selected={$activeTool === tool.id}
        tabindex={$activeTool === tool.id ? 0 : -1}
        title={`${tool.label} — ${tool.hint}`}
        onclick={() => select(tool.id)}
        onkeydown={(e) => onKey(e, i)}>
        <Icon name={tool.icon} size={18} />
        <span class="label">{tool.label}</span>
      </button>
    {/each}
  </div>
  <button
    role="tab"
    class="item"
    class:active={$activeTool === settings.id}
    aria-selected={$activeTool === settings.id}
    tabindex={$activeTool === settings.id ? 0 : -1}
    title={`${settings.label} — ${settings.hint}`}
    onclick={() => select(settings.id)}
    onkeydown={(e) => onKey(e, main.length)}>
    <Icon name={settings.icon} size={18} />
    <span class="label">{settings.label}</span>
  </button>
</div>

<style>
  .rail {
    display: grid;
    grid-template-rows: auto 1fr auto;
    gap: 4px;
    background: var(--surface-2);
    border-right: 1px solid var(--border);
    padding: 10px 7px;
    min-height: 0;
  }
  .brand {
    width: 28px;
    height: 28px;
    margin: 1px auto 6px;
    border-radius: 9px;
    background: linear-gradient(140deg, var(--accent), var(--accent-strong));
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.22);
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-height: 0;
    overflow-y: auto;
    scrollbar-width: none;
  }
  .group::-webkit-scrollbar {
    display: none;
  }
  .item {
    position: relative;
    display: grid;
    justify-items: center;
    gap: 2px;
    border: none;
    background: transparent;
    color: var(--text-faint);
    border-radius: 9px;
    padding: 5px 2px 4px;
    transition: background 0.14s ease, color 0.14s ease;
  }
  .item .label {
    font-size: 8.5px;
    font-weight: 600;
    letter-spacing: 0.01em;
    line-height: 1.05;
  }
  .item:hover {
    background: var(--surface-3);
    color: var(--text-dim);
  }
  .item.active {
    color: var(--accent);
    background: var(--accent-soft);
  }
  .item.active::before {
    content: '';
    position: absolute;
    left: -7px;
    top: 50%;
    transform: translateY(-50%);
    width: 3px;
    height: 22px;
    border-radius: 0 3px 3px 0;
    background: var(--accent);
  }

  @media (max-width: 560px) {
    .rail {
      grid-template-rows: none;
      grid-template-columns: auto 1fr auto;
      border-right: none;
      border-bottom: 1px solid var(--border);
    }
    .group {
      flex-direction: row;
      overflow-x: auto;
    }
    .item .label {
      display: none;
    }
    .item.active::before {
      left: 50%;
      top: auto;
      bottom: -6px;
      transform: translateX(-50%);
      width: 22px;
      height: 3px;
    }
  }
</style>
