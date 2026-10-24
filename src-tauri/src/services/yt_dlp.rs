use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde::Deserialize;
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::time::timeout;

use crate::errors::CadenceError;
use crate::models::search_result::SearchResult;

const YT_DLP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CONCURRENT_YT_DLP: usize = 2;
const CACHE_TTL: Duration = Duration::from_secs(30 * 60); // 30 minutos
const CIRCUIT_BREAKER_THRESHOLD: u32 = 5;
const CIRCUIT_BREAKER_RESET: Duration = Duration::from_secs(60);

/// Entrada de yt-dlp en modo flat-playlist.
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

/// Margen de seguridad: dejamos de usar una URL firmada un minuto antes de
/// que YouTube la considere expirada, para evitar 403 a mitad de canción.
const URL_EXPIRY_SAFETY_MARGIN: Duration = Duration::from_secs(60);

/// Calcula el TTL de una URL a partir de su parámetro `expire` (epoch secs).
///
/// Las URLs de googlevideo vienen firmadas y caducan (normalmente ~6h). Un TTL
/// fijo de 30 min puede devolver una URL ya muerta si la app queda abierta
/// mucho tiempo; y al revés, cachear más allá del `expire` garantiza un 403.
/// Si la URL no trae `expire`, caemos al TTL conservador de siempre.
fn ttl_from_url(url: &str, now_unix_secs: u64) -> Duration {
    let expire = url
        .split_once('?')
        .map(|(_, query)| query)
        .into_iter()
        .flat_map(|query| query.split('&'))
        .find_map(|pair| pair.strip_prefix("expire="))
        .and_then(|value| value.parse::<u64>().ok());

    match expire {
        Some(expire) => {
            let deadline = expire.saturating_sub(URL_EXPIRY_SAFETY_MARGIN.as_secs());
            if deadline <= now_unix_secs {
                Duration::ZERO
            } else {
                Duration::from_secs(deadline - now_unix_secs).min(CACHE_TTL)
            }
        }
        None => CACHE_TTL,
    }
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Entrada de cache para URLs de audio resueltas.
#[derive(Debug, Clone)]
struct CacheEntry {
    url: String,
    expires_at: Instant,
}

impl CacheEntry {
    fn new(url: String) -> Self {
        let expires_at = Instant::now() + ttl_from_url(&url, now_unix_secs());
        Self { url, expires_at }
    }

    fn is_valid(&self) -> bool {
        Instant::now() < self.expires_at
    }
}

/// Circuit breaker simple para yt-dlp.
#[derive(Debug)]
struct CircuitBreaker {
    failures: u32,
    last_failure: Option<Instant>,
    state: CircuitState,
}

#[derive(Debug, PartialEq, Eq)]
enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl CircuitBreaker {
    fn new() -> Self {
        Self {
            failures: 0,
            last_failure: None,
            state: CircuitState::Closed,
        }
    }

    fn record_success(&mut self) {
        self.failures = 0;
        self.state = CircuitState::Closed;
    }

    fn record_failure(&mut self) {
        self.failures += 1;
        self.last_failure = Some(Instant::now());
        if self.failures >= CIRCUIT_BREAKER_THRESHOLD {
            self.state = CircuitState::Open;
        }
    }

    fn can_execute(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                if let Some(last) = self.last_failure {
                    if last.elapsed() > CIRCUIT_BREAKER_RESET {
                        self.state = CircuitState::HalfOpen;
                        return true;
                    }
                }
                false
            }
            CircuitState::HalfOpen => true,
        }
    }
}

/// Servicio yt-dlp con cache, semáforo y circuit breaker.
pub struct YtDlpService {
    cache: Arc<DashMap<String, CacheEntry>>,
    search_cache: Arc<DashMap<String, Vec<SearchResult>>>,
    semaphore: Arc<Semaphore>,
    circuit_breaker: Arc<tokio::sync::Mutex<CircuitBreaker>>,
}

