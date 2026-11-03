//! Krovi extras: snippets, file-organizer rules, workflows and file-name search.
//!
//! Four small, local-only features that share one rule: **Krovi does only what you ask, when you
//! ask, and shows you first.**
//!
//! * [`snippets`]: saved text you copy to the clipboard on purpose, with `{date}`, `{time}` and
//!   `{clipboard}` filled in. No keyboard hook, no auto-expansion, nothing watches your typing.
//! * [`organizer`]: rules like "In Downloads, move .pdf files older than 7 days to
//!   Documents\PDFs". Preview the exact moves, confirm, run, undo. Moves only (never deletes,
//!   never overwrites), only inside your known folders, never through a link.
//! * [`workflows`]: named lists of Agent Mode command lines. They are planned and permission-checked
//!   by the ordinary Agent planner, line by line.
//! * [`search`]: file-name search in your known folders. No index, no background work, no file
//!   contents read, cancellable, bounded.
//!
//! Everything here is plain Rust and unit-tested against scratch folders under the system temp
//! directory. The Tauri glue (`src-tauri/src/extras.rs`) only stores the data, calls these
//! functions and turns their callbacks into events. No network anywhere in this crate.

pub mod fsx;
pub mod known;
#[cfg(all(windows, feature = "native"))]
pub mod native;
pub mod organizer;
pub mod search;
pub mod snippets;
#[cfg(test)]
pub(crate) mod testutil;
pub mod workflows;

pub use known::{KnownFolder, KnownFolders};
pub use organizer::{
    plan_moves, run_moves, undo_moves, validate_rule, Failure, Moved, PlanOptions, PlannedMove,
    Preview, Rule, RuleError, Rules, RunReport, UndoJournal, UndoReport, MAX_MOVES, RULES_KEY,
    UNDO_KEY,
};
pub use search::{parse_query, search_names, Hit, SearchOptions, Stop, Summary};
pub use snippets::{
    expand, uses_clipboard, validate_snippet, ExpandCtx, Expanded, Snippet, SnippetError, Snippets,
    SNIPPETS_KEY,
};
pub use workflows::{validate_workflow, Workflow, WorkflowError, Workflows, WORKFLOWS_KEY};
