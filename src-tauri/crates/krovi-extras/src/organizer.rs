//! File-organizer rules: "In Downloads, move .pdf files older than 7 days to Documents\PDFs".
//!
//! The flow is always **Preview -> Confirm -> Run**, and nothing runs by itself:
//!
//! 1. [`plan_moves`] only reads. It lists the exact moves (source, destination folder and final
//!    name) for the enabled rules, and says plainly what it left alone and why.
//! 2. The user reads the list and confirms. The host keeps the preview; the webview later quotes its
//!    id and never sends a path.
//! 3. [`run_moves`] re-checks every file against the disk right before it moves it, then moves it.
//!    Afterwards [`undo_moves`] can put the files back, but only the ones nothing has touched since.
//!
//! Safety rules, all enforced here and covered by tests:
//!
//! * Folders are limited to the user's six known folders ([`KnownFolder`]); a rule can only read
//!   the top level of one and write into a *sub*folder of one.
//! * **Never delete, never overwrite.** A move is a rename. The destination name is reserved with
//!   an atomic create-new first (so a file that appears at the last moment is never replaced), a
//!   clash gets ` (2)`, ` (3)`, ... and a rename that fails removes only the empty placeholder this
//!   code just created. There is no copy-then-delete fallback, so a move across drives is refused
//!   instead of done.
//! * **Links are never followed.** A reparse point (symlink, junction, OneDrive placeholder) is not
//!   listed, not moved, and not allowed anywhere on the destination path. Every source path is
//!   re-resolved with `canonicalize` and must still be itself, directly inside its known folder.
//! * Hidden and system files, files changed in the last two minutes and unfinished downloads
//!   (`.crdownload`, `.part`, ...) are left alone.
//! * At most [`MAX_MOVES`] moves per preview. Nothing is read from inside a file.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::fsx::{
    canonical, has_wildcard, inside_strict, io_reason, is_hidden_or_system, is_reparse,
    is_unfinished_download, modified_ms, path_too_long, same_path,
};
use crate::known::{KnownFolder, KnownFolders};

/// Settings key for the rules (a JSON array of [`Rule`]).
pub const RULES_KEY: &str = "organizer.rules.v1";
/// Settings key for the last run's undo journal (an [`UndoJournal`] or `null`).
pub const UNDO_KEY: &str = "organizer.undo.v1";
pub const MAX_RULES: usize = 20;
pub const MAX_MOVES: usize = 500;
pub const MAX_EXTENSIONS: usize = 20;
pub const MAX_EXTENSION_CHARS: usize = 12;
pub const MAX_CONTAINS_CHARS: usize = 60;
pub const MAX_SUBFOLDER_SEGMENTS: usize = 3;
pub const MAX_SEGMENT_CHARS: usize = 64;
pub const MAX_AGE_DAYS: u32 = 3650;
/// A file changed more recently than this is still being written; it is never moved.
pub const MIN_QUIET: Duration = Duration::from_secs(120);
/// `name (2).ext` .. `name (99).ext`; past that the file is left alone.
pub const MAX_CLASH_SUFFIX: u32 = 99;

// ---------------------------------------------------------------------------------------------
// rules
// ---------------------------------------------------------------------------------------------

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    /// Assigned on first save; leave empty for a new rule.
    #[serde(default)]
    pub id: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Where to look (the top level of this folder only).
    pub folder: KnownFolder,
    /// Lower-case extensions without the dot. Empty = any extension.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// The file name must contain this (any case). Empty = no such condition.
    #[serde(default)]
    pub name_contains: String,
    /// Not modified for at least this many days.
    #[serde(default)]
    pub older_than_days: Option<u32>,
    pub dest_folder: KnownFolder,
    /// A folder (up to three levels, `\`-separated) inside `dest_folder`.
    pub dest_subfolder: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleError {
    NoCondition,
    BadExtension,
    TooManyExtensions,
    ContainsBad,
    ContainsTooLong,
    AgeBad,
    SubfolderEmpty,
    SubfolderAbsolute,
    SubfolderBad,
    SubfolderTooDeep,
    SubfolderSegmentTooLong,
    TooMany,
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleError::NoCondition => write!(
                f,
                "Add at least one condition: a file type, a word in the name, or an age. A rule with none would move everything."
            ),
            RuleError::BadExtension => write!(
                f,
                "File types are letters and digits only, up to {MAX_EXTENSION_CHARS} characters, like pdf or jpg."
            ),
            RuleError::TooManyExtensions => write!(f, "At most {MAX_EXTENSIONS} file types per rule."),
            RuleError::ContainsBad => write!(
                f,
                "The word to look for in the name cannot contain \\ / : * ? \" < > | or control characters."
            ),
            RuleError::ContainsTooLong => {
                write!(f, "That word is too long (over {MAX_CONTAINS_CHARS} characters).")
            }
            RuleError::AgeBad => write!(f, "The age must be between 1 and {MAX_AGE_DAYS} days."),
            RuleError::SubfolderEmpty => write!(
                f,
                "Name the folder to move files into, for example PDFs. Files are never moved into the top of a folder."
            ),
            RuleError::SubfolderAbsolute => write!(
                f,
                "Give a folder name inside the destination, not a full path (no drive letter or leading slash)."
            ),
            RuleError::SubfolderBad => write!(
                f,
                "A folder name cannot contain \\\\ / : * ? \" < > |, end with a dot or space, be . or .., or be a reserved Windows name."
            ),
            RuleError::SubfolderTooDeep => {
                write!(f, "The destination can be at most {MAX_SUBFOLDER_SEGMENTS} folders deep.")
            }
            RuleError::SubfolderSegmentTooLong => {
                write!(f, "A folder name is too long (over {MAX_SEGMENT_CHARS} characters).")
            }
            RuleError::TooMany => write!(f, "At most {MAX_RULES} rules can be saved."),
        }
    }
}

impl std::error::Error for RuleError {}

fn clean_extensions(list: &[String]) -> Result<Vec<String>, RuleError> {
    let mut out: Vec<String> = Vec::new();
    for item in list {
        for tok in item.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
            let t = tok
                .trim()
                .trim_start_matches('*')
                .trim_start_matches('.')
                .to_lowercase();
            if t.is_empty() {
                continue;
            }
            if t.chars().count() > MAX_EXTENSION_CHARS
                || !t
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-'))
            {
                return Err(RuleError::BadExtension);
            }
            if !out.contains(&t) {
                out.push(t);
            }
        }
    }
    if out.len() > MAX_EXTENSIONS {
        return Err(RuleError::TooManyExtensions);
    }
    Ok(out)
}

const RESERVED: &[&str] = &["con", "prn", "aux", "nul"];

