// File organizer, browser demo + pure rules. The webview-side rules mirror the Rust crate (which is
// the authority and has its own tests against scratch folders); here: the same sentences and vectors,
// and the whole Preview -> Confirm -> Run -> Undo flow over the made-up DEMO files.
import { beforeEach, describe, expect, it } from 'vitest';
import {
  DEMO_ROOT,
  DEMO_RULES,
  FOLDER_IDS,
  LIMITS,
  __test,
  ageMatches,
  cleanSubfolder,
  confirmText,
  describeRule,
  emptyRule,
  folders,
  nameMatches,
  normalizeRule,
  parseExtensions,
  preview,
  removeRule,
  rules,
  run,
  saveRule,
  summarizeRun,
  summarizeUndo,
  undo,
  undoInfo,
  validateRuleInput,
  type RuleInput,
} from './organizer';

const rule = (over: Partial<RuleInput> = {}): RuleInput => ({
  enabled: true,
  folder: 'downloads',
  extensions: ['pdf'],
  nameContains: '',
  olderThanDays: 7,
  destFolder: 'documents',
  destSubfolder: 'PDFs',
  ...over,
});

describe('rule sentences', () => {
  it('reads back as the sentence it was written as', () => {
    expect(describeRule(rule())).toBe('In Downloads, move .pdf files older than 7 days to Documents\\PDFs');
    expect(describeRule(rule({ extensions: ['jpg', 'png'], nameContains: 'holiday', olderThanDays: 1 }))).toBe(
      'In Downloads, move .jpg, .png files with "holiday" in the name older than 1 day to Documents\\PDFs',
    );
    expect(describeRule(rule({ extensions: [], nameContains: 'draft', olderThanDays: null }))).toBe(
      'In Downloads, move files with "draft" in the name to Documents\\PDFs',
    );
  });
});

describe('file types', () => {
  it('are normalised', () => {
    expect(parseExtensions('*.PDF, .docx xlsx;  PDF').list).toEqual(['pdf', 'docx', 'xlsx']);
    expect(parseExtensions('').list).toEqual([]);
    expect(parseExtensions('*').list).toEqual([]);
  });

  it('are refused when they are not types', () => {
    for (const bad of ['a/b', 'pdf.exe', 'x'.repeat(LIMITS.extensionChars + 1)]) {
      expect(parseExtensions(bad).error).toContain('File types are letters and digits only');
    }
    const many = Array.from({ length: LIMITS.extensions + 1 }, (_, n) => `e${n}`).join(',');
    expect(parseExtensions(many).error).toContain('At most');
  });
});

describe('destination folders (the same vectors as the Rust tests)', () => {
  it('accept plain relative names', () => {
    expect(cleanSubfolder('PDFs').segments).toEqual(['PDFs']);
    expect(cleanSubfolder(' Work / 2024 \\ Invoices\\ ').segments).toEqual(['Work', '2024', 'Invoices']);
    expect(cleanSubfolder('com10').error).toBeNull();
    expect(cleanSubfolder('x'.repeat(LIMITS.segmentChars)).error).toBeNull();
  });

  it('refuse anything that could leave the destination or confuse Windows', () => {
    for (const bad of ['..', 'a\\..\\b', '.', 'a\\.\\b', 'C:\\Windows', 'c:x', 'con', 'NUL.txt', 'lpt1', 'COM1', 'a\\\\b', 'a?', 'a*', 'a<b', 'a|b', 'bad.', 'x\u0007y']) {
      expect(cleanSubfolder(bad).error, JSON.stringify(bad)).not.toBeNull();
    }
    expect(cleanSubfolder('\\Windows').error).toContain('not a full path');
    expect(cleanSubfolder('/etc').error).toContain('not a full path');
    expect(cleanSubfolder('  ').error).toContain('Name the folder');
    expect(cleanSubfolder('a\\b\\c\\d').error).toContain('at most 3 folders deep');
    expect(cleanSubfolder('x'.repeat(LIMITS.segmentChars + 1)).error).toContain('too long');
  });
});

