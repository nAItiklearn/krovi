<script lang="ts">
  // Browser-demo wallpaper: quiet monochrome silk with fine contour lines, so the glass has
  // something to refract. Not rendered in the native app (the real desktop is the backdrop)
  // and not with ?embed=1 (the website supplies its own).
  const contours = Array.from({ length: 11 }, (_, i) => {
    const y = 470 + i * 30;
    const k = i * 7;
    return `M -80 ${y + 160} C 260 ${y - 40 - k}, 560 ${y + 230 + k}, 960 ${y + 40} S 1480 ${y - 150 - k}, 1700 ${y - 20}`;
  });
</script>

<div class="backdrop" aria-hidden="true">
  <div class="sky"></div>
  <svg class="silk" viewBox="0 0 1600 1000" preserveAspectRatio="xMidYMid slice">
    <defs>
      <linearGradient id="bd-a" x1="0" y1="0" x2="1" y2="0.6">
        <stop offset="0" stop-color="var(--bd-r1)" stop-opacity="0" />
        <stop offset="0.5" stop-color="var(--bd-r1)" stop-opacity="1" />
        <stop offset="1" stop-color="var(--bd-r1)" stop-opacity="0" />
      </linearGradient>
      <linearGradient id="bd-b" x1="1" y1="0" x2="0" y2="1">
        <stop offset="0" stop-color="var(--bd-r2)" stop-opacity="0" />
        <stop offset="0.5" stop-color="var(--bd-r2)" stop-opacity="1" />
        <stop offset="1" stop-color="var(--bd-r2)" stop-opacity="0" />
      </linearGradient>
    </defs>
    <g class="soft">
      <path
        d="M-200 820 C 160 460, 540 1010, 900 600 S 1420 160, 1800 300 L1800 560 C 1450 430, 1180 860, 800 1040 S 140 1100, -200 1020 Z"
        fill="url(#bd-a)" />
      <path
        d="M-200 360 C 220 160, 580 560, 960 340 S 1400 -40, 1800 70 L1800 250 C 1420 190, 1160 520, 820 600 S 220 400, -200 560 Z"
        fill="url(#bd-b)" />
    </g>
    <g class="lines">
      {#each contours as d, i (i)}
        <path {d} style:opacity={0.35 + (i % 4) * 0.18} />
      {/each}
    </g>
  </svg>
  <div class="vignette"></div>
  <div class="grain"></div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    overflow: hidden;
    pointer-events: none;
    /* colours come from the --wall-* tokens so every theme paints its own wallpaper */
    --bd-top: var(--wall-top);
    --bd-mid: var(--wall-mid);
    --bd-bot: var(--wall-bot);
    --bd-glow: var(--wall-glow);
    --bd-r1: var(--wall-r1);
    --bd-r2: var(--wall-r2);
    --bd-line: var(--wall-line);
    --bd-vig: var(--wall-vig);
  }
  .sky {
    position: absolute;
    inset: 0;
    background:
      radial-gradient(1000px 560px at 50% -4%, var(--bd-glow), transparent 70%),
      linear-gradient(168deg, var(--bd-top) 0%, var(--bd-mid) 50%, var(--bd-bot) 100%);
  }
  .silk {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .soft {
    opacity: 0.42;
    filter: blur(38px);
  }
  .lines path {
    fill: none;
    stroke: var(--bd-line);
    stroke-width: 1;
  }
  .vignette {
    position: absolute;
    inset: 0;
    background: radial-gradient(130% 100% at 50% 20%, transparent 55%, var(--bd-vig) 100%);
  }
  .grain {
    position: absolute;
    inset: 0;
    opacity: 0.07;
    mix-blend-mode: overlay;
    background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='200' height='200'><filter id='n'><feTurbulence type='fractalNoise' baseFrequency='.8' numOctaves='2' stitchTiles='stitch'/><feColorMatrix values='0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 .7 0'/></filter><rect width='100%' height='100%' filter='url(%23n)'/></svg>");
  }
</style>
