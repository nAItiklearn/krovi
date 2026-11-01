<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import { closeVoiceSheets, pushToast, voiceHelpOpen, voiceResults, type VoiceSheet } from '../lib/stores';
  import { openVoiceResult, type VoiceResultItem } from '../lib/voice';
  import { resultIcon, resultVerb, resultsHeadline } from './voiceModel';
  import { pop } from '../lib/motion';

  // What a voice command found or listed: search hits, a folder's newest items, the candidates of
  // an ambiguous app name. Rust already did the command (executed), so rows only offer the next step:
  // a click opens the folder, shows the file in Explorer (never runs it) or launches the app.
  let { sheet }: { sheet: VoiceSheet } = $props();

  let busy = $state<string | null>(null);
  let opened = $state<Record<string, boolean>>({});
  let listEl: HTMLElement | undefined = $state();
  let opener: Element | null = null;

  const headline = $derived(resultsHeadline(sheet.feedback, sheet.rows.length));
  const count = $derived(`${sheet.rows.length} ${sheet.rows.length === 1 ? 'item' : 'items'}`);

  async function open(row: VoiceResultItem) {
    if (busy) return;
    busy = row.path;
    try {
      pushToast((await openVoiceResult(row.path)) || 'Opened', 2400);
      opened[row.path] = true;
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 3400, 'error');
    } finally {
      busy = null;
    }
  }

  function help() {
    voiceResults.set(null);
    voiceHelpOpen.set(true);
  }

  function onKey(e: KeyboardEvent) {
    const d = e.key === 'ArrowDown' ? 1 : e.key === 'ArrowUp' ? -1 : 0;
    if (!d || !listEl) return;
    e.preventDefault();
    const rows = [...listEl.querySelectorAll<HTMLElement>('.row')];
    const i = rows.indexOf(document.activeElement as HTMLElement);
    rows[Math.max(0, Math.min(rows.length - 1, i + d))]?.focus();
  }

  onMount(() => {
    opener = document.activeElement;
    // the panel may still be opening (voice started from the pill): keep trying until a row can take focus
    let tries = 0;
    const focusTimer = setInterval(() => {
      const first = listEl?.querySelector<HTMLElement>('.row');
      first?.focus({ preventScroll: true });
      if ((first && document.activeElement === first) || ++tries > 24) clearInterval(focusTimer);
    }, 50);
    return () => {
      clearInterval(focusTimer);
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
  <div class="sheet" role="dialog" aria-modal="true" aria-label="Voice results" transition:pop|global={{ duration: 280, from: 0.95, y: -10 }}>
    <header class="sheet-head">
      <span class="badge" aria-hidden="true"><Icon name="mic" size={15} /></span>
      <div class="ht">
        <strong>{headline}</strong>
        <span class="faint">{count}</span>
      </div>
      <button class="btn icon sm ghost" type="button" aria-label="Close results" onclick={closeVoiceSheets}>
        <Icon name="close" size={15} />
      </button>
    </header>

    <div class="sheet-body scroll" role="presentation" bind:this={listEl} onkeydown={onKey}>
    <ul aria-label="Results">
      {#each sheet.rows as row (row.path)}
        <li>
          <button class="row" type="button" disabled={busy === row.path} onclick={() => open(row)}>
            <span class="ico"><Icon name={resultIcon(row)} size={16} /></span>
            <span class="tx">
              <span class="nm ellipsis" title={row.path}>{row.name}</span>
              {#if row.detail}<span class="dt faint ellipsis">{row.detail}</span>{/if}
            </span>
            <span class="go">
              <span class="verb">{resultVerb(row.kind)}</span>
              {#if opened[row.path]}
                <Icon name="check" size={14} />
              {:else if busy === row.path}
                <Icon name="loader" size={14} />
              {:else}
                <Icon name="externalLink" size={14} />
              {/if}
            </span>
          </button>
        </li>
      {/each}
    </ul>
    </div>

    <footer class="sheet-foot">
      <span>Folders open, files are shown in Explorer and never run.</span>
      <button class="linkbtn" type="button" onclick={help}>What can I say?</button>
    </footer>
  </div>
</div>

<style>
  .sheet {
    width: min(600px, 100%);
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
    font-size: var(--t-md);
    font-weight: 620;
    letter-spacing: -0.01em;
    line-height: 1.35;
  }
  .ht span {
    font-size: var(--t-xs);
  }
  ul {
    display: grid;
    align-content: start;
    gap: 2px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 52px;
    padding: 6px 10px;
    border-radius: var(--r-3);
    text-align: left;
    color: var(--text);
    transition:
      background var(--dur-1),
      transform 0.45s var(--spring);
  }
  .row:hover,
  .row:focus-visible {
    background: var(--surface-2);
    outline: none;
    box-shadow: inset 0 0 0 1px var(--line);
  }
  .row:active {
    transform: scale(0.99);
    transition-duration: 0.08s;
  }
  .row:disabled {
    opacity: 0.6;
  }
  .ico {
    display: grid;
    place-items: center;
    flex: none;
    width: 34px;
    height: 34px;
    border-radius: 10px;
    background: var(--surface-1);
    border: 1px solid var(--line);
    color: var(--text-2);
  }
  .tx {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 1px;
  }
  .nm {
    font-size: var(--t-md);
    font-weight: 560;
  }
  .dt {
    font-size: var(--t-xs);
  }
  .go {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex: none;
    color: var(--text-3);
  }
  .verb {
    font-size: var(--t-xs);
    font-weight: 600;
    opacity: 0;
    transform: translateX(4px);
    transition:
      opacity var(--dur-2),
      transform 0.4s var(--spring);
  }
  .row:hover .verb,
  .row:focus-visible .verb {
    opacity: 1;
    transform: none;
  }
  .row:hover .go,
  .row:focus-visible .go {
    color: var(--text);
  }
</style>
