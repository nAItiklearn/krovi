<script lang="ts">
  import { icons, type IconName } from '../lib/icons';

  // One icon family, one stroke weight: the stroke is non-scaling (see base.css `.ic *`),
  // so a 13px glyph and a 40px glyph are drawn with the same pen.
  let { name, size = 18 }: { name: IconName; size?: number } = $props();

  const markup = $derived(icons[name] ?? '');
</script>

<svg
  class="ic"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.6"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false">
  {@html markup}
</svg>
