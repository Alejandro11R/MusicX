#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use tauri::Manager;
use tokio::sync::Mutex;

mod commands;
mod errors;
pub mod models;
pub mod services;

use services::player::PlayerService;

/// Tauri-managed state. `tokio::sync::Mutex` rather than `std::sync::Mutex`
/// because every `PlayerService` method is async and held across `.await`
/// points — a std mutex guard can't cross those without blocking the
/// runtime thread.
struct AppState {
    player: Arc<Mutex<PlayerService>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let socket_path = std::env::temp_dir().join("cadence-mpv.sock");
    // Bootstrap failure has no recovery path (no window exists yet to report
    // the error), so this is the one legitimate exception to the no-expect rule.
    #[allow(clippy::expect_used)]
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let player = tauri::async_runtime::block_on(PlayerService::connect(socket_path))?;
            app.manage(AppState {
                player: Arc::new(Mutex::new(player)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::search::search,
            commands::playback::play,
            commands::playback::pause,
            commands::playback::resume,
            commands::playback::stop,
            commands::playback::set_volume,
            commands::playback::seek,
            commands::player::state,
            commands::player::health,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        // Tauri's normal exit path does not guarantee Drop runs (see
        // MpvPlayer::kill), so mpv must be killed explicitly here or it
        // outlives the window that spawned it.
        if let tauri::RunEvent::ExitRequested { .. } = event {
            let app_handle = app_handle.clone();
            tauri::async_runtime::block_on(async move {
                let state = app_handle.state::<AppState>();
                state.player.lock().await.shutdown().await;
            });
        }
    });
}