impl YtDlpService {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            search_cache: Arc::new(DashMap::new()),
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_YT_DLP)),
            circuit_breaker: Arc::new(tokio::sync::Mutex::new(CircuitBreaker::new())),
        }
    }

    /// Busca en YouTube y retorna resultados.
    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<SearchResult>, CadenceError> {
        let cache_key = format!("search:{}:{}", query, limit);
        
        // Check cache first
        if let Some(cached) = self.search_cache.get(&cache_key) {
            if !cached.is_empty() {
                tracing::debug!(query = %query, limit, "Search cache hit");
                return Ok(cached.clone());
            }
        }

        let _permit = self.semaphore.acquire().await.map_err(|_| CadenceError::YtDlpExecution {
            message: "Semaphore closed".to_string(),
        })?;

        let mut cb = self.circuit_breaker.lock().await;
        if !cb.can_execute() {
            return Err(CadenceError::YtDlpExecution {
                message: "Circuit breaker open - too many failures".to_string(),
            });
        }
        drop(cb);

        let search_spec = format!("ytsearch{limit}:{query}");
        let output = self.run_yt_dlp_search(&["--flat-playlist", "-j", &search_spec]).await;

        let mut cb = self.circuit_breaker.lock().await;
        match &output {
            Ok(results) => {
                cb.record_success();
                // Cache successful results
                self.search_cache.insert(cache_key, results.clone());
                Ok(results.clone())
            }
            Err(_) => {
                cb.record_failure();
                // Return the error (move it out)
                output
            }
        }
    }

    /// Resuelve un video ID a URL de audio directa.
    pub async fn resolve_audio(&self, video_id: &str) -> Result<String, CadenceError> {
        // Check cache first
        if let Some(entry) = self.cache.get(video_id) {
            if entry.is_valid() {
                tracing::debug!(video_id = %video_id, "Audio URL cache hit");
                return Ok(entry.url.clone());
            } else {
                // Expired, remove
                self.cache.remove(video_id);
            }
        }

        let _permit = self.semaphore.acquire().await.map_err(|_| CadenceError::YtDlpExecution {
            message: "Semaphore closed".to_string(),
        })?;

        let mut cb = self.circuit_breaker.lock().await;
        if !cb.can_execute() {
            return Err(CadenceError::YtDlpExecution {
                message: "Circuit breaker open - too many failures".to_string(),
            });
        }
        drop(cb);

        let watch_url = format!("https://www.youtube.com/watch?v={video_id}");
        let output = self.run_yt_dlp_resolve(&["-f", "bestaudio", "-g", &watch_url]).await;

        let mut cb = self.circuit_breaker.lock().await;
        match &output {
            Ok(url) => {
                cb.record_success();
                self.cache.insert(video_id.to_string(), CacheEntry::new(url.clone()));
                Ok(url.clone())
            }
            Err(_) => {
                cb.record_failure();
                // Return the error (move it out)
                output
            }
        }
    }

    /// Invalida cache para un video específico.
    pub fn invalidate(&self, video_id: &str) {
        self.cache.remove(video_id);
    }

    /// Limpia cache expirado.
    pub fn cleanup_expired(&self) {
        self.cache.retain(|_, v| v.is_valid());
        self.search_cache.retain(|_, v| !v.is_empty());
    }

    /// Ejecuta yt-dlp para búsqueda y parsea resultados.
    async fn run_yt_dlp_search(&self, args: &[&str]) -> Result<Vec<SearchResult>, CadenceError> {
        let start = Instant::now();
        tracing::debug!(args = ?args, "[yt-dlp] launching search");

        let child = Command::new("yt-dlp")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(CadenceError::YtDlpSpawn)?;

        tracing::debug!(pid = ?child.id(), elapsed = ?start.elapsed(), "[yt-dlp] spawned");

        let output = timeout(YT_DLP_TIMEOUT, child.wait_with_output())
            .await
            .map_err(|_| {
                tracing::warn!(elapsed = ?start.elapsed(), "[yt-dlp] TIMED OUT");
                CadenceError::YtDlpTimeout
            })?
            .map_err(CadenceError::YtDlpIo)?;

        tracing::debug!(elapsed = ?start.elapsed(), status = %output.status, "[yt-dlp] finished");

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            tracing::warn!(stderr = %stderr, "[yt-dlp] execution failed");
            return Err(self.annotate_execution_failure(stderr, args));
        }

        // Parsear líneas JSON (flat-playlist -j)
        let stdout = String::from_utf8_lossy(&output.stdout);
        let entries = stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str::<YtDlpEntry>(line).map_err(CadenceError::YtDlpParse))
            .collect::<Result<Vec<_>, _>>()?;

        let results = entries
            .into_iter()
            .filter(|entry| entry.ie_key == "Youtube")
            .map(SearchResult::from)
            .collect();

        Ok(results)
    }

    /// Ejecuta yt-dlp para resolver audio URL.
    async fn run_yt_dlp_resolve(&self, args: &[&str]) -> Result<String, CadenceError> {
        let start = Instant::now();
        tracing::debug!(args = ?args, "[yt-dlp] launching resolve");

        let child = Command::new("yt-dlp")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(CadenceError::YtDlpSpawn)?;

        tracing::debug!(pid = ?child.id(), elapsed = ?start.elapsed(), "[yt-dlp] spawned");

        let output = timeout(YT_DLP_TIMEOUT, child.wait_with_output())
            .await
            .map_err(|_| {
                tracing::warn!(elapsed = ?start.elapsed(), "[yt-dlp] TIMED OUT");
                CadenceError::YtDlpTimeout
            })?
            .map_err(CadenceError::YtDlpIo)?;

        tracing::debug!(elapsed = ?start.elapsed(), status = %output.status, "[yt-dlp] finished");

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            tracing::warn!(stderr = %stderr, "[yt-dlp] execution failed");
            return Err(self.annotate_execution_failure(stderr, args));
        }

        let url = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();

        if url.is_empty() {
            return Err(CadenceError::YtDlpNoAudioStream {
                video_id: args.iter()
                    .find(|a| a.starts_with("https://www.youtube.com/watch?v="))
                    .and_then(|a| a.split("v=").nth(1))
                    .and_then(|a| a.split('&').next())
                    .unwrap_or("unknown")
                    .to_string(),
            });
        }

        Ok(url)
    }

    fn annotate_execution_failure(&self, stderr: String, args: &[&str]) -> CadenceError {
        if stderr.to_lowercase().contains("sign in") {
            let video_id = args.iter()
                .find(|a| a.starts_with("https://www.youtube.com/watch?v="))
                .and_then(|a| a.split("v=").nth(1))
                .and_then(|a| a.split('&').next())
                .unwrap_or("unknown")
                .to_string();
            CadenceError::YtDlpAuthRequired { video_id }
        } else if stderr.to_lowercase().contains("no audio") || stderr.to_lowercase().contains("no formats") {
            let video_id = args.iter()
                .find(|a| a.starts_with("https://www.youtube.com/watch?v="))
                .and_then(|a| a.split("v=").nth(1))
                .and_then(|a| a.split('&').next())
                .unwrap_or("unknown")
                .to_string();
            CadenceError::YtDlpNoAudioStream { video_id }
        } else {
            CadenceError::YtDlpExecution { message: stderr }
        }
    }
}

