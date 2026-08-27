// Demo host for tauri-plugin-multiline-taskband.
//
// The plugin is registered once; the frontend (src/main.js) drives every
// instance through the JS API (plugin:multiline-taskband|* commands).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_multiline_taskband::init())
        .invoke_handler(tauri::generate_handler![])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {});
}
