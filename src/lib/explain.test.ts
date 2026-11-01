import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import {
  MAX_CHARS,
  __test,
  bulletMarkerLen,
  collapseWs,
  countText,
  creditsFor,
  formatDuration,
  highlightParts,
  idleSpeak,
  loadEngines,
  noteBody,
  reduceSpeak,
  saveSettings,
  savedLabel,
  setElevenKey,
  shortVoiceName,
  speaking,
  speakable,
  splitUnits,
  summarize,
  summarizeLocal,
  testElevenVoice,
  type SpeakEvent,
  type SummaryLength,
} from './explain';

// The same fixtures as src-tauri/crates/krovi-explain/tests/fixtures (the Rust summariser's tests).
const LEGAL = `Limited Warranty

This warranty covers manufacturing defects in the Brewmate coffee maker for a period of two years from the date of purchase. If the appliance fails because of a defect in materials or workmanship during that period, the company will repair it or replace it, at its own choice and at no charge to you. The warranty does not cover damage caused by misuse, by connecting the appliance to a voltage other than the one printed on the rating label, or by repairs carried out by anyone who is not an authorised service agent. Descaling the appliance regularly is your responsibility, and scale build-up is not considered a defect. In no event shall the company be liable for any indirect or consequential loss, including loss of profit or loss of use, arising from the use of the appliance. Nothing in this warranty affects your statutory rights as a consumer.

To make a claim, you must contact customer service within thirty days of discovering the fault and provide your proof of purchase. The company may ask you to return the appliance at your own cost before a replacement is sent. This warranty is valid only in the country where the appliance was originally purchased.
`;

const TECH = `How a hash map works

A hash map stores key and value pairs so that a value can be found quickly from its key. Instead of searching through every entry, the map runs the key through a hash function that turns it into a number. That number decides which bucket of the underlying array the entry belongs in, so a lookup usually touches only one bucket. Two different keys can produce the same number, which is called a collision. A common way to handle collisions is to keep a short list of entries inside each bucket and compare keys one by one.

When the map becomes too full, lookups slow down because the buckets hold longer lists. To prevent this, the map grows its array when the load factor passes a limit, and every entry is moved to its new bucket in a step called rehashing. Rehashing is expensive, but it happens rarely enough that inserting stays fast on average. Hash maps do not keep keys in any particular order, so a program that needs sorted keys should use a different structure.
`;

const NEWS = `City council extends Riverside Park hours for summer

The city council has voted to extend the opening hours of Riverside Park through the summer months. From June 1, the park will stay open until 10 p.m. on weekdays and until 11 p.m. on weekends. The decision followed a survey in which 64 percent of residents said they wanted more evening access to the park. Council member Dana Ortiz said the longer hours would give working families more time outdoors.

The parks department will add lighting along the main path and two new waste bins near the playground, at a cost of about $18,500. Residents who live next to the park asked the council to keep noise limits in place after 9 p.m., and the council agreed to review any complaints after the first month. The farmers market will move to Saturday afternoons so that it can use the extended hours. A public meeting on the plan is scheduled for May 12 at the community centre. More information is available on the city website.
`;

/** The first three words of each chosen sentence - enough to identify it. */
const heads = (text: string, len: SummaryLength) => summarizeLocal(text, len).items.map((i) => i.text.split(' ').slice(0, 3).join(' '));

