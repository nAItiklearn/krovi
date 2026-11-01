//! The closed set of things "open ..." and "what's in ..." may name (Voice v3): folders and
//! installed apps, as enumerated by the OS layer (`os.rs`), plus the pure matching rules.
//!
//! Nothing here touches the OS. The recognizer can only hear names that are in this catalog (they
//! become words of the command grammar when a session starts), and the parser resolves the words
//! it hears back to exactly one entry, or to "not found" / "ambiguous". There is no searching the
//! whole disk and no guessing: a name that is not in the catalog is simply not understood.

use crate::grammar::{ONES, TEENS, TENS};
use crate::parser::edit_distance;

/// Most folders / apps kept (and offered to the recognizer).
pub const MAX_FOLDERS: usize = 80;
pub const MAX_APPS: usize = 400;

/// Words that mark a Start-menu entry as not being an app you would launch by name.
const JUNK_WORDS: &[&str] = &[
    "uninstall",
    "uninstaller",
    "readme",
    "license",
    "eula",
    "documentation",
    "help",
    "manual",
    "website",
    "tutorial",
    "tutorials",
    "changelog",
];

/// Brand words dropped from the front of a name to get the short spoken form
/// ("Google Chrome" -> "chrome", "Microsoft Edge" -> "edge").
const VENDOR_PREFIXES: &[&str] = &[
    "microsoft",
    "windows",
    "google",
    "adobe",
    "mozilla",
    "apple",
    "amazon",
    "the",
];

/// (spoken alias, the name it stands for). Only offered / used when the target is installed.
const APP_ALIASES: &[(&str, &str)] = &[
    ("vs code", "visual studio code"),
    ("v s code", "visual studio code"),
    ("vscode", "visual studio code"),
    ("cmd", "command prompt"),
    ("command line", "command prompt"),
    ("explorer", "file explorer"),
    ("windows explorer", "file explorer"),
    ("ppt", "powerpoint"),
    ("power point", "powerpoint"),
    ("ms word", "word"),
    ("ms excel", "excel"),
    ("ms paint", "paint"),
];

const FOLDER_ALIASES: &[(&str, &str)] = &[
    ("docs", "documents"),
    ("pics", "pictures"),
    ("movies", "videos"),
    ("downloaded files", "downloads"),
];

/// A folder or app the user can name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The display name, exactly as the OS gives it ("Visual Studio Code").
    pub name: String,
    /// Folder path, or app launch target (`shell:AppsFolder\<AUMID>` / a Start-menu `.lnk`).
    pub path: String,
    tokens: Vec<String>,
    short: Vec<String>,
}

impl Entry {
    pub fn new(name: impl Into<String>, path: impl Into<String>) -> Entry {
        let name = name.into();
        let tokens = spoken_words(&name);
        let short = match tokens.split_first() {
            Some((first, rest))
                if !rest.is_empty() && VENDOR_PREFIXES.contains(&first.as_str()) =>
            {
                rest.to_vec()
            }
            _ => tokens.clone(),
        };
        Entry {
            name,
            path: path.into(),
            tokens,
            short,
        }
    }

    /// The words a person would say for this name ("visual studio code").
    pub fn spoken(&self) -> String {
        self.tokens.join(" ")
    }

    /// Every spoken form offered to the recognizer: the full name and, if different, the short one.
    pub fn spoken_forms(&self) -> Vec<String> {
        let mut forms = vec![self.tokens.join(" ")];
        if self.short != self.tokens {
            forms.push(self.short.join(" "));
        }
        forms.retain(|f| !f.is_empty());
        forms
    }
}

/// The outcome of resolving a spoken name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    Found(Entry),
    /// Several entries match equally well: do nothing and ask for the full name.
    Ambiguous(Vec<Entry>),
    NotFound,
}

/// Known folders + the user's pinned/recent folders, and the installed apps.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Catalog {
    pub folders: Vec<Entry>,
    pub apps: Vec<Entry>,
}

