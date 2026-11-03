<script lang="ts">
  import { onMount } from 'svelte';
  import Segmented from '../components/Segmented.svelte';
  import SnippetsTab from './extras/SnippetsTab.svelte';
  import OrganizeTab from './extras/OrganizeTab.svelte';
  import WorkflowsTab from './extras/WorkflowsTab.svelte';
  import SearchTab from './extras/SearchTab.svelte';
  import { isTauri } from '../lib/bridge';
  import { extrasIntent, takeExtrasIntent, type ExtrasIntent, type ExtrasTab } from '../lib/extrasNav';

  // Extras: four small, local tools that do only what you ask, when you ask.
  //   Snippets   saved text you copy on purpose (no keyboard hook, nothing watches your typing)
  //   Organize   rules -> Preview -> Confirm -> Run -> Undo, moves only, only your known folders
  //   Workflows  named lists of Agent Mode commands, run with the same permission rules
  //   Search     file and folder NAMES in your known folders: no index, no background work
  // In the browser demo everything on screen is made up and labelled DEMO.

  const demo = !isTauri();

  const TABS: { id: ExtrasTab; label: string }[] = [
    { id: 'snippets', label: 'Snippets' },
    { id: 'organize', label: 'Organize' },
    { id: 'workflows', label: 'Workflows' },
    { id: 'search', label: 'Search' },
  ];

  let tab = $state<ExtrasTab>('snippets');
  /** A one-time request from the palette; the section it is for takes it and calls `consumed`. */
  let intent = $state<ExtrasIntent | null>(null);

  function consumed() {
    intent = null;
  }

  onMount(() => {
    // The palette hands over an intent (open Workflows and run one, open Search with a query...).
    // Subscribing also delivers one that was left before this view opened.
    return extrasIntent.subscribe((pending) => {
      if (!pending) return;
      const taken = takeExtrasIntent();
      if (!taken) return;
      tab = taken.tab;
      intent = taken;
    });
  });
</script>

<div class="view">
  <header class="view-head">
    <div>
      <h2>Extras</h2>
      <p>Snippets, a file organizer, workflows and file search. Everything stays on this PC and nothing runs by itself.</p>
    </div>
    <Segmented size="sm" label="Extras section" options={TABS} bind:value={tab} />
  </header>

  {#if demo}
    <div class="demo" role="note">
      <span class="tag accent">DEMO</span>
      <span>Made-up snippets, files, folders and results for this preview. Nothing on your computer is read, moved or opened.</span>
    </div>
  {/if}

  <div class="pane">
    {#if tab === 'snippets'}
      <SnippetsTab {demo} />
    {:else if tab === 'organize'}
      <OrganizeTab {demo} />
    {:else if tab === 'workflows'}
      <WorkflowsTab {demo} {intent} onconsumed={consumed} />
    {:else}
      <SearchTab {demo} {intent} onconsumed={consumed} />
    {/if}
  </div>
</div>

<style>
  .demo {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 12px;
    padding: 8px 12px;
    border-radius: var(--r-2);
    border: 1px dashed color-mix(in srgb, var(--accent) 55%, var(--line));
    background: var(--accent-soft);
    color: var(--text);
    font-size: var(--t-xs);
    line-height: 1.4;
  }
  .pane {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
</style>