describe('pure helpers', () => {
  it('formats durations', () => {
    expect(formatDuration(0)).toBe('0 s');
    expect(formatDuration(44.6)).toBe('45 s');
    expect(formatDuration(60)).toBe('1 min');
    expect(formatDuration(155)).toBe('2 min 35 s');
    expect(formatDuration(3900)).toBe('1 h 05 min');
    expect(formatDuration(-5)).toBe('0 s');
  });

  it('counts characters (by code point) and words', () => {
    expect(countText('')).toEqual({ chars: 0, words: 0 });
    expect(countText('  ')).toEqual({ chars: 0, words: 0 });
    expect(countText('one two  three')).toEqual({ chars: 14, words: 3 });
    expect(countText('a\u{1f600}b').chars).toBe(3);
  });

  it('words a saved-time label only when it is worth saying', () => {
    const st = (savedSecs: number) => ({ chars: 0, words: 0, sentences: 0, summaryWords: 0, summarySentences: 0, readSecs: 0, summaryReadSecs: 0, savedSecs, ratio: 0.3 });
    expect(savedLabel(st(2))).toBe('');
    expect(savedLabel(st(38))).toBe('Saves about 38 s of reading');
    expect(savedLabel(st(200))).toBe('Saves about 3 min 20 s of reading');
  });

  it('estimates ElevenLabs credits by model', () => {
    expect(creditsFor(1000, 'eleven_flash_v2_5')).toBe(500);
    expect(creditsFor(1001, 'eleven_flash_v2_5')).toBe(501);
    expect(creditsFor(1000, 'eleven_multilingual_v2')).toBe(1000);
    expect(creditsFor(1000, 'unknown-model')).toBe(1000);
  });

  it('splits text around the spoken range, clamped', () => {
    expect(highlightParts('hello world', null)).toEqual({ before: 'hello world', mid: '', after: '' });
    expect(highlightParts('hello world', { start: 0, end: 5 })).toEqual({ before: '', mid: 'hello', after: ' world' });
    expect(highlightParts('hello world', { start: 6, end: 99 })).toEqual({ before: 'hello ', mid: 'world', after: '' });
    expect(highlightParts('hello', { start: 3, end: 3 }).mid).toBe('');
    expect(highlightParts('hello', { start: -4, end: 2 }).mid).toBe('he');
  });

  it('shortens a voice name for a button', () => {
    expect(shortVoiceName('Microsoft David - English (United States)')).toBe('Microsoft David');
    expect(shortVoiceName('Microsoft Zira')).toBe('Microsoft Zira');
    expect(shortVoiceName('A'.repeat(50))).toBe(`${'A'.repeat(31)}…`);
    expect(shortVoiceName('')).toBe('');
  });

  it('builds a note body that names the source', () => {
    expect(noteBody('A very\nlong   text', 'Summary here.')).toBe('Summary of: A very long text\n\nSummary here.');
    expect(noteBody('x'.repeat(100), 's')).toMatch(/^Summary of: x{57}\.\.\.\n\ns$/);
  });
});

describe('sentence splitter (port of the Rust one)', () => {
  const t = (s: string) => splitUnits(s).map((u) => u.text);

  it('keeps titles, initials, dotted abbreviations and decimals whole', () => {
    expect(t('Dr. Smith met Mr. Jones at St. Mary\'s. J. K. Rowling was not there.')).toEqual(["Dr. Smith met Mr. Jones at St. Mary's.", 'J. K. Rowling was not there.']);
    expect(t('The U.S. economy grew on Jan. 5 at 3 p.m. while the E.U. waited. Then it rained.')).toEqual(['The U.S. economy grew on Jan. 5 at 3 p.m. while the E.U. waited.', 'Then it rained.']);
    expect(t('Prices rose 3.5 percent to $1,200.50 in v2.0.1. Analysts disagreed.')).toEqual(['Prices rose 3.5 percent to $1,200.50 in v2.0.1.', 'Analysts disagreed.']);
    expect(t('See https://example.com/a.b. Next one.')).toEqual(['See https://example.com/a.b.', 'Next one.']);
    expect(t('See Fig. 3 on p. 12. The answer is no. Done here.')).toEqual(['See Fig. 3 on p. 12.', 'The answer is no.', 'Done here.']);
  });

  it('handles quotes, ellipses and caseless scripts', () => {
    expect(t('"Stop!" he said. Then he left.')).toEqual(['"Stop!" he said.', 'Then he left.']);
    expect(t('Well... that happened. Really?! Yes.')).toEqual(['Well... that happened.', 'Really?!', 'Yes.']);
    expect(t('今日は晴れです。明日もです。')).toHaveLength(2);
  });

  it('turns bullets and numbered lists into units without their markers', () => {
    const input = 'Things to do:\n- Buy milk\n* Fix the sink\n1. First step here\n(a) Option alpha';
    const u = splitUnits(input);
    expect(u.map((x) => x.text)).toEqual(['Things to do:', 'Buy milk', 'Fix the sink', 'First step here', 'Option alpha']);
    expect(u[1].kind).toBe('bullet');
    expect(input.slice(u[1].start, u[1].end)).toBe('Buy milk');
    expect(bulletMarkerLen('-5 degrees')).toBe(0);
    expect(bulletMarkerLen('(see) the note')).toBe(0);
    expect(bulletMarkerLen('2024 was good')).toBe(0);
  });

  it('marks headings and re-flows wrapped lines with correct offsets', () => {
    const input = 'Terms\n\nThis sentence wraps\nacross two lines. Next one.';
    const u = splitUnits(input);
    expect(u[0].kind).toBe('heading');
    expect(u[1].text).toBe('This sentence wraps across two lines.');
    expect(input.slice(u[1].start, u[1].end)).toBe('This sentence wraps\nacross two lines.');
    expect(splitUnits('')).toEqual([]);
    expect(splitUnits('  \n \n')).toEqual([]);
  });

  it('makes sentences speakable', () => {
    expect(speakable('- Buy   milk')).toBe('Buy milk');
    expect(speakable('• **Bold** point.')).toBe('Bold point.');
    expect(speakable('See https://example.com/x, then stop.')).toBe('See link, then stop.');
    expect(collapseWs('  a \n b ')).toBe('a b');
  });
});

