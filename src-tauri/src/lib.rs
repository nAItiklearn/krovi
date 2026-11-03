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

mod agent;
mod ai;
mod circle;
mod clean;
mod commands;
mod dictation;
mod explain;
mod extras;
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
            dictation::setup(app)?;
            clean::setup(app)?;
            ai::setup(app)?;
            circle::setup(app)?;
            agent::setup(app)?;
            extras::setup(app)?;
            Ok(())
        })
        .on_window_event(shell::on_window_event)
        .invoke_handler(tauri::generate_handler![
            // [ai] — Settings → AI (BYOK providers, D-022/D-023)
            ai::ai_status,
            ai::ai_set_key,
            ai::ai_remove_key,
            ai::ai_set_account,
            ai::ai_test,
            ai::ai_models,
            ai::ai_select,
            ai::ai_set_gemini_consent,
            ai::ai_complete,
            ai::ai_open_keys_page,
            // [agent] — Agent Mode: intent -> plan -> permission -> execute
            agent::agent_warm,
            agent::agent_plan,
            agent::agent_run,
            agent::agent_tasks,
            agent::agent_cancel,
            agent::agent_integrations,
            agent::agent_mcp_list,
            agent::agent_mcp_save,
            agent::agent_mcp_remove,
            // [circle] — Circle to Ask + temporary visible screen awareness (D-021)
            circle::circle_start,
            circle::circle_cancel,
            circle::circle_overlay_ready,
            circle::circle_capture,
            circle::circle_ask_vision,
            circle::circle_clear,
            circle::circle_search,
            circle::circle_open_keys_page,
            circle::circle_ocr_status,
            circle::circle_get_settings,
            circle::circle_set_settings,
            circle::circle_set_key,
            circle::aware_start,
            circle::aware_stop,
            circle::aware_status,
            circle::aware_ask,
            // [extras] — Quick Launcher names, snippets, file organizer, workflows, file-name search
            extras::extras_apps,
            extras::snippets_list,
            extras::snippets_save,
            extras::snippets_remove,
            extras::snippets_use,
            extras::workflows_list,
            extras::workflows_save,
            extras::workflows_remove,
            extras::organizer_folders,
            extras::organizer_rules,
            extras::organizer_save_rule,
            extras::organizer_remove_rule,
            extras::organizer_preview,
            extras::organizer_run,
            extras::organizer_undo_info,
            extras::organizer_undo,
            extras::filesearch_start,
            extras::filesearch_cancel,
            extras::filesearch_open,
            // [clean] — Clean My OS: scan, review, Recycle Bin only
            clean::clean_categories,
            clean::clean_scan,
            clean::clean_cancel,
            clean::clean_delete,
            clean::clean_recycle_bin_size,
            // [dictation] — local Whisper dictation into any app (D-020)
            dictation::dictation_status,
            dictation::dictation_set_settings,
            dictation::dictation_toggle,
            dictation::dictation_cancel,
            dictation::voice_input_devices,
            dictation::mic_test_start,
            dictation::mic_test_level,
            dictation::mic_test_stop,
            dictation::voice_model_download,
            dictation::voice_model_cancel_download,
            dictation::voice_model_delete,
            // [live] — owner: live-data
            commands::load_clipboard,
            commands::add_clip,
            commands::toggle_clip_pin,
            commands::remove_clip,
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
            commands::save_world_clocks,
            commands::save_shelves,
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
            reminders::alarm_sync,
            reminders::reminder_add,
            reminders::reminder_set,
            reminders::reminder_snooze,
            reminders::reminder_dismiss,
            reminders::reminder_test_fire,
            // [/reminders]
            // [explain] — owner: explain
            explain::explain_summarize,
            explain::explain_cancel_summarize,
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
