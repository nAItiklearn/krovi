//! Text plumbing for the intent parser: a tokenizer that keeps byte offsets (so a note or a search
//! query is taken verbatim from the user's own words), payload cleaning, spoken numbers and
//! durations ("ten minutes", "an hour and a half", "1h30m").

/// One whitespace-delimited word of the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tok {
    /// The word with edge punctuation removed, original case ("sam@x.com", "https://a.b/c").
    pub txt: String,
    /// `txt` lower-cased: what the matchers look at.
    pub low: String,
    /// Byte range of the whole word (punctuation included) in the original input.
    pub start: usize,
    pub end: usize,
    /// The word ended with `;` (a hard step separator).
    pub semi: bool,
}

fn is_edge(c: char) -> bool {
    matches!(
        c,
        ',' | '.'
            | ';'
            | ':'
            | '!'
            | '?'
            | '"'
            | '\''
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '<'
            | '>'
            | '`'
            | '\u{201c}'
            | '\u{201d}'
            | '\u{2018}'
            | '\u{2019}'
    )
}

/// Trim edge punctuation, but keep a closing bracket that closes one inside the word, so
/// `alert(1)` and `Rust_(programming_language)` survive.
fn trim_edges(word: &str) -> &str {
    let mut s = word.trim_start_matches(is_edge);
    while let Some(last) = s.chars().next_back() {
        if !is_edge(last) {
            break;
        }
        let opener = match last {
            ')' => Some('('),
            ']' => Some('['),
            '}' => Some('{'),
            _ => None,
        };
        if let Some(open) = opener {
            let opens = s.matches(open).count();
            let closes = s.matches(last).count();
            if opens >= closes {
                break; // balanced: this bracket belongs to the word
            }
        }
        s = &s[..s.len() - last.len_utf8()];
    }
    s
}

fn push_tok(out: &mut Vec<Tok>, input: &str, start: usize, end: usize) {
    let word = &input[start..end];
    let txt = trim_edges(word);
    if txt.is_empty() || txt.chars().all(|c| c.is_ascii_punctuation()) {
        return; // punctuation only: stays inside payload slices, invisible to the matchers
    }
    out.push(Tok {
        txt: txt.to_string(),
        low: txt.to_lowercase(),
        start,
        end,
        semi: word.ends_with(';'),
    });
}

/// Split `input` into words, remembering where each one sits.
pub fn tokenize(input: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in input.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                push_tok(&mut out, input, s, i);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        push_tok(&mut out, input, s, input.len());
    }
    out
}

/// The lower-cased words of a token slice.
pub fn lows(toks: &[Tok]) -> Vec<&str> {
    toks.iter().map(|t| t.low.as_str()).collect()
}

fn unquote(t: &str) -> Option<&str> {
    for (a, b) in [
        ('"', '"'),
        ('\'', '\''),
        ('\u{201c}', '\u{201d}'),
        ('\u{2018}', '\u{2019}'),
    ] {
        if t.len() >= a.len_utf8() + b.len_utf8() && t.starts_with(a) && t.ends_with(b) {
            return Some(&t[a.len_utf8()..t.len() - b.len_utf8()]);
        }
    }
    None
}

/// Tidy free text taken from the user's words: collapse whitespace, drop a leading `:`/`-`, a
/// trailing `.` `,` `;` `:` and one pair of surrounding quotes. Case and inner punctuation are
/// untouched.
pub fn clean_payload(s: &str) -> String {
    let joined = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut t: &str = joined.trim_matches(|c: char| matches!(c, ':' | ',' | ';' | '-' | ' '));
    loop {
        let before = t;
        t = t.trim_end_matches(['.', ',', ';', ':', ' ']);
        if let Some(inner) = unquote(t) {
            t = inner.trim();
        }
        if t == before {
            break;
        }
    }
    t.to_string()
}

/// Upper-case the first character ("call mom" -> "Call mom").
pub fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Truncate to `max` characters, ending with `...` when something was cut.
pub fn preview(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(3)).collect();
    format!("{}...", cut.trim_end())
}

// ---------------------------------------------------------------------------------------------
// numbers
// ---------------------------------------------------------------------------------------------

const ONES: [&str; 20] = [
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
];

fn ones(w: &str) -> Option<u64> {
    ONES.iter().position(|x| *x == w).map(|p| p as u64)
}

fn tens(w: &str) -> Option<u64> {
    Some(match w {
        "twenty" => 20,
        "thirty" => 30,
        "forty" | "fourty" => 40,
        "fifty" => 50,
        "sixty" => 60,
        "seventy" => 70,
        "eighty" => 80,
        "ninety" => 90,
        _ => return None,
    })
}