impl Catalog {
    /// Dedupes by spoken name (first wins), drops entries nobody could say, drops junk Start-menu
    /// entries (uninstallers, readmes), and caps the sizes.
    pub fn new(folders: Vec<Entry>, apps: Vec<Entry>) -> Catalog {
        let folders = tidy(folders, |_| true, MAX_FOLDERS);
        let mut apps = tidy(
            apps,
            |e| !e.tokens.iter().any(|t| JUNK_WORDS.contains(&t.as_str())) && e.tokens.len() <= 6,
            MAX_APPS,
        );
        apps.sort_by_key(|e| e.name.to_lowercase());
        Catalog { folders, apps }
    }

    pub fn is_empty(&self) -> bool {
        self.folders.is_empty() && self.apps.is_empty()
    }

    /// Every folder name the recognizer should listen for, including the aliases whose target exists.
    pub fn folder_phrases(&self) -> Vec<String> {
        let mut out = phrases(&self.folders);
        for (alias, target) in FOLDER_ALIASES {
            if self.folders.iter().any(|e| e.spoken() == *target) {
                out.push((*alias).to_string());
            }
        }
        dedupe(out)
    }

    /// Every app name the recognizer should listen for, including aliases whose target is installed.
    pub fn app_phrases(&self) -> Vec<String> {
        let mut out = phrases(&self.apps);
        for (alias, target) in APP_ALIASES {
            let q = split(target);
            if matches!(best(&self.apps, &q).1.as_slice(), [_]) {
                out.push((*alias).to_string());
            }
        }
        dedupe(out)
    }

    /// Resolve spoken words (already normalized: lower-case, no punctuation) to a folder.
    pub fn find_folder(&self, words: &[&str]) -> Lookup {
        self.resolve(&self.folders, words, FOLDER_ALIASES).0
    }

    /// Resolve spoken words to an installed app.
    pub fn find_app(&self, words: &[&str]) -> Lookup {
        self.resolve(&self.apps, words, APP_ALIASES).0
    }

    /// Like `find_folder` / `find_app`, with the match quality (0 = none, 100 = exact name).
    pub fn find_folder_scored(&self, words: &[&str]) -> (Lookup, u32) {
        self.resolve(&self.folders, words, FOLDER_ALIASES)
    }

    pub fn find_app_scored(&self, words: &[&str]) -> (Lookup, u32) {
        self.resolve(&self.apps, words, APP_ALIASES)
    }

    fn resolve(
        &self,
        entries: &[Entry],
        words: &[&str],
        aliases: &[(&str, &str)],
    ) -> (Lookup, u32) {
        let mut q = query_tokens(words);
        if q.is_empty() {
            return (Lookup::NotFound, 0);
        }
        let joined = q.join(" ");
        if let Some((_, target)) = aliases.iter().find(|(a, _)| *a == joined) {
            q = split(target);
        }
        let (score, mut top) = best(entries, &q);
        match top.len() {
            0 => (Lookup::NotFound, 0),
            1 => (Lookup::Found(top.remove(0)), score),
            _ => {
                top.truncate(6);
                (Lookup::Ambiguous(top), score)
            }
        }
    }
}

fn split(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

fn dedupe(mut v: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    v.retain(|p| !p.is_empty() && seen.insert(p.clone()));
    v
}

fn phrases(entries: &[Entry]) -> Vec<String> {
    entries.iter().flat_map(|e| e.spoken_forms()).collect()
}

fn tidy(entries: Vec<Entry>, keep: impl Fn(&Entry) -> bool, cap: usize) -> Vec<Entry> {
    let mut seen = std::collections::HashSet::new();
    entries
        .into_iter()
        .filter(|e| !e.tokens.is_empty() && keep(e) && seen.insert(e.spoken()))
        .take(cap)
        .collect()
}

// ---------------------------------------------------------------------------------------------
// names as words
// ---------------------------------------------------------------------------------------------

/// The words a person would say for a display name: lower-case ASCII letters only, digits spelled
/// out, symbols named ("&" -> "and", "+" -> "plus"), bracketed parts ("(x64)") and apostrophes
/// dropped. "Notepad++" -> ["notepad", "plus", "plus"]; "7-Zip" -> ["seven", "zip"].
pub fn spoken_words(name: &str) -> Vec<String> {
    let mut cleaned = String::with_capacity(name.len());
    let mut depth = 0u32;
    for c in name.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            '&' => cleaned.push_str(" and "),
            '+' => cleaned.push_str(" plus "),
            '#' => cleaned.push_str(" sharp "),
            '\'' | '\u{2019}' | '\u{2018}' => {}
            c if c.is_alphanumeric() => cleaned.extend(c.to_lowercase()),
            _ => cleaned.push(' '),
        }
    }
    let mut out = Vec::new();
    for chunk in cleaned.split_whitespace() {
        // split letter/digit boundaries: "mp3" -> "mp", "3"
        let mut cur = String::new();
        let mut cur_digit = false;
        for c in chunk.chars() {
            let is_digit = c.is_ascii_digit();
            if !cur.is_empty() && is_digit != cur_digit {
                flush_word(&mut cur, cur_digit, &mut out);
            }
            cur_digit = is_digit;
            cur.push(c);
        }
        flush_word(&mut cur, cur_digit, &mut out);
    }
    out
}

