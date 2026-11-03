//! Local file-name search over the user's known folders.
//!
//! What this is, and is not:
//!
//! * It matches file and folder **names** only. It never opens a file and reads nothing from
//!   inside one.
//! * There is **no index and nothing runs in the background**. A search walks the folders when the
//!   user asks, streams what it finds as it goes, and stops. Nothing is kept afterwards except the
//!   list of hits the host holds so a result can be opened.
//! * It is bounded: a depth cap, a result cap, an entry cap and a time budget, and it can be
//!   cancelled at any moment.
//! * It never follows links (symlinks, junctions, OneDrive placeholders), and it skips hidden and
//!   system files and folders and a few folders that only hold other programs' data
//!   (`node_modules`, `.git`, the Recycle Bin).
//! * Folders are walked breadth first, so the shallow, usually more relevant, matches stream first.

use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::fsx::{inside_strict, is_hidden_or_system, is_reparse, modified_ms};
use crate::known::KnownFolder;

pub const MIN_QUERY_CHARS: usize = 2;
pub const MAX_QUERY_CHARS: usize = 100;
pub const MAX_TOKENS: usize = 6;
pub const DEFAULT_MAX_DEPTH: usize = 6;
pub const DEFAULT_MAX_RESULTS: usize = 200;
pub const DEFAULT_MAX_ENTRIES: u64 = 200_000;
pub const DEFAULT_BUDGET: Duration = Duration::from_secs(20);

/// Folders that only hold another program's files.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    ".svn",
    ".hg",
    "$recycle.bin",
    "system volume information",
];

/// The words to look for: lower-case, split on spaces. A name must contain every one of them.
pub fn parse_query(q: &str) -> Result<Vec<String>, String> {
    let t = q.trim();
    if t.chars().count() > MAX_QUERY_CHARS {
        return Err(format!(
            "That search is too long (over {MAX_QUERY_CHARS} characters)."
        ));
    }
    if t.chars().any(char::is_control) {
        return Err("A search cannot contain control characters.".into());
    }
    let tokens: Vec<String> = t.split_whitespace().map(str::to_lowercase).collect();
    if t.chars().filter(|c| !c.is_whitespace()).count() < MIN_QUERY_CHARS {
        return Err(format!("Type at least {MIN_QUERY_CHARS} characters."));
    }
    if tokens.len() > MAX_TOKENS {
        return Err(format!("Use at most {MAX_TOKENS} words."));
    }
    Ok(tokens)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    /// 1, 2, 3, ... within one search.
    pub id: u32,
    pub name: String,
    pub path: String,
    pub folder: KnownFolder,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stop {
    /// Every allowed folder was looked at.
    Finished,
    Cancelled,
    ResultLimit,
    EntryLimit,
    TimeLimit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub stop: Stop,
    /// Names looked at.
    pub scanned: u64,
    pub found: u32,
    pub skipped_links: u64,
    pub skipped_hidden: u64,
    /// Folders not entered because they are below the depth cap.
    pub skipped_deep: u64,
    pub unreadable: u64,
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub max_depth: usize,
    pub max_results: usize,
    pub max_entries: u64,
    pub budget: Duration,
}

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions {
            max_depth: DEFAULT_MAX_DEPTH,
            max_results: DEFAULT_MAX_RESULTS,
            max_entries: DEFAULT_MAX_ENTRIES,
            budget: DEFAULT_BUDGET,
        }
    }
}

fn skip_dir(name_lower: &str) -> bool {
    SKIP_DIRS.contains(&name_lower)
}

