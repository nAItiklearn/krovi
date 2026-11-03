//! On-device extractive summariser. Pure, deterministic, no network, no model download.
//!
//! "Extractive" means it picks the most important sentences of what you pasted and shows them
//! in their original order; it never writes new sentences, so it cannot invent facts.
//!
//! Pipeline (src/lib/explain.ts is a step-for-step TypeScript port; keep them in sync)
//! 1. [`text::split_units`] - sentences / bullets / headings, abbreviation- and list-aware.
//! 2. Sections: every heading starts one (the heading is its title). Text without headings, and
//!    any section longer than [`SEC_MAX`] sentences, is cut into chunks at paragraph boundaries.
//!    Chunks of one parent share a few sentences of read-only context (the overlap) so ranking is
//!    not blind at the seams.
//! 3. TF-IDF vectors over stemmed content words (IDF across the whole text; space-less scripts
//!    use character bigrams). Per section: TextRank on the section's similarity graph plus
//!    similarity to the section centroid, blended with similarity to the whole-document
//!    centroid, then mild priors (a gentle lead bonus, sentence length, cue phrases, overlap with
//!    the nearest heading). The graph is per section, so the cost stays linear in input size and
//!    nothing is pre-filtered away: an off-topic section keeps its own ranking.
//! 4. The output budget (a share of the sentences that grows slowly with input length) is split
//!    across sections by size, with at least one sentence for every substantial section.
//!    Inside a section, MMR (maximal marginal relevance) picks sentences one by one, penalising
//!    similarity to those already chosen; across sections a hard near-duplicate check applies.
//! 5. Chosen sentences are returned in document order.
//!
//! Extraction modes (action items, decisions, dates, questions) skip the ranking: every sentence
//! is tested against the cue rules in [`text`], near-duplicates are dropped, and the matches are
//! returned in document order.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::text::{self, Unit, UnitKind};

/// Longest input the summariser accepts (characters). Longer text is refused with an explicit
/// error, never silently cut. ~1M characters is a long book; the work is linear in its size.
pub const MAX_CHARS: usize = 1_000_000;
/// Average adult silent reading speed, words per minute.
pub const READ_WPM: f64 = 238.0;
/// Below this many words (or with fewer than two sentences) there is nothing worth cutting.
/// Two real sentences are still compressible to one; a single paragraph-shaped sentence is not.
const SHORT_INPUT_WORDS: usize = 25;
/// Sections are cut into chunks of about this many sentences (at a paragraph boundary)...
const SEC_TARGET: usize = 24;
/// ...whenever they are longer than this (a chunk is never longer than this plus [`SEC_MIN`]).
const SEC_MAX: usize = 40;
/// A trailing chunk shorter than this is folded into the one before it.
const SEC_MIN: usize = 8;
/// Context sentences borrowed from each neighbouring chunk of the same parent section.
const OVERLAP: usize = 3;
/// Two sentences this similar (cosine) are the same point; the second is skipped.
const DUP: f64 = 0.8;
/// Extraction modes list at most this many sentences (and say so when they stop).
const MAX_MATCHES: usize = 150;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SummaryLength {
    /// One to three sentences.
    Short,
    /// A summary that grows with the input and visits every substantial section
    /// (wire string `detailed`; the old `medium` still loads).
    #[default]
    #[serde(alias = "medium")]
    Detailed,
    /// Bullet points, short list items allowed (wire string `key-points`; the old `points`
    /// still loads).
    #[serde(alias = "points")]
    KeyPoints,
    /// Sentences that ask somebody to do something.
    ActionItems,
    /// Sentences that record a decision.
    Decisions,
    /// Sentences with a date, deadline or time (the phrase is returned too).
    Dates,
    /// Open questions and unresolved points.
    Questions,
}

/// The same enum under the name the other modes make more natural.
pub type SummaryMode = SummaryLength;

/// The pre-0.3 names, so existing callers (settings, the CLI example) keep compiling.
#[allow(non_upper_case_globals)]
impl SummaryLength {
    pub const Medium: SummaryLength = SummaryLength::Detailed;
    pub const Points: SummaryLength = SummaryLength::KeyPoints;

    /// Modes that filter by cue instead of ranking.
    pub fn is_extract(self) -> bool {
        matches!(
            self,
            SummaryLength::ActionItems
                | SummaryLength::Decisions
                | SummaryLength::Dates
                | SummaryLength::Questions
        )
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryItem {
    /// Index of the sentence in the input's sentence list (headings included), for stable keys.
    pub index: usize,
    pub text: String,
    /// Which section (0-based) the sentence belongs to.
    pub section: usize,
    /// Extraction modes: the phrase that matched (the date, "decided", "?", ...).
    pub hit: Option<String>,
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
    /// Ready to show: sentences joined by a space (short), paragraphs per section (detailed) or
    /// `\u{2022} ` lines (key points and the extraction modes).
    pub text: String,
    pub items: Vec<SummaryItem>,
    /// The input was too short to cut; `text` is the input itself.
    pub already_short: bool,
    pub note: Option<String>,
    pub stats: Stats,
    /// How many sections the text was divided into (headings, or paragraph chunks).
    pub sections: usize,
    /// How many of them contributed at least one sentence.
    pub sections_covered: usize,
    /// How many sentences were scored or scanned (headings and fragments excluded).
    pub sentences_considered: usize,
    /// `sections_covered / sections`, 0.0 to 1.0.
    pub coverage: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryError {
    Empty,
    TooLong {
        chars: usize,
        max: usize,
    },
    /// The cancel flag was raised while the summary was being worked out.
    Cancelled,
}

impl fmt::Display for SummaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SummaryError::Empty => write!(f, "Paste some text to summarize."),
            SummaryError::TooLong { chars, max } => write!(
                f,
                "That is {chars} characters; Krovi summarizes up to {max} at a time. Paste a smaller part."
            ),
            SummaryError::Cancelled => write!(f, "Summary cancelled."),
        }
    }
}
impl std::error::Error for SummaryError {}

/// Seconds to read `words` words.
pub fn read_seconds(words: usize) -> f64 {
    words as f64 / READ_WPM * 60.0
}

fn check(cancel: &AtomicBool) -> Result<(), SummaryError> {
    if cancel.load(Ordering::Relaxed) {
        Err(SummaryError::Cancelled)
    } else {
        Ok(())
    }
}

pub fn summarize(input: &str, length: SummaryLength) -> Result<Summary, SummaryError> {
    summarize_cancellable(input, length, &AtomicBool::new(false))
}

// ---------------------------------------------------------------------------------------
// Prepared text, candidates, sections
// ---------------------------------------------------------------------------------------

/// A sentence that may be chosen.
struct Cand {
    unit: usize,
    words: usize,
    para: usize,
    first_in_para: bool,
    bullet: bool,
}

/// A run of consecutive candidates `start..end` (indices into the candidate list).
struct Sec {
    start: usize,
    end: usize,
    /// The heading section this chunk was cut from (its context never crosses a heading).
    parent_start: usize,
    parent_end: usize,
    /// Unit index of the nearest heading above, if any.
    title: Option<usize>,
    words: usize,
}

fn candidates(
    units: &[Unit],
    clean: &[String],
    uwords: &[usize],
    min_prose: usize,
    min_bullet: usize,
) -> Vec<Cand> {
    units
        .iter()
        .enumerate()
        .filter_map(|(i, u)| {
            if u.kind == UnitKind::Heading || clean[i].is_empty() {
                return None;
            }
            let min = if u.kind == UnitKind::Bullet {
                min_bullet
            } else {
                min_prose
            };
            (uwords[i] >= min).then(|| Cand {
                unit: i,
                words: uwords[i],
                para: u.para,
                first_in_para: u.first_in_para,
                bullet: u.kind == UnitKind::Bullet,
            })
        })
        .collect()
}

/// Cut `a..b` into chunks of about [`SEC_TARGET`] candidates at paragraph boundaries.
fn chunk_ranges(cands: &[Cand], a: usize, b: usize) -> Vec<(usize, usize)> {
    if b - a <= SEC_MAX {
        return vec![(a, b)];
    }
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut start = a;
    for j in (a + 1)..b {
        let len = j - start;
        let boundary = cands[j].para != cands[j - 1].para;
        if (boundary && len >= SEC_TARGET) || len >= SEC_MAX {
            out.push((start, j));
            start = j;
        }
    }
    if b - start < SEC_MIN && !out.is_empty() {
        if let Some(last) = out.last_mut() {
            last.1 = b;
        }
    } else {
        out.push((start, b));
    }
    out
}