fn flush_word(cur: &mut String, digit: bool, out: &mut Vec<String>) {
    if cur.is_empty() {
        return;
    }
    if digit {
        out.extend(digits_to_words(cur));
    } else {
        let ascii: String = cur.chars().filter(char::is_ascii_lowercase).collect();
        if !ascii.is_empty() {
            out.push(ascii);
        }
    }
    cur.clear();
}

/// Normalized query words (digits spelled out the same way names are).
fn query_tokens(words: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for w in words {
        out.extend(spoken_words(w));
    }
    out
}

/// "365" -> "three sixty five", "7" -> "seven", "42" -> "forty two". Longer numbers are dropped
/// (nobody could say them the one way).
fn digits_to_words(digits: &str) -> Vec<String> {
    let n: u32 = match digits.parse() {
        Ok(n) if digits.len() <= 3 => n,
        _ => return Vec::new(),
    };
    fn below_100(n: u32) -> Vec<String> {
        match n {
            0 => vec![],
            1..=9 => vec![ONES[(n - 1) as usize].to_string()],
            10..=19 => vec![TEENS[(n - 10) as usize].to_string()],
            _ => {
                let mut v = vec![TENS[(n / 10 - 2) as usize].to_string()];
                let ones = n % 10;
                if ones > 0 {
                    v.push(ONES[(ones - 1) as usize].to_string());
                }
                v
            }
        }
    }
    if n == 0 {
        return vec!["zero".into()];
    }
    if n < 100 {
        return below_100(n);
    }
    let mut v = vec![ONES[(n / 100 - 1) as usize].to_string()];
    match n % 100 {
        0 => v.push("hundred".into()),
        r @ 1..=9 => {
            v.push("oh".into());
            v.extend(below_100(r));
        }
        r => v.extend(below_100(r)),
    }
    v
}

// ---------------------------------------------------------------------------------------------
// matching
// ---------------------------------------------------------------------------------------------

fn fuzzy_eq(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let len = a.chars().count().max(b.chars().count());
    let max = if len >= 9 {
        2
    } else if len >= 5 {
        1
    } else {
        return false;
    };
    edit_distance(a, b) <= max
}

/// 0 = no match; 100 = the name; 95 = the name with spacing differences ("vs code"/"vscode");
/// 80.. = every spoken word is a word of the name; 60.. = the same allowing a typo. Fewer extra
/// words in the name scores higher ("chrome" prefers "Google Chrome" to "Chrome Remote Desktop").
fn score(q: &[String], e: &Entry) -> u32 {
    if q.is_empty() {
        return 0;
    }
    if q == e.tokens.as_slice() || q == e.short.as_slice() {
        return 100;
    }
    let qc = q.concat();
    if qc == e.tokens.concat() || qc == e.short.concat() {
        return 95;
    }
    // A lone very short word ("a", "it") must not select anything by being merely contained.
    if q.len() == 1 && q[0].chars().count() < 3 {
        return 0;
    }
    let mut best_score = 0;
    for toks in [&e.tokens, &e.short] {
        let extras = toks.len().saturating_sub(q.len()) as u32;
        if q.iter().all(|t| toks.contains(t)) {
            best_score = best_score.max(80 - extras.min(15));
        } else if q.iter().all(|t| toks.iter().any(|x| fuzzy_eq(t, x))) {
            best_score = best_score.max(60 - extras.min(10));
        }
    }
    best_score
}

