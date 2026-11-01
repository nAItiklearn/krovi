<script lang="ts">
  import { untrack } from 'svelte';
  import { Tween } from 'svelte/motion';
  import { cubicOut } from 'svelte/easing';
  import { reduced } from '../lib/motion';

  // Tweened number: smoothly counts to the new value instead of snapping.
  let {
    value,
    format = (n: number) => String(Math.round(n)),
    duration = 700,
  }: { value: number; format?: (n: number) => string; duration?: number } = $props();

  const tween = new Tween(
    untrack(() => value),
    { easing: cubicOut },
  );

  $effect(() => {
    const v = value;
    tween.set(v, { duration: reduced() ? 0 : untrack(() => duration) });
  });
</script>

<span class="num">{format(tween.current)}</span>
