use std::sync::Arc;

use crate::errors::CadenceError;
use crate::models::player_state::AudioPreset;
use crate::models::search_result::SearchResult;
use crate::services::focus::{FocusEngine, FocusState};
use crate::services::orchestrator::{PlayerOrchestrator, QueueItem};

#[tauri::command]
pub async fn play(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
    track: SearchResult,
) -> Result<(), CadenceError> {
    tracing::info!(track_id = %track.id, title = %track.title, "Playing track");
    orchestrator.play(track).await
}

#[tauri::command]
pub async fn next(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
) -> Result<(), CadenceError> {
    orchestrator.next().await
}

#[tauri::command]
pub async fn previous(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
) -> Result<(), CadenceError> {
    orchestrator.previous().await
}

#[tauri::command]
pub async fn get_queue(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
) -> Result<Vec<QueueItem>, CadenceError> {
    Ok(orchestrator.get_queue().await)
}

#[tauri::command]
pub async fn add_to_queue(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
    track: SearchResult,
) -> Result<QueueItem, CadenceError> {
    Ok(orchestrator.add_to_queue(track).await)
}

#[tauri::command]
pub async fn add_to_queue_next(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
    track: SearchResult,
) -> Result<QueueItem, CadenceError> {
    Ok(orchestrator.add_to_queue_next(track).await)
}

#[tauri::command]
pub async fn remove_from_queue(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
    item_id: String,
) -> Result<bool, CadenceError> {
    Ok(orchestrator.remove_from_queue(&item_id).await)
}

#[tauri::command]
pub async fn clear_queue(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
) -> Result<(), CadenceError> {
    orchestrator.clear_queue().await;
    Ok(())
}

#[tauri::command]
pub async fn play_queue_index(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
    index: usize,
) -> Result<(), CadenceError> {
    orchestrator.play_queue_index(index).await
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

#[tauri::command]
pub async fn set_audio_preset(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
    preset: AudioPreset,
) -> Result<(), CadenceError> {
    orchestrator.set_audio_preset(preset).await
}

#[tauri::command]
pub async fn set_speed(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
    speed: f64,
) -> Result<(), CadenceError> {
    orchestrator.set_speed(speed).await
}

#[tauri::command]
pub async fn start_focus(
    focus: tauri::State<'_, Arc<FocusEngine>>,
) -> Result<(), CadenceError> {
    focus.start().await;
    Ok(())
}

#[tauri::command]
pub async fn pause_focus(
    focus: tauri::State<'_, Arc<FocusEngine>>,
) -> Result<(), CadenceError> {
    focus.pause().await;
    Ok(())
}

#[tauri::command]
pub async fn reset_focus(
    focus: tauri::State<'_, Arc<FocusEngine>>,
) -> Result<(), CadenceError> {
    focus.reset().await;
    Ok(())
}

#[tauri::command]
pub async fn skip_focus(
    focus: tauri::State<'_, Arc<FocusEngine>>,
) -> Result<(), CadenceError> {
    focus.skip().await;
    Ok(())
}

#[tauri::command]
pub async fn get_focus_state(
    focus: tauri::State<'_, Arc<FocusEngine>>,
) -> Result<FocusState, CadenceError> {
    Ok(focus.get_state().await)
}

#[tauri::command]
pub async fn set_focus_auto_pause(
    focus: tauri::State<'_, Arc<FocusEngine>>,
    enabled: bool,
) -> Result<(), CadenceError> {
    focus.set_auto_pause(enabled).await;
    Ok(())
}
