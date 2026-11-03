import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import {
  CancelledError,
  MAX_CHARS,
  __test,
  actionCue,
  bulletMarkerLen,
  collapseWs,
  countText,
  coverageLabel,
  creditsFor,
  decisionCue,
  displayClean,
  findDates,
  formatDuration,
  highlightParts,
  idleSpeak,
  isCancelled,
  loadEngines,
  noteBody,
  normalizeLength,
  questionCue,
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
  summarizeLocalAsync,
  testElevenVoice,
  wordCount,
  contentTerms,
  type SpeakEvent,
  type Summary,
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

// Six unrelated headed sections, five sentences each (the same text as `TOPICS` in summary.rs).
const TOPICS = `## Coffee roasting

Roasting turns green coffee beans brown and brings out the flavours locked inside them. A light roast keeps the bright acidity of the bean while a dark roast tastes smoky and bitter. The roaster listens for the first crack, a popping sound that tells the roaster the beans are drying out. Freshly roasted coffee needs a few days to release carbon dioxide before it brews well. Home roasters often use a popcorn machine or a small drum roaster to practise.

## Comet observation

Comets are balls of ice and dust that grow glowing tails when they approach the sun. Astronomers track each comet by measuring its orbit across several nights of observation. A telescope with a wide field of view shows the faint tail of a comet far better than a narrow one. Some comets return every few decades while others will not be seen again for thousands of years. Photographing a comet needs a steady tripod and an exposure of several seconds.

## Vegetable gardening

Tomato seedlings should be planted outdoors only after the last frost has passed. Compost mixed into the soil feeds the roots and helps the garden hold moisture through summer. Gardeners water deeply in the morning so that leaves dry out before evening and fungus stays away. Beans fix nitrogen in the soil, which is why they are planted beside hungry crops like corn. Mulch around each plant suppresses weeds and keeps the soil cool.

## Railway signalling

Early railways used flags and lamps to tell drivers whether the line ahead was clear. Block signalling divides the track into sections so that only one train occupies a section at a time. Modern interlocking systems prevent a signaller from setting two conflicting routes by mistake. Track circuits detect a train by the short circuit its wheels make between the two rails. A failed signal defaults to red so that every fault leaves the railway in a safe state.

## Chess endgames

A rook and king against a lone king is a forced win that every serious player should know. The opposition decides most pawn endgames, because the king that holds it can usually escape or advance. Bishops of opposite colours often draw even when one side is two pawns ahead. Practising the Lucena position shows how a rook builds a bridge to shelter the king. Time pressure ruins many technically won endgames in club tournaments.

## Volcano monitoring

Seismometers on the flanks of a volcano record the small earthquakes that rising magma causes. Scientists also measure sulphur dioxide in the gas plume because a sudden increase often comes before an eruption. Satellites detect centimetres of swelling in the ground that no hiker would notice. Warning levels tell nearby villages when to prepare and when to leave. Past eruptions preserved in ash layers help forecasters judge what a volcano may do next.
`;

// Meeting notes with actions, decisions, dates and questions (the same text as `MEETING` in summary.rs).
const MEETING = `Weekly sync notes

Attendees: Dana, Omar, Priya and Lee.

We decided to move the launch from March to April because the payment provider certificate is delayed. Omar agreed to keep the old pricing page until the launch. The team approved the new onboarding flow after a short demo.

Action items
- Dana to send the revised launch plan to the partners by Friday.
- [ ] Omar will update the pricing page before the April launch.
- Priya needs to confirm the certificate renewal date with the provider.
- Review the onboarding copy with legal.
- Remember to book the venue for the April 14 launch party.

Open questions
Who will own the support rota after launch? The budget for paid ads is still TBD. It is unclear whether the mobile release can wait.

Next meeting is on Tuesday at 3 pm. The final go/no-go call happens on 2026-04-10.
`;

const CHINESE =
  '克罗维是一个轻量的桌面应用。它把常用的小工具放在一个货架上。剪贴板、计时器和笔记都在同一个地方。所有数据都保存在你的电脑上。语音输入完全在本机完成，不会上传任何内容。你可以用右键把任何内容发送到货架。总之，克罗维让日常工作更简单。计时器结束时会播放提示音。番茄钟会记录每天的专注次数。笔记支持清单和语音输入。计算器会记住上一次的表达式。文件货架可以临时存放拖进来的文件。';

/** The first three words of each chosen sentence - enough to identify it. */
const heads = (text: string, len: SummaryLength) => summarizeLocal(text, len).items.map((i) => i.text.split(' ').slice(0, 3).join(' '));
const texts = (s: Summary) => s.items.map((i) => i.text);

/** Deterministic pseudo-random document: `sections` headed sections, each with ten words of its own. */
function synthetic(sections: number, sentences: number, perPara: number): { doc: string; topics: string[][] } {
  const SYL = ['ka', 'to', 'mi', 'ra', 'su', 'ne', 'vo', 'li', 'pa', 'du', 'ze', 'fo'];
  const FILLER = ['system', 'process', 'result', 'often', 'during', 'several', 'important', 'measure', 'changes', 'usually', 'between', 'because', 'records', 'support', 'history', 'pattern', 'careful', 'complete', 'regular', 'overall', 'needed', 'common', 'recent', 'basic'];
  let x = 12345;
  const next = (m: number) => {
    x = (Math.imul(x, 1664525) + 1013904223) >>> 0;
    return (x >>> 8) % m;
  };
  const word = () => Array.from({ length: 4 }, () => SYL[next(12)]).join('');
  const topics: string[][] = [];
  let doc = '';
  for (let s = 0; s < sections; s++) {
    const tw = Array.from({ length: 10 }, word);
    doc += `## Chapter ${s} on ${tw[0]}\n\n`;
    for (let i = 0; i < sentences; i++) {
      const parts = Array.from({ length: 14 + next(9) }, () => (next(10) < 4 ? tw[next(10)] : FILLER[next(24)]));
      const sentence = parts.join(' ');
      doc += `${sentence[0].toUpperCase()}${sentence.slice(1)}.${(i + 1) % perPara === 0 ? '\n\n' : ' '}`;
    }
    topics.push(tw);
  }
  return { doc, topics };
}

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
  const lens: SummaryLength[] = ['short', 'detailed', 'key-points'];

  it('matches the Rust summariser sentence-for-sentence on the shared fixtures', () => {
    expect(heads(TECH, 'short')).toEqual(['A hash map', 'Instead of searching']);
    expect(heads(TECH, 'detailed')).toEqual(['A hash map', 'Instead of searching', 'Hash maps do']);
    expect(heads(NEWS, 'short')).toEqual(['The city council', 'Residents who live']);
    expect(heads(NEWS, 'detailed')).toEqual(['The city council', 'From June 1,', 'Residents who live']);
    // the sectioned pipeline picks the limits-of-cover sentence for Short (the old lead-biased one took "If the appliance")
    expect(heads(LEGAL, 'short')).toEqual(['This warranty covers', 'The warranty does']);
    expect(heads(LEGAL, 'detailed')).toEqual(['This warranty covers', 'If the appliance', 'The warranty does']);
  });

  it('accepts the pre-0.3 mode names as aliases', () => {
    expect(normalizeLength('medium')).toBe('detailed');
    expect(normalizeLength('points')).toBe('key-points');
    expect(normalizeLength('dates')).toBe('dates');
    expect(normalizeLength('nonsense')).toBe('detailed');
    expect(summarizeLocal(TECH, 'medium').text).toBe(summarizeLocal(TECH, 'detailed').text);
    expect(summarizeLocal(TECH, 'points').text).toBe(summarizeLocal(TECH, 'key-points').text);
    expect(summarizeLocal(TECH, 'medium').length).toBe('detailed');
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
      const points = summarizeLocal(text, 'key-points');
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
    const hits = summarizeLocal(dup, 'detailed').items.filter((i) => i.text.toLowerCase().includes('budget'));
    expect(hits.length).toBeLessThanOrEqual(1);
  });

  it('is fast on a long document', () => {
    const big = `${LEGAL}\n\n${TECH}\n\n${NEWS}\n\n`.repeat(40);
    const t = Date.now();
    const s = summarizeLocal(big, 'detailed');
    expect(s.items.length).toBeGreaterThanOrEqual(3);
    expect(Date.now() - t).toBeLessThan(15000);
  });
});

