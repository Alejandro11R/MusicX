/// Explicitly ends the app. Closing the window only hides it (see
/// `run()`'s `on_window_event`), so this is the one path that actually
/// exits — bound to a keyboard shortcut in the frontend, not a window
/// control, since there's usually no window open to put one on.
#[tauri::command]
pub fn quit(app_handle: tauri::AppHandle) {
    app_handle.exit(0);
}
