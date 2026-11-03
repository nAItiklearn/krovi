// Snippets and workflows: the pure rules (they mirror the Rust crate, which is the authority), the
// placeholder expansion with the same vectors as the Rust tests, palette ranking, and the browser
// demo copy.
import { beforeEach, describe, expect, it } from 'vitest';
import {
  DEMO_APPS,
  DEMO_SNIPPETS,
  DEMO_WORKFLOWS,
  LIMITS,
  __resetDemo,
  expandSnippet,
  launcherApps,
  localDateTime,
  normalizeSnippet,
  parseWorkflowLines,
  rankSnippets,
  snippetPreview,
  snippets,
  usesClipboard,
  validateSnippet,
  validateWorkflow,
  workflows,
  type Snippet,
} from './extras';

const snip = (name: string, trigger = '', body = 'x', id = ''): Snippet => ({ id, name, trigger, body });

describe('snippet validation (same sentences as the backend)', () => {
  it('tidies a snippet', () => {
    expect(normalizeSnippet({ name: '  Email   sign-off ', trigger: ' ;sig ', body: 'Thanks,\r\nA\rB' })).toEqual({
      name: 'Email sign-off',
      trigger: ';sig',
      body: 'Thanks,\nA\nB',
    });
  });

  it('accepts a good snippet, with or without a keyword', () => {
    expect(validateSnippet({ name: 'A', trigger: '', body: 'line1\n\tline2' }, [])).toBeNull();
    expect(validateSnippet({ name: 'A', trigger: ';a', body: 'x' }, [])).toBeNull();
  });

  it('refuses what the backend refuses', () => {
    const v = (name: string, trigger: string, body: string) => validateSnippet({ name, trigger, body }, []);
    expect(v('', '', 'x')).toBe('Give the snippet a name.');
    expect(v('a\u0007b', '', 'x')).toBe('The name cannot contain control characters.');
    expect(v('n'.repeat(LIMITS.snippetName + 1), '', 'x')).toContain('too long');
    expect(v('a', 'has space', 'x')).toBe('A keyword cannot contain spaces or control characters.');
    expect(v('a', 't'.repeat(LIMITS.snippetTrigger + 1), 'x')).toContain('keyword is too long');
    expect(v('a', '', '  \n ')).toBe('The snippet has no text.');
    expect(v('a', '', 'bad\u0000nul')).toContain('control character');
    expect(v('a', '', 'b'.repeat(LIMITS.snippetBody + 1))).toContain('text is too long');
  });

  it('keeps names and keywords unique, but lets a snippet keep its own', () => {
    const others = [snip('Sign off', ';sig', 'x', 'sn-1')];
    expect(validateSnippet({ name: 'SIGN OFF', trigger: '', body: 'x' }, others)).toBe('A snippet with that name already exists.');
    expect(validateSnippet({ name: 'Other', trigger: ';SIG', body: 'x' }, others)).toBe('Another snippet already uses that keyword.');
    expect(validateSnippet({ id: 'sn-1', name: 'Sign off', trigger: ';sig', body: 'changed' }, others)).toBeNull();
  });
});

describe('placeholders (the same vectors as the Rust tests)', () => {
  // 14 Nov 2023, 22:13 on this PC's clock, whatever zone the test runs in
  const now = new Date(2023, 10, 14, 22, 13, 20);
  const x = (body: string, clipboard: string | null = null) => expandSnippet(body, { now, clipboard });

  it('formats the local date and time', () => {
    expect(localDateTime(now)).toEqual({ date: '2023-11-14', time: '22:13' });
    expect(localDateTime(new Date(2000, 1, 29, 0, 5))).toEqual({ date: '2000-02-29', time: '00:05' });
    expect(localDateTime(new Date(2026, 0, 3, 9, 7))).toEqual({ date: '2026-01-03', time: '09:07' });
  });

  it('fills the three placeholders and leaves everything else literal', () => {
    expect(x('On {date} at {time}: {clipboard}! {name} {Date} {', 'hello').text).toBe('On 2023-11-14 at 22:13: hello! {name} {Date} {');
  });

  it('never expands a substituted value again', () => {
    expect(x('{clipboard}', '{date} {time} {clipboard}').text).toBe('{date} {time} {clipboard}');
  });

  it('turns a missing clipboard into nothing and says so', () => {
    expect(x('a{clipboard}b')).toEqual({ text: 'ab', clipboardEmpty: true, clipboardTruncated: false });
    expect(x('a{clipboard}b', '').clipboardEmpty).toBe(true);
    expect(x('plain').clipboardEmpty).toBe(false);
  });

  it('cuts a huge clipboard and says so', () => {
    const e = x('{clipboard}', 'x'.repeat(LIMITS.clipboardChars + 50));
    expect([...e.text]).toHaveLength(LIMITS.clipboardChars);
    expect(e.clipboardTruncated).toBe(true);
  });

  it('keeps multibyte text intact', () => {
    expect(x('héllo {date} — ✓ {clipboard}', '日本').text).toBe('héllo 2023-11-14 — ✓ 日本');
  });

  it('only asks for the clipboard when the text uses it', () => {
    expect(usesClipboard('x {clipboard} y')).toBe(true);
    expect(usesClipboard('x {date} y')).toBe(false);
    expect(usesClipboard('{Clipboard}')).toBe(false);
  });
});