describe('sections, budget and coverage', () => {
  it('represents every section of a multi-section text', () => {
    const s = summarizeLocal(TOPICS, 'detailed');
    expect(s.sections).toBe(6);
    expect(s.sectionsCovered).toBe(6);
    expect(s.coverage).toBe(1);
    expect(s.sentencesConsidered).toBe(30);
    const topics = [['roast', 'coffee', 'bean'], ['comet', 'telescope', 'astronom'], ['tomato', 'garden', 'soil', 'compost', 'mulch'], ['signal', 'railway', 'track', 'train'], ['rook', 'endgame', 'bishop', 'pawn', 'king'], ['volcano', 'magma', 'eruption', 'seismometer', 'satellite']];
    topics.forEach((words, i) => {
      const it = s.items.find((x) => x.section === i);
      expect(it, `section ${i}`).toBeDefined();
      expect(words.some((w) => it!.text.toLowerCase().includes(w))).toBe(true);
    });
    expect(s.text.split('\n\n')).toHaveLength(6);
    expect(summarizeLocal(TOPICS, 'key-points').sectionsCovered).toBe(6);
    expect(summarizeLocal(TOPICS, 'short').items.length).toBeLessThanOrEqual(3);
  });

  it('grows the budget with the input but stays bounded; Short stays short', () => {
    const small = summarizeLocal(TOPICS, 'detailed');
    const { doc } = synthetic(12, 60, 6);
    const big = summarizeLocal(doc, 'detailed');
    expect(big.items.length).toBeGreaterThan(small.items.length);
    const share = big.items.length / big.sentencesConsidered;
    expect(share).toBeGreaterThan(0.04);
    expect(share).toBeLessThan(0.13);
    expect(summarizeLocal(doc, 'short').items.length).toBeLessThanOrEqual(3);
  });

  it('cuts text without headings into chunks that are all visited', () => {
    const { doc } = synthetic(1, 240, 6);
    const plain = doc.slice(doc.indexOf('\n\n') + 2);
    expect(splitUnits(plain).some((u) => u.kind === 'heading')).toBe(false);
    const s = summarizeLocal(plain, 'detailed');
    expect(s.sections).toBeGreaterThanOrEqual(8);
    expect(s.sectionsCovered).toBe(s.sections);
    expect(s.items[s.items.length - 1].index - s.items[0].index).toBeGreaterThan(150);
  });

  it('does not pre-filter an off-topic last section away', () => {
    const { doc } = synthetic(1, 1600, 8);
    const tail = '## Notes on penguins\n\nEmperor penguins huddle together through the Antarctic winter to share warmth. The males balance a single egg on their feet for two months without eating. Chicks are fed a rich milk made in the throat of both parents. Colonies gather on sea ice that forms each autumn along the coast.\n';
    const s = summarizeLocal(`${doc}\n${tail}`, 'detailed');
    expect(s.items.some((i) => /penguin|chick|egg|colonies/i.test(i.text))).toBe(true);
    expect(s.sectionsCovered).toBe(s.sections);
  });

  it('handles 60 000 words, covers every section and stays quick', () => {
    const { doc, topics } = synthetic(40, 90, 6);
    expect(wordCount(doc)).toBeGreaterThan(55000);
    const t = Date.now();
    const s = summarizeLocal(doc, 'detailed');
    const ms = Date.now() - t;
    expect(ms).toBeLessThan(8000);
    expect(s.sections).toBeGreaterThanOrEqual(40);
    expect(s.sectionsCovered).toBe(s.sections);
    topics.forEach((tw, i) => {
      expect(
        s.items.some((it) => tw.some((w) => it.text.toLowerCase().includes(w))),
        `chapter ${i}`,
      ).toBe(true);
    });
  });

  it('says how much was considered', () => {
    const s = summarizeLocal(TOPICS, 'detailed');
    expect(coverageLabel(s)).toBe('Covers all 6 sections · 30 sentences read');
    const one = summarizeLocal(TECH, 'detailed');
    expect(coverageLabel(one)).toMatch(/^\d+ sentences read$/);
    expect(coverageLabel(summarizeLocal('Short note. Nothing here.', 'short'))).toBe('');
    expect(coverageLabel(summarizeLocal(MEETING, 'dates'))).toMatch(/^Found in \d+ of 3 sections · \d+ sentences scanned$/);
  });
});