describe('summarizeLocal (port of the Rust summariser)', () => {
  const lens: SummaryLength[] = ['short', 'medium', 'points'];

  it('matches the Rust summariser sentence-for-sentence on the shared fixtures', () => {
    expect(heads(TECH, 'short')).toEqual(['A hash map', 'Instead of searching']);
    expect(heads(TECH, 'medium')).toEqual(['A hash map', 'Instead of searching', 'Hash maps do']);
    expect(heads(NEWS, 'short')).toEqual(['The city council', 'Residents who live']);
    expect(heads(NEWS, 'medium')).toEqual(['The city council', 'From June 1,', 'Residents who live']);
    expect(heads(LEGAL, 'short')).toEqual(['This warranty covers', 'If the appliance']);
    expect(heads(LEGAL, 'medium')).toEqual(['This warranty covers', 'If the appliance', 'The warranty does']);
  });

  it('is extractive, ordered, repeat-free and clearly shorter, in every mode', () => {
    for (const text of [LEGAL, TECH, NEWS]) {
      const flat = collapseWs(text);
      for (const len of lens) {
        const s = summarizeLocal(text, len);
        expect(s.alreadyShort).toBe(false);
        for (const it of s.items) expect(flat).toContain(it.text);
        const idx = s.items.map((i) => i.index);
        expect(idx).toEqual([...new Set(idx)].sort((a, b) => a - b));
        expect(s.stats.ratio).toBeLessThan(0.6);
        expect(s.stats.savedSecs).toBeGreaterThan(0);
      }
      expect(summarizeLocal(text, 'short').items.length).toBeLessThanOrEqual(2);
      const points = summarizeLocal(text, 'points');
      expect(points.text.split('\n').every((l) => l.startsWith('•'))).toBe(true);
      expect(points.text.split('\n')).toHaveLength(points.items.length);
    }
  });

  it('returns very short input as written with a note', () => {
    const s = summarizeLocal('Just a quick note. Meeting moved to 3 pm.', 'short');
    expect(s.alreadyShort).toBe(true);
    expect(s.note).toMatch(/already short/);
    expect(s.text).toBe('Just a quick note. Meeting moved to 3 pm.');
    expect(s.stats.savedSecs).toBe(0);
  });

  it('refuses empty and oversized input with a plain sentence', () => {
    expect(() => summarizeLocal('  ', 'short')).toThrow(/Paste some text/);
    expect(() => summarizeLocal('word '.repeat(MAX_CHARS / 5 + 10), 'short')).toThrow(/summarizes up to/);
  });

  it('does not choose two near-identical sentences', () => {
    const dup =
      'The committee approved the new budget for the coming fiscal year on Tuesday. On Tuesday the committee approved the new budget for the coming fiscal year. Rainfall across the northern region was well below the seasonal average this spring. Farmers there have started rationing irrigation water for their remaining crops. The local museum will reopen next month after two years of renovation work. Visitors can expect a new wing devoted to regional history and modern art.';
    const hits = summarizeLocal(dup, 'medium').items.filter((i) => i.text.toLowerCase().includes('budget'));
    expect(hits.length).toBeLessThanOrEqual(1);
  });

  it('is fast on a long document', () => {
    const big = `${LEGAL}\n\n${TECH}\n\n${NEWS}\n\n`.repeat(40);
    const t = Date.now();
    const s = summarizeLocal(big, 'medium');
    expect(s.items.length).toBeGreaterThanOrEqual(3);
    expect(Date.now() - t).toBeLessThan(15000);
  });
});

