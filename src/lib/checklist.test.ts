import { describe, it, expect } from 'vitest';
import {
  addItem,
  applyShortcut,
  backspaceAtStart,
  deleteAtEnd,
  enter,
  noteExcerpt,
  noteTitle,
  parseBody,
  pasteText,
  plainLine,
  serializeBlocks,
  setDone,
  shortcutFor,
  stats,
  toggleKind,
  toggleLine,
} from './checklist';

const body = 'Launch checklist\n- [x] core tests green\n- [ ] package installer\n\nCall Sam about the keys.';

describe('body <-> blocks', () => {
  it('parses text and checklist lines into blocks', () => {
    const b = parseBody(body);
    expect(b.map((x) => x.kind)).toEqual(['text', 'todo', 'todo', 'text', 'text']);
    expect(b[1]).toMatchObject({ text: 'core tests green', done: true });
    expect(b[2]).toMatchObject({ text: 'package installer', done: false });
    expect(b[3].text).toBe('');
  });

  it('round-trips canonical markdown byte for byte', () => {
    expect(serializeBlocks(parseBody(body))).toBe(body);
    expect(serializeBlocks(parseBody(''))).toBe('');
    expect(serializeBlocks(parseBody('just text\nmore'))).toBe('just text\nmore');
  });

  it('normalises other bullets, bare brackets and CRLF to the canonical form', () => {
    const b = parseBody('* [X] shouted\r\n[ ] bare\r\n+ [] empty box');
    expect(b.map((x) => [x.kind, x.done, x.text])).toEqual([
      ['todo', true, 'shouted'],
      ['todo', false, 'bare'],
      ['todo', false, 'empty box'],
    ]);
    expect(serializeBlocks(b)).toBe('- [x] shouted\n- [ ] bare\n- [ ] empty box');
  });

  it('does not mistake references or links for checkboxes', () => {
    expect(parseBody('[1] reference\n[link](http://x)\n[xx] nope').every((x) => x.kind === 'text')).toBe(true);
  });

  it('always yields at least one block', () => {
    expect(parseBody('')).toHaveLength(1);
  });
});

describe('toggling a line in a stored body', () => {
  it('flips only the box', () => {
    expect(toggleLine(body, 1)).toBe(body.replace('- [x] core', '- [ ] core'));
    expect(toggleLine(body, 2)).toBe(body.replace('- [ ] package', '- [x] package'));
  });

  it('leaves non-checklist lines and out-of-range lines untouched', () => {
    expect(toggleLine(body, 0)).toBe(body);
    expect(toggleLine(body, 99)).toBe(body);
    expect(toggleLine(toggleLine(body, 2), 2)).toBe(body);
  });
});

describe('stats and previews', () => {
  it('counts done / total', () => {
    expect(stats(body)).toEqual({ done: 1, total: 2 });
    expect(stats('plain')).toEqual({ done: 0, total: 0 });
  });

  it('shows marker-free titles and excerpts', () => {
    expect(plainLine('- [ ] buy milk')).toBe('buy milk');
    expect(noteTitle('\n- [x] first item\nsecond')).toBe('first item');
    expect(noteExcerpt(body)).toBe('core tests green · package installer · Call Sam about the keys.');
  });
});

describe('typing shortcuts', () => {
  it('recognises the list markers once the space is typed', () => {
    expect(shortcutFor('[] ')).toEqual({ done: false, rest: '' });
    expect(shortcutFor('- [ ] milk')).toEqual({ done: false, rest: 'milk' });
    expect(shortcutFor('[x] done thing')).toEqual({ done: true, rest: 'done thing' });
    expect(shortcutFor('[]')).toBeNull();
    expect(shortcutFor('hello [] ')).toBeNull();
  });

  it('converts a text line into a checklist item and keeps the remainder', () => {
    // what the editor holds while the user is typing: a *text* block whose value starts with the marker
    const typing = [{ id: 1, kind: 'text' as const, text: '[] milk', done: false }];
    const r = applyShortcut(typing, 0, '[] milk')!;
    expect(r.blocks[0]).toMatchObject({ kind: 'todo', text: 'milk', done: false });
    expect(r.focus).toEqual({ index: 0, caret: 0 });
    expect(applyShortcut(parseBody('- [ ] already'), 0, '- [ ] already')).toBeNull(); // already an item
  });
});

