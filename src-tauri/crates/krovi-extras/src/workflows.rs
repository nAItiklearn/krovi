//! Named workflows: an ordered list of Agent Mode command lines.
//!
//! A workflow is only a saved list of sentences such as `open spotify`, `set a timer for 25
//! minutes`, `open notion.so`. Nothing in it is special: when it runs, each line is planned by the
//! ordinary Agent Mode planner and goes through the same Safe / needs-your-OK / Blocked rules, one
//! line at a time. A line that needs confirmation still shows its card, and a workflow cannot do
//! anything the Agent panel cannot.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Settings key the host persists the workflows under (a JSON array of [`Workflow`]).
pub const WORKFLOWS_KEY: &str = "workflows.v1";
pub const MAX_WORKFLOWS: usize = 30;
pub const MAX_NAME_CHARS: usize = 60;
pub const MAX_LINES: usize = 12;
/// Agent Mode's own input limit is 500; a workflow line stays well inside it.
pub const MAX_LINE_CHARS: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    /// Assigned on first save; leave empty for a new workflow.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowError {
    NameEmpty,
    NameTooLong,
    NameBad,
    NameTaken,
    NoLines,
    TooManyLines,
    LineTooLong,
    LineBad,
    TooMany,
}

impl fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkflowError::NameEmpty => write!(f, "Give the workflow a name."),
            WorkflowError::NameTooLong => {
                write!(
                    f,
                    "That name is too long (over {MAX_NAME_CHARS} characters)."
                )
            }
            WorkflowError::NameBad => write!(f, "The name cannot contain control characters."),
            WorkflowError::NameTaken => write!(f, "A workflow with that name already exists."),
            WorkflowError::NoLines => write!(f, "Add at least one command line."),
            WorkflowError::TooManyLines => {
                write!(f, "A workflow can have at most {MAX_LINES} lines.")
            }
            WorkflowError::LineTooLong => {
                write!(f, "A line is too long (over {MAX_LINE_CHARS} characters).")
            }
            WorkflowError::LineBad => write!(f, "A line contains a control character."),
            WorkflowError::TooMany => write!(f, "At most {MAX_WORKFLOWS} workflows can be saved."),
        }
    }
}

impl std::error::Error for WorkflowError {}

/// Check one workflow and return it tidied: name spacing collapsed, each line trimmed with its
/// inner spacing collapsed, blank lines dropped. The id is left as given.
pub fn validate_workflow(w: &Workflow) -> Result<Workflow, WorkflowError> {
    let name = w.name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(WorkflowError::NameEmpty);
    }
    if w.name.chars().any(char::is_control) {
        return Err(WorkflowError::NameBad);
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(WorkflowError::NameTooLong);
    }
    let mut lines = Vec::new();
    for raw in &w.lines {
        let line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            continue;
        }
        if raw.chars().any(|c| c.is_control() && !c.is_whitespace()) {
            return Err(WorkflowError::LineBad);
        }
        if line.chars().count() > MAX_LINE_CHARS {
            return Err(WorkflowError::LineTooLong);
        }
        lines.push(line);
    }
    if lines.is_empty() {
        return Err(WorkflowError::NoLines);
    }
    if lines.len() > MAX_LINES {
        return Err(WorkflowError::TooManyLines);
    }
    Ok(Workflow {
        id: w.id.clone(),
        name,
        lines,
    })
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let t = out.trim_matches('-');
    let t = if t.is_empty() { "workflow" } else { t };
    t.chars()
        .take(32)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string()
}

/// The user's workflows. Order is insertion order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Workflows {
    items: Vec<Workflow>,
}

impl Workflows {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load the stored values. Lenient on purpose: an entry that no longer validates, or repeats an
    /// id or name, is dropped, so a hand-edited or older value can never wedge the feature.
    pub fn from_values(values: Vec<serde_json::Value>) -> Self {
        let mut out = Workflows::new();
        for v in values {
            let Ok(item) = serde_json::from_value::<Workflow>(v) else {
                continue;
            };
            if item.id.is_empty() || out.items.iter().any(|w| w.id == item.id) {
                continue;
            }
            let Ok(clean) = validate_workflow(&item) else {
                continue;
            };
            if out.name_taken(&clean.name, None) || out.items.len() >= MAX_WORKFLOWS {
                continue;
            }
            out.items.push(clean);
        }
        out
    }

    pub fn list(&self) -> &[Workflow] {
        &self.items
    }

    pub fn get(&self, id: &str) -> Option<&Workflow> {
        self.items.iter().find(|w| w.id == id)
    }

    fn name_taken(&self, name: &str, except: Option<&str>) -> bool {
        self.items
            .iter()
            .any(|w| Some(w.id.as_str()) != except && w.name.to_lowercase() == name.to_lowercase())
    }

