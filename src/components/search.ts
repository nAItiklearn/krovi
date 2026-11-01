// Tiny fuzzy matcher for the command palette. Pure and unit-tested.
// Prefix > word-start > substring > subsequence; matched ranges are returned for highlighting.

export interface Match {
  score: number;
  /** [start, end) ranges inside the *label* to highlight */
  ranges: [number, number][];
}

function wordStarts(s: string): number[] {
  const out: number[] = [];
  for (let i = 0; i < s.length; i++) if (i === 0 || /[\s\-_/.]/.test(s[i - 1])) out.push(i);
  return out;
}

/** Match a query against a label (highlighted) and optional extra terms (score only). */
export function match(query: string, label: string, extra: string[] = []): Match | null {
  const q = query.trim().toLowerCase();
  if (!q) return { score: 1, ranges: [] };
  const l = label.toLowerCase();

  if (l.startsWith(q)) return { score: 100 - Math.min(20, label.length - q.length), ranges: [[0, q.length]] };

  for (const i of wordStarts(l)) {
    if (l.startsWith(q, i)) return { score: 80 - Math.min(20, i), ranges: [[i, i + q.length]] };
  }

  const at = l.indexOf(q);
  if (at >= 0) return { score: 60 - Math.min(20, at), ranges: [[at, at + q.length]] };

  // keywords / hint text: matches but nothing to highlight in the label
  const hay = extra.join(' ').toLowerCase();
  if (hay.includes(q)) return { score: 40, ranges: [] };

  // subsequence in the label ("clpb" -> Clipboard)
  const ranges: [number, number][] = [];
  let qi = 0;
  for (let i = 0; i < l.length && qi < q.length; i++) {
    if (l[i] === q[qi]) {
      const last = ranges[ranges.length - 1];
      if (last && last[1] === i) last[1] = i + 1;
      else ranges.push([i, i + 1]);
      qi++;
    }
  }
  if (qi === q.length) return { score: 20 - Math.min(10, ranges.length), ranges };
  return null;
}

/** Split a label into plain/highlighted segments for rendering. */
export function segments(label: string, ranges: [number, number][]): { text: string; hit: boolean }[] {
  const out: { text: string; hit: boolean }[] = [];
  let pos = 0;
  for (const [a, b] of ranges) {
    if (a > pos) out.push({ text: label.slice(pos, a), hit: false });
    out.push({ text: label.slice(a, b), hit: true });
    pos = b;
  }
  if (pos < label.length) out.push({ text: label.slice(pos), hit: false });
  return out.length ? out : [{ text: label, hit: false }];
}
