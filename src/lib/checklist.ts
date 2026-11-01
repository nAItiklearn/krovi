// Inline checklists for notes: the note body stays plain markdown (`- [ ] item` / `- [x] item`),
// which is what the Rust store persists. This module is the pure model behind the editor:
// body <-> blocks, and every keyboard edit (Enter, Backspace, the `[] ` shortcut, paste) as a
// small function that returns the new blocks plus where the caret goes. No DOM in here.

export type BlockKind = 'text' | 'todo';

export interface Block {
  /** stable identity for keyed rendering; not persisted */
  id: number;
  kind: BlockKind;
  text: string;
  done: boolean;
}

export interface Focus {
  index: number;
  caret: number;
}

export interface Edit {
  blocks: Block[];
  focus: Focus;
}

let seq = 0;
export const nextBlockId = (): number => ++seq;

const mk = (kind: BlockKind, text = '', done = false): Block => ({ id: nextBlockId(), kind, text, done: kind === 'todo' && done });

/** A stored checklist line: `- [ ] text`, `- [x] text`, also `*`/`+` bullets and a bare `[ ] `. */
const TODO_LINE = /^[ \t]*(?:[-*+][ \t]+)?\[([ xX]?)\](?:[ \t]|$)(.*)$/;
/** What the user types to start a list: `[] `, `[ ] `, `- [ ] `, `- [] `, `* [x] `... (needs the trailing space). */
const TODO_SHORTCUT = /^(?:[-*+][ \t]+)?\[([ xX]?)\][ \t](.*)$/;

const norm = (s: string): string => s.replace(/\r\n?/g, '\n');

/** Split a note body into editor blocks. Always returns at least one block. */
export function parseBody(body: string): Block[] {
  const blocks = norm(body)
    .split('\n')
    .map((line): Block => {
      const m = TODO_LINE.exec(line);
      return m ? mk('todo', m[2], m[1].toLowerCase() === 'x') : mk('text', line);
    });
  return blocks.length ? blocks : [mk('text')];
}

/** Canonical markdown for the blocks: the inverse of `parseBody` for canonical input. */
export function serializeBlocks(blocks: Block[]): string {
  return blocks.map((b) => (b.kind === 'todo' ? `- [${b.done ? 'x' : ' '}] ${b.text}` : b.text)).join('\n');
}

/** Flip the checkbox on line `line` of a raw body, leaving every other character alone. */
export function toggleLine(body: string, line: number): string {
  const lines = norm(body).split('\n');
  const raw = lines[line];
  if (raw === undefined) return body;
  const m = /^([ \t]*(?:[-*+][ \t]+)?\[)([ xX]?)(\])/.exec(raw);
  if (!m) return body;
  const next = m[2].toLowerCase() === 'x' ? ' ' : 'x';
  lines[line] = m[1] + next + raw.slice(m[0].length - 1);
  return lines.join('\n');
}

/** If `text` starts with a list marker ("[] ", "- [ ] ", "[x] "...), the item it turns into. */
export function shortcutFor(text: string): { done: boolean; rest: string } | null {
  const m = TODO_SHORTCUT.exec(text);
  return m ? { done: m[1].toLowerCase() === 'x', rest: m[2] } : null;
}

/** `done / total` over the checklist items in a body (plain notes give total 0). */
export function stats(body: string): { done: number; total: number } {
  let done = 0;
  let total = 0;
  for (const line of norm(body).split('\n')) {
    const m = TODO_LINE.exec(line);
    if (m) {
      total++;
      if (m[1].toLowerCase() === 'x') done++;
    }
  }
  return { done, total };
}

/** A line with its list marker removed: what the note list shows as title / excerpt. */
export function plainLine(line: string): string {
  const m = TODO_LINE.exec(line);
  return (m ? m[2] : line).trim();
}

/** First non-empty line, marker stripped. */
export function noteTitle(body: string): string {
  for (const line of norm(body).split('\n')) {
    const t = plainLine(line);
    if (t) return t;
  }
  return '';
}

/** Everything after the title line, flattened for a one-line excerpt. */
export function noteExcerpt(body: string): string {
  const lines = norm(body).split('\n');
  const first = lines.findIndex((l) => plainLine(l));
  if (first < 0) return '';
  return lines
    .slice(first + 1)
    .map(plainLine)
    .filter(Boolean)
    .join(' · ');
}

// ---------------------------------------------------------------------------------------
// editing operations
// ---------------------------------------------------------------------------------------

const replaceAt = (blocks: Block[], i: number, b: Block): Block[] => blocks.map((x, k) => (k === i ? b : x));

/**
 * Enter at `caret`: split the line. On an EMPTY checklist item it ends the list instead
 * (the item becomes plain text), exactly like Notion / Keep.
 */