describe('snippet search', () => {
  const list = [
    snip('Email sign-off', ';sig', 'Thanks,\nAlex', 'a'),
    snip('Meeting notes header', ';mtg', 'Notes for {date}', 'b'),
    snip('Address', '', '1 Main Street, Springfield', 'c'),
    snip('Signature block', ';sigb', 'Alex Demo', 'd'),
  ];
  const ids = (q: string) => rankSnippets(q, list).map((h) => h.snippet.id);

  it('lists everything in order when nothing is typed', () => {
    expect(ids('')).toEqual(['a', 'b', 'c', 'd']);
    expect(ids('  ')).toEqual(['a', 'b', 'c', 'd']);
  });

  it('puts an exact keyword first, then a keyword that starts with the text', () => {
    expect(ids(';sig')[0]).toBe('a');
    expect(ids(';SIG')).toEqual(['a', 'd']);
    expect(ids(';m')).toEqual(['b']);
  });

  it('finds by name, with the highlighted range', () => {
    expect(ids('email')).toEqual(['a']);
    expect(rankSnippets('meeting', list)[0].ranges).toEqual([[0, 7]]);
  });

  it('finds by words in the text', () => {
    expect(ids('springfield')).toEqual(['c']);
    expect(ids('alex')).toEqual(['a', 'd']);
  });

  it('finds nothing for nonsense', () => {
    expect(ids('qqqq')).toEqual([]);
  });

  it('shows one tidy line of the text in a list', () => {
    expect(snippetPreview('line one\n\n  line   two')).toBe('line one line two');
    const long = snippetPreview('x'.repeat(200), 20);
    expect([...long]).toHaveLength(20);
    expect(long.endsWith('…')).toBe(true);
  });
});

describe('workflow validation', () => {
  it('splits a textarea into tidy lines', () => {
    expect(parseWorkflowLines('  open  spotify \n\n   \r\nset a timer for 25 minutes\n')).toEqual(['open spotify', 'set a timer for 25 minutes']);
  });

  it('accepts a good workflow and refuses what the backend refuses', () => {
    expect(validateWorkflow({ name: 'Focus', lines: ['open spotify'] }, [])).toBeNull();
    expect(validateWorkflow({ name: '', lines: ['open x'] }, [])).toBe('Give the workflow a name.');
    expect(validateWorkflow({ name: 'a', lines: [] }, [])).toBe('Add at least one command line.');
    expect(validateWorkflow({ name: 'a', lines: ['  ', ''] }, [])).toBe('Add at least one command line.');
    expect(validateWorkflow({ name: 'a', lines: ['ok', 'bad\u0000line'] }, [])).toBe('A line contains a control character.');
    expect(validateWorkflow({ name: 'a', lines: ['x'.repeat(LIMITS.workflowLine + 1)] }, [])).toContain('line is too long');
    const many = Array.from({ length: LIMITS.workflowLines + 1 }, (_, n) => `open app${n}`);
    expect(validateWorkflow({ name: 'a', lines: many }, [])).toContain('at most');
    expect(validateWorkflow({ name: 'a', lines: many.slice(0, LIMITS.workflowLines) }, [])).toBeNull();
  });

  it('keeps names unique but lets a workflow keep its own', () => {
    const others = [{ id: 'wf-1', name: 'Morning', lines: ['open gmail'] }];
    expect(validateWorkflow({ name: 'MORNING', lines: ['open x'] }, others)).toBe('A workflow with that name already exists.');
    expect(validateWorkflow({ id: 'wf-1', name: 'Morning', lines: ['open x'] }, others)).toBeNull();
  });
});

describe('browser demo copy', () => {
  beforeEach(() => __resetDemo());

  it('starts with the clearly made-up entries', async () => {
    expect((await snippets.list()).map((s) => s.id)).toEqual(DEMO_SNIPPETS.map((s) => s.id));
    expect(await workflows.list()).toEqual(DEMO_WORKFLOWS);
    expect(await launcherApps()).toEqual(DEMO_APPS);
  });

  it('adds, edits and removes a snippet, validating like the backend', async () => {
    await expect(snippets.save({ name: '', trigger: '', body: 'x' })).rejects.toThrow('Give the snippet a name.');
    const list = await snippets.save({ name: 'Hello', trigger: ';hi', body: 'Hi there' });
    const made = list.find((s) => s.name === 'Hello')!;
    expect(made.id).toBe('sn-hello');
    const edited = await snippets.save({ id: made.id, name: 'Hello', trigger: ';hi', body: 'Hi again' });
    expect(edited.filter((s) => s.name === 'Hello')).toHaveLength(1);
    expect(edited.find((s) => s.id === made.id)!.body).toBe('Hi again');
    await expect(snippets.save({ name: 'hello', trigger: '', body: 'x' })).rejects.toThrow('already exists');
    expect((await snippets.remove(made.id)).some((s) => s.id === made.id)).toBe(false);
  });

  it('copying a demo snippet never reads the clipboard and says it is a demo', async () => {
    const quote = DEMO_SNIPPETS.find((s) => s.body.includes('{clipboard}'))!;
    const used = await snippets.use(quote.id);
    expect(used.demo).toBe(true);
    expect(used.notes.join(' ')).toContain('DEMO');
    await expect(snippets.use('sn-nope')).rejects.toThrow('no longer exists');
  });

  it('adds and removes a workflow', async () => {
    const list = await workflows.save({ name: 'Wind down', lines: ['  open notepad ', '', 'set a timer for 5 minutes'] });
    const made = list.find((w) => w.name === 'Wind down')!;
    expect(made.lines).toEqual(['open notepad', 'set a timer for 5 minutes']);
    await expect(workflows.save({ name: 'WIND DOWN', lines: ['open x'] })).rejects.toThrow('already exists');
    expect((await workflows.remove(made.id)).some((w) => w.id === made.id)).toBe(false);
  });
});
