//! On-device extractive summariser. Pure, deterministic, no network, no model download.
//!
//! "Extractive" means it picks the most important sentences of what you pasted and shows them
//! in their original order; it never writes new sentences, so it cannot invent facts.
//!
//! Pipeline
//! 1. [`text::split_units`] - sentences / bullets / headings, abbreviation- and list-aware.
//! 2. Each sentence becomes a TF-IDF vector over stemmed content words (IDF is computed across
//!    the sentences of this very text, so the "important words" are the ones that are frequent
//!    here and rare per sentence).
//! 3. Two centrality signals, averaged: TextRank (power iteration on the sentence-similarity
//!    graph) and similarity to the TF-IDF centroid of the whole text.
//! 4. Priors that encode how real documents are written: the lead sentence and paragraph openers
//!    matter more (news, reports), sentences of 9-35 words read best, "in conclusion / must /
//!    shall not" cues are boosted, "for example" and boilerplate ("subscribe", "click here") are
//!    not, and sentences sharing words with the title/heading get a small lift.
//! 5. MMR (maximal marginal relevance) picks sentences one by one, penalising similarity to the
//!    ones already chosen, so two paraphrases of the same point never both appear.
//! 6. The chosen sentences are returned in original order.

use std::collections::{HashMap, HashSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::text::{self, Unit, UnitKind};

/// Longest input the summariser accepts (characters, counted as bytes). ~400k is a long book
/// chapter; the work is roughly linear in the number of sentences past the candidate cap.
pub const MAX_CHARS: usize = 400_000;
/// Sentences beyond this many are pre-filtered by centrality before the O(n^2) graph step.
const MAX_CANDIDATES: usize = 1_200;
/// Average adult silent reading speed, words per minute.
pub const READ_WPM: f64 = 238.0;
/// Below this many words there is nothing worth cutting.
const SHORT_INPUT_WORDS: usize = 45;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SummaryLength {
    /// About two sentences.
    Short,
    /// A short paragraph (3-7 sentences depending on the length of the input).
    #[default]
    Medium,
    /// Bullet points, one sentence each.
    Points,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryItem {
    /// Index of the sentence in the input's sentence list (headings included), for stable keys.
    pub index: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub chars: usize,
    pub words: usize,
    pub sentences: usize,
    pub summary_words: usize,
    pub summary_sentences: usize,
    /// Seconds to read the original / the summary at [`READ_WPM`].
    pub read_secs: f64,
    pub summary_read_secs: f64,
    pub saved_secs: f64,
    /// summary words / original words (1.0 when nothing was cut).
    pub ratio: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub length: SummaryLength,
    /// Ready to show: sentences joined by a space (short / medium) or `\u{2022} ` lines (points).
    pub text: String,
    pub items: Vec<SummaryItem>,
    /// The input was too short to cut; `text` is the input itself.
    pub already_short: bool,
    pub note: Option<String>,
    pub stats: Stats,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryError {
    Empty,
    TooLong { chars: usize, max: usize },
}

impl fmt::Display for SummaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SummaryError::Empty => write!(f, "Paste some text to summarize."),
            SummaryError::TooLong { chars, max } => write!(
                f,
                "That is {chars} characters; Krovi summarizes up to {max} at a time. Paste a smaller part."
            ),
        }
    }
}
impl std::error::Error for SummaryError {}

/// Seconds to read `words` words.
pub fn read_seconds(words: usize) -> f64 {
    words as f64 / READ_WPM * 60.0
}

