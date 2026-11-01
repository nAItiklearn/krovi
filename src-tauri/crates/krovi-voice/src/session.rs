//! The listening-session state machine: turns raw recognizer events into "keep going / finish
//! because X" decisions and the final text. Pure and clock-agnostic (callers pass milliseconds
//! since the session started), so every timeout rule is unit-tested without audio or COM.

use crate::types::Mode;

/// Events the engine layer reports, already decoded from SAPI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Raw {
    SoundStart,
    SoundEnd,
    PhraseStart,
    /// Provisional text for the phrase currently being spoken.
    Hypothesis(String),
    /// A finished phrase.
    Final(String),
    /// The engine's confidence (0-100) in the phrase reported right after it.
    Confidence(u8),
    /// Speech was detected but could not be matched (SAPI "false recognition").
    Rejected,
    /// The audio source ended (file input, or the device went away).
    StreamEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Finish {
    /// `stop()` was called.
    Stopped,
    /// Speech, then quiet for the configured time.
    Silence,
    /// Nothing was ever said.
    NoSpeech,
    /// Hit the hard cap.
    MaxDuration,
    /// The audio stream ended.
    StreamEnd,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Policy {
    /// Give up if no sound at all begins within this long.
    pub no_speech_ms: u64,
    /// Finish after this much quiet following the last speech activity.
    pub silence_ms: u64,
    /// Hard cap on one session.
    pub max_ms: u64,
    /// After `stop()`, how long to wait for an in-flight phrase to be finalised.
    pub stop_grace_ms: u64,
}

impl Policy {
    pub(crate) fn for_mode(mode: Mode) -> Policy {
        match mode {
            // Dictation: a thinking pause between sentences is normal. The engine itself takes
            // ~0.5 s to close an utterance, so 2.0 s here is ~2.5 s of silence for the user.
            Mode::Note => Policy {
                no_speech_ms: 8_000,
                silence_ms: 2_000,
                max_ms: 120_000,
                stop_grace_ms: 1_500,
            },
            // Commands are one short utterance; finish quickly once it is recognised.
            Mode::Command => Policy {
                no_speech_ms: 6_000,
                silence_ms: 900,
                max_ms: 20_000,
                stop_grace_ms: 1_200,
            },
        }
    }

    /// File input is read faster than real time and ends by itself; no wall-clock rules apply.
    pub(crate) fn for_file() -> Policy {
        Policy {
            no_speech_ms: u64::MAX / 4,
            silence_ms: u64::MAX / 4,
            max_ms: 120_000,
            stop_grace_ms: 0,
        }
    }
}

pub(crate) struct Collector {
    policy: Policy,
    finals: Vec<String>,
    /// Latest provisional text for the current phrase ("" once finalised).
    hypothesis: String,
    in_sound: bool,
    heard_sound: bool,
    last_activity_ms: u64,
    stop_at_ms: Option<u64>,
    stream_ended: bool,
    /// Lowest engine confidence (0-100) among the finished phrases that reported one.
    min_confidence: Option<u8>,
}

impl Collector {
    pub(crate) fn new(policy: Policy) -> Self {
        Collector {
            policy,
            finals: Vec::new(),
            hypothesis: String::new(),
            in_sound: false,
            heard_sound: false,
            last_activity_ms: 0,
            stop_at_ms: None,
            stream_ended: false,
            min_confidence: None,
        }
    }

    /// The least confident finished phrase, 0.0..=1.0 (`None` when the engine reported none).
    pub(crate) fn min_confidence(&self) -> Option<f32> {
        self.min_confidence.map(|c| f32::from(c) / 100.0)
    }

    /// Feed one event. Returns `Some(text)` when the live partial text changed and should be shown.
    pub(crate) fn on_raw(&mut self, ev: Raw, now_ms: u64) -> Option<String> {
        match ev {
            Raw::SoundStart => {
                self.in_sound = true;
                self.heard_sound = true;
                self.last_activity_ms = now_ms;
                None
            }
            Raw::SoundEnd => {
                self.in_sound = false;
                None
            }
            Raw::PhraseStart => {
                self.heard_sound = true;
                self.last_activity_ms = now_ms;
                None
            }
            Raw::Hypothesis(text) => {
                self.heard_sound = true;
                self.last_activity_ms = now_ms;
                let text = text.trim().to_string();
                if text.is_empty() || text == self.hypothesis {
                    return None;
                }
                self.hypothesis = text;
                Some(self.partial_text())
            }
            Raw::Final(text) => {
                self.heard_sound = true;
                self.last_activity_ms = now_ms;
                self.hypothesis.clear();
                let text = text.trim();
                if text.is_empty() {
                    return None;
                }
                self.finals.push(text.to_string());
                Some(self.partial_text())
            }
            Raw::Confidence(c) => {
                self.min_confidence = Some(self.min_confidence.map_or(c, |m| m.min(c)));
                None
            }
            Raw::Rejected => {
                self.heard_sound = true;
                self.last_activity_ms = now_ms;
                self.hypothesis.clear();
                None
            }
            Raw::StreamEnd => {
                self.stream_ended = true;
                None
            }
        }
    }

    /// Text to show while listening: finished phrases plus the one in progress.
    pub(crate) fn partial_text(&self) -> String {
        let mut parts: Vec<&str> = self.finals.iter().map(String::as_str).collect();
        if !self.hypothesis.is_empty() {
            parts.push(&self.hypothesis);
        }
        parts.join(" ")
    }

    pub(crate) fn request_stop(&mut self, now_ms: u64) {
        if self.stop_at_ms.is_none() {
            self.stop_at_ms = Some(now_ms);
        }
    }

    pub(crate) fn has_text(&self) -> bool {
        !self.finals.is_empty()
    }

    /// Should the session end now, and why?
    pub(crate) fn poll(&self, now_ms: u64) -> Option<Finish> {
        if self.stream_ended {
            return Some(Finish::StreamEnd);
        }
        if let Some(stop_at) = self.stop_at_ms {
            let phrase_in_flight = self.in_sound || !self.hypothesis.is_empty();
            if !phrase_in_flight || now_ms.saturating_sub(stop_at) >= self.policy.stop_grace_ms {
                return Some(Finish::Stopped);
            }
            return None;
        }
        if now_ms >= self.policy.max_ms {
            return Some(Finish::MaxDuration);
        }
        if !self.heard_sound {
            return (now_ms >= self.policy.no_speech_ms).then_some(Finish::NoSpeech);
        }
        let quiet_for = now_ms.saturating_sub(self.last_activity_ms);
        // If the engine never reports the sound ending (steady noise), do not wait forever.
        let required = if self.in_sound {
            self.policy.silence_ms.saturating_add(3_000)
        } else {
            self.policy.silence_ms
        };
        if quiet_for >= required {
            return Some(if self.has_text() {
                Finish::Silence
            } else {
                Finish::NoSpeech
            });
        }
        None
    }

    /// The finished phrases, in order (the recogniser's pauses). After a manual stop an
    /// unfinished phrase is better than nothing.
    pub(crate) fn final_parts(&self) -> Vec<String> {
        if self.finals.is_empty() && self.stop_at_ms.is_some() && !self.hypothesis.trim().is_empty()
        {
            return vec![self.hypothesis.trim().to_string()];
        }
        self.finals.clone()
    }

    /// The text to deliver. After a manual stop an unfinished phrase is better than nothing.
    pub(crate) fn final_text(&self) -> String {
        let mut text = self.finals.join(" ");
        if text.is_empty() && self.stop_at_ms.is_some() {
            text = self.hypothesis.clone();
        }
        text.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note() -> Collector {
        Collector::new(Policy::for_mode(Mode::Note))
    }

    #[test]
    fn the_lowest_phrase_confidence_is_kept() {
        let mut c = Collector::new(Policy::for_mode(Mode::Command));
        assert_eq!(c.min_confidence(), None);
        c.on_raw(Raw::Confidence(93), 100);
        c.on_raw(Raw::Final("open notepad".into()), 100);
        c.on_raw(Raw::Confidence(61), 200);
        c.on_raw(Raw::Final("launch it".into()), 200);
        assert_eq!(c.min_confidence(), Some(0.61));
    }

    #[test]
    fn no_speech_times_out() {
        let c = note();
        assert_eq!(c.poll(7_999), None);
        assert_eq!(c.poll(8_000), Some(Finish::NoSpeech));
        assert_eq!(c.final_text(), "");
    }

    #[test]
    fn silence_after_a_phrase_ends_the_session() {
        let mut c = note();
        c.on_raw(Raw::SoundStart, 1_000);
        assert_eq!(
            c.on_raw(Raw::Hypothesis("remember to".into()), 1_500)
                .as_deref(),
            Some("remember to")
        );
        assert_eq!(
            c.on_raw(Raw::Hypothesis("remember to call".into()), 1_900)
                .as_deref(),
            Some("remember to call")
        );
        assert_eq!(
            c.on_raw(Raw::Hypothesis("remember to call".into()), 2_000),
            None,
            "unchanged partial is not re-sent"
        );
        c.on_raw(Raw::SoundEnd, 2_400);
        assert_eq!(
            c.on_raw(Raw::Final("Remember to call the dentist".into()), 2_600)
                .as_deref(),
            Some("Remember to call the dentist")
        );
        assert_eq!(c.poll(4_599), None);
        assert_eq!(c.poll(4_600), Some(Finish::Silence));
        assert_eq!(c.final_text(), "Remember to call the dentist");
    }

    #[test]
    fn more_speech_resets_the_silence_clock_and_phrases_join() {
        let mut c = note();
        c.on_raw(Raw::SoundStart, 500);
        c.on_raw(Raw::SoundEnd, 1_500);
        c.on_raw(Raw::Final("First sentence.".into()), 1_600);
        c.on_raw(Raw::SoundStart, 3_000);
        assert_eq!(c.poll(3_100), None);
        c.on_raw(Raw::SoundEnd, 4_000);
        c.on_raw(Raw::Final("Second sentence.".into()), 4_100);
        assert_eq!(c.poll(5_000), None);
        assert_eq!(c.poll(6_100), Some(Finish::Silence));
        assert_eq!(c.final_text(), "First sentence. Second sentence.");
    }

    #[test]
    fn sound_that_never_ends_still_finishes() {
        let mut c = note();
        c.on_raw(Raw::SoundStart, 100);
        c.on_raw(Raw::Final("hello".into()), 900);
        // SoundEnd never arrives (steady background noise).
        assert_eq!(c.poll(4_000), None);
        assert_eq!(c.poll(5_900), Some(Finish::Silence));
    }

    #[test]
    fn speech_without_recognition_reports_no_speech() {
        let mut c = Collector::new(Policy::for_mode(Mode::Command));
        c.on_raw(Raw::SoundStart, 100);
        c.on_raw(Raw::SoundEnd, 400);
        c.on_raw(Raw::Rejected, 500);
        assert_eq!(c.poll(1_300), None);
        assert_eq!(c.poll(1_400), Some(Finish::NoSpeech));
        assert_eq!(c.final_text(), "");
    }

    #[test]
    fn command_mode_finishes_quickly() {
        let mut c = Collector::new(Policy::for_mode(Mode::Command));
        c.on_raw(Raw::SoundStart, 200);
        c.on_raw(Raw::SoundEnd, 900);
        c.on_raw(Raw::Final("open timers".into()), 1_000);
        assert_eq!(c.poll(1_899), None);
        assert_eq!(c.poll(1_900), Some(Finish::Silence));
    }

    #[test]
    fn stop_with_nothing_pending_finishes_immediately() {
        let mut c = note();
        c.on_raw(Raw::SoundStart, 100);
        c.on_raw(Raw::SoundEnd, 800);
        c.on_raw(Raw::Final("done talking".into()), 900);
        c.request_stop(1_000);
        assert_eq!(c.poll(1_000), Some(Finish::Stopped));
        assert_eq!(c.final_text(), "done talking");
    }

    #[test]
    fn stop_mid_phrase_waits_for_the_final_then_falls_back_to_the_hypothesis() {
        let mut c = note();
        c.on_raw(Raw::SoundStart, 100);
        c.on_raw(Raw::Hypothesis("buy milk and".into()), 600);
        c.request_stop(700);
        assert_eq!(c.poll(800), None, "phrase in flight: wait for the engine");
        // The engine does not finish within the grace period: use what we have.
        assert_eq!(c.poll(700 + 1_500), Some(Finish::Stopped));
        assert_eq!(c.final_text(), "buy milk and");
    }

    #[test]
    fn stop_mid_phrase_prefers_the_real_final() {
        let mut c = note();
        c.on_raw(Raw::SoundStart, 100);
        c.on_raw(Raw::Hypothesis("buy milk and".into()), 600);
        c.request_stop(700);
        c.on_raw(Raw::SoundEnd, 750);
        c.on_raw(Raw::Final("Buy milk and eggs".into()), 800);
        assert_eq!(c.poll(800), Some(Finish::Stopped));
        assert_eq!(c.final_text(), "Buy milk and eggs");
    }

    #[test]
    fn max_duration_caps_the_session() {
        let mut c = Collector::new(Policy::for_mode(Mode::Command));
        c.on_raw(Raw::SoundStart, 0);
        c.on_raw(Raw::Hypothesis("blah".into()), 19_000);
        assert_eq!(c.poll(20_000), Some(Finish::MaxDuration));
    }

    #[test]
    fn stream_end_finishes_file_input() {
        let mut c = Collector::new(Policy::for_file());
        c.on_raw(Raw::Final("open timers".into()), 50);
        assert_eq!(c.poll(60), None);
        c.on_raw(Raw::StreamEnd, 70);
        assert_eq!(c.poll(70), Some(Finish::StreamEnd));
        assert_eq!(c.final_text(), "open timers");
    }

    #[test]
    fn empty_and_whitespace_events_are_ignored() {
        let mut c = note();
        assert_eq!(c.on_raw(Raw::Hypothesis("   ".into()), 10), None);
        assert_eq!(c.on_raw(Raw::Final("  ".into()), 20), None);
        assert!(!c.has_text());
    }
}
