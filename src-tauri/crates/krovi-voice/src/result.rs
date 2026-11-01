//! Turn the recognizer's finished phrases into the `VoiceResult` the UI receives. Pure: the
//! clock and the captured context come in as arguments.

use crate::civil::LocalNow;
use crate::clean::clean_phrases;
use crate::intent::{parse, ContextAction, Env, Parsed};
use crate::parser::tidy_text;
use crate::types::{Mode, StartOptions, VoiceCommand, VoiceContext, VoiceResult};

/// An installed app is only launched when the recognizer is at least this sure of the phrase
/// (0.0..=1.0). Measured with synthesized speech: phrases that really were "launch <app>" scored
/// 0.92-0.95 with a male voice (0.62-0.95 with a female one), while ordinary sentences that the
/// closed grammar snapped to an app name ("start the car" -> "start the Claude") scored 0.54-0.73.
/// A lower score means "say it again", never a surprise program starting. `None` (no score, e.g.
/// typed text) is trusted.
pub const APP_LAUNCH_MIN_CONFIDENCE: f32 = 0.75;

pub(crate) fn build_result(
    mode: Mode,
    parts: &[String],
    opts: &StartOptions,
    now: LocalNow,
    confidence: Option<f32>,
) -> VoiceResult {
    match mode {
        Mode::Note => {
            let text = if opts.clean_transcription {
                clean_phrases(parts)
            } else {
                tidy_text(&parts.join(" "))
            };
            VoiceResult::new(Mode::Note, text, None)
        }
        Mode::Command => {
            let text = parts
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let mut result = VoiceResult::new(Mode::Command, text.clone(), None);
            result.context = opts.context.clone();
            if text.is_empty() {
                return result;
            }
            let env = Env::at(now, opts.context.clone())
                .with_clean(opts.clean_transcription)
                .with_catalog(opts.catalog.clone());
            match parse(&text, &env) {
                Parsed::Command(VoiceCommand::OpenApp { name, .. })
                    if confidence.is_some_and(|c| c < APP_LAUNCH_MIN_CONFIDENCE) =>
                {
                    result.feedback = Some(format!(
                        "I am not sure I heard that right, so I did not launch {name}. Say it again."
                    ));
                }
                Parsed::Command(c) => result.command = Some(c),
                Parsed::NeedsContext(action) => {
                    result.feedback = Some(needs_context_message(action, opts))
                }
                Parsed::Unresolved { feedback, results } => {
                    result.feedback = Some(feedback);
                    result.results = results;
                }
                Parsed::Unknown => {}
            }
            result
        }
    }
}