pub fn summarize(input: &str, length: SummaryLength) -> Result<Summary, SummaryError> {
    if input.trim().is_empty() {
        return Err(SummaryError::Empty);
    }
    if input.len() > MAX_CHARS {
        return Err(SummaryError::TooLong {
            chars: input.chars().count(),
            max: MAX_CHARS,
        });
    }

    let units = text::split_units(input);
    let words_total = text::word_count(input);
    let chars = input.chars().count();

    // Sentences that can be summarised: not headings, not fragments.
    let mut prose: Vec<usize> = units
        .iter()
        .enumerate()
        .filter(|(_, u)| u.kind != UnitKind::Heading)
        .map(|(i, _)| i)
        .collect();
    if prose.is_empty() {
        prose = (0..units.len()).collect();
    }
    let min_words = 4;
    let mut cands: Vec<usize> = prose
        .iter()
        .copied()
        .filter(|&i| text::word_count(&units[i].text) >= min_words)
        .collect();
    if cands.len() < 3 {
        cands = prose.clone();
    }

    let too_short = words_total < SHORT_INPUT_WORDS || cands.len() < 3;
    if too_short {
        let chosen: Vec<usize> = prose.clone();
        return Ok(finish(
            length,
            &units,
            &chosen,
            true,
            chars,
            words_total,
            units.len(),
        ));
    }

    let p_total = prose.len();
    let pos_of: HashMap<usize, usize> = prose.iter().enumerate().map(|(p, &u)| (u, p)).collect();

    // ---- TF-IDF vectors -------------------------------------------------------------
    let mut ids: HashMap<String, u32> = HashMap::new();
    let mut df: Vec<u32> = Vec::new();
    let mut counts: Vec<Vec<(u32, u32)>> = Vec::with_capacity(cands.len());
    for &ui in &cands {
        let mut c: HashMap<u32, u32> = HashMap::new();
        for t in text::content_terms(&units[ui].text) {
            let next = ids.len() as u32;
            let id = *ids.entry(t).or_insert(next);
            if id as usize >= df.len() {
                df.push(0);
            }
            *c.entry(id).or_insert(0) += 1;
        }
        for id in c.keys() {
            df[*id as usize] += 1;
        }
        let mut v: Vec<(u32, u32)> = c.into_iter().collect();
        v.sort_unstable_by_key(|x| x.0);
        counts.push(v);
    }
    let n = cands.len() as f32;
    let idf: Vec<f32> = df
        .iter()
        .map(|&d| ((n + 1.0) / (d as f32 + 0.5)).ln().max(0.0) + 0.15)
        .collect();
    let vecs: Vec<Vec<(u32, f32)>> = counts
        .iter()
        .map(|c| {
            let mut v: Vec<(u32, f32)> = c
                .iter()
                .map(|&(id, tf)| (id, (1.0 + (tf as f32).ln()) * idf[id as usize]))
                .collect();
            let norm = v.iter().map(|x| x.1 * x.1).sum::<f32>().sqrt();
            if norm > 0.0 {
                for x in &mut v {
                    x.1 /= norm;
                }
            }
            v
        })
        .collect();

    // Title / heading words (the first heading before any prose) lift sentences that share them.
    let title_ids: HashSet<u32> = units
        .iter()
        .take_while(|u| u.kind == UnitKind::Heading)
        .take(1)
        .flat_map(|u| text::content_terms(&u.text))
        .filter_map(|t| ids.get(&t).copied())
        .collect();

    // ---- centroid similarity (also used to pre-filter very long inputs) -------------------
    let mut centroid = vec![0f32; ids.len()];
    for v in &vecs {
        for &(id, w) in v {
            centroid[id as usize] += w;
        }
    }
    let cnorm = centroid.iter().map(|x| x * x).sum::<f32>().sqrt();
    if cnorm > 0.0 {
        for x in &mut centroid {
            *x /= cnorm;
        }
    }
    let centroid_sim: Vec<f32> = vecs
        .iter()
        .map(|v| v.iter().map(|&(id, w)| w * centroid[id as usize]).sum())
        .collect();

    let mut keep: Vec<usize> = (0..cands.len()).collect();
    if keep.len() > MAX_CANDIDATES {
        keep.sort_by(|&a, &b| centroid_sim[b].total_cmp(&centroid_sim[a]).then(a.cmp(&b)));
        keep.truncate(MAX_CANDIDATES);
        keep.sort_unstable();
    }
    let m = keep.len();

    // ---- similarity graph + TextRank ---------------------------------------------------
    let mut sim = vec![0f32; m * m];
    for a in 0..m {
        for b in (a + 1)..m {
            let s = dot(&vecs[keep[a]], &vecs[keep[b]]);
            sim[a * m + b] = s;
            sim[b * m + a] = s;
        }
    }
    let rank = text_rank(&sim, m);
    let cmax = keep.iter().map(|&k| centroid_sim[k]).fold(0f32, f32::max);

    // ---- final scores -------------------------------------------------------------
    let mut score = vec![0f32; m];
    for a in 0..m {
        let k = keep[a];
        let u = &units[cands[k]];
        let wc = text::word_count(&u.text);
        let base = if cmax > 0.0 {
            0.5 * rank[a] + 0.5 * centroid_sim[k] / cmax
        } else {
            1.0
        };
        let p = pos_of.get(&cands[k]).copied().unwrap_or(0);
        let title = if title_ids.is_empty() {
            1.0
        } else {
            let hit = vecs[k]
                .iter()
                .filter(|(id, _)| title_ids.contains(id))
                .count() as f32;
            1.0 + 0.3 * (hit / title_ids.len() as f32).min(1.0)
        };
        score[a] = base
            * position_prior(p, p_total, u.first_in_para)
            * length_prior(wc, length)
            * cue_prior(&u.text)
            * title;
    }
    let smax = score.iter().copied().fold(0f32, f32::max);
    if smax > 0.0 {
        for s in &mut score {
            *s /= smax;
        }
    }

    // ---- MMR selection --------------------------------------------------------------
    let (k_target, k_min, budget, lambda) = plan(length, p_total.min(cands.len()), words_total, m);
    let max_words = budget * 3 / 2;
    let words_of: Vec<usize> = (0..m)
        .map(|a| text::word_count(&units[cands[keep[a]]].text))
        .collect();
    let mut selected: Vec<usize> = Vec::new();
    let mut sel_words = 0usize;
    while selected.len() < k_target {
        let mut best: Option<(usize, f32)> = None;
        for a in 0..m {
            if selected.contains(&a) {
                continue;
            }
            let red = selected
                .iter()
                .map(|&s| sim[a * m + s])
                .fold(0f32, f32::max);
            if red >= 0.8 {
                continue; // a near-duplicate of something already chosen
            }
            if !selected.is_empty() && sel_words + words_of[a] > max_words {
                continue;
            }
            let gain = lambda * score[a] - (1.0 - lambda) * red;
            if best.is_none_or(|(_, g)| gain > g + 1e-6) {
                best = Some((a, gain));
            }
        }
        let Some((a, _)) = best else { break };
        selected.push(a);
        sel_words += words_of[a];
        if selected.len() >= k_min && sel_words >= budget {
            break;
        }
    }
    selected.sort_unstable();
    let chosen: Vec<usize> = selected.iter().map(|&a| cands[keep[a]]).collect();
    Ok(finish(
        length,
        &units,
        &chosen,
        false,
        chars,
        words_total,
        units.len(),
    ))
}

