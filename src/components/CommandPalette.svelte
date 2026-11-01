<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Icon from './Icon.svelte';
  import { match, segments } from './search';
  import {
    activeTool,
    emitIntent,
    openTool,
    paletteOpen,
    pomodoroStart,
    pushToast,
    voiceHelpOpen,
  } from '../lib/stores';
  import { TOOLS } from '../lib/tools';
  import { mediaTransport } from '../lib/live';
  import { startVoice } from '../lib/voice';
  import { quit } from '../lib/shell';
  import { tryEvaluate } from '../lib/calc';
  import { roundDisplay } from '../lib/units';
  import { pop } from '../lib/motion';
  import type { IconName } from '../lib/icons';

  // Spotlight for Krovi: one input, tools + actions, inline calculator. Everything is local.
  interface Cmd {
    id: string;
    label: string;
    hint: string;
    icon: IconName;
    group: 'Tools' | 'Actions';
    keywords: string[];
    kbd?: string;
    run: () => void;
  }

  const commands: Cmd[] = [
    ...TOOLS.map<Cmd>((t) => ({
      id: `go:${t.id}`,
      label: t.label,
      hint: t.hint,
      icon: t.icon,
      group: 'Tools',
      keywords: t.keywords,
      run: () => openTool(t.id),
    })),
    {
      id: 'pomo',
      label: 'Start a focus session',
      hint: 'Pomodoro, 25 minutes',
      icon: 'pomodoro',
      group: 'Actions',
      keywords: ['pomodoro', 'focus', 'work'],
      run: () => {
        pomodoroStart();
        openTool('pomodoro');
      },
    },
    {
      id: 'playpause',
      label: 'Play / pause media',
      hint: 'Whatever is playing right now',
      icon: 'play',
      group: 'Actions',
      keywords: ['music', 'song', 'spotify'],
      run: () => void mediaTransport('playpause'),
    },
    {
      id: 'next',
      label: 'Next track',
      hint: 'Skip forward',
      icon: 'next',
      group: 'Actions',
      keywords: ['skip', 'music'],
      run: () => void mediaTransport('next'),
    },
    {
      id: 'prev',
      label: 'Previous track',
      hint: 'Skip back',
      icon: 'prev',
      group: 'Actions',
      keywords: ['back', 'music'],
      run: () => void mediaTransport('previous'),
    },
    {
      id: 'voice',
      label: 'Voice command',
      hint: 'Say "open calendar", "start pomodoro"…',
      icon: 'mic',
      group: 'Actions',
      keywords: ['speak', 'listen', 'dictate'],
      run: () => void startVoice('command'),
    },
    {
      id: 'voice-help',
      label: 'What can I say?',
      hint: 'The phrases Voice understands',
      icon: 'circleHelp',
      group: 'Actions',
      keywords: ['voice', 'commands', 'help', 'phrases', 'speak', 'listen'],
      run: () => voiceHelpOpen.set(true),
    },
    {
      id: 'theme',
      label: 'Change appearance',
      hint: 'Themes and the idle companion',
      icon: 'sun',
      group: 'Actions',
      keywords: ['theme', 'dark', 'light', 'mode', 'colour', 'color', 'idle', 'eyes'],
      run: () => openTool('settings'),
    },
    {
      id: 'close',
      label: 'Close panel',
      hint: 'Return to the pill',
      icon: 'close',
      group: 'Actions',
      keywords: ['hide', 'collapse', 'dismiss'],
      kbd: 'Esc',
      run: () => emitIntent('collapse'),
    },
    {
      id: 'quit',
      label: 'Quit Krovi',
      hint: 'Exit completely',
      icon: 'power',
      group: 'Actions',
      keywords: ['exit', 'close app'],
      kbd: 'Ctrl Q',
      run: () => void quit(),
    },
  ];

  let q = $state('');
  let sel = $state(0);
  let inputEl: HTMLInputElement | undefined = $state();
  let listEl: HTMLElement | undefined = $state();
  let opener: Element | null = null;

  interface Row {
    cmd: Cmd;
    score: number;
    ranges: [number, number][];
  }

  const rows = $derived.by<Row[]>(() => {
    const out: Row[] = [];
    for (const cmd of commands) {
      const m = match(q, cmd.label, [cmd.hint, ...cmd.keywords]);
      if (m) out.push({ cmd, score: m.score, ranges: m.ranges });
    }
    if (q.trim()) out.sort((a, b) => b.score - a.score);
    return out;
  });

  // inline calculator: "12*7+3" -> 87 (Enter copies it)
  const calc = $derived.by(() => {
    const s = q.trim();
    if (!s || !/[0-9]/.test(s) || !/[-+*/%^()]/.test(s.replace(/^-/, ''))) return null;
    const r = tryEvaluate(s);
    return r.ok ? String(roundDisplay(r.value, 8)) : null;
  });

  const total = $derived(rows.length + (calc ? 1 : 0));

  $effect(() => {
    // keep the selection in range when the result set changes
    if (sel >= total) sel = Math.max(0, total - 1);
  });

  function close() {
    paletteOpen.set(false);
  }

  async function copyResult(v: string) {
    try {
      await navigator.clipboard?.writeText(v);
      pushToast(`Copied ${v}`);
    } catch {
      pushToast(`= ${v}`);
    }
  }

  function runAt(i: number) {
    if (calc && i === 0) {
      void copyResult(calc);
      close();
      return;
    }
    const r = rows[i - (calc ? 1 : 0)];
    if (!r) return;
    close();
    r.cmd.run();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      sel = total ? (sel + 1) % total : 0;
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      sel = total ? (sel - 1 + total) % total : 0;
    } else if (e.key === 'Enter') {
      e.preventDefault();
      runAt(sel);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
    } else if (e.key === 'Tab') {
      e.preventDefault(); // focus stays in the field; the list is arrow-driven
    }
  }

  $effect(() => {
    void sel;
    void tick().then(() => listEl?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' }));
  });

  onMount(() => {
    opener = document.activeElement;
    // the panel may still be opening (Ctrl+K from the pill): keep trying until the field can take focus
    let tries = 0;
    const focusTimer = setInterval(() => {
      inputEl?.focus({ preventScroll: true });
      if (document.activeElement === inputEl || ++tries > 20) clearInterval(focusTimer);
    }, 50);
    inputEl?.focus({ preventScroll: true });
    return () => {
      clearInterval(focusTimer);
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus({ preventScroll: true });
    };
  });

  const currentId = $derived(`pal-opt-${sel}`);
</script>

<div
  class="overlay"
  role="presentation"
  onpointerdown={(e) => {
    if (e.target === e.currentTarget) close();
  }}
  transition:pop|global={{ duration: 180, from: 1, y: 0 }}>
  <div
    class="box"
    role="dialog"
    aria-modal="true"
    aria-label="Command palette"
    transition:pop|global={{ duration: 280, from: 0.95, y: -10 }}>
    <div class="field">
      <Icon name="search" size={18} />
      <input
        bind:this={inputEl}
        bind:value={q}
        role="combobox"
        aria-expanded="true"
        aria-controls="pal-list"
        aria-activedescendant={total ? currentId : undefined}
        aria-label="Search tools and actions"
        placeholder="Search tools, run an action, or calculate"
        autocomplete="off"
        spellcheck="false"
        onkeydown={onKey} />
      <kbd>Esc</kbd>
    </div>

    <ul class="list scroll" id="pal-list" role="listbox" aria-label="Results" bind:this={listEl}>
      {#if calc}
        <li
          id="pal-opt-0"
          role="option"
          aria-selected={sel === 0}
          class="item calc"
          class:sel={sel === 0}
          onpointerenter={() => (sel = 0)}
          onclick={() => runAt(0)}
          onkeydown={() => {}}>
          <span class="ico"><Icon name="calculator" size={16} /></span>
          <span class="lbl num">= {calc}</span>
          <span class="hint">Copy result</span>
          {#if sel === 0}<kbd><Icon name="enter" size={11} /></kbd>{/if}
        </li>
      {/if}
      {#each rows as r, i (r.cmd.id)}
        {@const idx = i + (calc ? 1 : 0)}
        <li
          id="pal-opt-{idx}"
          role="option"
          aria-selected={sel === idx}
          class="item"
          class:sel={sel === idx}
          class:current={r.cmd.id === `go:${$activeTool}`}
          onpointerenter={() => (sel = idx)}
          onclick={() => runAt(idx)}
          onkeydown={() => {}}>
          <span class="ico"><Icon name={r.cmd.icon} size={16} /></span>
          <span class="lbl">
            {#each segments(r.cmd.label, r.ranges) as s, k (k)}{#if s.hit}<mark>{s.text}</mark>{:else}{s.text}{/if}{/each}
          </span>
          <span class="hint ellipsis">{r.cmd.hint}</span>
          {#if sel === idx}
            <kbd aria-hidden="true"><Icon name="enter" size={11} /></kbd>
          {:else if r.cmd.kbd}
            <kbd aria-hidden="true">{r.cmd.kbd}</kbd>
          {:else}
            <span class="grp">{r.cmd.group === 'Tools' ? 'Tool' : 'Action'}</span>
          {/if}
        </li>
      {/each}
      {#if !total}
        <li class="none" role="presentation">Nothing matches &ldquo;{q}&rdquo;</li>
      {/if}
    </ul>

    <div class="foot" aria-hidden="true">
      <span><kbd><Icon name="arrowUp" size={11} /></kbd><kbd><Icon name="arrowDown" size={11} /></kbd> Navigate</span>
      <span><kbd><Icon name="enter" size={11} /></kbd> Run</span>
      <span class="push"><kbd>Ctrl</kbd><kbd>K</kbd> Toggle</span>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: absolute;
    inset: 0;
    z-index: 50;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 64px;
    background: var(--scrim);
    -webkit-backdrop-filter: blur(10px) saturate(1.2);
    backdrop-filter: blur(10px) saturate(1.2);
    border-radius: var(--r-panel);
  }
    .box {
    width: 580px;
    max-height: 470px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-radius: var(--r-4);
    background: var(--pop-bg);
    border: 1px solid var(--line-strong);
    box-shadow: var(--shadow-pop);
    -webkit-backdrop-filter: blur(28px) saturate(1.7);
    backdrop-filter: blur(28px) saturate(1.7);
  }
  .field {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 16px;
    height: 56px;
    border-bottom: 1px solid var(--line);
    color: var(--text-2);
    flex: none;
  }
  input {
    flex: 1;
    min-width: 0;
    height: 100%;
    border: 0;
    background: transparent;
    color: var(--text);
    font-size: 17px;
    font-weight: 500;
    letter-spacing: -0.01em;
    outline: none;
  }
  input::placeholder {
    color: var(--text-3);
  }
  .list {
    padding: 6px;
    min-height: 0;
    flex: 1;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 44px;
    padding: 0 10px;
    border-radius: var(--r-3);
    color: var(--text);
    cursor: pointer;
    transition: background var(--dur-1);
  }
  .item.sel {
    background: var(--surface-2);
    box-shadow: inset 0 0 0 1px var(--line);
  }
  .ico {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 9px;
    background: var(--surface-1);
    border: 1px solid var(--line);
    color: var(--text-2);
    flex: none;
    transition:
      color var(--dur-1),
      transform 0.45s var(--spring-bouncy);
  }
  .sel .ico {
    color: var(--text);
    transform: scale(1.08);
  }
  .lbl {
    font-size: var(--t-md);
    font-weight: 560;
    white-space: nowrap;
  }
  mark {
    background: none;
    color: var(--text);
    font-weight: 700;
    text-decoration: underline;
    text-decoration-color: var(--accent);
    text-underline-offset: 3px;
    text-decoration-thickness: 2px;
  }
  .hint {
    flex: 1;
    min-width: 0;
    text-align: right;
    font-size: var(--t-xs);
    color: var(--text-3);
  }
  .grp {
    font-size: var(--t-micro);
    font-weight: 600;
    color: var(--text-3);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    min-width: 40px;
    text-align: right;
  }
  .calc .lbl {
    font-size: var(--t-lg);
  }
  .none {
    padding: 28px 12px;
    text-align: center;
    color: var(--text-3);
    font-size: var(--t-sm);
  }
  .foot {
    display: flex;
    gap: 16px;
    align-items: center;
    height: 38px;
    padding: 0 16px;
    border-top: 1px solid var(--line);
    font-size: var(--t-xs);
    color: var(--text-3);
    flex: none;
  }
  .foot span {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .push {
    margin-left: auto;
  }
</style>