/// Looks through `roots` for names containing every word of `tokens` (from [`parse_query`]).
/// `on_hits` receives the hits found in each folder as soon as that folder is finished, so a
/// caller can stream them to the screen. Reads names only.
pub fn search_names(
    roots: &[(KnownFolder, PathBuf)],
    tokens: &[String],
    opts: &SearchOptions,
    cancel: &AtomicBool,
    on_hits: &mut dyn FnMut(&[Hit]),
) -> Summary {
    let mut sum = Summary {
        stop: Stop::Finished,
        scanned: 0,
        found: 0,
        skipped_links: 0,
        skipped_hidden: 0,
        skipped_deep: 0,
        unreadable: 0,
    };
    let started = Instant::now();

    // a root that lies inside another root is covered by it
    let roots: Vec<&(KnownFolder, PathBuf)> = roots
        .iter()
        .filter(|(_, p)| !roots.iter().any(|(_, o)| inside_strict(p, o)))
        .collect();
    let mut queue: VecDeque<(KnownFolder, PathBuf, usize)> =
        roots.iter().map(|(k, p)| (*k, p.clone(), 0)).collect();

    'walk: while let Some((folder, dir, depth)) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            sum.stop = Stop::Cancelled;
            break;
        }
        if started.elapsed() >= opts.budget {
            sum.stop = Stop::TimeLimit;
            break;
        }
        let Ok(read) = fs::read_dir(&dir) else {
            sum.unreadable += 1;
            continue;
        };
        let mut entries: Vec<(String, fs::Metadata)> = Vec::new();
        for entry in read {
            let Ok(entry) = entry else {
                sum.unreadable += 1;
                continue;
            };
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                sum.unreadable += 1;
                continue;
            };
            match entry.metadata() {
                Ok(meta) => entries.push((name, meta)),
                Err(_) => sum.unreadable += 1,
            }
        }
        entries.sort_by_key(|(n, _)| n.to_lowercase());

        let mut hits: Vec<Hit> = Vec::new();
        for (name, meta) in entries {
            if cancel.load(Ordering::Relaxed) {
                sum.stop = Stop::Cancelled;
                flush(&mut hits, on_hits);
                break 'walk;
            }
            sum.scanned += 1;
            if sum.scanned >= opts.max_entries {
                sum.stop = Stop::EntryLimit;
                flush(&mut hits, on_hits);
                break 'walk;
            }
            // the clock is read every 512 names, not for every one
            if sum.scanned & 0x1FF == 0 && started.elapsed() >= opts.budget {
                sum.stop = Stop::TimeLimit;
                flush(&mut hits, on_hits);
                break 'walk;
            }
            if is_reparse(&meta) {
                sum.skipped_links += 1;
                continue;
            }
            if is_hidden_or_system(&meta, &name) {
                sum.skipped_hidden += 1;
                continue;
            }
            let lower = name.to_lowercase();
            let is_dir = meta.is_dir();
            if !is_dir && !meta.is_file() {
                continue;
            }
            if tokens.iter().all(|t| lower.contains(t.as_str())) {
                sum.found += 1;
                hits.push(Hit {
                    id: sum.found,
                    path: dir.join(&name).to_string_lossy().into_owned(),
                    name: name.clone(),
                    folder,
                    is_dir,
                    size: (!is_dir).then_some(meta.len()),
                    modified_ms: modified_ms(&meta),
                });
                if sum.found as usize >= opts.max_results {
                    sum.stop = Stop::ResultLimit;
                    flush(&mut hits, on_hits);
                    break 'walk;
                }
            }
            if is_dir && !skip_dir(&lower) {
                if depth < opts.max_depth {
                    queue.push_back((folder, dir.join(&name), depth + 1));
                } else {
                    sum.skipped_deep += 1;
                }
            }
        }
        flush(&mut hits, on_hits);
    }
    sum
}

