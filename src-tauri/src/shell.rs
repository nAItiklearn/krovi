//! Island shell (owner: panel-shell): pill/panel window model, tray, global
//! shortcut, single-instance. See docs/CONTRACTS.md §1.
//! STUB: minimal behaviour so the app compiles; replace with the real implementation.

use tauri::{Manager, Runtime, Window, WindowEvent, Wry};

pub fn plugins(builder: tauri::Builder<Wry>) -> tauri::Builder<Wry> {
    builder.plugin(tauri_plugin_global_shortcut::Builder::new().build())
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
    let summon = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
    let handle = app.handle().clone();
    app.global_shortcut()
        .on_shortcut(summon, move |_app, _s, ev| {
            if ev.state() == ShortcutState::Pressed {
                if let Some(w) = handle.get_webview_window("main") {
                    if w.is_visible().unwrap_or(false) {
                        let _ = w.hide();
                    } else {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
            }
        })?;
    Ok(())
}

pub fn on_window_event<R: Runtime>(_window: &Window<R>, _event: &WindowEvent) {}

#[tauri::command]
pub fn shell_set_mode(_mode: String) {}

#[tauri::command]
pub fn shell_quit(app: tauri::AppHandle) {
    app.exit(0);
}
