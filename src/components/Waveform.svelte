<script lang="ts">
  // Gentle voice-activity waveform: seven bars breathing out of phase.
  let { height = 16 }: { height?: number } = $props();
</script>

<span class="wave" style:height="{height}px" aria-hidden="true">
  {#each [0, 1, 2, 3, 4, 5, 6] as i (i)}
    <i style:--i={i}></i>
  {/each}
</span>

<style>
  .wave {
    display: inline-flex;
    align-items: center;
    gap: 2.5px;
  }
  i {
    display: block;
    width: 2.5px;
    height: 100%;
    border-radius: 3px;
    background: var(--accent);
    transform-origin: center;
    animation: breathe 1.1s var(--ease-in-out) infinite alternate;
    animation-delay: calc(var(--i) * -0.17s);
    animation-duration: calc(0.8s + var(--i) * 0.07s);
  }
  @keyframes breathe {
    from {
      transform: scaleY(0.18);
      opacity: 0.55;
    }
    to {
      transform: scaleY(1);
      opacity: 1;
    }
  }
</style>
