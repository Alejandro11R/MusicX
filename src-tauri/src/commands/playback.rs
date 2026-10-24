use std::sync::Arc;

use crate::errors::CadenceError;
use crate::models::search_result::SearchResult;
use crate::services::orchestrator::PlayerOrchestrator;

#[tauri::command]
pub async fn play(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
    track: SearchResult,
) -> Result<(), CadenceError> {
    tracing::info!(track_id = %track.id, title = %track.title, "Playing track");
    orchestrator.play(track).await
}

#[tauri::command]
pub async fn pause(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
) -> Result<(), CadenceError> {
    orchestrator.pause().await
}

#[tauri::command]
pub async fn resume(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
) -> Result<(), CadenceError> {
    orchestrator.resume().await
}

#[tauri::command]
pub async fn stop(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
) -> Result<(), CadenceError> {
    orchestrator.stop().await
}

#[tauri::command]
pub async fn set_volume(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
    level: f64,
) -> Result<(), CadenceError> {
    orchestrator.set_volume(level).await
}

#[tauri::command]
pub async fn seek(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
    position_seconds: f64,
) -> Result<(), CadenceError> {
    orchestrator.seek(position_seconds).await
}

#[tauri::command]
pub async fn set_loop(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
    enabled: bool,
) -> Result<(), CadenceError> {
    orchestrator.set_loop(enabled).await
}
