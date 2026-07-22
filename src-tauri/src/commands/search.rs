use crate::errors::CadenceError;
use crate::models::search_result::SearchResult;
use crate::services::yt_dlp;

#[tauri::command]
pub async fn search(query: String, limit: u32) -> Result<Vec<SearchResult>, CadenceError> {
    yt_dlp::search(&query, limit).await
}
