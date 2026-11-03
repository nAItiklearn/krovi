<script lang="ts">
  import Icon from './Icon.svelte';
  import AnimatedNumber from './AnimatedNumber.svelte';
  import RollingText from './RollingText.svelte';
  import Equalizer from './Equalizer.svelte';
  import Waveform from './Waveform.svelte';
  import { swap } from '../lib/motion';
  import { clockText, freeDisk, memPercent, snippet, type PillMode } from './pillModel';
  import type { SystemStats } from '../lib/live';
  import type { Activity, LiveTimer } from '../lib/stores';
  import type { DictationFace } from '../lib/dictation';

  // The idle face of Krovi: a calm, glanceable strip. All sizing is derived from the
  // measured content (see onmeasure) so the Island's spring always chases the real width.
  let {
    mode,
    stats,
    playing,
    hover,
    activity,
    timer,
    voice,
    partial,
    voiceError = '',
    showMic = false,
    dictation = null,
    action = null,
    onmeasure,
  }: {
    mode: PillMode;
    stats: SystemStats | null;
    playing: boolean;
    hover: boolean;
    activity: Activity | null;
    timer: LiveTimer | null;
    voice: 'listening' | 'processing' | 'error' | null;
    partial: string;
    voiceError?: string;
    /** reserve room on the right for the mic button the Island overlays */
    showMic?: boolean;
    /** global dictation (local Whisper): what to draw while it listens / works / has something to say */
    dictation?: DictationFace | null;
    /** the button the Island overlays on the right during dictation: cancel, or copy the blocked text */
    action?: 'cancel' | 'copy' | null;
    onmeasure: (w: number) => void;
  } = $props();

  const mem = $derived(memPercent(stats));
  const disk = $derived(freeDisk(stats));

  function measure(node: HTMLElement) {
    const report = () => onmeasure(node.offsetWidth);
    report();
    const ro = new ResizeObserver(report);
    ro.observe(node);
    return {
      destroy() {
        ro.disconnect();
      },
    };
  }

  const actIcon = $derived(activity?.kind === 'copy' ? 'copy' : activity?.kind === 'timer' ? 'bell' : 'sparkle');
</script>

