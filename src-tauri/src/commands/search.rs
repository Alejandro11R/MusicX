use std::sync::Arc;

use crate::errors::CadenceError;
use crate::models::search_result::SearchResult;
use crate::services::orchestrator::PlayerOrchestrator;

#[tauri::command]
pub async fn search(
    orchestrator: tauri::State<'_, Arc<PlayerOrchestrator>>,
    query: String,
    limit: u32,
) -> Result<Vec<SearchResult>, CadenceError> {
    orchestrator.search(&query, limit).await
}
