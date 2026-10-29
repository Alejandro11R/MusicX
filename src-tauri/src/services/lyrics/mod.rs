pub mod cache;
pub mod id3;
pub mod lrclib;
pub mod parser;
pub mod text;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::{broadcast, Mutex, OnceCell};

use crate::errors::CadenceError;
use crate::models::lyrics::{
    LyricsCacheStats, LyricsCandidate, LyricsDoc, LyricsEvent, LyricsRequest, LyricsSource,
};

use cache::LyricsCache;
use lrclib::LrclibClient;
use parser::parse_lrc;
use text::{clean_artist, clean_title};

type LyricsOutcome = Result<Arc<LyricsDoc>, String>;

pub struct LyricsService {
    cache: LyricsCache,
    lrclib: LrclibClient,
    in_flight: Mutex<HashMap<String, Arc<OnceCell<LyricsOutcome>>>>,
    enabled: AtomicBool,
    events: broadcast::Sender<LyricsEvent>,
}

impl Default for LyricsService {
    fn default() -> Self {
        Self::new()
    }
}

impl LyricsService {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(16);
        let service = Self {
            cache: LyricsCache::new(),
            lrclib: LrclibClient::new(),
            in_flight: Mutex::new(HashMap::new()),
            enabled: AtomicBool::new(true),
            events,
        };
        service.cache.spawn_prune();
        service
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LyricsEvent> {
        self.events.subscribe()
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Precarga silenciosa en caché para el siguiente track en la cola sin emitir eventos al frontend.
    pub fn prefetch(self: &Arc<Self>, request: LyricsRequest) {
        if !self.is_enabled() {
            return;
        }
        let service = Arc::clone(self);
        tokio::spawn(async move {
            let _ = service.load(request).await;
        });
    }

    /// Fire-and-forget desde el orquestador. Nunca bloquea `play()`.
    pub fn request(self: &Arc<Self>, request: LyricsRequest) {
        if !self.is_enabled() {
            return;
        }
        let service = Arc::clone(self);
        tokio::spawn(async move {
            let track_id = request.track_id.clone();
            match service.load(request).await {
                Ok(doc) => {
                    let _ = service.events.send(LyricsEvent::Ready((*doc).clone()));
                }
                Err(error) => {
                    tracing::warn!(track_id, error = %error, "lyrics lookup failed");
                    let _ = service.events.send(LyricsEvent::Unavailable {
                        track_id,
                        reason: "No se pudo contactar con el proveedor de letras.".into(),
                    });
                }
            }
        });
    }

    pub async fn load(&self, request: LyricsRequest) -> Result<Arc<LyricsDoc>, CadenceError> {
        if !self.is_enabled() {
            return Ok(Arc::new(LyricsDoc::empty(&request.track_id)));
        }

        let key = cache_key(&request);
        let cell = {
            let mut in_flight = self.in_flight.lock().await;
            Arc::clone(
                in_flight
                    .entry(key.clone())
                    .or_insert_with(|| Arc::new(OnceCell::new())),
            )
        };

        let outcome = cell
            .get_or_init(|| async {
                self.fetch_uncached(&request)
                    .await
                    .map_err(|error| error.to_string())
            })
            .await
            .clone();

        self.in_flight.lock().await.remove(&key);

        outcome.map_err(|message| CadenceError::Lyrics { message })
    }

    pub async fn search(&self, request: &LyricsRequest) -> Result<Vec<LyricsCandidate>, CadenceError> {
        self.lrclib.search(request).await
    }

    pub async fn load_by_id(&self, id: i64, track_id: &str) -> Result<Arc<LyricsDoc>, CadenceError> {
        match self.lrclib.get_by_id(id).await? {
            Some(mut doc) => {
                doc.track_id = track_id.to_string();
                Ok(Arc::new(doc))
            }
            None => Ok(Arc::new(LyricsDoc::empty(track_id))),
        }
    }

    pub async fn stats(&self) -> LyricsCacheStats {
        self.cache.stats().await
    }

    pub async fn clear_cache(&self) -> u64 {
        self.cache.clear().await
    }

    async fn fetch_uncached(&self, request: &LyricsRequest) -> Result<Arc<LyricsDoc>, CadenceError> {
        // 1. Archivo local: sidecar .lrc manda sobre cualquier proveedor.
        if let Some(path) = local_audio_path(request) {
            if let Some(sidecar) = sidecar_for(&path) {
                if let Ok(raw) = read_text_capped(&sidecar).await {
                    let parsed = parse_lrc(&raw);
                    if !parsed.lines.is_empty() {
                        return Ok(Arc::new(doc_from_parsed(request, parsed, LyricsSource::LocalLrc)));
                    }
                }
            }

            // 2. Letra embebida en ID3 (USLT/SYLT).
            if let Some(embedded) = id3::read_embedded(&path).await {
                let parsed = parse_lrc(&embedded);
                if !parsed.lines.is_empty() {
                    return Ok(Arc::new(doc_from_parsed(request, parsed, LyricsSource::Id3)));
                }
            }
        }

        // 3. Caché (memoria + disco). Un `force` la salta.
        let key = cache_key(request);
        if !request.force {
            if let Some(hit) = self.cache.get(&key).await {
                return Ok(hit);
            }
        }

        // 4. LRCLIB: exacto → difuso → vacío (negative cache).
        let doc = match self.lrclib.get_exact(request).await.ok().flatten() {
            Some(doc) => doc,
            None => match self.lrclib.search_best(request).await.ok().flatten() {
                Some(doc) => doc,
                None => LyricsDoc::empty(&request.track_id),
            },
        };

        let doc = Arc::new(doc);
        self.cache.put(key, Arc::clone(&doc)).await;
        Ok(doc)
    }
}

impl LyricsRequest {
    pub fn from_track(track: &crate::models::search_result::SearchResult) -> Self {
        let path = std::path::Path::new(&track.id);
        let is_local = path.is_absolute() && path.is_file();

        let (title, artist) = match track.artist.as_deref() {
            Some(artist)
                if artist != "Archivo Local"
                    && !artist.eq_ignore_ascii_case("cadence")
                    && !artist.to_lowercase().starts_with("cadence") =>
            {
                (clean_title(&track.title, Some(artist)), Some(artist.to_string()))
            }
            _ => (track.title.clone(), None),
        };

        Self {
            track_id: track.id.clone(),
            title,
            artist,
            album: None,
            duration_seconds: track.duration_seconds as f64,
            local_path: is_local.then(|| track.id.clone()),
            force: false,
        }
    }
}

fn doc_from_parsed(
    request: &LyricsRequest,
    parsed: parser::ParsedLrc,
    source: LyricsSource,
) -> LyricsDoc {
    LyricsDoc {
        track_id: request.track_id.clone(),
        source,
        synced: parsed.synced,
        instrumental: false,
        offset_ms: parsed.offset_ms,
        lines: parsed.lines,
        title: parsed.title,
        artist: parsed.artist,
    }
}

fn cache_key(request: &LyricsRequest) -> String {
    let normalize = |value: &str| {
        value
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };

    if let Some(path) = &request.local_path {
        return format!("local:{}", path);
    }

    let title = normalize(&clean_title(&request.title, request.artist.as_deref()));
    let artist = request
        .artist
        .as_deref()
        .map(clean_artist)
        .map(|value| normalize(&value))
        .unwrap_or_default();
    let duration = request.duration_seconds.round() as u64;

    format!("lrclib:{title}|{artist}|{duration}")
}

fn local_audio_path(request: &LyricsRequest) -> Option<PathBuf> {
    if let Some(path) = &request.local_path {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let path = PathBuf::from(&request.track_id);
    if path.is_absolute() && path.is_file() {
        Some(path)
    } else {
        None
    }
}

fn sidecar_for(audio: &Path) -> Option<PathBuf> {
    for extension in ["lrc", "LRC"] {
        let candidate = audio.with_extension(extension);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

async fn read_text_capped(path: &Path) -> Result<String, CadenceError> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| CadenceError::Lyrics {
            message: format!("lyrics metadata: {error}"),
        })?;
    if metadata.len() > 512 * 1024 {
        return Err(CadenceError::Lyrics {
            message: "lyrics file too large".into(),
        });
    }

    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| CadenceError::Lyrics {
            message: format!("lyrics read: {error}"),
        })?;

    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Ok(String::from_utf8_lossy(&bytes[3..]).into_owned());
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        return Ok(String::from_utf16_lossy(&units));
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect();
        return Ok(String::from_utf16_lossy(&units));
    }

    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
