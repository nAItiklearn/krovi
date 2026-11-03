//! Dictation post-processing: raw local speech transcript (Whisper) -> the text Krovi types into
//! the user's active app. Pure, deterministic, no network, no model, no OS calls.
//!
//! It is a thin layer over [`crate::clean`] (spoken punctuation, filler removal, stutters, times,
//! capitalisation, closing marks), which it reuses and never duplicates. What it adds, in order:
//!
//! 1. **Whisper quirks**: bracketed non-speech tags (`[BLANK_AUDIO]`, `(music)`, `♪`) are dropped,
//!    whitespace is collapsed, and a transcript that is only a lone "you" / "Thank you." (a classic
//!    silence hallucination) becomes empty.
//! 2. **Script guard**: if the text contains any non-Latin letters (Devanagari, CJK, Arabic, ...)
//!    nothing else is done to it - no English capitalisation, no filler rules - apart from the
//!    user's own replacements. Romanised Hinglish is Latin text and gets the normal handling.
//! 3. **Spoken self-correction** (Latin prose styles): "scratch that" / "delete that" remove the
//!    preceding sentence; `<A>, no wait, <B>` / `<A>, sorry I mean <B>` / `<A>, I mean <B>` replace
//!    the last words of a short clause A with B. Only these clear patterns are touched; a sentence
//!    such as "I mean it" is left alone. A repaired sentence of three words or more is closed with
//!    a full stop (clean.rs alone would only close four).
//! 4. **Style**: [`DictationStyle`] picks the layout (see its variants).
//! 5. **Replacements**: the user's vocabulary ("crovy" -> "Krovi") is applied last, on the final
//!    text, case-insensitively on whole words/phrases, longest phrase first, and the `written` text
//!    is inserted verbatim (never re-capitalised, never re-matched). They also apply to non-Latin
//!    text, since they are explicit user rules rather than English heuristics.
//!
//! Option semantics: `remove_fillers = false` keeps "um", "uh", "like" and "you know" exactly as
//! spoken (stutter collapse still applies). `auto_punctuate = false` skips the automatic closing
//! full stop / question mark; spoken punctuation ("comma", "period") is always honoured. The Code
//! style ignores both for symbols, drops only plain hesitations (um/uh/erm...) and never touches
//! identifiers.
//!
//! Limits: the self-correction rules deliberately miss most unpunctuated, ambiguous speech - when
//! unsure, the text is left exactly as clean.rs produced it. Contraction rules only cover a short
//! list of unambiguous forms.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::clean::{clean_fragment, clean_text, is_hesitation};

/// Maximum number of custom replacements.
pub const MAX_REPLACEMENTS: usize = 200;
/// Maximum length (characters) of a `spoken` phrase.
pub const MAX_SPOKEN_CHARS: usize = 60;
/// Maximum length (characters) of a `written` text.
pub const MAX_WRITTEN_CHARS: usize = 200;
/// Upper bound (characters) of [`initial_prompt`].
pub const PROMPT_MAX_CHARS: usize = 200;

const PROMPT_TERM_MAX_CHARS: usize = 40;
const MAX_CLAUSE_WORDS: usize = 6;
const MAX_CORRECTIONS: usize = 64;
/// Private-use marker appended to fillers that must survive clean.rs (`remove_fillers = false`).
const FILLER_MARK: char = '\u{E000}';

// ---------------------------------------------------------------------------------------------
// public types
// ---------------------------------------------------------------------------------------------

/// How the finished text is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DictationStyle {
    /// clean.rs behaviour: tidy sentences with a closing full stop.
    #[default]
    Plain,
    /// Like Plain, but a single short sentence (8 words or fewer) gets no trailing full stop.
    Message,
    /// Paragraphs on "new paragraph"; a greeting ("Hi Sam,") and a sign-off ("Thanks,\nJohn") on
    /// their own lines.
    Email,
    /// "new item" / "next item" / "bullet" start `- ` list items.
    Note,
    /// Spoken symbols ("open paren", "dot", "camel case ..."), no capitalisation, no full stop.
    Code,
    /// Plain with unambiguous contractions expanded ("don't" -> "do not").
    Professional,
    /// Plain with a few common phrases contracted ("do not" -> "don't").
    Casual,
}

/// One user vocabulary entry / pronunciation fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replacement {
    pub spoken: String,
    pub written: String,
}

/// Everything [`process`] needs besides the transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DictationOptions {
    pub style: DictationStyle,
    pub replacements: Vec<Replacement>,
    pub remove_fillers: bool,
    pub auto_punctuate: bool,
}

