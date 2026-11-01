<script lang="ts" module>
  import type { IconName } from '../lib/icons';

  export interface MenuItem {
    id: string;
    label: string;
    icon?: IconName;
    kbd?: string;
    danger?: boolean;
    separatorBefore?: boolean;
    run: () => void;
  }
</script>

<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Icon from './Icon.svelte';
  import { pop } from '../lib/motion';

  // Small popover menu: roving focus, Esc/outside-click to dismiss, restores focus on close.
  let {
    items,
    onclose,
    label,
  }: { items: MenuItem[]; onclose: (restoreFocus: boolean) => void; label: string } = $props();

  let el: HTMLElement | undefined = $state();

  function buttons(): HTMLButtonElement[] {
    return Array.from(el?.querySelectorAll<HTMLButtonElement>('button[role="menuitem"]') ?? []);
  }

  function onKey(e: KeyboardEvent) {
    const list = buttons();
    const cur = list.findIndex((b) => b === document.activeElement);
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onclose(true);
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      list[(cur + 1) % list.length]?.focus();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      list[(cur - 1 + list.length) % list.length]?.focus();
    } else if (e.key === 'Tab') {
      onclose(false);
    }
  }

  function choose(it: MenuItem) {
    onclose(true);
    it.run();
  }

  onMount(() => {
    void tick().then(() => buttons()[0]?.focus());
    const away = (e: PointerEvent) => {
      if (el && !el.contains(e.target as Node)) onclose(false);
    };
    // capture so it still runs when a child stops propagation
    window.addEventListener('pointerdown', away, true);
    return () => window.removeEventListener('pointerdown', away, true);
  });
</script>

<div
  class="menu"
  role="menu"
  aria-label={label}
  tabindex="-1"
  bind:this={el}
  onkeydown={onKey}
  transition:pop|global={{ duration: 200, y: -6, from: 0.95 }}>
  {#each items as it (it.id)}
    {#if it.separatorBefore}<hr class="divider" />{/if}
    <button type="button" role="menuitem" class:danger={it.danger} onclick={() => choose(it)}>
      {#if it.icon}<Icon name={it.icon} size={15} />{:else}<span class="ph"></span>{/if}
      <span class="lbl">{it.label}</span>
      {#if it.kbd}<kbd>{it.kbd}</kbd>{/if}
    </button>
  {/each}
</div>

<style>
  .menu {
    position: absolute;
    z-index: 30;
    top: calc(100% + 8px);
    right: 0;
    min-width: 224px;
    padding: 5px;
    border-radius: var(--r-3);
    background: var(--pop-bg);
    border: 1px solid var(--line-strong);
    box-shadow: var(--shadow-pop);
    -webkit-backdrop-filter: blur(24px) saturate(1.6);
    backdrop-filter: blur(24px) saturate(1.6);
    transform-origin: 90% 0;
  }
  button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 34px;
    padding: 0 10px;
    border-radius: var(--r-2);
    color: var(--text);
    font-size: var(--t-sm);
    font-weight: 550;
    text-align: left;
    transition: background var(--dur-1);
  }
  button:hover,
  button:focus-visible {
    background: var(--surface-2);
    outline: none;
  }
  button:focus-visible {
    box-shadow: inset 0 0 0 1.5px var(--accent);
  }
  button.danger:hover {
    color: var(--danger);
  }
  .lbl {
    flex: 1;
  }
  .ph {
    width: 15px;
  }
  .divider {
    margin: 5px 6px;
  }
</style>