    fn fresh_id(&self, name: &str) -> String {
        let base = format!("wf-{}", slug(name));
        let mut id = base.clone();
        let mut n = 2;
        while self.items.iter().any(|w| w.id == id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }

    /// Add a workflow, or update the one with the same id. Returns the stored workflow.
    pub fn upsert(&mut self, input: Workflow) -> Result<Workflow, WorkflowError> {
        let clean = validate_workflow(&input)?;
        let existing = (!input.id.is_empty())
            .then(|| input.id.clone())
            .filter(|id| self.get(id).is_some());
        if self.name_taken(&clean.name, existing.as_deref()) {
            return Err(WorkflowError::NameTaken);
        }
        match existing {
            Some(id) => {
                let stored = Workflow {
                    id: id.clone(),
                    ..clean
                };
                if let Some(slot) = self.items.iter_mut().find(|w| w.id == id) {
                    *slot = stored.clone();
                }
                Ok(stored)
            }
            None => {
                if self.items.len() >= MAX_WORKFLOWS {
                    return Err(WorkflowError::TooMany);
                }
                let stored = Workflow {
                    id: self.fresh_id(&clean.name),
                    ..clean
                };
                self.items.push(stored.clone());
                Ok(stored)
            }
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|w| w.id != id);
        self.items.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn wf(name: &str, lines: &[&str]) -> Workflow {
        Workflow {
            id: String::new(),
            name: name.into(),
            lines: lines.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn validation_tidies_lines_and_drops_blank_ones() {
        let ok = validate_workflow(&wf(
            "  Focus   time ",
            &[
                "open  spotify ",
                "",
                "   ",
                "set a timer for 25 minutes",
                "open notion.so",
            ],
        ))
        .unwrap();
        assert_eq!(ok.name, "Focus time");
        assert_eq!(
            ok.lines,
            vec![
                "open spotify",
                "set a timer for 25 minutes",
                "open notion.so"
            ]
        );
    }

    #[test]
    fn validation_refuses_what_agent_mode_could_not_take() {
        assert_eq!(
            validate_workflow(&wf("", &["open x"])),
            Err(WorkflowError::NameEmpty)
        );
        assert_eq!(
            validate_workflow(&wf("a\u{7}", &["open x"])),
            Err(WorkflowError::NameBad)
        );
        assert_eq!(
            validate_workflow(&wf(&"n".repeat(MAX_NAME_CHARS + 1), &["open x"])),
            Err(WorkflowError::NameTooLong)
        );
        assert_eq!(
            validate_workflow(&wf("a", &[])),
            Err(WorkflowError::NoLines)
        );
        assert_eq!(
            validate_workflow(&wf("a", &["  ", ""])),
            Err(WorkflowError::NoLines)
        );
        assert_eq!(
            validate_workflow(&wf("a", &["ok", "bad\u{0}line"])),
            Err(WorkflowError::LineBad)
        );
        let long = "x".repeat(MAX_LINE_CHARS + 1);
        assert_eq!(
            validate_workflow(&wf("a", &[long.as_str()])),
            Err(WorkflowError::LineTooLong)
        );
        let many: Vec<String> = (0..=MAX_LINES).map(|n| format!("open app{n}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert_eq!(
            validate_workflow(&wf("a", &many)),
            Err(WorkflowError::TooManyLines)
        );
        assert_eq!(
            validate_workflow(&wf("a", &many[..MAX_LINES])).map(|w| w.lines.len()),
            Ok(MAX_LINES)
        );
    }

    #[test]
    fn upsert_assigns_ids_and_keeps_names_unique() {
        let mut s = Workflows::new();
        let a = s.upsert(wf("Morning", &["open gmail"])).unwrap();
        assert_eq!(a.id, "wf-morning");
        let b = s.upsert(wf("Morning!", &["open github"])).unwrap();
        assert_eq!(b.id, "wf-morning-2");
        assert_eq!(
            s.upsert(wf("MORNING", &["open x"])),
            Err(WorkflowError::NameTaken)
        );

        let edited = s
            .upsert(Workflow {
                id: a.id.clone(),
                ..wf("Morning", &["open gmail", "open github"])
            })
            .unwrap();
        assert_eq!(edited.id, a.id);
        assert_eq!(s.get(&a.id).unwrap().lines.len(), 2);
        assert_eq!(s.list().len(), 2);

        // an id that is not stored is never trusted
        let c = s
            .upsert(Workflow {
                id: "evil".into(),
                ..wf("Third", &["open x"])
            })
            .unwrap();
        assert_eq!(c.id, "wf-third");

        assert!(s.remove(&b.id));
        assert!(!s.remove(&b.id));
    }

    #[test]
    fn the_list_is_capped() {
        let mut s = Workflows::new();
        for n in 0..MAX_WORKFLOWS {
            s.upsert(wf(&format!("flow {n}"), &["open x"])).unwrap();
        }
        assert_eq!(
            s.upsert(wf("one more", &["open x"])),
            Err(WorkflowError::TooMany)
        );
    }

    #[test]
    fn loading_drops_bad_duplicate_and_idless_entries() {
        let s = Workflows::from_values(vec![
            json!({"id": "wf-a", "name": "A", "lines": ["open x"]}),
            json!({"id": "wf-a", "name": "A2", "lines": ["open x"]}),
            json!({"id": "wf-b", "name": "a", "lines": ["open x"]}),
            json!({"id": "", "name": "No id", "lines": ["open x"]}),
            json!({"id": "wf-c", "name": "C", "lines": []}),
            json!({"id": "wf-d", "name": "D"}),
            json!(42),
            json!({"id": "wf-e", "name": "E", "lines": ["open y", " "]}),
        ]);
        let ids: Vec<&str> = s.list().iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["wf-a", "wf-e"]);
        assert_eq!(s.get("wf-e").unwrap().lines, vec!["open y"]);
    }
}
