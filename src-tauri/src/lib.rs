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
mod explain;
mod live;
mod reminders;
mod shell;
mod state;
mod transfer;
mod voice;

/// Entry point shared by the desktop binary.
pub fn run() {
    let builder =
        shell::plugins(tauri::Builder::default()).plugin(tauri_plugin_notification::init());

    builder
        .manage(state::AppState::seeded())
        .setup(|app| {
            live::setup(app)?;
            shell::setup(app)?;
            voice::setup(app)?;
            transfer::setup(app)?;
            reminders::setup(app)?;
            explain::setup(app)?;
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
            commands::weather_status,
            commands::weather_search,
            commands::weather_set_place,
            commands::weather_set_city,
            commands::load_media,
            commands::load_world_clocks,
            commands::load_currency_rates,
            commands::load_currency_info,
            commands::load_calendar,
            commands::add_calendar_event,
            commands::remove_calendar_event,
            commands::fetch_weather,
            commands::fetch_currency_rates,
            commands::convert_currency,
            commands::now_playing,
            commands::foreground_tick,
            live::live_snapshot,
            live::clipboard_copy,
            live::clipboard_clear,
            live::media_transport,
            // [/live]
            // [shell] — owner: panel-shell
            shell::shell_set_mode,
            shell::shell_get_mode,
            shell::shell_hold,
            shell::shell_quit,
            shell::shell_get_config,
            shell::shell_set_config,
            shell::shell_set_hit_rect,
            // [/shell]
            // [voice] — owner: voice
            voice::voice_available,
            voice::voice_start,
            voice::voice_stop,
            voice::voice_transcribe_file,
            voice::voice_get_settings,
            voice::voice_set_settings,
            voice::voice_known_targets,
            voice::voice_open_result,
            // [/voice]
            // [transfer] — owner: transfer
            transfer::transfer_share,
            transfer::transfer_receive,
            transfer::transfer_stop,
            transfer::transfer_status,
            transfer::transfer_pick_files,
            transfer::transfer_get_config,
            transfer::transfer_set_config,
            transfer::transfer_open_folder,
            transfer::transfer_reveal,
            transfer::transfer_firewall_status,
            transfer::transfer_firewall_allow,
            // [/transfer]
            // [reminders] — owner: reminders
            reminders::reminders_status,
            reminders::reminder_add,
            reminders::reminder_set,
            reminders::reminder_snooze,
            reminders::reminder_dismiss,
            reminders::reminder_test_fire,
            // [/reminders]
            // [explain] — owner: explain
            explain::explain_summarize,
            explain::explain_engines,
            explain::explain_get_settings,
            explain::explain_set_settings,
            explain::explain_set_key,
            explain::explain_eleven_voices,
            explain::explain_test_voice,
            explain::explain_open_keys_page,
            explain::explain_speak,
            explain::explain_stop_speaking,
            explain::explain_estimate,
            // [/explain]
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Krovi application");
}
