<script lang="ts">
  import { onMount } from 'svelte';
  import IdleCompanion from './IdleCompanion.svelte';
  import {
    IDLE_STYLES,
    PERSONALITIES,
    idleSettings,
    isIdle,
    markActivity,
    type IdleStyleId,
    type PersonalityId,
    type PoseId,
  } from '../../lib/idle';
  import { THEMES, parseChoice, resolvedTheme, themeChoice, type ThemeChoice } from '../../lib/theme';

  // Standalone harness for the idle companion + themes (dev only). Open
  //   /src/components/idle/preview.html                 everything
  //   /src/components/idle/preview.html?perf=on|off    ONE live pill, nothing else (for measuring)
  //   ...?theme=arcade&style=pixel&personality=hyper     preselect
  const q = new URLSearchParams(location.search);
  const perf = q.get('perf');

  const POSES: PoseId[] = ['droop', 'tilt', 'happy', 'perk', 'doze'];
  const asStyle = (v: string | null): IdleStyleId =>
    (IDLE_STYLES.find((s) => s.id === v)?.id ?? 'dot') as IdleStyleId;
  const asPers = (v: string | null): PersonalityId =>
    (PERSONALITIES.find((p) => p.id === v)?.id ?? 'curious') as PersonalityId;

  // harness-only knob for A/B cost experiments: ?css=<url-encoded css> is injected as a <style>
  const extraCss = q.get('css');
  if (extraCss) document.head.append(Object.assign(document.createElement('style'), { textContent: extraCss }));

  if (q.get('theme')) themeChoice.set(parseChoice(q.get('theme')));
  if (perf) {
    idleSettings.set({
      enabled: perf === 'on',
      style: asStyle(q.get('style')),
      personality: asPers(q.get('personality')),
      delayMs: Number(q.get('delay')) || 5500,
    });
  }

  let style = $state<IdleStyleId>(asStyle(q.get('style')));
  let personality = $state<PersonalityId>(asPers(q.get('personality')));
  const choices: ThemeChoice[] = ['system', ...THEMES.map((t) => t.id)];

  // The pose board freezes one pose per pill so every pose can be inspected (and screenshotted).
  let poseHost: HTMLElement | undefined = $state();
  onMount(() => {
    if (perf) return;
    const hold = () => {
      poseHost?.querySelectorAll<HTMLElement>('[data-force]').forEach((cell) => {
        const root = cell.querySelector<HTMLElement>('.idle');
        if (root) {
          root.dataset.pose = cell.dataset.force;
          root.style.setProperty('--side', '1');
        }
      });
    };
    hold();
    const t = setInterval(hold, 300);
    return () => clearInterval(t);
  });

  const stat = 'RAM 62%  ·  C: 214 GB free';
</script>