fn build_sections(units: &[Unit], cands: &[Cand]) -> Vec<Sec> {
    struct Group {
        start: usize,
        end: usize,
        title: Option<usize>,
    }
    let mut groups: Vec<Group> = Vec::new();
    let mut cur_start = 0usize;
    let mut cur_title: Option<usize> = None;
    let mut ci = 0usize;
    for (ui, u) in units.iter().enumerate() {
        if u.kind != UnitKind::Heading {
            continue;
        }
        while ci < cands.len() && cands[ci].unit < ui {
            ci += 1;
        }
        if ci > cur_start {
            groups.push(Group {
                start: cur_start,
                end: ci,
                title: cur_title,
            });
            cur_start = ci;
        }
        // nested headings with no text between them: the deepest one is the title
        cur_title = Some(ui);
    }
    if cands.len() > cur_start {
        groups.push(Group {
            start: cur_start,
            end: cands.len(),
            title: cur_title,
        });
    }
    let mut out: Vec<Sec> = Vec::new();
    for g in groups {
        for (s, e) in chunk_ranges(cands, g.start, g.end) {
            out.push(Sec {
                start: s,
                end: e,
                parent_start: g.start,
                parent_end: g.end,
                title: g.title,
                words: cands[s..e].iter().map(|c| c.words).sum(),
            });
        }
    }
    out
}

/// Which section each candidate belongs to.
fn section_of(cands: &[Cand], secs: &[Sec]) -> Vec<usize> {
    let mut v = vec![0usize; cands.len()];
    for (si, s) in secs.iter().enumerate() {
        for slot in &mut v[s.start..s.end] {
            *slot = si;
        }
    }
    v
}

// ---------------------------------------------------------------------------------------
// Budget
// ---------------------------------------------------------------------------------------

struct Plan {
    /// Sentences wanted.
    k: usize,
    /// Never trim below this many sentences when enforcing the word cap.
    k_min: usize,
    /// Soft word budget (Short stops once it is reached; the others trim down to `word_cap`).
    budget: usize,
    word_cap: usize,
    lambda: f64,
    /// Weight of the section-local score vs. the whole-document centroid.
    w_local: f64,
    w_glob: f64,
    /// Upper bound, as a share of all sentences, when a section guarantee needs more than `k`.
    hard_frac: f64,
}

/// Share of the sentences a Detailed summary keeps: 12% up to 100 sentences, easing to 5% at
/// 3 000 and beyond.
fn detail_share(n: usize) -> f64 {
    if n <= 100 {
        0.12
    } else {
        (0.12 - 0.07 * (n as f64 / 100.0).ln() / 30f64.ln()).max(0.05)
    }
}

fn plan(mode: SummaryLength, n: usize, words: usize) -> Plan {
    let w = words as f64;
    let nf = n as f64;
    let mut p = match mode {
        SummaryLength::Short => {
            let k = if n < 5 {
                1
            } else if n < 200 {
                2
            } else {
                3
            };
            Plan {
                k,
                k_min: 1,
                budget: if k == 3 {
                    100
                } else {
                    ((w * 0.45) as usize).clamp(15, 70)
                },
                word_cap: usize::MAX,
                lambda: 0.85,
                w_local: 0.4,
                w_glob: 0.6,
                hard_frac: 0.0,
            }
        }
        SummaryLength::KeyPoints => Plan {
            k: ((nf * detail_share(n) * 0.75).round() as usize).clamp(3, 70),
            k_min: 3,
            budget: 0,
            word_cap: ((w * 0.20) as usize).clamp(90, 1500),
            lambda: 0.65,
            w_local: 0.75,
            w_glob: 0.25,
            hard_frac: 0.09,
        },
        _ => Plan {
            k: ((nf * detail_share(n)).round() as usize).clamp(3, 90),
            k_min: 3,
            budget: 0,
            word_cap: ((w * 0.30) as usize).clamp(120, 2400),
            lambda: 0.70,
            w_local: 0.75,
            w_glob: 0.25,
            hard_frac: 0.12,
        },
    };
    // tiny inputs must really shrink, and the result is always shorter than the input
    p.k = p.k.min((3 * n / 5).max(1)).min(n.saturating_sub(1)).max(1);
    p.k_min = p.k_min.min(p.k);
    p
}

struct SecStat {
    cands: usize,
    words: usize,
    substantial: bool,
}

/// Split `k` sentences across sections: one for every substantial section first, the rest in
/// proportion to size (largest remainder), no section giving more than ~60% of its sentences.
/// When there are more substantial sections than `k`, the budget grows to cover them, up to
/// `hard`; beyond that the biggest sections are served first.
fn allocate(secs: &[SecStat], k: usize, hard: usize) -> Vec<usize> {
    let n = secs.len();
    let mut q = vec![0usize; n];
    let cap: Vec<usize> = secs.iter().map(|s| (s.cands * 3 / 5).max(1)).collect();
    let mut sub: Vec<usize> = (0..n).filter(|&i| secs[i].substantial).collect();
    let k_eff = if sub.len() > k {
        sub.len().min(hard.max(k))
    } else {
        k
    };
    if sub.len() > k_eff {
        sub.sort_by(|&a, &b| secs[b].words.cmp(&secs[a].words).then(a.cmp(&b)));
        sub.truncate(k_eff);
    }
    for &i in &sub {
        q[i] = 1;
    }
    let mut assigned = sub.len();
    loop {
        let remaining = k_eff.saturating_sub(assigned);
        if remaining == 0 {
            break;
        }
        let open: Vec<usize> = (0..n).filter(|&i| q[i] < cap[i]).collect();
        let wsum: f64 = open.iter().map(|&i| secs[i].words as f64).sum();
        if open.is_empty() || wsum <= 0.0 {
            break;
        }
        let mut gave = 0usize;
        let mut frac: Vec<(usize, f64)> = Vec::new();
        for &i in &open {
            let share = remaining as f64 * secs[i].words as f64 / wsum;
            let whole = (share.floor() as usize).min(cap[i] - q[i]);
            q[i] += whole;
            gave += whole;
            frac.push((i, share - share.floor()));
        }
        frac.sort_by(|a, b| {
            b.1.total_cmp(&a.1)
                .then(secs[b.0].words.cmp(&secs[a.0].words))
                .then(a.0.cmp(&b.0))
        });
        let mut left = remaining - gave;
        for &(i, _) in &frac {
            if left == 0 {
                break;
            }
            if q[i] < cap[i] {
                q[i] += 1;
                left -= 1;
                gave += 1;
            }
        }
        if gave == 0 {
            break;
        }
        assigned += gave;
    }
    q
}

// ---------------------------------------------------------------------------------------
// The summariser
// ---------------------------------------------------------------------------------------

