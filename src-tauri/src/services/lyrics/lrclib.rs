use std::time::{Duration, Instant};

use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;
use tokio::sync::{Mutex, Semaphore};

use crate::errors::CadenceError;
use crate::models::lyrics::{
    LyricsCandidate, LyricsDoc, LyricsRequest, LyricsSource,
};

use super::parser::{parse_lrc, MAX_LINES};
use super::text::{clean_artist, clean_title};

const BASE_URL: &str = "https://lrclib.net/";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MIN_INTERVAL: Duration = Duration::from_millis(200);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(6);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibTrack {
    id: i64,
    track_name: String,
    artist_name: String,
    #[serde(default)]
    album_name: Option<String>,
    #[serde(default)]
    duration: f64,
    #[serde(default)]
    instrumental: bool,
    #[serde(default)]
    plain_lyrics: Option<String>,
    #[serde(default)]
    synced_lyrics: Option<String>,
}

pub struct LrclibClient {
    http: Client,
    base: Url,
    gate: Semaphore,
    last_request: Mutex<Instant>,
}

impl LrclibClient {
    pub fn new() -> Self {
        let http = Client::builder()
            .user_agent(concat!(
                "Cadence/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/xalejo/cadence)"
            ))
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .pool_max_idle_per_host(2)
            .build()
            .unwrap_or_else(|_| Client::new());

        let base = Url::parse(BASE_URL).unwrap_or_else(|_| {
            Url::parse("https://lrclib.net/").unwrap_or_else(|_| unreachable!())
        });

        Self {
            http,
            base,
            gate: Semaphore::new(2),
            last_request: Mutex::new(Instant::now() - MIN_INTERVAL),
        }
    }

    /// Match exacto. `Ok(None)` = 404 (no existe), no es un error de red.
    pub async fn get_exact(
        &self,
        request: &LyricsRequest,
    ) -> Result<Option<LyricsDoc>, CadenceError> {
        let Some(artist) = request.artist.as_deref().filter(|a| !a.trim().is_empty()) else {
            return Ok(None);
        };

        let mut url = self.endpoint("api/get")?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("track_name", &clean_title(&request.title, Some(artist)));
            query.append_pair("artist_name", &clean_artist(artist));
            if let Some(album) = request.album.as_deref().filter(|a| !a.trim().is_empty()) {
                query.append_pair("album_name", album);
            }
            if request.duration_seconds >= 1.0 {
                query.append_pair(
                    "duration",
                    &format!("{}", request.duration_seconds.round() as u64),
                );
            }
        }

        match self.send(url).await? {
            Some(track) => Ok(Some(track.into_doc(request))),
            None => Ok(None),
        }
    }

    /// Fallback difuso. Devuelve el mejor candidato por encima del umbral.
    pub async fn search_best(
        &self,
        request: &LyricsRequest,
    ) -> Result<Option<LyricsDoc>, CadenceError> {
        let candidates = self.search_tracks(request).await?;

        let best = candidates
            .into_iter()
            .map(|candidate| {
                let score = score(&candidate, request);
                (score, candidate)
            })
            .filter(|(score, _)| *score >= 40)
            .max_by_key(|(score, _)| *score)
            .map(|(_, candidate)| candidate);

        Ok(best.map(|candidate| candidate.into_doc(request)))
    }

    /// Búsqueda manual: devuelve candidatos listos para la UI.
    pub async fn search(
        &self,
        request: &LyricsRequest,
    ) -> Result<Vec<LyricsCandidate>, CadenceError> {
        Ok(self
            .search_tracks(request)
            .await?
            .into_iter()
            .map(|track| LyricsCandidate {
                id: track.id,
                title: track.track_name,
                artist: track.artist_name,
                album: track.album_name,
                duration_seconds: track.duration,
                has_synced: track.synced_lyrics.is_some(),
                instrumental: track.instrumental,
            })
            .collect())
    }

    async fn search_tracks(
        &self,
        request: &LyricsRequest,
    ) -> Result<Vec<LrclibTrack>, CadenceError> {
        let is_placeholder_artist = request
            .artist
            .as_deref()
            .map(|a| {
                a.trim().is_empty()
                    || a.eq_ignore_ascii_case("cadence")
                    || a.to_lowercase().starts_with("cadence")
                    || a == "Archivo Local"
            })
            .unwrap_or(true);

        // Si tenemos un artista real, probar búsqueda exacta por track_name + artist_name primero
        if !is_placeholder_artist {
            let mut url = self.endpoint("api/search")?;
            {
                let mut query = url.query_pairs_mut();
                let title = clean_title(&request.title, request.artist.as_deref());
                let artist = request.artist.as_deref().unwrap();
                query.append_pair("track_name", &title);
                query.append_pair("artist_name", &clean_artist(artist));
            }

            if let Ok(response) = self.request(url).await {
                if response.status().is_success() {
                    if let Ok(tracks) = response.json::<Vec<LrclibTrack>>().await {
                        if !tracks.is_empty() {
                            return Ok(tracks);
                        }
                    }
                }
            }
        }

        // Búsqueda libre `q`: extremadamente efectiva para títulos de YouTube
        let query_text = clean_title(&request.title, None);
        let mut url = self.endpoint("api/search")?;
        url.query_pairs_mut().append_pair("q", &query_text);
        if let Ok(response) = self.request(url).await {
            if response.status().is_success() {
                if let Ok(tracks) = response.json::<Vec<LrclibTrack>>().await {
                    if !tracks.is_empty() {
                        return Ok(tracks);
                    }
                }
            }
        }

        // Si el título contiene " - ", probar con las partes individuales
        if let Some((part1, part2)) = request.title.split_once(" - ") {
            for part in [part1, part2] {
                let clean_part = clean_title(part, None);
                let mut url = self.endpoint("api/search")?;
                url.query_pairs_mut().append_pair("q", &clean_part);
                if let Ok(response) = self.request(url).await {
                    if response.status().is_success() {
                        if let Ok(tracks) = response.json::<Vec<LrclibTrack>>().await {
                            if !tracks.is_empty() {
                                return Ok(tracks);
                            }
                        }
                    }
                }
            }
        }

        Ok(Vec::new())
    }

    pub async fn get_by_id(&self, id: i64) -> Result<Option<LyricsDoc>, CadenceError> {
        let mut url = self.endpoint(&format!("api/get/{id}"))?;
        url.set_query(None);
        match self.send(url).await? {
            Some(track) => {
                let request = LyricsRequest {
                    track_id: String::new(),
                    title: track.track_name.clone(),
                    artist: Some(track.artist_name.clone()),
                    album: track.album_name.clone(),
                    duration_seconds: track.duration,
                    local_path: None,
                    force: true,
                };
                Ok(Some(track.into_doc(&request)))
            }
            None => Ok(None),
        }
    }

    fn endpoint(&self, path: &str) -> Result<Url, CadenceError> {
        self.base.join(path).map_err(|error| CadenceError::Lyrics {
            message: format!("lrclib url: {error}"),
        })
    }

    async fn send(&self, url: Url) -> Result<Option<LrclibTrack>, CadenceError> {
        let response = self.request(url).await?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(CadenceError::Lyrics {
                message: format!("lrclib returned {status}"),
            });
        }

        let bytes = response.bytes().await.map_err(|error| CadenceError::Lyrics {
            message: format!("lrclib body: {error}"),
        })?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(CadenceError::Lyrics {
                message: "lrclib response too large".into(),
            });
        }

        serde_json::from_slice::<LrclibTrack>(&bytes)
            .map(Some)
            .map_err(|error| CadenceError::Lyrics {
                message: format!("lrclib parse: {error}"),
            })
    }

    async fn request(&self, url: Url) -> Result<reqwest::Response, CadenceError> {
        let _permit = self.gate.acquire().await.map_err(|_| CadenceError::Lyrics {
            message: "lrclib gate closed".into(),
        })?;

        {
            let mut last = self.last_request.lock().await;
            let elapsed = last.elapsed();
            if elapsed < MIN_INTERVAL {
                tokio::time::sleep(MIN_INTERVAL - elapsed).await;
            }
            *last = Instant::now();
        }

        let response = self
            .http
            .get(url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|error| CadenceError::Lyrics {
                message: format!("lrclib request: {error}"),
            })?;

        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(2)
                .min(5);
            tokio::time::sleep(Duration::from_secs(retry_after)).await;
            return self
                .http
                .get(response.url().clone())
                .header("Accept", "application/json")
                .send()
                .await
                .map_err(|error| CadenceError::Lyrics {
                    message: format!("lrclib retry: {error}"),
                });
        }

        Ok(response)
    }
}

