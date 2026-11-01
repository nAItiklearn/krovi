<script lang="ts">
  import Icon from './Icon.svelte';
  import { toast } from '../lib/stores';
  import { pop } from '../lib/motion';
</script>

<div class="wrap" role="status" aria-live="polite">
  {#if $toast}
    {#key $toast.id}
      <div class="toast" transition:pop|global={{ duration: 300, from: 0.9, y: 14 }}>
        <Icon name={$toast.tone === 'error' ? 'alert' : 'circleCheck'} size={15} />
        <span>{$toast.text}</span>
      </div>
    {/key}
  {/if}
</div>

<style>
  .wrap {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 18px;
    z-index: 60;
    display: flex;
    justify-content: center;
    pointer-events: none;
  }
  .toast {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    padding: 0 16px 0 12px;
    border-radius: 999px;
    background: var(--invert-bg);
    color: var(--invert-fg);
    font-size: var(--t-sm);
    font-weight: 600;
    box-shadow: var(--shadow-toast);
  }
  .toast :global(.ic) {
    color: var(--invert-fg);
    opacity: 0.8;
  }
</style>
