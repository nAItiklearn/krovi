<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import State from '../components/State.svelte';
  import { bridge } from '../lib/bridge';
  import { copyText, native, pushToast } from '../lib/stores';
  import { spotlight } from '../lib/motion';

  const MAX = 9;
  let items = $state<string[]>([]); // index 0 = slot 1 (top of the stack)
  let draft = $state('');
  let status = $state<'loading' | 'ready' | 'error'>('loading');
  let copiedSlot = $state<number | null>(null);

  async function load() {
    status = 'loading';
    try {
      items = await bridge.loadPasteStack();
      status = 'ready';
    } catch {
      status = 'error';
    }
  }

  async function push() {
    const v = draft.trim();
    if (!v) return;
    draft = '';
    const res = await native<string[]>('push_paste', { value: v });
    items = res ?? [v, ...items].slice(0, MAX);
  }

  async function pop() {
    const top = items[0];
    if (top === undefined) return;
    await copyText(top);
    const res = await native<string[]>('pop_paste');
    items = res ?? items.slice(1);
    pushToast('Popped to clipboard');
  }

  let flash: ReturnType<typeof setTimeout> | null = null;
  async function copyAt(i: number) {
    await copyText(items[i]);
    copiedSlot = i;
    pushToast(`Slot ${i + 1} copied`);
    if (flash) clearTimeout(flash);
    flash = setTimeout(() => (copiedSlot = null), 1200);
  }

  onMount(load);
</script>

<div class="view">
  <div class="toolbar">
    <p class="faint">Queue up to nine snippets, then paste them back in order. Pop copies the top slot and removes it.</p>
    <button class="btn" onclick={pop} disabled={items.length === 0}><Icon name="copy" size={14} /> Pop top</button>
  </div>

  <form class="pusher" onsubmit={(e) => { e.preventDefault(); void push(); }}>
    <input class="input" placeholder="Type or paste something to push onto slot 1" bind:value={draft} aria-label="New snippet" />
    <button class="btn primary" type="submit" disabled={!draft.trim()}><Icon name="plus" size={15} /> Push</button>
  </form>

  {#if status === 'loading'}
    <div class="slots">
      {#each Array(6) as _, i (i)}<div class="skel" style="height:58px;border-radius:14px"></div>{/each}
    </div>
  {:else if status === 'error'}
    <State tone="error" title={'Couldn’t load the stack'} text="Something went wrong reading your saved snippets." action={{ label: 'Try again', run: load }} />
  {:else if items.length === 0}
    <State icon="paste" title="The stack is empty" text="Push a snippet above. Slot 1 is the next thing you will paste." />
  {:else}
    <ul class="slots" aria-label="Paste slots">
      {#each Array(MAX) as _, i (i)}
        {@const val = items[i]}
        <li>
          {#if val !== undefined}
            <button class="slot card filled" onclick={() => copyAt(i)} use:spotlight aria-label={`Copy slot ${i + 1}: ${val.slice(0, 50)}`}>
              <span class="num-badge">{i + 1}</span>
              <span class="val ellipsis">{val}</span>
              <span class="cp"><Icon name={copiedSlot === i ? 'check' : 'copy'} size={14} /></span>
            </button>
          {:else}
            <div class="slot empty-slot">
              <span class="num-badge dim">{i + 1}</span>
              <span class="val faint">Empty</span>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 14px;
    font-size: var(--t-sm);
    flex: none;
  }
  .toolbar p {
    max-width: 52ch;
    line-height: 1.45;
  }
  .pusher {
    display: flex;
    gap: 8px;
    margin-bottom: 16px;
    flex: none;
  }
  .slots {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 10px;
    align-content: start;
  }
  .slot {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 58px;
    padding: 0 12px;
    border-radius: var(--r-3);
  }
  .empty-slot {
    border: 1px dashed var(--line-strong);
    border-radius: var(--r-3);
    opacity: 0.7;
  }
  .num-badge {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    flex: none;
    border-radius: 9px;
    font-weight: 700;
    font-size: var(--t-sm);
    font-variant-numeric: tabular-nums;
    background: var(--invert-bg);
    color: var(--invert-fg);
  }
  .num-badge.dim {
    background: var(--surface-2);
    color: var(--text-3);
  }
  .val {
    flex: 1;
    font-size: var(--t-sm);
    font-weight: 500;
  }
  .cp {
    color: var(--text-3);
    opacity: 0;
    transition: opacity var(--dur-1);
  }
  .slot:hover .cp {
    opacity: 1;
  }
</style>
