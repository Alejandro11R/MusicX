use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::process::Command;
use tokio::time::timeout;

use crate::errors::CadenceError;
use crate::models::search_result::SearchResult;

const YT_DLP_TIMEOUT: Duration = Duration::from_secs(15);

/// The subset of yt-dlp's `--flat-playlist -j` output this service cares
/// about. The rest of the app never sees this shape directly, only
/// `SearchResult`. Flat mode skips per-video format/subtitle/storyboard
/// extraction, which is the bulk of what a plain `-j` search pays for
/// (measured: ~244KB/8.8s vs ~6KB/3.5s for the same 3-result query) — none
/// of that extra data is needed until resolve_audio() is called for one
/// specific track. The tradeoff: flat mode never includes `thumbnail`, so
/// SearchResult::from builds it from YouTube's predictable per-ID CDN URL
/// instead.
#[derive(Debug, Deserialize)]
struct YtDlpEntry {
    id: String,
    title: String,
    artist: Option<String>,
    duration: Option<f64>,
    ie_key: String,
}

impl From<YtDlpEntry> for SearchResult {
    fn from(entry: YtDlpEntry) -> Self {
        let thumbnail_url = format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", entry.id);
        SearchResult {
            id: entry.id,
            title: entry.title,
            artist: entry.artist,
            duration_seconds: entry.duration.unwrap_or(0.0) as u64,
            thumbnail_url,
        }
    }
}

/// Searches YouTube for `query` and returns up to `limit` results.
pub async fn search(query: &str, limit: u32) -> Result<Vec<SearchResult>, CadenceError> {
    let search_spec = format!("ytsearch{limit}:{query}");
    let output = run_yt_dlp(&["--flat-playlist", "-j", &search_spec]).await?;

    // yt-dlp prints one JSON object per line for multi-result searches.
    let entries = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<YtDlpEntry>(line).map_err(CadenceError::YtDlpParse))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(filter_playable(entries))
}

// A search can match a channel/playlist as well as videos (ie_key
// "YoutubeTab" instead of "Youtube") — not a track resolve_audio() can
// ever play, so it's dropped here rather than shown as a dead result.
// This can leave fewer than `limit` results; that's preferable to a
// result the user can click but nothing will play.
fn filter_playable(entries: Vec<YtDlpEntry>) -> Vec<SearchResult> {
    entries
        .into_iter()
        .filter(|entry| entry.ie_key == "Youtube")
        .map(SearchResult::from)
        .collect()
}

/// Resolves a video ID to a direct, playable audio-only stream URL.
/// The URL is short-lived (YouTube signs it with an expiry), so callers
/// should not persist it — resolve again when the track is actually played.
pub async fn resolve_audio(video_id: &str) -> Result<String, CadenceError> {
    let watch_url = format!("https://www.youtube.com/watch?v={video_id}");
    let output = run_yt_dlp(&["-f", "bestaudio", "-g", &watch_url])
        .await
        .map_err(|err| annotate_execution_failure(err, video_id))?;

    let url = output.lines().next().unwrap_or("").trim();
    if url.is_empty() {
        return Err(CadenceError::YtDlpNoAudioStream {
            video_id: video_id.to_string(),
        });
    }

    Ok(url.to_string())
}

/// yt-dlp's raw stderr isn't fit to show a user, but it's exactly what's
/// needed to reproduce a specific failing video from the terminal — log it
/// here rather than on the error that reaches the frontend, and name the
/// one failure mode ("Sign in to confirm...", age/region restrictions)
/// common enough to give its own error variant and message.
fn annotate_execution_failure(err: CadenceError, video_id: &str) -> CadenceError {
    let CadenceError::YtDlpExecution { message } = &err else {
        return err;
    };

    eprintln!("yt-dlp failed to resolve video {video_id}: {message}");

    if message.to_lowercase().contains("sign in") {
        CadenceError::YtDlpAuthRequired {
            video_id: video_id.to_string(),
        }
    } else {
        err
    }
}

async fn run_yt_dlp(args: &[&str]) -> Result<String, CadenceError> {
    let start = Instant::now();
    eprintln!("[yt-dlp] launching: yt-dlp {}", args.join(" "));

    // Without kill_on_drop, a timeout below drops `child.wait_with_output()`
    // (which owns the Child) without killing the process: yt-dlp keeps
    // running in the background and, having no reaper left once nothing
    // awaits it, becomes a zombie the moment it does exit on its own.
    let child = Command::new("yt-dlp")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(CadenceError::YtDlpSpawn)?;
    eprintln!(
        "[yt-dlp] spawned pid={:?} ({:?} since launch)",
        child.id(),
        start.elapsed()
    );

    eprintln!("[yt-dlp] waiting for output...");
    let output = timeout(YT_DLP_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| {
            eprintln!("[yt-dlp] TIMED OUT after {:?}", start.elapsed());
            CadenceError::YtDlpTimeout
        })?
        .map_err(CadenceError::YtDlpIo)?;
    eprintln!(
        "[yt-dlp] finished in {:?}, status={}",
        start.elapsed(),
        output.status
    );

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
        assert!(first.thumbnail_url.contains(&first.id));
    }

    // No network: flat-playlist mode never returns a thumbnail field, so
    // this checks the construction logic directly rather than trusting
    // yt-dlp to keep giving us a value it's known to omit.
    #[test]
    fn thumbnail_url_is_built_from_the_video_id() {
        let entry = YtDlpEntry {
            id: "abc123".to_string(),
            title: "Some Track".to_string(),
            artist: None,
            duration: Some(120.0),
            ie_key: "Youtube".to_string(),
        };

        let result = SearchResult::from(entry);

        assert_eq!(result.thumbnail_url, "https://i.ytimg.com/vi/abc123/hqdefault.jpg");
    }

    // No network: whether a given query surfaces a channel result isn't
    // guaranteed to stay reproducible, so this checks the filter directly.
    #[test]
    fn filter_playable_drops_non_video_entries() {
        let entries = vec![
            YtDlpEntry {
                id: "abc123".to_string(),
                title: "A Real Song".to_string(),
                artist: None,
                duration: Some(200.0),
                ie_key: "Youtube".to_string(),
            },
            YtDlpEntry {
                id: "UCRRmSKkhOKEO6vIBaxG-ejA".to_string(),
                title: "Some Artist".to_string(),
                artist: None,
                duration: None,
                ie_key: "YoutubeTab".to_string(),
            },
        ];

        let results = filter_playable(entries);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "abc123");
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

    // Doesn't hit the network: real "sign in" restrictions aren't
    // guaranteed to stay reproducible on a specific video, so this checks
    // the classification logic directly against a synthetic yt-dlp stderr.
    #[test]
    fn recognizes_sign_in_required_as_a_distinct_error() {
        let raw = CadenceError::YtDlpExecution {
            message: "ERROR: [youtube] abc123: Sign in to confirm your age.".to_string(),
        };

        let classified = annotate_execution_failure(raw, "abc123");

        assert!(matches!(
            classified,
            CadenceError::YtDlpAuthRequired { video_id } if video_id == "abc123"
        ));
    }

    #[test]
    fn leaves_other_execution_failures_unclassified() {
        let raw = CadenceError::YtDlpExecution {
            message: "ERROR: [youtube] abc123: Video unavailable.".to_string(),
        };

        let classified = annotate_execution_failure(raw, "abc123");

        assert!(matches!(classified, CadenceError::YtDlpExecution { .. }));
    }
}
