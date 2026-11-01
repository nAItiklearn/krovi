<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';
  import type { IconName } from '../lib/icons';

  // Empty and error states share one layout so first-run screens feel designed, not blank.
  let {
    icon = 'inbox',
    title,
    text = '',
    tone = 'empty',
    action,
    compact = false,
    children,
  }: {
    icon?: IconName;
    title: string;
    text?: string;
    tone?: 'empty' | 'error';
    action?: { label: string; run: () => void };
    compact?: boolean;
    children?: Snippet;
  } = $props();
</script>

<div class="state" class:error={tone === 'error'} class:compact role={tone === 'error' ? 'alert' : 'status'}>
  <div class="glyph"><Icon name={tone === 'error' ? 'alert' : icon} size={20} /></div>
  <h2>{title}</h2>
  {#if text}<p>{text}</p>{/if}
  {#if action}
    <button class="btn" onclick={action.run}>{action.label}</button>
  {/if}
  {@render children?.()}
</div>

<style>
  .state:not(.compact) {
    flex: 1;
  }
  .compact {
    min-height: 0;
    padding: 14px 12px;
  }
  .compact .glyph {
    width: 36px;
    height: 36px;
    margin-bottom: 4px;
  }
  .compact h2 {
    font-size: var(--t-md);
  }
  .compact p {
    font-size: var(--t-xs);
  }
</style>
