<script lang="ts" generics="T extends string">
  import Icon from './Icon.svelte';
  import type { IconName } from '../lib/icons';

  // Equal-width segmented control with a thumb that glides (spring easing) between options.
  let {
    options,
    value = $bindable(),
    label,
    onchange,
    size = 'md',
  }: {
    options: { id: T; label: string; icon?: IconName; badge?: number | string }[];
    value: T;
    label: string;
    onchange?: (v: T) => void;
    size?: 'sm' | 'md';
  } = $props();

  const index = $derived(Math.max(0, options.findIndex((o) => o.id === value)));

  function pick(id: T) {
    value = id;
    onchange?.(id);
  }

  function onKey(e: KeyboardEvent) {
    const d = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
    if (!d) return;
    e.preventDefault();
    pick(options[(index + d + options.length) % options.length].id);
  }
</script>

<div
  class="seg"
  class:sm={size === 'sm'}
  role="radiogroup"
  aria-label={label}
  tabindex="-1"
  style:--n={options.length}
  style:--i={index}
  onkeydown={onKey}>
  <span class="thumb" aria-hidden="true"></span>
  {#each options as o (o.id)}
    <button
      type="button"
      role="radio"
      aria-checked={o.id === value}
      tabindex={o.id === value ? 0 : -1}
      class:on={o.id === value}
      onclick={() => pick(o.id)}>
      {#if o.icon}<Icon name={o.icon} size={14} />{/if}
      <span>{o.label}</span>
      {#if o.badge !== undefined}<em class="badge">{o.badge}</em>{/if}
    </button>
  {/each}
</div>

<style>
  .seg {
    position: relative;
    display: inline-grid;
    grid-template-columns: repeat(var(--n), 1fr);
    padding: 3px;
    border-radius: var(--r-2);
    background: var(--surface-1);
    border: 1px solid var(--line);
    isolation: isolate;
  }
  .thumb {
    position: absolute;
    z-index: -1;
    top: 3px;
    bottom: 3px;
    left: 3px;
    width: calc((100% - 6px) / var(--n));
    border-radius: calc(var(--r-2) - 3px);
    background: var(--surface-3);
    box-shadow:
      inset 0 1px 0 var(--card-hi),
      var(--shadow-sm);
    transform: translateX(calc(var(--i) * 100%));
    transition: transform 0.55s var(--spring);
  }
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 28px;
    padding: 0 14px;
    border-radius: calc(var(--r-2) - 3px);
    color: var(--text-2);
    font-size: var(--t-sm);
    font-weight: 600;
    white-space: nowrap;
    transition: color var(--dur-1);
  }
  .sm button {
    height: 24px;
    padding: 0 10px;
    font-size: var(--t-xs);
  }
  button:hover {
    color: var(--text);
  }
  button.on {
    color: var(--text);
  }
  .badge {
    font-style: normal;
    font-size: var(--t-micro);
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }
</style>