describe('key points and short list items', () => {
  const LIST = 'Weekend list\n\n- Buy milk\n- Call the bank\n- Fix the sink\n- Water the plants\n- Book flights\n- Renew passport\n- Wash the car\n- Pay rent\n- Email Sam\n- Walk the dog\n- Pack bags\n- Print tickets\n';

  it('may pick list items of two or three words', () => {
    const s = summarizeLocal(LIST, 'key-points');
    expect(s.alreadyShort).toBe(false);
    expect(s.items.length).toBeGreaterThanOrEqual(3);
    expect(s.items.every((i) => wordCount(i.text) < 4)).toBe(true);
    expect(s.text.split('\n').every((l) => l.startsWith('•'))).toBe(true);
  });

  it('other modes still skip fragments', () => {
    const longer = `${LIST}- Make sure the tickets and the passports are in the same bag before leaving\n- Ask the neighbours to collect the post and to feed the cat while we are away\n`;
    for (const it of summarizeLocal(longer, 'detailed').items) expect(wordCount(it.text)).toBeGreaterThanOrEqual(4);
  });
});

describe('extraction modes', () => {
  it('finds action items, owners and checkboxes', () => {
    const s = summarizeLocal(MEETING, 'action-items');
    const t = texts(s);
    const has = (needle: string) => t.some((x) => x.includes(needle));
    expect(has('Dana to send the revised launch plan')).toBe(true);
    expect(has('Omar will update the pricing page')).toBe(true);
    expect(has('Priya needs to confirm')).toBe(true);
    expect(has('Review the onboarding copy')).toBe(true);
    expect(has('Remember to book the venue')).toBe(true);
    expect(has('We decided to move')).toBe(false);
    expect(has('Next meeting is on Tuesday')).toBe(false);
    expect(t.every((x) => !x.startsWith('['))).toBe(true);
    expect(s.text.split('\n').every((l) => l.startsWith('•'))).toBe(true);
    expect(s.alreadyShort).toBe(false);
  });

  it('finds decisions', () => {
    const s = summarizeLocal(MEETING, 'decisions');
    const t = texts(s);
    expect(t).toHaveLength(3);
    expect(t[0]).toContain('We decided to move the launch');
    expect(t[1]).toContain('Omar agreed to keep');
    expect(t[2]).toContain('The team approved the new onboarding flow');
    expect(s.items.map((i) => i.hit)).toEqual(['decided', 'agreed', 'approved']);
  });

  it('finds dates with the date phrase', () => {
    const s = summarizeLocal(MEETING, 'dates');
    const by = (needle: string) => s.items.find((i) => i.text.includes(needle));
    expect(by('Dana to send')?.hit).toBe('by Friday');
    expect(by('Next meeting')?.hit).toBe('on Tuesday, at 3 pm');
    expect(by('go/no-go')?.hit).toBe('on 2026-04-10');
    expect(by('April 14')).toBeDefined();
    expect(by('Review the onboarding copy')).toBeUndefined();
    expect(s.text).toContain('by Friday: Dana to send');
  });

  it('finds open questions', () => {
    const s = summarizeLocal(MEETING, 'questions');
    expect(texts(s)).toEqual(['Who will own the support rota after launch?', 'The budget for paid ads is still TBD.', 'It is unclear whether the mobile release can wait.']);
    expect(s.items.map((i) => i.hit)).toEqual(['?', 'TBD', 'unclear']);
  });

  it('works on input too short to summarise and says when nothing matches', () => {
    const one = summarizeLocal('Send the invoice by Friday.', 'dates');
    expect(one.alreadyShort).toBe(false);
    expect(one.items).toHaveLength(1);
    const none = summarizeLocal('Nothing planned for the weekend at all.', 'questions');
    expect(none.items).toHaveLength(0);
    expect(none.note).toMatch(/No open questions/);
  });

  it('drops repeats and says when it stops listing', () => {
    const one = 'Please send the signed contract back to the legal team by Friday afternoon.';
    expect(summarizeLocal(`${one} ${one} Please book a room for the kickoff meeting next week.`, 'action-items').items).toHaveLength(2);
    const { doc } = synthetic(1, 400, 400);
    const many = doc
      .split('\n\n')[1]
      .split('. ')
      .map((s, i) => `Please review the ${s.split(' ')[0]}${i} report number ${i * 7} before the deadline.`)
      .join(' ');
    const s = summarizeLocal(many, 'action-items');
    expect(s.items.length).toBeLessThanOrEqual(150);
    expect(s.items.length).toBeGreaterThan(100);
    if (s.items.length === 150) expect(s.note).toMatch(/strongest/);
  });

  it('detects cues one by one', () => {
    expect(findDates('Send the report by Friday.')).toEqual(['by Friday']);
    expect(findDates('Launch is in Q3 2026.')).toEqual(['in Q3 2026']);
    expect(findDates('Call at 15:30 on 3 Nov 2026.')).toEqual(['at 15:30', 'on 3 Nov 2026']);
    expect(findDates('Meet next Monday at 10am.')).toEqual(['next Monday', 'at 10am']);
    expect(findDates('Reply within 30 days please.')).toEqual(['within 30 days']);
    expect(findDates('In March we ship.')).toEqual(['In March']);
    expect(findDates('Ready by EOD tomorrow.')).toEqual(['by EOD', 'tomorrow']);
    expect(findDates('Done by the end of the month.')).toEqual(['by the end of the month']);
    expect(findDates('Doors open at 3 pm EST.')).toEqual(['at 3 pm EST']);
    expect(findDates('Launch April 14, 2026 at the venue.')).toEqual(['April 14, 2026']);
    expect(findDates('I may be late and the march was long.')).toEqual([]);
    expect(findDates('We sat in the sun and ate 12 apples.')).toEqual([]);
    expect(findDates('Version 3.5.1 has 1.5 million users.')).toEqual([]);

    const a = (s: string, bullet = false) => actionCue(s, bullet)?.[1] ?? null;
    expect(a('Dana to send the revised plan by Friday.')).toBe('Dana to send');
    expect(a('Priya needs to confirm the date.')).toBe('needs to');
    expect(a('[ ] Omar will update the page.', true)).toBe('checkbox');
    expect(a('Buy milk', true)).toBe('Buy');
    expect(a('Dana will send the draft on Monday.')).toBe('Dana will');
    expect(a('Can you send the notes?')).toBe('Can you');
    expect(a('The park will stay open until 10 p.m.')).toBeNull();
    expect(a('Review of the data shows growth.')).toBeNull();
    expect(a('Who will own the rota?')).toBeNull();

    expect(decisionCue('The board approved the budget.')).toBe('approved');
    expect(decisionCue("We'll go with option B.")).toBe("We'll go with");
    expect(decisionCue('The date is still to be decided.')).toBeNull();
    expect(decisionCue('Once approved, we ship.')).toBeNull();
    expect(questionCue('He asked "why?"')).toBe('?');
    expect(questionCue('Everything is settled.')).toBeNull();
  });
});

