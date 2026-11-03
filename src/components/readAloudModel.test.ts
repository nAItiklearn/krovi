import { describe, expect, it } from 'vitest';
import { RATE_MAX, RATE_MIN, type WinVoice } from '../lib/explain';
import { PREVIEW_SENTENCE, clampRate, elevenStatus, ratePercent, rateLabel, selectedVoiceId, voiceOptions } from './readAloudModel';

const voice = (id: string, name: string, over: Partial<WinVoice> = {}): WinVoice => ({ id, name, language: 'en-US', gender: '', natural: false, ...over });
const david = voice('david', 'Microsoft David - English (United States)');
const aria = voice('aria', 'Microsoft Aria Online (Natural) - English (United States)', { natural: true, language: 'en-GB' });

describe('voiceOptions', () => {
  it('names each voice the way the Explain tool does: short name, (natural), language', () => {
    expect(voiceOptions([david, aria])).toEqual([
      { id: 'david', label: 'Microsoft David · en-US' },
      { id: 'aria', label: 'Microsoft Aria Online (Natural) (natural) · en-GB' },
    ]);
  });

  it('leaves the language out when Windows did not report one, and copes with no voices', () => {
    expect(voiceOptions([voice('x', 'Zira', { language: '' })])).toEqual([{ id: 'x', label: 'Zira' }]);
    expect(voiceOptions([])).toEqual([]);
  });
});

describe('selectedVoiceId', () => {
  const windows = { voices: [david, aria], defaultVoiceId: 'aria' };

  it('uses the saved voice while Windows still has it', () => {
    expect(selectedVoiceId('david', windows)).toBe('david');
  });

  it('falls back to the Windows default when nothing is saved or the saved voice was uninstalled', () => {
    expect(selectedVoiceId(null, windows)).toBe('aria');
    expect(selectedVoiceId('gone', windows)).toBe('aria');
  });

  it('falls back to the first voice when the default is missing too, and to nothing without voices', () => {
    expect(selectedVoiceId(null, { voices: [david, aria], defaultVoiceId: null })).toBe('david');
    expect(selectedVoiceId(null, { voices: [david], defaultVoiceId: 'gone' })).toBe('david');
    expect(selectedVoiceId('david', { voices: [], defaultVoiceId: null })).toBe('');
    expect(selectedVoiceId('david', null)).toBe('');
    expect(selectedVoiceId(null, undefined)).toBe('');
  });
});

describe('speed', () => {
  it('keeps the speed inside the range the backend accepts', () => {
    expect(clampRate(0.1)).toBe(RATE_MIN);
    expect(clampRate(9)).toBe(RATE_MAX);
    expect(clampRate(1.2)).toBe(1.2);
    expect(clampRate(Number.NaN)).toBe(1);
  });

  it('labels it with one decimal and a multiplication sign', () => {
    expect(rateLabel(1)).toBe('1.0×');
    expect(rateLabel(1.25)).toBe('1.3×');
    expect(rateLabel(5)).toBe(`${RATE_MAX.toFixed(1)}×`);
  });

  it('places the slider fill between 0 and 100 percent', () => {
    expect(ratePercent(RATE_MIN)).toBe(0);
    expect(ratePercent(RATE_MAX)).toBe(100);
    expect(ratePercent(100)).toBe(100);
    const mid = ratePercent((RATE_MIN + RATE_MAX) / 2);
    expect(mid).toBeGreaterThan(49.9);
    expect(mid).toBeLessThan(50.1);
  });
});

describe('elevenStatus', () => {
  it('no key: optional, stays on this PC, nothing is sent', () => {
    for (const el of [null, undefined, { hasKey: false, keyHint: null }]) {
      const s = elevenStatus(el, { voiceEngine: 'windows', elevenVoiceName: null });
      expect(s).toMatchObject({ tone: 'off', label: 'Not set up', sendsText: false });
      expect(s.detail).toContain('Nothing is sent');
    }
  });

  it('a saved key that is not selected sends nothing, and shows only the last characters', () => {
    const s = elevenStatus({ hasKey: true, keyHint: 'a1b2' }, { voiceEngine: 'windows', elevenVoiceName: 'Rachel' });
    expect(s).toMatchObject({ tone: 'ready', label: 'Key saved', sendsText: false });
    expect(s.detail).toContain('ends in a1b2');
    expect(s.detail).toContain('Nothing is sent');
  });

  it('when ElevenLabs is the reading voice it says plainly that the text is sent', () => {
    const s = elevenStatus({ hasKey: true, keyHint: null }, { voiceEngine: 'elevenlabs', elevenVoiceName: 'Rachel - young american' });
    expect(s).toMatchObject({ tone: 'on', label: 'In use', sendsText: true });
    expect(s.detail).toContain('sent to ElevenLabs');
    expect(s.detail).toContain('Rachel');
    expect(s.detail).not.toContain('key ends in');
  });

  it('the engine choice means nothing without a key (a stale "elevenlabs" never claims text is sent)', () => {
    const s = elevenStatus({ hasKey: false, keyHint: null }, { voiceEngine: 'elevenlabs', elevenVoiceName: 'Rachel' });
    expect(s).toMatchObject({ tone: 'off', sendsText: false });
  });
});

describe('preview', () => {
  it('speaks one fixed sentence', () => {
    expect(PREVIEW_SENTENCE).toMatch(/^Hello from Krovi\./);
  });
});
