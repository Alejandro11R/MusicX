use tauri::State;

use crate::errors::CadenceError;
use crate::models::search_result::SearchResult;
use crate::AppState;

// Takes the full SearchResult (the frontend already has it from a prior
// search() call) rather than just a video_id, so PlayerService can keep it
// as `current` without a second round-trip to yt-dlp for its metadata.
#[tauri::command]
pub async fn play(app_state: State<'_, AppState>, track: SearchResult) -> Result<(), CadenceError> {
    app_state.player.lock().await.play(track).await
}

#[tauri::command]
pub async fn pause(app_state: State<'_, AppState>) -> Result<(), CadenceError> {
    app_state.player.lock().await.pause().await
}

#[tauri::command]
pub async fn resume(app_state: State<'_, AppState>) -> Result<(), CadenceError> {
    app_state.player.lock().await.resume().await
}

#[tauri::command]
pub async fn stop(app_state: State<'_, AppState>) -> Result<(), CadenceError> {
    app_state.player.lock().await.stop().await
}

#[tauri::command]
pub async fn set_volume(app_state: State<'_, AppState>, level: f64) -> Result<(), CadenceError> {
    app_state.player.lock().await.set_volume(level).await
}
