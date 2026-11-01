//! Sentence splitting and tokenising. Pure, no I/O.
//!
//! Used twice: by the summariser (which scores sentences) and by read-aloud (which speaks one
//! sentence at a time and tells the UI which character range is being spoken). Both need the
//! same notion of "a sentence", and the same byte offsets back into the text the user sees, so
//! the splitter lives in one place.
//!
//! What it handles, because pasted text is messy:
//! * abbreviations that end in a dot ("Dr.", "U.S.", "e.g.", "Inc.", "Fig. 3", "Jan. 5"),
//! * decimals, versions, money and URLs (a dot is only a candidate boundary when whitespace
//!   follows it),
//! * initials ("J. K. Rowling"),
//! * quotes and brackets that close a sentence (`"Stop!" he said.`),
//! * ellipses, `?!`, CJK full stops,
//! * bullets and numbered lists (each item is its own unit; the marker is not part of the text),
//! * headings (short lines without end punctuation) which are kept but not summarised,
//! * hard-wrapped lines (PDF / e-mail copy) which are re-flowed into one sentence.

/// What kind of line the unit came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    Sentence,
    Bullet,
    Heading,
}

/// One sentence-like piece of the input.
#[derive(Debug, Clone)]
pub struct Unit {
    /// The text with line breaks re-flowed to single spaces (bullet marker removed).
    pub text: String,
    /// Byte offset of the first character in the ORIGINAL input.
    pub start: usize,
    /// Byte offset one past the last character in the ORIGINAL input.
    pub end: usize,
    pub kind: UnitKind,
    /// Paragraph index (blank-line separated blocks).
    pub para: usize,
    /// First unit of its paragraph.
    pub first_in_para: bool,
}

#[derive(Clone, Copy)]
struct Line<'a> {
    /// Byte offset of the trimmed text in the original input.
    start: usize,
    text: &'a str,
    /// Leading whitespace characters that were trimmed.
    indent: usize,
}

// ---------------------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------------------

/// Split `input` into units (sentences, bullets, headings) with offsets into `input`.
pub fn split_units(input: &str) -> Vec<Unit> {
    let paragraphs = paragraphs(input);
    let mut units: Vec<Unit> = Vec::new();
    for (pi, lines) in paragraphs.iter().enumerate() {
        let blocks = blocks(lines);
        let mut first = true;
        for block in blocks {
            let mut produced = block_units(&block, pi);
            for u in &mut produced {
                u.first_in_para = first;
                first = false;
            }
            units.extend(produced);
        }
    }
    units
}

/// Convenience: just the sentence texts (headings excluded).
pub fn sentences(input: &str) -> Vec<String> {
    split_units(input)
        .into_iter()
        .filter(|u| u.kind != UnitKind::Heading)
        .map(|u| u.text)
        .collect()
}

/// Number of words (whitespace separated tokens that contain a letter or digit).
pub fn word_count(text: &str) -> usize {
    text.split_whitespace()
        .filter(|t| t.chars().any(|c| c.is_alphanumeric()))
        .count()
}

/// Collapse every run of whitespace to one space and trim.
pub fn collapse_ws(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending = false;
    for c in text.chars() {
        if c.is_whitespace() || c == '\u{00ad}' || c == '\u{200b}' {
            pending = !out.is_empty();
        } else {
            if pending {
                out.push(' ');
                pending = false;
            }
            out.push(c);
        }
    }
    out
}

