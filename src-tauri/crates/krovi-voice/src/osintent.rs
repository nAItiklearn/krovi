//! Voice v3 phrasings: "open [folder|app]", "what's in [folder]", "find [name]". Pure text in,
//! [`Parsed`] out. Names are resolved only against the [`Catalog`](crate::catalog::Catalog) in the
//! [`Env`]; nothing is ever guessed beyond the catalog's own matching rules.

use crate::catalog::{Catalog, Lookup};
use crate::intent::{Env, Parsed};
use crate::parser::{is_tool_name, Tok};
use crate::types::{ResultItem, ResultKind, VoiceCommand};

/// Longest search text kept (characters).
const MAX_QUERY: usize = 100;

// ---------------------------------------------------------------------------------------------
// "find invoice march" / "search for budget"
// ---------------------------------------------------------------------------------------------

const SEARCH_PREFIXES: &[&[&str]] = &[
    &["search", "my", "files", "for"],
    &["search", "my", "computer", "for"],
    &["search", "my", "pc", "for"],
    &["search", "files", "for"],
    &["search", "for", "files", "named"],
    &["search", "for", "a", "file", "named"],
    &["search", "for", "the", "file"],
    &["search", "for", "file"],
    &["search", "for"],
    &["find", "files", "named"],
    &["find", "a", "file", "named"],
    &["find", "the", "file", "named"],
    &["find", "the", "file"],
    &["find", "file", "named"],
    &["find", "files"],
    &["find", "file"],
    &["find", "me"],
    &["look", "for"],
    &["locate"],
    &["find"],
    &["search"],
];

/// The spoken prefixes the command grammar offers for a search (the body is free dictation).
///
/// A bare "find" / "look for" / "locate" is deliberately NOT here although the parser accepts
/// them when typed: measured, a grammar path "find <anything>" made the recognizer swallow noisy
/// audio that is really another command (female synthetic voice + white noise: 16/18 commands
/// understood without it, 9/18 with it). These six cost nothing measurable (15/18).
pub(crate) const SEARCH_GRAMMAR_PREFIXES: &[&str] = &[
    "find file",
    "find files",
    "find the file",
    "find me",
    "search for",
    "search my files for",
];

const QUERY_LEAD: &[&str] = &["named", "called", "the", "a", "an", "my"];
const WEB_WORDS: &[&str] = &[
    "web",
    "internet",
    "online",
    "google",
    "bing",
    "youtube",
    "wikipedia",
    "reddit",
    "amazon",
];

pub(crate) fn try_search(toks: &[Tok], text: &str) -> Option<Parsed> {
    let words: Vec<&str> = toks.iter().map(|t| t.norm.as_str()).collect();
    let plen = SEARCH_PREFIXES
        .iter()
        .filter(|p| words.len() >= p.len() && words[..p.len()] == ***p)
        .map(|p| p.len())
        .max()?;
    let mut idx = plen;
    while idx < words.len() && QUERY_LEAD.contains(&words[idx]) {
        idx += 1;
    }
    if idx >= words.len() {
        return Some(Parsed::Unresolved {
            feedback: "Say what to search for, for example: find invoice.".into(),
            results: vec![],
        });
    }
    if WEB_WORDS.contains(&words[idx]) {
        return Some(Parsed::Unresolved {
            feedback: "Krovi searches the files on this PC only, not the web.".into(),
            results: vec![],
        });
    }
    // Keep the user's own spelling of what follows, when it starts on a word boundary.
    let same_chunk = idx > 0 && toks[idx - 1].start == toks[idx].start;
    let raw = if same_chunk {
        words[idx..].join(" ")
    } else {
        text[toks[idx].start..].trim().to_string()
    };
    let query: String = raw
        .trim()
        .trim_end_matches(['.', ',', '!', '?', ';', ':'])
        .trim()
        .chars()
        .take(MAX_QUERY)
        .collect();
    if query.is_empty() {
        return Some(Parsed::Unresolved {
            feedback: "Say what to search for, for example: find invoice.".into(),
            results: vec![],
        });
    }
    Some(Parsed::Command(VoiceCommand::SearchFiles { query }))
}

// ---------------------------------------------------------------------------------------------
// "what's in downloads" / "tell me about my documents folder"
// ---------------------------------------------------------------------------------------------

