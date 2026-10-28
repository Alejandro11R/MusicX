use std::sync::Arc;

use tauri::State;

use crate::errors::CadenceError;
use crate::models::lyrics::{
    LyricsCacheStats, LyricsCandidate, LyricsDoc, LyricsRequest,
};
use crate::services::lyrics::LyricsService;

#[tauri::command]
pub async fn get_lyrics(
    lyrics: State<'_, Arc<LyricsService>>,
    request: LyricsRequest,
) -> Result<LyricsDoc, CadenceError> {
    let doc = lyrics.load(request).await?;
    Ok((*doc).clone())
}

#[tauri::command]
pub async fn search_lyrics(
    lyrics: State<'_, Arc<LyricsService>>,
    request: LyricsRequest,
) -> Result<Vec<LyricsCandidate>, CadenceError> {
    lyrics.search(&request).await
}

#[tauri::command]
pub async fn get_lyrics_by_id(
    lyrics: State<'_, Arc<LyricsService>>,
    id: i64,
    track_id: String,
) -> Result<LyricsDoc, CadenceError> {
    let doc = lyrics.load_by_id(id, &track_id).await?;
    Ok((*doc).clone())
}

#[tauri::command]
pub async fn lyrics_cache_stats(
    lyrics: State<'_, Arc<LyricsService>>,
) -> Result<LyricsCacheStats, CadenceError> {
    Ok(lyrics.stats().await)
}

#[tauri::command]
pub async fn clear_lyrics_cache(
    lyrics: State<'_, Arc<LyricsService>>,
) -> Result<u64, CadenceError> {
    Ok(lyrics.clear_cache().await)
}

#[tauri::command]
pub async fn set_lyrics_enabled(
    lyrics: State<'_, Arc<LyricsService>>,
    enabled: bool,
) -> Result<(), CadenceError> {
    lyrics.set_enabled(enabled);
    Ok(())
}