describe('space-less scripts', () => {
  it('counts Han, kana and Thai by character groups, not as one word', () => {
    expect(wordCount('今日は晴れです。')).toBe(4);
    expect(wordCount('Hello世界 ok')).toBe(3);
    expect(wordCount('สวัสดีครับ')).toBe(3);
    expect(wordCount('。、')).toBe(0);
    expect(wordCount(CHINESE)).toBeGreaterThan(60);
  });

  it('turns them into bigram terms', () => {
    expect(contentTerms('今天天气很好')).toHaveLength(5);
    expect(contentTerms('Krovi 很好')).toEqual(expect.arrayContaining(['krovi', '很好']));
  });

  it('summarises Chinese text', () => {
    const s = summarizeLocal(CHINESE, 'detailed');
    expect(s.alreadyShort).toBe(false);
    expect(s.stats.words).toBeGreaterThan(60);
    expect(s.items.length).toBeGreaterThanOrEqual(3);
    expect(s.stats.ratio).toBeLessThan(0.8);
    const short = summarizeLocal(CHINESE, 'short');
    expect(short.items.length).toBeGreaterThanOrEqual(1);
    expect(short.items.length).toBeLessThanOrEqual(2);
  });

  it('ranks Chinese sentences by shared characters', () => {
    const input = '委员会今天批准了明年的预算方案。预算方案包括教育和交通两部分。委员会认为预算方案需要更多讨论。预算方案将在下周公布。市民可以在网上查看预算方案的细节。委员会还将举行一次关于预算方案的公开会议。猫坐在窗台上睡觉。';
    const s = summarizeLocal(input, 'short');
    expect(s.items.length).toBeGreaterThan(0);
    for (const it of s.items) expect(it.text).toContain('预算');
  });
});

