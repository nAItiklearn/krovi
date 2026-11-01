<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import { closeVoiceSheets } from '../lib/stores';
  import { getVoiceTargets, type VoiceTargets } from '../lib/voice';
  import { HELP_FAMILIES, HELP_LIMITS, filterTargets, targetsStatus } from './voiceHelp';
  import { pop } from '../lib/motion';

  // "What can I say?" Voice is a closed grammar: it listens only for these phrasings, so this list
  // is the whole contract. The folder and app names are the real ones Windows lists on this PC.
  let targets = $state<VoiceTargets | null>(null);
  let q = $state('');
  let closeEl: HTMLButtonElement | undefined = $state();
  let sheetEl: HTMLElement | undefined = $state();
  let opener: Element | null = null;

  const status = $derived(targetsStatus(targets));
  const view = $derived(filterTargets(targets, q));

  onMount(() => {
    opener = document.activeElement;
    // The palette or the panel may hand focus back as they finish closing/opening: for a moment,
    // keep focus inside the sheet.
    let ticks = 0;
    const focusTimer = setInterval(() => {
      if (!sheetEl?.contains(document.activeElement)) closeEl?.focus({ preventScroll: true });
      if (++ticks > 14) clearInterval(focusTimer);
    }, 60);
    closeEl?.focus({ preventScroll: true });
    let dead = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let tries = 0;
    // the first scan of the Start menu can still be running: poll gently until it finishes
    const load = async () => {
      const t = await getVoiceTargets();
      if (dead) return;
      targets = t;
      if (!t.ready && ++tries < 30) timer = setTimeout(load, 1500);
    };
    void load();
    return () => {
      clearInterval(focusTimer);
      dead = true;
      if (timer) clearTimeout(timer);
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus({ preventScroll: true });
    };
  });
</script>