fn needs_context_message(action: ContextAction, opts: &StartOptions) -> String {
    if let Some(hint) = &opts.context_hint {
        return hint.clone();
    }
    match &opts.context {
        Some(VoiceContext::App { value }) => format!(
            "Nothing is selected in {value} to {}. Select it first, then try again.",
            action.describe()
        ),
        _ => format!(
            "Nothing is selected to {}. Select it first, then try again.",
            action.describe()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::build_result;
    use super::*;

    fn build(mode: Mode, parts: &[String], opts: &StartOptions, now: LocalNow) -> VoiceResult {
        build_result(mode, parts, opts, now, None)
    }
    use crate::civil::Date;
    use crate::types::VoiceCommand;

    fn now() -> LocalNow {
        LocalNow::new(Date::new(2026, 11, 1).unwrap(), 10, 30)
    }

    fn parts(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn note_mode_cleans_by_default_and_not_when_turned_off() {
        let p = parts(&["um remember to uh call the the dentist tomorrow morning"]);
        let on = build(Mode::Note, &p, &StartOptions::default(), now());
        assert_eq!(on.text, "Remember to call the dentist tomorrow morning.");
        assert_eq!(
            (on.command, on.context.is_none(), on.executed),
            (None, true, false)
        );
        let off = build(
            Mode::Note,
            &p,
            &StartOptions {
                clean_transcription: false,
                ..Default::default()
            },
            now(),
        );
        assert_eq!(
            off.text,
            "Um remember to uh call the the dentist tomorrow morning"
        );
    }

    #[test]
    fn command_mode_parses_and_records_the_context() {
        let ctx = VoiceContext::Text {
            value: "Buy oat milk".into(),
            app: Some("notepad".into()),
        };
        let opts = StartOptions {
            context: Some(ctx.clone()),
            ..Default::default()
        };
        let r = build(
            Mode::Command,
            &parts(&["save this as a note"]),
            &opts,
            now(),
        );
        assert_eq!(
            r.command,
            Some(VoiceCommand::NewNote {
                body: "Buy oat milk".into()
            })
        );
        assert_eq!(r.context, Some(ctx));
        assert_eq!(r.text, "save this as a note");
    }

    #[test]
    fn missing_selection_gives_feedback_not_a_command() {
        let r = build(
            Mode::Command,
            &parts(&["send this to my phone"]),
            &StartOptions::default(),
            now(),
        );
        assert_eq!(r.command, None);
        assert_eq!(
            r.feedback.as_deref(),
            Some("Nothing is selected to send to your phone. Select it first, then try again.")
        );
        let app = StartOptions {
            context: Some(VoiceContext::App {
                value: "chrome".into(),
            }),
            ..Default::default()
        };
        let r = build(
            Mode::Command,
            &parts(&["put this on the shelf"]),
            &app,
            now(),
        );
        assert!(r
            .feedback
            .unwrap()
            .contains("Nothing is selected in chrome"));
        let hinted = StartOptions {
            context_hint: Some("Context capture is off in Voice settings.".into()),
            ..Default::default()
        };
        let r = build(
            Mode::Command,
            &parts(&["save this as a note"]),
            &hinted,
            now(),
        );
        assert_eq!(
            r.feedback.as_deref(),
            Some("Context capture is off in Voice settings.")
        );
    }

    fn catalog_opts() -> StartOptions {
        use crate::catalog::{Catalog, Entry};
        StartOptions {
            catalog: Some(std::sync::Arc::new(Catalog::new(
                vec![Entry::new("Downloads", r"C:\u\Downloads")],
                vec![Entry::new("Notepad", r"shell:AppsFolder\notepad")],
            ))),
            ..Default::default()
        }
    }

    #[test]
    fn a_low_confidence_app_launch_is_not_carried_out_but_other_commands_are() {
        let opts = catalog_opts();
        let say = |text: &str, conf: Option<f32>| {
            build_result(Mode::Command, &parts(&[text]), &opts, now(), conf)
        };
        // sure enough: the command is produced
        let ok = say("open notepad", Some(0.94));
        assert!(matches!(ok.command, Some(VoiceCommand::OpenApp { .. })));
        // typed text has no score and is trusted
        assert!(say("open notepad", None).command.is_some());
        // unsure: nothing is launched, and the user is told why
        let unsure = say("open notepad", Some(0.62));
        assert_eq!(unsure.command, None);
        assert_eq!(
            unsure.feedback.as_deref(),
            Some("I am not sure I heard that right, so I did not launch Notepad. Say it again.")
        );
        // the gate is only for launching apps
        assert!(say("open downloads", Some(0.62)).command.is_some());
        assert!(say("find invoice", Some(0.62)).command.is_some());
        assert!(say("close", Some(0.62)).command.is_some());
    }

    #[test]
    fn unresolved_names_come_back_as_feedback_with_candidates() {
        let opts = catalog_opts();
        let r = build_result(
            Mode::Command,
            &parts(&["open photoshop"]),
            &opts,
            now(),
            Some(0.9),
        );
        assert_eq!(r.command, None);
        assert!(r.feedback.unwrap().contains("photoshop"));
        assert!(r.results.is_empty());
    }

    #[test]
    fn empty_command_text_yields_an_empty_result() {
        let r = build(Mode::Command, &[], &StartOptions::default(), now());
        assert_eq!((r.text.as_str(), r.command, r.feedback), ("", None, None));
    }

    #[test]
    fn spoken_reminders_and_notes_are_tidied_when_cleaning_is_on() {
        let r = build(
            Mode::Command,
            &parts(&["add a reminder for tomorrow at 9 to uh call mom"]),
            &StartOptions::default(),
            now(),
        );
        assert_eq!(
            r.command,
            Some(VoiceCommand::AddReminder {
                date: "2026-11-02".into(),
                time: Some("09:00".into()),
                title: "Call mom".into()
            })
        );
        let raw = build(
            Mode::Command,
            &parts(&["add a reminder for tomorrow at 9 to uh call mom"]),
            &StartOptions {
                clean_transcription: false,
                ..Default::default()
            },
            now(),
        );
        assert_eq!(
            raw.command,
            Some(VoiceCommand::AddReminder {
                date: "2026-11-02".into(),
                time: Some("09:00".into()),
                title: "Uh call mom".into()
            })
        );
    }
}