describe('speaking state', () => {
  const speaking1: SpeakEvent = { state: 'speaking', index: 1, count: 4, start: 10, end: 30, engine: 'windows', voice: 'Microsoft Zira' };

  it('follows the events', () => {
    let s = reduceSpeak(idleSpeak(), speaking1);
    expect(s).toMatchObject({ status: 'speaking', range: { start: 10, end: 30 }, engine: 'windows', voice: 'Microsoft Zira', index: 1, count: 4 });
    s = reduceSpeak(s, { state: 'notice', message: 'ElevenLabs is out of credits, so the rest is read with your Windows voice.' });
    expect(s.notice).toMatch(/out of credits/);
    expect(s.status).toBe('speaking');
    const done = reduceSpeak(s, { state: 'done', ms: 5 });
    expect(done.status).toBe('idle');
    expect(done.range).toBeNull();
    expect(done.notice).toMatch(/out of credits/); // the heads-up stays visible after reading ends
  });

  it('a failure ends reading and keeps the message', () => {
    const s = reduceSpeak(reduceSpeak(idleSpeak(), speaking1), { state: 'failed', message: 'Windows could not play this audio.' });
    expect(s.status).toBe('idle');
    expect(s.error).toMatch(/could not play/);
  });

  it('a range of zero width (a voice test) highlights nothing', () => {
    const s = reduceSpeak(idleSpeak(), { state: 'speaking', index: 0, count: 1, start: 0, end: 0, engine: 'elevenlabs', voice: 'Rachel' });
    expect(s.range).toBeNull();
    expect(s.engine).toBe('elevenlabs');
  });

  it('stopped returns to idle', () => {
    const s = reduceSpeak(reduceSpeak(idleSpeak(), speaking1), { state: 'stopped' });
    expect(s).toMatchObject({ status: 'idle', range: null });
  });
});

describe('browser demo API (no Tauri)', () => {
  const store = new Map<string, string>();
  beforeEach(() => {
    store.clear();
    __test.reset();
    vi.stubGlobal('localStorage', {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    });
  });
  afterEach(() => vi.unstubAllGlobals());

  it('summarize() runs the local algorithm', async () => {
    const s = await summarize(TECH, 'short');
    expect(s.items).toHaveLength(2);
    expect(get(speaking).status).toBe('idle');
  });

  it('defaults to the Windows voice with ElevenLabs off', async () => {
    const e = await loadEngines();
    expect(e.settings.voiceEngine).toBe('windows');
    expect(e.elevenlabs.hasKey).toBe(false);
    expect(e.settings.elevenConsented).toBe(false);
  });

  it('rejects a key that is clearly not a key, and never persists any key text', async () => {
    const bad = await setElevenKey('short');
    expect(bad.saved).toBe(false);
    expect(bad.tone).toBe('error');

    const secret = 'sk_demo_0123456789abcdefghij';
    const ok = await setElevenKey(secret);
    expect(ok.saved).toBe(true);
    expect(ok.voices.length).toBeGreaterThan(0);
    expect(ok.message).toMatch(/Demo/);
    await saveSettings({ ...ok.settings, voiceEngine: 'elevenlabs' });
    // nothing stored in localStorage contains the key, and there is no field for one
    for (const v of store.values()) expect(v).not.toContain(secret);
    expect(Object.keys(JSON.parse(store.get('krovi.explain.settings') ?? '{}')).some((k) => /key|secret|token/i.test(k))).toBe(false);
  });

  it('ElevenLabs cannot be the engine without a key, and removing the key switches back', async () => {
    const s = await saveSettings({ ...(await loadEngines()).settings, voiceEngine: 'elevenlabs', elevenVoiceId: 'x'.repeat(20) });
    expect(s.voiceEngine).toBe('windows');
    await setElevenKey('sk_demo_0123456789abcdefghij');
    const on = await saveSettings({ ...(await loadEngines()).settings, voiceEngine: 'elevenlabs' });
    expect(on.voiceEngine).toBe('elevenlabs');
    const removed = await setElevenKey(null);
    expect(removed.hasKey).toBe(false);
    expect(removed.settings.voiceEngine).toBe('windows');
    expect(removed.settings.elevenConsented).toBe(false);
  });

  it('Test voice says clearly when there is no key, and succeeds in the demo once saved', async () => {
    const no = await testElevenVoice();
    expect(no.ok).toBe(false);
    expect(no.message).toMatch(/save your ElevenLabs key/i);
    await setElevenKey('sk_demo_0123456789abcdefghij');
    const yes = await testElevenVoice();
    expect(yes.ok).toBe(true);
    expect(yes.message).toMatch(/Demo/);
  });
});
