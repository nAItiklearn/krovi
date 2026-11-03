//! Wire and in-memory shapes. Everything the webview receives is camelCase.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Serialize, Serializer};

use crate::categories::Shape;

#[allow(clippy::ptr_arg)]
fn ser_path<S: Serializer>(p: &PathBuf, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&p.to_string_lossy())
}

/// What the UI shows for one category before anything is scanned.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CategoryInfo {
    pub id: &'static str,
    pub title: &'static str,
    /// Plain English: what this is.
    pub description: &'static str,
    /// Plain English: what could go wrong and what Krovi does about it.
    pub risk_note: &'static str,
    pub default_selected: bool,
    /// Short label next to the title, e.g. "Your files - review first".
    pub badge: Option<&'static str>,
    /// Report-only: no checkbox, nothing is ever deleted (the Recycle Bin).
    pub read_only: bool,
}

/// One thing that can be moved to the Recycle Bin: a file, or a whole folder whose entire
/// contents were checked.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScanItem {
    /// Stable within one scan: `<category>#<n>`. This is what the UI sends back.
    pub id: String,
    pub category: &'static str,
    #[serde(serialize_with = "ser_path")]
    pub path: PathBuf,
    /// Friendly name for folders that are not obvious from the path ("Chrome / Default / Cache").
    pub label: Option<String>,
    pub is_dir: bool,
    pub bytes: u64,
    /// Files inside (1 for a file).
    pub files: u64,
    /// Newest modification inside, epoch ms.
    pub modified_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CategoryResult {
    pub id: &'static str,
    /// Everything Krovi would move for this category (listed + unlisted).
    pub bytes: u64,
    pub files: u64,
    /// The largest items first. Only these can be selected.
    pub items: Vec<ScanItem>,
    /// Smaller items beyond the per-category list cap. Counted in `bytes`/`files`, never touched.
    pub unlisted_items: u64,
    pub unlisted_bytes: u64,
    pub unlisted_files: u64,
    /// Kept because it changed within the category's minimum age (it may be in use).
    pub skipped_recent: u64,
    /// Links (symlinks, junctions, cloud placeholders) that were not followed.
    pub skipped_links: u64,
    /// Folders nested deeper than the depth cap.
    pub skipped_deep: u64,
    /// Entries Windows would not let Krovi read (no permission, vanished, ...).
    pub unreadable: u64,
    /// How many of the category's folders exist on this PC.
    pub roots_found: u32,
}

/// A fixed, validated copy of what was scanned. Not serialised: used to re-check every path.
#[derive(Clone, Debug)]
pub struct Guard {
    pub category: &'static str,
    pub roots: Vec<GuardRoot>,
    pub min_age: Option<Duration>,
}

#[derive(Clone, Debug)]
pub struct GuardRoot {
    /// Canonical path of the folder at scan time.
    pub canon: PathBuf,
    pub shape: Shape,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scan {
    /// Increases with every scan; a delete request must quote it.
    pub scan_id: u64,
    pub categories: Vec<CategoryResult>,
    /// The scan was stopped early: the list is correct but incomplete.
    pub cancelled: bool,
    pub scanned_at_ms: u64,
    #[serde(skip)]
    pub guards: Vec<Guard>,
}

impl Scan {
    pub fn category(&self, id: &str) -> Option<&CategoryResult> {
        self.categories.iter().find(|c| c.id == id)
    }

    pub fn guard(&self, id: &str) -> Option<&Guard> {
        self.guards.iter().find(|g| g.category == id)
    }

    pub fn item(&self, id: &str) -> Option<&ScanItem> {
        self.categories
            .iter()
            .flat_map(|c| c.items.iter())
            .find(|i| i.id == id)
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecycleBinSize {
    pub bytes: u64,
    pub items: u64,
}

/// What the UI asks to delete. Only ids from the stored scan are honoured.
#[derive(Clone, Debug, Default)]
pub struct DeleteRequest {
    pub scan_id: u64,
    pub item_ids: Vec<String>,
    /// Every item of these categories (read-only categories have none).
    pub category_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub path: String,
    pub reason: String,
}

impl Failure {
    pub fn new(path: &Path, reason: impl Into<String>) -> Failure {
        Failure {
            path: path.to_string_lossy().into_owned(),
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeleteReport {
    /// Bytes now in the Recycle Bin (disk space is only freed once it is emptied).
    pub freed_bytes: u64,
    /// Items moved to the Recycle Bin.
    pub deleted: u64,
    pub deleted_files: u64,
    pub failed: Vec<Failure>,
    /// Items that were already gone when the delete ran.
    pub already_gone: u64,
    pub cancelled: bool,
    /// Valid items never attempted because the run was cancelled.
    pub not_attempted: u64,
}