/// (sentences wanted, minimum before the word budget may stop us, soft word budget, MMR lambda)
fn plan(
    length: SummaryLength,
    prose: usize,
    words: usize,
    cands: usize,
) -> (usize, usize, usize, f32) {
    let w = words as f32;
    let (mut k, k_min, budget, lambda) = match length {
        SummaryLength::Short => (if prose >= 5 { 2 } else { 1 }, 1, 70usize, 0.85f32),
        SummaryLength::Medium => (
            ((prose as f32 * 0.2).round() as usize).clamp(3, 7),
            3,
            ((w * 0.2) as usize).clamp(90, 240),
            0.70,
        ),
        SummaryLength::Points => (
            ((prose as f32 * 0.15).round() as usize).clamp(3, 8),
            3,
            ((w * 0.15) as usize).clamp(70, 200),
            0.65,
        ),
    };
    // always shorter than the input
    k = k.min(cands.saturating_sub(1)).max(1);
    (k, k_min.min(k), budget, lambda)
}

fn finish(
    length: SummaryLength,
    units: &[Unit],
    chosen: &[usize],
    already_short: bool,
    chars: usize,
    words: usize,
    sentences: usize,
) -> Summary {
    let items: Vec<SummaryItem> = chosen
        .iter()
        .map(|&i| SummaryItem {
            index: i,
            text: text::collapse_ws(&units[i].text),
        })
        .collect();
    let text = if length == SummaryLength::Points && !already_short {
        items
            .iter()
            .map(|it| format!("\u{2022} {}", it.text))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        items
            .iter()
            .map(|it| it.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let summary_words = text::word_count(&text);
    let read = read_seconds(words);
    let sread = read_seconds(summary_words);
    let note = already_short.then(|| "This is already short, so here it is as written. Paste something longer to see a summary.".to_string());
    Summary {
        length,
        stats: Stats {
            chars,
            words,
            sentences,
            summary_words,
            summary_sentences: items.len(),
            read_secs: read,
            summary_read_secs: sread,
            saved_secs: (read - sread).max(0.0),
            ratio: if words == 0 {
                1.0
            } else {
                (summary_words as f64 / words as f64).min(1.0)
            },
        },
        text,
        items,
        already_short,
        note,
    }
}

// ---------------------------------------------------------------------------------------
// Scoring pieces
// ---------------------------------------------------------------------------------------

fn dot(a: &[(u32, f32)], b: &[(u32, f32)]) -> f32 {
    let (mut i, mut j, mut s) = (0, 0, 0f32);
    while i < a.len() && j < b.len() {
        match a[i].0.cmp(&b[j].0) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                s += a[i].1 * b[j].1;
                i += 1;
                j += 1;
            }
        }
    }
    s
}