fn is_reserved_name(seg: &str) -> bool {
    let stem = seg.split('.').next().unwrap_or("").to_lowercase();
    if RESERVED.contains(&stem.as_str()) {
        return true;
    }
    for prefix in ["com", "lpt"] {
        if let Some(n) = stem.strip_prefix(prefix) {
            if n.len() == 1 && n.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
}

/// A destination folder typed by the user, as clean path segments. Relative only: no drive, no
/// leading slash, no `.` or `..`, no characters Windows treats specially.
pub fn clean_subfolder(raw: &str) -> Result<Vec<String>, RuleError> {
    let t = raw.trim();
    if t.starts_with('\\') || t.starts_with('/') {
        return Err(RuleError::SubfolderAbsolute);
    }
    let t = t.trim_end_matches(['\\', '/']).trim();
    if t.is_empty() {
        return Err(RuleError::SubfolderEmpty);
    }
    if t.contains(':') {
        return Err(RuleError::SubfolderAbsolute);
    }
    let segs: Vec<String> = t
        .replace('/', "\\")
        .split('\\')
        .map(|s| s.trim().to_string())
        .collect();
    if segs.len() > MAX_SUBFOLDER_SEGMENTS {
        return Err(RuleError::SubfolderTooDeep);
    }
    for s in &segs {
        if s.is_empty()
            || s == "."
            || s == ".."
            || s.ends_with('.')
            || s.chars()
                .any(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
            || is_reserved_name(s)
        {
            return Err(RuleError::SubfolderBad);
        }
        if s.chars().count() > MAX_SEGMENT_CHARS {
            return Err(RuleError::SubfolderSegmentTooLong);
        }
    }
    Ok(segs)
}

/// Check one rule and return it normalised. The id is left as given.
pub fn validate_rule(r: &Rule) -> Result<Rule, RuleError> {
    let extensions = clean_extensions(&r.extensions)?;
    let contains = r.name_contains.trim().to_string();
    if contains.chars().count() > MAX_CONTAINS_CHARS {
        return Err(RuleError::ContainsTooLong);
    }
    if contains.chars().any(|c| {
        c.is_control() || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
    }) {
        return Err(RuleError::ContainsBad);
    }
    let older = match r.older_than_days {
        None | Some(0) => None,
        Some(d) if d > MAX_AGE_DAYS => return Err(RuleError::AgeBad),
        Some(d) => Some(d),
    };
    if extensions.is_empty() && contains.is_empty() && older.is_none() {
        return Err(RuleError::NoCondition);
    }
    let segs = clean_subfolder(&r.dest_subfolder)?;
    Ok(Rule {
        id: r.id.clone(),
        enabled: r.enabled,
        folder: r.folder,
        extensions,
        name_contains: contains,
        older_than_days: older,
        dest_folder: r.dest_folder,
        dest_subfolder: segs.join("\\"),
    })
}

impl Rule {
    /// The rule as a sentence: "In Downloads, move .pdf files older than 7 days to Documents\PDFs".
    pub fn describe(&self) -> String {
        let what = if self.extensions.is_empty() {
            "files".to_string()
        } else {
            let list: Vec<String> = self.extensions.iter().map(|e| format!(".{e}")).collect();
            format!("{} files", list.join(", "))
        };
        let mut s = format!("In {}, move {what}", self.folder.label());
        if !self.name_contains.is_empty() {
            s.push_str(&format!(" with \"{}\" in the name", self.name_contains));
        }
        if let Some(d) = self.older_than_days {
            s.push_str(&format!(
                " older than {d} day{}",
                if d == 1 { "" } else { "s" }
            ));
        }
        s.push_str(&format!(
            " to {}\\{}",
            self.dest_folder.label(),
            self.dest_subfolder
        ));
        s
    }

    /// The destination folder as segments (already validated when the rule was saved).
    pub fn segments(&self) -> Vec<String> {
        clean_subfolder(&self.dest_subfolder).unwrap_or_default()
    }

    /// The extension and name conditions, which need only the file's name.
    pub fn name_matches(&self, name: &str) -> bool {
        if !self.extensions.is_empty() {
            match name.rsplit_once('.') {
                Some((stem, ext))
                    if !stem.is_empty() && self.extensions.contains(&ext.to_lowercase()) => {}
                _ => return false,
            }
        }
        if !self.name_contains.is_empty()
            && !name
                .to_lowercase()
                .contains(&self.name_contains.to_lowercase())
        {
            return false;
        }
        true
    }

    /// The age condition. A file with no readable time, or one dated in the future, never matches
    /// an age condition.
    pub fn age_matches(&self, modified: Option<SystemTime>, now: SystemTime) -> bool {
        match self.older_than_days {
            None => true,
            Some(days) => modified
                .and_then(|m| now.duration_since(m).ok())
                .is_some_and(|age| age >= Duration::from_secs(u64::from(days) * 86_400)),
        }
    }
}

fn slug_id(taken: &[Rule]) -> String {
    let mut n = 1;
    loop {
        let id = format!("rule-{n}");
        if !taken.iter().any(|r| r.id == id) {
            return id;
        }
        n += 1;
    }
}

/// The user's rules, in the order they apply (the first rule to claim a file wins).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Rules {
    items: Vec<Rule>,
}

impl Rules {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load the stored values. Lenient: an entry that no longer validates or repeats an id is
    /// dropped.
    pub fn from_values(values: Vec<serde_json::Value>) -> Self {
        let mut out = Rules::new();
        for v in values {
            let Ok(item) = serde_json::from_value::<Rule>(v) else {
                continue;
            };
            if item.id.is_empty() || out.items.iter().any(|r| r.id == item.id) {
                continue;
            }
            let Ok(clean) = validate_rule(&item) else {
                continue;
            };
            if out.items.len() < MAX_RULES {
                out.items.push(clean);
            }
        }
        out
    }

    pub fn list(&self) -> &[Rule] {
        &self.items
    }

    /// Add a rule, or update the one with the same id. Returns the stored rule.
    pub fn upsert(&mut self, input: Rule) -> Result<Rule, RuleError> {
        let clean = validate_rule(&input)?;
        if !input.id.is_empty() {
            if let Some(slot) = self.items.iter_mut().find(|r| r.id == input.id) {
                let stored = Rule {
                    id: input.id.clone(),
                    ..clean
                };
                *slot = stored.clone();
                return Ok(stored);
            }
        }
        if self.items.len() >= MAX_RULES {
            return Err(RuleError::TooMany);
        }
        let stored = Rule {
            id: slug_id(&self.items),
            ..clean
        };
        self.items.push(stored.clone());
        Ok(stored)
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|r| r.id != id);
        self.items.len() != before
    }
}

// ---------------------------------------------------------------------------------------------
// preview
// ---------------------------------------------------------------------------------------------

/// One move the preview proposes. The webview only ever receives these; it never sends one back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedMove {
    pub id: String,
    pub rule_id: String,
    pub src_folder: KnownFolder,
    pub dest_folder: KnownFolder,
    /// The destination subfolder, `\`-separated, below `dest_folder`.
    pub dest_sub: String,
    pub from: String,
    /// The destination folder (it may not exist yet; it is created when the move runs).
    pub to_dir: String,
    /// The final file name: the original, or `name (2).ext` after a clash.
    pub to_name: String,
    pub size: u64,
    pub modified_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub preview_id: u64,
    pub moves: Vec<PlannedMove>,
    /// Plain sentences about what was left alone and why.
    pub notes: Vec<String>,
    /// More than [`MAX_MOVES`] files matched; only the first are listed.
    pub truncated: bool,
    pub created_ms: u64,
    /// Enabled rules whose folders exist.
    pub rules_checked: usize,
}

#[derive(Debug, Clone)]
pub struct PlanOptions {
    pub now: SystemTime,
    pub max_moves: usize,
}

impl Default for PlanOptions {
    fn default() -> Self {
        PlanOptions {
            now: SystemTime::now(),
            max_moves: MAX_MOVES,
        }
    }
}

#[derive(Default)]
struct Counts {
    links: u64,
    hidden: u64,
    recent: u64,
    too_long: u64,
    no_name: u64,
    odd_names: u64,
    unreadable: u64,
}

fn key(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

pub fn system_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn clash_name(name: &str, n: u32) -> String {
    if n <= 1 {
        return name.to_string();
    }
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem} ({n}).{ext}"),
        _ => format!("{name} ({n})"),
    }
}

/// The first of `name`, `name (2)`, ... that is not on disk and not already promised to another
/// move in this preview.
fn free_name(dir: &Path, name: &str, reserved: &HashSet<String>) -> Option<String> {
    for n in 1..=MAX_CLASH_SUFFIX {
        let cand = clash_name(name, n);
        let p = dir.join(&cand);
        if reserved.contains(&key(&p)) {
            continue;
        }
        match fs::symlink_metadata(&p) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Some(cand),
            _ => continue,
        }
    }
    None
}

