<script lang="ts">
  import { tick, untrack } from 'svelte';
  import {
    addItem,
    applyShortcut,
    backspaceAtStart,
    deleteAtEnd,
    enter,
    parseBody,
    pasteText,
    serializeBlocks,
    setDone,
    toggleKind,
    type Block,
    type Edit,
    type Focus,
  } from '../lib/checklist';

  // A note is plain markdown on disk, but edited as lines: free text and real checklist items
  // side by side. Each line is its own textarea so the caret model stays native; the pure edit
  // rules (Enter, Backspace, "[] ", paste) live in lib/checklist.ts and are unit-tested.
  let {
    body,
    noteId,
    onchange,
  }: {
    body: string;
    noteId: number;
    /** Called with the canonical markdown. `immediate` = persist now (checkbox toggles), not debounced. */
    onchange: (body: string, immediate: boolean) => void;
  } = $props();

  let blocks = $state<Block[]>(parseBody(untrack(() => body)));
  let lastOut = untrack(() => serializeBlocks(blocks));
  let lastId = untrack(() => noteId);
  let activeIndex = $state<number | null>(null);
  let popId = $state<number | null>(null);
  const els = new Map<number, HTMLTextAreaElement>();

  // Adopt the body from outside (another note selected, a voice edit) but never fight our own echo.
  $effect(() => {
    const id = noteId;
    const incoming = body;
    untrack(() => {
      if (id !== lastId || incoming !== lastOut) {
        blocks = parseBody(incoming);
        lastOut = incoming;
        lastId = id;
        activeIndex = null;
      }
    });
  });

  function fit(el: HTMLTextAreaElement) {
    el.style.height = '0px';
    el.style.height = `${el.scrollHeight}px`;
  }

  function reg(node: HTMLTextAreaElement, arg: { id: number; text: string }) {
    els.set(arg.id, node);
    fit(node);
    return {
      update(next: { id: number; text: string }) {
        els.set(next.id, node);
        fit(node);
      },
      destroy() {
        els.delete(arg.id);
      },
    };
  }

  async function focusAt(f: Focus) {
    await tick();
    const id = blocks[f.index]?.id;
    const el = id !== undefined ? els.get(id) : undefined;
    if (!el) return;
    el.focus();
    const c = Math.max(0, Math.min(f.caret, el.value.length));
    el.setSelectionRange(c, c);
  }

  function emit(immediate: boolean) {
    lastOut = serializeBlocks(blocks);
    onchange(lastOut, immediate);
  }
  function apply(e: Edit, immediate = false) {
    blocks = e.blocks;
    emit(immediate);
    void focusAt(e.focus);
  }

  async function toggle(i: number) {
    const b = blocks[i];
    if (!b || b.kind !== 'todo') return;
    popId = null;
    blocks = setDone(blocks, i, !b.done);
    emit(true); // a tick is a deliberate act: save it now, not after the typing debounce
    await tick();
    popId = b.id;
    setTimeout(() => {
      if (popId === b.id) popId = null;
    }, 700);
  }

  function onInput(i: number, e: Event & { currentTarget: HTMLTextAreaElement }) {
    const el = e.currentTarget;
    const v = el.value;
    if (v.includes('\n')) {
      apply(pasteText(blocks, i, 0, blocks[i].text.length, v));
      return;
    }
    const sc = applyShortcut(blocks, i, v);
    if (sc) {
      apply(sc);
      return;
    }
    blocks = blocks.map((b, k) => (k === i ? { ...b, text: v } : b));
    emit(false);
    fit(el);
  }

  function onKey(i: number, e: KeyboardEvent & { currentTarget: HTMLTextAreaElement }) {
    if (e.isComposing) return;
    const el = e.currentTarget;
    const s = el.selectionStart;
    const end = el.selectionEnd;
    const mod = e.ctrlKey || e.metaKey;

    if (e.key === 'Enter' && mod) {
      // Ctrl+Enter ticks the item you are typing in
      if (blocks[i].kind === 'todo') {
        e.preventDefault();
        void toggle(i);
      }
    } else if (e.key === 'Enter' && !e.altKey) {
      e.preventDefault();
      apply(enter(blocks, i, s));
    } else if (e.key === 'Backspace' && s === 0 && end === 0) {
      const r = backspaceAtStart(blocks, i);
      if (r) {
        e.preventDefault();
        apply(r);
      }
    } else if (e.key === 'Delete' && s === el.value.length && end === s) {
      const r = deleteAtEnd(blocks, i);
      if (r) {
        e.preventDefault();
        apply(r);
      }
    } else if (e.key === 'ArrowUp' && s === 0 && end === 0 && i > 0) {
      e.preventDefault();
      void focusAt({ index: i - 1, caret: blocks[i - 1].text.length });
    } else if (e.key === 'ArrowDown' && s === el.value.length && end === s && i < blocks.length - 1) {
      e.preventDefault();
      void focusAt({ index: i + 1, caret: 0 });
    } else if (mod && e.shiftKey && e.code === 'Digit7') {
      e.preventDefault();
      apply(toggleKind(blocks, i));
    }
  }

  function onPaste(i: number, e: ClipboardEvent & { currentTarget: HTMLTextAreaElement }) {
    const text = e.clipboardData?.getData('text/plain') ?? '';
    if (!text.includes('\n')) return; // single-line paste stays native (undo-friendly)
    e.preventDefault();
    const el = e.currentTarget;
    apply(pasteText(blocks, i, el.selectionStart, el.selectionEnd, text));
  }

  /** Toolbar: turn the empty line you are on into a checklist item, or add one below. */
  export function addChecklistItem() {
    apply(addItem(blocks, activeIndex));
  }
  /** Put the caret in the first line (used right after "New note"). */
  export function focusFirst() {
    void focusAt({ index: 0, caret: 0 });
  }
  /** Progress for the header chip. */
  export function counts() {
    const todos = blocks.filter((b) => b.kind === 'todo');
    return { done: todos.filter((b) => b.done).length, total: todos.length };
  }
