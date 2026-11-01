//! Live hub (owner: live-data). Background watchers that push events to the UI:
//! `clipboard-changed`, `media-changed`, `stats-tick` — see docs/CONTRACTS.md §2.
//! STUB: replace with the real implementation.

use serde_json::{json, Value};

pub fn setup(_app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[tauri::command]
pub fn live_snapshot() -> Value {
    json!({ "clipboard": [], "media": null, "stats": null })
}