/// Walks the part of the destination that already exists and refuses a link or a file in the way.
fn check_existing_dest(dest_root: &Path, segs: &[String]) -> Result<(), String> {
    let mut cur = dest_root.to_path_buf();
    for seg in segs {
        cur.push(seg);
        match fs::symlink_metadata(&cur) {
            Ok(m) => {
                if is_reparse(&m) {
                    return Err(
                        "the destination folder is a link, and Krovi never follows links.".into(),
                    );
                }
                if !m.is_dir() {
                    return Err("a file has the destination folder's name.".into());
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err("Krovi could not read the destination folder.".into()),
        }
    }
    Ok(())
}

/// The top level of `root`, sorted by name so the preview is the same every time.
fn list_entries(root: &Path, c: &mut Counts) -> io::Result<Vec<(String, fs::Metadata)>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(root)? {
        let Ok(entry) = entry else {
            c.unreadable += 1;
            continue;
        };
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            c.odd_names += 1;
            continue;
        };
        // DirEntry::metadata does not follow links
        match entry.metadata() {
            Ok(meta) => out.push((name, meta)),
            Err(_) => c.unreadable += 1,
        }
    }
    out.sort_by_key(|(n, _)| n.to_lowercase());
    Ok(out)
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Reads the enabled rules' source folders and lists the moves that would happen. Changes nothing
/// on disk.
pub fn plan_moves(
    preview_id: u64,
    rules: &[Rule],
    folders: &KnownFolders,
    opts: &PlanOptions,
) -> Preview {
    let mut preview = Preview {
        preview_id,
        moves: Vec::new(),
        notes: Vec::new(),
        truncated: false,
        created_ms: system_ms(opts.now),
        rules_checked: 0,
    };
    let mut c = Counts::default();
    let mut missing: Vec<KnownFolder> = Vec::new();
    // files an earlier rule already claimed, and destination names already promised
    let mut claimed: HashSet<String> = HashSet::new();
    let mut reserved: HashSet<String> = HashSet::new();

    'rules: for rule in rules.iter().filter(|r| r.enabled) {
        let (Some(src_root), Some(dest_root)) =
            (folders.get(rule.folder), folders.get(rule.dest_folder))
        else {
            for k in [rule.folder, rule.dest_folder] {
                if folders.get(k).is_none() && !missing.contains(&k) {
                    missing.push(k);
                }
            }
            continue;
        };
        let segs = rule.segments();
        if segs.is_empty() {
            continue;
        }
        if let Err(why) = check_existing_dest(dest_root, &segs) {
            preview
                .notes
                .push(format!("Skipped \"{}\": {why}", rule.describe()));
            continue;
        }
        let dest_dir = segs
            .iter()
            .fold(dest_root.to_path_buf(), |acc, s| acc.join(s));
        let entries = match list_entries(src_root, &mut c) {
            Ok(v) => v,
            Err(_) => {
                c.unreadable += 1;
                continue;
            }
        };
        preview.rules_checked += 1;

        for (name, meta) in entries {
            if !rule.name_matches(&name) {
                continue;
            }
            if meta.is_dir() {
                continue;
            }
            if is_reparse(&meta) {
                c.links += 1;
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            if is_hidden_or_system(&meta, &name) {
                c.hidden += 1;
                continue;
            }
            let modified = meta.modified().ok();
            if !rule.age_matches(modified, opts.now) {
                continue;
            }
            let quiet = modified
                .and_then(|m| opts.now.duration_since(m).ok())
                .is_some_and(|age| age >= MIN_QUIET);
            if is_unfinished_download(&name) || !quiet {
                c.recent += 1;
                continue;
            }
            let from = src_root.join(&name);
            if has_wildcard(&from) {
                continue;
            }
            if claimed.contains(&key(&from)) {
                continue;
            }
            let Some(final_name) = free_name(&dest_dir, &name, &reserved) else {
                c.no_name += 1;
                continue;
            };
            let target = dest_dir.join(&final_name);
            if path_too_long(&target) {
                c.too_long += 1;
                continue;
            }
            if preview.moves.len() >= opts.max_moves {
                preview.truncated = true;
                break 'rules;
            }
            claimed.insert(key(&from));
            reserved.insert(key(&target));
            preview.moves.push(PlannedMove {
                id: format!("m{}", preview.moves.len() + 1),
                rule_id: rule.id.clone(),
                src_folder: rule.folder,
                dest_folder: rule.dest_folder,
                dest_sub: segs.join("\\"),
                from: from.to_string_lossy().into_owned(),
                to_dir: dest_dir.to_string_lossy().into_owned(),
                to_name: final_name,
                size: meta.len(),
                modified_ms: modified_ms(&meta).unwrap_or(0),
            });
        }
    }

    for k in missing {
        preview.notes.push(format!(
            "{} was not found on this PC, so the rules that use it were skipped.",
            k.label()
        ));
    }
    if c.links > 0 {
        preview.notes.push(format!(
            "{} {} (OneDrive and similar). Krovi never follows links, so {} left alone.",
            plural(c.links, "matching file", "matching files"),
            if c.links == 1 {
                "is a link or cloud placeholder"
            } else {
                "are links or cloud placeholders"
            },
            if c.links == 1 { "it was" } else { "they were" },
        ));
    }
    if c.hidden > 0 {
        preview.notes.push(format!(
            "{} matching hidden or system {} left alone.",
            c.hidden,
            if c.hidden == 1 {
                "file was"
            } else {
                "files were"
            },
        ));
    }
    if c.recent > 0 {
        preview.notes.push(format!(
            "{} changed in the last two minutes or still {} like unfinished downloads, so {} left alone.",
            plural(c.recent, "matching file", "matching files"),
            if c.recent == 1 { "looks" } else { "look" },
            if c.recent == 1 { "it was" } else { "they were" },
        ));
    }
    if c.too_long > 0 {
        preview.notes.push(format!(
            "{} would end up with a path too long for Windows, so {} left alone.",
            plural(c.too_long, "matching file", "matching files"),
            if c.too_long == 1 {
                "it was"
            } else {
                "they were"
            },
        ));
    }
    if c.no_name > 0 {
        preview.notes.push(format!(
            "{} could not get a free name in the destination (names up to ({MAX_CLASH_SUFFIX}) are tried), so {} left alone.",
            plural(c.no_name, "matching file", "matching files"),
            if c.no_name == 1 { "it was" } else { "they were" },
        ));
    }
    if c.odd_names > 0 {
        preview.notes.push(format!(
            "{} with unusual names were skipped.",
            plural(c.odd_names, "file", "files")
        ));
    }
    if c.unreadable > 0 {
        preview.notes.push(format!(
            "{} could not be read, so {} skipped.",
            plural(c.unreadable, "item", "items"),
            if c.unreadable == 1 {
                "it was"
            } else {
                "they were"
            },
        ));
    }
    if preview.truncated {
        preview.notes.push(format!(
            "More than {MAX_MOVES} files match. Only the first {MAX_MOVES} are listed: run these, then preview again for the rest."
        ));
    }
    preview
}

// ---------------------------------------------------------------------------------------------
// moving
// ---------------------------------------------------------------------------------------------

/// A file that Krovi moved. This is also the undo journal's entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Moved {
    pub from: String,
    pub to: String,
    pub size: u64,
    pub modified_ms: u64,
    pub src_folder: KnownFolder,
    pub dest_folder: KnownFolder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub moved: Vec<Moved>,
    pub failed: Vec<Failure>,
    pub already_gone: u64,
}

#[derive(Debug, PartialEq, Eq)]
enum Invalid {
    Gone,
    Link,
    Changed,
    Outside,
    TooLong,
    BadName,
    Unreadable(io::ErrorKind),
    Dest(String),
    Io(String),
}

impl Invalid {
    fn reason(&self) -> String {
        match self {
            Invalid::Gone => "It was already gone.".into(),
            Invalid::Link => "It is now a link (shortcut or cloud placeholder). Krovi never follows links, so it was left alone.".into(),
            Invalid::Changed => "It changed after the preview, so it was left alone. Preview again to see it.".into(),
            Invalid::Outside => "It is no longer where the preview found it, so it was left alone.".into(),
            Invalid::TooLong => "Its new path would be too long for Windows, so it was left alone.".into(),
            Invalid::BadName => "Its name contains a character the Windows shell treats specially, so it was left alone.".into(),
            Invalid::Unreadable(io::ErrorKind::PermissionDenied) => "Krovi has no permission to open it, so it was left alone.".into(),
            Invalid::Unreadable(_) => "Krovi could not read it, so it was left alone.".into(),
            Invalid::Dest(s) | Invalid::Io(s) => s.clone(),
        }
    }
}

fn unreadable(e: io::Error) -> Invalid {
    match e.kind() {
        io::ErrorKind::NotFound => Invalid::Gone,
        k => Invalid::Unreadable(k),
    }
}