describe('Enter', () => {
  it('continues the list from the caret', () => {
    const r = enter(parseBody('- [ ] buy milk'), 0, 3);
    expect(r.blocks.map((b) => [b.kind, b.text])).toEqual([
      ['todo', 'buy'],
      ['todo', ' milk'],
    ]);
    expect(r.focus).toEqual({ index: 1, caret: 0 });
  });

  it('starts a fresh unchecked item even when the current one is done', () => {
    const r = enter(parseBody('- [x] done thing'), 0, 'done thing'.length);
    expect(r.blocks[1]).toMatchObject({ kind: 'todo', done: false, text: '' });
    expect(r.blocks[0].done).toBe(true);
  });

  it('ends the list on an empty item', () => {
    const r = enter(parseBody('- [ ] a\n- [ ] '), 1, 0);
    expect(r.blocks[1]).toMatchObject({ kind: 'text', text: '' });
    expect(r.blocks).toHaveLength(2);
  });

  it('splits plain text lines too', () => {
    const r = enter(parseBody('hello world'), 0, 5);
    expect(r.blocks.map((b) => b.text)).toEqual(['hello', ' world']);
    expect(r.blocks.every((b) => b.kind === 'text')).toBe(true);
  });
});

describe('Backspace and Delete', () => {
  it('turns a checklist item back into text without losing the words', () => {
    const r = backspaceAtStart(parseBody('- [x] words stay'), 0)!;
    expect(r.blocks[0]).toMatchObject({ kind: 'text', text: 'words stay', done: false });
  });

  it('joins a text line onto the previous one and parks the caret at the seam', () => {
    const r = backspaceAtStart(parseBody('abc\ndef'), 1)!;
    expect(r.blocks.map((b) => b.text)).toEqual(['abcdef']);
    expect(r.focus).toEqual({ index: 0, caret: 3 });
  });

  it('does nothing at the start of the note', () => {
    expect(backspaceAtStart(parseBody('abc'), 0)).toBeNull();
  });

  it('pulls the next line up on Delete at the end', () => {
    const r = deleteAtEnd(parseBody('abc\ndef'), 0)!;
    expect(r.blocks.map((b) => b.text)).toEqual(['abcdef']);
    expect(deleteAtEnd(parseBody('only'), 0)).toBeNull();
  });
});

describe('toolbar and shortcut edits', () => {
  it('Ctrl+Shift+7 flips a line between text and checklist item', () => {
    const on = toggleKind(parseBody('buy milk'), 0);
    expect(on.blocks[0]).toMatchObject({ kind: 'todo', text: 'buy milk' });
    expect(toggleKind(on.blocks, 0).blocks[0]).toMatchObject({ kind: 'text', text: 'buy milk' });
  });

  it('add item reuses an empty line, otherwise appends below', () => {
    const empty = addItem(parseBody('title\n'), 1);
    expect(empty.blocks).toHaveLength(2);
    expect(empty.blocks[1].kind).toBe('todo');
    const appended = addItem(parseBody('title'), null);
    expect(appended.blocks.map((b) => b.kind)).toEqual(['text', 'todo']);
    expect(appended.focus.index).toBe(1);
  });

  it('checking an item changes only its done flag', () => {
    const b = parseBody('- [ ] a\n- [ ] b');
    expect(serializeBlocks(setDone(b, 1, true))).toBe('- [ ] a\n- [x] b');
    expect(setDone(parseBody('text'), 0, true)[0].done).toBe(false);
  });
});

describe('paste', () => {
  it('inserts single lines inline', () => {
    const r = pasteText(parseBody('ab'), 0, 1, 1, 'XY');
    expect(r.blocks[0].text).toBe('aXYb');
    expect(r.focus).toEqual({ index: 0, caret: 3 });
  });

  it('splits multi-line paste into blocks and keeps list markers live', () => {
    const r = pasteText(parseBody('start end'), 0, 6, 6, 'one\n- [x] two\nthree');
    expect(r.blocks.map((b) => [b.kind, b.text, b.done])).toEqual([
      ['text', 'start one', false],
      ['todo', 'two', true],
      ['text', 'threeend', false],
    ]);
    expect(r.focus).toEqual({ index: 2, caret: 5 });
  });
});