<div
  class="sheet-overlay"
  role="presentation"
  onpointerdown={(e) => {
    if (e.target === e.currentTarget) closeVoiceSheets();
  }}
  transition:pop|global={{ duration: 180, from: 1, y: 0 }}>
  <div class="sheet" role="dialog" aria-modal="true" aria-label="What can I say?" bind:this={sheetEl} transition:pop|global={{ duration: 280, from: 0.95, y: -10 }}>
    <header class="sheet-head">
      <span class="badge" aria-hidden="true"><Icon name="mic" size={15} /></span>
      <div class="ht">
        <strong>What can I say?</strong>
        <span class="faint">Voice listens for these phrases only. They are matched on this PC.</span>
      </div>
      <button class="btn icon sm ghost" type="button" bind:this={closeEl} aria-label="Close" onclick={closeVoiceSheets}>
        <Icon name="close" size={15} />
      </button>
    </header>

    <div class="sheet-body scroll">
      <ul class="fams">
        {#each HELP_FAMILIES as f (f.id)}
          <li class="fam">
            <div class="fh">
              <span class="ico"><Icon name={f.icon} size={15} /></span>
              <h2>{f.title}</h2>
            </div>
            <code class="tpl" title="The exact grammar: {'{a|b}'} one of, [x] optional, &lt;n&gt; a number, &lt;text&gt; free speech">{f.template}</code>
            <ul class="exs">
              {#each f.examples as e (e)}<li>“{e}”</li>{/each}
            </ul>
            {#if f.note}<p class="note faint">{f.note}</p>{/if}
          </li>
        {/each}
      </ul>

      <section class="targets" aria-label="Your folders and apps">
        <div class="th">
          <div>
            <h2>Your folders and apps</h2>
            <p class="faint">The only names Voice acts on. They come from Windows on this PC.</p>
          </div>
          <input class="input sm-in" type="search" placeholder="Filter" aria-label="Filter folders and apps" bind:value={q} />
        </div>

        {#if status === 'loading'}
          <div class="chips" aria-hidden="true">
            {#each [84, 64, 96, 72, 88, 60] as w, i (i)}<span class="skel" style="width:{w}px;height:26px;border-radius:999px"></span>{/each}
          </div>
          <p class="stat faint" role="status"><Icon name="loader" size={13} /> Still loading your folders and apps…</p>
        {:else}
          <h3 class="eyebrow">Folders</h3>
          <div class="chips">
            {#each view.folders as f (f.path)}
              <span class="chip" title={f.path}><Icon name="folder" size={13} />{f.name}</span>
            {:else}
              <span class="faint none">{q ? 'No folder matches.' : 'No folders yet.'}</span>
            {/each}
          </div>

          <h3 class="eyebrow">Apps</h3>
          {#if status === 'partial'}
            <p class="stat faint" role="status"><Icon name="loader" size={13} /> Still loading: apps appear as the Start menu scan finishes.</p>
          {/if}
          <div class="chips">
            {#each view.apps as a (a)}
              <span class="chip"><Icon name="appWindow" size={13} />{a}</span>
            {:else}
              {#if status === 'ready'}<span class="faint none">{q ? 'No app matches.' : 'No apps found.'}</span>{/if}
            {/each}
            {#if view.moreApps > 0}<span class="faint more">+{view.moreApps} more. Type to filter.</span>{/if}
          </div>
        {/if}
      </section>

      <p class="limits faint"><Icon name="shield" size={13} /> {HELP_LIMITS}</p>
    </div>

    <footer class="sheet-foot">
      <span>Say it, or type the same words in the command palette.</span>
      <span><kbd>Esc</kbd> closes</span>
    </footer>
  </div>
</div>

<style>
  .sheet {
    width: min(700px, 100%);
  }
  .badge {
    display: grid;
    place-items: center;
    flex: none;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: var(--surface-2);
    border: 1px solid var(--line);
    color: var(--text-2);
  }
  .ht {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 2px;
  }
  .ht strong {
    font-size: var(--t-lg);
    font-weight: 650;
    letter-spacing: -0.015em;
  }
  .ht span {
    font-size: var(--t-xs);
  }
  .sheet-body {
    padding: 12px;
  }

  .fams {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
  }
  .fam {
    display: grid;
    align-content: start;
    gap: 9px;
    padding: 12px 13px 13px;
    border-radius: var(--r-3);
    background: var(--surface-1);
    border: 1px solid var(--line);
  }
  .fh {
    display: flex;
    align-items: center;
    gap: 9px;
  }
  .ico {
    display: grid;
    place-items: center;
    flex: none;
    width: 26px;
    height: 26px;
    border-radius: 8px;
    background: var(--surface-2);
    color: var(--text-2);
  }
  .fh h2,
  .th h2 {
    font-size: var(--t-md);
    font-weight: 620;
    letter-spacing: -0.01em;
  }
  .tpl {
    font-family: var(--font-mono);
    font-weight: 400;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-3);
    overflow-wrap: anywhere;
  }
  .exs {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .exs li {
    padding: 4px 10px;
    border-radius: 13px;
    background: var(--surface-2);
    color: var(--text);
    font-size: var(--t-xs);
    font-weight: 560;
    line-height: 1.35;
  }
  .note {
    font-size: var(--t-xs);
    line-height: 1.5;
  }

  .targets {
    margin-top: 12px;
    padding: 13px;
    border-radius: var(--r-3);
    background: var(--surface-1);
    border: 1px solid var(--line);
  }
  .th {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 14px;
    margin-bottom: 10px;
  }
  .th p {
    margin-top: 2px;
    font-size: var(--t-xs);
  }
  .sm-in {
    width: 150px;
    height: 30px;
    flex: none;
  }
  .eyebrow {
    margin: 10px 0 7px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chips .chip {
    height: 26px;
    padding: 0 10px;
  }
  .stat {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 8px 0 2px;
    font-size: var(--t-xs);
  }
  .none,
  .more {
    font-size: var(--t-xs);
    padding: 4px 2px;
  }
  .limits {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 12px 4px 4px;
    font-size: var(--t-xs);
    line-height: 1.5;
  }
  .limits :global(.ic) {
    flex: none;
    margin-top: 2px;
  }
</style>
