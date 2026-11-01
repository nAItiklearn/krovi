//! Krovi desktop shell (Tauri 2) — wiring only.
//!
//! Every behaviour lives in an owned module (see docs/CONTRACTS.md §5):
//! - `state`    shared AppState (live-data)
//! - `commands` data commands over krovi-core / krovi-store / krovi-net (live-data)
//! - `live`     background live hub: OS clipboard, GSMTC media, system stats (live-data)
//! - `shell`    Island window model, tray, global shortcut, single-instance (panel-shell)
//! - `voice`    local speech: dictation notes + voice commands (voice)
//!
//! Each agent edits only its own `[block]` in the handler list below.

mod commands;
mod live;
mod shell;
mod state;
mod voice;

/// Entry point shared by the desktop binary.
pub fn run() {
    let builder = shell::plugins(tauri::Builder::default());

    builder
        .manage(state::AppState::seeded())
        .setup(|app| {
            live::setup(app)?;
            shell::setup(app)?;
            voice::setup(app)?;
            Ok(())
        })
        .on_window_event(shell::on_window_event)
        .invoke_handler(tauri::generate_handler![
            // [live] — owner: live-data
            commands::load_clipboard,
            commands::add_clip,
            commands::toggle_clip_pin,
            commands::remove_clip,
            commands::load_paste_stack,
            commands::push_paste,
            commands::pop_paste,
            commands::load_notes,
            commands::create_note,
            commands::update_note,
            commands::delete_note,
            commands::load_tasks,
            commands::add_task,
            commands::toggle_task,
            commands::load_shelves,
            commands::load_screen_time,
            commands::load_weather,
            commands::load_media,
            commands::load_world_clocks,
            commands::load_currency_rates,
            commands::load_calendar,
            commands::fetch_weather,
            commands::fetch_currency_rates,
            commands::convert_currency,
            commands::now_playing,
            commands::media_transport,
            commands::foreground_tick,
            live::live_snapshot,
            // [/live]
            // [shell] — owner: panel-shell
            shell::shell_set_mode,
            shell::shell_quit,
            // [/shell]
            // [voice] — owner: voice
            voice::voice_available,
            // [/voice]
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Krovi application");
}
