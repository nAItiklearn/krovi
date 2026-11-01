<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import State from '../components/State.svelte';
  import Equalizer from '../components/Equalizer.svelte';
  import { media, mediaPosition, mediaTransport } from '../lib/live';
  import { now } from '../lib/stores';

  const m = $derived($media);
  // position interpolates locally between the 5s resyncs from the OS media session
  const pos = $derived(mediaPosition(m, $now));
  const pct = $derived(m && m.durationSec > 0 ? Math.min(100, (pos / m.durationSec) * 100) : 0);
  const fmt = (s: number) => {
    const t = Math.max(0, Math.floor(s));
    return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`;
  };
  const appName = (s: string) => s.replace(/\.exe$/i, '').replace(/^\w/, (c) => c.toUpperCase());
</script>

<div class="view">
  {#if m}
    <div class="stage">
      <div class="artcol">
        <div class="glow" aria-hidden="true">
          {#if m.artwork}<img src={m.artwork} alt="" />{/if}
        </div>
        <div class="art" class:playing={m.playing}>
          {#if m.artwork}
            <img src={m.artwork} alt={`Artwork for ${m.title}`} />
          {:else}
            <Icon name="disc" size={64} />
          {/if}
        </div>
      </div>

      <div class="info">
        <span class="eyebrow">
          {#if m.playing}<Equalizer height={12} />{:else}<Icon name="pause" size={12} />{/if}
          {m.playing ? 'Now playing' : 'Paused'} · {appName(m.sourceApp)}
        </span>
        <h2 class="title" title={m.title}>{m.title}</h2>
        <p class="artist">{m.artist || 'Unknown artist'}</p>
        {#if m.album}<p class="album faint">{m.album}</p>{/if}

        <div class="prog">
          <div class="bar" role="progressbar" aria-label="Track position" aria-valuemin="0" aria-valuemax={Math.round(m.durationSec)} aria-valuenow={Math.round(pos)}>
            <i style:width="{pct}%"></i>
          </div>
          <div class="times num faint"><span>{fmt(pos)}</span><span>{m.durationSec > 0 ? fmt(m.durationSec) : '--:--'}</span></div>
        </div>

        <div class="ctl">
          <button class="btn-round" aria-label="Previous track" onclick={() => mediaTransport('previous')}>
            <Icon name="prev" size={22} />
          </button>
          <button class="btn-round big" aria-label={m.playing ? 'Pause' : 'Play'} onclick={() => mediaTransport('playpause')}>
            <Icon name={m.playing ? 'pause' : 'play'} size={24} />
          </button>
          <button class="btn-round" aria-label="Next track" onclick={() => mediaTransport('next')}>
            <Icon name="next" size={22} />
          </button>
        </div>
      </div>
    </div>
  {:else}
    <div class="empty">
      <State
        icon="headphones"
        title="Nothing playing"
        text="Play music or a video in Spotify, your browser or any media app. Krovi reads the system media session, so controls work for whatever is active." />
    </div>
  {/if}
</div>

<style>
  .stage {
    flex: 1;
    display: grid;
    grid-template-columns: 280px 1fr;
    align-items: center;
    gap: 40px;
    padding: 0 12px;
  }
  .artcol {
    position: relative;
    display: grid;
    place-items: center;
  }
  .glow {
    position: absolute;
    inset: -10px;
    border-radius: 40px;
    overflow: hidden;
    opacity: 0.55;
    filter: blur(34px) saturate(1.4);
    background: radial-gradient(circle, var(--surface-3), transparent 70%);
  }
  .glow img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    transform: scale(1.15);
  }
  .art {
    position: relative;
    display: grid;
    place-items: center;
    width: 260px;
    height: 260px;
    border-radius: 26px;
    overflow: hidden;
    color: var(--text-3);
    background:
      radial-gradient(circle at 30% 18%, var(--surface-3), transparent 70%),
      linear-gradient(145deg, var(--surface-2), var(--surface-1));
    border: 1px solid var(--line-strong);
    box-shadow:
      var(--shadow-art-lg),
      inset 0 1px 0 var(--card-hi);
    transition: transform 0.7s var(--spring);
  }
  .art:not(.playing) {
    transform: scale(0.94);
  }
  .art img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .info {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .eyebrow {
    margin-bottom: 8px;
  }
  .title {
    font-family: var(--font-display);
    font-size: 30px;
    font-weight: 680;
    letter-spacing: -0.03em;
    line-height: 1.12;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .artist {
    font-size: var(--t-xl);
    letter-spacing: -0.015em;
    color: var(--text-2);
  }
  .album {
    font-size: var(--t-sm);
  }
  .prog {
    margin-top: 26px;
  }
  .bar {
    height: 5px;
    border-radius: 5px;
    background: var(--surface-3);
    overflow: hidden;
  }
  .bar i {
    display: block;
    height: 100%;
    border-radius: 5px;
    background: var(--text);
    transition: width 1s linear;
  }
  .times {
    display: flex;
    justify-content: space-between;
    margin-top: 8px;
    font-size: var(--t-xs);
  }
  .ctl {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-top: 14px;
  }
  .ctl :global(.btn-round) {
    width: 46px;
    height: 46px;
  }
  .ctl :global(.btn-round.big) {
    width: 60px;
    height: 60px;
  }
  .empty {
    flex: 1;
    display: grid;
    place-items: center;
  }
</style>
