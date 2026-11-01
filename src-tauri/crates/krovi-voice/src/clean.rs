//! Clean transcription for dictated notes: local, rule-based, deterministic.
//!
//! What it does (in this order): turns spoken punctuation ("comma", "new line") into marks,
//! removes hesitations (um, uh, er, ...), "you know" and "like" only where they are clearly
//! filler, collapses stutters ("the the") and restarted phrases ("I want to I want to go"),
//! fixes times ("330" -> "3:30", "p m" -> "PM"), capitalises sentences and the pronoun "I",
//! and ends a sentence of four or more words with a full stop (or a question mark when it opens
//! with a question word). Short fragments ("buy milk") are left without a full stop.
//!
//! It never invents words and never reorders anything; when a rule is not clearly safe it does
//! nothing. It is a tidy-up, not a language model.

use crate::civil::{MONTHS, WEEKDAYS};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tk {
    W(String),
    P(char),
    Line,
    Para,
}

fn lower(s: &str) -> String {
    s.to_lowercase()
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn has_digit(s: &str) -> bool {
    s.bytes().any(|b| b.is_ascii_digit())
}

// ---------------------------------------------------------------------------------------------
// lexing
// ---------------------------------------------------------------------------------------------

fn lex(text: &str) -> Vec<Tk> {
    let mut out = Vec::new();
    let mut pending_breaks = 0usize;
    for line in text.split('\n') {
        if line.trim().is_empty() {
            pending_breaks += 1;
            continue;
        }
        // newlines already in the text survive: one is a line break, a blank line a paragraph
        if !out.is_empty() {
            out.push(if pending_breaks >= 1 {
                Tk::Para
            } else {
                Tk::Line
            });
        }
        pending_breaks = 0;
        lex_line(line, &mut out);
    }
    out
}

fn lex_line(text: &str, out: &mut Vec<Tk>) {
    for raw in text.split_whitespace() {
        let mut core = raw;
        let mut trailing: Vec<char> = Vec::new();
        while let Some(c) = core.chars().last() {
            if matches!(c, '.' | ',' | ';' | ':' | '!' | '?') {
                trailing.push(c);
                core = &core[..core.len() - c.len_utf8()];
            } else {
                break;
            }
        }
        trailing.reverse();
        // "p.m." / "a.m." keep their dots inside the word, so the stop is not a sentence end.
        let l = lower(core);
        if (l == "p.m" || l == "a.m") && trailing.first() == Some(&'.') {
            out.push(Tk::W(if l == "p.m" { "PM" } else { "AM" }.to_string()));
            trailing.remove(0);
        } else if !core.is_empty() {
            out.push(Tk::W(core.to_string()));
        }
        for c in trailing {
            // collapse "..." / "?!" runs into a single mark
            if matches!(out.last(), Some(Tk::P(p)) if *p == c) {
                continue;
            }
            out.push(Tk::P(c));
        }
    }
}

fn render(tokens: &[Tk]) -> String {
    let mut s = String::new();
    let mut need_space = false;
    for t in tokens {
        match t {
            Tk::W(w) => {
                if need_space {
                    s.push(' ');
                }
                s.push_str(w);
                need_space = true;
            }
            Tk::P(c) => {
                s.push(*c);
                need_space = true;
            }
            Tk::Line => {
                s.push('\n');
                need_space = false;
            }
            Tk::Para => {
                s.push_str("\n\n");
                need_space = false;
            }
        }
    }
    s
}

// ---------------------------------------------------------------------------------------------
// pass 1: spoken punctuation
// ---------------------------------------------------------------------------------------------

/// Words that, right before "period"/"comma"/"colon", mean the ordinary noun.
const NOUN_BEFORE: &[&str] = &[
    "the", "a", "an", "this", "that", "same", "grace", "time", "trial", "billing", "each", "per",
    "whole", "entire", "last", "first", "next", "previous", "full", "oxford", "serial", "my",
    "his", "her", "your", "our", "their", "large", "small",
];

fn word_at(tokens: &[Tk], i: usize) -> Option<String> {
    match tokens.get(i) {
        Some(Tk::W(w)) => Some(lower(w)),
        _ => None,
    }
}

fn spoken_punctuation(tokens: Vec<Tk>) -> Vec<Tk> {
    let mut out: Vec<Tk> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let w = word_at(&tokens, i);
        let w2 = word_at(&tokens, i + 1);
        let prev = out.last().and_then(|t| {
            if let Tk::W(p) = t {
                Some(lower(p))
            } else {
                None
            }
        });
        let noun_context = prev.as_deref().is_some_and(|p| NOUN_BEFORE.contains(&p))
            || matches!(
                w2.as_deref(),
                Some("of") | Some("between") | Some("is") | Some("was")
            );
        match (w.as_deref(), w2.as_deref()) {
            (Some("new"), Some("paragraph")) => {
                out.push(Tk::Para);
                i += 2;
            }
            (Some("new"), Some("line")) | (Some("newline"), _) => {
                out.push(Tk::Line);
                i += if w.as_deref() == Some("newline") {
                    1
                } else {
                    2
                };
            }
            (Some("full"), Some("stop")) => {
                out.push(Tk::P('.'));
                i += 2;
            }
            (Some("question"), Some("mark")) => {
                out.push(Tk::P('?'));
                i += 2;
            }
            (Some("exclamation"), Some("mark")) | (Some("exclamation"), Some("point")) => {
                out.push(Tk::P('!'));
                i += 2;
            }
            (Some("period"), _) if !noun_context && !out.is_empty() => {
                out.push(Tk::P('.'));
                i += 1;
            }
            (Some("comma"), _) if !noun_context && !out.is_empty() => {
                out.push(Tk::P(','));
                i += 1;
            }
            (Some("semicolon"), _) if !out.is_empty() => {
                out.push(Tk::P(';'));
                i += 1;
            }
            (Some("colon"), _)
                if !noun_context && !out.is_empty() && tokens.get(i + 1).is_some() =>
            {
                out.push(Tk::P(':'));
                i += 1;
            }
            _ => {
                out.push(tokens[i].clone());
                i += 1;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// pass 2: fillers
// ---------------------------------------------------------------------------------------------

fn is_hesitation(w: &str) -> bool {
    matches!(
        lower(w).as_str(),
        "um" | "uh"
            | "er"
            | "erm"
            | "ah"
            | "hmm"
            | "mm"
            | "mmm"
            | "uhm"
            | "umm"
            | "uhh"
            | "uhhh"
            | "eh"
            | "ehm"
            | "hm"
            | "ahh"
    )
}

/// "you know" after one of these is a real clause ("as you know", "do you know").
const YOU_KNOW_BLOCK: &[&str] = &[
    "as", "if", "what", "how", "do", "did", "does", "that", "than", "when", "before", "unless",
    "whether", "i", "we", "they", "if", "so", "and", "but", "why", "where", "who", "can", "could",
    "would", "will", "should",
];

fn sentence_edge(t: Option<&Tk>) -> bool {
    matches!(
        t,
        None | Some(Tk::P('.'))
            | Some(Tk::P('!'))
            | Some(Tk::P('?'))
            | Some(Tk::P(';'))
            | Some(Tk::P(':'))
            | Some(Tk::Line)
            | Some(Tk::Para)
    )
}

fn is_comma(t: Option<&Tk>) -> bool {
    matches!(t, Some(Tk::P(',')))
}

fn drop_fillers(tokens: Vec<Tk>) -> Vec<Tk> {
    let mut out: Vec<Tk> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let w = word_at(&tokens, i);
        match w.as_deref() {
            Some(h) if is_hesitation(h) => {
                i += 1;
                continue;
            }
            Some("you") if word_at(&tokens, i + 1).as_deref() == Some("know") => {
                let prev = out.last();
                let next = tokens.get(i + 2);
                let blocked =
                    matches!(prev, Some(Tk::W(p)) if YOU_KNOW_BLOCK.contains(&lower(p).as_str()));
                if sentence_edge(prev) && is_comma(next) {
                    // "You know, I think ..."
                    i += 3;
                    continue;
                }
                if is_comma(prev) && is_comma(next) {
                    // "it was, you know, great"
                    out.pop();
                    i += 3;
                    continue;
                }
                if is_comma(prev) && sentence_edge(next) {
                    // "it was great, you know."
                    out.pop();
                    i += 2;
                    continue;
                }
                if !blocked && matches!(prev, Some(Tk::W(_))) && sentence_edge(next) {
                    // "it was great you know."
                    i += 2;
                    continue;
                }
            }
            Some("like") => {
                let prev = out.last();
                let next = tokens.get(i + 1);
                if is_comma(prev) && is_comma(next) {
                    // "to the store, like, right now"
                    out.pop();
                    i += 2;
                    continue;
                }
                if sentence_edge(prev) && is_comma(next) {
                    // "Like, I don't know"
                    i += 2;
                    continue;
                }
            }
            _ => {}
        }
        out.push(tokens[i].clone());
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------------------------
// pass 3: stutters and restarted phrases
// ---------------------------------------------------------------------------------------------

/// Words that are genuinely allowed to repeat in ordinary speech/writing.
const LEGIT_REPEAT: &[&str] = &[
    "had", "that", "very", "so", "no", "ha", "bye", "yeah", "really", "more", "many", "knock",
    "tick", "bora", "buffalo", "blah", "yes", "go", "now", "far",
];

fn same_word(a: &Tk, b: &Tk) -> bool {
    matches!((a, b), (Tk::W(x), Tk::W(y)) if lower(x) == lower(y))
}

/// Find `n` words, an optional comma, then the same `n` words again. Returns (start, gap).
fn find_restart(tokens: &[Tk], n: usize) -> Option<(usize, usize)> {
    'start: for i in 0..tokens.len() {
        if i + 2 * n > tokens.len() {
            return None;
        }
        if !tokens[i..i + n].iter().all(|t| matches!(t, Tk::W(_))) {
            continue;
        }
        for gap in [0usize, 1] {
            let second = i + n + gap;
            if second + n > tokens.len() {
                continue;
            }
            if gap == 1 && !is_comma(tokens.get(i + n)) {
                continue;
            }
            let all_same = (0..n).all(|k| same_word(&tokens[i + k], &tokens[second + k]));
            if !all_same {
                continue;
            }
            // single words: skip legitimate repeats and numbers
            if n == 1 {
                if let Tk::W(w) = &tokens[i] {
                    if LEGIT_REPEAT.contains(&lower(w).as_str()) || has_digit(w) {
                        continue 'start;
                    }
                }
            }
            return Some((i, n + gap));
        }
    }
    None
}

fn dedupe(mut tokens: Vec<Tk>) -> Vec<Tk> {
    for _ in 0..20 {
        let mut changed = false;
        for n in [5usize, 4, 3, 2, 1] {
            while let Some((start, len)) = find_restart(&tokens, n) {
                tokens.drain(start..start + len);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // a comma left dangling by a removal: ", ," / "., ,"
    let mut out: Vec<Tk> = Vec::with_capacity(tokens.len());
    for t in tokens {
        if matches!((&t, out.last()), (Tk::P(','), Some(Tk::P(',')))) {
            continue;
        }
        out.push(t);
    }
    out
}

// ---------------------------------------------------------------------------------------------
// pass 4: times and am/pm
// ---------------------------------------------------------------------------------------------

fn is_meridiem(t: Option<&Tk>) -> bool {
    matches!(t, Some(Tk::W(w)) if matches!(w.as_str(), "AM" | "PM"))
}

/// A cue that a following 3-4 digit number is a clock time.
fn time_cue(prev: Option<&str>, prev2: Option<&str>) -> bool {
    match prev {
        Some("at") | Some("by") | Some("around") | Some("until") | Some("till") | Some("from")
        | Some("before") | Some("after") => true,
        Some("to") | Some("for") => matches!(
            prev2,
            Some("moved")
                | Some("pushed")
                | Some("rescheduled")
                | Some("shifted")
                | Some("scheduled")
                | Some("changed")
                | Some("delayed")
                | Some("brought")
        ),
        _ => false,
    }
}

fn fix_times(tokens: Vec<Tk>) -> Vec<Tk> {
    let mut out: Vec<Tk> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        // "p m" / "a m" / "pm" after a number -> PM / AM
        if let Tk::W(w) = &tokens[i] {
            let l = lower(w);
            let after_number = matches!(out.last(), Some(Tk::W(p)) if has_digit(p));
            if after_number && (l == "pm" || l == "am") {
                out.push(Tk::W(l.to_uppercase()));
                i += 1;
                continue;
            }
            if after_number
                && (l == "p" || l == "a")
                && word_at(&tokens, i + 1).as_deref() == Some("m")
            {
                out.push(Tk::W(if l == "p" { "PM" } else { "AM" }.to_string()));
                i += 2;
                continue;
            }
        }
        out.push(tokens[i].clone());
        i += 1;
    }

    // now "330" -> "3:30" and "3 30 PM" -> "3:30 PM"
    let mut res: Vec<Tk> = Vec::with_capacity(out.len());
    let mut i = 0;
    while i < out.len() {
        if let Tk::W(w) = &out[i] {
            let prev = match res.last() {
                Some(Tk::W(p)) => Some(lower(p)),
                _ => None,
            };
            let prev2 = match res.len().checked_sub(2).and_then(|k| res.get(k)) {
                Some(Tk::W(p)) => Some(lower(p)),
                _ => None,
            };
            let next_is_mer = is_meridiem(out.get(i + 1));
            let cue = time_cue(prev.as_deref(), prev2.as_deref());
            if is_digits(w) && (w.len() == 3 || w.len() == 4) && (cue || next_is_mer) {
                let (h, m) = w.split_at(w.len() - 2);
                let (hv, mv): (u32, u32) = (h.parse().unwrap_or(99), m.parse().unwrap_or(99));
                let plausible = mv < 60
                    && ((w.len() == 3 && (1..=9).contains(&hv))
                        || (w.len() == 4 && (1..=23).contains(&hv)));
                // 1900-2100 right after "in" is a year, not 19:00-21:00
                let year_like = w.len() == 4
                    && (1900..=2100).contains(&w.parse::<u32>().unwrap_or(0))
                    && !next_is_mer
                    && prev.as_deref() != Some("at");
                if plausible && !year_like {
                    res.push(Tk::W(format!("{hv}:{mv:02}")));
                    i += 1;
                    continue;
                }
            }
            // "3 30 PM"
            if is_digits(w) && w.len() <= 2 {
                if let (Some(Tk::W(m)), true) = (out.get(i + 1), is_meridiem(out.get(i + 2))) {
                    if is_digits(m)
                        && m.len() == 2
                        && m.parse::<u32>().is_ok_and(|v| v < 60)
                        && w.parse::<u32>().is_ok_and(|v| (1..=12).contains(&v))
                    {
                        res.push(Tk::W(format!("{}:{}", w.parse::<u32>().unwrap_or(0), m)));
                        i += 2;
                        continue;
                    }
                }
            }
        }
        res.push(out[i].clone());
        i += 1;
    }
    res
}

// ---------------------------------------------------------------------------------------------
// pass 5: capitalisation
// ---------------------------------------------------------------------------------------------

fn capitalise_word(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn fix_case(tokens: Vec<Tk>) -> Vec<Tk> {
    let mut cap_next = true;
    tokens
        .into_iter()
        .map(|t| match t {
            Tk::W(w) => {
                let l = lower(&w);
                let mut word = w;
                if l == "i" {
                    word = "I".into();
                } else if matches!(l.as_str(), "i'm" | "i'll" | "i've" | "i'd") {
                    word = capitalise_word(&word);
                } else if let Some(d) = WEEKDAYS.iter().find(|d| lower(d) == l) {
                    word = (*d).to_string();
                } else if let Some(m) = MONTHS.iter().find(|m| lower(m) == l) {
                    // "may" and "march" are ordinary words too: leave them alone
                    if !matches!(l.as_str(), "may" | "march") {
                        word = (*m).to_string();
                    }
                }
                if cap_next {
                    word = capitalise_word(&word);
                }
                cap_next = false;
                Tk::W(word)
            }
            Tk::P(c) => {
                if matches!(c, '.' | '!' | '?') {
                    cap_next = true;
                }
                Tk::P(c)
            }
            other => {
                cap_next = true;
                other
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// pass 6: sentence ending
// ---------------------------------------------------------------------------------------------

const QUESTION_WORDS: &[&str] = &[
    "who", "whom", "whose", "what", "when", "where", "why", "how", "which", "is", "are", "was",
    "were", "do", "does", "did", "can", "could", "will", "would", "should", "shall",
];

fn end_sentence(mut tokens: Vec<Tk>) -> Vec<Tk> {
    // the last sentence = tokens after the last sentence mark / line break
    let start = tokens
        .iter()
        .rposition(|t| matches!(t, Tk::P('.' | '!' | '?') | Tk::Line | Tk::Para))
        .map_or(0, |p| p + 1);
    let tail = &tokens[start..];
    if matches!(tail.last(), Some(Tk::P(_))) || tail.is_empty() {
        return tokens;
    }
    let words: Vec<&String> = tail
        .iter()
        .filter_map(|t| if let Tk::W(w) = t { Some(w) } else { None })
        .collect();
    if words.len() < 4 && !(words.len() >= 3 && QUESTION_WORDS.contains(&lower(words[0]).as_str()))
    {
        return tokens;
    }
    let question = QUESTION_WORDS.contains(&lower(words[0]).as_str());
    tokens.push(Tk::P(if question { '?' } else { '.' }));
    tokens
}

// ---------------------------------------------------------------------------------------------
// public API
// ---------------------------------------------------------------------------------------------

fn pipeline(text: &str, finish: bool) -> Vec<Tk> {
    let t = lex(text);
    let t = spoken_punctuation(t);
    let t = drop_fillers(t);
    let t = dedupe(t);
    let t = fix_times(t);
    let t = fix_case(t);
    if finish {
        end_sentence(t)
    } else {
        t
    }
}

/// Clean one finished phrase (adds the closing full stop / question mark where it is safe).
pub fn clean_text(text: &str) -> String {
    render(&pipeline(text, true))
}

/// Clean several finished phrases (the recogniser's pauses) and join them.
pub fn clean_phrases(parts: &[String]) -> String {
    let mut out = String::new();
    for part in parts {
        let cleaned = clean_text(part);
        if cleaned.is_empty() {
            continue;
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push(' ');
        }
        out.push_str(&cleaned);
    }
    out
}

/// Clean a short fragment (a note body or reminder title): same rules, but no closing full stop.
pub fn clean_fragment(text: &str) -> String {
    render(&pipeline(text, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(s: &str) -> String {
        clean_text(s)
    }

    #[test]
    fn hesitations_are_removed() {
        assert_eq!(
            c("um remember to uh call the dentist tomorrow morning"),
            "Remember to call the dentist tomorrow morning."
        );
        assert_eq!(
            c("so, um, I think we should go"),
            "So, I think we should go."
        );
        assert_eq!(c("er buy milk"), "Buy milk");
        assert_eq!(c("uhm"), "");
    }

    #[test]
    fn you_know_only_at_clause_edges() {
        assert_eq!(
            c("you know, I think we should leave now"),
            "I think we should leave now."
        );
        assert_eq!(c("it was, you know, really good"), "It was really good.");
        assert_eq!(c("we met at the park, you know."), "We met at the park.");
        assert_eq!(c("that was great you know"), "That was great");
        assert_eq!(c("it works, you know"), "It works");
        // real clauses survive
        assert_eq!(
            c("as you know, the meeting moved"),
            "As you know, the meeting moved."
        );
        assert_eq!(
            c("do you know what time it is"),
            "Do you know what time it is?"
        );
        assert_eq!(c("you know what I mean"), "You know what I mean.");
    }

    #[test]
    fn like_only_when_clearly_filler() {
        assert_eq!(
            c("i'm going to the store, like, right now"),
            "I'm going to the store right now."
        );
        assert_eq!(
            c("like, I don't know about that"),
            "I don't know about that."
        );
        assert_eq!(c("i like pizza"), "I like pizza");
        assert_eq!(c("it looks like a bridge"), "It looks like a bridge.");
        assert_eq!(c("people like you are rare"), "People like you are rare.");
    }

    #[test]
    fn stutters_and_restarts_collapse() {
        assert_eq!(
            c("the the meeting is is at three"),
            "The meeting is at three."
        );
        assert_eq!(c("i i think so"), "I think so");
        assert_eq!(c("i want to i want to go home"), "I want to go home.");
        assert_eq!(
            c("we should go to the, we should go to the market"),
            "We should go to the market."
        );
        assert_eq!(c("send the the the report"), "Send the report");
    }

    #[test]
    fn legitimate_repeats_survive() {
        assert_eq!(
            c("he said that that was fine"),
            "He said that that was fine."
        );
        assert_eq!(c("she had had enough"), "She had had enough.");
        assert_eq!(c("it is very very cold"), "It is very very cold.");
        assert_eq!(c("call 555 555 about it"), "Call 555 555 about it.");
    }

    #[test]
    fn spoken_punctuation_becomes_marks() {
        assert_eq!(
            c("buy milk comma eggs comma and bread period"),
            "Buy milk, eggs, and bread."
        );
        assert_eq!(c("are you coming question mark"), "Are you coming?");
        assert_eq!(
            c("great exclamation mark see you there full stop"),
            "Great! See you there."
        );
        assert_eq!(
            c("remember this semicolon call mom"),
            "Remember this; call mom."
        );
        assert_eq!(
            c("first item new line second item"),
            "First item\nSecond item"
        );
        assert_eq!(
            c("intro new paragraph the details follow"),
            "Intro\n\nThe details follow"
        );
    }

    #[test]
    fn punctuation_words_used_as_nouns_stay() {
        assert_eq!(
            c("the trial period ends friday"),
            "The trial period ends Friday."
        );
        assert_eq!(c("a period of rest"), "A period of rest.");
        assert_eq!(
            c("during the grace period we are fine"),
            "During the grace period we are fine."
        );
    }

    #[test]
    fn times_get_a_colon() {
        assert_eq!(
            c("the meeting with the design team moved to 330 please send them the mockups"),
            "The meeting with the design team moved to 3:30 please send them the mockups."
        );
        assert_eq!(c("lunch at 1230"), "Lunch at 12:30");
        assert_eq!(c("see you at 3 30 p m"), "See you at 3:30 PM.");
        assert_eq!(c("the call is at 4 pm"), "The call is at 4 PM.");
        assert_eq!(
            c("the call is at 4 p.m. sharp"),
            "The call is at 4 PM sharp."
        );
    }

    #[test]
    fn plain_numbers_are_left_alone() {
        assert_eq!(c("we shipped 330 units"), "We shipped 330 units.");
        assert_eq!(c("it costs 1200 dollars"), "It costs 1200 dollars.");
        assert_eq!(c("born in 1999"), "Born in 1999");
        assert_eq!(c("buy 12 apples and 3 pears"), "Buy 12 apples and 3 pears.");
    }

    #[test]
    fn capitalisation_and_pronoun_i() {
        assert_eq!(
            c("monday at nine i'll be there"),
            "Monday at nine I'll be there."
        );
        assert_eq!(
            c("it's due in november and i said so"),
            "It's due in November and I said so."
        );
        assert_eq!(
            c("what a lovely day. let's go outside"),
            "What a lovely day. Let's go outside"
        );
        // ordinary words that are also month names stay lower-case
        assert_eq!(c("you may go now please"), "You may go now please.");
    }

    #[test]
    fn closing_punctuation_rules() {
        assert_eq!(
            c("buy milk"),
            "Buy milk",
            "short fragments get no full stop"
        );
        assert_eq!(c("pick up the parcel today"), "Pick up the parcel today.");
        assert_eq!(c("what time is the meeting"), "What time is the meeting?");
        assert_eq!(c("is it ready"), "Is it ready?");
        assert_eq!(c("already done."), "Already done.");
        assert_eq!(c("thanks!"), "Thanks!");
    }

    #[test]
    fn phrases_are_cleaned_and_joined() {
        let parts = vec![
            "um the meeting moved to the afternoon".to_string(),
            "uh please tell the the team".to_string(),
        ];
        assert_eq!(
            clean_phrases(&parts),
            "The meeting moved to the afternoon. Please tell the team."
        );
        assert_eq!(clean_phrases(&["".to_string(), "  ".to_string()]), "");
    }

    #[test]
    fn fragments_get_no_closing_mark_but_are_tidied() {
        assert_eq!(
            clean_fragment("um pick up the the parcel today"),
            "Pick up the parcel today"
        );
        assert_eq!(clean_fragment("call mom"), "Call mom");
        assert_eq!(
            clean_fragment("what time is the meeting"),
            "What time is the meeting"
        );
    }

    #[test]
    fn already_clean_text_is_stable() {
        for s in [
            "Remember to call the dentist tomorrow morning.",
            "Buy milk, eggs, and bread.",
            "First item\nSecond item",
            "Meet at 3:30 PM.",
        ] {
            assert_eq!(clean_text(s), s, "second pass must not change: {s:?}");
        }
    }

    #[test]
    fn empty_and_whitespace() {
        assert_eq!(c(""), "");
        assert_eq!(c("   \n "), "");
        assert_eq!(c("um uh er"), "");
    }

    #[test]
    fn idempotent_on_messy_input() {
        for s in [
            "um so the the meeting is like, at 330 you know, i think",
            "buy milk comma eggs period new line call mom",
        ] {
            let once = c(s);
            assert_eq!(c(&once), once, "not idempotent: {s:?} -> {once:?}");
        }
    }
}