/// TextRank by power iteration over the thresholded similarity graph, scaled to max = 1.
fn text_rank(sim: &[f32], m: usize) -> Vec<f32> {
    const THRESH: f32 = 0.05;
    const D: f32 = 0.85;
    if m == 0 {
        return Vec::new();
    }
    let mut adj: Vec<Vec<(usize, f32)>> = vec![Vec::new(); m];
    let mut out_sum = vec![0f32; m];
    for a in 0..m {
        for b in 0..m {
            let s = sim[a * m + b];
            if a != b && s >= THRESH {
                adj[a].push((b, s));
                out_sum[a] += s;
            }
        }
    }
    let mut r = vec![1.0 / m as f32; m];
    for _ in 0..80 {
        let dangling: f32 = (0..m).filter(|&j| out_sum[j] == 0.0).map(|j| r[j]).sum();
        let mut next = vec![(1.0 - D) / m as f32 + D * dangling / m as f32; m];
        for a in 0..m {
            let mut acc = 0f32;
            for &(j, w) in &adj[a] {
                acc += w / out_sum[j] * r[j];
            }
            next[a] += D * acc;
        }
        let delta: f32 = next.iter().zip(&r).map(|(x, y)| (x - y).abs()).sum();
        r = next;
        if delta < 1e-6 {
            break;
        }
    }
    let max = r.iter().copied().fold(0f32, f32::max);
    if max > 0.0 {
        for x in &mut r {
            *x /= max;
        }
    }
    r
}

/// Lead bias plus paragraph openers and the closing sentence.
fn position_prior(p: usize, total: usize, first_in_para: bool) -> f32 {
    // The opening sentences carry the topic (news's inverted pyramid, a definition, an abstract).
    let lead = 1.0 + 0.5 * (-(p as f32) / 2.5).exp();
    let para = if first_in_para { 1.08 } else { 1.0 };
    let last = if p + 1 == total && total > 3 {
        1.05
    } else {
        1.0
    };
    lead * para * last
}

/// Sentences of 9-35 words read best; fragments and run-ons are down-weighted.
fn length_prior(words: usize, length: SummaryLength) -> f32 {
    let (lo, hi, hard) = match length {
        SummaryLength::Points => (8.0, 28.0, 70.0),
        _ => (9.0, 35.0, 90.0),
    };
    let w = words as f32;
    if w < 5.0 {
        0.5
    } else if w < lo {
        0.6 + 0.4 * (w - 5.0) / (lo - 5.0)
    } else if w <= hi {
        1.0
    } else if w < hard {
        1.0 - 0.35 * (w - hi) / (hard - hi)
    } else {
        0.6
    }
}

