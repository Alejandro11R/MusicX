#![deny(clippy::unwrap_used, clippy::expect_used)]

mod commands;
mod errors;
mod models;
mod services;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Bootstrap failure has no recovery path (no window exists yet to report
    // the error), so this is the one legitimate exception to the no-expect rule.
    #[allow(clippy::expect_used)]
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