describe('cancellation and limits', () => {
  it('stops between phases when asked', () => {
    expect(() => summarizeLocal(NEWS, 'detailed', () => true)).toThrow(CancelledError);
    expect(() => summarizeLocal(NEWS, 'dates', () => true)).toThrow(/Summary cancelled/);
    let calls = 0;
    const s = summarizeLocal(NEWS, 'detailed', () => (++calls, false));
    expect(s.items.length).toBeGreaterThan(0);
    expect(calls).toBeGreaterThan(2);
    expect(isCancelled(new CancelledError())).toBe(true);
    expect(isCancelled('Summary cancelled.')).toBe(true);
    expect(isCancelled(new Error('boom'))).toBe(false);
  });

  it('the async driver stops on abort and otherwise matches the sync one', async () => {
    const ctl = new AbortController();
    ctl.abort();
    await expect(summarizeLocalAsync(NEWS, 'short', ctl.signal)).rejects.toThrow(CancelledError);
    await expect(summarizeLocalAsync(TOPICS, 'detailed')).resolves.toMatchObject({ text: summarizeLocal(TOPICS, 'detailed').text });
    const mid = new AbortController();
    const { doc } = synthetic(20, 60, 6);
    const run = summarizeLocalAsync(doc, 'detailed', mid.signal);
    setTimeout(() => mid.abort(), 0);
    await expect(run).rejects.toThrow(CancelledError);
  });

  it('refuses over-long input explicitly instead of cutting it', () => {
    const big = 'word '.repeat(MAX_CHARS / 5 + 10);
    expect(() => summarizeLocal(big, 'short')).toThrow(/summarizes up to 1,000,000/);
  });
});

describe('display cleaning', () => {
  it('removes markdown noise but keeps file_name.rs', () => {
    expect(displayClean('**Bold** point.')).toBe('Bold point.');
    expect(displayClean('run `cargo test` now')).toBe('run cargo test now');
    expect(displayClean('| --- | --- |')).toBe('');
    expect(displayClean('| Template | Examples |')).toBe('Template · Examples');
    expect(displayClean('file_name.rs is kept')).toBe('file_name.rs is kept');
    expect(displayClean('#### Heading line')).toBe('Heading line');
  });

  it('splits markdown headings, tables and danda like the Rust splitter', () => {
    const u = splitUnits('#### Krovi tools\n\nThe shelf holds widgets. Timers live here too.');
    expect(u[0]).toMatchObject({ kind: 'heading', text: 'Krovi tools' });
    expect(splitUnits('यह पहला वाक्य है। यह दूसरा वाक्य है। और यह तीसरा है।')).toHaveLength(3);
    expect(splitUnits('#tag trends on social media. Sometimes twice.')[0].kind).toBe('sentence');
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