impl LrclibTrack {
    fn into_doc(self, request: &LyricsRequest) -> LyricsDoc {
        let synced = self
            .synced_lyrics
            .as_deref()
            .map(parse_lrc)
            .filter(|parsed| parsed.synced && !parsed.lines.is_empty());

        let (lines, is_synced) = match synced {
            Some(parsed) => (parsed.lines, true),
            None => match self.plain_lyrics.as_deref() {
                Some(plain) if !plain.trim().is_empty() => (parse_plain(plain), false),
                _ => (Vec::new(), false),
            },
        };

        LyricsDoc {
            track_id: request.track_id.clone(),
            source: LyricsSource::Lrclib,
            synced: is_synced,
            instrumental: self.instrumental,
            offset_ms: 0,
            lines,
            title: Some(self.track_name),
            artist: Some(self.artist_name),
        }
    }
}

fn parse_plain(raw: &str) -> Vec<crate::models::lyrics::LyricLine> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(MAX_LINES)
        .map(|text| crate::models::lyrics::LyricLine {
            time_ms: 0,
            text: text.to_string(),
            words: None,
        })
        .collect()
}

fn score(candidate: &LrclibTrack, request: &LyricsRequest) -> i32 {
    let raw_title = request.title.to_lowercase();
    let candidate_title = candidate.track_name.to_lowercase();
    let candidate_artist = candidate.artist_name.to_lowercase();

    let mut score = 0;
    if candidate_title == raw_title {
        score += 100;
    } else if raw_title.contains(&candidate_title) || candidate_title.contains(&raw_title) {
        score += 60;
    } else {
        let title_words: Vec<&str> = raw_title.split_whitespace().collect();
        let matched = title_words.iter().filter(|w| candidate_title.contains(**w)).count();
        if !title_words.is_empty() {
            score += ((matched as f32 / title_words.len() as f32) * 45.0) as i32;
        }
    }

    if let Some(artist) = request.artist.as_deref().filter(|a| !a.eq_ignore_ascii_case("cadence")) {
        let wanted_artist = artist.to_lowercase();
        if candidate_artist == wanted_artist || raw_title.contains(&candidate_artist) {
            score += 40;
        } else if candidate_artist.contains(&wanted_artist) || wanted_artist.contains(&candidate_artist) {
            score += 20;
        }
    } else if raw_title.contains(&candidate_artist) {
        score += 40;
    }

    if candidate.synced_lyrics.is_some() {
        score += 25;
    }

    if request.duration_seconds >= 1.0 && candidate.duration >= 1.0 {
        let difference = (candidate.duration - request.duration_seconds).abs();
        score -= (difference.min(30.0) as i32) * 3;
        if difference <= 3.0 {
            score += 25;
        }
    }

    score
}
