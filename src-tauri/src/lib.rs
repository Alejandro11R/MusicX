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
        // Must be the first plugin registered (Tauri's own requirement) —
        // it needs to intercept a second launch before anything else runs.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let player = tauri::async_runtime::block_on(PlayerService::connect(socket_path))?;
            app.manage(AppState {
                player: Arc::new(Mutex::new(player)),
            });

            if let Some(icon) = app.default_window_icon().cloned() {
                let quit_i = tauri::menu::MenuItem::with_id(app, "quit", "Salir de Cadence", true, None::<&str>)?;
                let toggle_i = tauri::menu::MenuItem::with_id(app, "toggle", "Mostrar / Ocultar", true, None::<&str>)?;
                let menu = tauri::menu::Menu::with_items(app, &[&toggle_i, &quit_i])?;

                let _tray = tauri::tray::TrayIconBuilder::new()
                    .icon(icon)
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| {
                        match event.id.as_ref() {
                            "quit" => {
                                app.exit(0);
                            }
                            "toggle" => {
                                if let Some(window) = app.get_webview_window("main") {
                                    if window.is_visible().unwrap_or(false) {
                                        let _ = window.hide();
                                    } else {
                                        let _ = window.show();
                                        let _ = window.set_focus();
                                    }
                                }
                            }
                            _ => {}
                        }
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let tauri::tray::TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } = event
                        {
                            let app = tray.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                if window.is_visible().unwrap_or(false) {
                                    let _ = window.hide();
                                } else {
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
                            }
                        }
                    })
                    .build(app)?;
            }

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
            commands::playback::set_loop,
            commands::player::state,
            commands::player::health,
            commands::app::quit,
        ])
        // Closing the window hides it instead of quitting — mpv keeps
        // playing, and a second launch (single-instance, above) just
        // re-shows this same window. commands::app::quit is the only way
        // to actually end the process.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
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