describe('rule validation', () => {
  it('needs a condition and a sane age', () => {
    const bare = rule({ extensions: [], olderThanDays: null });
    expect(validateRuleInput(bare)).toContain('Add at least one condition');
    expect(validateRuleInput({ ...bare, olderThanDays: 0 })).toContain('Add at least one condition');
    expect(validateRuleInput({ ...bare, olderThanDays: 30 })).toBeNull();
    expect(validateRuleInput({ ...bare, olderThanDays: LIMITS.maxAgeDays + 1 })).toContain('between 1 and 3650');
    expect(validateRuleInput({ ...bare, olderThanDays: 7.5 })).toContain('between 1 and 3650');
    expect(validateRuleInput({ ...bare, nameContains: 'a/b' })).toContain('cannot contain');
    expect(validateRuleInput({ ...bare, nameContains: 'x'.repeat(LIMITS.containsChars + 1) })).toContain('too long');
    expect(validateRuleInput(rule({ destSubfolder: '' }))).toContain('Name the folder');
    expect(validateRuleInput(rule())).toBeNull();
  });

  it('normalises like the backend (types parsed, age 0 -> none, folder tidied)', () => {
    expect(normalizeRule(rule({ extensions: ['*.PDF', 'Docx'], olderThanDays: 0, nameContains: ' inv ', destSubfolder: ' Work / 2024 ' }))).toEqual({
      enabled: true,
      folder: 'downloads',
      extensions: ['pdf', 'docx'],
      nameContains: 'inv',
      olderThanDays: null,
      destFolder: 'documents',
      destSubfolder: 'Work\\2024',
    });
    expect(emptyRule().destSubfolder).toBe('');
  });
});

describe('conditions', () => {
  const r = { extensions: ['pdf', 'docx'], nameContains: 'Invoice' };
  it('match names by type and word, ignoring case', () => {
    expect(nameMatches(r, 'my-INVOICE-2024.PDF')).toBe(true);
    expect(nameMatches(r, 'invoice.docx')).toBe(true);
    expect(nameMatches(r, 'invoice.txt')).toBe(false);
    expect(nameMatches(r, 'invoice')).toBe(false);
    expect(nameMatches(r, '.pdf')).toBe(false);
    expect(nameMatches(r, 'report.pdf')).toBe(false);
    expect(nameMatches({ extensions: [], nameContains: '' }, 'anything')).toBe(true);
  });

  it('match ages', () => {
    const now = 1_000_000_000_000;
    const day = 86_400_000;
    expect(ageMatches({ olderThanDays: 7 }, now - 8 * day, now)).toBe(true);
    expect(ageMatches({ olderThanDays: 7 }, now - 7 * day, now)).toBe(true);
    expect(ageMatches({ olderThanDays: 7 }, now - 6 * day, now)).toBe(false);
    expect(ageMatches({ olderThanDays: 7 }, null, now)).toBe(false);
    expect(ageMatches({ olderThanDays: 7 }, now + day, now)).toBe(false);
    expect(ageMatches({ olderThanDays: null }, null, now)).toBe(true);
  });
});