fn flush(hits: &mut Vec<Hit>, on_hits: &mut dyn FnMut(&[Hit])) {
    if !hits.is_empty() {
        on_hits(&hits[..]);
        hits.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{make_dir_link, remove_dir_link, TempDir};

    fn root(t: &TempDir) -> Vec<(KnownFolder, PathBuf)> {
        vec![(KnownFolder::Documents, t.path().to_path_buf())]
    }

    fn run(
        roots: &[(KnownFolder, PathBuf)],
        q: &str,
        opts: &SearchOptions,
    ) -> (Vec<Hit>, Summary, usize) {
        let tokens = parse_query(q).unwrap();
        let mut all = Vec::new();
        let mut calls = 0;
        let sum = search_names(roots, &tokens, opts, &AtomicBool::new(false), &mut |h| {
            calls += 1;
            all.extend_from_slice(h);
        });
        (all, sum, calls)
    }

    fn names(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.name.as_str()).collect()
    }

    #[test]
    fn queries_are_split_lowered_and_bounded() {
        assert_eq!(parse_query("  Tax  2024 ").unwrap(), vec!["tax", "2024"]);
        assert!(parse_query("a").unwrap_err().contains("at least"));
        assert!(parse_query("   ").is_err());
        assert!(parse_query("").is_err());
        assert!(parse_query("a\u{0}b").unwrap_err().contains("control"));
        assert!(parse_query(&"x".repeat(MAX_QUERY_CHARS + 1))
            .unwrap_err()
            .contains("too long"));
        assert!(parse_query("a b c d e f g")
            .unwrap_err()
            .contains("at most"));
        assert!(parse_query("ab").is_ok());
    }

    #[test]
    fn matches_file_and_folder_names_not_contents() {
        let t = TempDir::new("srch");
        t.write("Invoice-2024.pdf", 10);
        t.write("notes.txt", 2000); // its bytes are all 'k': contents are never looked at
        t.write("sub/another invoice.docx", 3);
        t.mkdir("Invoices archive");
        t.write("unrelated.txt", 1);
        let (hits, sum, _) = run(&root(&t), "INVOICE", &SearchOptions::default());
        let mut n = names(&hits);
        n.sort();
        assert_eq!(
            n,
            vec![
                "Invoice-2024.pdf",
                "Invoices archive",
                "another invoice.docx"
            ]
        );
        assert_eq!(sum.stop, Stop::Finished);
        assert_eq!(sum.found, 3);
        // contents are not searched
        let (hits, _, _) = run(&root(&t), "kkkk", &SearchOptions::default());
        assert!(hits.is_empty());
    }

    #[test]
    fn every_word_must_match_in_any_order() {
        let t = TempDir::new("srch-words");
        t.write("tax return 2024.pdf", 1);
        t.write("tax 2023.pdf", 1);
        let (hits, _, _) = run(&root(&t), "2024 tax", &SearchOptions::default());
        assert_eq!(names(&hits), vec!["tax return 2024.pdf"]);
    }

    #[test]
    fn hits_carry_kind_size_path_and_ids() {
        let t = TempDir::new("srch-meta");
        t.write("alpha.txt", 12);
        t.mkdir("alpha-dir");
        let (hits, _, _) = run(&root(&t), "alpha", &SearchOptions::default());
        assert_eq!(hits.len(), 2);
        let file = hits.iter().find(|h| !h.is_dir).unwrap();
        assert_eq!(file.size, Some(12));
        assert!(file.modified_ms.is_some());
        assert!(file.path.ends_with("alpha.txt"));
        let dir = hits.iter().find(|h| h.is_dir).unwrap();
        assert_eq!(dir.size, None);
        assert_eq!(hits.iter().map(|h| h.id).collect::<Vec<_>>(), vec![1, 2]);
        assert!(hits.iter().all(|h| h.folder == KnownFolder::Documents));
    }

    #[test]
    fn shallow_matches_come_first_and_stream_per_folder() {
        let t = TempDir::new("srch-order");
        t.write("deep/er/target-deep.txt", 1);
        t.write("mid/target-mid.txt", 1);
        t.write("target-top.txt", 1);
        let (hits, _, calls) = run(&root(&t), "target", &SearchOptions::default());
        assert_eq!(
            names(&hits),
            vec!["target-top.txt", "target-mid.txt", "target-deep.txt"]
        );
        assert_eq!(
            calls, 3,
            "each folder's hits are delivered when that folder is done"
        );
    }

    #[test]
    fn the_depth_cap_is_respected_and_reported() {
        let t = TempDir::new("srch-depth");
        t.write("a/b/c/target.txt", 1); // lives in a folder at depth 3
        t.write("a/target-shallow.txt", 1);
        let opts = SearchOptions {
            max_depth: 2,
            ..SearchOptions::default()
        };
        let (hits, sum, _) = run(&root(&t), "target", &opts);
        assert_eq!(names(&hits), vec!["target-shallow.txt"]);
        assert_eq!(sum.skipped_deep, 1, "folder c is below the cap");
        let (hits, _, _) = run(
            &root(&t),
            "target",
            &SearchOptions {
                max_depth: 3,
                ..opts
            },
        );
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn links_are_not_followed() {
        let t = TempDir::new("srch-link");
        let elsewhere = TempDir::new("srch-link-target");
        elsewhere.write("target-secret.txt", 1);
        let link = t.path().join("shortcut-target");
        if !make_dir_link(&link, elsewhere.path()) {
            eprintln!("skipped: this machine does not allow creating directory links");
            return;
        }
        t.write("target-real.txt", 1);
        let (hits, sum, _) = run(&root(&t), "target", &SearchOptions::default());
        assert_eq!(
            names(&hits),
            vec!["target-real.txt"],
            "neither the link nor what is behind it"
        );
        assert_eq!(sum.skipped_links, 1);
        remove_dir_link(&link);
    }

    #[test]
    fn folders_that_only_hold_other_programs_data_are_not_entered() {
        let t = TempDir::new("srch-skip");
        t.write("node_modules/pkg/target.js", 1);
        t.write(".git/objects/target", 1);
        t.write("src/target.rs", 1);
        let (hits, _, _) = run(&root(&t), "target", &SearchOptions::default());
        assert_eq!(names(&hits), vec!["target.rs"]);
    }

    #[cfg(windows)]
    #[test]
    fn hidden_files_and_folders_are_skipped() {
        let t = TempDir::new("srch-hidden");
        let hidden_file = t.write("target-hidden.txt", 1);
        let hidden_dir = t.mkdir("target-hidden-dir");
        t.write("target-hidden-dir/inside-target.txt", 1);
        t.write("target-visible.txt", 1);
        let attrib = |p: &std::path::Path| {
            std::process::Command::new("attrib")
                .arg("+H")
                .arg(p)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        };
        if !(attrib(&hidden_file) && attrib(&hidden_dir)) {
            eprintln!("skipped: attrib is not available");
            return;
        }
        let (hits, sum, _) = run(&root(&t), "target", &SearchOptions::default());
        assert_eq!(names(&hits), vec!["target-visible.txt"]);
        assert_eq!(sum.skipped_hidden, 2);
    }

    #[test]
    fn the_result_cap_stops_the_search() {
        let t = TempDir::new("srch-cap");
        for n in 0..8 {
            t.write(&format!("match-{n}.txt"), 1);
        }
        let opts = SearchOptions {
            max_results: 3,
            ..SearchOptions::default()
        };
        let (hits, sum, _) = run(&root(&t), "match", &opts);
        assert_eq!(hits.len(), 3);
        assert_eq!((sum.stop, sum.found), (Stop::ResultLimit, 3));
    }

    #[test]
    fn the_entry_cap_stops_the_search() {
        let t = TempDir::new("srch-entries");
        for n in 0..20 {
            t.write(&format!("file-{n}.txt"), 1);
        }
        let opts = SearchOptions {
            max_entries: 5,
            ..SearchOptions::default()
        };
        let (_, sum, _) = run(&root(&t), "file", &opts);
        assert_eq!(sum.stop, Stop::EntryLimit);
        assert!(sum.scanned <= 5);
    }

    #[test]
    fn a_zero_budget_stops_at_once() {
        let t = TempDir::new("srch-time");
        t.write("target.txt", 1);
        let opts = SearchOptions {
            budget: Duration::ZERO,
            ..SearchOptions::default()
        };
        let (_, sum, _) = run(&root(&t), "target", &opts);
        assert_eq!(sum.stop, Stop::TimeLimit);
    }

    #[test]
    fn cancelling_stops_the_walk_and_keeps_what_was_found() {
        let t = TempDir::new("srch-cancel");
        t.write("target-1.txt", 1);
        t.write("one/target-2.txt", 1);
        t.write("one/two/target-3.txt", 1);
        let tokens = parse_query("target").unwrap();
        let cancel = AtomicBool::new(false);
        let mut got = 0;
        let sum = search_names(
            &root(&t),
            &tokens,
            &SearchOptions::default(),
            &cancel,
            &mut |h| {
                got += h.len();
                cancel.store(true, Ordering::Relaxed); // the user pressed Stop after the first batch
            },
        );
        assert_eq!(sum.stop, Stop::Cancelled);
        assert_eq!(got, 1);
    }

    #[test]
    fn a_folder_inside_another_searched_folder_is_not_searched_twice() {
        let t = TempDir::new("srch-nest");
        t.write("pics/target.jpg", 1);
        let docs = t.path().to_path_buf();
        let pics = t.path().join("pics");
        let roots = vec![
            (KnownFolder::Documents, docs),
            (KnownFolder::Pictures, pics),
        ];
        let (hits, _, _) = run(&roots, "target", &SearchOptions::default());
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn a_missing_folder_is_counted_not_fatal() {
        let t = TempDir::new("srch-missing");
        t.write("target.txt", 1);
        // a folder that does not exist and is not inside the other one
        let gone =
            std::env::temp_dir().join(format!("krovi-extras-missing-{}", std::process::id()));
        let roots = vec![
            (KnownFolder::Music, gone),
            (KnownFolder::Documents, t.path().to_path_buf()),
        ];
        let (hits, sum, _) = run(&roots, "target", &SearchOptions::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(sum.unreadable, 1);
    }
}