impl Default for YtDlpService {
    fn default() -> Self {
        Self::new()
    }
}

/// Instancia global singleton para el servicio yt-dlp.
static YT_DLP_SERVICE: Lazy<YtDlpService> = Lazy::new(YtDlpService::new);

/// Busca en YouTube (API pública compatible con código existente).
pub async fn search(query: &str, limit: u32) -> Result<Vec<SearchResult>, CadenceError> {
    YT_DLP_SERVICE.search(query, limit).await
}

/// Resuelve audio URL (API pública compatible con código existente).
pub async fn resolve_audio(video_id: &str) -> Result<String, CadenceError> {
    YT_DLP_SERVICE.resolve_audio(video_id).await
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn search_returns_results_shaped_as_search_result() {
        let results = search("Imagine Dragons Believer", 1).await.expect("search should succeed");
        assert_eq!(results.len(), 1);
        let first = &results[0];
        assert!(!first.id.is_empty());
        assert!(!first.title.is_empty());
        assert!(first.duration_seconds > 0);
        assert!(first.thumbnail_url.contains(&first.id));
    }

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
        let results: Vec<SearchResult> = entries.into_iter().filter(|e| e.ie_key == "Youtube").map(SearchResult::from).collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "abc123");
    }

    #[tokio::test]
    async fn search_respects_the_requested_limit() {
        let results = search("Linkin Park Numb", 3).await.expect("search should succeed");
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn resolve_audio_returns_a_direct_stream_url() {
        let results = search("Imagine Dragons Believer", 1).await.expect("search should succeed");
        let video_id = &results[0].id;
        let url = resolve_audio(video_id).await.expect("resolve_audio should succeed");
        assert!(url.starts_with("https://"));
    }

    #[tokio::test]
    async fn resolve_audio_fails_for_a_nonexistent_video_id() {
        let result = resolve_audio("this-id-does-not-exist-000").await;
        assert!(result.is_err());
    }

    #[test]
    fn recognizes_sign_in_required_as_a_distinct_error() {
        let service = YtDlpService::new();
        let raw = "ERROR: [youtube] abc123: Sign in to confirm your age.";
        let classified = service.annotate_execution_failure(raw.to_string(), &["https://www.youtube.com/watch?v=abc123"]);
        assert!(matches!(classified, CadenceError::YtDlpAuthRequired { video_id } if video_id == "abc123"));
    }

    #[test]
    fn leaves_other_execution_failures_unclassified() {
        let service = YtDlpService::new();
        let raw = "ERROR: [youtube] abc123: Video unavailable.";
        let classified = service.annotate_execution_failure(raw.to_string(), &["https://www.youtube.com/watch?v=abc123"]);
        assert!(matches!(classified, CadenceError::YtDlpExecution { .. }));
    }

    #[tokio::test]
    async fn cache_works_for_resolve_audio() {
        let service = YtDlpService::new();
        let results = search("Imagine Dragons Believer", 1).await.expect("search");
        let video_id = &results[0].id;
        
        // Primera llamada - cache miss
        let url1 = service.resolve_audio(video_id).await.expect("resolve 1");
        // Segunda llamada - cache hit
        let url2 = service.resolve_audio(video_id).await.expect("resolve 2");
        assert_eq!(url1, url2);
    }

    #[tokio::test]
    async fn semaphore_limits_concurrency() {
        let service = YtDlpService::new();
        let permits = service.semaphore.available_permits();
        assert_eq!(permits, MAX_CONCURRENT_YT_DLP);
    }

    #[test]
    fn ttl_uses_signed_expiry_minus_safety_margin() {
        let now = 1_000_000;
        let url = format!(
            "https://rr1.googlevideo.com/videoplayback?expire={}&mime=audio",
            now + 3600
        );
        // La firma dura 1h, pero el TTL se capa a 30 min: re-resolvemos antes
        // de acercarnos al borde.
        assert_eq!(ttl_from_url(&url, now), CACHE_TTL);

        // Si la firma caduca antes que el TTL por defecto, manda la firma
        // (menos el margen de seguridad de 60s).
        let short = format!("https://x.test/a?expire={}", now + 600);
        assert_eq!(ttl_from_url(&short, now), Duration::from_secs(540));
    }

    #[test]
    fn ttl_is_zero_for_already_expired_urls() {
        let now = 1_000_000;
        let url = format!("https://rr1.googlevideo.com/videoplayback?expire={}", now - 10);
        assert_eq!(ttl_from_url(&url, now), Duration::ZERO);
        // También si está dentro del margen de seguridad.
        let almost = format!("https://x.test/a?expire={}", now + 30);
        assert_eq!(ttl_from_url(&almost, now), Duration::ZERO);
    }

    #[test]
    fn ttl_falls_back_to_default_without_expire_param() {
        let url = "https://rr1.googlevideo.com/videoplayback?mime=audio";
        assert_eq!(ttl_from_url(url, 1_000_000), CACHE_TTL);
    }

    #[test]
    fn ttl_is_capped_at_default_ttl() {
        let now = 1_000_000;
        let url = format!("https://x.test/a?expire={}", now + 24 * 3600);
        assert_eq!(ttl_from_url(&url, now), CACHE_TTL);
    }

    #[test]
    fn expired_url_is_not_cached() {
        let entry = CacheEntry::new(format!(
            "https://x.test/a?expire={}",
            now_unix_secs().saturating_sub(5)
        ));
        assert!(!entry.is_valid());
    }
}