/// The highest score (>= 60) and every entry that reaches it.
fn best(entries: &[Entry], q: &[String]) -> (u32, Vec<Entry>) {
    let mut top = 0;
    let mut hits: Vec<Entry> = Vec::new();
    for e in entries {
        let s = score(q, e);
        if s < 60 || s < top {
            continue;
        }
        if s > top {
            top = s;
            hits.clear();
        }
        hits.push(e.clone());
    }
    hits.sort_by_key(|e| e.name.to_lowercase());
    (top, hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> Entry {
        Entry::new(name, format!("shell:AppsFolder\\{name}"))
    }

    fn folder(name: &str, path: &str) -> Entry {
        Entry::new(name, path)
    }

    fn sample() -> Catalog {
        Catalog::new(
            vec![
                folder("Desktop", r"C:\Users\me\Desktop"),
                folder("Documents", r"C:\Users\me\Documents"),
                folder("Downloads", r"C:\Users\me\Downloads"),
                folder("Pictures", r"C:\Users\me\Pictures"),
                folder("Music", r"C:\Users\me\Music"),
                folder("Videos", r"C:\Users\me\Videos"),
                folder("Krovi downloads", r"C:\Users\me\Downloads\Krovi"),
                folder("Projects", r"D:\Projects"),
            ],
            vec![
                app("Notepad"),
                app("Spotify"),
                app("Visual Studio Code"),
                app("Visual Studio Code - Insiders"),
                app("Google Chrome"),
                app("Chrome Remote Desktop"),
                app("Microsoft Edge"),
                app("Notepad++"),
                app("7-Zip File Manager"),
                app("Command Prompt"),
                app("File Explorer"),
                app("Uninstall Spotify"),
                app("Microsoft 365 (Office)"),
                app("Word"),
                app("Calculator"),
            ],
        )
    }

    fn words(s: &str) -> Vec<&str> {
        s.split_whitespace().collect()
    }

    #[test]
    fn names_become_sayable_words() {
        assert_eq!(spoken_words("Notepad++"), ["notepad", "plus", "plus"]);
        assert_eq!(
            spoken_words("7-Zip File Manager"),
            ["seven", "zip", "file", "manager"]
        );
        assert_eq!(
            spoken_words("Microsoft 365 (Office)"),
            ["microsoft", "three", "sixty", "five"]
        );
        assert_eq!(spoken_words("Rock & Roll"), ["rock", "and", "roll"]);
        assert_eq!(spoken_words("Bob's Café"), ["bobs", "caf"]);
        assert_eq!(spoken_words("MP3 Player"), ["mp", "three", "player"]);
        assert_eq!(spoken_words("Windows 11"), ["windows", "eleven"]);
        assert_eq!(
            spoken_words("Office 2019"),
            ["office"],
            "long numbers are dropped"
        );
        assert!(spoken_words("!!!").is_empty());
    }

    #[test]
    fn junk_and_duplicates_are_dropped_and_sizes_capped() {
        let c = sample();
        assert!(c.apps.iter().all(|a| !a.name.starts_with("Uninstall")));
        let dup = Catalog::new(vec![], vec![app("Spotify"), app("SPOTIFY"), app("spotify")]);
        assert_eq!(dup.apps.len(), 1);
        let many: Vec<Entry> = (0..1000).map(|i| app(&format!("app{} name", i))).collect();
        assert!(Catalog::new(vec![], many).apps.len() <= MAX_APPS);
    }

    #[test]
    fn exact_names_and_short_forms_resolve() {
        let c = sample();
        let name = |l: Lookup| match l {
            Lookup::Found(e) => e.name,
            other => panic!("{other:?}"),
        };
        assert_eq!(name(c.find_app(&words("notepad"))), "Notepad");
        assert_eq!(name(c.find_app(&words("spotify"))), "Spotify");
        assert_eq!(
            name(c.find_app(&words("visual studio code"))),
            "Visual Studio Code"
        );
        assert_eq!(name(c.find_app(&words("chrome"))), "Google Chrome");
        assert_eq!(name(c.find_app(&words("edge"))), "Microsoft Edge");
        assert_eq!(name(c.find_app(&words("google chrome"))), "Google Chrome");
        assert_eq!(name(c.find_app(&words("notepad plus plus"))), "Notepad++");
        assert_eq!(name(c.find_app(&words("7 zip"))), "7-Zip File Manager");
        assert_eq!(name(c.find_app(&words("word"))), "Word");
        assert_eq!(name(c.find_folder(&words("downloads"))), "Downloads");
        assert_eq!(
            name(c.find_folder(&words("krovi downloads"))),
            "Krovi downloads"
        );
        assert_eq!(name(c.find_folder(&words("projects"))), "Projects");
    }

    #[test]
    fn aliases_and_spacing_resolve() {
        let c = sample();
        let name = |l: Lookup| match l {
            Lookup::Found(e) => e.name,
            other => panic!("{other:?}"),
        };
        assert_eq!(name(c.find_app(&words("vs code"))), "Visual Studio Code");
        assert_eq!(name(c.find_app(&words("vscode"))), "Visual Studio Code");
        assert_eq!(name(c.find_app(&words("cmd"))), "Command Prompt");
        assert_eq!(name(c.find_app(&words("explorer"))), "File Explorer");
        assert_eq!(name(c.find_folder(&words("docs"))), "Documents");
        assert_eq!(name(c.find_folder(&words("pics"))), "Pictures");
        assert_eq!(name(c.find_folder(&words("movies"))), "Videos");
    }

    #[test]
    fn fuzzy_tolerates_a_typo_but_not_guessing() {
        let c = sample();
        let found = |s: &str| matches!(c.find_app(&words(s)), Lookup::Found(_));
        assert!(found("spotfy"), "one typo in a long word");
        assert!(found("notepd"));
        assert!(!found("photoshop"), "not installed");
        assert!(!found("door"));
        assert!(!found("a"), "one tiny word selects nothing");
        assert!(!found(""));
        assert_eq!(c.find_folder(&words("secrets")), Lookup::NotFound);
        assert_eq!(c.find_folder(&words("windows")), Lookup::NotFound);
    }

    #[test]
    fn fewer_extra_words_wins_and_true_ties_are_ambiguous() {
        let c = sample();
        // "code" is in both "Visual Studio Code" and "... - Insiders": the plainer one wins.
        match c.find_app(&words("code")) {
            Lookup::Found(e) => assert_eq!(e.name, "Visual Studio Code"),
            other => panic!("{other:?}"),
        }
        let tie = Catalog::new(
            vec![],
            vec![app("Photo Editor Pro"), app("Photo Viewer Plus")],
        );
        match tie.find_app(&words("photo")) {
            Lookup::Ambiguous(v) => assert_eq!(v.len(), 2),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn phrases_cover_full_short_and_alias_forms() {
        let c = sample();
        let apps = c.app_phrases();
        for want in [
            "notepad",
            "google chrome",
            "chrome",
            "microsoft edge",
            "edge",
            "visual studio code",
            "vs code",
            "notepad plus plus",
            "seven zip file manager",
            "cmd",
            "explorer",
        ] {
            assert!(apps.contains(&want.to_string()), "missing {want}: {apps:?}");
        }
        let folders = c.folder_phrases();
        for want in [
            "downloads",
            "krovi downloads",
            "docs",
            "pics",
            "movies",
            "music",
        ] {
            assert!(folders.contains(&want.to_string()), "missing {want}");
        }
        assert!(
            !Catalog::default()
                .app_phrases()
                .contains(&"vs code".to_string()),
            "aliases only when the target is installed"
        );
        // every offered word is plain a-z
        for p in apps.iter().chain(&folders) {
            assert!(
                p.chars().all(|c| c.is_ascii_lowercase() || c == ' '),
                "{p:?}"
            );
        }
    }

    #[test]
    fn scores_rank_exact_above_contained_above_fuzzy() {
        let c = sample();
        assert_eq!(c.find_app_scored(&words("notepad")).1, 100);
        assert!(c.find_app_scored(&words("chrome")).1 >= 80);
        assert!(c.find_app_scored(&words("spotfy")).1 < 80);
        assert_eq!(c.find_app_scored(&words("nothing here")).1, 0);
    }
}