/// A whole number at the start of `w`: digits ("10"), words ("ten", "twenty five",
/// "twenty-five", "one hundred and five"). Returns the value and how many words it used.
pub fn parse_int(w: &[&str]) -> Option<(u64, usize)> {
    let first = *w.first()?;
    if !first.is_empty() && first.len() <= 7 && first.bytes().all(|b| b.is_ascii_digit()) {
        return Some((first.parse().ok()?, 1));
    }
    if let Some((a, b)) = first.split_once('-') {
        if let (Some(t), Some(o)) = (tens(a), ones(b).filter(|o| (1..10).contains(o))) {
            return Some((t + o, 1));
        }
    }
    let mut i = 0usize;
    let mut total = 0u64;
    let mut matched = false;
    if w.get(1) == Some(&"hundred") {
        if let Some(n) = ones(w[0]).filter(|n| (1..10).contains(n)) {
            total = n * 100;
            i = 2;
            matched = true;
            let next_is_number = w
                .get(i + 1)
                .is_some_and(|x| tens(x).is_some() || ones(x).is_some());
            if w.get(i) == Some(&"and") && next_is_number {
                i += 1;
            }
        }
    }
    if let Some(t) = w.get(i).and_then(|x| tens(x)) {
        total += t;
        i += 1;
        matched = true;
        if let Some(o) = w
            .get(i)
            .and_then(|x| ones(x))
            .filter(|o| (1..10).contains(o))
        {
            total += o;
            i += 1;
        }
    } else if let Some(v) = w.get(i).and_then(|x| ones(x)) {
        total += v;
        i += 1;
        matched = true;
    }
    matched.then_some((total, i))
}

/// "1.5", "0.25" (one token, exactly one dot).
fn parse_decimal(tok: &str) -> Option<f64> {
    let (a, b) = tok.split_once('.')?;
    let digits = |s: &str| !s.is_empty() && s.len() <= 6 && s.bytes().all(|c| c.is_ascii_digit());
    if digits(a) && digits(b) {
        tok.parse().ok()
    } else {
        None
    }
}

// ---------------------------------------------------------------------------------------------
// durations
// ---------------------------------------------------------------------------------------------

/// Seconds in one named unit.
pub fn unit_secs(w: &str) -> Option<f64> {
    Some(match w {
        "second" | "seconds" | "sec" | "secs" | "s" => 1.0,
        "minute" | "minutes" | "min" | "mins" | "m" => 60.0,
        "hour" | "hours" | "hr" | "hrs" | "h" => 3600.0,
        "day" | "days" => 86_400.0,
        "week" | "weeks" => 604_800.0,
        _ => return None,
    })
}

/// "10min", "90s", "1.5h", "1h30m": digits and unit letters in one word.
fn parse_fused(tok: &str) -> Option<(f64, f64)> {
    let b = tok.as_bytes();
    let (mut i, mut total, mut last, mut pairs) = (0usize, 0.0f64, 0.0f64, 0u32);
    while i < b.len() {
        let s = i;
        while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
            i += 1;
        }
        if i == s {
            return None;
        }
        let num: f64 = tok[s..i].parse().ok()?;
        let us = i;
        while i < b.len() && b[i].is_ascii_alphabetic() {
            i += 1;
        }
        if i == us {
            return None;
        }
        let unit = unit_secs(&tok[us..i])?;
        total += num * unit;
        last = unit;
        pairs += 1;
    }
    (pairs > 0).then_some((total, last))
}

/// One duration term: (seconds, words used, the unit's seconds).
fn term(w: &[&str]) -> Option<(f64, usize, f64)> {
    let t0 = *w.first()?;
    if let Some((secs, unit)) = parse_fused(t0) {
        return Some((secs, 1, unit));
    }
    if t0 == "half" {
        let mut k = 1;
        if matches!(w.get(k), Some(&"a") | Some(&"an")) {
            k += 1;
        }
        let u = unit_secs(w.get(k)?)?;
        return Some((u / 2.0, k + 1, u));
    }
    if (t0 == "a" || t0 == "an") && w.get(1) == Some(&"half") {
        let u = unit_secs(w.get(2)?)?;
        return Some((u / 2.0, 3, u));
    }
    if t0 == "quarter" && w.get(1) == Some(&"of") && matches!(w.get(2), Some(&"an") | Some(&"a")) {
        let u = unit_secs(w.get(3)?)?;
        return Some((u / 4.0, 4, u));
    }
    let (mut qty, mut k) = if t0 == "a" || t0 == "an" {
        (1.0, 1)
    } else if let Some(d) = parse_decimal(t0) {
        (d, 1)
    } else {
        let (n, c) = parse_int(w)?;
        (n as f64, c)
    };
    // "one and a half hours"
    if w.get(k) == Some(&"and")
        && w.get(k + 1) == Some(&"a")
        && w.get(k + 2) == Some(&"half")
        && w.get(k + 3).and_then(|u| unit_secs(u)).is_some()
    {
        qty += 0.5;
        k += 3;
    }
    let u = unit_secs(w.get(k)?)?;
    Some((qty * u, k + 1, u))
}

