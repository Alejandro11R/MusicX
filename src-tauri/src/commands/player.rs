use std::sync::Arc;

use crate::errors::CadenceError;
use crate::models::player_state::PlayerState;
use crate::services::orchestrator::PlayerOrchestrator;
use tauri::{Emitter, State};

#[tauri::command]
pub async fn state(
    orchestrator: State<'_, Arc<PlayerOrchestrator>>,
) -> Result<PlayerState, CadenceError> {
    Ok(orchestrator.get_state().await)
}

#[derive(Debug, serde::Serialize)]
pub struct HealthStatus {
    pub mpv: bool,
    pub yt_dlp: bool,
}

/// Diagnostic-only command: lets the frontend tell apart different failure modes.
#[tauri::command]
pub async fn health(
    orchestrator: State<'_, Arc<PlayerOrchestrator>>,
) -> Result<HealthStatus, CadenceError> {
    // mpv health: the supervisor reports whether the IPC socket is alive.
    let mpv_ok = orchestrator.is_connected();

    // yt-dlp health: check if binary exists and runs.
    let yt_dlp_ok = tokio::process::Command::new("yt-dlp")
        .arg("--version")
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false);

    Ok(HealthStatus {
        mpv: mpv_ok,
        yt_dlp: yt_dlp_ok,
    })
}

/// Suscribe a eventos del orquestador (TrackStarted, TrackEnded, StateChanged,
/// ConnectionChanged, PlaybackError). El frontend usa `listen` de Tauri.
///
/// Es idempotente: si ya hay una suscripción activa (p. ej. el componente se
/// re-montó), no se registra una segunda tarea emisora.
#[tauri::command]
pub async fn subscribe_events(
    orchestrator: State<'_, Arc<PlayerOrchestrator>>,
    app: tauri::AppHandle,
) -> Result<(), CadenceError> {
    let Some(mut events) = orchestrator.take_events_subscription() else {
        return Ok(());
    };
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let _ = app_handle.emit("orchestrator-event", &event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "orchestrator event listener lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    Ok(())
}
