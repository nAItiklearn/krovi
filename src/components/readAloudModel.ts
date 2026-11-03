// Pure helpers for Settings > Read aloud (unit-tested). The voice list, the speed limits and the
// ElevenLabs key status all come from lib/explain.ts (the Explain tool's own commands); nothing
// here talks to Rust or decides anything the backend decides.
import { RATE_MAX, RATE_MIN, shortVoiceName, type Engines, type ExplainSettings, type WinVoice } from '../lib/explain';

/** The sentence "Preview" speaks. Fixed, so a preview never reads anything the user wrote or copied. */
export const PREVIEW_SENTENCE = 'Hello from Krovi. This is the voice that will read your summary.';

export interface VoiceOption {
  id: string;
  label: string;
}

/** "Microsoft David (natural) · en-US", the same wording the Explain tool's voice list uses. */
export function voiceOptions(voices: readonly WinVoice[]): VoiceOption[] {
  return voices.map((v) => ({
    id: v.id,
    label: `${shortVoiceName(v.name)}${v.natural ? ' (natural)' : ''}${v.language ? ` · ${v.language}` : ''}`,
  }));
}

/**
 * The voice the dropdown should show: the saved one if Windows still has it, else the Windows default,
 * else the first voice, else nothing. (A voice that was uninstalled after being chosen must not leave
 * the dropdown blank or pointing at something that cannot speak.)
 */
export function selectedVoiceId(
  saved: ExplainSettings['windowsVoice'],
  windows: Pick<Engines['windows'], 'voices' | 'defaultVoiceId'> | null | undefined,
): string {
  const voices = windows?.voices ?? [];
  const has = (id: string | null | undefined): id is string => !!id && voices.some((v) => v.id === id);
  if (has(saved)) return saved;
  const fallback = windows?.defaultVoiceId;
  if (has(fallback)) return fallback;
  return voices[0]?.id ?? '';
}

export function clampRate(rate: number): number {
  if (!Number.isFinite(rate)) return 1;
  return Math.min(RATE_MAX, Math.max(RATE_MIN, rate));
}

/** "1.2×" */
export function rateLabel(rate: number): string {
  return `${clampRate(rate).toFixed(1)}×`;
}

/** 0..100, how far along the slider track the rate sits (the slider fill is drawn from it). */
export function ratePercent(rate: number): number {
  return ((clampRate(rate) - RATE_MIN) / (RATE_MAX - RATE_MIN)) * 100;
}

export interface ElevenStatus {
  /** off: no key, ready: key saved but not used, on: reading aloud goes to ElevenLabs. */
  tone: 'off' | 'ready' | 'on';
  label: string;
  detail: string;
  /** True only when text is actually sent to ElevenLabs while reading aloud. */
  sendsText: boolean;
}

/** What to tell the user about the optional ElevenLabs voice, from the engine report and the saved settings. */
export function elevenStatus(
  el: Pick<Engines['elevenlabs'], 'hasKey' | 'keyHint'> | null | undefined,
  settings: Pick<ExplainSettings, 'voiceEngine' | 'elevenVoiceName'>,
): ElevenStatus {
  if (!el?.hasKey) {
    return {
      tone: 'off',
      label: 'Not set up',
      detail:
        'Reading aloud uses the Windows voice above, which stays on this PC. Nothing is sent anywhere. To use ElevenLabs instead, add your own key in Explain › Voice settings.',
      sendsText: false,
    };
  }
  const hint = el.keyHint ? ` (key ends in ${el.keyHint})` : '';
  if (settings.voiceEngine === 'elevenlabs') {
    const voice = settings.elevenVoiceName ? ` ${shortVoiceName(settings.elevenVoiceName)}` : '';
    return {
      tone: 'on',
      label: 'In use',
      detail: `Summaries are read with your ElevenLabs voice${voice}${hint}. The sentences being read are sent to ElevenLabs. Change it in Explain › Voice settings.`,
      sendsText: true,
    };
  }
  return {
    tone: 'ready',
    label: 'Key saved',
    detail: `Your ElevenLabs key is saved${hint}, but reading aloud still uses the Windows voice. Nothing is sent. Switch in Explain › Voice settings.`,
    sendsText: false,
  };
}
