// Demo host for tauri-plugin-multiline-taskband.
//
// The plugin is registered once; the frontend (src/main.js) drives every
// instance through the JS API (plugin:multiline-taskband|* commands).
//
// Closing any window (main or the "popup" settings window) hides it instead
// of destroying it: the plugin reuses the popup window via get_webview_window
// + show, so destroying it would break the next click.

/// Quit the whole app. Invoked from the right-click context menu's
/// "退出 App" item (the menu itself is built by the plugin; the host decides
/// what each action does).
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::Manager;
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_multiline_taskband::init())
        .invoke_handler(tauri::generate_handler![quit_app])
        .on_menu_event(|app, event| {
            // Right-click context-menu actions, handled on the Rust side so
            // they work even when the main window is hidden (a hidden
            // window's JS may not run reliably, so the menu must not depend
            // on it re-showing itself).
            if let Some((_inst, action)) = event.id().0.split_once("::") {
                match action {
                    "open-settings" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                }
            }
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {});
}
