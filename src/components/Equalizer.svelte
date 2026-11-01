<script lang="ts">
  // Four-bar "now playing" indicator. Pure CSS motion: transform-only, so it costs ~nothing.
  let { playing = true, height = 14 }: { playing?: boolean; height?: number } = $props();
</script>

<span class="eq" class:paused={!playing} style:height="{height}px" aria-hidden="true">
  <i></i><i></i><i></i><i></i>
</span>

<style>
  .eq {
    display: inline-flex;
    align-items: flex-end;
    gap: 2px;
  }
  i {
    display: block;
    width: 2.5px;
    height: 100%;
    border-radius: 2px;
    background: var(--accent);
    transform-origin: bottom;
    animation: eq 0.9s var(--ease-in-out) infinite alternate;
  }
  i:nth-child(1) {
    animation-duration: 0.74s;
    animation-delay: -0.2s;
  }
  i:nth-child(2) {
    animation-duration: 0.93s;
    animation-delay: -0.5s;
  }
  i:nth-child(3) {
    animation-duration: 0.61s;
    animation-delay: -0.1s;
  }
  i:nth-child(4) {
    animation-duration: 0.83s;
    animation-delay: -0.7s;
  }
  .paused i {
    animation-play-state: paused;
    transform: scaleY(0.25);
  }
  @keyframes eq {
    from {
      transform: scaleY(0.22);
    }
    to {
      transform: scaleY(1);
    }
  }
</style>