/// Lower-case content terms of `text` for scoring: stop words and one-letter tokens removed,
/// light suffix stemming so "agreements" and "agreement" meet.
pub fn content_terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut Vec<String>| {
        if !word.is_empty() {
            let w = std::mem::take(word);
            if w.chars().count() >= 2 && !is_stopword(&w) {
                out.push(stem(&w));
            }
        }
    };
    for c in text.chars() {
        if c.is_alphanumeric() {
            for l in c.to_lowercase() {
                word.push(l);
            }
        } else if (c == '\'' || c == '\u{2019}') && !word.is_empty() {
            // "don't" -> "dont": drop the apostrophe, keep the word whole
        } else {
            flush(&mut word, &mut out);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// A very small stemmer: enough to merge plurals and common verb endings. Not linguistically
/// complete on purpose; both sides of every comparison go through the same function.
pub fn stem(w: &str) -> String {
    let n = w.chars().count();
    if n <= 3 || w.chars().any(|c| c.is_ascii_digit()) {
        return w.to_string();
    }
    let mut s = w.to_string();
    let strip = |s: &mut String, suffix: &str, min_left: usize| -> bool {
        if s.ends_with(suffix) && s.chars().count() >= suffix.chars().count() + min_left {
            let cut = s.len() - suffix.len();
            s.truncate(cut);
            true
        } else {
            false
        }
    };
    if s.ends_with("ies") && n > 4 {
        s.truncate(s.len() - 3);
        s.push('y');
    } else if s.ends_with("sses") {
        s.truncate(s.len() - 2);
    } else if s.ends_with("ing") && n > 5 {
        strip(&mut s, "ing", 3);
        undouble(&mut s);
    } else if s.ends_with("ed") && n > 4 {
        strip(&mut s, "ed", 3);
        undouble(&mut s);
    } else if s.ends_with("ly") && n > 5 {
        strip(&mut s, "ly", 3);
    } else if s.ends_with('s') && !s.ends_with("ss") && !s.ends_with("us") && !s.ends_with("is") {
        s.pop();
    }
    s
}

fn undouble(s: &mut String) {
    let b: Vec<char> = s.chars().collect();
    if b.len() >= 2 {
        let (a, c) = (b[b.len() - 2], b[b.len() - 1]);
        if a == c && !matches!(a, 'l' | 's' | 'z' | 'a' | 'e' | 'i' | 'o' | 'u') {
            s.pop();
        }
    }
}

/// A sentence made speakable: bullet / list markers and markdown noise removed, whitespace
/// collapsed, bare URLs replaced by "link". The highlight range always refers to the
/// original text; this is only what the voice is given.
pub fn speakable(text: &str) -> String {
    let mut t = collapse_ws(text);
    // leading list marker or bullet glyph
    loop {
        let trimmed = t.trim_start();
        let cut = bullet_marker_len(trimmed);
        if cut > 0 {
            t = trimmed[cut..].trim_start().to_string();
        } else {
            t = trimmed.to_string();
            break;
        }
    }
    let t = t.replace(['*', '_', '`', '#'], "");
    let words: Vec<String> = t
        .split(' ')
        .map(|w| {
            let bare = w.trim_matches(|c: char| {
                matches!(c, '(' | ')' | '[' | ']' | '<' | '>' | ',' | ';' | '"')
            });
            let lower = bare.to_ascii_lowercase();
            if lower.starts_with("http://")
                || lower.starts_with("https://")
                || lower.starts_with("www.")
            {
                let trail: String = w
                    .chars()
                    .rev()
                    .take_while(|c| matches!(c, '.' | ',' | ')' | ';' | '!' | '?'))
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                format!("link{trail}")
            } else {
                w.to_string()
            }
        })
        .collect();
    words.join(" ")
}

/// UTF-16 length of `s[..byte_idx]` - the unit the web UI slices strings in.
pub fn utf16_offset(s: &str, byte_idx: usize) -> usize {
    let idx = byte_idx.min(s.len());
    let idx = (0..=idx)
        .rev()
        .find(|i| s.is_char_boundary(*i))
        .unwrap_or(0);
    s[..idx].encode_utf16().count()
}

// ---------------------------------------------------------------------------------------
// Lines, paragraphs, blocks
// ---------------------------------------------------------------------------------------

fn paragraphs(input: &str) -> Vec<Vec<Line<'_>>> {
    let mut paras: Vec<Vec<Line>> = Vec::new();
    let mut cur: Vec<Line> = Vec::new();
    let mut offset = 0usize;
    for raw in input.split('\n') {
        let line_off = offset;
        offset += raw.len() + 1;
        let no_cr = raw.strip_suffix('\r').unwrap_or(raw);
        let lead = no_cr.len() - no_cr.trim_start().len();
        let text = no_cr.trim();
        if text.is_empty() {
            if !cur.is_empty() {
                paras.push(std::mem::take(&mut cur));
            }
            continue;
        }
        let indent = no_cr[..lead]
            .chars()
            .map(|c| if c == '\t' { 4 } else { 1 })
            .sum();
        cur.push(Line {
            start: line_off + lead,
            text,
            indent,
        });
    }
    if !cur.is_empty() {
        paras.push(cur);
    }
    paras
}

struct Block<'a> {
    kind: UnitKind,
    lines: Vec<Line<'a>>,
}