/// Creates the destination folder chain below `dest_root` one level at a time, refusing a link or a
/// file anywhere on it, and returns the resolved folder. Folders are only ever created.
fn prepare_dest_dir(dest_root: &Path, segs: &[String]) -> Result<PathBuf, String> {
    let mut cur = dest_root.to_path_buf();
    for seg in segs {
        cur.push(seg);
        let verdict = |m: &fs::Metadata| -> Result<(), String> {
            if is_reparse(m) {
                return Err(
                    "The destination folder is a link, and Krovi never follows links.".into(),
                );
            }
            if !m.is_dir() {
                return Err("A file has the destination folder's name.".into());
            }
            Ok(())
        };
        match fs::symlink_metadata(&cur) {
            Ok(m) => verdict(&m)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                match fs::create_dir(&cur) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => {
                        return Err(format!(
                            "Krovi could not create the destination folder. {}",
                            io_reason(&e)
                        ))
                    }
                }
                let m = fs::symlink_metadata(&cur).map_err(|_| {
                    "Krovi could not read the destination folder it just made.".to_string()
                })?;
                verdict(&m)?;
            }
            Err(_) => return Err("Krovi could not read the destination folder.".into()),
        }
    }
    let canon = canonical(&cur)
        .map_err(|_| "Krovi could not resolve the destination folder.".to_string())?;
    if !same_path(&canon, &cur) || !inside_strict(&canon, dest_root) {
        return Err(
            "The destination folder is not where Krovi expected, so nothing was moved there."
                .into(),
        );
    }
    Ok(canon)
}

/// Reserves a name with an atomic create-new. Fails with `AlreadyExists` if anything has it.
fn reserve(p: &Path) -> io::Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(p)
        .map(|_| ())
}

/// Removes the empty placeholder [`reserve`] made, and only that: it must still be an empty regular
/// file. Nothing else is ever deleted.
fn release(p: &Path) {
    if let Ok(m) = fs::symlink_metadata(p) {
        if m.is_file() && m.len() == 0 && !is_reparse(&m) {
            let _ = fs::remove_file(p);
        }
    }
}

/// Renames `from` to `dir/name`, or to `dir/name (2)` ... if that name is taken. Nothing that
/// already exists is replaced: the name is reserved first, and the rename then replaces only the
/// empty placeholder this function made.
fn move_without_overwrite(from: &Path, dir: &Path, name: &str) -> io::Result<PathBuf> {
    for n in 1..=MAX_CLASH_SUFFIX {
        let cand = dir.join(clash_name(name, n));
        match reserve(&cand) {
            Ok(()) => {
                return match fs::rename(from, &cand) {
                    Ok(()) => Ok(cand),
                    Err(e) => {
                        release(&cand);
                        Err(e)
                    }
                };
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free name in the destination",
    ))
}

fn run_one(m: &PlannedMove, folders: &KnownFolders) -> Result<Moved, Invalid> {
    let src_root = folders.get(m.src_folder).ok_or(Invalid::Outside)?;
    let dest_root = folders.get(m.dest_folder).ok_or(Invalid::Outside)?;
    let from = PathBuf::from(&m.from);
    if has_wildcard(&from) {
        return Err(Invalid::BadName);
    }
    let name = from
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(Invalid::BadName)?
        .to_string();
    // directly inside its known folder, and nowhere else
    if !from.parent().is_some_and(|p| same_path(p, src_root)) {
        return Err(Invalid::Outside);
    }
    let segs = clean_subfolder(&m.dest_sub).map_err(|_| Invalid::Outside)?;

    let meta = fs::symlink_metadata(&from).map_err(unreadable)?;
    if is_reparse(&meta) {
        return Err(Invalid::Link);
    }
    if !meta.is_file() || is_hidden_or_system(&meta, &name) {
        return Err(Invalid::Changed);
    }
    // the path must still resolve to itself: a link swapped in anywhere along it makes it differ
    let canon = canonical(&from).map_err(unreadable)?;
    if !same_path(&canon, &from) {
        return Err(Invalid::Outside);
    }
    if meta.len() != m.size || modified_ms(&meta).unwrap_or(0) != m.modified_ms {
        return Err(Invalid::Changed);
    }
    let prospective = segs
        .iter()
        .fold(dest_root.to_path_buf(), |a, s| a.join(s))
        .join(&name);
    if path_too_long(&prospective) {
        return Err(Invalid::TooLong);
    }

    let dest_dir = prepare_dest_dir(dest_root, &segs).map_err(Invalid::Dest)?;
    let to =
        move_without_overwrite(&canon, &dest_dir, &name).map_err(|e| Invalid::Io(io_reason(&e)))?;
    Ok(Moved {
        from: canon.to_string_lossy().into_owned(),
        to: to.to_string_lossy().into_owned(),
        size: m.size,
        modified_ms: m.modified_ms,
        src_folder: m.src_folder,
        dest_folder: m.dest_folder,
    })
}

/// Moves what the preview listed. Each file is re-checked against the disk right before it is
/// moved; one that changed, became a link, vanished or cannot be moved is reported and left alone.
pub fn run_moves(moves: &[PlannedMove], folders: &KnownFolders) -> RunReport {
    let mut report = RunReport::default();
    for m in moves {
        match run_one(m, folders) {
            Ok(done) => report.moved.push(done),
            Err(Invalid::Gone) => report.already_gone += 1,
            Err(e) => report.failed.push(Failure {
                path: m.from.clone(),
                reason: e.reason(),
            }),
        }
    }
    report
}

// ---------------------------------------------------------------------------------------------
// undo
// ---------------------------------------------------------------------------------------------

/// What the last run moved, kept so it can be undone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoJournal {
    pub run_id: u64,
    pub at_ms: u64,
    pub moves: Vec<Moved>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoReport {
    pub restored: u64,
    pub failed: Vec<Failure>,
}

fn undo_one(e: &Moved, folders: &KnownFolders) -> Result<(), String> {
    let src_root = folders
        .get(e.src_folder)
        .ok_or("The original folder was not found.")?;
    let dest_root = folders
        .get(e.dest_folder)
        .ok_or("The destination folder was not found.")?;
    let to = PathBuf::from(&e.to);
    let from = PathBuf::from(&e.from);
    if has_wildcard(&to) || has_wildcard(&from) {
        return Err("A name contains a character the Windows shell treats specially.".into());
    }
    if !inside_strict(&to, dest_root) || !from.parent().is_some_and(|p| same_path(p, src_root)) {
        return Err("The recorded paths are not inside the folders Krovi works in.".into());
    }
    let meta = match fs::symlink_metadata(&to) {
        Ok(m) => m,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err("It is no longer where Krovi put it, so it was left alone.".into())
        }
        Err(_) => return Err("Krovi could not read it, so it was left alone.".into()),
    };
    if is_reparse(&meta) || !meta.is_file() {
        return Err("It is no longer an ordinary file, so it was left alone.".into());
    }
    if meta.len() != e.size || modified_ms(&meta).unwrap_or(0) != e.modified_ms {
        return Err(
            "It changed or was replaced after it was moved, so it was left where it is.".into(),
        );
    }
    let canon = canonical(&to)
        .map_err(|_| "Krovi could not resolve it, so it was left alone.".to_string())?;
    if !same_path(&canon, &to) {
        return Err("It is no longer where Krovi put it, so it was left alone.".into());
    }
    match fs::symlink_metadata(&from) {
        Ok(_) => {
            return Err(
                "Something with the original name is there now, so it was not moved back.".into(),
            )
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {
            return Err("Krovi could not read the original folder, so it was left alone.".into())
        }
    }
    match reserve(&from) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            return Err(
                "Something with the original name is there now, so it was not moved back.".into(),
            )
        }
        Err(err) => return Err(io_reason(&err)),
    }
    if let Err(err) = fs::rename(&canon, &from) {
        release(&from);
        return Err(io_reason(&err));
    }
    Ok(())
}

