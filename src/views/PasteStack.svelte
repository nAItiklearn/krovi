<script lang="ts">
  import { onMount } from 'svelte';
  import { bridge } from '../lib/bridge';
  import Icon from '../components/Icon.svelte';

  const MAX = 9;
  let items = $state<string[]>([]); // index 0 = slot 1 (top)
  let draft = $state('');
  let copiedSlot = $state<number | null>(null);

  function push() {
    const v = draft.trim();
    if (!v) return;
    items = [v, ...items].slice(0, MAX);
    draft = '';
  }

  function pop() {
    items = items.slice(1);
  }

  function removeAt(i: number) {
    items = items.filter((_, idx) => idx !== i);
  }

  async function copyAt(i: number) {
    try {
      await navigator.clipboard?.writeText(items[i]);
    } catch {
      /* clipboard may be blocked in the demo */
    }
    copiedSlot = i + 1;
    setTimeout(() => (copiedSlot = null), 1100);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter') push();
  }

  onMount(async () => {
    items = await bridge.loadPasteStack();
  });
</script>

<div class="head">
  <div>
    <p class="eyebrow"><Icon name="paste" size={13} /> Paste stack</p>
    <p class="muted sub">Actively push copies onto numbered slots (1–9) and pop them in order — distinct from the passive clipboard history.</p>
  </div>
  <button class="btn sm" onclick={pop} disabled={items.length === 0}>
    <Icon name="chevronRight" size={15} /> Pop top
  </button>
</div>

<div class="pusher">
  <input class="input" placeholder="Type something to push onto slot 1…" bind:value={draft} onkeydown={onKey} />
  <button class="btn accent" onclick={push} disabled={!draft.trim()}>
    <Icon name="plus" size={16} /> Push
  </button>
</div>

<div class="slots">
  {#each Array(MAX) as _, i (i)}
    {@const val = items[i]}
    <div class="slot" class:filled={!!val}>
      <span class="num">{i + 1}</span>
      {#if val}
        <span class="val">{val}</span>
        <div class="actions">
          <button class="mini" title="Copy" onclick={() => copyAt(i)}>
            <Icon name={copiedSlot === i + 1 ? 'check' : 'copy'} size={14} />
          </button>
          <button class="mini" title="Remove" onclick={() => removeAt(i)}>
            <Icon name="trash" size={14} />
          </button>
        </div>
      {:else}
        <span class="val faint">empty</span>
      {/if}
    </div>
  {/each}
</div>

<style>
  .head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 14px;
    margin-bottom: 14px;
  }
  .sub {
    font-size: 12.5px;
    max-width: 60ch;
    margin: 0;
  }
  .pusher {
    display: flex;
    gap: 9px;
    margin-bottom: 16px;
  }
  .pusher .input {
    flex: 1;
  }
  .slots {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    gap: 10px;
  }
  .slot {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 11px 13px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--surface-2);
  }
  .slot.filled {
    border-color: var(--border);
  }
  .num {
    width: 26px;
    height: 26px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 7px;
    font-weight: 700;
    font-size: 13px;
    background: var(--surface-3);
    color: var(--text-faint);
    font-family: var(--mono);
  }
  .slot.filled .num {
    background: var(--accent);
    color: var(--accent-contrast);
  }
  .val {
    flex: 1;
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .actions {
    display: flex;
    gap: 2px;
  }
  .mini {
    border: none;
    background: transparent;
    color: var(--text-faint);
    padding: 5px;
    border-radius: 7px;
  }
  .mini:hover {
    color: var(--text);
    background: var(--surface-3);
  }
</style>