fn cue_prior(sentence: &str) -> f32 {
    // Phrases that announce the point. Kept specific on purpose: a bare "the key" or "the main"
    // also matches "the key through a hash function" and "the main path".
    const BOOST: &[&str] = &[
        "in conclusion",
        "in summary",
        "to summarize",
        "to summarise",
        "in short",
        "overall,",
        "importantly",
        "the main idea",
        "the main point",
        "the key point",
        "the key idea",
        "the purpose of",
        "the goal of",
        "this means",
        "as a result",
        "therefore",
        " must ",
        "shall not",
        "is prohibited",
        "are prohibited",
        "is required",
        "are required",
        "in no event",
    ];
    const DOWN: &[&str] = &[
        "for example",
        "for instance",
        "e.g.",
        "such as",
        "photo:",
        "image:",
        "caption:",
        "source:",
    ];
    const JUNK: &[&str] = &[
        "subscribe",
        "click here",
        "read more",
        "sign up",
        "advertisement",
        "all rights reserved",
        "share this",
        "follow us",
    ];
    let l = sentence.to_lowercase();
    let mut f = 1.0f32;
    if BOOST.iter().any(|c| l.contains(c)) {
        f *= 1.10;
    }
    if DOWN
        .iter()
        .any(|c| l.starts_with(c) || l.contains(&format!(", {c}")))
    {
        f *= 0.88;
    }
    if JUNK.iter().any(|c| l.contains(c)) {
        f *= 0.4;
    }
    if l.trim_end().ends_with('?') {
        f *= 0.85;
    }
    if l.starts_with('"') || l.starts_with('\u{201c}') {
        f *= 0.92;
    }
    // Specifics (figures, dates, money) are usually what a reader wants kept.
    if l.chars().any(|c| c.is_ascii_digit()) {
        f *= 1.05;
        if l.contains('$')
            || l.contains('%')
            || l.contains("percent")
            || l.contains("million")
            || l.contains("billion")
        {
            f *= 1.03;
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEGAL: &str = include_str!("../tests/fixtures/legal.txt");
    const TECH: &str = include_str!("../tests/fixtures/technical.txt");
    const NEWS: &str = include_str!("../tests/fixtures/news.txt");

    fn all() -> [(&'static str, &'static str); 3] {
        [("legal", LEGAL), ("technical", TECH), ("news", NEWS)]
    }

    /// Every summary sentence is a real sentence of the input (extractive, never invented).
    fn assert_extractive(input: &str, s: &Summary) {
        let normalized = text::collapse_ws(input);
        for it in &s.items {
            assert!(
                normalized.contains(&it.text),
                "not extractive: {:?}",
                it.text
            );
        }
    }

    #[test]
    fn real_texts_are_summarised_in_every_mode() {
        for (name, input) in all() {
            let words = text::word_count(input);
            for len in [
                SummaryLength::Short,
                SummaryLength::Medium,
                SummaryLength::Points,
            ] {
                let s = summarize(input, len).unwrap();
                assert!(!s.already_short, "{name}");
                assert_extractive(input, &s);
                assert!(
                    s.stats.summary_words < words * 6 / 10,
                    "{name} {len:?}: {} of {words}",
                    s.stats.summary_words
                );
                assert!(
                    s.stats.saved_secs > 0.0 && s.stats.ratio < 0.6,
                    "{name} {len:?}"
                );
                let idx: Vec<usize> = s.items.iter().map(|i| i.index).collect();
                let mut sorted = idx.clone();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(
                    idx, sorted,
                    "{name}: sentences must be in original order, no repeats"
                );
            }
        }
    }

    #[test]
    fn lengths_are_ordered_and_sized_as_promised() {
        for (name, input) in all() {
            let short = summarize(input, SummaryLength::Short).unwrap();
            let medium = summarize(input, SummaryLength::Medium).unwrap();
            let points = summarize(input, SummaryLength::Points).unwrap();
            assert!(
                (1..=2).contains(&short.items.len()),
                "{name}: short has {}",
                short.items.len()
            );
            assert!(
                (3..=7).contains(&medium.items.len()),
                "{name}: medium has {}",
                medium.items.len()
            );
            assert!(
                (3..=8).contains(&points.items.len()),
                "{name}: points has {}",
                points.items.len()
            );
            assert!(
                short.stats.summary_words <= medium.stats.summary_words,
                "{name}"
            );
            assert!(
                points.text.lines().all(|l| l.starts_with('\u{2022}')),
                "{name}"
            );
            assert_eq!(points.text.lines().count(), points.items.len());
            assert!(!short.text.contains('\u{2022}'));
        }
    }

    #[test]
    fn legal_summary_keeps_what_is_covered_and_what_is_not() {
        let s = summarize(LEGAL, SummaryLength::Medium).unwrap();
        let t = s.text.to_lowercase();
        // The point of a warranty clause: what is covered, and the limits on it.
        assert!(t.contains("warranty") || t.contains("defect"), "{t}");
        assert!(
            t.contains("not cover") || t.contains("liable") || t.contains("claim"),
            "{t}"
        );
    }

    #[test]
    fn technical_summary_keeps_the_core_idea() {
        let s = summarize(TECH, SummaryLength::Medium).unwrap();
        let t = s.text.to_lowercase();
        // the defining sentence ("A hash map stores key and value pairs ...") must survive
        assert!(t.contains("hash map stores key and value pairs"), "{t}");
        assert!(t.contains("hash function") || t.contains("bucket"), "{t}");
    }

    #[test]
    fn news_summary_keeps_the_lead() {
        let s = summarize(NEWS, SummaryLength::Short).unwrap();
        let first_units: Vec<usize> = s.items.iter().map(|i| i.index).collect();
        assert!(
            first_units.iter().any(|&i| i <= 2),
            "the lead sentence should be near the top: {first_units:?}"
        );
    }

    #[test]
    fn cue_phrases_do_not_fire_on_ordinary_words() {
        assert_eq!(
            cue_prior("the map runs the key through a hash function"),
            1.0
        );
        assert_eq!(cue_prior("add lighting along the main path"), 1.0);
        assert!(cue_prior("In conclusion, it works well.") > 1.0);
        assert!(cue_prior("Subscribe to our newsletter today.") < 0.5);
        assert!(cue_prior("It cost $5 million in 2024.") > 1.0);
    }

    #[test]
    fn the_opening_sentence_is_favoured_over_an_equally_central_one() {
        assert!(position_prior(0, 10, true) > position_prior(4, 10, false));
        assert!(position_prior(0, 10, true) > 1.4);
    }

    #[test]
    fn near_duplicate_sentences_are_not_both_chosen() {
        let dup = "The committee approved the new budget for the coming fiscal year on Tuesday. \
                   On Tuesday the committee approved the new budget for the coming fiscal year. \
                   Rainfall across the northern region was well below the seasonal average this spring. \
                   Farmers there have started rationing irrigation water for their remaining crops. \
                   The local museum will reopen next month after two years of renovation work. \
                   Visitors can expect a new wing devoted to regional history and modern art.";
        let s = summarize(dup, SummaryLength::Medium).unwrap();
        let budget_hits = s
            .items
            .iter()
            .filter(|i| i.text.to_lowercase().contains("budget"))
            .count();
        assert!(budget_hits <= 1, "{:?}", s.items);
    }

    #[test]
    fn short_input_is_returned_as_is_with_a_note() {
        let s = summarize(
            "Just a quick note. Meeting moved to 3 pm.",
            SummaryLength::Short,
        )
        .unwrap();
        assert!(s.already_short);
        assert!(s.note.is_some());
        assert_eq!(s.text, "Just a quick note. Meeting moved to 3 pm.");
        assert_eq!(s.stats.saved_secs, 0.0);
        let one = summarize("Itis", SummaryLength::Points).unwrap();
        assert!(one.already_short);
    }

    #[test]
    fn empty_and_oversized_input_are_errors() {
        assert_eq!(
            summarize("   \n ", SummaryLength::Short).unwrap_err(),
            SummaryError::Empty
        );
        let big = "word ".repeat(MAX_CHARS / 5 + 10);
        match summarize(&big, SummaryLength::Short) {
            Err(SummaryError::TooLong { max, .. }) => assert_eq!(max, MAX_CHARS),
            other => panic!("expected TooLong, got {other:?}"),
        }
    }

    #[test]
    fn text_without_sentence_punctuation_does_not_panic() {
        let blob = "alpha beta gamma delta epsilon ".repeat(60);
        let s = summarize(&blob, SummaryLength::Medium).unwrap();
        assert!(s.stats.chars > 0);
    }

    #[test]
    fn deterministic() {
        for (_, input) in all() {
            let a = summarize(input, SummaryLength::Medium).unwrap();
            let b = summarize(input, SummaryLength::Medium).unwrap();
            assert_eq!(a.text, b.text);
        }
    }

    #[test]
    fn reading_time_saved_adds_up() {
        let s = summarize(NEWS, SummaryLength::Short).unwrap();
        let st = &s.stats;
        assert!((st.read_secs - read_seconds(st.words)).abs() < 1e-6);
        assert!((st.saved_secs - (st.read_secs - st.summary_read_secs)).abs() < 1e-6);
        assert_eq!(st.summary_sentences, s.items.len());
    }

    #[test]
    fn a_long_document_is_fast_enough() {
        // ~170 KB, 30 000+ words: the candidate cap keeps the graph step bounded.
        let big = format!("{LEGAL}\n\n{TECH}\n\n{NEWS}\n\n").repeat(40);
        let t = std::time::Instant::now();
        let s = summarize(&big, SummaryLength::Medium).unwrap();
        let ms = t.elapsed().as_millis();
        assert!(s.items.len() >= 3);
        assert!(ms < 20_000, "took {ms} ms");
    }

    #[test]
    fn bullet_lists_summarise_to_bullets_of_the_same_items() {
        let input = "Project update\n\n- The migration to the new database finished two days ahead of schedule with zero downtime for customers.\n\
- Support tickets about login problems fell by sixty percent after the authentication service was rewritten.\n\
- The mobile app release is blocked on a payment provider certificate that expires at the end of the month.\n\
- Hiring for the two open engineering roles continues and four candidates reached the final interview stage.\n\
- Office move planning has started and the lease on the current space ends in March of next year.";
        let s = summarize(input, SummaryLength::Points).unwrap();
        assert!(s.items.len() >= 3);
        for it in &s.items {
            assert!(!it.text.starts_with('-'), "{it:?}");
        }
    }
}