/// A spoken or typed duration at the start of `w`, greedily: "10 minutes", "an hour and a half",
/// "1 hour 30 minutes", "half an hour", "90s", "1h30m". Returns whole seconds (rounded) and the
/// number of words used. Values are clamped to ten years so later arithmetic cannot overflow.
pub fn parse_duration(w: &[&str]) -> Option<(u64, usize)> {
    const CAP: f64 = 315_360_000.0;
    let (mut total, mut i, mut last_unit, mut terms) = (0.0f64, 0usize, 0.0f64, 0u32);
    loop {
        if terms > 0
            && last_unit > 0.0
            && w.get(i) == Some(&"and")
            && w.get(i + 1) == Some(&"a")
            && w.get(i + 2) == Some(&"half")
        {
            total += last_unit / 2.0;
            i += 3;
            last_unit = 0.0;
            continue;
        }
        let j = if terms > 0 && w.get(i) == Some(&"and") {
            i + 1
        } else {
            i
        };
        match term(&w[j.min(w.len())..]) {
            Some((secs, used, unit)) => {
                total += secs;
                i = j + used;
                last_unit = unit;
                terms += 1;
            }
            None => break,
        }
    }
    if terms == 0 {
        return None;
    }
    Some((total.round().min(CAP) as u64, i))
}