describe('DEMO: preview -> confirm -> run -> undo', () => {
  beforeEach(() => __test.reset());

  it('lists the six folders under C:\\DEMO\\', async () => {
    const f = await folders();
    expect(f.map((x) => x.id)).toEqual([...FOLDER_IDS]);
    expect(f.every((x) => x.path?.startsWith(DEMO_ROOT))).toBe(true);
  });

  it('starts with the made-up rules and validates new ones like the backend', async () => {
    expect(await rules()).toEqual(DEMO_RULES);
    await expect(saveRule(rule({ extensions: [], olderThanDays: null }))).rejects.toThrow('Add at least one condition');
    const list = await saveRule(rule({ extensions: ['docx'], destSubfolder: 'Word' }));
    expect(list.at(-1)!.id).toBe('rule-3');
    const edited = await saveRule({ ...rule({ extensions: ['docx', 'doc'] }), id: 'rule-3' });
    expect(edited).toHaveLength(3);
    expect(edited[2].extensions).toEqual(['docx', 'doc']);
    expect((await removeRule('rule-3')).map((x) => x.id)).toEqual(['rule-1', 'rule-2']);
  });

  it('previews exactly the old matching files, with a numbered name for a clash, and moves nothing', async () => {
    const p = await preview();
    expect(p.demo).toBe(true);
    expect(p.moves.map((m) => [m.ruleId, m.toName])).toEqual([
      ['rule-1', 'invoice-april.pdf'],
      ['rule-1', 'invoice-march (2).pdf'], // Documents\PDFs\invoice-march.pdf is already there
      ['rule-2', 'holiday-photo.jpg'],
    ]);
    expect(p.moves.every((m) => m.from.startsWith(DEMO_ROOT) && m.toDir.startsWith(DEMO_ROOT))).toBe(true);
    expect(p.moves.find((m) => m.toName === 'invoice-april.pdf')!.toDir).toBe(`${DEMO_ROOT}Documents\\PDFs`);
    // looking again gives the same answer: nothing was moved
    expect((await preview()).moves.map((m) => m.toName)).toEqual(p.moves.map((m) => m.toName));
  });

  it('leaves files that are still arriving alone and says so', async () => {
    await saveRule(rule({ extensions: ['crdownload'], olderThanDays: null, destSubfolder: 'Partial' }));
    const p = await preview();
    expect(p.moves.some((m) => m.toName.endsWith('.crdownload'))).toBe(false);
    expect(p.notes.join(' ')).toContain('unfinished downloads');
  });

  it('does not run on its own: it needs the id of a preview that was shown', async () => {
    await expect(run(1)).rejects.toThrow('There is no preview to run');
    const p = await preview();
    await expect(run(p.previewId + 1)).rejects.toThrow('out of date');
  });

  it('runs a preview once, never replaces a file, and keeps an undo list', async () => {
    const p = await preview();
    expect(await undoInfo()).toBeNull();
    const r = await run(p.previewId);
    expect(r.demo).toBe(true);
    expect(r.moved.map((m) => m.to.split('\\').pop())).toEqual(['invoice-april.pdf', 'invoice-march (2).pdf', 'holiday-photo.jpg']);
    expect(r.failed).toEqual([]);
    expect(summarizeRun(r)).toMatchObject({ headline: 'Moved 3 files.', clean: true });
    await expect(run(p.previewId)).rejects.toThrow('There is no preview to run');
    // the moved files are gone from Downloads, so the same rules now find nothing
    expect((await preview()).moves).toEqual([]);
    expect(await undoInfo()).toMatchObject({ count: 3 });
  });

  it('puts the files back with Undo, then has nothing left to undo', async () => {
    const before = (await preview()).moves.map((m) => m.from);
    await run((await preview()).previewId);
    const u = await undo();
    expect(u).toMatchObject({ restored: 3, demo: true });
    expect(u.failed).toEqual([]);
    expect(summarizeUndo(u)).toMatchObject({ headline: 'Put back 3 files.', clean: true });
    expect((await preview()).moves.map((m) => m.from)).toEqual(before);
    expect(await undoInfo()).toBeNull();
    await expect(undo()).rejects.toThrow('nothing to undo');
  });

  it('shows the exact confirm sentence', async () => {
    const p = await preview();
    expect(confirmText(p.moves)).toMatch(/^Move 3 files \(/);
  });
});

describe('summaries', () => {
  it('say what happened without overclaiming', () => {
    expect(summarizeRun({ moved: [], failed: [], alreadyGone: 0 }).headline).toBe('Nothing was moved.');
    const s = summarizeRun({ moved: [], failed: [{ path: 'a', reason: 'x' }], alreadyGone: 2 });
    expect(s.clean).toBe(false);
    expect(s.lines.join(' ')).toContain('could not be moved');
    expect(s.lines.join(' ')).toContain('already disappeared');
    expect(summarizeUndo({ restored: 0, failed: [{ path: 'a', reason: 'x' }] }).lines.join(' ')).toContain('left where it is');
  });
});
