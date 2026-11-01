//! Local voice (owner: voice): dictation → notes, and voice commands.
//! See docs/CONTRACTS.md §3. STUB: replace with the real implementation.

use serde_json::{json, Value};

pub fn setup(_app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[tauri::command]
pub fn voice_available() -> Value {
    json!({ "dictation": false, "commands": false, "engine": "none" })
}
