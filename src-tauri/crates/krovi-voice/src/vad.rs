//! Voice activity detection for 16 kHz mono audio: pure, deterministic, cheap.
//!
//! Energy-based with an adaptive noise floor. It decides three things for the local
//! dictation path:
//! * **is anyone speaking** (so a silent recording is never sent to Whisper, which tends
//!   to hallucinate "Thank you." on silence),
//! * **where speech starts and ends** ([`trim`]: leading/trailing silence is cut before
//!   transcription, which saves time and memory),
//! * **end of speech** for hands-free mode ([`Endpointer`]: speech, then a configurable
//!   stretch of silence).
//!
//! It is not a neural VAD; it is tuned to be conservative (keep speech, trim only clear
//! silence) and is unit-tested on synthetic signals.

/// Samples per second the whole local speech path uses.
pub const SAMPLE_RATE: usize = 16_000;
/// Analysis frame: 30 ms.
pub const FRAME: usize = SAMPLE_RATE * 30 / 1000;
/// RMS below this is silence whatever the floor (≈ -54 dBFS).
const ABS_MIN_RMS: f32 = 0.002;
/// Speech must be this many times louder than the noise floor.
const FLOOR_RATIO: f32 = 3.0;
/// Frames of padding kept around detected speech by [`trim`] (≈ 210 ms).
const PAD_FRAMES: usize = 7;

/// Root-mean-square level of a buffer (0.0 for empty).
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// Per-frame RMS levels.
pub fn frame_levels(samples: &[f32]) -> Vec<f32> {
    samples.chunks(FRAME).map(rms).collect()
}

/// Noise floor: the 10th-percentile frame level (robust to a few loud frames).
pub fn noise_floor(levels: &[f32]) -> f32 {
    if levels.is_empty() {
        return 0.0;
    }
    let mut v: Vec<f32> = levels.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[v.len() / 10]
}

/// Room noise is rarely louder than this (≈ -44 dBFS). Capping the floor keeps a clip that
/// is speech from start to finish (push-to-talk, no pauses) from being mistaken for noise.
const MAX_FLOOR: f32 = 0.006;

fn threshold(floor: f32) -> f32 {
    (floor.min(MAX_FLOOR) * FLOOR_RATIO).max(ABS_MIN_RMS)
}

/// Shortest loud run counted as speech: 3 frames = 90 ms (a click, even one that straddles
/// two frames, is shorter; a syllable is longer).
const MIN_RUN: usize = 3;