const LIST_PREFIXES: &[(&[&str], bool)] = &[
    // (prefix, true = this phrasing always means a folder question)
    (&["show", "me", "whats", "in"], true),
    (&["show", "me", "what", "is", "in"], true),
    (&["show", "me", "whats", "inside"], true),
    (&["whats", "inside", "of"], true),
    (&["what", "is", "inside", "of"], true),
    (&["whats", "inside"], true),
    (&["what", "is", "inside"], true),
    (&["whats", "in"], true),
    (&["what", "is", "in"], true),
    (&["list", "the", "contents", "of"], true),
    (&["list", "contents", "of"], true),
    (&["list", "files", "in"], true),
    (&["list"], false),
    (&["tell", "me", "about"], false),
];

pub(crate) const LIST_GRAMMAR_PREFIXES: &[&str] = &[
    "what's in",
    "what is in",
    "what's inside",
    "what is inside",
    "tell me about",
    "list",
    "list the contents of",
    "show me what's in",
];

const DETERMINERS: &[&str] = &["the", "my", "this", "our"];
const FOLDER_NOUNS: &[&str] = &["folder", "directory", "folders"];
const APP_NOUNS: &[&str] = &["app", "application", "program", "software"];

fn catalog_of(env: &Env) -> Option<&Catalog> {
    env.catalog.as_deref().filter(|c| !c.is_empty())
}

