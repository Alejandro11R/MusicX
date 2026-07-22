use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use tokio::process::Command;
use tokio::time::timeout;

use crate::errors::CadenceError;
use crate::models::search_result::SearchResult;

const YT_DLP_TIMEOUT: Duration = Duration::from_secs(15);

/// The subset of yt-dlp's `-j` JSON output this service cares about. The
/// rest of the app never sees this shape directly, only `SearchResult`.
#[derive(Debug, Deserialize)]
struct YtDlpEntry {
    id: String,
    title: String,
    artist: Option<String>,
    duration: Option<f64>,
    thumbnail: Option<String>,
}

impl From<YtDlpEntry> for SearchResult {
    fn from(entry: YtDlpEntry) -> Self {
        SearchResult {
            id: entry.id,
            title: entry.title,
            artist: entry.artist,
            duration_seconds: entry.duration.unwrap_or(0.0) as u64,
            thumbnail_url: entry.thumbnail.unwrap_or_default(),
        }
    }
}

/// Searches YouTube for `query` and returns up to `limit` results.
pub async fn search(query: &str, limit: u32) -> Result<Vec<SearchResult>, CadenceError> {
    let search_spec = format!("ytsearch{limit}:{query}");
    let output = run_yt_dlp(&["-j", &search_spec]).await?;

    // yt-dlp prints one JSON object per line for multi-result searches.
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<YtDlpEntry>(line)
                .map(SearchResult::from)
                .map_err(CadenceError::YtDlpParse)
        })
        .collect()
}

/// Resolves a video ID to a direct, playable audio-only stream URL.
/// The URL is short-lived (YouTube signs it with an expiry), so callers
/// should not persist it — resolve again when the track is actually played.
pub async fn resolve_audio(video_id: &str) -> Result<String, CadenceError> {
    let watch_url = format!("https://www.youtube.com/watch?v={video_id}");
    let output = run_yt_dlp(&["-f", "bestaudio", "-g", &watch_url]).await?;

    let url = output.lines().next().unwrap_or("").trim();
    if url.is_empty() {
        return Err(CadenceError::YtDlpNoAudioStream {
            video_id: video_id.to_string(),
        });
    }

    Ok(url.to_string())
}

async fn run_yt_dlp(args: &[&str]) -> Result<String, CadenceError> {
    let child = Command::new("yt-dlp")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(CadenceError::YtDlpSpawn)?;

    let output = timeout(YT_DLP_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| CadenceError::YtDlpTimeout)?
        .map_err(CadenceError::YtDlpIo)?;

    if !output.status.success() {
        return Err(CadenceError::YtDlpExecution {
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn search_returns_results_shaped_as_search_result() {
        let results = search("Imagine Dragons Believer", 1)
            .await
            .expect("search should succeed");

        assert_eq!(results.len(), 1);
        let first = &results[0];
        assert!(!first.id.is_empty());
        assert!(!first.title.is_empty());
        assert!(first.duration_seconds > 0);
        assert!(!first.thumbnail_url.is_empty());
    }

    #[tokio::test]
    async fn search_respects_the_requested_limit() {
        let results = search("Linkin Park Numb", 3)
            .await
            .expect("search should succeed");

        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn resolve_audio_returns_a_direct_stream_url() {
        let results = search("Imagine Dragons Believer", 1)
            .await
            .expect("search should succeed");
        let video_id = &results[0].id;

        let url = resolve_audio(video_id)
            .await
            .expect("resolve_audio should succeed");

        assert!(url.starts_with("https://"));
    }

    #[tokio::test]
    async fn resolve_audio_fails_for_a_nonexistent_video_id() {
        let result = resolve_audio("this-id-does-not-exist-000").await;
        assert!(result.is_err());
    }
}