{#snippet livepill()}
  <!-- the real flow: waits `delayMs`, then the eyes replace the stats; hover/click wakes them -->
  <div
    class="pill live"
    role="presentation"
    onpointerenter={markActivity}
    onpointerleave={markActivity}
    onpointerdown={markActivity}>
    <span class="row" class:hide={$idleSettings.enabled && $isIdle}>{stat}</span>
    <IdleCompanion active />
  </div>
{/snippet}

{#if perf}
  <main class="solo">{@render livepill()}</main>
{:else}
  <div class="page scroll">
    <header>
      <h1>Krovi · idle companion</h1>
      <div class="bar">
        <span class="lbl">Theme</span>
        {#each choices as c (c)}
          <button class="chip" class:on={$themeChoice === c} onclick={() => themeChoice.set(c)}>
            {c === 'system' ? 'System' : THEMES.find((t) => t.id === c)?.name}
          </button>
        {/each}
        <span class="sp"></span>
        <span class="lbl">Style</span>
        {#each IDLE_STYLES as s (s.id)}
          <button class="chip" class:on={style === s.id} onclick={() => (style = s.id)}>{s.name}</button>
        {/each}
        <span class="sp"></span>
        <span class="lbl">Personality</span>
        {#each PERSONALITIES as p (p.id)}
          <button class="chip" class:on={personality === p.id} onclick={() => (personality = p.id)}>{p.name}</button>
        {/each}
      </div>
      <p class="dim">
        resolved theme <b>{$resolvedTheme}</b> · move the pointer near a pill and the eyes glance at it · the live
        pill below uses the real idle clock (<b>{$isIdle ? 'idle' : 'awake'}</b>)
      </p>
    </header>

    <section>
      <h2>Live pill <small>real flow: stats, then eyes after {$idleSettings.delayMs} ms; hover or click wakes it</small></h2>
      <div class="cell">
        <div class="row-ctl">
          <label>
            <input type="checkbox" checked={$idleSettings.enabled} onchange={(e) => idleSettings.update((s) => ({ ...s, enabled: e.currentTarget.checked }))} />
            companion enabled
          </label>
          <button class="chip" onclick={() => idleSettings.update((s) => ({ ...s, style, personality, delayMs: 2000 }))}>use selection, 2 s delay</button>
        </div>
        {@render livepill()}
      </div>
    </section>

    <section>
      <h2>Styles × personalities <small>theme: {$resolvedTheme}</small></h2>
      <div class="grid g4">
        {#each IDLE_STYLES as s (s.id)}
          {#each PERSONALITIES as p (p.id)}
            <div class="cell">
              <div class="pill"><IdleCompanion active preview style={s.id} personality={p.id} /></div>
              <span class="cap">{s.name} · {p.name}</span>
            </div>
          {/each}
        {/each}
      </div>
    </section>

    <section>
      <h2>Poses <small>frozen: {style} · {personality}</small></h2>
      <div class="grid g5" bind:this={poseHost}>
        {#each POSES as pose (pose)}
          <div class="cell" data-force={pose}>
            <div class="pill"><IdleCompanion active preview {style} {personality} /></div>
            <span class="cap">{pose}</span>
          </div>
        {/each}
      </div>
    </section>

    <section>
      <h2>Themes × styles <small>personality: {personality}</small></h2>
      <div class="boards">
        {#each THEMES as t (t.id)}
          <div class="scope" data-theme-scope={t.id}>
            <div class="scope-head"><b>{t.name}</b> <span>{t.scheme}</span></div>
            <div class="grid g6">
              {#each IDLE_STYLES as s (s.id)}
                <div class="pill"><IdleCompanion active preview style={s.id} {personality} /></div>
              {/each}
            </div>
            <div class="swatches">
              <i style:background={t.swatch[0]}></i><i style:background={t.swatch[1]}></i><i style:background={t.swatch[2]}></i>
              <span>Aa text</span><span class="d2">Aa text-2</span><span class="d3">Aa text-3</span>
              <em class="acc">accent</em>
            </div>
          </div>
        {/each}
      </div>
    </section>
  </div>
{/if}

<style>
  .page {
    position: absolute;
    inset: 0;
    padding: 22px 28px 60px;
    background: var(--page);
    color: var(--text);
    font-family: var(--font);
    overflow-y: auto;
  }
  .solo {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: start center;
    padding-top: 10px;
    background: var(--page);
  }
  h1 {
    font-size: 20px;
    font-weight: 650;
    margin: 0 0 12px;
  }
  h2 {
    font-size: 13px;
    font-weight: 650;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-2);
    margin: 28px 0 12px;
  }
  h2 small {
    text-transform: none;
    letter-spacing: 0;
    font-weight: 500;
    color: var(--text-3);
    margin-left: 8px;
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
  }
  .lbl {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.07em;
    color: var(--text-3);
    margin-right: 2px;
  }
  .sp {
    width: 14px;
  }
  .chip {
    height: 26px;
    padding: 0 11px;
    border-radius: 99px;
    font-size: 12.5px;
    background: var(--surface-2);
    color: var(--text-2);
    border: 1px solid var(--line);
  }
  .chip.on {
    background: var(--invert-bg);
    color: var(--invert-fg);
    border-color: transparent;
  }
  .dim {
    color: var(--text-2);
    font-size: 12.5px;
    margin: 12px 0 0;
  }
  .grid {
    display: grid;
    gap: 14px 18px;
  }
  .g4 {
    grid-template-columns: repeat(4, 212px);
  }
  .g5 {
    grid-template-columns: repeat(5, 212px);
  }
  .g6 {
    grid-template-columns: repeat(3, 212px);
    gap: 12px;
  }
  .cell {
    display: grid;
    gap: 6px;
    justify-items: start;
  }
  .cap {
    font-size: 11.5px;
    color: var(--text-3);
  }
  .row-ctl {
    display: flex;
    gap: 14px;
    align-items: center;
    font-size: 12.5px;
    color: var(--text-2);
  }

  /* a pill like the Island's: same size, same shadow, same token */
  .pill {
    position: relative;
    width: 212px;
    height: 36px;
    border-radius: 18px;
    background: var(--pill-bg);
    box-shadow: var(--shadow-pill);
    overflow: hidden;
    color: var(--pill-fg);
  }
  .row {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    font-size: 12.5px;
    font-weight: 600;
    white-space: pre;
    transition: opacity 260ms ease;
  }
  .row.hide {
    opacity: 0;
    transition-duration: 340ms;
  }

  .boards {
    display: flex;
    flex-wrap: wrap;
    gap: 18px;
  }
  .scope {
    padding: 14px;
    border-radius: 16px;
    background: var(--page);
    color: var(--text);
    border: 1px solid var(--line);
    display: grid;
    gap: 12px;
  }
  .scope-head {
    display: flex;
    gap: 8px;
    align-items: baseline;
    font-size: 13px;
  }
  .scope-head span {
    color: var(--text-3);
    font-size: 11px;
  }
  .swatches {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 12px;
  }
  .swatches i {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 1px solid var(--line-strong);
  }
  .d2 {
    color: var(--text-2);
  }
  .d3 {
    color: var(--text-3);
  }
  .acc {
    font-style: normal;
    color: var(--accent);
  }
</style>