fn blocks<'a>(lines: &[Line<'a>]) -> Vec<Block<'a>> {
    let mut out: Vec<Block<'a>> = Vec::new();
    for (k, line) in lines.iter().enumerate() {
        let marker = bullet_marker_len(line.text);
        if marker > 0 {
            let rest = line.text[marker..].trim_start();
            let skipped = line.text.len() - rest.len();
            out.push(Block {
                kind: UnitKind::Bullet,
                lines: vec![Line {
                    start: line.start + skipped,
                    text: rest,
                    indent: line.indent,
                }],
            });
            continue;
        }
        if is_heading_line(lines, k) {
            out.push(Block {
                kind: UnitKind::Heading,
                lines: vec![*line],
            });
            continue;
        }
        match out.last_mut() {
            Some(prev) if prev.kind == UnitKind::Bullet => {
                // A wrapped bullet continues on an indented or lower-case line; anything else
                // after a list is a fresh block of prose.
                let continues =
                    line.indent > 0 || line.text.chars().next().is_some_and(|c| c.is_lowercase());
                if continues {
                    prev.lines.push(*line);
                } else {
                    out.push(Block {
                        kind: UnitKind::Sentence,
                        lines: vec![*line],
                    });
                }
            }
            Some(prev) if prev.kind == UnitKind::Sentence => prev.lines.push(*line),
            _ => out.push(Block {
                kind: UnitKind::Sentence,
                lines: vec![*line],
            }),
        }
    }
    out
}

/// A short line, capitalised, with no sentence punctuation at its end, that starts its
/// paragraph and is followed by nothing or by a capitalised / bulleted line.
fn is_heading_line(lines: &[Line], k: usize) -> bool {
    let line = lines[k];
    if k != 0 {
        return false;
    }
    let t = line.text;
    if t.chars().count() > 70 || word_count(t) > 10 || word_count(t) == 0 {
        return false;
    }
    let first = t.chars().next().unwrap_or(' ');
    if !(first.is_uppercase() || first.is_ascii_digit()) {
        return false;
    }
    let last = t.chars().last().unwrap_or(' ');
    if matches!(
        last,
        '.' | '!'
            | '?'
            | '\u{2026}'
            | ','
            | ';'
            | ':'
            | '"'
            | '\''
            | '\u{201d}'
            | '\u{2019}'
            | ')'
            | ']'
    ) {
        return false;
    }
    match lines.get(1) {
        None => true,
        Some(next) => {
            bullet_marker_len(next.text) > 0
                || next
                    .text
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_uppercase() || c.is_ascii_digit())
        }
    }
}

/// Byte length of a leading list marker plus its whitespace ("- ", "* ", "• ", "12. ", "(a) ",
/// "iv) "), or 0.
pub fn bullet_marker_len(s: &str) -> usize {
    let mut it = s.char_indices();
    let Some((_, c0)) = it.next() else { return 0 };
    // glyph bullets
    if matches!(
        c0,
        '-' | '*'
            | '+'
            | '\u{2022}'
            | '\u{00b7}'
            | '\u{2013}'
            | '\u{2014}'
            | '\u{25aa}'
            | '\u{25ba}'
            | '\u{2192}'
            | '\u{2713}'
            | '\u{25cf}'
            | '\u{25e6}'
    ) {
        let rest = &s[c0.len_utf8()..];
        if rest.starts_with(|c: char| c.is_whitespace()) {
            return s.len() - rest.trim_start().len();
        }
        return 0;
    }
    // "12." / "12)" (1-3 digits)
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if (1..=3).contains(&digits) {
        let rest = &s[digits..];
        if let Some(p) = rest.chars().next() {
            if (p == '.' || p == ')') && rest[1..].starts_with(|c: char| c.is_whitespace()) {
                return s.len() - rest[1..].trim_start().len();
            }
        }
    }
    // "(a) " "(iv) " "(3) " "a) " - a single letter, a roman numeral or up to 3 digits, never
    // a word ("(see) the note" is prose).
    if c0 == '(' {
        if let Some(close) = s.find(')') {
            let inner = &s[1..close];
            let rest = &s[close + 1..];
            if is_list_label(inner) && rest.starts_with(|c: char| c.is_whitespace()) {
                return s.len() - rest.trim_start().len();
            }
        }
    } else if c0.is_ascii_lowercase() {
        let letters = s.chars().take_while(|c| c.is_ascii_lowercase()).count();
        if letters <= 4 && is_list_label(&s[..letters]) {
            let rest = &s[letters..];
            if rest.starts_with(')') && rest[1..].starts_with(|c: char| c.is_whitespace()) {
                return s.len() - rest[1..].trim_start().len();
            }
        }
    }
    0
}

fn is_list_label(inner: &str) -> bool {
    const ROMAN: &[&str] = &[
        "i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii",
    ];
    if inner.is_empty() {
        return false;
    }
    if inner.chars().all(|c| c.is_ascii_digit()) {
        return inner.len() <= 3;
    }
    let lower = inner.to_ascii_lowercase();
    (inner.chars().count() == 1 && inner.chars().all(|c| c.is_ascii_alphabetic()))
        || ROMAN.contains(&lower.as_str())
}

