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

    let socket_path = if cfg!(windows) {
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "default".to_string());
        std::path::PathBuf::from(format!(r"\\.\pipe\cadence-mpv-{}", user))
    } else {
        std::env::temp_dir().join("cadence-mpv.sock")
    };

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
            // Inicializar servicio de letras (LyricsService) con caché y cliente LRCLIB
            let lyrics = Arc::new(services::lyrics::LyricsService::new());
            app.manage(lyrics.clone());
            tracing::info!("LyricsService initialized");

            // Bloqueante a propósito: si el frontend llama un comando antes de
            // que el estado esté gestionado, Tauri falla con "state not
            // managed". Conectar mpv tarda ~100-300 ms.
            let orchestrator = tauri::async_runtime::block_on(
                PlayerOrchestratorBuilder::new(socket_path.clone())
                    .with_lyrics(lyrics.clone())
                    .build(),
            )?;
            app.manage(orchestrator.clone());
            tracing::info!("PlayerOrchestrator initialized");

            let focus = services::focus::FocusEngine::new(orchestrator.clone());
            app.manage(focus.clone());
            tracing::info!("FocusEngine initialized");

            services::control_server::ControlServer::spawn(orchestrator.clone(), focus.clone());

            let mut focus_rx = focus.subscribe_state();
            let app_handle_focus = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while focus_rx.changed().await.is_ok() {
                    let state = focus_rx.borrow().clone();
                    use tauri::Emitter;
                    let _ = app_handle_focus.emit("focus-state-changed", state);
                }
            });

            // Bridge para eventos de letras en tiempo real hacia el frontend
            let mut lyrics_rx = lyrics.subscribe();
            let app_handle_lyrics = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = lyrics_rx.recv().await {
                    use tauri::Emitter;
                    let _ = app_handle_lyrics.emit("lyrics-event", event);
                }
            });

            // Señales de apagado limpio según el sistema operativo
            #[cfg(unix)]
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
            #[cfg(windows)]
            {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if tokio::signal::ctrl_c().await.is_ok() {
                        tracing::info!("Ctrl-C received, exiting cleanly");
                        app_handle.exit(0);
                    }
                });
            }

            if let Some(icon) = app.default_window_icon().cloned() {
                let quit_i = tauri::menu::MenuItem::with_id(app, "quit", "Salir de Cadence", true, None::<&str>)?;
                let toggle_i = tauri::menu::MenuItem::with_id(app, "toggle", "Mostrar / Ocultar", true, None::<&str>)?;
                let next_i = tauri::menu::MenuItem::with_id(app, "next", "Siguiente tema (Next)", true, None::<&str>)?;
                let play_pause_i = tauri::menu::MenuItem::with_id(app, "play_pause", "Play / Pausa", true, None::<&str>)?;
                let menu = tauri::menu::Menu::with_items(app, &[&toggle_i, &play_pause_i, &next_i, &quit_i])?;

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
                            "play_pause" => {
                                if let Some(orchestrator) = app.try_state::<Arc<PlayerOrchestrator>>() {
                                    let orch = orchestrator.inner().clone();
                                    tauri::async_runtime::spawn(async move {
                                        let state = orch.get_state().await;
                                        if state.status == crate::models::player_state::PlaybackStatus::Playing {
                                            let _ = orch.pause().await;
                                        } else {
                                            let _ = orch.resume().await;
                                        }
                                    });
                                }
                            }
                            "next" => {
                                if let Some(orchestrator) = app.try_state::<Arc<PlayerOrchestrator>>() {
                                    let orch = orchestrator.inner().clone();
                                    tauri::async_runtime::spawn(async move {
                                        let _ = orch.next().await;
                                    });
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
            commands::playback::next,
            commands::playback::previous,
            commands::playback::get_queue,
            commands::playback::add_to_queue,
            commands::playback::add_to_queue_next,
            commands::playback::remove_from_queue,
            commands::playback::clear_queue,
            commands::playback::play_queue_index,
            commands::playback::set_audio_preset,
            commands::playback::set_speed,
            commands::playback::start_focus,
            commands::playback::pause_focus,
            commands::playback::reset_focus,
            commands::playback::skip_focus,
            commands::playback::get_focus_state,
            commands::playback::set_focus_auto_pause,
            commands::playback::set_focus_durations,
            commands::player::state,
            commands::player::health,
            commands::player::subscribe_events,
            commands::lyrics::get_lyrics,
            commands::lyrics::search_lyrics,
            commands::lyrics::get_lyrics_by_id,
            commands::lyrics::lyrics_cache_stats,
            commands::lyrics::clear_lyrics_cache,
            commands::lyrics::set_lyrics_enabled,
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