export function enter(blocks: Block[], index: number, caret: number): Edit {
  const b = blocks[index];
  if (b.kind === 'todo' && b.text === '') {
    return { blocks: replaceAt(blocks, index, { ...b, kind: 'text', done: false }), focus: { index, caret: 0 } };
  }
  const at = Math.max(0, Math.min(caret, b.text.length));
  const head: Block = { ...b, text: b.text.slice(0, at) };
  const tail = mk(b.kind, b.text.slice(at));
  const out = [...blocks.slice(0, index), head, tail, ...blocks.slice(index + 1)];
  return { blocks: out, focus: { index: index + 1, caret: 0 } };
}

/**
 * Backspace with the caret at the very start: a checklist item turns back into text; a text
 * line joins the line above. Returns null when there is nothing to do (first line, plain text).
 */
export function backspaceAtStart(blocks: Block[], index: number): Edit | null {
  const b = blocks[index];
  if (b.kind === 'todo') {
    return { blocks: replaceAt(blocks, index, { ...b, kind: 'text', done: false }), focus: { index, caret: 0 } };
  }
  if (index === 0) return null;
  const prev = blocks[index - 1];
  const merged: Block = { ...prev, text: prev.text + b.text };
  return {
    blocks: [...blocks.slice(0, index - 1), merged, ...blocks.slice(index + 1)],
    focus: { index: index - 1, caret: prev.text.length },
  };
}

/** Delete with the caret at the very end: pull the next line up into this one. */
export function deleteAtEnd(blocks: Block[], index: number): Edit | null {
  if (index >= blocks.length - 1) return null;
  const b = blocks[index];
  const next = blocks[index + 1];
  return {
    blocks: [...blocks.slice(0, index), { ...b, text: b.text + next.text }, ...blocks.slice(index + 2)],
    focus: { index, caret: b.text.length },
  };
}

/** Typing `[] ` at the start of a text line turns it into a real checklist item. */
export function applyShortcut(blocks: Block[], index: number, value: string): Edit | null {
  const b = blocks[index];
  if (b.kind !== 'text') return null;
  const sc = shortcutFor(value);
  if (!sc) return null;
  return {
    blocks: replaceAt(blocks, index, { ...b, kind: 'todo', text: sc.rest, done: sc.done }),
    focus: { index, caret: 0 },
  };
}

/** Ctrl+Shift+7: flip a line between text and checklist item (keeping its text). */
export function toggleKind(blocks: Block[], index: number): Edit {
  const b = blocks[index];
  const next: Block = b.kind === 'todo' ? { ...b, kind: 'text', done: false } : { ...b, kind: 'todo', done: false };
  return { blocks: replaceAt(blocks, index, next), focus: { index, caret: next.text.length } };
}

export function setDone(blocks: Block[], index: number, done: boolean): Block[] {
  const b = blocks[index];
  return b && b.kind === 'todo' ? replaceAt(blocks, index, { ...b, done }) : blocks;
}

/**
 * "Add checklist item": an empty text line becomes the item; otherwise a new item is appended
 * after `index` (or at the end when nothing is focused).
 */
export function addItem(blocks: Block[], index: number | null): Edit {
  if (index !== null && blocks[index] && blocks[index].kind === 'text' && blocks[index].text === '') {
    return { blocks: replaceAt(blocks, index, { ...blocks[index], kind: 'todo', done: false }), focus: { index, caret: 0 } };
  }
  const at = index === null ? blocks.length - 1 : index;
  const item = mk('todo');
  return {
    blocks: [...blocks.slice(0, at + 1), item, ...blocks.slice(at + 1)],
    focus: { index: at + 1, caret: 0 },
  };
}

/** Paste multi-line text at the caret: each line becomes a block, list markers become items. */
export function pasteText(blocks: Block[], index: number, start: number, end: number, pasted: string): Edit {
  const b = blocks[index];
  const head = b.text.slice(0, start);
  const tail = b.text.slice(end);
  const lines = norm(pasted).split('\n');
  if (lines.length === 1) {
    const text = head + lines[0] + tail;
    return { blocks: replaceAt(blocks, index, { ...b, text }), focus: { index, caret: head.length + lines[0].length } };
  }
  const first: Block = { ...b, text: head + lines[0] };
  const middle = lines.slice(1, -1).map((l) => parseBody(l)[0]);
  const lastParsed = parseBody(lines[lines.length - 1])[0];
  const last: Block = { ...lastParsed, text: lastParsed.text + tail };
  return {
    blocks: [...blocks.slice(0, index), first, ...middle, last, ...blocks.slice(index + 1)],
    focus: { index: index + lines.length - 1, caret: lastParsed.text.length },
  };
}