{#key mode}
  <span class="row" class:hover class:mic={showMic || action === 'cancel'} class:copy={action === 'copy'} use:measure in:swap={{ y: 6, duration: 260, delay: 50 }}>
    {#if mode === 'voice'}
      {#if dictation}
        {#if dictation.kind === 'notice'}
          <span class="act-ico" class:bad={dictation.tone !== 'info'}><Icon name={dictation.tone === 'info' ? 'micOff' : 'alert'} size={14} /></span>
          <span class="strong">{dictation.label}</span>
          {#if dictation.detail}<span class="soft ellipsis snip">{snippet(dictation.detail, dictation.copy ? 24 : 38)}</span>{/if}
        {:else}
          <span class="act-ico"><Icon name="mic" size={14} /></span>
          <span class="strong">{dictation.label}</span>
          {#if dictation.kind === 'listening'}
            <!-- level-driven: only a transform changes, so it costs no layout -->
            <span class="meter" aria-hidden="true"><i style:transform="scaleX({dictation.level})"></i></span>
          {:else}
            <span class="spin" aria-hidden="true"><Icon name="loader" size={13} /></span>
          {/if}
        {/if}
      {:else if voice === 'error'}
        <span class="act-ico bad"><Icon name="alert" size={14} /></span>
        <span class="strong">Voice input failed</span>
        {#if voiceError}<span class="soft ellipsis snip">{snippet(voiceError, 38)}</span>{/if}
      {:else}
        <Waveform height={14} />
        <span class="strong">{voice === 'processing' ? 'Processing' : 'Listening'}</span>
        {#if partial}<span class="soft ellipsis part">{snippet(partial, 26)}</span>{/if}
      {/if}
    {:else if mode === 'activity' && activity}
      <span class="act-ico"><Icon name={actIcon} size={14} /></span>
      <span class="strong" class:cut={!!activity.chip}>{activity.kind === 'copy' ? 'Copied' : activity.text}</span>
      {#if activity.kind === 'copy'}
        <span class="soft ellipsis snip">{snippet(activity.text, 40)}</span>
      {:else if activity.detail}
        <span class="soft">{activity.detail}</span>
      {/if}
      {#if activity.chip}
        <!-- what a voice command used as "this": quiet, but always visible so nothing is read invisibly -->
        <span class="ctx" title={activity.chip.title}>
          <Icon name={activity.chip.icon} size={11} /><span class="ellipsis">{activity.chip.label}</span>
        </span>
      {/if}
    {:else}
      {#if mem !== null}
        <span class="seg" aria-hidden="true">
          <Icon name="ram" size={13} />
          <span class="lbl">RAM</span>
          <AnimatedNumber value={mem} format={(n) => `${Math.round(n)}%`} />
        </span>
        {#if disk}
          <span class="dot" aria-hidden="true"></span>
          <span class="seg" aria-hidden="true">
            <Icon name="disk" size={13} />
            <span class="lbl">{stats?.diskLabel || 'C:'}</span>
            {#if disk.unit === 'GB'}
              <AnimatedNumber value={disk.value} format={(n) => `${Math.round(n)} GB`} />
            {:else}
              <span class="num">{disk.text}</span>
            {/if}
            <span class="lbl">free</span>
          </span>
        {/if}
      {:else}
        <span class="brand">Krovi</span>
      {/if}
      {#if timer}
        <span class="dot" aria-hidden="true"></span>
        <span class="seg timer" aria-hidden="true">
          <Icon name="timers" size={13} />
          {#if hover}<span class="lbl">{timer.label}</span>{/if}
          <RollingText text={clockText(timer.remainingMs)} />
        </span>
      {/if}
      {#if playing}
        <span class="dot" aria-hidden="true"></span>
        <Equalizer height={13} />
      {/if}
    {/if}
  </span>
{/key}

<style>
  .row {
    display: inline-flex;
    align-items: center;
    gap: 9px;
    height: 100%;
    padding: 0 14px;
    color: var(--pill-fg);
    font-size: 12.5px;
    font-weight: 600;
    letter-spacing: -0.005em;
    white-space: nowrap;
    width: max-content;
  }
  .seg {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .seg :global(.ic) {
    color: var(--pill-fg-2);
  }
  .lbl {
    display: none;
    color: var(--pill-fg-2);
    font-weight: 500;
    animation: lbl-in 0.32s var(--ease-out) both;
  }
  .hover .lbl {
    display: inline;
  }
  @keyframes lbl-in {
    from {
      opacity: 0;
      filter: blur(3px);
    }
  }
  .dot {
    width: 3px;
    height: 3px;
    border-radius: 50%;
    background: var(--pill-fg-2);
    opacity: 0.55;
  }
  .brand {
    letter-spacing: 0.04em;
    font-weight: 650;
  }
  .row.mic {
    padding-right: 42px;
  }
  .row.copy {
    padding-right: 80px;
  }
  /* dictation: a short level bar (transform only) and a spinner that exists only while it works */
  .meter {
    flex: none;
    width: 38px;
    height: 4px;
    border-radius: 999px;
    overflow: hidden;
    background: color-mix(in srgb, var(--pill-fg) 18%, transparent);
  }
  .meter i {
    display: block;
    width: 100%;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
    transform-origin: left center;
    transition: transform 110ms var(--ease-out);
  }
  .spin {
    display: grid;
    place-items: center;
    color: var(--pill-fg-2);
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .act-ico.bad {
    color: var(--danger);
  }
  .strong {
    font-weight: 650;
  }
  .soft {
    color: var(--pill-fg-2);
    font-weight: 500;
  }
  .part {
    max-width: 130px;
    font-style: italic;
  }
  .snip {
    max-width: 210px;
  }
  .strong.cut {
    max-width: 176px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ctx {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-width: 152px;
    height: 18px;
    padding: 0 7px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--pill-fg) 14%, transparent);
    color: var(--pill-fg);
    font-size: 11px;
    font-weight: 600;
  }
  .ctx :global(.ic) {
    flex: none;
    color: var(--pill-fg-2);
  }

  .act-ico {
    display: grid;
    place-items: center;
    color: var(--accent);
  }
  .timer :global(.roll) {
    color: var(--pill-fg);
  }
</style>