// ---------------------------------------------------------------------------------------
// Block -> units
// ---------------------------------------------------------------------------------------

fn block_units(block: &Block, para: usize) -> Vec<Unit> {
    // Re-flow: join the lines with one space and remember where each line sits in the
    // joined string so ranges map back to the original text.
    let mut joined = String::new();
    let mut segs: Vec<(usize, usize, usize)> = Vec::new(); // (joined_off, orig_off, len)
    for (i, l) in block.lines.iter().enumerate() {
        if i > 0 {
            // a line wrapped inside a hyphenated word keeps the hyphen but loses the space
            let wrapped_hyphen = joined.ends_with('-')
                && joined
                    .chars()
                    .rev()
                    .nth(1)
                    .is_some_and(|c| c.is_alphabetic())
                && l.text.chars().next().is_some_and(|c| c.is_lowercase());
            if !wrapped_hyphen {
                joined.push(' ');
            }
        }
        segs.push((joined.len(), l.start, l.text.len()));
        joined.push_str(l.text);
    }

    let to_orig_start = |j: usize| -> usize {
        for &(jo, oo, len) in &segs {
            if j >= jo && j < jo + len {
                return oo + (j - jo);
            }
            if j < jo {
                return oo;
            }
        }
        segs.last().map(|&(_, oo, len)| oo + len).unwrap_or(0)
    };
    let to_orig_end = |j: usize| -> usize {
        for &(jo, oo, len) in &segs {
            if j > jo && j <= jo + len {
                return oo + (j - jo);
            }
            if j <= jo {
                return oo;
            }
        }
        segs.last().map(|&(_, oo, len)| oo + len).unwrap_or(0)
    };

    if block.kind == UnitKind::Heading {
        let (s, e) = (0, joined.len());
        return vec![Unit {
            text: joined.clone(),
            start: to_orig_start(s),
            end: to_orig_end(e),
            kind: UnitKind::Heading,
            para,
            first_in_para: false,
        }];
    }

    split_ranges(&joined)
        .into_iter()
        .map(|(s, e)| Unit {
            text: joined[s..e].to_string(),
            start: to_orig_start(s),
            end: to_orig_end(e),
            kind: block.kind,
            para,
            first_in_para: false,
        })
        .collect()
}

// ---------------------------------------------------------------------------------------
// The sentence boundary decision
// ---------------------------------------------------------------------------------------

fn is_terminator(c: char) -> bool {
    matches!(
        c,
        '.' | '!' | '?' | '\u{2026}' | '\u{3002}' | '\u{ff01}' | '\u{ff1f}'
    )
}
fn is_cjk_terminator(c: char) -> bool {
    matches!(c, '\u{3002}' | '\u{ff01}' | '\u{ff1f}')
}
fn is_closer(c: char) -> bool {
    matches!(
        c,
        '"' | '\'' | '\u{201d}' | '\u{2019}' | ')' | ']' | '}' | '\u{00bb}' | '\u{300d}'
    )
}
fn is_opener(c: char) -> bool {
    matches!(
        c,
        '"' | '\'' | '\u{201c}' | '\u{2018}' | '(' | '[' | '{' | '\u{00ab}' | '\u{300c}'
    )
}

/// Titles and similar: a dot after these never ends a sentence.
const NEVER: &[&str] = &[
    "mr", "mrs", "ms", "mx", "dr", "prof", "st", "mt", "gen", "gov", "sen", "rep", "lt", "col",
    "capt", "sgt", "cmdr", "adm", "hon", "rev", "mme", "mlle", "messrs", "vs", "v", "cf", "viz",
    "ca", "esp", "incl", "approx", "est", "dept", "univ", "assn", "bros", "ave", "blvd", "e.g",
    "i.e", "et", "ph.d",
];
/// Reference words: "Fig. 3", "p. 12", "Sec. 4" - a number follows, no boundary.
const REFS: &[&str] = &[
    "no", "nos", "fig", "figs", "eq", "eqs", "sec", "secs", "art", "arts", "para", "paras", "ch",
    "vol", "vols", "pp", "p", "pg", "pt", "ex", "tab", "cl", "cls", "ss",
];
/// Abbreviations that can end a sentence: only a boundary if a plausible sentence start follows.
const SOFT: &[&str] = &[
    "inc", "ltd", "co", "corp", "llc", "plc", "jr", "sr", "al", "u.s", "u.k", "a.m", "p.m", "b.c",
    "a.d", "d.c", "u.n", "e.u",
];
const MONTHS: &[&str] = &[
    "jan", "feb", "mar", "apr", "jun", "jul", "aug", "sep", "sept", "oct", "nov", "dec",
];
/// Words that very commonly begin a sentence (checked after a soft abbreviation).
const STARTERS: &[&str] = &[
    "the",
    "a",
    "an",
    "it",
    "its",
    "this",
    "that",
    "these",
    "those",
    "we",
    "they",
    "he",
    "she",
    "i",
    "in",
    "on",
    "at",
    "for",
    "to",
    "if",
    "as",
    "but",
    "however",
    "there",
    "then",
    "when",
    "after",
    "before",
    "our",
    "their",
    "his",
    "her",
    "you",
    "your",
    "one",
    "some",
    "all",
    "many",
    "most",
    "such",
    "so",
    "thus",
    "also",
    "according",
    "today",
    "yesterday",
    "meanwhile",
    "although",
    "while",
    "since",
    "because",
    "with",
    "by",
    "from",
    "under",
    "both",
    "each",
    "no",
    "not",
    "what",
    "who",
    "how",
    "why",
    "where",
    "which",
    "my",
    "us",
    "now",
    "here",
];

