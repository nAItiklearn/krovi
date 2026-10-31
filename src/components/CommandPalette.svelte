<script lang="ts">
  import { activeTool, paletteOpen, pinned, themePref } from '../lib/stores';
  import { TOOLS } from '../lib/tools';
  import type { ToolId } from '../lib/types';
  import Icon from './Icon.svelte';

  interface Action {
    id: string;
    label: string;
    hint: string;
    icon: string;
    run: () => void;
  }

  const actions: Action[] = [
    ...TOOLS.map((t) => ({
      id: `go:${t.id}`,
      label: `Open ${t.label}`,
      hint: t.hint,
      icon: t.icon,
      run: () => activeTool.set(t.id as ToolId),
    })),
    {
      id: 'act:theme',
      label: 'Cycle theme',
      hint: 'light → dark → auto',
      icon: 'moon',
      run: () => themePref.update((t) => (t === 'light' ? 'dark' : t === 'dark' ? 'auto' : 'light')),
    },
    {
      id: 'act:pin',
      label: 'Toggle pin',
      hint: 'keep the panel open',
      icon: 'pin',
      run: () => pinned.update((p) => !p),
    },
  ];

  let q = $state('');
  let sel = $state(0);
  let inputEl: HTMLInputElement;

  const filtered = $derived(
    actions.filter((a) => {
      const n = q.trim().toLowerCase();
      if (!n) return true;
      return a.label.toLowerCase().includes(n) || a.hint.toLowerCase().includes(n);
    }),
  );

  $effect(() => {
    // keep selection in range when the filter changes
    if (sel >= filtered.length) sel = Math.max(0, filtered.length - 1);
  });

  $effect(() => {
    inputEl?.focus();
  });

  function choose(a: Action) {
    a.run();
    paletteOpen.set(false);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      sel = Math.min(sel + 1, filtered.length - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      sel = Math.max(sel - 1, 0);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const a = filtered[sel];
      if (a) choose(a);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      paletteOpen.set(false);
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="scrim" role="presentation" onclick={() => paletteOpen.set(false)}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="palette"
    role="dialog"
    aria-modal="true"
    aria-label="Command palette"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}>
    <div class="search">
      <Icon name="search" size={18} />
      <input
        bind:this={inputEl}
        bind:value={q}
        class="pinput"
        placeholder="Search tools and actions…"
        aria-label="Command palette search"
        onkeydown={onKey} />
      <kbd>Esc</kbd>
    </div>
    <ul class="list scroll" role="listbox" aria-label="Commands">
      {#each filtered as a, i (a.id)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          role="option"
          aria-selected={i === sel}
          class:selected={i === sel}
          onmousemove={() => (sel = i)}
          onclick={() => choose(a)}>
          <Icon name={a.icon} size={17} />
          <span class="lbl">{a.label}</span>
          <span class="hint faint">{a.hint}</span>
        </li>
      {:else}
        <li class="empty faint">No matching commands</li>
      {/each}
    </ul>
    <div class="foot faint">
      <span><kbd>↑</kbd><kbd>↓</kbd> navigate</span>
      <span><kbd>↵</kbd> run</span>
      <span><kbd>Ctrl</kbd>+<kbd>K</kbd> toggle</span>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgba(8, 12, 15, 0.45);
    backdrop-filter: blur(2px);
    display: grid;
    justify-items: center;
    align-items: start;
    padding-top: 12vh;
    z-index: 50;
  }
  .palette {
    width: min(540px, 92vw);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border);
    color: var(--text-dim);
  }
  .pinput {
    flex: 1;
    border: none;
    background: transparent;
    color: var(--text);
    font-size: 15px;
    outline: none;
    font-family: inherit;
  }
  .list {
    max-height: 46vh;
    margin: 0;
    padding: 6px;
    list-style: none;
  }
  .list li {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 9px 11px;
    border-radius: 9px;
    color: var(--text);
    cursor: pointer;
  }
  .list li.selected {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .list li .lbl {
    font-size: 13.5px;
    font-weight: 550;
  }
  .list li .hint {
    margin-left: auto;
    font-size: 12px;
  }
  .list li.selected .hint {
    color: var(--accent);
    opacity: 0.8;
  }
  .empty {
    justify-content: center;
    cursor: default;
  }
  .foot {
    display: flex;
    gap: 14px;
    padding: 8px 14px;
    border-top: 1px solid var(--border);
    font-size: 11.5px;
  }
  kbd {
    font-family: var(--mono);
    font-size: 10.5px;
    background: var(--surface-3);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 1px 5px;
    margin: 0 1px;
  }
</style>
