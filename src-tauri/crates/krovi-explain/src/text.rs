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

/// Writing systems that put no spaces between words. `1` = Han ideographs and Japanese kana
/// (about two characters per word), `2` = Thai / Lao / Myanmar / Khmer (about four), `0` = every
/// other character. Without this a whole Chinese paragraph counts as ONE word.
pub fn spaceless_class(c: char) -> u8 {
    match c as u32 {
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF66..=0xFF9F => {
            1
        }
        0x0E00..=0x0EFF | 0x1000..=0x109F | 0x1780..=0x17FF => 2,
        _ => 0,
    }
}

/// Number of words: whitespace separated tokens that contain a letter or digit, where a run of
/// space-less script counts as one word per ~2 characters (Han / kana) or ~4 (Thai and friends).
pub fn word_count(text: &str) -> usize {
    let mut n = 0usize;
    for tok in text.split_whitespace() {
        let (mut han, mut thai, mut other) = (0usize, 0usize, false);
        for c in tok.chars() {
            match spaceless_class(c) {
                1 => han += 1,
                2 => thai += 1,
                _ => {
                    if c.is_alphanumeric() {
                        other = true;
                    }
                }
            }
        }
        n += usize::from(other) + han.div_ceil(2) + thai.div_ceil(4);
    }
    n
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
///
/// Space-less scripts (Chinese, Japanese, Thai, ...) have no word boundaries to split on, so a
/// run of them becomes overlapping character bigrams (trigrams for Thai and friends): the usual
/// dictionary-free way to compare such sentences.
pub fn content_terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut run: Vec<char> = Vec::new();
    let mut run_class = 0u8;
    let flush = |word: &mut String, out: &mut Vec<String>| {
        if !word.is_empty() {
            let w = std::mem::take(word);
            if w.chars().count() >= 2 && !is_stopword(&w) {
                out.push(stem(&w));
            }
        }
    };
    for c in text.chars() {
        let class = spaceless_class(c);
        if class != 0 {
            flush(&mut word, &mut out);
            if class != run_class {
                flush_run(&mut run, run_class, &mut out);
                run_class = class;
            }
            run.push(c);
            continue;
        }
        flush_run(&mut run, run_class, &mut out);
        run_class = 0;
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
    flush_run(&mut run, run_class, &mut out);
    out
}

/// Emit the terms of one run of space-less script (see [`content_terms`]).
fn flush_run(run: &mut Vec<char>, class: u8, out: &mut Vec<String>) {
    if run.is_empty() {
        return;
    }
    let n = if class == 2 { 3 } else { 2 };
    if run.len() < n {
        // a short run is one term: a single ideograph is often a whole word
        out.push(run.iter().collect());
    } else {
        for w in run.windows(n) {
            out.push(w.iter().collect());
        }
    }
    run.clear();
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

/// Markdown noise removed for display: emphasis and code markers, leading heading hashes,
/// table separator rows dropped, table pipes turned into separators. The original text and
/// every offset into it are untouched; this is only what a summary shows.
pub fn display_clean(s: &str) -> String {
    let mut t = collapse_ws(s);
    // A pure table separator row ("| --- | :---: |") carries nothing: drop it.
    let trimmed = t.trim().to_string();
    if trimmed.starts_with('|')
        && trimmed
            .chars()
            .all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
        && trimmed.contains('-')
    {
        return String::new();
    }
    // A content table row reads better without its outer pipes.
    if trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.chars().count() > 1 {
        let inner = trimmed[..trimmed.len() - 1].trim_start_matches('|').trim();
        t = inner.replace(" | ", " \u{b7} ");
    }
    // Leading heading hashes (a heading that slipped through as prose).
    loop {
        let cut = atx_marker_len(&t);
        if cut > 0 {
            t = t[cut..].to_string();
        } else {
            break;
        }
    }
    // Emphasis and code markers. Interior underscores survive (file_name.rs stays whole,
    // 5*3 stays math); a marker at a word edge is emphasis and goes.
    let chars: Vec<char> = t.chars().collect();
    let mut out = String::with_capacity(t.len());
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '`' => {}
            '*' | '_' => {
                let prev = if i == 0 { None } else { Some(chars[i - 1]) };
                let next = chars.get(i + 1).copied();
                let at_edge = |x: Option<char>| x.is_none_or(|p| !p.is_alphanumeric());
                if !at_edge(prev) && !at_edge(next) {
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
    }
    collapse_ws(&out)
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
        // Markdown table rows are their own units; re-flowing them with neighbouring
        // prose would glue several rows into one "sentence".
        if line.text.starts_with('|') {
            out.push(Block {
                kind: UnitKind::Sentence,
                lines: vec![*line],
            });
            continue;
        }
        // Markdown ATX headings ("## Title") are headings wherever they appear, not only at
        // paragraph starts; a `#`-line would otherwise be reflowed into the next sentence.
        let atx = atx_marker_len(line.text);
        if atx > 0 {
            let rest = line.text[atx..].trim_start();
            let skipped = line.text.len() - rest.len();
            out.push(Block {
                kind: UnitKind::Heading,
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
    if !heading_starter(first) {
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
            | '\u{3002}'
            | '\u{ff01}'
            | '\u{ff1f}'
            | '\u{ff0c}'
            | '\u{3001}'
            | '\u{ff1a}'
            | '\u{ff1b}'
    ) {
        return false;
    }
    match lines.get(1) {
        None => true,
        Some(next) => {
            bullet_marker_len(next.text) > 0
                || next.text.chars().next().is_some_and(heading_starter)
        }
    }
}

/// A heading starts with a capital, a digit, or a Han / kana character (those have no case).
fn heading_starter(c: char) -> bool {
    c.is_uppercase() || c.is_ascii_digit() || spaceless_class(c) == 1
}

/// Byte length of a Markdown ATX heading marker ("##" plus the required space), or 0.
/// "##Title" (no space) is not a heading, matching CommonMark.
fn atx_marker_len(s: &str) -> usize {
    let hashes = s.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) {
        let rest = &s[hashes..];
        if rest.starts_with(|c: char| c.is_whitespace()) {
            return s.len() - rest.trim_start().len();
        }
    }
    0
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
        '.' | '!'
            | '?'
            | '\u{2026}'
            | '\u{3002}'
            | '\u{ff01}'
            | '\u{ff1f}'
            | '\u{0964}'
            | '\u{0965}'
    )
}
fn is_cjk_terminator(c: char) -> bool {
    matches!(c, '\u{3002}' | '\u{ff01}' | '\u{ff1f}')
}
/// Devanagari danda (`\u{0964}`) and double danda (`\u{0965}`): unconditional sentence ends,
/// like the CJK full stop. No case or abbreviation logic applies to them.
fn is_danda(c: char) -> bool {
    matches!(c, '\u{0964}' | '\u{0965}')
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
        if is_cjk_terminator(chars[j].1) || is_danda(chars[j].1) {
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
// Cue detection: action items, decisions, dates, open questions
//
// Plain word-sequence matching on a lower-cased token stream. No model, no regex engine, and the
// TypeScript port (src/lib/explain.ts) follows the same rules step for step.
// ---------------------------------------------------------------------------------------

/// One word of a sentence: lower-cased, with its byte span in the sentence. Dots, dashes, colons,
/// slashes and apostrophes BETWEEN alphanumerics stay inside the token (`2026-11-03`, `10:30`,
/// `follow-up`, `don't`, `p.m`).
#[derive(Debug, Clone)]
pub struct Tok {
    pub lc: String,
    pub start: usize,
    pub end: usize,
    /// Starts with a capital letter (or follows an `@`): a name, a title or a sentence start.
    pub cap: bool,
}

pub fn tokens(s: &str) -> Vec<Tok> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < n {
        if !chars[i].1.is_alphanumeric() {
            i += 1;
            continue;
        }
        let st = i;
        let mut j = i + 1;
        while j < n {
            let c = chars[j].1;
            if c.is_alphanumeric() {
                j += 1;
            } else if matches!(c, '-' | '/' | ':' | '.' | '\'' | '\u{2019}')
                && j + 1 < n
                && chars[j + 1].1.is_alphanumeric()
            {
                j += 1;
            } else {
                break;
            }
        }
        let start = chars[st].0;
        let end = if j < n { chars[j].0 } else { s.len() };
        let raw = &s[start..end];
        let lc: String = raw
            .chars()
            .flat_map(|c| c.to_lowercase())
            .map(|c| if c == '\u{2019}' { '\'' } else { c })
            .collect();
        let first = raw.chars().next().unwrap_or(' ');
        let after_at = st > 0 && chars[st - 1].1 == '@';
        out.push(Tok {
            lc,
            start,
            end,
            cap: first.is_uppercase() || after_at,
        });
        i = j;
    }
    out
}

fn is_seq(t: &[Tok], i: usize, words: &[&str]) -> bool {
    i + words.len() <= t.len() && words.iter().enumerate().all(|(k, w)| t[i + k].lc == *w)
}

fn tok_at(t: &[Tok], i: usize) -> &str {
    t.get(i).map_or("", |x| x.lc.as_str())
}

fn phrase_of(s: &str, t: &[Tok], a: usize, b: usize) -> String {
    s[t[a].start..t[b - 1].end].to_string()
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `true` when the sentence ends in a question mark (closing quotes / brackets ignored).
pub fn ends_with_question(s: &str) -> bool {
    let t = s.trim_end().trim_end_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | '\u{201d}' | '\u{2019}' | ')' | ']' | '}' | '\u{00bb}' | '\u{300d}'
        )
    });
    t.ends_with('?') || t.ends_with('\u{ff1f}')
}

// ---- dates -------------------------------------------------------------------------------

fn month_of(w: &str) -> Option<u8> {
    const M: &[(&str, u8)] = &[
        ("january", 1),
        ("jan", 1),
        ("february", 2),
        ("feb", 2),
        ("march", 3),
        ("mar", 3),
        ("april", 4),
        ("apr", 4),
        ("may", 5),
        ("june", 6),
        ("jun", 6),
        ("july", 7),
        ("jul", 7),
        ("august", 8),
        ("aug", 8),
        ("september", 9),
        ("sept", 9),
        ("sep", 9),
        ("october", 10),
        ("oct", 10),
        ("november", 11),
        ("nov", 11),
        ("december", 12),
        ("dec", 12),
    ];
    M.iter().find(|(n, _)| *n == w).map(|&(_, m)| m)
}

/// "3", "03", "3rd", "21st" -> the day of the month.
fn day_of(w: &str) -> Option<u32> {
    let digits = w.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let suffix = &w[digits.len()..];
    if digits.len() > 2 || !all_digits(digits) {
        return None;
    }
    if !suffix.is_empty() && !matches!(suffix, "st" | "nd" | "rd" | "th") {
        return None;
    }
    let d: u32 = digits.parse().ok()?;
    (1..=31).contains(&d).then_some(d)
}

fn year_of(w: &str) -> Option<u32> {
    if w.len() != 4 || !all_digits(w) {
        return None;
    }
    let y: u32 = w.parse().ok()?;
    (1900..=2100).contains(&y).then_some(y)
}

fn is_weekday(w: &str, prev: &str) -> bool {
    match w {
        "monday" | "tuesday" | "wednesday" | "thursday" | "friday" | "saturday" | "sunday"
        | "tue" | "tues" | "thu" | "thur" | "thurs" | "fri" => true,
        // "sun", "sat", "wed" and "mon" are also ordinary words: only after a preposition
        "mon" | "wed" | "sat" | "sun" => matches!(
            prev,
            "by" | "on" | "next" | "this" | "last" | "every" | "until" | "before"
        ),
        _ => false,
    }
}

/// `2026-11-03`, `3/11/2026`, `11.03.26`: three numeric parts that can be a calendar date.
fn is_numeric_date(w: &str) -> bool {
    let parts: Vec<&str> = w.split(['-', '/', '.']).collect();
    if parts.len() != 3 || !parts.iter().all(|p| p.len() <= 4 && all_digits(p)) {
        return false;
    }
    let l: Vec<usize> = parts.iter().map(|p| p.len()).collect();
    let n: Vec<u32> = parts.iter().map(|p| p.parse().unwrap_or(0)).collect();
    let month = |v: u32| (1..=12).contains(&v);
    let day = |v: u32| (1..=31).contains(&v);
    if l[0] == 4 && l[1] <= 2 && l[2] <= 2 {
        return month(n[1]) && day(n[2]);
    }
    if l[0] <= 2 && l[1] <= 2 && (l[2] == 4 || l[2] == 2) {
        return (month(n[0]) && day(n[1])) || (day(n[0]) && month(n[1]));
    }
    false
}

/// `3pm`, `10:30`, `10:30am` -> (has am/pm, has a colon). `None` for anything else.
fn clock(w: &str) -> Option<(bool, bool)> {
    let (body, mer) = match w.strip_suffix("am").or_else(|| w.strip_suffix("pm")) {
        Some(b) => (b, true),
        None => (w, false),
    };
    let (h, m) = match body.split_once(':') {
        Some((h, m)) => (h, Some(m)),
        None => (body, None),
    };
    if h.len() > 2 || !all_digits(h) {
        return None;
    }
    let hv: u32 = h.parse().ok()?;
    if let Some(m) = m {
        if m.len() != 2 || !all_digits(m) || m.parse::<u32>().ok()? > 59 {
            return None;
        }
    }
    let colon = m.is_some();
    if !mer && !colon {
        return None;
    }
    let ok = if mer {
        (1..=12).contains(&hv)
    } else {
        hv <= 23
    };
    ok.then_some((mer, colon))
}

fn is_quarter(w: &str) -> bool {
    let b = w.as_bytes();
    (b.len() == 2 && b[0] == b'q' && (b'1'..=b'4').contains(&b[1]))
        || (w.starts_with("fy") && matches!(w.len(), 4 | 6) && all_digits(&w[2..]))
}

/// Words that open a date phrase ("by Friday", "in Q3", "until March").
const DATE_PREFIX: &[&str] = &[
    "by", "on", "at", "in", "next", "this", "last", "every", "until", "till", "before", "after",
    "from", "since", "through", "due", "around",
];
const COUNT_WORDS: &[&str] = &[
    "a", "an", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    "twelve", "fifteen", "twenty", "thirty", "sixty", "ninety",
];
const TIME_UNITS: &[&str] = &[
    "minute", "minutes", "min", "mins", "hour", "hours", "hr", "hrs", "day", "days", "week",
    "weeks", "month", "months", "year", "years", "quarter", "quarters",
];
const PERIOD_UNITS: &[&str] = &[
    "week",
    "weekend",
    "month",
    "quarter",
    "year",
    "day",
    "morning",
    "afternoon",
    "evening",
    "night",
    "fortnight",
    "sprint",
];
const REL_WORDS: &[&str] = &[
    "next",
    "this",
    "last",
    "every",
    "coming",
    "previous",
    "following",
    "upcoming",
];
const SINGLE_REL: &[&str] = &[
    "today",
    "tonight",
    "tomorrow",
    "yesterday",
    "eod",
    "eow",
    "eom",
    "eoq",
    "eoy",
    "cob",
    "noon",
    "midnight",
];
const TIMEZONES: &[&str] = &[
    "utc", "gmt", "est", "edt", "cst", "cdt", "mst", "mdt", "pst", "pdt", "cet", "cest", "ist",
    "bst", "jst", "et", "pt",
];
const ORDINAL_Q: &[&str] = &[
    "first", "second", "third", "fourth", "1st", "2nd", "3rd", "4th",
];

/// End (exclusive) of a date / time phrase that starts at token `i`, if one does.
fn date_span(t: &[Tok], i: usize) -> Option<usize> {
    let w = t[i].lc.as_str();
    let at = |k: usize| tok_at(t, k);
    let prev = if i > 0 { t[i - 1].lc.as_str() } else { "" };
    let meridiem = |k: usize| matches!(at(k), "am" | "pm" | "a.m" | "p.m");

    if is_numeric_date(w) {
        return Some(i + 1);
    }
    // clock times: 3pm, 10:30, 10:30 am, 3 pm, 15:30 UTC
    if let Some((has_mer, _)) = clock(w) {
        let mut e = i + 1;
        if !has_mer && meridiem(e) {
            e += 1;
        }
        if TIMEZONES.contains(&at(e)) {
            e += 1;
        }
        return Some(e);
    }
    if all_digits(w)
        && w.len() <= 2
        && (1..=12).contains(&w.parse::<u32>().unwrap_or(0))
        && meridiem(i + 1)
    {
        let mut e = i + 2;
        if TIMEZONES.contains(&at(e)) {
            e += 1;
        }
        return Some(e);
    }
    if SINGLE_REL.contains(&w) || is_weekday(w, prev) {
        return Some(i + 1);
    }
    // Q3, Q3 2026, H1, FY26, "third quarter 2026"
    if is_quarter(w) {
        return Some(i + 1 + usize::from(year_of(at(i + 1)).is_some()));
    }
    if ORDINAL_Q.contains(&w) && at(i + 1) == "quarter" {
        return Some(i + 2 + usize::from(year_of(at(i + 2)).is_some()));
    }
    // day first: 3 Nov 2026, 3rd of November
    if day_of(w).is_some() {
        let mut e = i + 1;
        if at(e) == "of" {
            e += 1;
        }
        if month_of(at(e)).is_some() {
            e += 1;
            return Some(e + usize::from(year_of(at(e)).is_some()));
        }
    }
    // month first: Nov 3, November 2026, in March. "may" and "march" are also ordinary words.
    if month_of(w).is_some() {
        if day_of(at(i + 1)).is_some() {
            let e = i + 2;
            return Some(e + usize::from(year_of(at(e)).is_some()));
        }
        if year_of(at(i + 1)).is_some() {
            return Some(i + 2);
        }
        let ambiguous = matches!(w, "may" | "march");
        if !ambiguous
            || matches!(
                prev,
                "in" | "by"
                    | "until"
                    | "before"
                    | "after"
                    | "since"
                    | "during"
                    | "from"
                    | "early"
                    | "mid"
                    | "late"
                    | "end"
                    | "of"
            )
        {
            return Some(i + 1);
        }
        return None;
    }
    if REL_WORDS.contains(&w) && PERIOD_UNITS.contains(&at(i + 1)) {
        return Some(i + 2);
    }
    // in / within / after N units
    if matches!(w, "in" | "within" | "after") {
        let c = at(i + 1);
        if (all_digits(c) || COUNT_WORDS.contains(&c)) && TIME_UNITS.contains(&at(i + 2)) {
            return Some(i + 3);
        }
    }
    // end of the day / week / month / March
    if matches!(w, "end" | "start" | "beginning" | "middle") && at(i + 1) == "of" {
        let mut e = i + 2;
        if at(e) == "the" {
            e += 1;
        }
        if at(e) == "business" && at(e + 1) == "day" {
            return Some(e + 2);
        }
        if matches!(
            at(e),
            "day" | "week" | "month" | "quarter" | "year" | "sprint"
        ) || month_of(at(e)).is_some()
        {
            return Some(e + 1);
        }
    }
    if matches!(w, "deadline" | "deadlines") {
        return Some(i + 1);
    }
    if w == "due" && at(i + 1) == "date" {
        return Some(i + 2);
    }
    // a bare year after a preposition: "in 2026"
    if year_of(w).is_some()
        && matches!(
            prev,
            "in" | "since"
                | "by"
                | "until"
                | "before"
                | "after"
                | "during"
                | "from"
                | "through"
                | "of"
                | "fy"
        )
    {
        return Some(i + 1);
    }
    None
}

/// Every date / deadline / time phrase in `s`, in order ("by Friday", "Q3 2026", "at 3 pm").
/// At most four are returned.
pub fn find_dates(s: &str) -> Vec<String> {
    let t = tokens(s);
    let n = t.len();
    let mut out: Vec<String> = Vec::new();
    let mut last_end = 0usize;
    let mut i = 0usize;
    while i < n && out.len() < 4 {
        let Some(e) = date_span(&t, i) else {
            i += 1;
            continue;
        };
        let mut st = i;
        // pull in one leading preposition / qualifier: "by Friday", "no later than 5pm"
        if st >= last_end + 3 && is_seq(&t, st - 3, &["no", "later", "than"]) {
            st -= 3;
        } else if st > last_end && DATE_PREFIX.contains(&t[st - 1].lc.as_str()) {
            st -= 1;
        } else if st >= last_end + 2
            && t[st - 1].lc == "the"
            && DATE_PREFIX.contains(&t[st - 2].lc.as_str())
        {
            st -= 2;
        }
        let p = phrase_of(s, &t, st, e);
        if !out.contains(&p) {
            out.push(p);
        }
        last_end = e;
        i = e;
    }
    out
}

// ---- action items, decisions, open questions ---------------------------------------------

/// Sentence-initial words that look like names because they are capitalised.
const NON_NAME: &[&str] = &[
    "the",
    "a",
    "an",
    "this",
    "that",
    "these",
    "those",
    "it",
    "its",
    "there",
    "here",
    "he",
    "she",
    "they",
    "we",
    "i",
    "you",
    "who",
    "what",
    "when",
    "where",
    "why",
    "how",
    "which",
    "if",
    "as",
    "in",
    "on",
    "at",
    "for",
    "to",
    "by",
    "with",
    "from",
    "after",
    "before",
    "during",
    "however",
    "also",
    "but",
    "and",
    "or",
    "so",
    "then",
    "today",
    "tomorrow",
    "yesterday",
    "tonight",
    "next",
    "last",
    "every",
    "all",
    "some",
    "many",
    "most",
    "our",
    "their",
    "my",
    "your",
    "his",
    "her",
    "no",
    "not",
    "once",
    "while",
    "although",
    "because",
    "since",
    "please",
    "note",
    "todo",
    "action",
    "attendees",
    "agenda",
    "notes",
    "update",
    "summary",
    "decision",
    "question",
    "open",
    "owner",
    "status",
];

fn is_name(t: &Tok) -> bool {
    t.cap
        && t.lc.chars().count() >= 2
        && t.lc.chars().all(|c| c.is_alphabetic())
        && !NON_NAME.contains(&t.lc.as_str())
        && month_of(&t.lc).is_none()
        && !is_weekday(&t.lc, "")
}

/// Verbs that open an instruction ("Send the report."). Safe to read as imperatives anywhere.
const IMPERATIVES: &[&str] = &[
    "send",
    "email",
    "call",
    "update",
    "review",
    "schedule",
    "prepare",
    "create",
    "write",
    "fix",
    "confirm",
    "book",
    "submit",
    "share",
    "finalize",
    "finalise",
    "draft",
    "contact",
    "follow",
    "add",
    "remove",
    "arrange",
    "complete",
    "finish",
    "deploy",
    "investigate",
    "ping",
    "organize",
    "organise",
    "buy",
    "pay",
    "sign",
    "reply",
    "approve",
    "notify",
    "remind",
    "clean",
    "publish",
    "renew",
    "cancel",
    "collect",
    "gather",
    "print",
    "upload",
    "download",
    "install",
    "ask",
    "tell",
    "verify",
    "migrate",
    "refactor",
    "document",
    "assign",
    "escalate",
    "merge",
];
/// More verbs that are only trusted as instructions at the start of a list item ("Order pizza").
const IMPERATIVES_LIST: &[&str] = &[
    "plan",
    "order",
    "test",
    "set",
    "file",
    "move",
    "record",
    "report",
    "get",
    "make",
    "take",
    "put",
    "bring",
    "pick",
    "clear",
    "close",
    "open",
    "check",
    "run",
    "start",
    "stop",
    "build",
    "learn",
    "practice",
    "try",
    "find",
    "look",
    "think",
    "decide",
    "discuss",
    "list",
    "track",
    "wrap",
    "ship",
    "launch",
    "polish",
    "read",
    "reach",
    "turn",
    "walk",
    "water",
    "feed",
    "wash",
    "pack",
    "return",
    "drop",
    "grab",
    "fill",
    "choose",
    "compare",
    "research",
    "negotiate",
    "hire",
    "interview",
    "train",
    "invite",
    "thank",
    "congratulate",
];
const NOT_AFTER_IMPERATIVE: &[&str] = &[
    "is", "are", "was", "were", "has", "have", "had", "will", "can", "would", "could", "should",
    "may", "might", "of", "to", "and", "or", "does", "did",
];

fn is_verb(w: &str, list_item: bool) -> bool {
    IMPERATIVES.contains(&w) || (list_item && IMPERATIVES_LIST.contains(&w))
}

/// Length of a leading checkbox marker (`[ ]`, `[x]`, `☐`, `☑`) plus its whitespace, or 0.
pub fn checkbox_len(s: &str) -> usize {
    for m in [
        "[ ]", "[x]", "[X]", "[]", "\u{2610}", "\u{2611}", "\u{2612}",
    ] {
        if let Some(rest) = s.strip_prefix(m) {
            return s.len() - rest.trim_start().len();
        }
    }
    0
}

/// Is this sentence an action item? `(strength 1..=3, the phrase that matched)`.
/// `list_item` is true for bullets, where a bare verb opens an instruction ("Buy milk").
pub fn action_cue(s: &str, list_item: bool) -> Option<(u8, String)> {
    let trimmed = s.trim_start();
    let cb = checkbox_len(trimmed);
    if cb > 0 {
        return Some((3, "checkbox".to_string()));
    }
    let t = tokens(s);
    if t.is_empty() {
        return None;
    }
    let n = t.len();
    let q = ends_with_question(s);
    // a request: "Can you send the notes?"
    const REQUEST: &[&[&str]] = &[
        &["can", "you"],
        &["could", "you"],
        &["would", "you"],
        &["can", "someone"],
        &["could", "someone"],
    ];
    for i in 0..n {
        for p in REQUEST {
            if is_seq(&t, i, p) {
                return Some((2, phrase_of(s, &t, i, i + p.len())));
            }
        }
    }
    if q {
        return None;
    }
    // an owner: "Dana to send the plan", "Dana: send the plan"
    if is_name(&t[0]) && n >= 3 && t[1].lc == "to" && is_verb(&t[2].lc, true) {
        return Some((3, phrase_of(s, &t, 0, 3)));
    }
    if is_name(&t[0]) && n >= 2 && s[t[0].end..].starts_with(':') && is_verb(&t[1].lc, true) {
        return Some((3, phrase_of(s, &t, 0, 2)));
    }
    const STRONG: &[&[&str]] = &[
        &["todo"],
        &["to-do"],
        &["action", "item"],
        &["action", "items"],
        &["follow", "up"],
        &["follow-up"],
        &["next", "step"],
        &["next", "steps"],
        &["need", "to"],
        &["needs", "to"],
        &["has", "to"],
        &["have", "to"],
        &["must"],
        &["required", "to"],
        &["responsible", "for"],
        &["make", "sure"],
        &["remember", "to"],
        &["don't", "forget"],
        &["be", "sure", "to"],
        &["please"],
        &["kindly"],
        &["i'll"],
        &["we'll"],
        &["let's"],
    ];
    for i in 0..n {
        for p in STRONG {
            if is_seq(&t, i, p) {
                return Some((2, phrase_of(s, &t, i, i + p.len())));
            }
        }
    }
    // will / shall / should / going to: only with a person doing it ("I will", "Dana will"),
    // otherwise "the park will stay open" would be an action item
    for i in 1..n {
        let w = t[i].lc.as_str();
        let span = match w {
            "will" | "shall" | "should" => 1,
            "going" | "ought" if tok_at(&t, i + 1) == "to" => 2,
            _ => 0,
        };
        if span > 0 {
            let p = t[i - 1].lc.as_str();
            if matches!(p, "i" | "we" | "you") || is_name(&t[i - 1]) {
                return Some((1, phrase_of(s, &t, i - 1, i + span)));
            }
        }
    }
    // an instruction: "Send the report by Friday."
    let first = t[0].lc.as_str();
    if is_verb(first, list_item)
        && n >= 2
        && !NOT_AFTER_IMPERATIVE.contains(&t[1].lc.as_str())
        && !s[t[0].end..].starts_with(':')
    {
        return Some((if list_item { 2 } else { 1 }, phrase_of(s, &t, 0, 1)));
    }
    None
}

/// Is this sentence a decision? The phrase that matched ("decided", "agreed", ...).
pub fn decision_cue(s: &str) -> Option<String> {
    if ends_with_question(s) {
        return None;
    }
    let low = s.to_lowercase();
    const OPEN: &[&str] = &[
        "to be decided",
        "yet to decide",
        "yet to be decided",
        "not yet decided",
        "not decided",
        "undecided",
        "no decision",
        "tbd",
        "if we decide",
        "whether to",
    ];
    if OPEN.iter().any(|g| low.contains(g)) {
        return None;
    }
    let t = tokens(s);
    const PHRASES: &[&[&str]] = &[
        &["signed", "off"],
        &["will", "go", "with"],
        &["we'll", "go", "with"],
        &["going", "with"],
        &["will", "proceed", "with"],
    ];
    for i in 0..t.len() {
        let w = t[i].lc.as_str();
        let mut len = match w {
            "decided" | "agreed" | "approved" | "resolved" | "settled" | "chose" | "chosen"
            | "voted" | "adopted" | "finalized" | "finalised" | "endorsed" | "ratified"
            | "rejected" | "vetoed" | "opted" | "decision" | "decisions" => 1,
            _ => 0,
        };
        if len == 0 {
            if let Some(p) = PHRASES.iter().find(|p| is_seq(&t, i, p)) {
                len = p.len();
            }
        }
        if len == 0 {
            continue;
        }
        if matches!(w, "decision" | "decisions")
            && matches!(
                tok_at(&t, i + 1),
                "tree" | "trees" | "making" | "maker" | "makers" | "support" | "boundary"
            )
        {
            continue;
        }
        // "if we agree", "once approved", "unless decided": a condition, not a decision
        let from = i.saturating_sub(3);
        if t[from..i].iter().any(|p| {
            matches!(
                p.lc.as_str(),
                "if" | "whether" | "unless" | "once" | "until"
            )
        }) {
            continue;
        }
        return Some(phrase_of(s, &t, i, i + len));
    }
    None
}

/// Is this sentence an open question or unresolved point? `"?"` or the phrase that matched.
pub fn question_cue(s: &str) -> Option<String> {
    if ends_with_question(s) {
        return Some("?".to_string());
    }
    let t = tokens(s);
    const PHRASES: &[&[&str]] = &[
        &["tbd"],
        &["tbc"],
        &["unclear"],
        &["unresolved"],
        &["undecided"],
        &["undetermined"],
        &["unsure"],
        &["open", "question"],
        &["open", "questions"],
        &["open", "issue"],
        &["open", "issues"],
        &["to", "be", "decided"],
        &["to", "be", "determined"],
        &["to", "be", "confirmed"],
        &["not", "sure"],
        &["not", "yet", "decided"],
        &["need", "to", "decide"],
        &["needs", "to", "be", "decided"],
        &["remains", "to", "be", "seen"],
        &["up", "in", "the", "air"],
        &["i", "wonder"],
        &["wondering", "whether"],
        &["wondering", "if"],
    ];
    for i in 0..t.len() {
        if let Some(p) = PHRASES.iter().find(|p| is_seq(&t, i, p)) {
            return Some(phrase_of(s, &t, i, i + p.len()));
        }
    }
    None
}

// ---------------------------------------------------------------------------------------
// Stop words (English, plus Hindi / Hinglish function words)
// ---------------------------------------------------------------------------------------

pub fn is_stopword(w: &str) -> bool {
    STOP.binary_search(&w).is_ok() || STOP_MULTI.binary_search(&w).is_ok()
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

/// Hindi (Devanagari) and Hinglish (Roman) function words, so Devanagari and mixed
/// Hindi/English text get real term weighting instead of treating every particle as a
/// content word. Sorted for binary search. Content words are deliberately absent.
const STOP_MULTI: &[&str] = &[
    "apna",
    "apne",
    "apni",
    "aur",
    "bhi",
    "hai",
    "hain",
    "hua",
    "hui",
    "isliye",
    "jab",
    "ka",
    "kar",
    "karna",
    "karo",
    "ke",
    "ki",
    "ko",
    "kya",
    "lekin",
    "liye",
    "matlab",
    "mein",
    "nahi",
    "nhi",
    "par",
    "se",
    "tak",
    "tha",
    "thi",
    "ya",
    "yaani",
    "yeh",
    "अंदर",
    "अपना",
    "अपनी",
    "अपने",
    "अभी",
    "आदि",
    "इन",
    "इनका",
    "इस",
    "इसका",
    "इसकी",
    "इसके",
    "इसमें",
    "इसे",
    "उन",
    "उनका",
    "उनकी",
    "उनके",
    "उस",
    "उसका",
    "उसकी",
    "उसके",
    "उसमें",
    "उसे",
    "एक",
    "ऐसा",
    "ऐसी",
    "ऐसे",
    "और",
    "कभी",
    "करता",
    "करते",
    "करना",
    "का",
    "काफी",
    "कि",
    "किया",
    "किस",
    "किसी",
    "की",
    "के",
    "केवल",
    "खुद",
    "गए",
    "गया",
    "जब",
    "जहाँ",
    "जा",
    "जितना",
    "जिस",
    "जिसे",
    "जो",
    "तक",
    "तब",
    "तरह",
    "तो",
    "था",
    "थी",
    "थे",
    "दिया",
    "दूसरा",
    "दूसरी",
    "दूसरे",
    "न",
    "नहीं",
    "ना",
    "पर",
    "पहले",
    "पूरा",
    "पे",
    "फिर",
    "बहुत",
    "बाद",
    "बिल्कुल",
    "भर",
    "भी",
    "मगर",
    "में",
    "सकता",
    "सकते",
    "सब",
    "सभी",
    "साथ",
    "से",
    "हम",
    "हमारा",
    "हमारी",
    "हमारे",
    "ही",
    "हुआ",
    "हुई",
    "हुए",
    "है",
    "हैं",
    "हो",
    "होता",
    "होती",
    "होते",
    "होना",
    "होने",
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

    #[test]
    fn multi_stopword_list_is_sorted_for_binary_search() {
        let mut sorted = STOP_MULTI.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, STOP_MULTI, "STOP_MULTI must stay sorted");
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            STOP_MULTI.len(),
            "STOP_MULTI must not contain duplicates"
        );
    }

    #[test]
    fn hindi_danda_splits_sentences() {
        let t = texts("\u{92f}\u{939} \u{92a}\u{939}\u{932}\u{940} \u{935}\u{93e}\u{915}\u{94d}\u{92f} \u{939}\u{948}\u{964} \u{92f}\u{939} \u{926}\u{942}\u{938}\u{930}\u{93e} \u{935}\u{93e}\u{915}\u{94d}\u{92f} \u{939}\u{948}\u{964} \u{914}\u{930} \u{92f}\u{939} \u{924}\u{940}\u{938}\u{930}\u{93e} \u{939}\u{948}\u{964}");
        assert_eq!(t.len(), 3, "{t:?}");
    }

    #[test]
    fn atx_headings_are_heading_units() {
        let input = "#### Krovi tools\n\nThe shelf holds widgets. Timers live here too.";
        let u = split_units(input);
        assert_eq!(u[0].kind, UnitKind::Heading);
        assert_eq!(u[0].text, "Krovi tools");
        assert_eq!(&input[u[0].start..u[0].end], "Krovi tools");
        assert_eq!(u[1].kind, UnitKind::Sentence);
    }

    #[test]
    fn hash_without_space_is_not_a_heading() {
        let u = split_units("#tag trends on social media. Sometimes twice.");
        assert_eq!(u[0].kind, UnitKind::Sentence);
    }

    #[test]
    fn spaceless_scripts_are_counted_by_character_groups() {
        // 7 kana / ideographs -> 4 "words"; used to be 1 for the whole sentence
        assert_eq!(
            word_count("\u{4eca}\u{65e5}\u{306f}\u{6674}\u{308c}\u{3067}\u{3059}\u{3002}"),
            4
        );
        assert_eq!(word_count("Hello\u{4e16}\u{754c} ok"), 3);
        // ten Thai code points -> 3
        assert_eq!(
            word_count("\u{e2a}\u{e27}\u{e31}\u{e2a}\u{e14}\u{e35}\u{e04}\u{e23}\u{e31}\u{e1a}"),
            3
        );
        assert_eq!(word_count("\u{3002}\u{3001}"), 0);
    }

    #[test]
    fn spaceless_scripts_become_bigram_terms() {
        let t = content_terms("\u{4eca}\u{5929}\u{5929}\u{6c14}\u{5f88}\u{597d}");
        assert_eq!(t.len(), 5, "{t:?}");
        assert!(t.contains(&"\u{5929}\u{6c14}".to_string()));
        // a latin word next to them is still a normal term
        let mixed = content_terms("Krovi \u{5f88}\u{597d}");
        assert!(
            mixed.contains(&"krovi".to_string()) && mixed.contains(&"\u{5f88}\u{597d}".to_string())
        );
    }

    #[test]
    fn cjk_headings_are_recognised() {
        let u = split_units("\u{5173}\u{4e8e}\u{6211}\u{4eec}\n\n\u{6211}\u{4eec}\u{662f}\u{4e00}\u{4e2a}\u{5c0f}\u{56e2}\u{961f}\u{3002}");
        assert_eq!(u[0].kind, UnitKind::Heading, "{u:?}");
    }

    fn dates(s: &str) -> Vec<String> {
        find_dates(s)
    }

    #[test]
    fn date_phrases_are_found_with_their_preposition() {
        assert_eq!(dates("Send the report by Friday."), ["by Friday"]);
        assert_eq!(dates("Launch is in Q3 2026."), ["in Q3 2026"]);
        assert_eq!(
            dates("Call at 15:30 on 3 Nov 2026."),
            ["at 15:30", "on 3 Nov 2026"]
        );
        assert_eq!(
            dates("Meet next Monday at 10am."),
            ["next Monday", "at 10am"]
        );
        assert_eq!(dates("Reply within 30 days please."), ["within 30 days"]);
        assert_eq!(dates("In March we ship."), ["In March"]);
        assert_eq!(dates("Ready by EOD tomorrow."), ["by EOD", "tomorrow"]);
        assert_eq!(
            dates("Done by the end of the month."),
            ["by the end of the month"]
        );
        assert_eq!(dates("The final call is on 2026-04-10."), ["on 2026-04-10"]);
        assert_eq!(dates("Doors open at 3 pm EST."), ["at 3 pm EST"]);
        assert_eq!(
            dates("Launch April 14, 2026 at the venue."),
            ["April 14, 2026"]
        );
    }

    #[test]
    fn ordinary_words_are_not_dates() {
        assert!(dates("I may be late and the march was long.").is_empty());
        assert!(dates("We sat in the sun and ate 12 apples.").is_empty());
        assert!(dates("Version 3.5.1 has 1.5 million users.").is_empty());
        assert!(dates("The ratio was 4 to 1.").is_empty());
    }

    #[test]
    fn action_cues() {
        let a = |s: &str, bullet: bool| action_cue(s, bullet).map(|x| x.1);
        assert_eq!(
            a("Dana to send the revised plan by Friday.", false).as_deref(),
            Some("Dana to send")
        );
        assert_eq!(
            a("Priya needs to confirm the date.", false).as_deref(),
            Some("needs to")
        );
        assert_eq!(
            a("[ ] Omar will update the page.", true).as_deref(),
            Some("checkbox")
        );
        assert_eq!(a("Buy milk", true).as_deref(), Some("Buy"));
        assert_eq!(a("Send the report to Sam.", false).as_deref(), Some("Send"));
        assert_eq!(
            a("Dana will send the draft on Monday.", false).as_deref(),
            Some("Dana will")
        );
        assert_eq!(
            a("I will check the logs tonight.", false).as_deref(),
            Some("I will")
        );
        assert_eq!(
            a("Can you send the notes?", false).as_deref(),
            Some("Can you")
        );
        assert_eq!(a("TODO fix the build", true).as_deref(), Some("TODO"));
        // description, not instruction
        assert!(a("The park will stay open until 10 p.m.", false).is_none());
        assert!(a("Review of the data shows growth.", false).is_none());
        assert!(a("Who will own the rota?", false).is_none());
        assert!(a("Access to Europe is easy.", false).is_none());
    }

    #[test]
    fn decision_cues() {
        assert_eq!(
            decision_cue("We decided to move the launch.").as_deref(),
            Some("decided")
        );
        assert_eq!(
            decision_cue("The board approved the budget.").as_deref(),
            Some("approved")
        );
        assert_eq!(
            decision_cue("We'll go with option B.").as_deref(),
            Some("We'll go with")
        );
        assert!(decision_cue("The date is still to be decided.").is_none());
        assert!(decision_cue("Did we agree on the date?").is_none());
        assert!(decision_cue("Once approved, we ship.").is_none());
        assert!(decision_cue("A decision tree splits the data.").is_none());
    }

    #[test]
    fn question_cues() {
        assert_eq!(question_cue("Who owns the rota?").as_deref(), Some("?"));
        assert_eq!(question_cue("He asked \"why?\"").as_deref(), Some("?"));
        assert_eq!(
            question_cue("The budget is still TBD.").as_deref(),
            Some("TBD")
        );
        assert_eq!(
            question_cue("It is unclear whether it can wait.").as_deref(),
            Some("unclear")
        );
        assert!(question_cue("Everything is settled.").is_none());
    }

    #[test]
    fn display_clean_strips_markdown_noise() {
        assert_eq!(display_clean("**Bold** point."), "Bold point.");
        assert_eq!(display_clean("run `cargo test` now"), "run cargo test now");
        assert_eq!(display_clean("| --- | --- |"), "");
        assert_eq!(
            display_clean("| Template | Examples |"),
            "Template \u{b7} Examples"
        );
        assert_eq!(
            display_clean("file_name.rs is kept"),
            "file_name.rs is kept"
        );
        assert_eq!(display_clean("#### Heading line"), "Heading line");
    }
}
