use serde::Serialize;
use tauri::State;
use tokio::process::Command;

use crate::errors::CadenceError;
use crate::models::player_state::PlayerState;
use crate::AppState;

#[tauri::command]
pub async fn state(app_state: State<'_, AppState>) -> Result<PlayerState, CadenceError> {
    app_state.player.lock().await.state().await
}

#[derive(Debug, Serialize)]
pub struct HealthStatus {
    pub mpv: bool,
    pub yt_dlp: bool,
}

/// Diagnostic-only command: lets the frontend (or a developer in devtools)
/// tell apart "invoke() itself is broken", "mpv is unreachable" and
/// "yt-dlp is missing" with a single call, instead of guessing from a
/// failed search or play.
///
/// Always `Ok` in practice — Tauri requires async commands that borrow
/// state to return a `Result`, and mpv/yt-dlp failures are reported as
/// `false` fields rather than an `Err`, since a health check that can
/// itself fail defeats the point.
#[tauri::command]
pub async fn health(app_state: State<'_, AppState>) -> Result<HealthStatus, CadenceError> {
    let mpv = app_state.player.lock().await.state().await.is_ok();
    let yt_dlp = Command::new("yt-dlp")
        .arg("--version")
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false);

    Ok(HealthStatus { mpv, yt_dlp })
}