</script>

<div class="ed" role="group" aria-label="Note">
  {#each blocks as b, i (b.id)}
    <div class="blk" class:todo={b.kind === 'todo'} class:done={b.kind === 'todo' && b.done}>
      {#if b.kind === 'todo'}
        <button
          type="button"
          class="cb"
          class:pop={popId === b.id}
          role="checkbox"
          aria-checked={b.done}
          aria-label={b.text.trim() || 'Empty checklist item'}
          onclick={() => toggle(i)}>
          <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
            <path class="tick" d="M3.6 8.6l3 3 5.8-6.4" pathLength="1" />
          </svg>
        </button>
      {/if}
      <textarea
        rows="1"
        value={b.text}
        use:reg={{ id: b.id, text: b.text }}
        placeholder={blocks.length === 1 && i === 0 && b.text === '' ? 'Start typing. Type [] for a checklist.' : ''}
        aria-label={b.kind === 'todo' ? 'Checklist item' : 'Note text'}
        spellcheck="true"
        onfocus={() => (activeIndex = i)}
        oninput={(e) => onInput(i, e)}
        onkeydown={(e) => onKey(i, e)}
        onpaste={(e) => onPaste(i, e)}></textarea>
    </div>
  {/each}
</div>

<style>
  .ed {
    display: flex;
    flex-direction: column;
    padding: 6px 10px 18px 0;
  }
  .blk {
    display: flex;
    align-items: flex-start;
    gap: 11px;
    min-height: 27px;
  }
  textarea {
    flex: 1;
    min-width: 0;
    display: block;
    resize: none;
    overflow: hidden;
    border: 0;
    padding: 1px 0;
    background: transparent;
    color: var(--text);
    font-size: 15px;
    line-height: 25px;
    letter-spacing: -0.003em;
    outline: none;
    transition:
      color 0.35s var(--ease-out),
      text-decoration-color 0.35s var(--ease-out);
    text-decoration: line-through transparent;
    text-decoration-thickness: 1.5px;
  }
  textarea::placeholder {
    color: var(--text-3);
  }
  .done textarea {
    color: var(--text-3);
    text-decoration-color: var(--text-3);
  }

  .cb {
    position: relative;
    flex: none;
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    margin-top: 3px;
    border-radius: 6px;
    border: 1.5px solid var(--line-strong);
    background: transparent;
    color: var(--invert-fg);
    transition:
      background 0.25s var(--ease-out),
      border-color 0.25s var(--ease-out),
      transform 0.45s var(--spring);
  }
  .cb:hover {
    border-color: var(--text-2);
    background: var(--surface-2);
  }
  .cb:active {
    transform: scale(0.88);
    transition-duration: 0.06s;
  }
  .cb[aria-checked='true'] {
    background: var(--invert-bg);
    border-color: var(--invert-bg);
  }
  .tick {
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-dasharray: 1;
    stroke-dashoffset: 1;
    transition: stroke-dashoffset 0.28s var(--ease-out);
  }
  .cb[aria-checked='true'] .tick {
    stroke-dashoffset: 0;
    transition-delay: 0.05s;
  }
  /* the satisfying part: the box squashes, springs past 1, and a soft ring leaves it */
  .cb.pop {
    animation: cb-pop 0.5s var(--ease-out);
  }
  .cb.pop::after {
    content: '';
    position: absolute;
    inset: -2px;
    border-radius: 8px;
    pointer-events: none;
    animation: cb-ring 0.6s var(--ease-out) both;
  }
  @keyframes cb-pop {
    0% {
      transform: scale(0.82);
    }
    55% {
      transform: scale(1.14);
    }
    100% {
      transform: scale(1);
    }
  }
  @keyframes cb-ring {
    from {
      box-shadow: 0 0 0 0 var(--accent-glow);
      opacity: 1;
    }
    to {
      box-shadow: 0 0 0 9px transparent;
      opacity: 0;
    }
  }
</style>