/// Frame-by-frame speech decision: loud frames count only inside a run of at least
/// [`MIN_RUN`] consecutive loud frames.
pub fn speech_frames(levels: &[f32]) -> Vec<bool> {
    let thr = threshold(noise_floor(levels));
    let raw: Vec<bool> = levels.iter().map(|&l| l > thr).collect();
    let mut out = vec![false; raw.len()];
    let mut i = 0;
    while i < raw.len() {
        if !raw[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < raw.len() && raw[i] {
            i += 1;
        }
        if i - start >= MIN_RUN {
            out[start..i].iter_mut().for_each(|v| *v = true);
        }
    }
    out
}

/// Total voiced duration in milliseconds.
pub fn voiced_ms(samples: &[f32]) -> u64 {
    let n = speech_frames(&frame_levels(samples))
        .iter()
        .filter(|&&s| s)
        .count();
    (n * FRAME * 1000 / SAMPLE_RATE) as u64
}

/// Enough speech to be worth transcribing (at least `min_ms` of voiced frames).
pub fn has_speech(samples: &[f32], min_ms: u64) -> bool {
    voiced_ms(samples) >= min_ms
}

/// The sample range holding speech, padded; `None` when there is no speech.
pub fn speech_bounds(samples: &[f32]) -> Option<(usize, usize)> {
    let frames = speech_frames(&frame_levels(samples));
    let first = frames.iter().position(|&s| s)?;
    let last = frames.iter().rposition(|&s| s)?;
    let start = first.saturating_sub(PAD_FRAMES) * FRAME;
    let end = ((last + 1 + PAD_FRAMES) * FRAME).min(samples.len());
    Some((start, end))
}

/// Cut leading and trailing silence; returns an empty slice when nothing was said.
pub fn trim(samples: &[f32]) -> &[f32] {
    match speech_bounds(samples) {
        Some((a, b)) => &samples[a..b],
        None => &samples[..0],
    }
}

/// Streaming end-of-speech detector for hands-free capture. Feed frames as they arrive;
/// it reports [`Endpoint::Ended`] once speech was heard and then `silence_ms` of quiet
/// followed, or [`Endpoint::NoSpeech`] if nothing was said within `lead_ms`.
#[derive(Debug, Clone)]
pub struct Endpointer {
    silence_frames: usize,
    lead_frames: usize,
    floor: f32,
    seen: usize,
    heard: bool,
    quiet_run: usize,
    voiced_run: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    Listening,
    Speaking,
    Ended,
    NoSpeech,
}

impl Endpointer {
    pub fn new(silence_ms: u64, lead_ms: u64) -> Self {
        let per = |ms: u64| ((ms as usize * SAMPLE_RATE / 1000) / FRAME).max(1);
        Self {
            silence_frames: per(silence_ms),
            lead_frames: per(lead_ms),
            floor: f32::MAX,
            seen: 0,
            heard: false,
            quiet_run: 0,
            voiced_run: 0,
        }
    }

    /// Feed one frame (≈ [`FRAME`] samples).
    pub fn push(&mut self, frame: &[f32]) -> Endpoint {
        let level = rms(frame);
        self.seen += 1;
        // track the floor: fast down, slow up (so speech does not lift it)
        self.floor = if self.floor == f32::MAX || level < self.floor {
            level
        } else {
            self.floor * 0.995 + level * 0.005
        };
        let voiced = level > threshold(self.floor);
        if voiced {
            self.voiced_run += 1;
            self.quiet_run = 0;
            if self.voiced_run >= MIN_RUN {
                self.heard = true;
            }
        } else {
            self.voiced_run = 0;
            self.quiet_run += 1;
        }
        if self.heard {
            if self.quiet_run >= self.silence_frames {
                Endpoint::Ended
            } else {
                Endpoint::Speaking
            }
        } else if self.seen >= self.lead_frames {
            Endpoint::NoSpeech
        } else {
            Endpoint::Listening
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(ms: usize, amp: f32) -> Vec<f32> {
        let n = SAMPLE_RATE * ms / 1000;
        (0..n)
            .map(|i| {
                amp * (i as f32 * 2.0 * std::f32::consts::PI * 220.0 / SAMPLE_RATE as f32).sin()
            })
            .collect()
    }
    fn hiss(ms: usize, amp: f32) -> Vec<f32> {
        // deterministic pseudo-noise
        let n = SAMPLE_RATE * ms / 1000;
        let mut x: u32 = 12345;
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                amp * ((x >> 16) as f32 / 32_768.0 - 1.0)
            })
            .collect()
    }
    fn cat(parts: &[Vec<f32>]) -> Vec<f32> {
        parts.concat()
    }

    #[test]
    fn rms_of_sine_and_empty() {
        assert_eq!(rms(&[]), 0.0);
        let r = rms(&tone(1000, 0.5));
        assert!((r - 0.5 / 2f32.sqrt()).abs() < 0.01, "{r}");
    }

    #[test]
    fn silence_and_room_noise_are_not_speech() {
        assert!(!has_speech(&vec![0.0; SAMPLE_RATE * 3], 200));
        assert!(!has_speech(&hiss(3000, 0.001), 200));
        assert!(trim(&hiss(2000, 0.001)).is_empty());
    }

    #[test]
    fn speech_over_noise_is_detected_and_trimmed() {
        let audio = cat(&[hiss(1500, 0.004), tone(1200, 0.2), hiss(1500, 0.004)]);
        assert!(has_speech(&audio, 500));
        let (a, b) = speech_bounds(&audio).unwrap();
        let start_ms = a * 1000 / SAMPLE_RATE;
        let end_ms = b * 1000 / SAMPLE_RATE;
        assert!((1200..=1500).contains(&start_ms), "start {start_ms}");
        assert!((2700..=3000).contains(&end_ms), "end {end_ms}");
        assert!(trim(&audio).len() < audio.len());
    }

    #[test]
    fn a_clip_that_is_all_speech_still_counts() {
        let audio = tone(1500, 0.15);
        assert!(has_speech(&audio, 1000));
        assert_eq!(trim(&audio).len(), audio.len());
    }

    #[test]
    fn a_single_click_is_ignored() {
        let mut audio = vec![0.0; SAMPLE_RATE * 2];
        for s in audio.iter_mut().skip(16_000).take(FRAME) {
            *s = 0.5;
        }
        assert!(!has_speech(&audio, 30));
    }

    #[test]
    fn endpointer_ends_after_silence_following_speech() {
        let mut ep = Endpointer::new(600, 5000);
        let audio = cat(&[hiss(500, 0.003), tone(1000, 0.2), hiss(2000, 0.003)]);
        let mut states = Vec::new();
        for f in audio.chunks(FRAME) {
            states.push(ep.push(f));
        }
        assert!(states.contains(&Endpoint::Speaking));
        let ended_at = states.iter().position(|s| *s == Endpoint::Ended).unwrap();
        let ms = ended_at * FRAME * 1000 / SAMPLE_RATE;
        // speech stops at 1500 ms, + 600 ms silence
        assert!((2050..=2250).contains(&ms), "ended at {ms} ms");
    }

    #[test]
    fn endpointer_gives_up_when_nobody_speaks() {
        let mut ep = Endpointer::new(600, 1000);
        let mut last = Endpoint::Listening;
        for f in hiss(1500, 0.002).chunks(FRAME) {
            last = ep.push(f);
            if last == Endpoint::NoSpeech {
                break;
            }
        }
        assert_eq!(last, Endpoint::NoSpeech);
    }

    #[test]
    fn long_recording_is_fast() {
        // 20 minutes of audio
        let audio = hiss(20 * 60 * 1000, 0.003);
        let t = std::time::Instant::now();
        let _ = trim(&audio);
        assert!(t.elapsed().as_secs_f32() < 5.0, "{:?}", t.elapsed());
    }
}