/// Puts back the files of the last run, newest first. A file is moved back only if it is still
/// exactly what was moved (same size and modification time, not a link) and its original name is
/// free; anything else is reported and left where it is. Folders the run created are left in
/// place (nothing is ever deleted).
pub fn undo_moves(journal: &UndoJournal, folders: &KnownFolders) -> UndoReport {
    let mut report = UndoReport::default();
    for e in journal.moves.iter().rev() {
        match undo_one(e, folders) {
            Ok(()) => report.restored += 1,
            Err(reason) => report.failed.push(Failure {
                path: e.to.clone(),
                reason,
            }),
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{make_dir_link, remove_dir_link, set_age, TempDir, DAY};

    struct Fx {
        t: TempDir,
        folders: KnownFolders,
        dl: PathBuf,
        docs: PathBuf,
    }

    fn fx() -> Fx {
        let t = TempDir::new("org");
        let dl = t.mkdir("Downloads");
        let docs = t.mkdir("Documents");
        let folders = KnownFolders::from_pairs([
            (KnownFolder::Downloads, dl.clone()),
            (KnownFolder::Documents, docs.clone()),
        ]);
        Fx {
            t,
            folders,
            dl,
            docs,
        }
    }

    fn rule(exts: &[&str], older: Option<u32>) -> Rule {
        validate_rule(&Rule {
            id: "rule-1".into(),
            enabled: true,
            folder: KnownFolder::Downloads,
            extensions: exts.iter().map(|s| s.to_string()).collect(),
            name_contains: String::new(),
            older_than_days: older,
            dest_folder: KnownFolder::Documents,
            dest_subfolder: "PDFs".into(),
        })
        .unwrap()
    }

    fn plan(f: &Fx, rules: &[Rule]) -> Preview {
        plan_moves(1, rules, &f.folders, &PlanOptions::default())
    }

    // ---- rules ----

    #[test]
    fn a_rule_reads_back_as_the_sentence_it_was_written_as() {
        let r = rule(&["pdf"], Some(7));
        assert_eq!(
            r.describe(),
            "In Downloads, move .pdf files older than 7 days to Documents\\PDFs"
        );
        let mut r2 = r.clone();
        r2.extensions = vec!["jpg".into(), "png".into()];
        r2.name_contains = "holiday".into();
        r2.older_than_days = Some(1);
        assert_eq!(
            r2.describe(),
            "In Downloads, move .jpg, .png files with \"holiday\" in the name older than 1 day to Documents\\PDFs"
        );
    }

    #[test]
    fn extensions_are_normalised_and_bad_ones_refused() {
        let ok =
            clean_extensions(&["*.PDF".into(), ".docx, xlsx;  PDF".into(), "".into()]).unwrap();
        assert_eq!(ok, vec!["pdf", "docx", "xlsx"]);
        assert_eq!(
            clean_extensions(&["a/b".into()]),
            Err(RuleError::BadExtension)
        );
        assert_eq!(
            clean_extensions(&["pdf.exe".into()]),
            Err(RuleError::BadExtension)
        );
        assert_eq!(
            clean_extensions(&["x".repeat(MAX_EXTENSION_CHARS + 1)]),
            Err(RuleError::BadExtension)
        );
        let many: Vec<String> = (0..=MAX_EXTENSIONS).map(|n| format!("e{n}")).collect();
        assert_eq!(clean_extensions(&many), Err(RuleError::TooManyExtensions));
        assert_eq!(
            clean_extensions(&["*".into()]).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn destination_folders_must_be_plain_relative_names() {
        assert_eq!(clean_subfolder("PDFs").unwrap(), vec!["PDFs"]);
        assert_eq!(
            clean_subfolder(" Work / 2024 \\ Invoices\\ ").unwrap(),
            vec!["Work", "2024", "Invoices"]
        );
        for bad in [
            "..",
            "a\\..\\b",
            ".",
            "a\\.\\b",
            "C:\\Windows",
            "c:x",
            "con",
            "NUL.txt",
            "lpt1",
            "a\\\\b",
            "a?",
            "a*",
            "a<b",
            "a|b",
            "bad.",
            "x\u{7}y",
        ] {
            assert!(clean_subfolder(bad).is_err(), "{bad:?} should be refused");
        }
        assert_eq!(
            clean_subfolder("\\Windows"),
            Err(RuleError::SubfolderAbsolute)
        );
        assert_eq!(clean_subfolder("/etc"), Err(RuleError::SubfolderAbsolute));
        assert_eq!(clean_subfolder("  "), Err(RuleError::SubfolderEmpty));
        assert_eq!(
            clean_subfolder("a\\b\\c\\d"),
            Err(RuleError::SubfolderTooDeep)
        );
        assert_eq!(
            clean_subfolder(&"x".repeat(MAX_SEGMENT_CHARS + 1)),
            Err(RuleError::SubfolderSegmentTooLong)
        );
        assert!(clean_subfolder(&"x".repeat(MAX_SEGMENT_CHARS)).is_ok());
        // com10 is an ordinary name, com1 is not
        assert!(clean_subfolder("com10").is_ok());
        assert!(clean_subfolder("COM1").is_err());
    }

    #[test]
    fn a_rule_needs_a_condition_and_a_sane_age() {
        let base = Rule {
            id: String::new(),
            enabled: true,
            folder: KnownFolder::Downloads,
            extensions: vec![],
            name_contains: String::new(),
            older_than_days: None,
            dest_folder: KnownFolder::Documents,
            dest_subfolder: "Old".into(),
        };
        assert_eq!(validate_rule(&base), Err(RuleError::NoCondition));
        // an age of 0 is no condition at all
        assert_eq!(
            validate_rule(&Rule {
                older_than_days: Some(0),
                ..base.clone()
            }),
            Err(RuleError::NoCondition)
        );
        assert!(validate_rule(&Rule {
            older_than_days: Some(30),
            ..base.clone()
        })
        .is_ok());
        assert_eq!(
            validate_rule(&Rule {
                older_than_days: Some(MAX_AGE_DAYS + 1),
                ..base.clone()
            }),
            Err(RuleError::AgeBad)
        );
        assert_eq!(
            validate_rule(&Rule {
                name_contains: "a/b".into(),
                ..base.clone()
            }),
            Err(RuleError::ContainsBad)
        );
        assert_eq!(
            validate_rule(&Rule {
                name_contains: "x".repeat(MAX_CONTAINS_CHARS + 1),
                ..base.clone()
            }),
            Err(RuleError::ContainsTooLong)
        );
        assert_eq!(
            validate_rule(&Rule {
                name_contains: "inv".into(),
                dest_subfolder: "".into(),
                ..base
            }),
            Err(RuleError::SubfolderEmpty)
        );
    }

    #[test]
    fn conditions_match_names_and_ages() {
        let mut r = rule(&["pdf", "docx"], Some(7));
        r.name_contains = "Invoice".into();
        assert!(r.name_matches("my-INVOICE-2024.PDF"));
        assert!(r.name_matches("invoice.docx"));
        assert!(!r.name_matches("invoice.txt"));
        assert!(
            !r.name_matches("invoice"),
            "no extension is not a match for a type"
        );
        assert!(!r.name_matches(".pdf"), "a dot file has no extension");
        assert!(!r.name_matches("report.pdf"));

        let now = SystemTime::now();
        assert!(r.age_matches(Some(now - 8 * DAY), now));
        assert!(r.age_matches(Some(now - 7 * DAY), now));
        assert!(!r.age_matches(Some(now - 6 * DAY), now));
        assert!(
            !r.age_matches(None, now),
            "no readable time never matches an age rule"
        );
        assert!(
            !r.age_matches(Some(now + DAY), now),
            "a date in the future is not old"
        );
        assert!(rule(&["pdf"], None).age_matches(None, now));
    }

    #[test]
    fn the_store_numbers_rules_and_is_lenient_when_loading() {
        let mut s = Rules::new();
        let a = s
            .upsert(Rule {
                id: String::new(),
                ..rule(&["pdf"], Some(7))
            })
            .unwrap();
        let b = s
            .upsert(Rule {
                id: String::new(),
                ..rule(&["jpg"], None)
            })
            .unwrap();
        assert_eq!((a.id.as_str(), b.id.as_str()), ("rule-1", "rule-2"));
        let edited = s
            .upsert(Rule {
                id: a.id.clone(),
                ..rule(&["pdf", "docx"], Some(30))
            })
            .unwrap();
        assert_eq!(edited.id, a.id);
        assert_eq!(s.list().len(), 2);
        // an id that is not stored is never trusted
        let c = s
            .upsert(Rule {
                id: "evil".into(),
                ..rule(&["png"], None)
            })
            .unwrap();
        assert_eq!(c.id, "rule-3");
        assert!(s.remove(&b.id) && !s.remove(&b.id));

        let loaded = Rules::from_values(vec![
            serde_json::to_value(&edited).unwrap(),
            serde_json::json!({"id": "rule-9", "folder": "downloads", "destFolder": "documents", "destSubfolder": "..\\x", "extensions": ["pdf"]}),
            serde_json::json!({"id": "rule-8", "folder": "windows", "destFolder": "documents", "destSubfolder": "x"}),
            serde_json::to_value(&edited).unwrap(),
        ]);
        assert_eq!(loaded.list().len(), 1);
    }

    #[test]
    fn the_store_is_capped() {
        let mut s = Rules::new();
        for _ in 0..MAX_RULES {
            s.upsert(Rule {
                id: String::new(),
                ..rule(&["pdf"], None)
            })
            .unwrap();
        }
        assert_eq!(
            s.upsert(Rule {
                id: String::new(),
                ..rule(&["pdf"], None)
            }),
            Err(RuleError::TooMany)
        );
    }

    // ---- preview ----

    #[test]
    fn the_preview_lists_exactly_the_old_matching_files_and_changes_nothing() {
        let f = fx();
        f.t.write_aged("Downloads/old.pdf", 10, 10 * DAY);
        f.t.write_aged("Downloads/OLD2.PDF", 20, 9 * DAY);
        f.t.write_aged("Downloads/young.pdf", 30, 2 * DAY);
        f.t.write_aged("Downloads/old.txt", 40, 20 * DAY);
        f.t.write_aged("Downloads/noext", 5, 20 * DAY);
        f.t.mkdir("Downloads/folder.pdf"); // a folder with a matching name
        let p = plan(&f, &[rule(&["pdf"], Some(7))]);
        let mut names: Vec<&str> = p.moves.iter().map(|m| m.to_name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["OLD2.PDF", "old.pdf"]);
        assert_eq!(p.rules_checked, 1);
        assert!(!p.truncated && p.notes.is_empty(), "{:?}", p.notes);
        let m = p.moves.iter().find(|m| m.to_name == "old.pdf").unwrap();
        assert_eq!(m.size, 10);
        assert!(m.from.ends_with("old.pdf") && m.to_dir.ends_with("PDFs"));
        assert_eq!(m.dest_sub, "PDFs");
        // nothing moved, nothing created
        assert!(f.dl.join("old.pdf").exists());
        assert!(!f.docs.join("PDFs").exists());
    }

    #[test]
    fn disabled_rules_and_missing_folders_are_reported_not_guessed() {
        let f = fx();
        f.t.write_aged("Downloads/a.pdf", 1, 10 * DAY);
        let mut off = rule(&["pdf"], None);
        off.enabled = false;
        assert!(plan(&f, &[off]).moves.is_empty());

        let mut to_music = rule(&["pdf"], None);
        to_music.dest_folder = KnownFolder::Music; // not present in this fixture
        let p = plan(&f, &[to_music]);
        assert!(p.moves.is_empty());
        assert!(
            p.notes.iter().any(|n| n.contains("Music was not found")),
            "{:?}",
            p.notes
        );
    }

    #[test]
    fn files_still_being_written_and_unfinished_downloads_are_left_alone() {
        let f = fx();
        f.t.write("Downloads/just-now.pdf", 3); // changed a moment ago
        f.t.write_aged("Downloads/video.pdf.crdownload", 3, 30 * DAY);
        f.t.write_aged("Downloads/settled.pdf", 3, Duration::from_secs(600));
        let p = plan(&f, &[rule(&["pdf", "crdownload"], None)]);
        let names: Vec<&str> = p.moves.iter().map(|m| m.to_name.as_str()).collect();
        assert_eq!(names, vec!["settled.pdf"]);
        assert!(
            p.notes
                .iter()
                .any(|n| n.contains("2 matching files changed in the last two minutes")),
            "{:?}",
            p.notes
        );
    }

    #[test]
    fn a_name_clash_is_planned_as_a_numbered_name_never_an_overwrite() {
        let f = fx();
        f.t.write_aged("Downloads/report.pdf", 1, 10 * DAY);
        f.t.write("Documents/PDFs/report.pdf", 77);
        f.t.write("Documents/PDFs/report (2).pdf", 78);
        let p = plan(&f, &[rule(&["pdf"], None)]);
        assert_eq!(p.moves.len(), 1);
        assert_eq!(p.moves[0].to_name, "report (3).pdf");
        assert_eq!(
            fs::metadata(f.docs.join("PDFs").join("report.pdf"))
                .unwrap()
                .len(),
            77
        );
    }

    #[test]
    fn two_rules_never_claim_the_same_file_and_never_promise_the_same_name() {
        let f = fx();
        f.t.write_aged("Downloads/a.pdf", 1, 10 * DAY);
        let first = Rule {
            id: "rule-1".into(),
            ..rule(&["pdf"], None)
        };
        let second = Rule {
            id: "rule-2".into(),
            dest_subfolder: "Other".into(),
            ..rule(&["pdf"], None)
        };
        let p = plan(&f, &[first.clone(), second]);
        assert_eq!(p.moves.len(), 1, "the first rule to match wins");
        assert_eq!(p.moves[0].rule_id, "rule-1");

        // two different files with the same name from two different places cannot both be a.pdf
        let docs_rule = Rule {
            id: "rule-3".into(),
            folder: KnownFolder::Documents,
            dest_folder: KnownFolder::Documents,
            dest_subfolder: "PDFs".into(),
            ..rule(&["pdf"], None)
        };
        f.t.write_aged("Documents/a.pdf", 2, 10 * DAY);
        let p = plan(&f, &[first, docs_rule]);
        let mut names: Vec<&str> = p.moves.iter().map(|m| m.to_name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["a (2).pdf", "a.pdf"]);
    }

    #[test]
    fn the_preview_is_capped_and_says_so() {
        let f = fx();
        for n in 0..5 {
            f.t.write_aged(&format!("Downloads/f{n}.pdf"), 1, 10 * DAY);
        }
        let p = plan_moves(
            1,
            &[rule(&["pdf"], None)],
            &f.folders,
            &PlanOptions {
                max_moves: 3,
                ..PlanOptions::default()
            },
        );
        assert_eq!(p.moves.len(), 3);
        assert!(p.truncated);
        assert!(
            p.notes.iter().any(|n| n.contains("Only the first")),
            "{:?}",
            p.notes
        );
        let ids: Vec<&str> = p.moves.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["m1", "m2", "m3"]);
    }

    #[test]
    fn the_preview_does_not_look_into_subfolders() {
        let f = fx();
        f.t.write_aged("Downloads/sub/deep.pdf", 1, 10 * DAY);
        f.t.write_aged("Downloads/top.pdf", 1, 10 * DAY);
        let p = plan(&f, &[rule(&["pdf"], None)]);
        assert_eq!(p.moves.len(), 1);
        assert_eq!(p.moves[0].to_name, "top.pdf");
    }

    #[test]
    fn a_link_that_matches_is_not_followed_or_planned() {
        let f = fx();
        let elsewhere = TempDir::new("org-elsewhere");
        elsewhere.write_aged("secret.pdf", 4, 30 * DAY);
        let link = f.dl.join("shortcut.pdf");
        if !make_dir_link(&link, elsewhere.path()) {
            eprintln!("skipped: this machine does not allow creating directory links");
            return;
        }
        let p = plan(&f, &[rule(&["pdf"], None)]);
        assert!(p.moves.is_empty(), "{:?}", p.moves);
        assert!(
            p.notes.iter().any(|n| n.contains("cloud placeholder")),
            "{:?}",
            p.notes
        );
        assert!(elsewhere.path().join("secret.pdf").exists());
        remove_dir_link(&link);
    }

    #[cfg(windows)]
    #[test]
    fn hidden_files_are_left_alone() {
        let f = fx();
        let hidden = f.t.write_aged("Downloads/hidden.pdf", 1, 10 * DAY);
        f.t.write_aged("Downloads/shown.pdf", 1, 10 * DAY);
        let ok = std::process::Command::new("attrib")
            .arg("+H")
            .arg(&hidden)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("skipped: attrib is not available");
            return;
        }
        let p = plan(&f, &[rule(&["pdf"], None)]);
        let names: Vec<&str> = p.moves.iter().map(|m| m.to_name.as_str()).collect();
        assert_eq!(names, vec!["shown.pdf"]);
        assert!(
            p.notes.iter().any(|n| n.contains("hidden or system")),
            "{:?}",
            p.notes
        );
    }

    #[test]
    fn a_link_on_the_destination_path_skips_the_rule() {
        let f = fx();
        f.t.write_aged("Downloads/a.pdf", 1, 10 * DAY);
        let elsewhere = TempDir::new("org-dest-elsewhere");
        let link = f.docs.join("PDFs");
        if !make_dir_link(&link, elsewhere.path()) {
            eprintln!("skipped: this machine does not allow creating directory links");
            return;
        }
        let p = plan(&f, &[rule(&["pdf"], None)]);
        assert!(p.moves.is_empty());
        assert!(
            p.notes
                .iter()
                .any(|n| n.contains("Skipped") && n.contains("link")),
            "{:?}",
            p.notes
        );
        remove_dir_link(&link);
    }

    // ---- run ----

    #[test]
    fn running_moves_the_files_keeps_their_time_and_creates_the_folder() {
        let f = fx();
        let src = f.t.write_aged("Downloads/old.pdf", 10, 10 * DAY);
        let before = fs::metadata(&src).unwrap().modified().unwrap();
        f.t.write_aged("Downloads/young.pdf", 5, DAY);
        let p = plan(&f, &[rule(&["pdf"], Some(7))]);
        let r = run_moves(&p.moves, &f.folders);
        assert_eq!(r.moved.len(), 1, "{:?}", r.failed);
        assert!(r.failed.is_empty() && r.already_gone == 0);
        let dest = f.docs.join("PDFs").join("old.pdf");
        assert!(dest.is_file() && !src.exists());
        assert_eq!(fs::metadata(&dest).unwrap().len(), 10);
        assert_eq!(
            fs::metadata(&dest).unwrap().modified().unwrap(),
            before,
            "a move keeps the modified time"
        );
        assert!(f.dl.join("young.pdf").exists(), "not selected, not touched");
        assert_eq!(r.moved[0].to, canonical(&dest).unwrap().to_string_lossy());
    }

    #[test]
    fn a_file_that_appeared_at_the_destination_after_the_preview_is_never_replaced() {
        let f = fx();
        f.t.write_aged("Downloads/report.pdf", 1, 10 * DAY);
        let p = plan(&f, &[rule(&["pdf"], None)]);
        assert_eq!(p.moves[0].to_name, "report.pdf");
        // somebody puts a different report.pdf there between the preview and the run
        f.t.write("Documents/PDFs/report.pdf", 99);
        let r = run_moves(&p.moves, &f.folders);
        assert_eq!(r.moved.len(), 1, "{:?}", r.failed);
        assert!(
            r.moved[0].to.ends_with("report (2).pdf"),
            "{}",
            r.moved[0].to
        );
        assert_eq!(
            fs::metadata(f.docs.join("PDFs").join("report.pdf"))
                .unwrap()
                .len(),
            99,
            "untouched"
        );
        assert_eq!(
            fs::metadata(f.docs.join("PDFs").join("report (2).pdf"))
                .unwrap()
                .len(),
            1
        );
        assert!(!f.dl.join("report.pdf").exists());
    }

    #[test]
    fn two_files_with_one_name_both_survive() {
        let f = fx();
        f.t.write_aged("Downloads/a.pdf", 1, 10 * DAY);
        f.t.write_aged("Documents/a.pdf", 2, 10 * DAY);
        let first = Rule {
            id: "rule-1".into(),
            ..rule(&["pdf"], None)
        };
        let docs_rule = Rule {
            id: "rule-2".into(),
            folder: KnownFolder::Documents,
            dest_folder: KnownFolder::Documents,
            ..rule(&["pdf"], None)
        };
        let p = plan(&f, &[first, docs_rule]);
        let r = run_moves(&p.moves, &f.folders);
        assert_eq!(r.moved.len(), 2, "{:?}", r.failed);
        let mut sizes: Vec<u64> = ["a.pdf", "a (2).pdf"]
            .iter()
            .map(|n| fs::metadata(f.docs.join("PDFs").join(n)).unwrap().len())
            .collect();
        sizes.sort();
        assert_eq!(sizes, vec![1, 2]);
    }

    #[test]
    fn a_file_that_changed_after_the_preview_is_left_alone() {
        let f = fx();
        let a = f.t.write_aged("Downloads/a.pdf", 10, 10 * DAY);
        let b = f.t.write_aged("Downloads/b.pdf", 10, 10 * DAY);
        f.t.write_aged("Downloads/c.pdf", 10, 10 * DAY);
        let p = plan(&f, &[rule(&["pdf"], None)]);
        fs::write(&a, b"now longer than before").unwrap(); // size changed
        set_age(&b, Duration::from_secs(60)); // same size, new time
        let r = run_moves(&p.moves, &f.folders);
        assert_eq!(r.moved.len(), 1);
        assert_eq!(r.failed.len(), 2);
        assert!(
            r.failed
                .iter()
                .all(|x| x.reason.contains("changed after the preview")),
            "{:?}",
            r.failed
        );
        assert!(a.exists() && b.exists());
    }

    #[test]
    fn a_file_that_vanished_is_counted_not_failed() {
        let f = fx();
        let a = f.t.write_aged("Downloads/a.pdf", 1, 10 * DAY);
        f.t.write_aged("Downloads/b.pdf", 1, 10 * DAY);
        let p = plan(&f, &[rule(&["pdf"], None)]);
        fs::remove_file(a).unwrap();
        let r = run_moves(&p.moves, &f.folders);
        assert_eq!((r.moved.len(), r.already_gone, r.failed.len()), (1, 1, 0));
    }

    #[test]
    fn a_folder_swapped_for_a_link_on_the_destination_path_is_refused_and_nothing_lands_there() {
        let f = fx();
        f.t.write_aged("Downloads/a.pdf", 1, 10 * DAY);
        let p = plan(&f, &[rule(&["pdf"], None)]);
        // after the preview, `Documents\PDFs` becomes a link to somewhere else
        let elsewhere = TempDir::new("org-run-elsewhere");
        let link = f.docs.join("PDFs");
        if !make_dir_link(&link, elsewhere.path()) {
            eprintln!("skipped: this machine does not allow creating directory links");
            return;
        }
        let r = run_moves(&p.moves, &f.folders);
        assert_eq!(r.moved.len(), 0);
        assert!(r.failed[0].reason.contains("link"), "{:?}", r.failed);
        assert!(f.dl.join("a.pdf").exists());
        assert_eq!(
            fs::read_dir(elsewhere.path()).unwrap().count(),
            0,
            "nothing was written through the link"
        );
        remove_dir_link(&link);
    }

    #[test]
    fn a_link_swapped_into_the_source_path_is_refused() {
        let f = fx();
        f.t.write_aged("Downloads/sub/a.pdf", 1, 10 * DAY);
        // a hand-made move whose source goes through a folder that is really a link elsewhere
        let elsewhere = TempDir::new("org-src-elsewhere");
        elsewhere.write_aged("a.pdf", 1, 10 * DAY);
        fs::remove_dir_all(f.dl.join("sub")).unwrap();
        let link = f.dl.join("sub");
        if !make_dir_link(&link, elsewhere.path()) {
            eprintln!("skipped: this machine does not allow creating directory links");
            return;
        }
        let forged = PlannedMove {
            id: "m1".into(),
            rule_id: "rule-1".into(),
            src_folder: KnownFolder::Downloads,
            dest_folder: KnownFolder::Documents,
            dest_sub: "PDFs".into(),
            from: link.join("a.pdf").to_string_lossy().into_owned(),
            to_dir: f.docs.join("PDFs").to_string_lossy().into_owned(),
            to_name: "a.pdf".into(),
            size: 1,
            modified_ms: 0,
        };
        let r = run_moves(&[forged], &f.folders);
        assert_eq!(r.moved.len(), 0);
        assert!(
            elsewhere.path().join("a.pdf").exists(),
            "the file behind the link is untouched"
        );
        remove_dir_link(&link);
    }

    #[test]
    fn forged_moves_outside_the_known_folders_are_refused() {
        let f = fx();
        let outside = TempDir::new("org-outside");
        let victim = outside.write_aged("precious.pdf", 3, 30 * DAY);
        let nested = f.t.write_aged("Downloads/sub/deep.pdf", 3, 30 * DAY);
        let mk = |from: &Path, sub: &str, src: KnownFolder| PlannedMove {
            id: "m1".into(),
            rule_id: "r".into(),
            src_folder: src,
            dest_folder: KnownFolder::Documents,
            dest_sub: sub.into(),
            from: from.to_string_lossy().into_owned(),
            to_dir: String::new(),
            to_name: "x".into(),
            size: 3,
            modified_ms: 0,
        };
        let dotdot =
            f.dl.join("..")
                .join("..")
                .join(outside.path().file_name().unwrap())
                .join("precious.pdf");
        let r = run_moves(
            &[
                mk(&victim, "PDFs", KnownFolder::Downloads), // not in Downloads at all
                mk(&dotdot, "PDFs", KnownFolder::Downloads), // reaches out with ..
                mk(&nested, "PDFs", KnownFolder::Downloads), // below the top level
                mk(&f.dl.join("a.pdf"), "..\\Escape", KnownFolder::Downloads), // forged destination
                mk(&f.dl.join("a.pdf"), "", KnownFolder::Downloads), // no subfolder
                mk(&victim, "PDFs", KnownFolder::Pictures),  // a folder that is not known here
            ],
            &f.folders,
        );
        assert_eq!(r.moved.len(), 0);
        assert_eq!(r.failed.len(), 6, "{:?}", r.failed);
        assert!(victim.exists() && nested.exists());
        assert!(!f.t.path().join("Escape").exists() && !f.docs.join("PDFs").exists());
    }

    #[cfg(windows)]
    #[test]
    fn a_file_in_use_is_reported_and_leaves_no_placeholder_behind() {
        use std::os::windows::fs::OpenOptionsExt;
        let f = fx();
        let locked = f.t.write_aged("Downloads/locked.pdf", 5, 10 * DAY);
        f.t.write_aged("Downloads/free.pdf", 5, 10 * DAY);
        let p = plan(&f, &[rule(&["pdf"], None)]);
        // an exclusive handle (share mode 0) makes the rename fail like an app holding the file
        let _held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&locked)
            .unwrap();
        let r = run_moves(&p.moves, &f.folders);
        assert_eq!(r.moved.len(), 1);
        assert_eq!(r.failed.len(), 1);
        assert!(r.failed[0].path.ends_with("locked.pdf"));
        assert!(locked.exists());
        let left: Vec<String> = fs::read_dir(f.docs.join("PDFs"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            left,
            vec!["free.pdf"],
            "no empty placeholder is left for the failed move"
        );
    }

    // ---- undo ----

    fn journal(r: &RunReport) -> UndoJournal {
        UndoJournal {
            run_id: 1,
            at_ms: 0,
            moves: r.moved.clone(),
        }
    }

    #[test]
    fn undo_puts_unchanged_files_back_and_leaves_the_new_folder_in_place() {
        let f = fx();
        let a = f.t.write_aged("Downloads/a.pdf", 7, 10 * DAY);
        let before = fs::metadata(&a).unwrap().modified().unwrap();
        let p = plan(&f, &[rule(&["pdf"], None)]);
        let r = run_moves(&p.moves, &f.folders);
        assert!(!a.exists());
        let u = undo_moves(&journal(&r), &f.folders);
        assert_eq!((u.restored, u.failed.len()), (1, 0), "{:?}", u.failed);
        assert!(a.is_file());
        assert_eq!(fs::metadata(&a).unwrap().modified().unwrap(), before);
        assert!(
            f.docs.join("PDFs").is_dir(),
            "nothing is deleted, not even the folder the run made"
        );
        assert_eq!(fs::read_dir(f.docs.join("PDFs")).unwrap().count(), 0);
    }

    #[test]
    fn undo_leaves_a_file_alone_that_changed_or_whose_old_name_is_taken() {
        let f = fx();
        f.t.write_aged("Downloads/edited.pdf", 7, 10 * DAY);
        f.t.write_aged("Downloads/taken.pdf", 7, 10 * DAY);
        f.t.write_aged("Downloads/gone.pdf", 7, 10 * DAY);
        f.t.write_aged("Downloads/fine.pdf", 7, 10 * DAY);
        let p = plan(&f, &[rule(&["pdf"], None)]);
        let r = run_moves(&p.moves, &f.folders);
        assert_eq!(r.moved.len(), 4);
        let pdfs = f.docs.join("PDFs");
        fs::write(pdfs.join("edited.pdf"), b"edited after the move!").unwrap();
        f.t.write("Downloads/taken.pdf", 99); // a new file took the old name
        fs::remove_file(pdfs.join("gone.pdf")).unwrap();
        let u = undo_moves(&journal(&r), &f.folders);
        assert_eq!(u.restored, 1, "{:?}", u.failed);
        assert_eq!(u.failed.len(), 3);
        assert!(f.dl.join("fine.pdf").exists());
        assert!(
            pdfs.join("edited.pdf").exists(),
            "the edited file stays where it is"
        );
        assert!(
            pdfs.join("taken.pdf").exists(),
            "and the one whose name is taken"
        );
        assert_eq!(
            fs::metadata(f.dl.join("taken.pdf")).unwrap().len(),
            99,
            "the newer file is never overwritten"
        );
        let why: String = u
            .failed
            .iter()
            .map(|x| x.reason.as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert!(
            why.contains("changed or was replaced")
                && why.contains("original name")
                && why.contains("no longer where Krovi put it"),
            "{why}"
        );
    }

    #[test]
    fn a_tampered_journal_cannot_move_files_outside_the_known_folders() {
        let f = fx();
        let outside = TempDir::new("org-undo-outside");
        let stray = outside.write_aged("stray.pdf", 3, 5 * DAY);
        let meta = fs::metadata(&stray).unwrap();
        let ms = modified_ms(&meta).unwrap();
        let entry = |to: &Path, from: &Path| Moved {
            from: from.to_string_lossy().into_owned(),
            to: to.to_string_lossy().into_owned(),
            size: 3,
            modified_ms: ms,
            src_folder: KnownFolder::Downloads,
            dest_folder: KnownFolder::Documents,
        };
        let j = UndoJournal {
            run_id: 1,
            at_ms: 0,
            moves: vec![
                entry(&stray, &f.dl.join("stray.pdf")), // "moved" file is outside Documents
                entry(&f.docs.join("x.pdf"), &outside.path().join("y.pdf")), // restore target is outside Downloads
            ],
        };
        let u = undo_moves(&j, &f.folders);
        assert_eq!(u.restored, 0);
        assert_eq!(u.failed.len(), 2);
        assert!(stray.exists() && !f.dl.join("stray.pdf").exists());
    }

    #[test]
    fn moving_back_never_replaces_a_file_that_is_there() {
        let t = TempDir::new("org-moveback");
        let src = t.write("a.txt", 1);
        let dst = t.write("b.txt", 2);
        assert!(
            reserve(&dst).is_err(),
            "an existing file cannot be reserved"
        );
        let r = move_without_overwrite(&src, t.path(), "b.txt").unwrap();
        assert!(r.ends_with("b (2).txt"));
        assert_eq!(fs::metadata(&dst).unwrap().len(), 2);
        assert!(!src.exists());
    }
}
