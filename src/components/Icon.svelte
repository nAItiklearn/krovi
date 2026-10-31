<script lang="ts">
  import { icons } from '../lib/icons';

  let {
    name,
    size = 20,
    stroke = 1.75,
    fill = 'none',
  }: { name: string; size?: number; stroke?: number; fill?: string } = $props();

  const markup = $derived(icons[name] ?? '');
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill={fill}
  stroke="currentColor"
  stroke-width={stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false">
  {@html markup}
</svg>