impl Default for DictationOptions {
    fn default() -> Self {
        Self {
            style: DictationStyle::Plain,
            replacements: Vec::new(),
            remove_fillers: true,
            auto_punctuate: true,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// public functions
// ---------------------------------------------------------------------------------------------

/// Turn a raw transcript into the text to type. Never panics; an empty or noise-only transcript
/// gives an empty string.
pub fn process(raw: &str, opts: &DictationOptions) -> String {
    let text = normalize(raw);
    if text.is_empty() {
        return String::new();
    }
    // Email applies the replacements itself, per paragraph, before it looks for the greeting and
    // sign-off, so that a replacement like "sam" -> "Sam" makes "hi sam" a recognisable greeting.
    let (out, replaced) = if has_non_latin(&text) {
        (text, false)
    } else if opts.style == DictationStyle::Code {
        (code(&text, opts), false)
    } else {
        (prose(&text, opts), opts.style == DictationStyle::Email)
    };
    let out = if replaced {
        out
    } else {
        apply_replacements(&out, &opts.replacements)
    };
    if opts.style == DictationStyle::Code {
        out.trim_matches(' ').to_string()
    } else {
        out.trim().to_string()
    }
}

/// A Whisper `initial_prompt` that biases recognition towards the user's own terms: the
/// vocabulary entries first, then the `written` side of the replacements, comma-separated,
/// de-duplicated (case-insensitive), each term at most 40 characters and the whole prompt at most
/// [`PROMPT_MAX_CHARS`]. It is a bare term list - no sentence is invented. Empty when there is
/// nothing to bias.
pub fn initial_prompt(vocabulary: &[String], replacements: &[Replacement]) -> String {
    let candidates = vocabulary
        .iter()
        .map(String::as_str)
        .chain(replacements.iter().map(|r| r.written.as_str()));
    let mut seen: HashSet<String> = HashSet::new();
    let mut terms: Vec<String> = Vec::new();
    let mut used = 0usize;
    for cand in candidates {
        let term = cand.split_whitespace().collect::<Vec<_>>().join(" ");
        let n = term.chars().count();
        if n == 0 || n > PROMPT_TERM_MAX_CHARS || term.chars().any(char::is_control) {
            continue;
        }
        if !seen.insert(term.to_lowercase()) {
            continue;
        }
        let add = n + if terms.is_empty() { 0 } else { 2 };
        if used + add > PROMPT_MAX_CHARS {
            break;
        }
        used += add;
        terms.push(term);
    }
    terms.join(", ")
}

/// Check a replacement list against the limits: at most [`MAX_REPLACEMENTS`] entries, `spoken`
/// 1..=[`MAX_SPOKEN_CHARS`] characters (surrounding blanks ignored, not empty), `written` at most
/// [`MAX_WRITTEN_CHARS`] characters, and no control characters in either.
pub fn validate_replacements(list: &[Replacement]) -> Result<(), String> {
    if list.len() > MAX_REPLACEMENTS {
        return Err(format!(
            "too many replacements ({}); the limit is {MAX_REPLACEMENTS}",
            list.len()
        ));
    }
    for (i, r) in list.iter().enumerate() {
        let n = i + 1;
        let spoken = r.spoken.trim();
        if spoken.is_empty() {
            return Err(format!("replacement {n}: the spoken phrase is empty"));
        }
        if spoken.chars().count() > MAX_SPOKEN_CHARS {
            return Err(format!(
                "replacement {n}: the spoken phrase is longer than {MAX_SPOKEN_CHARS} characters"
            ));
        }
        if r.spoken.chars().any(char::is_control) {
            return Err(format!(
                "replacement {n}: the spoken phrase contains a control character"
            ));
        }
        if r.written.chars().count() > MAX_WRITTEN_CHARS {
            return Err(format!(
                "replacement {n}: the written text is longer than {MAX_WRITTEN_CHARS} characters"
            ));
        }
        if r.written.chars().any(char::is_control) {
            return Err(format!(
                "replacement {n}: the written text contains a control character"
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// small shared helpers
// ---------------------------------------------------------------------------------------------

fn capitalise_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// The token without surrounding punctuation ("(5)," -> "5"; "don't," -> "don't").
fn word_core(tok: &str) -> &str {
    tok.trim_matches(|c: char| !c.is_alphanumeric())
}

fn word_lc(tok: &str) -> String {
    word_core(tok).to_lowercase()
}

fn ends_alnum(tok: &str) -> bool {
    tok.chars().last().is_some_and(char::is_alphanumeric)
}

fn ends_clause(tok: &str) -> bool {
    matches!(tok.chars().last(), Some(',' | ';'))
}

const ABBREVIATIONS: &[&str] = &[
    "dr", "mr", "mrs", "ms", "prof", "sr", "jr", "st", "vs", "inc", "ltd",
];

/// Does this token end a sentence? ("Dr.", "e.g." and an initial like "J." do not.)
fn ends_sentence(tok: &str) -> bool {
    let Some(last) = tok.chars().last() else {
        return false;
    };
    if !matches!(last, '.' | '!' | '?') {
        return false;
    }
    if last == '.' {
        let body = tok.trim_end_matches('.');
        if body.contains('.') || ABBREVIATIONS.contains(&body.to_lowercase().as_str()) {
            return false;
        }
        let mut ch = body.chars();
        if let (Some(c), None) = (ch.next(), ch.next()) {
            if c.is_uppercase() && c != 'I' {
                return false;
            }
        }
    }
    true
}

/// Split a token into (leading punctuation, core, trailing punctuation).
fn split_token(t: &str) -> (&str, &str, &str) {
    let Some(start) = t
        .char_indices()
        .find(|(_, c)| c.is_alphanumeric())
        .map(|(i, _)| i)
    else {
        return (t, "", "");
    };
    let end = t
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_alphanumeric())
        .map_or(t.len(), |(i, c)| i + c.len_utf8());
    (&t[..start], &t[start..end], &t[end..])
}

fn map_lines(text: &str, f: fn(&str) -> String) -> String {
    text.split('\n').map(f).collect::<Vec<_>>().join("\n")
}

// ---------------------------------------------------------------------------------------------
// step 1: Whisper quirks
// ---------------------------------------------------------------------------------------------

/// Labels that mean "no speech here" (compared lower-case, letters and single spaces only).
const NON_SPEECH: &[&str] = &[
    "music",
    "music playing",
    "applause",
    "laughter",
    "laughs",
    "silence",
    "blank audio",
    "no audio",
    "inaudible",
    "unintelligible",
    "noise",
    "background noise",
    "static",
    "coughing",
    "cough",
    "sighs",
    "sigh",
    "sneezes",
    "breathing",
    "crosstalk",
    "clapping",
    "singing",
    "whistling",
    "beep",
    "pause",
    "typing",
    "foreign language",
    "speaking foreign language",
    "speaking in foreign language",
];

const NON_SPEECH_SUFFIX: &[&str] = &[
    "music", "noise", "noises", "applause", "laughter", "silence", "sound", "sounds",
];

fn is_non_speech_label(s: &str) -> bool {
    let letters: String = s
        .chars()
        .map(|c| {
            if c.is_alphabetic() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect();
    let label = letters.split_whitespace().collect::<Vec<_>>().join(" ");
    if label.is_empty() {
        return false;
    }
    if NON_SPEECH.contains(&label.as_str()) {
        return true;
    }
    let words: Vec<&str> = label.split(' ').collect();
    words.len() <= 3 && words.last().is_some_and(|w| NON_SPEECH_SUFFIX.contains(w))
}

fn is_note_glyph(c: char) -> bool {
    matches!(c, '♪' | '♫' | '♬' | '♩')
}

/// `[...]`: always a Whisper tag (not a bare number, which might be a citation).
fn bracket_tag_end(chars: &[char], i: usize) -> Option<usize> {
    let limit = (i + 62).min(chars.len());
    let rel = chars[i + 1..limit]
        .iter()
        .position(|&c| c == ']' || c == '[')?;
    let e = i + 1 + rel;
    if chars[e] != ']' {
        return None;
    }
    let inner: String = chars[i + 1..e].iter().collect();
    let t = inner.trim();
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(e)
}

/// `(...)`: only when it is clearly a non-speech label such as "(music)" / "(soft music)".
fn paren_tag_end(chars: &[char], i: usize) -> Option<usize> {
    let limit = (i + 42).min(chars.len());
    let rel = chars[i + 1..limit]
        .iter()
        .position(|&c| c == ')' || c == '(')?;
    let e = i + 1 + rel;
    if chars[e] != ')' {
        return None;
    }
    let inner: String = chars[i + 1..e].iter().collect();
    is_non_speech_label(&inner).then_some(e)
}

/// A music-note glyph. If a second glyph follows within 80 characters and what lies between is
/// only a non-speech label, the whole span goes; otherwise just the glyph.
fn note_span_end(chars: &[char], i: usize) -> usize {
    let limit = (i + 81).min(chars.len());
    if let Some(rel) = chars[i + 1..limit].iter().position(|&c| is_note_glyph(c)) {
        let e = i + 1 + rel;
        let inner: String = chars[i + 1..e].iter().collect();
        if inner.trim().is_empty() || is_non_speech_label(&inner) {
            return e;
        }
    }
    i
}

fn strip_non_speech(raw: &str) -> (String, bool) {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::with_capacity(raw.len());
    let mut dropped = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == FILLER_MARK
            || c == '\u{feff}'
            || c == '\u{200b}'
            || (c.is_control() && !c.is_whitespace())
        {
            i += 1;
            continue;
        }
        let end = match c {
            '[' => bracket_tag_end(&chars, i),
            '(' => paren_tag_end(&chars, i),
            c if is_note_glyph(c) => Some(note_span_end(&chars, i)),
            _ => None,
        };
        if let Some(e) = end {
            dropped = true;
            let next = chars[e + 1..].iter().copied().find(|c| !c.is_whitespace());
            if matches!(next, Some(',' | '.' | ';' | ':' | '!' | '?')) {
                while out.ends_with(' ') {
                    out.pop();
                }
            } else {
                out.push(' ');
            }
            i = e + 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    (out, dropped)
}

/// "you" / "Thank you." as the *entire* transcript is what Whisper invents from silence.
fn is_lone_hallucination(text: &str) -> bool {
    let t = text
        .trim()
        .trim_matches(|c: char| matches!(c, '.' | '!' | '?' | ',' | ' '))
        .to_lowercase();
    matches!(
        t.as_str(),
        "you" | "thank you" | "thank you for watching" | "thanks for watching"
    )
}

fn normalize(raw: &str) -> String {
    let (stripped, dropped) = strip_non_speech(raw);
    let mut text = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    if dropped {
        text = text
            .trim_start_matches([',', '.', ';', ':', '!', '?', ' '])
            .to_string();
    }
    if is_lone_hallucination(&text) {
        return String::new();
    }
    text
}

// ---------------------------------------------------------------------------------------------
// step 2: script guard
// ---------------------------------------------------------------------------------------------

fn is_latin_letter(c: char) -> bool {
    c.is_ascii()
        || ('\u{00C0}'..='\u{024F}').contains(&c)
        || ('\u{1E00}'..='\u{1EFF}').contains(&c)
        || matches!(c, '\u{00AA}' | '\u{00BA}')
}

/// True when any letter is outside the Latin script.
fn has_non_latin(text: &str) -> bool {
    text.chars()
        .any(|c| c.is_alphabetic() && !is_latin_letter(c))
}

// ---------------------------------------------------------------------------------------------
// step 5: replacements
// ---------------------------------------------------------------------------------------------

struct Compiled {
    spoken: Vec<char>,
    written: String,
    words: usize,
}

fn fold_char(c: char) -> char {
    match c {
        '\u{2019}' | '\u{2018}' => '\'',
        _ => c.to_lowercase().next().unwrap_or(c),
    }
}

/// Valid entries only (invalid ones are skipped, never an error here), longest phrase first.
fn compile_replacements(list: &[Replacement]) -> Vec<Compiled> {
    let mut seen: HashSet<Vec<char>> = HashSet::new();
    let mut out = Vec::new();
    for r in list.iter().take(MAX_REPLACEMENTS) {
        let spoken = r.spoken.split_whitespace().collect::<Vec<_>>().join(" ");
        let n = spoken.chars().count();
        if n == 0 || n > MAX_SPOKEN_CHARS || spoken.chars().any(char::is_control) {
            continue;
        }
        if r.written.chars().count() > MAX_WRITTEN_CHARS || r.written.chars().any(char::is_control)
        {
            continue;
        }
        let folded: Vec<char> = spoken.chars().map(fold_char).collect();
        if !seen.insert(folded.clone()) {
            continue;
        }
        out.push(Compiled {
            spoken: folded,
            written: r.written.clone(),
            words: spoken.split(' ').count(),
        });
    }
    out.sort_by(|a, b| {
        b.words
            .cmp(&a.words)
            .then(b.spoken.len().cmp(&a.spoken.len()))
    });
    out
}

fn is_punct_boundary(c: char) -> bool {
    c.is_whitespace()
        || (c.is_ascii_punctuation() && !matches!(c, '_' | '\''))
        || matches!(
            c,
            '“' | '”'
                | '‘'
                | '«'
                | '»'
                | '—'
                | '–'
                | '…'
                | '。'
                | '、'
                | '，'
                | '！'
                | '？'
                | '：'
                | '；'
                | '（'
                | '）'
                | '「'
                | '」'
                | '।'
                | '॥'
                | '،'
                | '؛'
                | '؟'
        )
}

/// Is `chars[k]` a word boundary? An apostrophe between two letters is part of the word.
fn is_boundary_at(chars: &[char], k: usize) -> bool {
    let c = chars[k];
    if c == '\'' || c == '\u{2019}' {
        let internal = k > 0
            && k + 1 < chars.len()
            && chars[k - 1].is_alphanumeric()
            && chars[k + 1].is_alphanumeric();
        return !internal;
    }
    is_punct_boundary(c)
}

fn end_ok(chars: &[char], e: usize) -> bool {
    if e >= chars.len() || is_boundary_at(chars, e) {
        return true;
    }
    // possessive: "crovy's" still matches "crovy"
    if matches!(chars[e], '\'' | '\u{2019}') && matches!(chars.get(e + 1).copied(), Some('s' | 'S'))
    {
        return e + 2 >= chars.len() || is_boundary_at(chars, e + 2);
    }
    false
}

/// Match `pat` (folded) at `chars[i..]`; blanks in the pattern match a run of spaces/tabs.
fn match_at(chars: &[char], i: usize, pat: &[char]) -> Option<usize> {
    let mut t = i;
    for &p in pat {
        if p == ' ' {
            if !matches!(chars.get(t).copied(), Some(' ' | '\t')) {
                return None;
            }
            while matches!(chars.get(t).copied(), Some(' ' | '\t')) {
                t += 1;
            }
        } else {
            if t >= chars.len() || fold_char(chars[t]) != p {
                return None;
            }
            t += 1;
        }
    }
    Some(t)
}

fn tidy_after_deletion(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == ' ' && (out.ends_with(' ') || out.ends_with('\n') || out.is_empty()) {
            continue;
        }
        if (matches!(c, ',' | '.' | ';' | ':' | '!' | '?') || c == '\n') && out.ends_with(' ') {
            out.pop();
        }
        out.push(c);
    }
    out.trim().to_string()
}

fn apply_replacements(text: &str, list: &[Replacement]) -> String {
    if list.is_empty() || text.is_empty() {
        return text.to_string();
    }
    let compiled = compile_replacements(list);
    if compiled.is_empty() {
        return text.to_string();
    }
    let mut by_first: HashMap<char, Vec<usize>> = HashMap::new();
    for (k, c) in compiled.iter().enumerate() {
        by_first.entry(c.spoken[0]).or_default().push(k);
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 16);
    let mut deleted = false;
    let mut i = 0;
    while i < chars.len() {
        if i == 0 || is_boundary_at(&chars, i - 1) {
            if let Some(cands) = by_first.get(&fold_char(chars[i])) {
                let hit = cands.iter().find_map(|&k| {
                    match_at(&chars, i, &compiled[k].spoken)
                        .filter(|&e| end_ok(&chars, e))
                        .map(|e| (k, e))
                });
                if let Some((k, e)) = hit {
                    out.push_str(&compiled[k].written);
                    deleted |= compiled[k].written.is_empty();
                    i = e;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    if deleted {
        tidy_after_deletion(&out)
    } else {
        out
    }
}

// ---------------------------------------------------------------------------------------------
// prose styles: filler protection, clean.rs, self-correction
// ---------------------------------------------------------------------------------------------

/// Mark the fillers clean.rs would remove so that `remove_fillers = false` keeps them.
fn protect_fillers(text: &str) -> String {
    let words: Vec<&str> = text.split(' ').collect();
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    for (i, w) in words.iter().enumerate() {
        let core_end = w.trim_end_matches(['.', ',', ';', ':', '!', '?']).len();
        let (core, trail) = w.split_at(core_end);
        let lc = core.to_lowercase();
        let next_is_know = words.get(i + 1).is_some_and(|n| word_lc(n) == "know");
        let mark =
            is_hesitation(&lc) || lc == "like" || (lc == "you" && trail.is_empty() && next_is_know);
        if mark {
            out.push(format!("{core}{FILLER_MARK}{trail}"));
        } else {
            out.push((*w).to_string());
        }
    }
    out.join(" ")
}

/// Cheap pre-check so ordinary text is cleaned only once.
fn may_have_correction(text: &str) -> bool {
    text.split_whitespace().any(|w| {
        let c = word_core(w);
        ["scratch", "delete", "wait", "mean"]
            .iter()
            .any(|k| c.eq_ignore_ascii_case(k))
    })
}

/// Close a repaired sentence of three or more words (clean.rs closes from four).
fn close_corrected(mut s: String) -> String {
    let Some(last) = s.chars().last() else {
        return s;
    };
    if !last.is_alphanumeric() {
        return s;
    }
    let start = s.rfind(['.', '!', '?', '\n']).map_or(0, |p| p + 1);
    if s[start..].split_whitespace().count() >= 3 {
        s.push('.');
    }
    s
}

/// One prose chunk through clean.rs (+ self-correction). `finish` = add the closing mark.
fn paragraph_with(text: &str, opts: &DictationOptions, finish: bool) -> String {
    let protected: Cow<str> = if opts.remove_fillers {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(protect_fillers(text))
    };
    let cleaned = if may_have_correction(&protected) {
        let s1 = clean_fragment(&protected);
        let s2 = self_correct(&s1);
        if s2 == s1 {
            if finish {
                clean_text(&protected)
            } else {
                s1
            }
        } else if finish {
            close_corrected(clean_text(&s2))
        } else {
            clean_fragment(&s2)
        }
    } else if finish {
        clean_text(&protected)
    } else {
        clean_fragment(&protected)
    };
    cleaned.replace(FILLER_MARK, "")
}

fn prose(text: &str, opts: &DictationOptions) -> String {
    let body = match opts.style {
        DictationStyle::Email => email(text, opts),
        DictationStyle::Note => note(text, opts),
        _ => paragraph_with(text, opts, opts.auto_punctuate),
    };
    match opts.style {
        DictationStyle::Message => message_tidy(body),
        DictationStyle::Professional => map_lines(&body, expand_line),
        DictationStyle::Casual => map_lines(&body, contract_line),
        _ => body,
    }
}

// ---------------------------------------------------------------------------------------------
// spoken self-correction (runs on already-cleaned text)
// ---------------------------------------------------------------------------------------------

/// Words right before "scratch that" / "delete that" that make it part of a sentence
/// ("we should scratch that idea", "I want to delete that").
const SCRATCH_BLOCK_BEFORE: &[&str] = &[
    "to", "we", "you", "i", "they", "should", "can", "could", "would", "will", "might", "must",
    "please", "just", "let's", "lets", "and", "or", "but", "if", "then", "not", "don't", "never",
];

const NUMBER_WORDS: &[&str] = &[
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "seventy",
    "eighty",
    "ninety",
    "hundred",
    "thousand",
    "half",
    "noon",
    "midnight",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Number,
    Name,
    Unit,
    Other,
}

fn classify(tok: &str, sentence_first: bool) -> Class {
    let c = word_core(tok);
    let Some(first) = c.chars().next() else {
        return Class::Other;
    };
    if c.chars().any(|ch| ch.is_ascii_digit())
        && c.chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, ':' | '.' | ','))
    {
        return Class::Number;
    }
    let lc = c.to_lowercase();
    if NUMBER_WORDS.contains(&lc.as_str()) {
        return Class::Number;
    }
    if lc == "pm" || lc == "am" {
        return Class::Unit;
    }
    if !sentence_first
        && first.is_uppercase()
        && c != "I"
        && c.chars()
            .all(|ch| ch.is_alphabetic() || matches!(ch, '-' | '\''))
    {
        return Class::Name;
    }
    Class::Other
}

fn self_correct(text: &str) -> String {
    // None = the line was emptied by a correction and disappears
    let mut lines: Vec<Option<String>> = Vec::new();
    for line in text.split('\n') {
        if line.trim().is_empty() {
            lines.push(Some(String::new()));
            continue;
        }
        let fixed = correct_line(line);
        lines.push(if fixed.is_empty() { None } else { Some(fixed) });
    }
    let mut out: Vec<String> = Vec::new();
    for l in lines.into_iter().flatten() {
        if l.is_empty() && !matches!(out.last(), Some(p) if !p.is_empty()) {
            continue;
        }
        out.push(l);
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out.join("\n")
}

fn correct_line(line: &str) -> String {
    let mut toks: Vec<String> = line
        .split(' ')
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect();
    for _ in 0..MAX_CORRECTIONS {
        let next =
            (0..toks.len()).find_map(|i| try_scratch(&toks, i).or_else(|| try_replace(&toks, i)));
        match next {
            Some(t) => toks = t,
            None => break,
        }
    }
    toks.join(" ")
}

/// "scratch that" / "delete that": remove the preceding sentence (everything before it if it is
/// the first) together with the command.
fn try_scratch(toks: &[String], i: usize) -> Option<Vec<String>> {
    let first = word_lc(&toks[i]);
    if first != "scratch" && first != "delete" {
        return None;
    }
    if !ends_alnum(&toks[i]) {
        return None;
    }
    let that = toks.get(i + 1)?;
    if word_lc(that) != "that" {
        return None;
    }
    let standalone = i + 2 >= toks.len() || !ends_alnum(that);
    let at_clause_start = i == 0 || ends_clause(&toks[i - 1]) || ends_sentence(&toks[i - 1]);
    let blocked = i > 0 && SCRATCH_BLOCK_BEFORE.contains(&word_lc(&toks[i - 1]).as_str());
    let ok = if first == "scratch" {
        at_clause_start || (standalone && !blocked)
    } else {
        standalone && (at_clause_start || !blocked)
    };
    if !ok {
        return None;
    }
    let start = if i <= 1 {
        0
    } else {
        (0..i - 1)
            .rev()
            .find(|&j| ends_sentence(&toks[j]))
            .map_or(0, |j| j + 1)
    };
    let mut out = toks[..start].to_vec();
    out.extend_from_slice(&toks[i + 2..]);
    Some(out)
}

/// `<A>, no wait, <B>` / `<A>, sorry I mean <B>` / `<A>, I mean <B>`: B replaces the last
/// words of A. Only for short, parallel phrases (see the module docs); otherwise `None`.
fn try_replace(toks: &[String], i: usize) -> Option<Vec<String>> {
    if i == 0 {
        return None;
    }
    let c0 = word_lc(&toks[i]);
    let c1 = toks.get(i + 1).map(|t| word_lc(t)).unwrap_or_default();
    let c2 = toks.get(i + 2).map(|t| word_lc(t)).unwrap_or_default();
    // (marker length, strict, may follow a full stop)
    let (mlen, strict, after_period) = if c0 == "no" && c1 == "wait" {
        (2, false, true)
    } else if c0 == "sorry" && c1 == "i" && c2 == "mean" {
        (3, false, true)
    } else if c0 == "i" && c1 == "mean" {
        (2, true, false)
    } else {
        return None;
    };
    for t in &toks[i..i + mlen] {
        if let Some(c) = t.chars().last() {
            if !c.is_alphanumeric() && c != ',' {
                return None;
            }
        }
    }
    let prev = &toks[i - 1];
    let prev_last = prev.chars().last()?;
    if !(prev_last == ',' || (after_period && prev_last == '.' && ends_sentence(prev))) {
        return None;
    }
    let a_start = (0..i - 1)
        .rev()
        .find(|&j| ends_sentence(&toks[j]) || ends_clause(&toks[j]))
        .map_or(0, |j| j + 1);
    let na = i - a_start;
    if na == 0 || na > MAX_CLAUSE_WORDS {
        return None;
    }
    let b_start = i + mlen;
    if b_start >= toks.len() {
        return None;
    }
    let b_end = (b_start..toks.len())
        .find(|&k| matches!(toks[k].chars().last(), Some(',' | ';' | '.' | '!' | '?')))
        .map_or(toks.len(), |k| k + 1);
    let nb = b_end - b_start;
    if nb == 0 || nb > MAX_CLAUSE_WORDS || nb > na {
        return None;
    }
    let t_idx = i - nb;
    let at_sentence_start = t_idx == a_start && (a_start == 0 || ends_sentence(&toks[a_start - 1]));
    let tc = classify(&toks[t_idx], at_sentence_start);
    let bc = classify(&toks[b_start], false);
    if tc == Class::Unit || bc == Class::Unit {
        return None;
    }
    let compatible = if nb >= 2 {
        word_lc(&toks[t_idx]) == word_lc(&toks[b_start]) || (tc == bc && tc != Class::Other)
    } else if strict {
        tc == bc && tc != Class::Other
    } else {
        tc == bc
    };
    if !compatible {
        return None;
    }
    let mut out = toks[..t_idx].to_vec();
    out.extend_from_slice(&toks[b_start..]);
    Some(out)
}

// ---------------------------------------------------------------------------------------------
// Message
// ---------------------------------------------------------------------------------------------

fn message_tidy(s: String) -> String {
    if s.contains('\n') {
        return s;
    }
    let Some(body) = s.strip_suffix('.') else {
        return s;
    };
    if body.is_empty() || body.ends_with('.') {
        return s;
    }
    let toks: Vec<&str> = body.split(' ').collect();
    if toks.len() > 8 || toks[..toks.len() - 1].iter().any(|t| ends_sentence(t)) {
        return s;
    }
    body.to_string()
}

// ---------------------------------------------------------------------------------------------
// Email
// ---------------------------------------------------------------------------------------------

fn split_paragraphs(text: &str) -> Vec<String> {
    let toks: Vec<&str> = text.split(' ').collect();
    let mut pieces: Vec<String> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if toks[i].eq_ignore_ascii_case("new")
            && toks.get(i + 1).is_some_and(|t| word_lc(t) == "paragraph")
        {
            pieces.push(finish_piece(&cur));
            cur.clear();
            i += 2;
            continue;
        }
        cur.push(toks[i]);
        i += 1;
    }
    pieces.push(finish_piece(&cur));
    pieces.retain(|p| !p.is_empty());
    pieces
}

fn finish_piece(toks: &[&str]) -> String {
    toks.join(" ")
        .trim()
        .trim_end_matches([',', ';', ' '])
        .to_string()
}

const GREETINGS: &[&str] = &["hi", "hello", "hey", "dear", "hiya", "greetings"];
const GROUP_WORDS: &[&str] = &[
    "there", "everyone", "all", "team", "folks", "guys", "friends", "sir", "madam", "both",
];

/// "Hi Sam, the meeting moved" -> ("Hi Sam,", "The meeting moved"). `None` when it is not
/// clearly a greeting line.
fn split_greeting(p: &str) -> Option<(String, String)> {
    if p.contains('\n') {
        return None;
    }
    let toks: Vec<&str> = p.split(' ').collect();
    let g = word_lc(toks.first()?);
    let glen = if g == "good" {
        let second = word_lc(toks.get(1)?);
        if !matches!(second.as_str(), "morning" | "afternoon" | "evening") {
            return None;
        }
        2
    } else if GREETINGS.contains(&g.as_str()) {
        1
    } else {
        return None;
    };
    // "Hi," / "Hello." with punctuation on the greeting word itself: leave alone
    if !toks[..glen].iter().all(|t| ends_alnum(t)) || toks.len() <= glen {
        return None;
    }
    let end = (glen..toks.len())
        .find(|&k| !ends_alnum(toks[k]))
        .unwrap_or(toks.len() - 1);
    let names = &toks[glen..=end];
    if names.is_empty() || names.len() > 3 {
        return None;
    }
    if let Some(last) = names.last().and_then(|t| t.chars().last()) {
        if !last.is_alphanumeric() && !matches!(last, ',' | '.' | '!') {
            return None;
        }
    }
    for t in names {
        let c = word_core(t);
        let ok_chars = !c.is_empty()
            && c.chars()
                .all(|ch| ch.is_alphabetic() || matches!(ch, '-' | '\''));
        let ok_word = c.chars().next().is_some_and(char::is_uppercase)
            || GROUP_WORDS.contains(&c.to_lowercase().as_str());
        if !ok_chars || !ok_word {
            return None;
        }
    }
    let mut line = toks[..end].join(" ");
    line.push(' ');
    line.push_str(toks[end].trim_end_matches(|c: char| !c.is_alphanumeric()));
    line.push(',');
    let rest = capitalise_first(&toks[end + 1..].join(" "));
    Some((line, rest))
}

const SIGNOFFS: &[&[&str]] = &[
    &["best", "regards"],
    &["kind", "regards"],
    &["warm", "regards"],
    &["best", "wishes"],
    &["warm", "wishes"],
    &["yours", "sincerely"],
    &["yours", "truly"],
    &["many", "thanks"],
    &["thank", "you"],
    &["thanks"],
    &["regards"],
    &["sincerely"],
    &["cheers"],
    &["best"],
];

/// Put a closing phrase ("Thanks", "Best regards") and an optional capitalised name (at most
/// three words) on their own lines.
fn split_signoff(paras: &mut Vec<String>) {
    let Some(last) = paras.last() else {
        return;
    };
    let (head, line) = match last.rfind('\n') {
        Some(p) => (&last[..=p], &last[p + 1..]),
        None => ("", last.as_str()),
    };
    let toks: Vec<&str> = line.split(' ').collect();
    let s = (0..toks.len().saturating_sub(1))
        .rev()
        .find(|&k| ends_sentence(toks[k]))
        .map_or(0, |k| k + 1);
    let sent = &toks[s..];
    let Some(plen) = SIGNOFFS
        .iter()
        .find(|p| {
            sent.len() >= p.len()
                && p.iter().enumerate().all(|(k, w)| word_lc(sent[k]) == *w)
                && sent[..p.len() - 1].iter().all(|t| ends_alnum(t))
        })
        .map(|p| p.len())
    else {
        return;
    };
    let phrase = &sent[..plen];
    let rest = &sent[plen..];
    if rest.len() > 3 {
        return;
    }
    let phrase_last = phrase[plen - 1];
    let ptail = &phrase_last
        [word_core(phrase_last).len() + phrase_last.find(word_core(phrase_last)).unwrap_or(0)..];
    if !rest.is_empty() {
        if !(ptail.is_empty() || ptail == ",") {
            return;
        }
        for (k, t) in rest.iter().enumerate() {
            let c = word_core(t);
            let name_like = c.chars().next().is_some_and(char::is_uppercase)
                && c.chars()
                    .all(|ch| ch.is_alphabetic() || matches!(ch, '-' | '\''));
            let punct_ok = if k + 1 == rest.len() {
                t[t.find(c).unwrap_or(0) + c.len()..]
                    .chars()
                    .all(|ch| matches!(ch, '.' | '!'))
            } else {
                ends_alnum(t)
            };
            if !name_like || !punct_ok {
                return;
            }
        }
    } else if !ptail.chars().all(|ch| matches!(ch, '.' | ',' | '!')) {
        return;
    }
    let mut phrase_text = phrase[..plen - 1].join(" ");
    if !phrase_text.is_empty() {
        phrase_text.push(' ');
    }
    phrase_text.push_str(word_core(phrase_last));
    let block = if rest.is_empty() {
        match ptail {
            "," => phrase_text.push(','),
            "!" => phrase_text.push('!'),
            _ => {}
        }
        phrase_text
    } else {
        phrase_text.push(',');
        let name = rest.join(" ").trim_end_matches(['.', '!']).to_string();
        format!("{phrase_text}\n{name}")
    };
    let top = format!("{head}{}", toks[..s].join(" "));
    if top.trim().is_empty() {
        let n = paras.len();
        paras[n - 1] = block;
    } else {
        let n = paras.len();
        paras[n - 1] = top.trim_end().to_string();
        paras.push(block);
    }
}

fn email(text: &str, opts: &DictationOptions) -> String {
    let mut paras: Vec<String> = split_paragraphs(text)
        .iter()
        .map(|p| {
            apply_replacements(
                &paragraph_with(p, opts, opts.auto_punctuate),
                &opts.replacements,
            )
        })
        .filter(|p| !p.is_empty())
        .collect();
    if paras.is_empty() {
        return String::new();
    }
    if let Some((line, rest)) = split_greeting(&paras[0]) {
        paras[0] = line;
        if !rest.is_empty() {
            paras.insert(1, rest);
        }
    }
    split_signoff(&mut paras);
    paras.join("\n\n")
}

// ---------------------------------------------------------------------------------------------
// Note
// ---------------------------------------------------------------------------------------------

/// Before these, "next item" / "bullet" is an ordinary noun phrase ("the next item on the agenda").
const LIST_BLOCK_BEFORE: &[&str] = &[
    "a", "an", "the", "this", "that", "one", "each", "every", "silver", "magic",
];

fn note(text: &str, opts: &DictationOptions) -> String {
    let toks: Vec<&str> = text.split(' ').collect();
    let mut pieces: Vec<Vec<&str>> = vec![Vec::new()];
    let mut found = false;
    let mut i = 0;
    while i < toks.len() {
        let c = word_lc(toks[i]);
        let c2 = toks.get(i + 1).map(|t| word_lc(t)).unwrap_or_default();
        let blocked = i > 0 && LIST_BLOCK_BEFORE.contains(&word_lc(toks[i - 1]).as_str());
        let marker = if blocked {
            0
        } else if (c == "new" || c == "next") && c2 == "item" {
            2
        } else if c == "bullet" && c2 == "point" {
            2
        } else if c == "bullet" && !matches!(c2.as_str(), "train" | "proof" | "points" | "list") {
            1
        } else {
            0
        };
        if marker > 0 {
            found = true;
            pieces.push(Vec::new());
            i += marker;
        } else {
            if let Some(p) = pieces.last_mut() {
                p.push(toks[i]);
            }
            i += 1;
        }
    }
    if !found {
        return paragraph_with(text, opts, opts.auto_punctuate);
    }
    let edge = |c: char| matches!(c, ',' | '.' | ';' | ':' | ' ');
    let mut lines: Vec<String> = Vec::new();
    let heading = pieces[0].join(" ");
    let heading = heading.trim_end_matches([',', ';', ' ']);
    if !heading.is_empty() {
        let h = paragraph_with(heading, opts, opts.auto_punctuate);
        if !h.is_empty() {
            lines.push(h);
        }
    }
    for piece in &pieces[1..] {
        let joined = piece.join(" ");
        let body = joined.trim_start_matches(edge);
        if body.is_empty() {
            continue;
        }
        let item = paragraph_with(body, opts, false);
        let item = item.trim().trim_end_matches(['.', ',', ';']);
        if !item.is_empty() {
            lines.push(format!("- {item}"));
        }
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------------------------
// Professional / Casual
// ---------------------------------------------------------------------------------------------

const EXPAND: &[(&str, &str)] = &[
    ("don't", "do not"),
    ("doesn't", "does not"),
    ("didn't", "did not"),
    ("can't", "cannot"),
    ("won't", "will not"),
    ("wouldn't", "would not"),
    ("shouldn't", "should not"),
    ("couldn't", "could not"),
    ("isn't", "is not"),
    ("aren't", "are not"),
    ("wasn't", "was not"),
    ("weren't", "were not"),
    ("haven't", "have not"),
    ("hasn't", "has not"),
    ("hadn't", "had not"),
    ("i'm", "I am"),
    ("i've", "I have"),
    ("i'll", "I will"),
    ("you're", "you are"),
    ("we're", "we are"),
    ("they're", "they are"),
    ("you've", "you have"),
    ("we've", "we have"),
    ("they've", "they have"),
    ("you'll", "you will"),
    ("we'll", "we will"),
    ("they'll", "they will"),
    ("let's", "let us"),
    ("gonna", "going to"),
    ("wanna", "want to"),
];

/// "'s" forms expanded only for these subjects: "is", or "has" before "been"/"got".
const EXPAND_S: &[(&str, &str)] = &[
    ("it's", "it"),
    ("that's", "that"),
    ("there's", "there"),
    ("here's", "here"),
    ("what's", "what"),
];

const CONTRACT_NEG: &[(&str, &str, &str)] = &[
    ("do", "not", "don't"),
    ("does", "not", "doesn't"),
    ("did", "not", "didn't"),
    ("will", "not", "won't"),
    ("would", "not", "wouldn't"),
    ("should", "not", "shouldn't"),
    ("could", "not", "couldn't"),
    ("is", "not", "isn't"),
    ("are", "not", "aren't"),
    ("was", "not", "wasn't"),
    ("were", "not", "weren't"),
    ("have", "not", "haven't"),
    ("has", "not", "hasn't"),
    ("had", "not", "hadn't"),
];

/// Pronoun + verb: contracted only mid-clause (never "Yes, it is.").
const CONTRACT_PRON: &[(&str, &str, &str)] = &[
    ("i", "am", "I'm"),
    ("you", "are", "you're"),
    ("we", "are", "we're"),
    ("they", "are", "they're"),
    ("it", "is", "it's"),
    ("that", "is", "that's"),
    ("i", "will", "I'll"),
    ("you", "will", "you'll"),
    ("we", "will", "we'll"),
    ("they", "will", "they'll"),
];

/// (lower-case form, was first letter capitalised). `None` for odd casing (e.g. all caps).
fn case_of(core: &str) -> Option<(String, bool)> {
    let n = core.replace('\u{2019}', "'");
    let lc = n.to_lowercase();
    if n == lc {
        Some((lc, false))
    } else if n == capitalise_first(&lc) {
        Some((lc, true))
    } else {
        None
    }
}

fn expand_line(line: &str) -> String {
    let toks: Vec<&str> = line.split(' ').collect();
    let mut out: Vec<String> = Vec::with_capacity(toks.len());
    for (i, t) in toks.iter().enumerate() {
        let (lead, core, trail) = split_token(t);
        let mut replaced = None;
        // quick reject: every expandable form has an apostrophe, or is gonna / wanna
        let candidate = core.contains(['\'', '\u{2019}'])
            || core.eq_ignore_ascii_case("gonna")
            || core.eq_ignore_ascii_case("wanna");
        if let Some((lc, cap)) = candidate.then(|| case_of(core)).flatten() {
            let target = if let Some((_, to)) = EXPAND.iter().find(|(k, _)| *k == lc) {
                Some((*to).to_string())
            } else if let Some((_, subj)) = EXPAND_S.iter().find(|(k, _)| *k == lc) {
                let next = toks.get(i + 1).map(|n| word_lc(n)).unwrap_or_default();
                let verb = if next == "been" || next == "got" {
                    "has"
                } else {
                    "is"
                };
                Some(format!("{subj} {verb}"))
            } else {
                None
            };
            if let Some(to) = target {
                let to = if cap { capitalise_first(&to) } else { to };
                replaced = Some(format!("{lead}{to}{trail}"));
            }
        }
        out.push(replaced.unwrap_or_else(|| (*t).to_string()));
    }
    out.join(" ")
}

fn contract_line(line: &str) -> String {
    let toks: Vec<&str> = line.split(' ').collect();
    let mut out: Vec<String> = Vec::with_capacity(toks.len());
    let mut i = 0;
    while i < toks.len() {
        let (lead, core, trail) = split_token(toks[i]);
        // quick reject: the first word of every contractible pair is short
        let first = (!core.is_empty() && core.len() <= 6 && trail.is_empty())
            .then(|| case_of(core))
            .flatten();
        if let Some((lc, cap)) = first {
            let put = |s: &str| {
                if cap {
                    capitalise_first(s)
                } else {
                    s.to_string()
                }
            };
            if lc == "cannot" {
                out.push(format!("{lead}{}", put("can't")));
                i += 1;
                continue;
            }
            if let Some(nxt) = toks.get(i + 1) {
                let (nlead, ncore, ntrail) = split_token(nxt);
                let nlc = ncore.to_lowercase();
                if nlead.is_empty() && !ncore.is_empty() {
                    let third = toks.get(i + 2).map(|t| word_lc(t)).unwrap_or_default();
                    let hit = CONTRACT_NEG
                        .iter()
                        .find(|(a, b, _)| *a == lc && *b == nlc)
                        .map(|(_, _, c)| *c)
                        .or_else(|| {
                            CONTRACT_PRON
                                .iter()
                                .find(|(a, b, _)| *a == lc && *b == nlc)
                                .filter(|_| {
                                    ntrail.is_empty() && !third.is_empty() && third != "not"
                                })
                                .map(|(_, _, c)| *c)
                        });
                    if let Some(c) = hit {
                        out.push(format!("{lead}{}{ntrail}", put(c)));
                        i += 2;
                        continue;
                    }
                }
            }
        }
        out.push(toks[i].to_string());
        i += 1;
    }
    out.join(" ")
}

// ---------------------------------------------------------------------------------------------
// Code
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Word,
    /// no space either side: `.` `_` `-` `/`
    Tight,
    /// no space after, none before a word: `(` `[` `{`
    Open,
    /// no space before: `)` `]` `}`
    Close,
    /// no space before, one after: `,` `:` `;`
    After,
    /// spaces both sides: `=` `==`
    Op,
    /// space before, none after: `--`
    Flag,
    /// a line break
    Nl,
}

const SYMBOLS: &[(&[&str], &str, Kind)] = &[
    (&["open", "parenthesis"], "(", Kind::Open),
    (&["close", "parenthesis"], ")", Kind::Close),
    (&["open", "paren"], "(", Kind::Open),
    (&["close", "paren"], ")", Kind::Close),
    (&["open", "bracket"], "[", Kind::Open),
    (&["close", "bracket"], "]", Kind::Close),
    (&["open", "brace"], "{", Kind::Open),
    (&["close", "brace"], "}", Kind::Close),
    (&["new", "line"], "\n", Kind::Nl),
    (&["newline"], "\n", Kind::Nl),
    (&["semicolon"], ";", Kind::After),
    (&["underscore"], "_", Kind::Tight),
    (&["equals"], "=", Kind::Op),
    (&["comma"], ",", Kind::After),
    (&["colon"], ":", Kind::After),
    (&["slash"], "/", Kind::Tight),
    (&["dash"], "-", Kind::Tight),
    (&["dot"], ".", Kind::Tight),
];

/// Hesitations that cannot plausibly be identifiers; the only fillers Code removes.
const CODE_HESITATIONS: &[&str] = &["um", "uh", "uhm", "umm", "uhh", "erm", "ehm"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum CaseStyle {
    Camel,
    Snake,
}

enum Cd {
    Word(String),
    Sym(String, Kind),
    Newline,
}

fn match_symbol(lcs: &[String], i: usize) -> Option<(&'static str, Kind, usize)> {
    SYMBOLS.iter().find_map(|(words, out, kind)| {
        (i + words.len() <= lcs.len() && words.iter().enumerate().all(|(k, w)| lcs[i + k] == *w))
            .then_some((*out, *kind, words.len()))
    })
}

fn case_command(lcs: &[String], i: usize) -> Option<CaseStyle> {
    if lcs.get(i + 1).map(String::as_str) != Some("case") {
        return None;
    }
    match lcs[i].as_str() {
        "camel" => Some(CaseStyle::Camel),
        "snake" => Some(CaseStyle::Snake),
        _ => None,
    }
}

fn build_identifier(style: CaseStyle, parts: &[&str]) -> String {
    let words: Vec<String> = parts
        .iter()
        .map(|p| {
            p.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect();
    match style {
        CaseStyle::Snake => words.join("_"),
        CaseStyle::Camel => words
            .iter()
            .enumerate()
            .map(|(i, w)| {
                if i == 0 {
                    w.clone()
                } else {
                    capitalise_first(w)
                }
            })
            .collect(),
    }
}

fn needs_space(prev: Option<Kind>, cur: Kind) -> bool {
    use Kind::*;
    let Some(p) = prev else {
        return false;
    };
    match (p, cur) {
        (_, Flag) => !matches!(p, Open),
        (Flag, _) => false,
        (_, Tight) | (Tight, _) => false,
        (_, After) | (_, Close) => false,
        (Open, _) => false,
        (_, Open) => matches!(p, After | Op),
        (_, Op) => true,
        (Op | After, _) => true,
        _ => true,
    }
}

fn render_code(toks: &[Cd]) -> String {
    let mut out = String::new();
    let mut prev: Option<Kind> = None;
    for t in toks {
        match t {
            Cd::Newline => {
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push('\n');
                prev = None;
            }
            Cd::Word(w) => {
                if needs_space(prev, Kind::Word) {
                    out.push(' ');
                }
                out.push_str(w);
                prev = Some(Kind::Word);
            }
            Cd::Sym(s, k) => {
                if needs_space(prev, *k) {
                    out.push(' ');
                }
                out.push_str(s);
                prev = Some(*k);
            }
        }
    }
    out
}

fn code(text: &str, opts: &DictationOptions) -> String {
    // Whisper's own end-of-word punctuation is pause-driven noise here; symbols are spoken.
    let words: Vec<&str> = text
        .split(' ')
        .map(|w| w.trim_end_matches(['.', ',', '!', '?']))
        .filter(|w| !w.is_empty())
        .collect();
    let lcs: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    let hesitation = |k: usize| opts.remove_fillers && CODE_HESITATIONS.contains(&lcs[k].as_str());
    let mut toks: Vec<Cd> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if let Some(style) = case_command(&lcs, i) {
            let mut j = i + 2;
            let mut parts: Vec<&str> = Vec::new();
            while j < words.len()
                && match_symbol(&lcs, j).is_none()
                && case_command(&lcs, j).is_none()
            {
                if !hesitation(j) {
                    parts.push(words[j]);
                }
                j += 1;
            }
            let ident = build_identifier(style, &parts);
            if ident.is_empty() {
                toks.push(Cd::Word(words[i].to_string()));
                toks.push(Cd::Word(words[i + 1].to_string()));
                i += 2;
            } else {
                toks.push(Cd::Word(ident));
                i = j;
            }
            continue;
        }
        if let Some((sym, kind, n)) = match_symbol(&lcs, i) {
            if kind == Kind::Nl {
                toks.push(Cd::Newline);
            } else {
                toks.push(Cd::Sym(sym.to_string(), kind));
            }
            i += n;
            continue;
        }
        if hesitation(i) {
            i += 1;
            continue;
        }
        toks.push(Cd::Word(words[i].to_string()));
        i += 1;
    }
    // "equals equals" -> "==", "colon colon" -> "::", "dash dash" -> "--"
    let mut merged: Vec<Cd> = Vec::with_capacity(toks.len());
    for t in toks {
        if let (Cd::Sym(s, _), Some(Cd::Sym(ps, pk))) = (&t, merged.last_mut()) {
            if s.as_str() == "=" && ps.as_str() == "=" && *pk == Kind::Op {
                ps.push('=');
                continue;
            }
            if s.as_str() == ":" && ps.as_str() == ":" && *pk == Kind::After {
                ps.push(':');
                *pk = Kind::Tight;
                continue;
            }
            if s.as_str() == "-" && ps.as_str() == "-" && *pk == Kind::Tight {
                ps.push('-');
                *pk = Kind::Flag;
                continue;
            }
        }
        merged.push(t);
    }
    render_code(&merged)
}

// ---------------------------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(style: DictationStyle) -> DictationOptions {
        DictationOptions {
            style,
            ..DictationOptions::default()
        }
    }

    fn plain(s: &str) -> String {
        process(s, &DictationOptions::default())
    }

    fn style(st: DictationStyle, s: &str) -> String {
        process(s, &opts(st))
    }

    fn rep(spoken: &str, written: &str) -> Replacement {
        Replacement {
            spoken: spoken.into(),
            written: written.into(),
        }
    }

    fn with_reps(s: &str, reps: Vec<Replacement>) -> String {
        process(
            s,
            &DictationOptions {
                replacements: reps,
                ..DictationOptions::default()
            },
        )
    }

    // --- Whisper quirks -----------------------------------------------------------------

    #[test]
    fn non_speech_tags_are_dropped() {
        assert_eq!(plain("[BLANK_AUDIO]"), "");
        assert_eq!(plain("(music)"), "");
        assert_eq!(plain("[inaudible]"), "");
        assert_eq!(plain("♪"), "");
        assert_eq!(plain("♪ Music ♪"), "");
        assert_eq!(plain("[Music] [Applause]"), "");
        assert_eq!(plain("(upbeat music)"), "");
        assert_eq!(plain("hello [inaudible] world"), "Hello world");
        assert_eq!(
            plain("Send the report [BLANK_AUDIO] to the team today"),
            "Send the report to the team today."
        );
        assert_eq!(plain("Hello [Music], world"), "Hello, world");
    }

    #[test]
    fn ordinary_brackets_and_parentheses_survive() {
        assert!(plain("see reference [1] for details please").contains("[1]"));
        assert!(plain("call me on (555) 123 4567").contains("(555)"));
        assert_eq!(
            plain("the sample (the second one) is ready"),
            "The sample (the second one) is ready."
        );
    }

    #[test]
    fn whitespace_is_collapsed_and_trimmed() {
        assert_eq!(plain("  hello \n  world\t "), "Hello world");
        assert_eq!(plain(""), "");
        assert_eq!(plain("   \n "), "");
        assert_eq!(plain("\u{feff}\u{200b}hello"), "Hello");
    }

    #[test]
    fn lone_hallucinations_are_dropped_only_when_they_are_everything() {
        for s in [
            "you",
            "You.",
            " you ",
            "Thank you.",
            "thank you!",
            "Thanks for watching!",
            "Thank you for watching.",
        ] {
            assert_eq!(plain(s), "", "{s:?}");
        }
        assert_eq!(
            plain("thank you for the update"),
            "Thank you for the update."
        );
        assert_eq!(
            plain("you are right about that"),
            "You are right about that."
        );
        assert_eq!(plain("hello you"), "Hello you");
        assert_eq!(plain("[BLANK_AUDIO] you"), "");
    }

    // --- plain = clean.rs ---------------------------------------------------------------

    #[test]
    fn plain_is_clean_rs() {
        for s in [
            "um remember to uh call the dentist tomorrow morning",
            "buy milk comma eggs comma and bread period",
            "the meeting with the design team moved to 330 please send them the mockups",
            "monday at nine i'll be there",
            "buy milk",
        ] {
            assert_eq!(plain(s), crate::clean::clean_text(s), "{s:?}");
        }
    }

    // --- self-correction ----------------------------------------------------------------

    #[test]
    fn scratch_that_removes_the_preceding_sentence() {
        assert_eq!(plain("Buy milk. Scratch that. Buy eggs."), "Buy eggs.");
        assert_eq!(plain("buy milk period scratch that buy eggs"), "Buy eggs");
        assert_eq!(
            plain("First thing. Second thing. Scratch that."),
            "First thing."
        );
        assert_eq!(plain("Buy milk. Scratch that."), "");
        assert_eq!(plain("send the report scratch that"), "");
        assert_eq!(plain("scratch that"), "");
        assert_eq!(
            plain("Call Anna today. Delete that. Email Ben tomorrow morning."),
            "Email Ben tomorrow morning."
        );
    }

    #[test]
    fn scratch_that_inside_a_sentence_is_left_alone() {
        assert_eq!(
            plain("we should scratch that idea"),
            "We should scratch that idea."
        );
        assert_eq!(plain("I want to delete that."), "I want to delete that.");
        assert_eq!(
            plain("Please review it. Delete that file from the repo."),
            "Please review it. Delete that file from the repo."
        );
        // no clause boundary and not at the end: not a clear command
        assert_eq!(
            plain("buy milk scratch that buy eggs"),
            "Buy milk scratch that buy eggs."
        );
    }

    #[test]
    fn no_wait_replaces_the_last_words() {
        assert_eq!(plain("meet at five, no wait, six"), "Meet at six.");
        assert_eq!(
            plain("send it to John, sorry I mean Jane"),
            "Send it to Jane."
        );
        assert_eq!(plain("meet at five, I mean six"), "Meet at six.");
        assert_eq!(plain("Meet at five. No wait, six."), "Meet at six.");
        assert_eq!(
            plain("meet at five thirty, no wait, six thirty"),
            "Meet at six thirty."
        );
        assert_eq!(
            plain("the meeting is at the park, no wait, the beach"),
            "The meeting is at the beach."
        );
        // text after B is kept
        assert_eq!(
            plain("meet at five, no wait, six, and bring the report"),
            "Meet at six, and bring the report."
        );
    }

    #[test]
    fn corrections_only_fire_in_the_clear_pattern() {
        // "I mean it" is a sentence, not a correction
        assert_eq!(plain("I mean it"), crate::clean::clean_text("I mean it"));
        assert_eq!(plain("well, I mean it"), "Well, I mean it.");
        assert_eq!(plain("I think, I mean it"), "I think, I mean it.");
        // clause A too long
        assert_eq!(
            plain("please tell the team that we meet at five, no wait, six"),
            "Please tell the team that we meet at five, no wait, six."
        );
        // B longer than A: could swallow unrelated text
        assert_eq!(
            plain("meet at five, no wait, six and bring the report"),
            "Meet at five, no wait, six and bring the report."
        );
        // a number must be replaced by a number, a name by a name
        assert_eq!(
            plain("pay five dollars, no wait, Friday"),
            "Pay five dollars, no wait, Friday."
        );
        // no comma before the marker
        assert_eq!(
            plain("meet at five no wait six"),
            "Meet at five no wait six."
        );
        // a genuine question is not a correction
        assert_eq!(
            plain("do I wait? No, wait for me"),
            "Do I wait? No, wait for me."
        );
    }

    #[test]
    fn code_style_skips_self_correction() {
        assert_eq!(
            style(DictationStyle::Code, "foo scratch that"),
            "foo scratch that"
        );
    }

    // --- replacements -------------------------------------------------------------------

    #[test]
    fn replacements_are_whole_word_case_insensitive_and_verbatim() {
        let r = || vec![rep("crovy", "Krovi"), rep("tauri", "Tauri")];
        assert_eq!(
            with_reps("crovy is a great app", r()),
            "Krovi is a great app."
        );
        assert_eq!(
            with_reps("CROVY is a great app", r()),
            "Krovi is a great app."
        );
        assert_eq!(
            with_reps("we built crovy with tauri, really", r()),
            "We built Krovi with Tauri, really."
        );
        // not inside another word
        assert_eq!(
            with_reps("crovyland is far away from here", r()),
            "Crovyland is far away from here."
        );
        // possessive still matches
        assert_eq!(
            with_reps("this is crovy's best release yet", r()),
            "This is Krovi's best release yet."
        );
    }

    #[test]
    fn replacement_text_is_inserted_verbatim() {
        assert_eq!(
            with_reps("iphone is nice to use", vec![rep("iphone", "iPhone")]),
            "iPhone is nice to use."
        );
        assert_eq!(
            with_reps("hello world", vec![rep("world", "w0rld, ok")]),
            "Hello w0rld, ok"
        );
    }

    #[test]
    fn longest_phrase_wins_and_output_is_not_rematched() {
        let r = vec![
            rep("next", "NEXT"),
            rep("next js", "Next.js"),
            rep("js", "JS"),
        ];
        assert_eq!(
            with_reps("we use next js here", r.clone()),
            "We use Next.js here."
        );
        assert_eq!(with_reps("js is fun to write", r), "JS is fun to write.");
        // a -> b, b -> c must not chain
        assert_eq!(with_reps("a b", vec![rep("a", "b"), rep("b", "c")]), "b c");
    }

    #[test]
    fn replacements_handle_phrases_apostrophes_and_deletion() {
        assert_eq!(
            with_reps("open  ai   is here today", vec![rep("open ai", "OpenAI")]),
            "OpenAI is here today."
        );
        // "don" must not match inside "don't"
        assert_eq!(
            with_reps("I don't know today", vec![rep("don", "DON")]),
            "I don't know today."
        );
        assert_eq!(
            with_reps(
                "we basically need more time here",
                vec![rep("basically", "")]
            ),
            "We need more time here."
        );
    }

    #[test]
    fn invalid_replacements_are_ignored_by_process() {
        let r = vec![
            rep("", "x"),
            rep("   ", "x"),
            rep("hello", "bad\u{7}text"),
            rep("tauri", "Tauri"),
        ];
        assert_eq!(
            with_reps("tauri and hello again", r),
            "Tauri and hello again."
        );
    }

    #[test]
    fn replacements_apply_in_every_style() {
        let o = |st| DictationOptions {
            style: st,
            replacements: vec![rep("crovy", "Krovi")],
            ..DictationOptions::default()
        };
        assert_eq!(process("hi crovy", &o(DictationStyle::Message)), "Hi Krovi");
        assert_eq!(
            process("crovy dot app", &o(DictationStyle::Code)),
            "Krovi.app"
        );
        assert_eq!(
            process(
                "hi crovy new paragraph thanks Sam",
                &o(DictationStyle::Email)
            ),
            "Hi Krovi,\n\nThanks,\nSam"
        );
    }

    // --- styles -------------------------------------------------------------------------

    #[test]
    fn message_drops_the_full_stop_on_one_short_sentence() {
        assert_eq!(
            style(DictationStyle::Message, "see you at five"),
            "See you at five"
        );
        assert_eq!(
            style(DictationStyle::Message, "are you coming tonight"),
            "Are you coming tonight?"
        );
        assert_eq!(
            style(DictationStyle::Message, "sounds good. see you soon."),
            "Sounds good. See you soon."
        );
        assert_eq!(
            style(
                DictationStyle::Message,
                "i will be there at around five thirty in the evening tonight"
            ),
            "I will be there at around five thirty in the evening tonight."
        );
        assert_eq!(style(DictationStyle::Message, "great!"), "Great!");
    }

    #[test]
    fn email_has_greeting_paragraphs_and_signoff() {
        assert_eq!(
            style(
                DictationStyle::Email,
                "hi Sam new paragraph the meeting moved to 330 new paragraph thanks John"
            ),
            "Hi Sam,\n\nThe meeting moved to 3:30.\n\nThanks,\nJohn"
        );
        assert_eq!(style(DictationStyle::Email, "hi Sam"), "Hi Sam,");
        assert_eq!(
            style(
                DictationStyle::Email,
                "Hi Sam, the meeting moved to Friday. Thanks, John."
            ),
            "Hi Sam,\n\nThe meeting moved to Friday.\n\nThanks,\nJohn"
        );
        assert_eq!(
            style(
                DictationStyle::Email,
                "good morning Anna, please find the report attached. Best regards, Maria Lopez"
            ),
            "Good morning Anna,\n\nPlease find the report attached.\n\nBest regards,\nMaria Lopez"
        );
    }

    #[test]
    fn email_leaves_non_greetings_and_non_signoffs_alone() {
        // not a greeting line: too many words before the first mark
        assert_eq!(
            style(
                DictationStyle::Email,
                "hello world this is a test of the thing"
            ),
            "Hello world this is a test of the thing."
        );
        // "thanks for ..." is a sentence, not a sign-off
        assert_eq!(
            style(
                DictationStyle::Email,
                "the report is ready. Thanks for the update on the project."
            ),
            "The report is ready. Thanks for the update on the project."
        );
        // "best" opening a normal sentence
        assert_eq!(
            style(DictationStyle::Email, "the best way to do this is simple"),
            "The best way to do this is simple."
        );
        // lowercase "hey look at this," is not a greeting
        assert_eq!(
            style(DictationStyle::Email, "hey look at this, I fixed it"),
            "Hey look at this, I fixed it."
        );
    }

    #[test]
    fn note_builds_a_dash_list() {
        assert_eq!(
            style(
                DictationStyle::Note,
                "shopping list new item milk new item eggs next item bread"
            ),
            "Shopping list\n- Milk\n- Eggs\n- Bread"
        );
        assert_eq!(
            style(DictationStyle::Note, "bullet milk bullet eggs"),
            "- Milk\n- Eggs"
        );
        assert_eq!(
            style(
                DictationStyle::Note,
                "bullet point call mom, bullet point um buy gifts."
            ),
            "- Call mom\n- Buy gifts"
        );
        // nouns, not markers
        assert_eq!(
            style(
                DictationStyle::Note,
                "the next item on the agenda is budget"
            ),
            "The next item on the agenda is budget."
        );
        assert_eq!(
            style(DictationStyle::Note, "a bullet train to the city"),
            "A bullet train to the city."
        );
        assert_eq!(style(DictationStyle::Note, "bullet"), "");
    }

    #[test]
    fn code_maps_spoken_symbols() {
        let c = |s: &str| style(DictationStyle::Code, s);
        assert_eq!(
            c("foo dot bar open paren x comma y close paren"),
            "foo.bar(x, y)"
        );
        assert_eq!(c("x equals 5"), "x = 5");
        assert_eq!(c("a equals equals b"), "a == b");
        assert_eq!(c("my underscore var colon int"), "my_var: int");
        assert_eq!(c("src slash main dot rs"), "src/main.rs");
        assert_eq!(c("pre dash release"), "pre-release");
        assert_eq!(c("git commit dash dash amend"), "git commit --amend");
        assert_eq!(c("a semicolon b"), "a; b");
        assert_eq!(
            c("import os new line print open paren os dot getcwd open paren close paren close paren"),
            "import os\nprint(os.getcwd())"
        );
        assert_eq!(
            c("http colon slash slash example dot com"),
            "http://example.com"
        );
        assert_eq!(c("std colon colon fs"), "std::fs");
        assert_eq!(c("arr open bracket 0 close bracket"), "arr[0]");
        assert_eq!(
            c("fn main open paren close paren open brace close brace"),
            "fn main(){}"
        );
    }

    #[test]
    fn code_case_commands() {
        let c = |s: &str| style(DictationStyle::Code, s);
        assert_eq!(c("camel case get user name"), "getUserName");
        assert_eq!(c("snake case get user name"), "get_user_name");
        assert_eq!(
            c("let camel case my value equals snake case other thing"),
            "let myValue = other_thing"
        );
        assert_eq!(c("camel case foo open paren close paren"), "foo()");
        // nothing to convert: the words stay
        assert_eq!(c("camel case"), "camel case");
        assert_eq!(c("snake case HTTP request"), "http_request");
    }

    #[test]
    fn code_has_no_capitalisation_full_stop_or_stutter_collapse() {
        let c = |s: &str| style(DictationStyle::Code, s);
        assert_eq!(c("return value."), "return value");
        assert_eq!(c("Open paren x close paren."), "(x)");
        assert_eq!(c("foo foo bar"), "foo foo bar");
        assert_eq!(c("i equals 1"), "i = 1");
        assert_eq!(c("foo dot bar\n"), "foo.bar");
        assert_eq!(c("x semicolon new line"), "x;\n");
        // Whisper's pause commas next to words are dropped; symbols are spoken
        assert_eq!(c("open paren, x, close paren."), "(x)");
    }

    #[test]
    fn code_fillers_only_plain_hesitations() {
        let c = |s: &str| style(DictationStyle::Code, s);
        assert_eq!(c("um x equals 5"), "x = 5");
        assert_eq!(c("x equals like"), "x = like");
        assert_eq!(c("er equals 1"), "er = 1"); // "er" could be an identifier
        let keep = DictationOptions {
            style: DictationStyle::Code,
            remove_fillers: false,
            ..DictationOptions::default()
        };
        assert_eq!(process("um x", &keep), "um x");
    }

    #[test]
    fn professional_expands_unambiguous_contractions() {
        let p = |s: &str| style(DictationStyle::Professional, s);
        assert_eq!(
            p("we can't ship it because it's late"),
            "We cannot ship it because it is late."
        );
        assert_eq!(
            p("I'm gonna check and I won't be long"),
            "I am going to check and I will not be long."
        );
        assert_eq!(p("it's been a long week"), "It has been a long week.");
        assert_eq!(p("let's meet at five"), "Let us meet at five.");
        assert_eq!(p("I wanna go"), "I want to go");
        assert_eq!(
            p("they don't know and we haven't asked"),
            "They do not know and we have not asked."
        );
        // possessives and names are not contractions
        assert_eq!(p("John's idea is good"), "John's idea is good.");
        // shouted text is left alone
        assert_eq!(p("DON'T do that"), "DON'T do that");
        // ambiguous ones stay
        assert_eq!(p("he's going and I'd agree"), "He's going and I'd agree.");
    }

    #[test]
    fn casual_contracts_a_few_common_phrases() {
        let c = |s: &str| style(DictationStyle::Casual, s);
        assert_eq!(
            c("I do not know and we cannot go"),
            "I don't know and we can't go."
        );
        assert_eq!(c("I am happy"), "I'm happy");
        assert_eq!(c("it is fine today"), "It's fine today.");
        assert_eq!(c("I will not go"), "I won't go.");
        assert_eq!(c("We are going to the store"), "We're going to the store.");
        assert_eq!(c("please do not touch it"), "Please don't touch it.");
        // clause-final forms and ambiguous phrases stay
        assert_eq!(c("yes it is."), "Yes it is.");
        assert_eq!(
            c("I have a plan and I am going to the store now"),
            "I have a plan and I'm going to the store now."
        );
        assert_eq!(c("let us know if you can"), "Let us know if you can.");
    }

    // --- scripts ------------------------------------------------------------------------

    #[test]
    fn non_latin_text_passes_through() {
        assert_eq!(plain("मुझे कल मीटिंग में जाना है"), "मुझे कल मीटिंग में जाना है");
        assert_eq!(plain("  नमस्ते   दुनिया [BLANK_AUDIO] "), "नमस्ते दुनिया");
        assert_eq!(plain("你好 世界"), "你好 世界");
        assert_eq!(plain("مرحبا بالعالم"), "مرحبا بالعالم");
        // mixed script: untouched, no English rules (no "I", no capital, no full stop)
        assert_eq!(
            plain("मुझे meeting में i think um जाना है"),
            "मुझे meeting में i think um जाना है"
        );
        for st in [
            DictationStyle::Message,
            DictationStyle::Email,
            DictationStyle::Note,
            DictationStyle::Code,
            DictationStyle::Professional,
            DictationStyle::Casual,
        ] {
            assert_eq!(
                style(st, "मुझे कल जाना है new paragraph"),
                "मुझे कल जाना है new paragraph",
                "{st:?}"
            );
        }
    }

    #[test]
    fn replacements_still_apply_to_non_latin_text() {
        assert_eq!(
            with_reps("मुझे crovy पसंद है।", vec![rep("crovy", "Krovi")]),
            "मुझे Krovi पसंद है।"
        );
        assert_eq!(
            with_reps("क्रोवी बहुत अच्छा है", vec![rep("क्रोवी", "Krovi")]),
            "Krovi बहुत अच्छा है"
        );
        // a Devanagari word is not matched inside a longer word
        assert_eq!(with_reps("क्रोवीलैंड", vec![rep("क्रोवी", "Krovi")]), "क्रोवीलैंड");
    }

    #[test]
    fn romanised_hinglish_and_accents_get_latin_handling() {
        assert_eq!(
            plain("main kal office jaunga i think"),
            "Main kal office jaunga I think."
        );
        assert_eq!(plain("café at noon"), "Café at noon");
    }

    // --- options ------------------------------------------------------------------------

    #[test]
    fn fillers_can_be_kept() {
        let keep = DictationOptions {
            remove_fillers: false,
            ..DictationOptions::default()
        };
        assert_eq!(plain("um I think uh we should go"), "I think we should go.");
        assert_eq!(
            process("um I think uh we should go", &keep),
            "Um I think uh we should go."
        );
        assert_eq!(
            process("i'm going to the store, like, right now", &keep),
            "I'm going to the store, like, right now."
        );
        assert_eq!(
            process("it works, you know, mostly", &keep),
            "It works, you know, mostly."
        );
        // stutters still collapse
        assert_eq!(
            process("the the report is ready", &keep),
            "The report is ready."
        );
        // and no marker ever leaks
        assert!(!process("um uh like you know", &keep).contains(FILLER_MARK));
    }

    #[test]
    fn auto_punctuate_off_adds_no_closing_mark() {
        let off = DictationOptions {
            auto_punctuate: false,
            ..DictationOptions::default()
        };
        assert_eq!(
            process("pick up the parcel today", &off),
            "Pick up the parcel today"
        );
        assert_eq!(
            process("buy milk comma eggs period", &off),
            "Buy milk, eggs."
        );
    }

    #[test]
    fn options_round_trip_through_serde() {
        let o: DictationOptions = serde_json::from_str(r#"{"style":"email"}"#).unwrap();
        assert_eq!(o.style, DictationStyle::Email);
        assert!(o.remove_fillers && o.auto_punctuate && o.replacements.is_empty());
        let o: DictationOptions = serde_json::from_str("{}").unwrap();
        assert_eq!(o, DictationOptions::default());
        let json = serde_json::to_string(&o).unwrap();
        assert_eq!(serde_json::from_str::<DictationOptions>(&json).unwrap(), o);
    }

    // --- initial prompt -----------------------------------------------------------------

    #[test]
    fn initial_prompt_is_a_bounded_term_list() {
        let v = vec!["Krovi".to_string(), "Tauri".to_string()];
        let r = vec![rep("next js", "Next.js"), rep("crovy", "krovi")];
        assert_eq!(initial_prompt(&v, &r), "Krovi, Tauri, Next.js");
        assert_eq!(initial_prompt(&[], &[]), "");
        // never a sentence
        assert!(!initial_prompt(&v, &r).ends_with('.'));
    }

    #[test]
    fn initial_prompt_skips_junk_and_respects_the_budget() {
        let v: Vec<String> = vec![
            "  ".into(),
            "has\u{7}control".into(),
            "x".repeat(41),
            "  multi   space ".into(),
            "MULTI SPACE".into(),
        ];
        assert_eq!(initial_prompt(&v, &[]), "multi space");
        let many: Vec<String> = (0..100).map(|i| format!("term{i:03}")).collect();
        let p = initial_prompt(&many, &[]);
        assert!(p.chars().count() <= PROMPT_MAX_CHARS, "{}", p.len());
        assert!(p.starts_with("term000, term001"));
        assert!(p.chars().count() > 150, "should fill most of the budget");
        // deterministic
        assert_eq!(p, initial_prompt(&many, &[]));
    }

    // --- validation ---------------------------------------------------------------------

    #[test]
    fn validation_limits() {
        assert!(validate_replacements(&[]).is_ok());
        assert!(validate_replacements(&[rep("crovy", "Krovi"), rep("a", "")]).is_ok());
        let many: Vec<Replacement> = (0..200).map(|i| rep(&format!("w{i}"), "x")).collect();
        assert!(validate_replacements(&many).is_ok());
        let too_many: Vec<Replacement> = (0..201).map(|i| rep(&format!("w{i}"), "x")).collect();
        assert!(validate_replacements(&too_many)
            .unwrap_err()
            .contains("too many"));
        assert!(validate_replacements(&[rep("", "x")])
            .unwrap_err()
            .contains("empty"));
        assert!(validate_replacements(&[rep("   ", "x")]).is_err());
        assert!(validate_replacements(&[rep(&"a".repeat(60), "x")]).is_ok());
        assert!(validate_replacements(&[rep(&"a".repeat(61), "x")]).is_err());
        assert!(validate_replacements(&[rep("a", &"b".repeat(200))]).is_ok());
        assert!(validate_replacements(&[rep("a", &"b".repeat(201))]).is_err());
        assert!(validate_replacements(&[rep("a\nb", "x")]).is_err());
        assert!(validate_replacements(&[rep("a", "x\ty")]).is_err());
        let e = validate_replacements(&[rep("ok", "x"), rep("", "y")]).unwrap_err();
        assert!(e.contains("replacement 2"), "{e}");
    }

    // --- robustness / performance -------------------------------------------------------

    #[test]
    fn hostile_input_never_panics() {
        let styles = [
            DictationStyle::Plain,
            DictationStyle::Message,
            DictationStyle::Email,
            DictationStyle::Note,
            DictationStyle::Code,
            DictationStyle::Professional,
            DictationStyle::Casual,
        ];
        let long_token = "x".repeat(5000);
        let long_fillers = "um ".repeat(500);
        let inputs = [
            "[",
            "(",
            "♪",
            "♪♪♪",
            "]",
            ")",
            "[[",
            "((music))",
            "scratch that",
            "delete that",
            "no wait",
            "i mean",
            ", no wait,",
            "sorry i mean",
            "new paragraph",
            "new item",
            "bullet",
            "camel case",
            "snake case",
            "open paren",
            "hi",
            "thanks",
            "best regards,",
            "dear",
            "good morning",
            "\u{e000}",
            "\u{e000}um",
            "., ,",
            "...",
            "?!",
            "ǅ İ ß ΣΑΣ",
            "don't",
            "'",
            "’s",
            "a'",
            "I'm",
            long_token.as_str(),
            long_fillers.as_str(),
        ];
        let reps = vec![rep("a", "b"), rep("", "x"), rep("ß", "ss"), rep("’s", "")];
        for st in styles {
            for fillers in [true, false] {
                for auto in [true, false] {
                    let o = DictationOptions {
                        style: st,
                        replacements: reps.clone(),
                        remove_fillers: fillers,
                        auto_punctuate: auto,
                    };
                    for s in inputs {
                        let _ = process(s, &o);
                    }
                }
            }
        }
    }

    #[test]
    fn output_is_deterministic() {
        let o = DictationOptions {
            style: DictationStyle::Email,
            replacements: vec![rep("crovy", "Krovi")],
            ..DictationOptions::default()
        };
        let s =
            "hi Sam um crovy is at 330 scratch that no it's at four, no wait, five. thanks John";
        assert_eq!(process(s, &o), process(s, &o));
    }

    fn long_text(words: usize) -> String {
        let sentences = [
            "um so the the meeting with the design team moved to 330 on monday",
            "please send them the mockups and uh the updated roadmap comma thanks",
            "i think we should go with crovy for the tauri build you know",
            "buy milk and call the dentist before the weekend",
            "what time is the review with the engineering group tomorrow",
        ];
        let mut out = String::new();
        let mut n = 0;
        let mut i = 0;
        while n < words {
            let s = sentences[i % sentences.len()];
            n += s.split(' ').count();
            out.push_str(s);
            out.push(' ');
            i += 1;
        }
        out
    }

    #[test]
    fn twenty_minutes_of_speech_is_fast() {
        let text = long_text(3000);
        assert!(text.split_whitespace().count() >= 3000);
        let reps: Vec<Replacement> = (0..50)
            .map(|i| rep(&format!("term {i}"), &format!("Term{i}")))
            .chain([rep("crovy", "Krovi"), rep("tauri", "Tauri")])
            .collect();
        for st in [
            DictationStyle::Plain,
            DictationStyle::Message,
            DictationStyle::Email,
            DictationStyle::Note,
            DictationStyle::Code,
            DictationStyle::Professional,
            DictationStyle::Casual,
        ] {
            let o = DictationOptions {
                style: st,
                replacements: reps.clone(),
                remove_fillers: st != DictationStyle::Note,
                auto_punctuate: true,
            };
            let t = std::time::Instant::now();
            let out = process(&text, &o);
            let ms = t.elapsed().as_millis();
            eprintln!("3000 words, {st:?}: {ms} ms");
            assert!(!out.is_empty(), "{st:?}");
            // generous bound for slow CI machines; typical debug time is far below 100 ms
            assert!(ms < 2000, "{st:?} took {ms} ms");
        }
        // with the self-correction path forced on as well
        let corrected = format!("{text} meet at five, no wait, six. scratch that. done now");
        let t = std::time::Instant::now();
        let out = process(&corrected, &DictationOptions::default());
        assert!(!out.is_empty());
        assert!(t.elapsed().as_millis() < 3000);
    }
}