fn strip_determiners<'a>(mut w: &'a [&'a str]) -> &'a [&'a str] {
    while let Some((first, tail)) = w.split_first() {
        if DETERMINERS.contains(first) || *first == "a" || *first == "an" {
            w = tail;
        } else {
            break;
        }
    }
    w
}

/// Strip one trailing "folder"/"app" style noun; says which it was.
fn strip_noun<'a>(w: &'a [&'a str]) -> (&'a [&'a str], Option<Explicit>) {
    if let Some((last, head)) = w.split_last() {
        if !head.is_empty() {
            if FOLDER_NOUNS.contains(last) {
                return (head, Some(Explicit::Folder));
            }
            if APP_NOUNS.contains(last) {
                return (head, Some(Explicit::App));
            }
        }
    }
    (w, None)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Explicit {
    Folder,
    App,
}

fn row(kind: ResultKind, e: &crate::catalog::Entry) -> ResultItem {
    ResultItem::new(kind, e.name.clone(), e.path.clone())
}

fn ambiguous(what: &str, spoken: &str, kind: ResultKind, hits: &[crate::catalog::Entry]) -> Parsed {
    let names: Vec<&str> = hits.iter().map(|e| e.name.as_str()).collect();
    Parsed::Unresolved {
        feedback: format!(
            "More than one {what} matches \"{spoken}\": {}. Say the full name.",
            names.join(", ")
        ),
        results: hits.iter().map(|e| row(kind, e)).collect(),
    }
}

pub(crate) fn try_list(words: &[&str], env: &Env) -> Option<Parsed> {
    let catalog = catalog_of(env)?;
    let (plen, always) = LIST_PREFIXES
        .iter()
        .filter(|(p, _)| words.len() > p.len() && words[..p.len()] == **p)
        .map(|(p, always)| (p.len(), *always))
        .max_by_key(|(l, _)| *l)?;
    let (rest, _) = strip_noun(strip_determiners(&words[plen..]));
    if rest.is_empty() {
        return None;
    }
    let spoken = rest.join(" ");
    match catalog.find_folder(rest) {
        Lookup::Found(e) => Some(Parsed::Command(VoiceCommand::ListFolder {
            name: e.name,
            path: e.path,
        })),
        Lookup::Ambiguous(hits) => Some(ambiguous("folder", &spoken, ResultKind::Folder, &hits)),
        Lookup::NotFound if always => Some(Parsed::Unresolved {
            feedback: format!(
                "I only know my folders (Desktop, Documents, Downloads, Pictures, Music, Videos, Krovi downloads and your pinned or recent folders); \"{spoken}\" is not one of them."
            ),
            results: vec![],
        }),
        // "tell me about the weather": not a folder question at all
        Lookup::NotFound => None,
    }
}

// ---------------------------------------------------------------------------------------------
// "open downloads" / "launch spotify"
// ---------------------------------------------------------------------------------------------

/// Apps first (a Krovi tool of the same name is only used if no installed app matches).
const APP_VERBS: &[&[&str]] = &[
    &["launch"],
    &["start", "up"],
    &["fire", "up"],
    &["run"],
    &["start"],
];
/// Folders first.
const FOLDER_VERBS: &[&[&str]] = &[
    &["go", "to"],
    &["browse", "to"],
    &["browse"],
    &["take", "me", "to"],
    &["navigate", "to"],
    &["explore"],
];
/// Krovi tools win, then the better folder/app match.
const BOTH_VERBS: &[&[&str]] = &[
    &["bring", "up"],
    &["pull", "up"],
    &["show", "me"],
    &["open", "up"],
    &["open"],
    &["show"],
    &["view"],
    &["display"],
];

/// The grammar's verbs, per kind (the grammar module builds its templates from these).
pub(crate) const FOLDER_GRAMMAR_VERBS: &[&str] = &[
    "open",
    "show",
    "show me",
    "go to",
    "browse",
    "take me to",
    "bring up",
];
pub(crate) const APP_GRAMMAR_VERBS: &[&str] = &["open", "launch", "start", "run", "start up"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum VerbKind {
    App,
    Folder,
    Both,
}

pub(crate) enum OsOpen {
    Resolved(Parsed),
    /// It looked like "open X" but X is neither a folder nor an app: the feedback to give if no
    /// built-in tool matches either.
    Miss(String),
    Pass,
}

fn leading_verb(words: &[&str]) -> Option<(usize, VerbKind)> {
    let mut best: Option<(usize, VerbKind)> = None;
    for (list, kind) in [
        (APP_VERBS, VerbKind::App),
        (FOLDER_VERBS, VerbKind::Folder),
        (BOTH_VERBS, VerbKind::Both),
    ] {
        for v in list {
            if words.len() > v.len()
                && words[..v.len()] == **v
                && !best.is_some_and(|(l, _)| v.len() <= l)
            {
                best = Some((v.len(), kind));
            }
        }
    }
    best
}

pub(crate) fn try_open(words: &[&str], env: &Env) -> OsOpen {
    let Some(catalog) = catalog_of(env) else {
        return OsOpen::Pass;
    };
    let Some((vlen, kind)) = leading_verb(words) else {
        return OsOpen::Pass;
    };
    let (rest, explicit) = strip_noun(strip_determiners(&words[vlen..]));
    if rest.is_empty() || rest.len() > 6 {
        return OsOpen::Pass;
    }
    let spoken = rest.join(" ");

    let folder = |l: Lookup| match l {
        Lookup::Found(e) => Some(Parsed::Command(VoiceCommand::OpenFolder {
            name: e.name,
            path: e.path,
        })),
        Lookup::Ambiguous(hits) => Some(ambiguous("folder", &spoken, ResultKind::Folder, &hits)),
        Lookup::NotFound => None,
    };
    let app = |l: Lookup| match l {
        Lookup::Found(e) => Some(Parsed::Command(VoiceCommand::OpenApp {
            name: e.name,
            path: e.path,
        })),
        Lookup::Ambiguous(hits) => Some(ambiguous("app", &spoken, ResultKind::App, &hits)),
        Lookup::NotFound => None,
    };

    let resolved = match (explicit, kind) {
        (Some(Explicit::Folder), _) => folder(catalog.find_folder(rest)),
        (Some(Explicit::App), _) => app(catalog.find_app(rest)),
        (None, VerbKind::App) => app(catalog.find_app(rest)),
        (None, VerbKind::Folder) => folder(catalog.find_folder(rest)),
        (None, VerbKind::Both) => {
            // A Krovi tool called exactly that wins ("open timers"); say "folder" or "launch"
            // for the Windows folder / app of the same name.
            if is_tool_name(rest) {
                return OsOpen::Pass;
            }
            let (f_look, f_score) = catalog.find_folder_scored(rest);
            let (a_look, a_score) = catalog.find_app_scored(rest);
            if f_score >= a_score {
                folder(f_look).or_else(|| app(a_look))
            } else {
                app(a_look).or_else(|| folder(f_look))
            }
        }
    };
    if let Some(p) = resolved {
        return OsOpen::Resolved(p);
    }
    OsOpen::Miss(match (explicit, kind) {
        (Some(Explicit::Folder), _) | (None, VerbKind::Folder) => {
            format!("No known folder is called \"{spoken}\".")
        }
        (Some(Explicit::App), _) => format!("No installed app is called \"{spoken}\"."),
        _ => format!("No folder or installed app is called \"{spoken}\"."),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Catalog, Entry};
    use crate::civil::{Date, LocalNow};
    use crate::intent::parse;
    use std::sync::Arc;
    use VoiceCommand::*;

    fn catalog() -> Arc<Catalog> {
        let app = |n: &str| Entry::new(n, format!("shell:AppsFolder\\{n}"));
        Arc::new(Catalog::new(
            vec![
                Entry::new("Desktop", r"C:\u\Desktop"),
                Entry::new("Documents", r"C:\u\Documents"),
                Entry::new("Downloads", r"C:\u\Downloads"),
                Entry::new("Pictures", r"C:\u\Pictures"),
                Entry::new("Music", r"C:\u\Music"),
                Entry::new("Videos", r"C:\u\Videos"),
                Entry::new("Krovi downloads", r"C:\u\Downloads\Krovi"),
                Entry::new("Projects", r"D:\Projects"),
            ],
            vec![
                app("Notepad"),
                app("Spotify"),
                app("Visual Studio Code"),
                app("Google Chrome"),
                app("Calculator"),
                app("Settings"),
                app("Photo Editor Pro"),
                app("Photo Viewer Plus"),
            ],
        ))
    }

    fn env() -> Env {
        Env::at(LocalNow::new(Date::new(2026, 11, 1).unwrap(), 10, 30), None)
            .with_catalog(Some(catalog()))
    }

    fn cmd(s: &str) -> Option<VoiceCommand> {
        parse(s, &env()).command()
    }

    fn folder(name: &str, path: &str) -> Option<VoiceCommand> {
        Some(OpenFolder {
            name: name.into(),
            path: path.into(),
        })
    }

    fn app(name: &str) -> Option<VoiceCommand> {
        Some(OpenApp {
            name: name.into(),
            path: format!("shell:AppsFolder\\{name}"),
        })
    }

    #[test]
    fn open_folder_phrasings() {
        for s in [
            "open downloads",
            "Open the downloads folder.",
            "show my downloads",
            "go to downloads",
            "take me to downloads",
            "bring up downloads",
            "browse downloads",
            "open my Downloads folder please",
        ] {
            assert_eq!(cmd(s), folder("Downloads", r"C:\u\Downloads"), "{s}");
        }
        assert_eq!(
            cmd("open krovi downloads"),
            folder("Krovi downloads", r"C:\u\Downloads\Krovi")
        );
        assert_eq!(cmd("open projects"), folder("Projects", r"D:\Projects"));
        assert_eq!(cmd("open docs"), folder("Documents", r"C:\u\Documents"));
        assert_eq!(cmd("open the desktop"), folder("Desktop", r"C:\u\Desktop"));
    }

    #[test]
    fn open_app_phrasings() {
        for s in [
            "launch notepad",
            "open notepad",
            "start notepad",
            "run notepad",
            "open the notepad app",
            "fire up notepad",
        ] {
            assert_eq!(cmd(s), app("Notepad"), "{s}");
        }
        assert_eq!(cmd("launch vs code"), app("Visual Studio Code"));
        assert_eq!(cmd("open spotify"), app("Spotify"));
        assert_eq!(cmd("open chrome"), app("Google Chrome"));
        assert_eq!(cmd("start up spotify"), app("Spotify"));
    }

    #[test]
    fn krovi_tools_win_for_open_but_launch_picks_the_app() {
        let tool = |t| Some(OpenTool { tool: t });
        use crate::types::ToolId::*;
        assert_eq!(cmd("open timers"), tool(Timers));
        assert_eq!(cmd("open the calendar"), tool(Calendar));
        assert_eq!(cmd("open settings"), tool(Settings), "Krovi's own Settings");
        assert_eq!(cmd("open calculator"), tool(Calculator));
        assert_eq!(
            cmd("launch settings"),
            app("Settings"),
            "the Windows app, by 'launch'"
        );
        assert_eq!(cmd("launch calculator"), app("Calculator"));
        // the Music folder needs the word folder (Krovi has a Music tool)
        assert_eq!(cmd("open music"), tool(Media));
        assert_eq!(cmd("open music folder"), folder("Music", r"C:\u\Music"));
        // with no app of that name installed, launch falls back to the tool
        assert_eq!(cmd("launch timers"), tool(Timers));
        // the Explain tool, with a catalog present (no folder/app is named like it)
        for s in [
            "open explain",
            "open the explainer",
            "show me the summarizer",
            "go to summary",
            "launch summarize",
            "open read aloud",
        ] {
            assert_eq!(cmd(s), tool(Explain), "{s}");
        }
    }

    #[test]
    fn unknown_names_are_not_guessed() {
        let r = parse("open photoshop", &env());
        match r {
            Parsed::Unresolved { feedback, results } => {
                assert!(feedback.contains("photoshop"), "{feedback}");
                assert!(results.is_empty());
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            parse("open the door", &env()),
            Parsed::Unresolved { .. }
        ));
        // without a catalog nothing changes: unknown stays unknown
        let bare = Env::at(LocalNow::new(Date::new(2026, 11, 1).unwrap(), 10, 30), None);
        assert_eq!(parse("open photoshop", &bare), Parsed::Unknown);
        assert_eq!(parse("open downloads", &bare), Parsed::Unknown);
    }

    #[test]
    fn ambiguous_names_list_the_candidates_and_do_nothing() {
        match parse("open photo", &env()) {
            Parsed::Unresolved { feedback, results } => {
                assert!(feedback.contains("More than one"), "{feedback}");
                assert_eq!(results.len(), 2);
                assert!(results.iter().all(|r| r.kind == ResultKind::App));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn list_folder_phrasings() {
        for s in [
            "what's in downloads",
            "what's in my downloads folder",
            "What is in the Downloads folder?",
            "tell me about downloads",
            "tell me about my downloads folder",
            "list downloads",
            "list the contents of downloads",
            "show me what's in downloads",
            "what's inside downloads",
        ] {
            assert_eq!(
                cmd(s),
                Some(ListFolder {
                    name: "Downloads".into(),
                    path: r"C:\u\Downloads".into()
                }),
                "{s}"
            );
        }
        // "tell me about" is only a folder question when the name is a folder
        assert_eq!(parse("tell me about the weather", &env()), Parsed::Unknown);
        assert!(matches!(
            parse("what's in secrets", &env()),
            Parsed::Unresolved { .. }
        ));
    }

    #[test]
    fn search_phrasings_keep_the_users_words() {
        let q = |s: &str| cmd(s);
        let want = |s: &str| Some(SearchFiles { query: s.into() });
        assert_eq!(q("find invoice"), want("invoice"));
        assert_eq!(q("Find invoice March."), want("invoice March"));
        assert_eq!(q("search for budget 2026"), want("budget 2026"));
        assert_eq!(q("search my files for tax return"), want("tax return"));
        assert_eq!(q("look for resume"), want("resume"));
        assert_eq!(q("locate boarding pass"), want("boarding pass"));
        assert_eq!(q("find the file named Q3-results"), want("Q3-results"));
        assert_eq!(q("find me passport scan"), want("passport scan"));
        assert_eq!(q("find files budget"), want("budget"));
        assert_eq!(q("find report.pdf"), want("report.pdf"));
    }

    #[test]
    fn search_without_a_term_or_for_the_web_is_explained() {
        for s in ["find", "search", "search for"] {
            match parse(s, &env()) {
                Parsed::Unresolved { feedback, .. } => {
                    assert!(feedback.contains("search for"), "{s}: {feedback}")
                }
                other => panic!("{s}: {other:?}"),
            }
        }
        match parse("search the web for cats", &env()) {
            Parsed::Unresolved { feedback, .. } => {
                assert!(feedback.contains("not the web"), "{feedback}")
            }
            other => panic!("{other:?}"),
        }
        match parse("search google for cats", &env()) {
            Parsed::Unresolved { feedback, .. } => assert!(feedback.contains("not the web")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn existing_commands_are_untouched() {
        use crate::types::ToolId::*;
        assert_eq!(cmd("start a pomodoro"), Some(StartPomodoro));
        assert_eq!(
            cmd("start a timer for 5 minutes"),
            Some(StartTimer { seconds: 300 })
        );
        assert_eq!(cmd("close"), Some(Collapse));
        assert_eq!(
            cmd("show me the clipboard"),
            Some(OpenTool { tool: Clipboard })
        );
        assert_eq!(cmd("go to the calendar"), Some(OpenTool { tool: Calendar }));
        assert_eq!(
            cmd("new note buy milk"),
            Some(NewNote {
                body: "Buy milk".into()
            })
        );
    }
}
