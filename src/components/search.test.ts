import { describe, it, expect } from 'vitest';
import { match, segments } from './search';

describe('command palette matcher', () => {
  it('matches everything for an empty query', () => {
    expect(match('', 'Clipboard')?.score).toBeGreaterThan(0);
  });

  it('ranks prefix > word start > substring > keyword > subsequence', () => {
    const prefix = match('cl', 'Clipboard')!;
    const word = match('cl', 'World Clocks')!;
    const sub = match('oc', 'World Clocks')!;
    const kw = match('music', 'Media', ['music', 'player'])!;
    const subseq = match('clpb', 'Clipboard')!;
    expect(prefix.score).toBeGreaterThan(word.score);
    expect(word.score).toBeGreaterThan(sub.score);
    expect(sub.score).toBeGreaterThan(kw.score);
    expect(kw.score).toBeGreaterThan(subseq.score);
  });

  it('is case-insensitive and rejects non-matches', () => {
    expect(match('CLIP', 'Clipboard')).not.toBeNull();
    expect(match('zzz', 'Clipboard', ['copy'])).toBeNull();
  });

  it('returns highlight ranges inside the label', () => {
    expect(match('clip', 'Clipboard')!.ranges).toEqual([[0, 4]]);
    expect(match('clk', 'World Clocks')!.ranges.length).toBeGreaterThan(0);
  });

  it('splits a label into plain and highlighted segments', () => {
    expect(segments('Clipboard', [[0, 4]])).toEqual([
      { text: 'Clip', hit: true },
      { text: 'board', hit: false },
    ]);
    expect(segments('Notes', [])).toEqual([{ text: 'Notes', hit: false }]);
  });
});
