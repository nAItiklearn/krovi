import { describe, it, expect } from 'vitest';
import { visible, query, counts } from './clip';
import type { ClipEntry } from './types';

function e(id: number, kind: ClipEntry['kind'], content: string, pinned = false): ClipEntry {
  return { id, kind, content, pinned, created_at: id };
}

const sample: ClipEntry[] = [
  e(5, 'Text', 'Hello World'),
  e(4, 'Link', 'https://rust-lang.org'),
  e(3, 'Image', 'blobs/shot.png', true),
  e(2, 'Link', 'https://example.com'),
  e(1, 'Text', 'goodbye'),
];

describe('clipboard helpers', () => {
  it('floats pinned entries to the top, preserving order within groups', () => {
    const v = visible(sample);
    expect(v[0].id).toBe(3); // the only pinned one
    expect(v.slice(1).map((x) => x.id)).toEqual([5, 4, 2, 1]);
  });

  it('filters by kind', () => {
    expect(query(sample, 'Link', null).map((x) => x.id)).toEqual([4, 2]);
    expect(query(sample, 'Text', null).map((x) => x.id)).toEqual([5, 1]);
  });

  it('searches case-insensitively by substring', () => {
    const hits = query(sample, null, 'WOR');
    expect(hits.map((x) => x.id)).toEqual([5]);
    expect(query(sample, null, '').length).toBe(sample.length);
  });

  it('combines kind + needle', () => {
    const hits = query(sample, 'Link', 'rust');
    expect(hits.map((x) => x.id)).toEqual([4]);
  });

  it('counts entries per kind', () => {
    expect(counts(sample)).toEqual({ all: 5, Text: 2, Link: 2, Image: 1 });
  });
});
