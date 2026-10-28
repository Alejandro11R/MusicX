use serde::{Deserialize, Serialize};

/// De dónde salió la letra. `None` significa "buscamos y no hay".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LyricsSource {
    #[default]
    None,
    Lrclib,
    LocalLrc,
    Id3,
}

/// Palabra con timestamp (LRC enriquecido: `[00:12.00]<00:12.00>Hello <00:12.40>world`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricWord {
    pub time_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<u64>,
    /// Incluye el espacio original para poder reconstruir la línea sin perder separadores.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<LyricWord>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LyricsDoc {
    /// Se compara en el frontend para descartar respuestas de un track anterior.
    pub track_id: String,
    pub source: LyricsSource,
    /// `false` = letra plana (sin timestamps). La UI no resalta ni auto-scrollea.
    pub synced: bool,
    pub instrumental: bool,
    /// Offset declarado en el propio LRC (`[offset:+200]`), ya aplicado a `lines`.
    pub offset_ms: i32,
    pub lines: Vec<LyricLine>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
}

impl LyricsDoc {
    pub fn empty(track_id: &str) -> Self {
        Self {
            track_id: track_id.to_string(),
            source: LyricsSource::None,
            synced: false,
            instrumental: false,
            offset_ms: 0,
            lines: Vec::new(),
            title: None,
            artist: None,
        }
    }
}

/// Lo que manda el frontend. No se acepta una URL: el host está fijado en el backend.
#[derive(Debug, Clone, Deserialize)]
pub struct LyricsRequest {
    pub track_id: String,
    pub title: String,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration_seconds: f64,
    /// Solo para pistas locales. Si falta, el backend intenta `track_id` como ruta.
    #[serde(default)]
    pub local_path: Option<String>,
    /// Salta la caché (botón "recargar letra").
    #[serde(default)]
    pub force: bool,
}

/// Resultado de `/api/search`, para el selector manual.
#[derive(Debug, Clone, Serialize)]
pub struct LyricsCandidate {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_seconds: f64,
    pub has_synced: bool,
    pub instrumental: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LyricsCacheStats {
    pub memory_entries: usize,
    pub disk_entries: u64,
    pub disk_bytes: u64,
}

/// Canal propio, separado de `orchestrator-event`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum LyricsEvent {
    Ready(LyricsDoc),
    Unavailable { track_id: String, reason: String },
}

/// Búsqueda binaria: última línea cuyo `time_ms <= t`.
/// `None` antes de la primera línea.
pub fn line_at(lines: &[LyricLine], time_ms: u64) -> Option<usize> {
    let index = lines.partition_point(|line| line.time_ms <= time_ms);
    index.checked_sub(1)
}
