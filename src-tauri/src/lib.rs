#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use tauri::Manager;

mod commands;
pub mod errors;
pub mod models;
pub mod services;

use services::orchestrator::{PlayerOrchestrator, PlayerOrchestratorBuilder};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Inicializar tracing
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let socket_path = std::env::temp_dir().join("cadence-mpv.sock");

    #[allow(clippy::expect_used)]
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            // Bloqueante a propósito: si el frontend llama un comando antes de
            // que el estado esté gestionado, Tauri falla con "state not
            // managed". Conectar mpv tarda ~100-300 ms.
            let orchestrator = tauri::async_runtime::block_on(
                PlayerOrchestratorBuilder::new(socket_path.clone()).build(),
            )?;
            app.manage(orchestrator);
            tracing::info!("PlayerOrchestrator initialized");

            // SIGTERM no dispara `ExitRequested` por sí solo: sin esto, el
            // proceso muere y deja a mpv huérfano (lo limpia el pidfile en el
            // próximo arranque, pero mejor apagarlo bien).
            {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    use tokio::signal::unix::{signal, SignalKind};
                    let Ok(mut term) = signal(SignalKind::terminate()) else {
                        return;
                    };
                    term.recv().await;
                    tracing::info!("SIGTERM received, exiting cleanly");
                    app_handle.exit(0);
                });
            }

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
            commands::player::subscribe_events,
            commands::app::quit,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            // Bloqueante: si solo spawneamos, el runtime puede apagarse antes
            // de que mpv reciba el shutdown y quedaría un proceso huérfano.
            if let Some(orchestrator) = app_handle.try_state::<Arc<PlayerOrchestrator>>() {
                tauri::async_runtime::block_on(orchestrator.shutdown());
            }
        }
    });
}