/// "10 minutes", "1 hour 30 minutes", "90 seconds" -> "1 minute 30 seconds".
pub fn fmt_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let part = |n: u64, unit: &str| format!("{n} {unit}{}", if n == 1 { "" } else { "s" });
    let mut parts = Vec::new();
    if h > 0 {
        parts.push(part(h, "hour"));
    }
    if m > 0 {
        parts.push(part(m, "minute"));
    }
    if s > 0 || parts.is_empty() {
        parts.push(part(s, "second"));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(|w| w.to_lowercase()).collect()
    }
    fn dur(s: &str) -> Option<(u64, usize)> {
        let w = words(s);
        let r: Vec<&str> = w.iter().map(String::as_str).collect();
        parse_duration(&r)
    }
    fn int(s: &str) -> Option<(u64, usize)> {
        let w = words(s);
        let r: Vec<&str> = w.iter().map(String::as_str).collect();
        parse_int(&r)
    }

    #[test]
    fn tokenizer_keeps_offsets_and_trims_edges() {
        let input = "  Take a note:  buy milk, please! ";
        let t = tokenize(input);
        let txt: Vec<&str> = t.iter().map(|x| x.txt.as_str()).collect();
        assert_eq!(txt, ["Take", "a", "note", "buy", "milk", "please"]);
        assert_eq!(t[3].low, "buy");
        assert_eq!(&input[t[3].start..t[4].end], "buy milk,");
    }

    #[test]
    fn tokenizer_drops_punctuation_only_words_but_slices_keep_them() {
        for input in ["note a - b", "note a : b", "note a & b", "note a -- b"] {
            let t = tokenize(input);
            assert_eq!(t.len(), 3, "{input}");
            assert_eq!(&input[t[1].start..t[2].end], &input[5..], "{input}");
        }
        // words that merely contain punctuation, or are emoji, stay
        assert_eq!(tokenize("c++ e-mail \u{1f600}").len(), 3);
    }

    #[test]
    fn closing_brackets_that_belong_to_the_word_are_kept() {
        let t = tokenize("go to javascript:alert(1) (https://a.b/c) https://en.wikipedia.org/wiki/Rust_(language). [x]");
        let txt: Vec<&str> = t.iter().map(|x| x.txt.as_str()).collect();
        assert_eq!(
            txt,
            [
                "go",
                "to",
                "javascript:alert(1)",
                "https://a.b/c",
                "https://en.wikipedia.org/wiki/Rust_(language)",
                "x"
            ]
        );
    }

    #[test]
    fn tokenizer_marks_semicolons_and_keeps_urls_whole() {
        let t = tokenize("open youtube; open (https://a.b/c?x=1&y=2).");
        assert!(t[1].semi);
        assert_eq!(t.last().unwrap().txt, "https://a.b/c?x=1&y=2");
        assert!(tokenize("   ").is_empty());
        assert!(tokenize("").is_empty());
    }

    #[test]
    fn tokenizer_handles_multibyte_text() {
        let input = "note caf\u{e9} \u{2014} cr\u{e8}me br\u{fb}l\u{e9}e";
        let t = tokenize(input);
        assert_eq!(
            &input[t[1].start..t.last().unwrap().end],
            "caf\u{e9} \u{2014} cr\u{e8}me br\u{fb}l\u{e9}e"
        );
    }

    #[test]
    fn clean_payload_rules() {
        for (raw, want) in [
            ("  buy   milk. ", "buy milk"),
            (": buy milk", "buy milk"),
            ("\"buy milk\".", "buy milk"),
            ("\u{201c}buy milk\u{201d}", "buy milk"),
            ("it's 5:30 now", "it's 5:30 now"),
            ("a; b;", "a; b"),
            ("  ", ""),
            ("...", ""),
            ("Fix the O'Brien thing!", "Fix the O'Brien thing!"),
        ] {
            assert_eq!(clean_payload(raw), want, "{raw:?}");
        }
    }

    #[test]
    fn capitalize_and_preview() {
        assert_eq!(capitalize("call mom"), "Call mom");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("\u{e9}clair"), "\u{c9}clair");
        assert_eq!(preview("short", 10), "short");
        assert_eq!(preview("abcdefghijkl", 8), "abcde...");
    }

    #[test]
    fn integers_digits_and_words() {
        for (s, want) in [
            ("10 minutes", (10, 1)),
            ("ten minutes", (10, 1)),
            ("twenty five minutes", (25, 2)),
            ("twenty-five minutes", (25, 1)),
            ("nineteen", (19, 1)),
            ("one hundred and five secs", (105, 4)),
            ("two hundred", (200, 2)),
            ("ninety", (90, 1)),
            ("zero", (0, 1)),
            ("forty two", (42, 2)),
        ] {
            assert_eq!(int(s), Some(want), "{s}");
        }
        for bad in ["minutes", "", "twenty-", "foo", "99999999 x"] {
            assert_eq!(int(bad), None, "{bad}");
        }
    }

    #[test]
    fn durations_table() {
        for (s, want) in [
            ("10 minutes", (600, 2)),
            ("1 minute", (60, 2)),
            ("90 seconds", (90, 2)),
            ("a minute", (60, 2)),
            ("an hour", (3600, 2)),
            ("2 hours", (7200, 2)),
            ("ten min", (600, 2)),
            ("twenty five minutes", (1500, 3)),
            ("half an hour", (1800, 3)),
            ("half hour", (1800, 2)),
            ("a half hour", (1800, 3)),
            ("quarter of an hour", (900, 4)),
            ("an hour and a half", (5400, 5)),
            ("one and a half hours", (5400, 5)),
            ("1.5 hours", (5400, 2)),
            ("1 hour 30 minutes", (5400, 4)),
            ("1 hour and 30 minutes", (5400, 5)),
            ("2 hours 15 mins", (8100, 4)),
            ("10min", (600, 1)),
            ("90s", (90, 1)),
            ("1h30m", (5400, 1)),
            ("2h", (7200, 1)),
            ("1.5h", (5400, 1)),
            ("3 days", (259_200, 2)),
            ("a week", (604_800, 2)),
            ("5 minutes and then", (300, 2)),
            ("5 minutes to stretch", (300, 2)),
            ("30 seconds called tea", (30, 2)),
        ] {
            assert_eq!(dur(s), Some(want), "{s}");
        }
    }

    #[test]
    fn durations_negative_cases() {
        for bad in [
            "",
            "minutes",
            "ten",
            "10",
            "soon",
            "a thing",
            "10 apples",
            "5pm",
            "half",
            "and a half",
            "-5 minutes",
        ] {
            assert_eq!(dur(bad), None, "{bad}");
        }
    }

    #[test]
    fn duration_is_clamped_not_overflowing() {
        let (secs, _) = dur("9999999 weeks").unwrap();
        assert_eq!(secs, 315_360_000);
    }

    #[test]
    fn duration_formatting() {
        assert_eq!(fmt_duration(600), "10 minutes");
        assert_eq!(fmt_duration(60), "1 minute");
        assert_eq!(fmt_duration(90), "1 minute 30 seconds");
        assert_eq!(fmt_duration(5400), "1 hour 30 minutes");
        assert_eq!(fmt_duration(3600), "1 hour");
        assert_eq!(fmt_duration(1), "1 second");
        assert_eq!(fmt_duration(86_400), "24 hours");
        assert_eq!(fmt_duration(0), "0 seconds");
    }
}
