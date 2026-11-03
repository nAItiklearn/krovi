<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import AskPanel from '../circle/AskPanel.svelte';
  import AwarePanel from '../circle/AwarePanel.svelte';
  import { isTauri } from '../lib/bridge';
  import { circleHeld, startCircle } from '../lib/circle';

  // Circle to Ask: the shortcut (or the button) dims the screen; drag around anything and Krovi
  // reads just that area once, on this PC. The circled result appears here. Screen awareness is a
  // separate, user-started, time-limited session with an always-visible indicator.
  const demo = !isTauri();
  let error = $state<string | null>(null);

  async function begin() {
    error = null;
    try {
      await startCircle();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }
</script>

<div class="view" role="presentation">
  <header class="view-head">
    <p class="lede">
      Drag around anything on screen and Krovi reads that area on this PC: copy the text, search it, summarise it, or ask
      your own AI provider. Only the area you select is captured, once, and the picture is never saved.
    </p>
    <button class="btn primary" onclick={begin} disabled={demo}><Icon name="search" size={14} /> Circle something</button>
  </header>
  {#if demo}
    <p class="note">Circle to Ask reads your screen, so it only runs in the Krovi desktop app.</p>
  {/if}
  {#if error}<p class="note bad" role="alert">{error}</p>{/if}

  {#if $circleHeld}
    <AskPanel />
  {:else}
    <p class="note">Tip: press <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>C</kbd> anywhere to start.</p>
  {/if}

  <AwarePanel />
</div>

<style>
  .note {
    margin: 10px 0;
    color: var(--text-2);
    font-size: 0.85rem;
  }
  .note.bad {
    color: var(--danger);
  }
  kbd {
    padding: 1px 6px;
    border: 1px solid var(--line-2, var(--line));
    border-radius: 6px;
    font: inherit;
    font-size: 0.78rem;
  }
</style>