fn word_before(chars: &[(usize, char)], i: usize) -> String {
    let mut j = i;
    while j > 0 {
        let c = chars[j - 1].1;
        if c.is_alphanumeric() || c == '.' || c == '\'' || c == '\u{2019}' {
            j -= 1;
        } else {
            break;
        }
    }
    chars[j..i]
        .iter()
        .map(|&(_, c)| c)
        .collect::<String>()
        .trim_start_matches('.')
        .to_lowercase()
}

fn word_after(chars: &[(usize, char)], m: usize) -> String {
    chars[m..]
        .iter()
        .map(|&(_, c)| c)
        .take_while(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// `i..=j` is the run of terminators, `m` the first non-space character after the closers.
fn is_boundary(chars: &[(usize, char)], i: usize, j: usize, m: usize) -> bool {
    let term = chars[i].1;
    let next = chars[m].1;
    if matches!(term, '?' | '!') {
        return !next.is_lowercase();
    }
    // '.' or an ellipsis
    if next.is_lowercase() {
        return false;
    }
    // A sentence can open with a capital, a digit, a quote/bracket, or a letter of a script
    // without case (CJK, Arabic, Hebrew, ...).
    let opens = next.is_uppercase() || next.is_numeric() || is_opener(next) || next.is_alphabetic();
    if j > i || term == '\u{2026}' {
        return opens;
    }
    if !opens {
        return false;
    }
    // The dot ends a URL or an e-mail address: "see https://example.com/a.b. Next" - a boundary.
    let mut s = i;
    while s > 0 && !chars[s - 1].1.is_whitespace() {
        s -= 1;
    }
    let token: String = chars[s..i]
        .iter()
        .map(|&(_, c)| c)
        .collect::<String>()
        .to_lowercase();
    if token.contains('/') || token.contains('@') || token.starts_with("www.") {
        return true;
    }
    let w = word_before(chars, i);
    if w.is_empty() {
        return true;
    }
    if NEVER.contains(&w.as_str()) {
        return false;
    }
    if REFS.contains(&w.as_str()) {
        // "No. 5", "Fig. 3": a digit follows, so it is a reference. "..is no. The" is a sentence end.
        return !next.is_numeric();
    }
    if w.chars().count() == 1 && w.chars().all(|c| c.is_alphabetic()) {
        // an initial ("J. K. Rowling") - but a lone "I" or "a" can end a sentence
        return false;
    }
    if MONTHS.contains(&w.as_str()) {
        return false;
    }
    if SOFT.contains(&w.as_str()) {
        let nw = word_after(chars, m);
        return STARTERS.contains(&nw.as_str());
    }
    if w.contains('.') && !w.chars().any(|c| c.is_ascii_digit()) {
        // other dotted abbreviations (U.S, Ph.D, a.m): split only before a typical sentence start.
        // A dotted token with digits ("v2.0.1.", "3.5.") is a number, not an abbreviation.
        let nw = word_after(chars, m);
        return STARTERS.contains(&nw.as_str());
    }
    if w == "etc" {
        return true;
    }
    true
}

/// Byte ranges (trimmed) of the sentences of one block of text.
fn split_ranges(s: &str) -> Vec<(usize, usize)> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let n = chars.len();
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut push = |start: usize, end_inclusive: usize| {
        if start > end_inclusive || start >= n {
            return;
        }
        let b0 = chars[start].0;
        let b1 = chars[end_inclusive].0 + chars[end_inclusive].1.len_utf8();
        let slice = &s[b0..b1];
        let lead = slice.len() - slice.trim_start().len();
        let t = slice.trim();
        if !t.is_empty() {
            out.push((b0 + lead, b0 + lead + t.len()));
        }
    };

    let mut start = 0usize;
    let mut i = 0usize;
    while i < n {
        let c = chars[i].1;
        if !is_terminator(c) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j + 1 < n && is_terminator(chars[j + 1].1) {
            j += 1;
        }
        let mut k = j;
        while k + 1 < n && is_closer(chars[k + 1].1) {
            k += 1;
        }
        if k + 1 >= n {
            push(start, k);
            start = n;
            break;
        }
        if is_cjk_terminator(chars[j].1) {
            let mut m = k + 1;
            while m < n && chars[m].1.is_whitespace() {
                m += 1;
            }
            push(start, k);
            start = m;
            i = m;
            continue;
        }
        if chars[k + 1].1.is_whitespace() {
            let mut m = k + 1;
            while m < n && chars[m].1.is_whitespace() {
                m += 1;
            }
            if m >= n {
                push(start, k);
                start = n;
                break;
            }
            if is_boundary(&chars, i, j, m) {
                push(start, k);
                start = m;
                i = m;
                continue;
            }
        }
        i = k + 1;
    }
    if start < n {
        push(start, n - 1);
    }
    out
}

// ---------------------------------------------------------------------------------------
// Stop words (English)
// ---------------------------------------------------------------------------------------

pub fn is_stopword(w: &str) -> bool {
    STOP.binary_search(&w).is_ok()
}

/// Sorted for binary search.
const STOP: &[&str] = &[
    "a",
    "about",
    "above",
    "after",
    "again",
    "against",
    "all",
    "also",
    "am",
    "an",
    "and",
    "any",
    "are",
    "arent",
    "as",
    "at",
    "be",
    "because",
    "been",
    "before",
    "being",
    "below",
    "between",
    "both",
    "but",
    "by",
    "can",
    "cannot",
    "cant",
    "could",
    "couldnt",
    "did",
    "didnt",
    "do",
    "does",
    "doesnt",
    "doing",
    "dont",
    "down",
    "during",
    "each",
    "either",
    "else",
    "etc",
    "even",
    "ever",
    "every",
    "few",
    "for",
    "from",
    "further",
    "had",
    "hadnt",
    "has",
    "hasnt",
    "have",
    "havent",
    "having",
    "he",
    "hed",
    "her",
    "here",
    "hers",
    "herself",
    "hes",
    "him",
    "himself",
    "his",
    "how",
    "however",
    "i",
    "id",
    "if",
    "ill",
    "im",
    "in",
    "into",
    "is",
    "isnt",
    "it",
    "its",
    "itself",
    "ive",
    "just",
    "let",
    "lets",
    "like",
    "may",
    "me",
    "might",
    "more",
    "most",
    "much",
    "must",
    "my",
    "myself",
    "neither",
    "no",
    "nor",
    "not",
    "now",
    "of",
    "off",
    "on",
    "once",
    "one",
    "only",
    "onto",
    "or",
    "other",
    "ought",
    "our",
    "ours",
    "ourselves",
    "out",
    "over",
    "own",
    "per",
    "rather",
    "said",
    "same",
    "say",
    "says",
    "shall",
    "she",
    "shed",
    "shes",
    "should",
    "shouldnt",
    "so",
    "some",
    "such",
    "than",
    "that",
    "thats",
    "the",
    "their",
    "theirs",
    "them",
    "themselves",
    "then",
    "there",
    "therefore",
    "theres",
    "these",
    "they",
    "theyd",
    "theyll",
    "theyre",
    "theyve",
    "this",
    "those",
    "through",
    "thus",
    "to",
    "too",
    "under",
    "until",
    "up",
    "upon",
    "us",
    "use",
    "used",
    "using",
    "very",
    "via",
    "was",
    "wasnt",
    "we",
    "wed",
    "well",
    "were",
    "weren",
    "werent",
    "weve",
    "what",
    "whats",
    "when",
    "where",
    "wheres",
    "whether",
    "which",
    "while",
    "who",
    "whom",
    "whose",
    "why",
    "will",
    "with",
    "within",
    "without",
    "wont",
    "would",
    "wouldnt",
    "yet",
    "you",
    "youd",
    "youll",
    "your",
    "youre",
    "yours",
    "yourself",
    "yourselves",
    "youve",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(input: &str) -> Vec<String> {
        split_units(input).into_iter().map(|u| u.text).collect()
    }

    #[test]
    fn stopword_list_is_sorted_for_binary_search() {
        let mut sorted = STOP.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, STOP, "STOP must stay sorted");
        sorted.dedup();
        assert_eq!(sorted.len(), STOP.len(), "STOP must not contain duplicates");
    }

    #[test]
    fn splits_plain_sentences() {
        let t = texts("The cat sat. The dog barked! Did it rain? Yes it did.");
        assert_eq!(
            t,
            [
                "The cat sat.",
                "The dog barked!",
                "Did it rain?",
                "Yes it did."
            ]
        );
    }

    #[test]
    fn titles_and_initials_do_not_split() {
        let t = texts(
            "Dr. Smith met Mr. Jones and Mrs. Lee at St. Mary's. J. K. Rowling was not there.",
        );
        assert_eq!(
            t,
            [
                "Dr. Smith met Mr. Jones and Mrs. Lee at St. Mary's.",
                "J. K. Rowling was not there."
            ]
        );
    }

    #[test]
    fn dotted_abbreviations_and_dates() {
        let t = texts("The U.S. economy grew on Jan. 5 at 3 p.m. while the E.U. waited. We left, e.g. for lunch. Then it rained.");
        assert_eq!(
            t,
            [
                "The U.S. economy grew on Jan. 5 at 3 p.m. while the E.U. waited.",
                "We left, e.g. for lunch.",
                "Then it rained."
            ]
        );
    }

    #[test]
    fn soft_abbreviation_splits_only_before_a_sentence_start() {
        let t = texts("It was made by Acme Inc. The company said so. Acme Inc. Announced profits.");
        assert_eq!(
            t,
            [
                "It was made by Acme Inc.",
                "The company said so.",
                "Acme Inc. Announced profits."
            ]
        );
    }

    #[test]
    fn decimals_money_versions_and_urls_stay_whole() {
        let t = texts("Prices rose 3.5 percent to $1,200.50 in v2.0.1 of the report at https://example.com/a.b. Analysts disagreed.");
        assert_eq!(
            t,
            ["Prices rose 3.5 percent to $1,200.50 in v2.0.1 of the report at https://example.com/a.b.", "Analysts disagreed."]
        );
    }

    #[test]
    fn a_sentence_may_end_in_a_version_number_or_decimal() {
        let t = texts("It shipped in v2.0.1. Analysts disagreed. Growth was 3.5. Nobody cared.");
        assert_eq!(
            t,
            [
                "It shipped in v2.0.1.",
                "Analysts disagreed.",
                "Growth was 3.5.",
                "Nobody cared."
            ]
        );
    }

    #[test]
    fn references_with_numbers_do_not_split() {
        let t = texts("See Fig. 3 and No. 5 on p. 12 for details. The answer is no. The next part starts here.");
        assert_eq!(
            t,
            [
                "See Fig. 3 and No. 5 on p. 12 for details.",
                "The answer is no.",
                "The next part starts here."
            ]
        );
    }

    #[test]
    fn quotes_and_brackets_close_the_sentence() {
        let t = texts(
            "\"Stop!\" he said. Then he left (quickly). \"Why?\" she asked. He did not answer.",
        );
        assert_eq!(
            t,
            [
                "\"Stop!\" he said.",
                "Then he left (quickly).",
                "\"Why?\" she asked.",
                "He did not answer."
            ]
        );
    }

    #[test]
    fn ellipsis_and_interrobang() {
        let t = texts("Well... that happened. Really?! Yes. And then... nothing.");
        assert_eq!(
            t,
            [
                "Well... that happened.",
                "Really?!",
                "Yes.",
                "And then... nothing."
            ]
        );
    }

    #[test]
    fn bullets_and_numbered_lists_become_units_without_markers() {
        let input = "Things to do:\n- Buy milk\n- Call the bank about the loan.\n* Fix the sink\n1. First step here\n2) Second step here\n(a) Option alpha\n(iv) Option four";
        let u = split_units(input);
        let t: Vec<&str> = u.iter().map(|u| u.text.as_str()).collect();
        assert_eq!(
            t,
            [
                "Things to do:",
                "Buy milk",
                "Call the bank about the loan.",
                "Fix the sink",
                "First step here",
                "Second step here",
                "Option alpha",
                "Option four"
            ]
        );
        assert_eq!(u[1].kind, UnitKind::Bullet);
        assert_eq!(&input[u[1].start..u[1].end], "Buy milk");
    }

    #[test]
    fn minus_sign_and_hyphenated_numbers_are_not_bullets() {
        let t = texts("-5 degrees is cold. 3-4 weeks is long.");
        assert_eq!(t, ["-5 degrees is cold.", "3-4 weeks is long."]);
    }

    #[test]
    fn headings_are_marked_and_kept() {
        let input = "Terms and Conditions\n\nPlease read this carefully before use. It matters.\n\nLIMITATION OF LIABILITY\nIn no event shall we be liable.";
        let u = split_units(input);
        assert_eq!(u[0].kind, UnitKind::Heading);
        assert_eq!(u[0].text, "Terms and Conditions");
        assert_eq!(u[3].kind, UnitKind::Heading);
        assert_eq!(u[4].text, "In no event shall we be liable.");
    }

    #[test]
    fn hard_wrapped_lines_are_reflowed_into_one_sentence() {
        let input = "This sentence was copied from a PDF and wraps\nacross three lines because the page was narrow\nand it ends here. Next one.";
        let u = split_units(input);
        assert_eq!(u.len(), 2);
        assert_eq!(
            u[0].text,
            "This sentence was copied from a PDF and wraps across three lines because the page was narrow and it ends here."
        );
        // offsets still point at the original characters, newlines included
        assert!(input[u[0].start..u[0].end].starts_with("This sentence"));
        assert!(input[u[0].start..u[0].end].ends_with("ends here."));
    }

    #[test]
    fn offsets_slice_back_to_the_sentence() {
        let input = "First sentence here.  Second one follows!\r\n\r\nA new paragraph starts. It has two sentences.";
        for u in split_units(input) {
            let slice = &input[u.start..u.end];
            assert_eq!(
                collapse_ws(slice),
                collapse_ws(&u.text),
                "slice {slice:?} vs {:?}",
                u.text
            );
        }
        let u = split_units(input);
        assert_eq!(u.len(), 4);
        assert_eq!(u[2].para, 1);
        assert!(u[2].first_in_para && !u[3].first_in_para);
    }

    #[test]
    fn unicode_offsets_are_char_boundaries() {
        let input = "Caf\u{e9} s'ouvre. \u{4eca}\u{65e5}\u{306f}\u{6674}\u{308c}\u{3067}\u{3059}\u{3002}\u{660e}\u{65e5}\u{3082}\u{3067}\u{3059}\u{3002} Done \u{1f600}!";
        let u = split_units(input);
        assert_eq!(
            u.len(),
            4,
            "{:?}",
            u.iter().map(|u| &u.text).collect::<Vec<_>>()
        );
        for x in &u {
            assert!(input.is_char_boundary(x.start) && input.is_char_boundary(x.end));
            assert_eq!(&input[x.start..x.end], x.text);
        }
    }

    #[test]
    fn utf16_offsets_count_surrogate_pairs() {
        let s = "a\u{1f600}b";
        assert_eq!(utf16_offset(s, 0), 0);
        assert_eq!(utf16_offset(s, 1), 1);
        assert_eq!(utf16_offset(s, 5), 3); // after the emoji: 1 + 2 UTF-16 units
        assert_eq!(utf16_offset(s, 99), 4);
    }

    #[test]
    fn empty_and_whitespace_input() {
        assert!(split_units("").is_empty());
        assert!(split_units("  \n\t \r\n ").is_empty());
        assert_eq!(texts("no punctuation at all"), ["no punctuation at all"]);
    }

    #[test]
    fn word_count_ignores_symbols() {
        assert_eq!(word_count("one two  three - \u{2014} 4 % x"), 5);
        assert_eq!(word_count(""), 0);
    }

    #[test]
    fn content_terms_drop_stopwords_and_merge_variants() {
        let t = content_terms("The agreements and the agreement; they're running, it ran.");
        assert!(!t.iter().any(|w| w == "the" || w == "and" || w == "theyre"));
        assert_eq!(
            t.iter().filter(|w| w.as_str() == "agreement").count(),
            2,
            "{t:?}"
        );
        assert!(t.contains(&"run".to_string()), "{t:?}");
    }

    #[test]
    fn speakable_strips_markers_markdown_and_urls() {
        assert_eq!(speakable("- Buy   milk"), "Buy milk");
        assert_eq!(speakable("\u{2022} **Bold** point."), "Bold point.");
        assert_eq!(
            speakable("See https://example.com/x, then stop."),
            "See link, then stop."
        );
        assert_eq!(speakable("1. First"), "First");
    }

    #[test]
    fn bullet_markers() {
        assert_eq!(bullet_marker_len("- x"), 2);
        assert_eq!(bullet_marker_len("\u{2022}   x"), 6);
        assert_eq!(bullet_marker_len("12. x"), 4);
        assert_eq!(bullet_marker_len("(a) x"), 4);
        assert_eq!(bullet_marker_len("b) x"), 3);
        assert_eq!(bullet_marker_len("-x"), 0);
        assert_eq!(bullet_marker_len("2024 was good"), 0);
        assert_eq!(bullet_marker_len("1234. too many digits"), 0);
        assert_eq!(bullet_marker_len("plain"), 0);
    }
}
