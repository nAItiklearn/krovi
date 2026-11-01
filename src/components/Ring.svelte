<script lang="ts">
  import type { Snippet } from 'svelte';
  import { clamp } from '../lib/motion';

  // Progress ring. The arc animates with a CSS transition, so every ring in the app
  // eases to its value the same way. Monochrome by default; `accent` marks something live.
  let {
    value,
    size = 64,
    stroke = 5,
    tone = 'default',
    label = '',
    children,
  }: {
    value: number;
    size?: number;
    stroke?: number;
    tone?: 'default' | 'accent' | 'dim';
    label?: string;
    children?: Snippet;
  } = $props();

  const r = $derived((size - stroke) / 2);
  const circ = $derived(2 * Math.PI * r);
  const dash = $derived(circ * (1 - clamp(value, 0, 1)));
</script>

<div
  class="ring"
  data-tone={tone}
  style:width="{size}px"
  style:height="{size}px"
  role={label ? 'img' : undefined}
  aria-label={label || undefined}>
  <svg width={size} height={size} viewBox="0 0 {size} {size}" aria-hidden="true">
    <circle class="track" cx={size / 2} cy={size / 2} {r} stroke-width={stroke} fill="none" />
    <circle
      class="arc"
      cx={size / 2}
      cy={size / 2}
      {r}
      stroke-width={stroke}
      fill="none"
      stroke-linecap="round"
      stroke-dasharray={circ}
      stroke-dashoffset={dash}
      transform="rotate(-90 {size / 2} {size / 2})" />
  </svg>
  {#if children}
    <div class="center">{@render children()}</div>
  {/if}
</div>

<style>
  .ring {
    position: relative;
    flex: none;
  }
  svg {
    position: absolute;
    inset: 0;
  }
  .track {
    stroke: var(--surface-3);
  }
  .arc {
    stroke: var(--text);
    transition: stroke-dashoffset 0.9s var(--ease-out);
  }
  [data-tone='accent'] .arc {
    stroke: var(--accent);
  }
  [data-tone='dim'] .arc {
    stroke: var(--text-2);
  }
  .center {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    text-align: center;
  }
</style>