/// Like [`summarize`], but stops with [`SummaryError::Cancelled`] soon after `cancel` is set.
/// The flag is checked between phases and between sections.
pub fn summarize_cancellable(
    input: &str,
    length: SummaryLength,
    cancel: &AtomicBool,
) -> Result<Summary, SummaryError> {
    if input.trim().is_empty() {
        return Err(SummaryError::Empty);
    }
    let chars = input.chars().count();
    if chars > MAX_CHARS {
        return Err(SummaryError::TooLong {
            chars,
            max: MAX_CHARS,
        });
    }
    check(cancel)?;

    let units = text::split_units(input);
    check(cancel)?;
    let words_total = text::word_count(input);
    let clean: Vec<String> = units
        .iter()
        .map(|u| {
            if u.kind == UnitKind::Heading {
                String::new()
            } else {
                text::display_clean(&u.text)
            }
        })
        .collect();
    let uwords: Vec<usize> = units.iter().map(|u| text::word_count(&u.text)).collect();
    let ctx = Ctx {
        units: &units,
        clean: &clean,
        uwords: &uwords,
        mode: length,
        chars,
        words: words_total,
    };

    if length.is_extract() {
        return extract(&ctx, cancel);
    }

    let min_bullet = if length == SummaryLength::KeyPoints {
        2
    } else {
        4
    };
    let mut cands = candidates(&units, &clean, &uwords, 4, min_bullet);
    if cands.len() < 2 {
        cands = candidates(&units, &clean, &uwords, 1, 1);
    }
    if words_total < SHORT_INPUT_WORDS || cands.len() < 2 {
        // Nothing worth cutting: hand the text back, say so.
        let mut chosen: Vec<usize> = (0..units.len())
            .filter(|&i| units[i].kind != UnitKind::Heading)
            .collect();
        if chosen.is_empty() {
            chosen = (0..units.len()).collect();
        }
        let items = chosen
            .iter()
            .filter_map(|&i| {
                let t = text::display_clean(&units[i].text);
                (!t.is_empty()).then_some(SummaryItem {
                    index: i,
                    text: t,
                    section: 0,
                    hit: None,
                })
            })
            .collect::<Vec<_>>();
        let text = items
            .iter()
            .map(|it| it.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let considered = items.len();
        return Ok(assemble(&ctx, items, text, true, None, 1, considered));
    }

    let secs = build_sections(&units, &cands);
    let sec_of = section_of(&cands, &secs);
    let n = cands.len();
    let p = plan(length, n, words_total);

    // ---- TF-IDF vectors (IDF over every candidate sentence) ---------------------------------
    let mut ids: HashMap<String, u32> = HashMap::new();
    let mut df: Vec<u32> = Vec::new();
    let mut counts: Vec<Vec<(u32, u32)>> = Vec::with_capacity(n);
    for c in &cands {
        let mut tf: HashMap<u32, u32> = HashMap::new();
        for t in text::content_terms(&units[c.unit].text) {
            let next = ids.len() as u32;
            let id = *ids.entry(t).or_insert(next);
            if id as usize >= df.len() {
                df.push(0);
            }
            *tf.entry(id).or_insert(0) += 1;
        }
        for id in tf.keys() {
            df[*id as usize] += 1;
        }
        let mut v: Vec<(u32, u32)> = tf.into_iter().collect();
        v.sort_unstable_by_key(|x| x.0);
        counts.push(v);
    }
    check(cancel)?;
    let nf = n as f64;
    let idf: Vec<f64> = df
        .iter()
        .map(|&d| ((nf + 1.0) / (d as f64 + 0.5)).ln().max(0.0) + 0.15)
        .collect();
    let vecs: Vec<Vec<(u32, f64)>> = counts
        .iter()
        .map(|c| {
            let mut v: Vec<(u32, f64)> = c
                .iter()
                .map(|&(id, tf)| (id, (1.0 + (tf as f64).ln()) * idf[id as usize]))
                .collect();
            let norm = v.iter().map(|x| x.1 * x.1).sum::<f64>().sqrt();
            if norm > 0.0 {
                for x in &mut v {
                    x.1 /= norm;
                }
            }
            v
        })
        .collect();
    let vocab = ids.len();

    // Heading words lift the sentences that share them: the section's own heading a little more
    // than the document's first heading.
    let title_set = |unit: Option<usize>| -> HashSet<u32> {
        unit.map(|u| {
            text::content_terms(&units[u].text)
                .iter()
                .filter_map(|t| ids.get(t).copied())
                .collect()
        })
        .unwrap_or_default()
    };
    let doc_title = title_set(
        units
            .first()
            .filter(|u| u.kind == UnitKind::Heading)
            .map(|_| 0),
    );
    let sec_titles: Vec<HashSet<u32>> = secs.iter().map(|s| title_set(s.title)).collect();

    // ---- document centroid ---------------------------------------------------------------
    let mut centroid = vec![0f64; vocab];
    for v in &vecs {
        for &(id, w) in v {
            centroid[id as usize] += w;
        }
    }
    let cnorm = centroid.iter().map(|x| x * x).sum::<f64>().sqrt();
    if cnorm > 0.0 {
        for x in &mut centroid {
            *x /= cnorm;
        }
    }
    let doc_sim: Vec<f64> = vecs
        .iter()
        .map(|v| v.iter().map(|&(id, w)| w * centroid[id as usize]).sum())
        .collect();
    let gmax = doc_sim.iter().copied().fold(0f64, f64::max);

    // ---- sections: how many sentences each gets -------------------------------------------------
    let mut sorted_words: Vec<usize> = secs.iter().map(|s| s.words).collect();
    sorted_words.sort_unstable();
    let median = sorted_words[sorted_words.len() / 2];
    let threshold = (median / 3).clamp(12, 40);
    let stats: Vec<SecStat> = secs
        .iter()
        .map(|s| SecStat {
            cands: s.end - s.start,
            words: s.words,
            substantial: s.end - s.start >= 2 && s.words >= threshold,
        })
        .collect();
    // Covering every substantial section may need more than `k`: up to 12% (9% for key points)
    // of all sentences, and always enough for a dozen sections.
    let hard = ((p.hard_frac * nf).ceil() as usize).max(12);
    let quota = if length == SummaryLength::Short {
        vec![0usize; secs.len()]
    } else {
        allocate(&stats, p.k, hard)
    };

    // ---- per-section ranking ---------------------------------------------------------------
    let mut raw = vec![0f64; n];
    let mut orders: Vec<Vec<usize>> = Vec::with_capacity(secs.len());
    let mut scratch = vec![0f64; vocab];
    for (si, sec) in secs.iter().enumerate() {
        check(cancel)?;
        let (a, b) = (sec.start, sec.end);
        let ca = a.saturating_sub(OVERLAP).max(sec.parent_start);
        let cb = (b + OVERLAP).min(sec.parent_end);
        let m = cb - ca;
        let off = a - ca;
        let core = b - a;

        let mut sim = vec![0f64; m * m];
        for x in 0..m {
            for y in (x + 1)..m {
                let s = dot(&vecs[ca + x], &vecs[ca + y]);
                sim[x * m + y] = s;
                sim[y * m + x] = s;
            }
        }
        let rank = text_rank(&sim, m);

        // section centroid (core plus context)
        let mut touched: Vec<u32> = Vec::new();
        for v in &vecs[ca..cb] {
            for &(id, w) in v {
                if scratch[id as usize] == 0.0 {
                    touched.push(id);
                }
                scratch[id as usize] += w;
            }
        }
        let snorm = touched
            .iter()
            .map(|&id| scratch[id as usize] * scratch[id as usize])
            .sum::<f64>()
            .sqrt();
        let cent: Vec<f64> = (0..core)
            .map(|t| {
                let s: f64 = vecs[a + t]
                    .iter()
                    .map(|&(id, w)| w * scratch[id as usize])
                    .sum();
                if snorm > 0.0 {
                    s / snorm
                } else {
                    0.0
                }
            })
            .collect();
        for id in touched {
            scratch[id as usize] = 0.0;
        }

        let rmax = rank[off..off + core].iter().copied().fold(0f64, f64::max);
        let cmax = cent.iter().copied().fold(0f64, f64::max);
        let sec_title = &sec_titles[si];
        for t in 0..core {
            let c = a + t;
            let rn = if rmax > 0.0 {
                rank[off + t] / rmax
            } else {
                1.0
            };
            let cn = if cmax > 0.0 { cent[t] / cmax } else { 1.0 };
            let local = 0.5 * rn + 0.5 * cn;
            let glob = if gmax > 0.0 { doc_sim[c] / gmax } else { 1.0 };
            let base = p.w_local * local + p.w_glob * glob;
            let cand = &cands[c];
            let u = &units[cand.unit];
            let title = 1.0
                + 0.22 * overlap_share(&vecs[c], sec_title)
                + 0.08 * overlap_share(&vecs[c], &doc_title);
            raw[c] = base
                * position_prior(t, core, c, cand.first_in_para)
                * length_prior(cand.words, length, cand.bullet)
                * cue_prior(&u.text)
                * title;
        }
        let smax = raw[a..b].iter().copied().fold(0f64, f64::max);
        let ss: Vec<f64> = (a..b)
            .map(|c| if smax > 0.0 { raw[c] / smax } else { 0.0 })
            .collect();

        // MMR inside the section: a few more than the quota, as replacements for duplicates
        let limit = if length == SummaryLength::Short {
            4.min(core)
        } else {
            (quota[si] + 3).min(core)
        };
        let mut order: Vec<usize> = Vec::new();
        let mut red = vec![0f64; core];
        let mut taken = vec![false; core];
        while order.len() < limit {
            let mut best: Option<(usize, f64)> = None;
            for t in 0..core {
                if taken[t] || red[t] >= DUP {
                    continue;
                }
                let gain = p.lambda * ss[t] - (1.0 - p.lambda) * red[t];
                if best.is_none_or(|(_, g)| gain > g + 1e-9) {
                    best = Some((t, gain));
                }
            }
            let Some((t, _)) = best else { break };
            taken[t] = true;
            order.push(a + t);
            for u in 0..core {
                if !taken[u] {
                    red[u] = red[u].max(sim[(off + t) * m + (off + u)]);
                }
            }
        }
        orders.push(order);
    }
    check(cancel)?;

    // ---- merge the sections ------------------------------------------------------------------
    let mut kept: Vec<usize> = Vec::new();
    if length == SummaryLength::Short {
        // a global MMR over each section's best few
        let pool: Vec<usize> = orders.iter().flatten().copied().collect();
        let gmax_raw = raw.iter().copied().fold(0f64, f64::max);
        let score = |c: usize| {
            if gmax_raw > 0.0 {
                raw[c] / gmax_raw
            } else {
                0.0
            }
        };
        let max_words = p.budget * 3 / 2;
        let mut red = vec![0f64; pool.len()];
        let mut taken = vec![false; pool.len()];
        let mut sel_words = 0usize;
        while kept.len() < p.k {
            let mut best: Option<(usize, f64)> = None;
            for (pi, &c) in pool.iter().enumerate() {
                if taken[pi] || red[pi] >= DUP {
                    continue;
                }
                if !kept.is_empty() && sel_words + cands[c].words > max_words {
                    continue;
                }
                let gain = p.lambda * score(c) - (1.0 - p.lambda) * red[pi];
                if best.is_none_or(|(_, g)| gain > g + 1e-9) {
                    best = Some((pi, gain));
                }
            }
            let Some((pi, _)) = best else { break };
            taken[pi] = true;
            kept.push(pool[pi]);
            sel_words += cands[pool[pi]].words;
            for (qi, &c) in pool.iter().enumerate() {
                if !taken[qi] {
                    red[qi] = red[qi].max(dot(&vecs[pool[pi]], &vecs[c]));
                }
            }
            if kept.len() >= p.k_min && sel_words >= p.budget {
                break;
            }
        }
    } else {
        let mut got = vec![0usize; secs.len()];
        for si in 0..secs.len() {
            for &c in &orders[si] {
                if got[si] >= quota[si] {
                    break;
                }
                if kept.iter().any(|&k| dot(&vecs[k], &vecs[c]) >= DUP) {
                    continue; // the same point was already made in an earlier section
                }
                kept.push(c);
                got[si] += 1;
            }
        }
        // keep the whole under the word cap, never dropping a section's only sentence
        let mut total: usize = kept.iter().map(|&c| cands[c].words).sum();
        while total > p.word_cap && kept.len() > p.k_min {
            let mut per = vec![0usize; secs.len()];
            for &c in &kept {
                per[sec_of[c]] += 1;
            }
            let mut victim: Option<usize> = None;
            for (pos, &c) in kept.iter().enumerate() {
                if per[sec_of[c]] < 2 {
                    continue;
                }
                let better = match victim {
                    None => true,
                    Some(v) => {
                        let o = kept[v];
                        raw[c] < raw[o] || (raw[c] == raw[o] && c > o)
                    }
                };
                if better {
                    victim = Some(pos);
                }
            }
            let Some(v) = victim else { break };
            total -= cands[kept[v]].words;
            kept.remove(v);
        }
    }
    kept.sort_unstable();

    let items: Vec<SummaryItem> = kept
        .iter()
        .map(|&c| SummaryItem {
            index: cands[c].unit,
            text: clean[cands[c].unit].clone(),
            section: sec_of[c],
            hit: None,
        })
        .collect();
    let text = match length {
        SummaryLength::KeyPoints => bullets(&items),
        SummaryLength::Short => items
            .iter()
            .map(|it| it.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        _ => by_section(&items),
    };
    Ok(assemble(&ctx, items, text, false, None, secs.len(), n))
}

/// Everything about the input the later steps need.
struct Ctx<'a> {
    units: &'a [Unit],
    clean: &'a [String],
    uwords: &'a [usize],
    mode: SummaryLength,
    chars: usize,
    words: usize,
}

fn bullets(items: &[SummaryItem]) -> String {
    items
        .iter()
        .map(|it| format!("\u{2022} {}", it.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One paragraph per section that contributed.
fn by_section(items: &[SummaryItem]) -> String {
    let mut out = String::new();
    let mut prev: Option<usize> = None;
    for it in items {
        match prev {
            None => {}
            Some(s) if s == it.section => out.push(' '),
            Some(_) => out.push_str("\n\n"),
        }
        out.push_str(&it.text);
        prev = Some(it.section);
    }
    out
}

fn assemble(
    ctx: &Ctx,
    items: Vec<SummaryItem>,
    text: String,
    already_short: bool,
    note: Option<String>,
    sections: usize,
    sentences_considered: usize,
) -> Summary {
    let summary_words = text::word_count(&text);
    let read = read_seconds(ctx.words);
    let sread = read_seconds(summary_words);
    let covered = items
        .iter()
        .map(|i| i.section)
        .collect::<HashSet<_>>()
        .len();
    let note = if already_short {
        Some("This is already short, so here it is as written. Paste something longer to see a summary.".to_string())
    } else {
        note
    };
    Summary {
        length: ctx.mode,
        stats: Stats {
            chars: ctx.chars,
            words: ctx.words,
            sentences: ctx.units.len(),
            summary_words,
            summary_sentences: items.len(),
            read_secs: read,
            summary_read_secs: sread,
            saved_secs: (read - sread).max(0.0),
            ratio: if ctx.words == 0 {
                1.0
            } else {
                (summary_words as f64 / ctx.words as f64).min(1.0)
            },
        },
        text,
        items,
        already_short,
        note,
        sections,
        sections_covered: covered,
        sentences_considered,
        coverage: if sections == 0 {
            0.0
        } else {
            covered as f64 / sections as f64
        },
    }
}

// ---------------------------------------------------------------------------------------
// Extraction modes
// ---------------------------------------------------------------------------------------

fn extract(ctx: &Ctx, cancel: &AtomicBool) -> Result<Summary, SummaryError> {
    let cands = candidates(ctx.units, ctx.clean, ctx.uwords, 2, 2);
    let secs = build_sections(ctx.units, &cands);
    let sec_of = section_of(&cands, &secs);
    let mode = ctx.mode;

    // (candidate, strength, phrase that matched)
    let mut found: Vec<(usize, u8, String)> = Vec::new();
    for sec in &secs {
        check(cancel)?;
        for c in sec.start..sec.end {
            let u = &ctx.units[cands[c].unit];
            let hit: Option<(u8, String)> = match mode {
                SummaryLength::ActionItems => text::action_cue(&u.text, cands[c].bullet),
                SummaryLength::Decisions => text::decision_cue(&u.text).map(|h| (2, h)),
                SummaryLength::Questions => {
                    text::question_cue(&u.text).map(|h| (if h == "?" { 3 } else { 2 }, h))
                }
                _ => {
                    let d = text::find_dates(&u.text);
                    (!d.is_empty()).then(|| ((d.len() as u8 + 1).min(3), d.join(", ")))
                }
            };
            if let Some((strength, h)) = hit {
                found.push((c, strength, h));
            }
        }
    }
    check(cancel)?;
    let total = found.len();

    // strongest first, then document order; near-duplicates and the cap drop the rest
    let mut order: Vec<usize> = (0..found.len()).collect();
    order.sort_by(|&x, &y| found[y].1.cmp(&found[x].1).then(x.cmp(&y)));
    let mut ids: HashMap<String, u32> = HashMap::new();
    let mvec: Vec<Vec<(u32, f64)>> = found
        .iter()
        .map(|(c, _, _)| {
            let mut tf: HashMap<u32, u32> = HashMap::new();
            for t in text::content_terms(&ctx.units[cands[*c].unit].text) {
                let next = ids.len() as u32;
                let id = *ids.entry(t).or_insert(next);
                *tf.entry(id).or_insert(0) += 1;
            }
            let mut v: Vec<(u32, f64)> = tf
                .into_iter()
                .map(|(id, n)| (id, 1.0 + (n as f64).ln()))
                .collect();
            v.sort_unstable_by_key(|x| x.0);
            let norm = v.iter().map(|x| x.1 * x.1).sum::<f64>().sqrt();
            if norm > 0.0 {
                for x in &mut v {
                    x.1 /= norm;
                }
            }
            v
        })
        .collect();
    let mut kept: Vec<usize> = Vec::new();
    let mut capped = false;
    for &f in &order {
        if kept.len() >= MAX_MATCHES {
            capped = true;
            break;
        }
        if kept.iter().any(|&k| dot(&mvec[k], &mvec[f]) >= 0.85) {
            continue;
        }
        kept.push(f);
    }
    kept.sort_unstable();

    let items: Vec<SummaryItem> = kept
        .iter()
        .map(|&f| {
            let (c, _, h) = &found[f];
            let unit = cands[*c].unit;
            let mut t = ctx.clean[unit].as_str();
            if mode == SummaryLength::ActionItems {
                t = t[text::checkbox_len(t)..].trim_start();
            }
            SummaryItem {
                index: unit,
                text: t.to_string(),
                section: sec_of[*c],
                hit: Some(h.clone()),
            }
        })
        .collect();
    let text = if mode == SummaryLength::Dates {
        items
            .iter()
            .map(|it| format!("\u{2022} {}: {}", it.hit.as_deref().unwrap_or(""), it.text))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        bullets(&items)
    };
    let note = if items.is_empty() {
        Some(
            match mode {
                SummaryLength::ActionItems => "No action items found in this text.",
                SummaryLength::Decisions => "No decisions found in this text.",
                SummaryLength::Dates => "No dates or deadlines found in this text.",
                _ => "No open questions found in this text.",
            }
            .to_string(),
        )
    } else if capped {
        Some(format!(
            "Showing the {} strongest of {total} matches.",
            items.len()
        ))
    } else {
        None
    };
    Ok(assemble(
        ctx,
        items,
        text,
        false,
        note,
        secs.len(),
        cands.len(),
    ))
}

// ---------------------------------------------------------------------------------------
// Scoring pieces
// ---------------------------------------------------------------------------------------

fn dot(a: &[(u32, f64)], b: &[(u32, f64)]) -> f64 {
    let (mut i, mut j, mut s) = (0, 0, 0f64);
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

/// Share of the heading's words (that occur in the text at all) found in this sentence.
fn overlap_share(v: &[(u32, f64)], title: &HashSet<u32>) -> f64 {
    if title.is_empty() {
        return 0.0;
    }
    let hit = v.iter().filter(|(id, _)| title.contains(id)).count() as f64;
    (hit / title.len() as f64).min(1.0)
}

/// TextRank by power iteration over the thresholded similarity graph, scaled to max = 1.
fn text_rank(sim: &[f64], m: usize) -> Vec<f64> {
    const THRESH: f64 = 0.05;
    const D: f64 = 0.85;
    if m == 0 {
        return Vec::new();
    }
    let mut adj: Vec<Vec<(usize, f64)>> = vec![Vec::new(); m];
    let mut out_sum = vec![0f64; m];
    for a in 0..m {
        for b in 0..m {
            let s = sim[a * m + b];
            if a != b && s >= THRESH {
                adj[a].push((b, s));
                out_sum[a] += s;
            }
        }
    }
    let mut r = vec![1.0 / m as f64; m];
    for _ in 0..80 {
        let dangling: f64 = (0..m).filter(|&j| out_sum[j] == 0.0).map(|j| r[j]).sum();
        let mut next = vec![(1.0 - D) / m as f64 + D * dangling / m as f64; m];
        for a in 0..m {
            let mut acc = 0f64;
            for &(j, w) in &adj[a] {
                acc += w / out_sum[j] * r[j];
            }
            next[a] += D * acc;
        }
        let delta: f64 = next.iter().zip(&r).map(|(x, y)| (x - y).abs()).sum();
        r = next;
        if delta < 1e-9 {
            break;
        }
    }
    let max = r.iter().copied().fold(0f64, f64::max);
    if max > 0.0 {
        for x in &mut r {
            *x /= max;
        }
    }
    r
}

/// A mild lead bonus: the first sentences of a section and of the document tend to carry the
/// topic, but a strong sentence further down must still be able to win.
fn position_prior(p_sec: usize, sec_len: usize, p_doc: usize, first_in_para: bool) -> f64 {
    let lead_sec = 1.0 + 0.12 * (-(p_sec as f64) / 2.0).exp();
    let lead_doc = 1.0 + 0.10 * (-(p_doc as f64) / 3.0).exp();
    let para = if first_in_para { 1.04 } else { 1.0 };
    let last = if p_sec + 1 == sec_len && sec_len > 3 {
        1.03
    } else {
        1.0
    };
    lead_sec * lead_doc * para * last
}

/// Sentences of 9-35 words read best; fragments and run-ons are down-weighted. List items in
/// Key points are meant to be short, so they are only mildly penalised below 3 words.
fn length_prior(words: usize, mode: SummaryLength, bullet: bool) -> f64 {
    let short_ok = bullet && mode == SummaryLength::KeyPoints;
    let (lo, hi, hard) = match (mode, short_ok) {
        (SummaryLength::KeyPoints, true) => (3.0, 28.0, 70.0),
        (SummaryLength::KeyPoints, false) => (8.0, 28.0, 70.0),
        _ => (9.0, 35.0, 90.0),
    };
    let floor = if short_ok { 2.0 } else { 5.0 };
    let w = words as f64;
    if w < floor {
        0.5
    } else if w < lo {
        0.6 + 0.4 * (w - floor) / (lo - floor)
    } else if w <= hi {
        1.0
    } else if w < hard {
        1.0 - 0.35 * (w - hi) / (hard - hi)
    } else {
        0.6
    }
}

fn cue_prior(sentence: &str) -> f64 {
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
        // Hindi / Hinglish cue phrases (lowercase matching; Devanagari has no case)
        "निष्कर्ष",
        "सारांश",
        "मुख्य बात",
        "इसका मतलब",
        "इसलिए",
        "matlab",
        "yaani",
        "isliye",
        // Chinese
        "总之",
        "综上",
        "结论",
        "因此",
        "重要的是",
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
        "जैसे",
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
    let mut f = 1.0f64;
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

    const SHORT: SummaryLength = SummaryLength::Short;
    const DETAILED: SummaryLength = SummaryLength::Detailed;
    const POINTS: SummaryLength = SummaryLength::KeyPoints;

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

    // ---- fixtures built in code -----------------------------------------------------------

    /// Six headed sections about unrelated things, five sentences each.
    const TOPICS: &str = "## Coffee roasting\n\n\
Roasting turns green coffee beans brown and brings out the flavours locked inside them. \
A light roast keeps the bright acidity of the bean while a dark roast tastes smoky and bitter. \
The roaster listens for the first crack, a popping sound that tells the roaster the beans are drying out. \
Freshly roasted coffee needs a few days to release carbon dioxide before it brews well. \
Home roasters often use a popcorn machine or a small drum roaster to practise.\n\n\
## Comet observation\n\n\
Comets are balls of ice and dust that grow glowing tails when they approach the sun. \
Astronomers track each comet by measuring its orbit across several nights of observation. \
A telescope with a wide field of view shows the faint tail of a comet far better than a narrow one. \
Some comets return every few decades while others will not be seen again for thousands of years. \
Photographing a comet needs a steady tripod and an exposure of several seconds.\n\n\
## Vegetable gardening\n\n\
Tomato seedlings should be planted outdoors only after the last frost has passed. \
Compost mixed into the soil feeds the roots and helps the garden hold moisture through summer. \
Gardeners water deeply in the morning so that leaves dry out before evening and fungus stays away. \
Beans fix nitrogen in the soil, which is why they are planted beside hungry crops like corn. \
Mulch around each plant suppresses weeds and keeps the soil cool.\n\n\
## Railway signalling\n\n\
Early railways used flags and lamps to tell drivers whether the line ahead was clear. \
Block signalling divides the track into sections so that only one train occupies a section at a time. \
Modern interlocking systems prevent a signaller from setting two conflicting routes by mistake. \
Track circuits detect a train by the short circuit its wheels make between the two rails. \
A failed signal defaults to red so that every fault leaves the railway in a safe state.\n\n\
## Chess endgames\n\n\
A rook and king against a lone king is a forced win that every serious player should know. \
The opposition decides most pawn endgames, because the king that holds it can usually escape or advance. \
Bishops of opposite colours often draw even when one side is two pawns ahead. \
Practising the Lucena position shows how a rook builds a bridge to shelter the king. \
Time pressure ruins many technically won endgames in club tournaments.\n\n\
## Volcano monitoring\n\n\
Seismometers on the flanks of a volcano record the small earthquakes that rising magma causes. \
Scientists also measure sulphur dioxide in the gas plume because a sudden increase often comes before an eruption. \
Satellites detect centimetres of swelling in the ground that no hiker would notice. \
Warning levels tell nearby villages when to prepare and when to leave. \
Past eruptions preserved in ash layers help forecasters judge what a volcano may do next.\n";

    /// Meeting notes: actions, decisions, dates and questions in one place.
    const MEETING: &str = "Weekly sync notes\n\n\
Attendees: Dana, Omar, Priya and Lee.\n\n\
We decided to move the launch from March to April because the payment provider certificate is delayed. \
Omar agreed to keep the old pricing page until the launch. \
The team approved the new onboarding flow after a short demo.\n\n\
Action items\n\
- Dana to send the revised launch plan to the partners by Friday.\n\
- [ ] Omar will update the pricing page before the April launch.\n\
- Priya needs to confirm the certificate renewal date with the provider.\n\
- Review the onboarding copy with legal.\n\
- Remember to book the venue for the April 14 launch party.\n\n\
Open questions\n\
Who will own the support rota after launch? The budget for paid ads is still TBD. It is unclear whether the mobile release can wait.\n\n\
Next meeting is on Tuesday at 3 pm. The final go/no-go call happens on 2026-04-10.\n";

    /// Pseudo-words so topics never overlap: deterministic, no dictionary needed.
    fn word(seed: u64) -> String {
        const SYL: [&str; 12] = [
            "ka", "to", "mi", "ra", "su", "ne", "vo", "li", "pa", "du", "ze", "fo",
        ];
        let mut x = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let mut w = String::new();
        for _ in 0..4 {
            w.push_str(SYL[((x >> 33) % 12) as usize]);
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
        }
        w
    }

    /// `sections` headed sections of about `sentences` sentences each, `per_para` sentences per
    /// paragraph. Each section has its own ten topic words; all share a filler vocabulary.
    fn synthetic(sections: usize, sentences: usize, per_para: usize) -> (String, Vec<Vec<String>>) {
        const FILLER: [&str; 24] = [
            "system",
            "process",
            "result",
            "often",
            "during",
            "several",
            "important",
            "measure",
            "changes",
            "usually",
            "between",
            "because",
            "records",
            "support",
            "history",
            "pattern",
            "careful",
            "complete",
            "regular",
            "overall",
            "needed",
            "common",
            "recent",
            "basic",
        ];
        let mut doc = String::new();
        let mut topics: Vec<Vec<String>> = Vec::new();
        let mut rng = 12345u64;
        let mut next = |m: usize| {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((rng >> 33) as usize) % m
        };
        for s in 0..sections {
            let tw: Vec<String> = (0..10).map(|j| word((s * 100 + j) as u64 + 7)).collect();
            doc.push_str(&format!("## Chapter {s} on {}\n\n", tw[0]));
            for i in 0..sentences {
                let len = 14 + next(9);
                let mut parts: Vec<String> = Vec::new();
                for _ in 0..len {
                    if next(10) < 4 {
                        parts.push(tw[next(10)].clone());
                    } else {
                        parts.push(FILLER[next(24)].to_string());
                    }
                }
                let mut sentence = parts.join(" ");
                sentence.replace_range(..1, &sentence[..1].to_uppercase());
                doc.push_str(&sentence);
                doc.push('.');
                doc.push_str(if (i + 1) % per_para == 0 { "\n\n" } else { " " });
            }
            topics.push(tw);
        }
        (doc, topics)
    }

    // ---- the original contract, kept --------------------------------------------------------

    #[test]
    fn real_texts_are_summarised_in_every_mode() {
        for (name, input) in all() {
            let words = text::word_count(input);
            for len in [SHORT, DETAILED, POINTS] {
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
            let short = summarize(input, SHORT).unwrap();
            let medium = summarize(input, DETAILED).unwrap();
            let points = summarize(input, POINTS).unwrap();
            assert!(
                (1..=2).contains(&short.items.len()),
                "{name}: short has {}",
                short.items.len()
            );
            assert!(
                (3..=7).contains(&medium.items.len()),
                "{name}: detailed has {}",
                medium.items.len()
            );
            assert!(
                (3..=8).contains(&points.items.len()),
                "{name}: key points has {}",
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
        let s = summarize(LEGAL, DETAILED).unwrap();
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
        let s = summarize(TECH, DETAILED).unwrap();
        let t = s.text.to_lowercase();
        // the defining sentence ("A hash map stores key and value pairs ...") must survive
        assert!(t.contains("hash map stores key and value pairs"), "{t}");
        assert!(t.contains("hash function") || t.contains("bucket"), "{t}");
    }

    #[test]
    fn news_summary_keeps_the_lead() {
        let s = summarize(NEWS, SHORT).unwrap();
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

    /// The lead bonus was 1.5x; it is now a gentle nudge so a central sentence further down
    /// can still win (see `a_central_sentence_beats_the_lead`).
    #[test]
    fn the_opening_sentence_is_favoured_only_mildly() {
        assert!(position_prior(0, 10, 0, true) > position_prior(4, 10, 4, false));
        let lead = position_prior(0, 10, 0, true);
        assert!((1.15..1.35).contains(&lead), "{lead}");
    }

    #[test]
    fn a_central_sentence_beats_the_lead() {
        let input = "Welcome to our newsletter, thanks for reading it today. \
                     The glacier retreated four hundred metres because warming accelerated glacier melt across the glacier basin. \
                     Meltwater from the retreating glacier now feeds a new glacier lake in the basin. \
                     Researchers measured the glacier retreat and the melt rate with the same glacier instruments. \
                     Our office will be closed on Monday for the holiday. \
                     Glacier retreat and melt in the basin are expected to continue as warming continues.";
        let s = summarize(input, SHORT).unwrap();
        assert!(
            !s.items[0].text.starts_with("Welcome"),
            "the greeting should not win: {:?}",
            s.items
        );
    }

    #[test]
    fn near_duplicate_sentences_are_not_both_chosen() {
        let dup = "The committee approved the new budget for the coming fiscal year on Tuesday. \
                   On Tuesday the committee approved the new budget for the coming fiscal year. \
                   Rainfall across the northern region was well below the seasonal average this spring. \
                   Farmers there have started rationing irrigation water for their remaining crops. \
                   The local museum will reopen next month after two years of renovation work. \
                   Visitors can expect a new wing devoted to regional history and modern art.";
        let s = summarize(dup, DETAILED).unwrap();
        let budget_hits = s
            .items
            .iter()
            .filter(|i| i.text.to_lowercase().contains("budget"))
            .count();
        assert!(budget_hits <= 1, "{:?}", s.items);
    }

    #[test]
    fn short_input_is_returned_as_is_with_a_note() {
        let s = summarize("Just a quick note. Meeting moved to 3 pm.", SHORT).unwrap();
        assert!(s.already_short);
        assert!(s.note.is_some());
        assert_eq!(s.text, "Just a quick note. Meeting moved to 3 pm.");
        assert_eq!(s.stats.saved_secs, 0.0);
        assert_eq!((s.sections, s.sections_covered, s.coverage), (1, 1, 1.0));
        let one = summarize("Itis", POINTS).unwrap();
        assert!(one.already_short);
    }

    /// The old 45-word floor made Medium return the input's first sentences nearly verbatim;
    /// a 60-word e-mail must now actually be cut.
    #[test]
    fn a_short_email_is_actually_compressed() {
        let input = "The vendor pushed the delivery date again. \
                     They now claim the parts ship on Friday, but they said that last week too. \
                     If anything is still broken, tell me by Thursday noon. \
                     I will be travelling and slow to answer after that.";
        let words = text::word_count(input);
        assert!(words >= 40 && words <= 70, "{words}");
        let s = summarize(input, DETAILED).unwrap();
        assert!(!s.already_short);
        assert!(
            s.items.len() < 4,
            "detailed must drop at least one sentence: {:?}",
            s.items
        );
        assert!(s.stats.ratio <= 0.8);
        let short = summarize(input, SHORT).unwrap();
        assert!(!short.already_short);
        assert_eq!(short.items.len(), 1);
        assert!(
            short.stats.ratio <= 0.5,
            "short must cut hard, got {} of {words}",
            short.stats.summary_words
        );
    }

    /// Two long sentences used to hit the "< 3 sentences" echo path and come back verbatim.
    #[test]
    fn a_two_sentence_input_is_summarized_not_echoed() {
        let input = "The rollout plan looked safe on paper because every step had an owner and a rollback path. \
                     In practice the database migration locked the orders table for two hours and nobody could check out.";
        let s = summarize(input, DETAILED).unwrap();
        assert!(!s.already_short, "{:?}", s.items);
        assert_eq!(s.items.len(), 1);
        assert!(s.stats.ratio < 0.9);
    }

    /// Devanagari uses danda, not full stops; without it every Hindi text was one unit and
    /// came back "already short".
    #[test]
    fn hindi_text_is_summarized() {
        let input = "क्रोवी एक हल्का विंडोज़ ऐप है। यह शेल्फ पर छोटे विजेट रखता है। \
                     क्लिपबोर्ड, टाइमर और नोट्स सब एक जगह मिलते हैं। \
                     सब कुछ आपके कंप्यूटर पर ही चलता है। \
                     कोई भी डेटा क्लाउड पर नहीं जाता। \
                     आवाज़ से नोट बनाना भी संभव है। \
                     राइट-क्लिक से कुछ भी शेल्फ पर भेजें।";
        let s = summarize(input, DETAILED).unwrap();
        assert!(!s.already_short, "{:?}", s.items);
        assert!(s.stats.ratio < 0.85, "ratio {}", s.stats.ratio);
        assert!(s.items.len() >= 2);
    }

    /// Markdown pasted into the box used to come out with raw **, #### and table pipes.
    #[test]
    fn markdown_noise_is_cleaned_from_output() {
        let input = "#### Krovi tools\n\n\
                     The **clipboard** shelf keeps the last forty clips. `Ctrl+Shift+V` opens the stack. \
                     Timers live on the shelf and finish with a sound. \
                     The pomodoro ring counts focus sessions per day. \
                     Notes support checklists and voice input. \
                     The calculator remembers the last expression. \n\n\
                     | Tool | Status |\n| --- | --- |\n| Clipboard | ready |\n\n\
                     Everything runs locally on your machine. No account is needed for any of it.";
        let s = summarize(input, DETAILED).unwrap();
        assert!(
            !s.text.contains('*') && !s.text.contains('#') && !s.text.contains('`'),
            "{}",
            s.text
        );
        assert!(!s.text.contains("---"), "{}", s.text);
        assert!(!s.text.contains('|'), "{}", s.text);
        for it in &s.items {
            assert!(
                !it.text.starts_with(|c: char| c == '|' || c == '-'),
                "{it:?}"
            );
        }
    }

    #[test]
    fn empty_and_oversized_input_are_explicit_errors() {
        assert_eq!(summarize("   \n ", SHORT).unwrap_err(), SummaryError::Empty);
        let big = "word ".repeat(MAX_CHARS / 5 + 10);
        match summarize(&big, SHORT) {
            Err(SummaryError::TooLong { max, chars }) => {
                assert_eq!(max, MAX_CHARS);
                assert!(chars > MAX_CHARS);
            }
            other => panic!("expected TooLong, got {other:?}"),
        }
        let msg = SummaryError::TooLong {
            chars: 2_000_000,
            max: MAX_CHARS,
        }
        .to_string();
        assert!(msg.contains("2000000") && msg.contains("1000000"), "{msg}");
    }

    #[test]
    fn text_without_sentence_punctuation_does_not_panic() {
        let blob = "alpha beta gamma delta epsilon ".repeat(60);
        let s = summarize(&blob, DETAILED).unwrap();
        assert!(s.stats.chars > 0);
    }

    #[test]
    fn deterministic() {
        for (_, input) in all() {
            let a = summarize(input, DETAILED).unwrap();
            let b = summarize(input, DETAILED).unwrap();
            assert_eq!(a.text, b.text);
        }
        let (doc, _) = synthetic(8, 30, 5);
        assert_eq!(
            summarize(&doc, DETAILED).unwrap().text,
            summarize(&doc, DETAILED).unwrap().text
        );
    }

    #[test]
    fn reading_time_saved_adds_up() {
        let s = summarize(NEWS, SHORT).unwrap();
        let st = &s.stats;
        assert!((st.read_secs - read_seconds(st.words)).abs() < 1e-6);
        assert!((st.saved_secs - (st.read_secs - st.summary_read_secs)).abs() < 1e-6);
        assert_eq!(st.summary_sentences, s.items.len());
    }

    #[test]
    fn bullet_lists_summarise_to_bullets_of_the_same_items() {
        let input = "Project update\n\n- The migration to the new database finished two days ahead of schedule with zero downtime for customers.\n\
- Support tickets about login problems fell by sixty percent after the authentication service was rewritten.\n\
- The mobile app release is blocked on a payment provider certificate that expires at the end of the month.\n\
- Hiring for the two open engineering roles continues and four candidates reached the final interview stage.\n\
- Office move planning has started and the lease on the current space ends in March of next year.";
        let s = summarize(input, POINTS).unwrap();
        assert!(s.items.len() >= 3);
        for it in &s.items {
            assert!(!it.text.starts_with('-'), "{it:?}");
        }
    }

    // ---- new: wire strings, aliases, cancellation -------------------------------------------

    #[test]
    fn mode_wire_strings_and_old_aliases() {
        let parse = |s: &str| serde_json::from_str::<SummaryLength>(&format!("\"{s}\"")).unwrap();
        assert_eq!(parse("short"), SummaryLength::Short);
        assert_eq!(parse("detailed"), SummaryLength::Detailed);
        assert_eq!(parse("key-points"), SummaryLength::KeyPoints);
        assert_eq!(parse("action-items"), SummaryLength::ActionItems);
        assert_eq!(parse("decisions"), SummaryLength::Decisions);
        assert_eq!(parse("dates"), SummaryLength::Dates);
        assert_eq!(parse("questions"), SummaryLength::Questions);
        // saved settings and older callers
        assert_eq!(parse("medium"), SummaryLength::Detailed);
        assert_eq!(parse("points"), SummaryLength::KeyPoints);
        assert_eq!(SummaryLength::Medium, SummaryLength::Detailed);
        assert_eq!(SummaryLength::Points, SummaryLength::KeyPoints);
        assert_eq!(
            serde_json::to_string(&SummaryLength::ActionItems).unwrap(),
            "\"action-items\""
        );
        assert_eq!(SummaryLength::default(), SummaryLength::Detailed);
    }

    #[test]
    fn a_raised_cancel_flag_stops_the_work() {
        let flag = AtomicBool::new(true);
        for mode in [SHORT, DETAILED, SummaryLength::Dates] {
            assert_eq!(
                summarize_cancellable(NEWS, mode, &flag).unwrap_err(),
                SummaryError::Cancelled
            );
        }
        let off = AtomicBool::new(false);
        assert!(summarize_cancellable(NEWS, SHORT, &off).is_ok());
    }

    // ---- new: sections ----------------------------------------------------------------------

    #[test]
    fn every_section_of_a_multi_section_text_is_represented() {
        let s = summarize(TOPICS, DETAILED).unwrap();
        assert_extractive(TOPICS, &s);
        assert_eq!(s.sections, 6, "{s:#?}");
        assert_eq!(s.sections_covered, 6, "{:?}", s.items);
        assert!((s.coverage - 1.0).abs() < 1e-9);
        assert_eq!(s.sentences_considered, 30);
        // section i of the output is topic i of the input
        for sec in 0..6 {
            assert!(
                s.items.iter().any(|i| i.section == sec),
                "section {sec} has no sentence: {:?}",
                s.items
                    .iter()
                    .map(|i| (i.section, &i.text))
                    .collect::<Vec<_>>()
            );
        }
        let topics: [&[&str]; 6] = [
            &["roast", "coffee", "bean"],
            &["comet", "telescope", "astronom"],
            &["tomato", "garden", "soil", "compost", "mulch"],
            &["signal", "railway", "track", "train"],
            &["rook", "endgame", "bishop", "pawn", "king"],
            &["volcano", "magma", "eruption", "seismometer", "satellite"],
        ];
        for (i, words) in topics.iter().enumerate() {
            let it = s.items.iter().find(|x| x.section == i).unwrap();
            let low = it.text.to_lowercase();
            assert!(words.iter().any(|w| low.contains(w)), "{i}: {low}");
        }
        // one paragraph per section in the shown text
        assert_eq!(s.text.split("\n\n").count(), 6, "{}", s.text);
        // the same holds for Key points
        let p = summarize(TOPICS, POINTS).unwrap();
        assert_eq!(p.sections_covered, 6, "{:?}", p.items);
    }

    #[test]
    fn short_mode_still_gives_one_to_three_sentences_for_a_sectioned_text() {
        let s = summarize(TOPICS, SHORT).unwrap();
        assert!((1..=3).contains(&s.items.len()), "{:?}", s.items);
    }

    #[test]
    fn the_budget_grows_with_the_input_but_stays_bounded() {
        let small = summarize(TOPICS, DETAILED).unwrap();
        let (big_doc, _) = synthetic(12, 60, 6);
        let big = summarize(&big_doc, DETAILED).unwrap();
        assert!(big.items.len() > small.items.len());
        let share = big.items.len() as f64 / big.sentences_considered as f64;
        assert!((0.04..=0.13).contains(&share), "share {share}");
        assert!(big.items.len() <= 90 + 12, "{}", big.items.len());
        let big_short = summarize(&big_doc, SHORT).unwrap();
        assert!(big_short.items.len() <= 3);
    }

    #[test]
    fn unheaded_text_is_cut_into_chunks_that_are_all_visited() {
        let (doc, _) = synthetic(1, 240, 6);
        // drop the generated heading: no heading anywhere in the text
        let doc = doc.split_once("\n\n").unwrap().1.to_string();
        assert!(text::split_units(&doc)
            .iter()
            .all(|u| u.kind != UnitKind::Heading));
        let s = summarize(&doc, DETAILED).unwrap();
        assert!(s.sections >= 8, "{} sections", s.sections);
        assert_eq!(s.sections_covered, s.sections, "{:?}", s.items.len());
        let last = s.items.last().unwrap().index;
        let first = s.items.first().unwrap().index;
        assert!(last > first + 150, "spread {first}..{last}");
    }

    #[test]
    fn an_off_topic_section_is_not_pre_filtered_away() {
        // The old summariser kept only the 1 200 sentences closest to the centroid; a short,
        // different section at the end vanished.
        let (long, _) = synthetic(1, 1600, 8);
        let tail = "## Notes on penguins\n\n\
            Emperor penguins huddle together through the Antarctic winter to share warmth. \
            The males balance a single egg on their feet for two months without eating. \
            Chicks are fed a rich milk made in the throat of both parents. \
            Colonies gather on sea ice that forms each autumn along the coast.\n";
        let doc = format!("{long}\n{tail}");
        let s = summarize(&doc, DETAILED).unwrap();
        assert!(
            s.items
                .iter()
                .any(|i| i.text.to_lowercase().contains("penguin")
                    || i.text.to_lowercase().contains("chick")
                    || i.text.to_lowercase().contains("egg")
                    || i.text.to_lowercase().contains("colonies")),
            "the last section is missing"
        );
        assert_eq!(s.sections_covered, s.sections);
    }

    #[test]
    fn sixty_thousand_words_finish_fast_and_cover_every_section() {
        let (doc, topics) = synthetic(40, 90, 6);
        let words = text::word_count(&doc);
        assert!(words >= 55_000, "{words} words");
        let t = std::time::Instant::now();
        let s = summarize(&doc, DETAILED).unwrap();
        let ms = t.elapsed().as_millis();
        eprintln!(
            "{words} words, {} items, {} sections: {ms} ms (debug)",
            s.items.len(),
            s.sections
        );
        assert!(ms < 2_500, "took {ms} ms");
        assert!(s.sections >= 40, "{} sections", s.sections);
        assert_eq!(s.sections_covered, s.sections, "coverage {}", s.coverage);
        assert!(s.sentences_considered > 3_000, "{}", s.sentences_considered);
        for (i, tw) in topics.iter().enumerate() {
            assert!(
                s.items.iter().any(|it| tw
                    .iter()
                    .any(|w| it.text.to_lowercase().contains(w.as_str()))),
                "chapter {i} is missing"
            );
        }
        assert!(s.stats.words >= 55_000);
        // and the other ranking modes cope with it as well
        let t = std::time::Instant::now();
        let p = summarize(&doc, POINTS).unwrap();
        let k = summarize(&doc, SHORT).unwrap();
        assert!(t.elapsed().as_millis() < 5_000);
        assert_eq!(p.sections_covered, p.sections);
        assert!(k.items.len() <= 3);
    }

    #[test]
    fn a_long_document_is_fast_enough() {
        let big = format!("{LEGAL}\n\n{TECH}\n\n{NEWS}\n\n").repeat(40);
        let t = std::time::Instant::now();
        let s = summarize(&big, DETAILED).unwrap();
        let ms = t.elapsed().as_millis();
        assert!(s.items.len() >= 3);
        assert!(ms < 5_000, "took {ms} ms");
    }

    // ---- new: key points and short bullets ---------------------------------------------------

    #[test]
    fn key_points_may_pick_short_list_items() {
        let input =
            "Weekend list\n\n- Buy milk\n- Call the bank\n- Fix the sink\n- Water the plants\n\
- Book flights\n- Renew passport\n- Wash the car\n- Pay rent\n- Email Sam\n- Walk the dog\n\
- Pack bags\n- Print tickets\n";
        let s = summarize(input, POINTS).unwrap();
        assert!(!s.already_short, "{:?}", s.items);
        assert!(s.items.len() >= 3, "{:?}", s.items);
        assert!(
            s.items.iter().all(|i| text::word_count(&i.text) < 4),
            "every item here is a short bullet: {:?}",
            s.items
        );
        assert!(s.text.lines().all(|l| l.starts_with('\u{2022}')));
        assert_extractive(input, &s);
    }

    #[test]
    fn other_modes_still_skip_fragments() {
        let input = "Weekend list\n\n- Buy milk\n- Call the bank\n- Fix the sink\n- Water the plants\n\
- Book flights\n- Renew passport\n- Wash the car\n- Pay rent\n- Email Sam\n- Walk the dog\n\
- Pack bags\n- Print tickets\n- Make sure the tickets and the passports are in the same bag before leaving\n\
- Ask the neighbours to collect the post and to feed the cat while we are away\n";
        let s = summarize(input, DETAILED).unwrap();
        for it in &s.items {
            assert!(text::word_count(&it.text) >= 4, "{it:?}");
        }
    }

    // ---- new: extraction modes ---------------------------------------------------------------

    fn texts(s: &Summary) -> Vec<String> {
        s.items.iter().map(|i| i.text.clone()).collect()
    }

    #[test]
    fn action_items_are_extracted() {
        let s = summarize(MEETING, SummaryLength::ActionItems).unwrap();
        assert_extractive(MEETING, &s);
        let t = texts(&s);
        let has = |needle: &str| t.iter().any(|x| x.contains(needle));
        assert!(has("Dana to send the revised launch plan"), "{t:?}");
        assert!(has("Omar will update the pricing page"), "{t:?}");
        assert!(has("Priya needs to confirm"), "{t:?}");
        assert!(has("Review the onboarding copy"), "{t:?}");
        assert!(has("Remember to book the venue"), "{t:?}");
        assert!(!has("We decided to move"), "{t:?}");
        assert!(!has("Next meeting is on Tuesday"), "{t:?}");
        // the checkbox marker is not part of what is shown
        assert!(t.iter().all(|x| !x.starts_with('[')), "{t:?}");
        assert!(
            s.text.lines().all(|l| l.starts_with('\u{2022}')),
            "{}",
            s.text
        );
        assert!(!s.already_short);
        assert!(s.sections_covered >= 1 && s.sections >= 3);
    }

    #[test]
    fn decisions_are_extracted() {
        let s = summarize(MEETING, SummaryLength::Decisions).unwrap();
        let t = texts(&s);
        let has = |needle: &str| t.iter().any(|x| x.contains(needle));
        assert!(has("We decided to move the launch"), "{t:?}");
        assert!(has("Omar agreed to keep"), "{t:?}");
        assert!(has("The team approved the new onboarding flow"), "{t:?}");
        assert!(!has("Priya needs to confirm"), "{t:?}");
        assert!(!has("Who will own"), "{t:?}");
        let hits: Vec<&str> = s.items.iter().filter_map(|i| i.hit.as_deref()).collect();
        assert!(
            hits.contains(&"decided") && hits.contains(&"agreed"),
            "{hits:?}"
        );
    }

    #[test]
    fn dates_are_extracted_with_the_date_phrase() {
        let s = summarize(MEETING, SummaryLength::Dates).unwrap();
        let by = |needle: &str| s.items.iter().find(|i| i.text.contains(needle));
        let friday = by("Dana to send").expect("the Friday deadline");
        assert_eq!(friday.hit.as_deref(), Some("by Friday"));
        let meeting = by("Next meeting").expect("the next meeting");
        let hit = meeting.hit.as_deref().unwrap();
        assert!(hit.contains("Tuesday") && hit.contains("3 pm"), "{hit}");
        assert_eq!(
            by("go/no-go").and_then(|i| i.hit.as_deref()),
            Some("on 2026-04-10")
        );
        assert!(by("April 14").is_some());
        assert!(by("Review the onboarding copy").is_none());
        assert!(by("Priya needs to confirm").is_none());
        assert!(s.text.contains("by Friday: Dana to send"), "{}", s.text);
        assert_extractive(MEETING, &s);
    }

    #[test]
    fn open_questions_are_extracted() {
        let s = summarize(MEETING, SummaryLength::Questions).unwrap();
        let t = texts(&s);
        let has = |needle: &str| t.iter().any(|x| x.contains(needle));
        assert!(has("Who will own the support rota after launch?"), "{t:?}");
        assert!(has("still TBD"), "{t:?}");
        assert!(has("It is unclear whether"), "{t:?}");
        assert_eq!(t.len(), 3, "{t:?}");
    }

    #[test]
    fn extraction_works_on_input_too_short_to_summarise() {
        let s = summarize("Send the invoice by Friday.", SummaryLength::Dates).unwrap();
        assert!(!s.already_short);
        assert_eq!(s.items.len(), 1);
        let none = summarize(
            "Nothing planned for the weekend at all.",
            SummaryLength::Questions,
        )
        .unwrap();
        assert!(none.items.is_empty());
        assert!(none.note.as_deref().unwrap().contains("No open questions"));
    }

    #[test]
    fn extraction_drops_repeats_and_reports_when_it_stops() {
        let one = "Please send the signed contract back to the legal team by Friday afternoon.";
        let doc = format!("{one} {one} Please book a room for the kickoff meeting next week.");
        let s = summarize(&doc, SummaryLength::ActionItems).unwrap();
        assert_eq!(s.items.len(), 2, "{:?}", texts(&s));

        // many different matches: capped, and the note says so
        let mut big = String::new();
        for i in 0..400 {
            big.push_str(&format!(
                "Please review the {} report number {} before the deadline. ",
                word(i as u64),
                word(i as u64 + 9000)
            ));
        }
        let s = summarize(&big, SummaryLength::ActionItems).unwrap();
        assert_eq!(s.items.len(), MAX_MATCHES);
        assert!(
            s.note.as_deref().unwrap().contains("strongest"),
            "{:?}",
            s.note
        );
    }

    #[test]
    fn extraction_reports_sections_and_sentences_scanned() {
        let (doc, _) = synthetic(10, 40, 5);
        let s = summarize(&doc, SummaryLength::Dates).unwrap();
        assert!(s.sections >= 10);
        assert!(s.sentences_considered > 300);
        assert!(s.coverage <= 1.0);
    }

    // ---- new: space-less scripts -------------------------------------------------------------

    const CHINESE: &str = "克罗维是一个轻量的桌面应用。它把常用的小工具放在一个货架上。\
剪贴板、计时器和笔记都在同一个地方。所有数据都保存在你的电脑上。\
语音输入完全在本机完成，不会上传任何内容。你可以用右键把任何内容发送到货架。\
总之，克罗维让日常工作更简单。计时器结束时会播放提示音。\
番茄钟会记录每天的专注次数。笔记支持清单和语音输入。\
计算器会记住上一次的表达式。文件货架可以临时存放拖进来的文件。";

    #[test]
    fn chinese_text_is_not_one_giant_word() {
        let words = text::word_count(CHINESE);
        assert!(
            words > 60,
            "{words} words for {} chars",
            CHINESE.chars().count()
        );
        let s = summarize(CHINESE, DETAILED).unwrap();
        assert!(!s.already_short, "{:?}", s.items);
        assert!(s.stats.words > 60);
        assert!(s.items.len() >= 3, "{:?}", s.items);
        assert!(s.stats.ratio < 0.8, "ratio {}", s.stats.ratio);
        assert_extractive(CHINESE, &s);
        let short = summarize(CHINESE, SHORT).unwrap();
        assert!((1..=2).contains(&short.items.len()));
    }

    /// Whole sentences used to be single tokens, so no two sentences ever looked alike and the
    /// ranking was arbitrary. With character bigrams the on-topic sentences find each other.
    #[test]
    fn cjk_ranking_uses_shared_characters() {
        let input = "委员会今天批准了明年的预算方案。预算方案包括教育和交通两部分。\
委员会认为预算方案需要更多讨论。预算方案将在下周公布。\
市民可以在网上查看预算方案的细节。委员会还将举行一次关于预算方案的公开会议。\
猫坐在窗台上睡觉。";
        let s = summarize(input, SHORT).unwrap();
        assert!(!s.already_short, "{:?}", s.items);
        assert!(!s.items.is_empty());
        for it in &s.items {
            assert!(it.text.contains("预算"), "{:?}", s.items);
        }
    }

    #[test]
    fn japanese_and_mixed_scripts_count_sensibly() {
        let jp = "今日は晴れです。明日も晴れるでしょう。週末は雨が降るかもしれません。傘を持っていきましょう。\
来週は気温が下がる予報です。コートを出しておくと安心です。";
        let words = text::word_count(jp);
        assert!((15..=40).contains(&words), "{words}");
        let mixed = "Krovi \u{306f}\u{4fbf}\u{5229}\u{3067}\u{3059}\u{3002}";
        assert!(text::word_count(mixed) >= 3);
    }
}
