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

        // Forced refreshes must not join a cache-reading lookup.
        let key = format!("{}|force:{}", cache_key(&request), request.force);
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

        let mut in_flight = self.in_flight.lock().await;
        if in_flight
            .get(&key)
            .is_some_and(|entry| Arc::ptr_eq(entry, &cell))
        {
            in_flight.remove(&key);
        }
        drop(in_flight);

        outcome
            .map(|mut doc| {
                if doc.track_id != request.track_id {
                    Arc::make_mut(&mut doc).track_id = request.track_id.clone();
                }
                doc
            })
            .map_err(|message| CadenceError::Lyrics { message })
    }

    pub async fn search(
        &self,
        request: &LyricsRequest,
    ) -> Result<Vec<LyricsCandidate>, CadenceError> {
        self.lrclib.search(request).await
    }

    pub async fn load_by_id(
        &self,
        id: i64,
        track_id: &str,
    ) -> Result<Arc<LyricsDoc>, CadenceError> {
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

    async fn fetch_uncached(
        &self,
        request: &LyricsRequest,
    ) -> Result<Arc<LyricsDoc>, CadenceError> {
        // 1. Archivo local: sidecar .lrc manda sobre cualquier proveedor.
        if let Some(path) = local_audio_path(request) {
            if let Some(sidecar) = sidecar_for(&path) {
                if let Ok(raw) = read_text_capped(&sidecar).await {
                    let parsed = parse_lrc(&raw);
                    if !parsed.lines.is_empty() {
                        return Ok(Arc::new(doc_from_parsed(
                            request,
                            parsed,
                            LyricsSource::LocalLrc,
                        )));
                    }
                }
            }

            // 2. Letra embebida en ID3 (USLT/SYLT).
            if let Some(embedded) = id3::read_embedded(&path).await {
                let parsed = parse_lrc(&embedded);
                if !parsed.lines.is_empty() {
                    return Ok(Arc::new(doc_from_parsed(
                        request,
                        parsed,
                        LyricsSource::Id3,
                    )));
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
        let exact = self.lrclib.get_exact(request).await;
        let doc = match exact {
            Ok(Some(doc)) => doc,
            exact => match self.lrclib.search_best(request).await {
                Ok(Some(doc)) => doc,
                fallback => {
                    // Absence is definitive only when every attempted lookup succeeded.
                    if let Err(error) = exact {
                        return Err(error);
                    }
                    fallback?;
                    LyricsDoc::empty(&request.track_id)
                }
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
                (
                    clean_title(&track.title, Some(artist)),
                    Some(artist.to_string()),
                )
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicUsize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const TRACK: &str =
        r#"{"id":1,"trackName":"Song","artistName":"Artist","plainLyrics":"Hello"}"#;
    const RESULTS: &str =
        r#"[{"id":1,"trackName":"Song","artistName":"Artist","plainLyrics":"Hello"}]"#;

    struct Fixture {
        service: Arc<LyricsService>,
        calls: Arc<AtomicUsize>,
        server: tokio::task::JoinHandle<()>,
        dir: PathBuf,
    }

    impl Fixture {
        async fn new(responses: Vec<(u16, &'static str)>) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}/", listener.local_addr().unwrap());
            let calls = Arc::new(AtomicUsize::new(0));
            let count = calls.clone();
            let server = tokio::spawn(async move {
                let mut responses = VecDeque::from(responses);
                loop {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut buffer = [0; 4096];
                    socket.read(&mut buffer).await.unwrap();
                    count.fetch_add(1, Ordering::SeqCst);
                    let (status, body) = responses.pop_front().unwrap_or((200, "[]"));
                    if status == 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                        continue;
                    }
                    let reply = format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\nRetry-After: 0\r\n\r\n{body}", body.len());
                    let _ = socket.write_all(reply.as_bytes()).await;
                }
            });
            let dir = std::env::temp_dir().join(format!(
                "cadence-lyrics-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let (events, _) = broadcast::channel(16);
            Self {
                service: Arc::new(LyricsService {
                    cache: LyricsCache::isolated(dir.clone()),
                    lrclib: LrclibClient::for_test(base.parse().unwrap()),
                    in_flight: Mutex::new(HashMap::new()),
                    enabled: AtomicBool::new(true),
                    events,
                }),
                calls,
                server,
                dir,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.server.abort();
            std::fs::remove_dir_all(&self.dir).unwrap();
        }
    }

    #[tokio::test]
    async fn coalesced_failure_retries_without_disk_mutation() {
        let fixture = Fixture::new(vec![(500, ""), (500, ""), (500, ""), (200, TRACK)]).await;
        assert_eq!(std::fs::read_dir(&fixture.dir).unwrap().count(), 0);

        let (a, b) = tokio::join!(
            fixture.service.load(request("first")),
            fixture.service.load(request("second"))
        );
        assert!(a.is_err());
        assert!(b.is_err());
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 3);
        assert_eq!(fixture.service.stats().await.memory_entries, 0);
        assert_eq!(std::fs::read_dir(&fixture.dir).unwrap().count(), 0);

        let retry = fixture.service.load(request("retry")).await.unwrap();
        assert_eq!(retry.track_id, "retry");
        assert_eq!(retry.lines[0].text, "Hello");
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 4);
    }

    fn request(id: &str) -> LyricsRequest {
        LyricsRequest {
            track_id: id.into(),
            title: "Song".into(),
            artist: Some("Artist".into()),
            album: None,
            duration_seconds: 0.0,
            local_path: None,
            force: false,
        }
    }

    #[tokio::test]
    async fn definitive_absence_is_cached_and_rebound() {
        let fixture = Fixture::new(vec![(404, "")]).await;
        let first = fixture.service.load(request("first")).await.unwrap();
        assert_eq!(first.source, LyricsSource::None);
        let calls = fixture.calls.load(Ordering::SeqCst);
        let second = fixture.service.load(request("second")).await.unwrap();
        assert_eq!(second.track_id, "second");
        assert_eq!(fixture.calls.load(Ordering::SeqCst), calls);
    }

    #[tokio::test]
    async fn transient_errors_are_not_negative_cached() {
        for response in [(500, ""), (429, ""), (200, "broken"), (0, "")] {
            // All attempted fallbacks fail, including the automatic 429 retry.
            let mut responses = vec![response; if response.0 == 429 { 6 } else { 3 }];
            responses.push((200, TRACK));
            let fixture = Fixture::new(responses).await;
            assert!(
                fixture.service.load(request("first")).await.is_err(),
                "{response:?}"
            );
            assert_eq!(fixture.service.stats().await.memory_entries, 0);
            assert_eq!(fixture.service.stats().await.disk_entries, 0);
            let calls = fixture.calls.load(Ordering::SeqCst);
            let retry = fixture.service.load(request("retry")).await.unwrap();
            assert_eq!(retry.track_id, "retry");
            assert_eq!(retry.lines[0].text, "Hello");
            assert!(fixture.calls.load(Ordering::SeqCst) > calls);
        }
    }

    #[tokio::test]
    async fn fallback_success_recovers_errors_but_absence_does_not() {
        let fixture = Fixture::new(vec![(500, ""), (200, "[]"), (200, "[]")]).await;
        assert!(fixture.service.load(request("failed")).await.is_err());
        let fixture = Fixture::new(vec![(500, ""), (500, ""), (200, RESULTS)]).await;
        let doc = fixture.service.load(request("recovered")).await.unwrap();
        assert_eq!(doc.lines[0].text, "Hello");
        assert_eq!(doc.track_id, "recovered");
        let fixture = Fixture::new(vec![
            (404, ""),
            (500, ""),
            (
                200,
                r#"[{"id":2,"trackName":"Unrelated","artistName":"Other"}]"#,
            ),
        ])
        .await;
        assert!(fixture.service.load(request("unmatched")).await.is_err());
    }

    #[tokio::test]
    async fn successful_exact_and_coalesced_forced_requests_keep_identity() {
        let fixture = Fixture::new(vec![(200, TRACK), (200, TRACK)]).await;
        let mut first = request("first");
        first.force = true;
        let mut second = request("second");
        second.force = true;
        let (a, b) = tokio::join!(fixture.service.load(first), fixture.service.load(second));
        assert_eq!(a.unwrap().track_id, "first");
        let b = b.unwrap();
        assert_eq!(b.track_id, "second");
        assert_eq!(b.lines[0].text, "Hello");
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
        let cached = fixture.service.load(request("cached")).await.unwrap();
        assert_eq!(cached.track_id, "cached");
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
        let mut refresh = request("refresh");
        refresh.force = true;
        assert_eq!(
            fixture.service.load(refresh).await.unwrap().track_id,
            "refresh"
        );
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn manual_search_and_by_id_keep_successful_parsing() {
        let fixture = Fixture::new(vec![(200, RESULTS), (200, TRACK), (404, "")]).await;
        let candidates = fixture.service.search(&request("manual")).await.unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, 1);
        let doc = fixture.service.load_by_id(1, "chosen").await.unwrap();
        assert_eq!(doc.track_id, "chosen");
        assert_eq!(doc.lines[0].text, "Hello");
        let absent = fixture.service.load_by_id(2, "absent").await.unwrap();
        assert_eq!(absent.source, LyricsSource::None);
    }

    #[tokio::test]
    async fn prefetch_is_silent_and_request_keeps_unavailable_event() {
        let fixture = Fixture::new(vec![(200, TRACK)]).await;
        let mut events = fixture.service.subscribe();
        fixture.service.prefetch(request("prefetch"));
        tokio::task::yield_now().await;
        fixture.service.load(request("foreground")).await.unwrap();
        assert!(matches!(
            events.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        fixture.service.request(request("visible"));
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(event, LyricsEvent::Ready(doc) if doc.track_id == "visible"));

        let fixture = Fixture::new(vec![(500, ""); 3]).await;
        let mut events = fixture.service.subscribe();
        fixture.service.request(request("failed"));
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(event, LyricsEvent::Unavailable { track_id, reason }
            if track_id == "failed" && reason == "No se pudo contactar con el proveedor de letras.")
        );
    }

    #[tokio::test]
    async fn disk_hits_and_local_sidecars_preserve_priority() {
        let mut fixture = Fixture::new(vec![(200, TRACK)]).await;
        fixture.service.load(request("original")).await.unwrap();
        Arc::get_mut(&mut fixture.service).unwrap().cache =
            LyricsCache::isolated(fixture.dir.clone());
        let disk = fixture.service.load(request("disk")).await.unwrap();
        assert_eq!(disk.track_id, "disk");
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
        let audio = fixture.dir.join("song.mp3");
        std::fs::write(&audio, []).unwrap();
        std::fs::write(audio.with_extension("lrc"), "[00:01.00]Local line").unwrap();
        let mut local = request("local");
        local.local_path = Some(audio.to_string_lossy().into_owned());
        let doc = fixture.service.load(local).await.unwrap();
        assert_eq!(doc.source, LyricsSource::LocalLrc);
        assert_eq!(doc.lines[0].text, "Local line");
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    }
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
