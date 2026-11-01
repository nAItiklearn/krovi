<script lang="ts">
  import { rollIn, rollOut } from '../lib/motion';

  // Odometer-style text: only the glyphs that changed roll; everything else stays still.
  // Use with tabular numerals so the width never jitters.
  let { text }: { text: string } = $props();

  const chars = $derived(text.split(''));
</script>

<span class="roll num" role="img" aria-label={text}>
  {#each chars as c, i (i)}
    <span class="slot" aria-hidden="true">
      {#key c}
        <span class="ch" in:rollIn out:rollOut>{c}</span>
      {/key}
    </span>
  {/each}
</span>

<style>
  .roll {
    display: inline-flex;
    white-space: pre;
    line-height: 1.15;
  }
  .slot {
    display: inline-grid;
    overflow: hidden;
    /* padding gives the blur + overshoot room inside the clip */
    padding: 0.06em 0;
    margin: -0.06em 0;
  }
  .ch {
    grid-area: 1 / 1;
    text-align: center;
  }
</style>
