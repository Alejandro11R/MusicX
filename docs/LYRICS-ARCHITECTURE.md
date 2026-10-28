# Cadence — Letras sincronizadas (estilo Apple Music)

**Auditoría técnica y diseño de arquitectura**
**Fecha:** 2026-10-28
**Alcance:** `src-tauri/src/**`, `src/**`, `Cargo.toml`, `tauri.conf.json`, `package.json`.
**Restricción:** rendimiento nativo extremo, cero dependencias que no paguen su coste, y respeto al presupuesto de compositing que ya identificó `docs/AUDIT-2026-10-28.md`.

---

## 0. TL;DR — las seis decisiones que definen el diseño

1. **El backend posee los datos; el frontend posee el reloj.**
   Rust descarga, parsea y cachea. React **no** hace polling ni se re-renderiza por frame: un único `requestAnimationFrame` escribe `transform` y una CSS variable. Cero IPC a 60 Hz.

2. **Las letras no entran en `PlayerState`.**
   `PlayerState` se clona a 2 Hz (audit M1/M6). Meter un `LyricsDoc` de 10–30 KB ahí multiplicaría allocaciones por 60 cada minuto. Va por un canal de eventos propio y efímero.

3. **La descarga nunca bloquea `play()`.**
   `LyricsService::request()` hace `tokio::spawn`. El target de `play()` con URL cacheada es <150 ms; esperar a LRCLIB lo rompería. Si la letra llega tarde, el frontend la ignora por `track_id`.

4. **Caché en tres capas + negative cache + single-flight.**
   Memoria (64 docs) → disco (`~/.cache/cadence/lyrics`, TTL 30 días) → LRCLIB. Un 404 se cachea 24 h para no martillear la API. Dos consumidores que piden la misma letra comparten una sola petición.

5. **Scroll por `transform: translate3d`, no `scrollIntoView`.**
   `scrollIntoView` mueve todos los ancestros, no se puede limitar y cada llamada cancela la animación anterior. Con offsets medidos una vez y `translate3d` el coste es solo del compositor, sin reflow ni layout reads en el frame.

6. **Nada de `backdrop-filter` nuevo.**
   El audit F1 ya señala el blur anidado como el mayor coste de GPU. El panel de letras usa un tinte sólido translúcido + **una** aura estática con `radial-gradient`. El blur por línea se limita a ±4 líneas y no se anima por frame.

**Esfuerzo estimado:** 4–6 días para una v1 completa (backend + UI + tests), 2–3 días extra para word-sync, offset manual, búsqueda manual y prefetch. Una sola dependencia nueva directa: `reqwest` (ya está en el árbol de Tauri; el coste marginal es TLS).

---

## 1. Auditoría del estado actual

### 1.1 Puntos de integración reales

| Punto | Archivo | Qué aporta / qué falta |
|---|---|---|
| Estado del reproductor | `src-tauri/src/models/player_state.rs:27-42` | `status`, `current: Option<SearchResult>`, `position_seconds`, `duration_seconds`, `speed`. **No** hay `album` ni `local_path`. |
| Track | `src-tauri/src/models/search_result.rs:3-10` | `id`, `title`, `artist: Option<String>`, `duration_seconds: u64`, `thumbnail_url`. Suficiente para LRCLIB salvo `album`. |
| Play / cola | `src-tauri/src/services/orchestrator.rs:519-600` | Punto exacto donde lanzar la carga de letras sin bloquear. `TrackStarted` se emite al final. |
| Eventos | `src-tauri/src/services/orchestrator.rs:139-156` | `broadcast(64)`, un solo receptor que reenvía a Tauri (`commands/player.rs:49-73`). |
| Tick de progreso | `src-tauri/src/services/orchestrator.rs:247-265` | 2 Hz, llama `get_state()` (audit M1). Suficiente para re-anclar el reloj de letras. |
| Seek | `src-tauri/src/services/orchestrator.rs:616-633` | Ya existe y recupera el stream si la URL expiró. **No duplicar** con `player_seek`; reutilizar. |
| IPC frontend | `src/lib/tauri.ts:74-208` | Wrappers finos + unión `OrchestratorEvent`. |
| NowPlaying | `src/App.tsx:437-660` | Card inferior; aquí va el botón “Letras”. |
| Color de portada | `src/lib/album-color.ts:22-68` | Canvas 32×32, color promedio. Se puede mejorar a paleta de 2 colores. |
| Presupuesto visual | `src/index.css:84-112`, `src/styles/liquid-glass.css:36-97` | Blur anidado ya en el límite. El panel de letras **no** debe añadir otro. |
| Pistas locales | `src/App.tsx:1133-1145`, `:1218-1228` | `id` = ruta absoluta, `artist` = “Archivo Local”, `duration_seconds` = 0. Para letras locales hay que detectar la ruta en el backend. |

### 1.2 Hallazgos específicos de esta feature

| ID | Severidad | Hallazgo | Corrección |
|---|---|---|---|
| **LY1** | P0 | No existe modelo de letras ni proveedor. `SearchResult` no tiene `album`; `PlayerState` no tiene letras. | `models/lyrics.rs` separado + `LyricsService`. |
| **LY2** | P0 | Si la descarga se hace dentro de `play()` o del event loop del orquestador, se añaden 200–800 ms a cada track y se bloquean eventos de mpv (mismo patrón que C1 del audit). | `tokio::spawn` + `LyricsService::request()`. Nunca `await` en `play()`. |
| **LY3** | P0 | `App.tsx` se re-renderiza a 2 Hz por `Progress`. Si el panel de letras recibe `position_seconds` sin memoización, se diffean ~100 líneas 2 veces/s. | Lista memoizada; el reloj escribe en refs/DOM, no en estado global. |
| **LY4** | P1 | Un `backdrop-filter` o `filter: blur()` por línea dispara capas offscreen y repaints continuos. | Un solo tinte + aura estática; blur solo en ±4 líneas y sin animarlo por frame. |
| **LY5** | P1 | `scrollIntoView({ behavior: "smooth" })` repetido cancela la animación, mueve ancestros y provoca jank. | Offsets cacheados + `translate3d` con transición CSS. |
| **LY6** | P1 | Cadence reproduce a 0.85x–1.25x. Los timestamps de LRCLIB son a 1x; extrapolar sin `speed` deriva 15 % (9 s por minuto a 0.85x). | `time = pos + (wall_elapsed × speed)`. |
| **LY7** | P1 | Títulos de YouTube (“Numb (Official Video)”), versiones live/remix y duraciones distintas producen matches erróneos. | `clean_title`, tolerancia de duración, fallback `/api/search`, offset manual y “buscar otra letra”. |
| **LY8** | P1 | No hay caché de letras. Reutilizar `search_cache` sería un error: no tiene TTL ni cota (audit M4). | Caché propia: LRU memoria + disco + negative cache + single-flight. |
| **LY9** | P2 | `CadenceError` tiene `match` exhaustivos en `user_message()` y `From<CadenceError> for FrontendError` (`errors.rs:57-131`). Un variant nuevo rompe la compilación si se olvida. | Añadir `Lyrics { message }` y actualizar ambos `match`. |
| **LY10** | P2 | Privacidad/legal: cada reproducción enviaría título y artista a un tercero; las letras tienen copyright. | Ajuste “letras online” (opt-out), atribución visible, caché local, no empaquetar letras. |

### 1.3 Restricciones que condicionan el código

- `src-tauri/src/lib.rs:1` tiene `#![deny(clippy::unwrap_used, clippy::expect_used)]`. **Todo** el código nuevo debe usar `?`, `unwrap_or`, `let Some(..) else`. Los tests pueden llevar `#[allow]`.
- El `broadcast` del orquestador es de 64 y un solo receptor. Un `LyricsDoc` grande ahí compite con `Progress`; por eso va por un canal aparte.
- `SearchResult.duration_seconds` es `u64`; LRCLIB espera segundos enteros. Para locales es `0`: hay que omitir `duration` en la query.
- La ventana mide 430×660 (`tauri.conf.json`). El panel de letras debe caber sin convertir la app en un reproductor de escritorio; opción “modo letras” que expande a ~820×660.
- `csp: null` hoy. Las letras se renderizan como texto (React escapa), nunca con `dangerouslySetInnerHTML`.

---

## 2. Decisiones de arquitectura (ADR)

### ADR-01 — Backend de datos, frontend de reloj

**Contexto:** sincronizar 100 líneas a 60 fps con IPC sería absurdo: 60 eventos/s, 60 re-renders/s.

**Decisión:** Rust entrega el documento una vez. El frontend extrapola la posición con `requestAnimationFrame` a partir de las muestras de `Progress` (2 Hz), la velocidad y el offset. La línea activa se calcula con búsqueda binaria local.

**Consecuencias:** 0 IPC durante la reproducción; el único coste es un `rAF` mientras el panel está visible y `status === Playing`. Al pausar, el `rAF` se cancela.

### ADR-02 — Letras fuera de `PlayerState`

**Contexto:** `get_state()` clona `SearchResult` y se llama a 2 Hz (audit M1/M6).

**Decisión:** `LyricsDoc` vive en el `LyricsService` y viaja por `lyrics-event`. El frontend lo guarda en estado local del panel, indexado por `track_id`.

**Consecuencias:** `PlayerState` sigue siendo pequeño; el panel puede descartar letras al cambiar de track sin tocar el estado global.

### ADR-03 — Canal de eventos separado

**Decisión:** `LyricsService` tiene su propio `broadcast::Sender<LyricsEvent>` (capacidad 16), puenteado a Tauri como `"lyrics-event"`. No se añade `LyricsReady` a `OrchestratorEvent`.

**Consecuencias:** el canal caliente del orquestador no arrastra payloads de 20 KB; si el frontend pierde un evento, puede recuperarlo con `get_lyrics`.

### ADR-04 — Caché de tres capas con negative cache y single-flight

**Decisión:**
- Memoria: `TinyLru<LyricsDoc>` de 64 entradas (sin dependencia; 30 líneas).
- Disco: `~/.cache/cadence/lyrics/<fnv1a64>.json`, escritura atómica (`tmp` + `rename`), TTL 30 días, presupuesto 50 MB / 5000 archivos.
- Negativo: un 404 se guarda 24 h (`source: None`, `lines: []`).
- Single-flight: `tokio::sync::OnceCell` por clave; N llamadas concurrentes = 1 petición HTTP.

**Consecuencias:** cache hit <1 ms; repetir un track no toca la red; la API recibe como máximo una petición por track y TTL.

### ADR-05 — Orden de proveedores: local → ID3 → LRCLIB

**Decisión:** para un archivo local, primero `.lrc` sidecar (mismo stem), luego letra embebida (USLT/SYLT), luego LRCLIB usando los tags. Para YouTube, directo a LRCLIB.

**Consecuencias:** el usuario manda sobre la base de datos; una letra editada a mano nunca se pisa con una remota.

### ADR-06 — Scroll por transform, no scroll nativo

**Contexto:** se compararon tres opciones.

| Opción | Pros | Contras |
|---|---|---|
| `scrollIntoView` | 1 línea | Mueve ancestros, no se puede limitar, cancela animaciones, dispara `scroll` |
| `container.scrollTo` | Accesible, rueda/touch gratis | WebKitGTK puede hacer el smooth scroll en main thread; mezclar con blur produce jank |
| `translate3d` | Solo compositor, centrado exacto, control total | Hay que gestionar rueda/teclado a mano (~60 líneas) |

**Decisión:** `translate3d` sobre un track interno, con `overflow: hidden` en el viewport. Offsets medidos con `ResizeObserver` + `document.fonts.ready`, nunca dentro del `rAF`. La rueda se captura en modo “navegación manual” durante 3 s y luego se recentra.

**Consecuencias:** 60 fps estables incluso con blur; se pierde el scroll nativo, se compensa con `tabIndex`, flechas y `aria-current`.

### ADR-07 — Sin blur nuevo

**Decisión:** el panel usa `background: rgba(8,11,20,.82)` + una sola aura `radial-gradient` estática. El blur de líneas es CSS puro sobre ±4 vecinos, sin animación por frame.

**Consecuencias:** no se suma al problema F1; el aura se actualiza solo al cambiar de track (2 variables CSS).

### ADR-08 — Nada de visualizador reactivo al audio

**Contexto:** el audio vive en un proceso mpv separado. Un FFT real requeriría loopback/PipeWire o un segundo backend de audio.

**Decisión:** no se hace. El aura “respira” con `opacity`/`scale` ligados al estado de reproducción, no al espectro. Se documenta como límite consciente.

---

## 3. Flujo completo

```text
mpv time-pos
   │ observe_property (MpvEventService)
   ▼
PlayerOrchestrator ── Progress (2 Hz) ──────────────────────────────┐
   │                                                                │
   │ play(track)                                                    │
   ├──► LyricsService::request(track)  [tokio::spawn, no await]     │
   │        ├─ 1. sidecar .lrc / ID3 (si local)                     │
   │        ├─ 2. TinyLru (memoria)                                 │
   │        ├─ 3. disco (~/.cache/cadence/lyrics)                   │
   │        └─ 4. LRCLIB: /api/get → /api/search                    │
   │                 │                                              │
   │                 ▼                                              │
   │        lyrics-event { Ready(LyricsDoc) }                       │
   │                 │                                              │
   ▼                 ▼                                              ▼
React LyricsPanel: rAF clock (pos + speed + offset) ──► activeIndex (≈0.25 Hz)
   │                                   │
   │                                   └─► transform translate3d + CSS var --p
   └── click línea ──► seek(seconds) ──► PlayerOrchestrator.seek
```

**Secuencia de una reproducción:**

1. `play(track)` resuelve el audio (yt-dlp o ruta local) y fija `current_track`.
2. Lanza `lyrics.request(LyricsRequest::from_track(&track))` y sigue con `loadfile`.
3. El panel se abre al instante con un skeleton; no espera a la red.
4. Cuando llega `Ready`, si `doc.track_id === current.id`, se pinta. Si el usuario ya cambió de track, se descarta.
5. El reloj arranca en el primer `Progress`; la línea activa se actualiza por búsqueda binaria.
6. Click en una línea → `seek(time_ms / 1000)`; el `seekGuardRef` existente en `App.tsx` evita que un `Progress` viejo devuelva el slider atrás.

---

## 4. Backend Rust

### 4.1 Dependencias

```toml
# src-tauri/Cargo.toml
[dependencies]
# Ya está en el árbol como dependencia transitiva de Tauri.
# El coste marginal es TLS; no añade un segundo stack HTTP.
reqwest = { version = "0.13", default-features = false, features = ["json", "rustls"] }
```

> **Ojo con los nombres de feature:** en `reqwest` 0.13 la feature se llama `rustls`, **no** `rustls-tls` (ese nombre era de la rama 0.11/0.12 y ya no existe). Tampoco hay `webpki-roots` ni `rustls-native-certs`: la 0.13 valida certificados con `rustls-platform-verifier`, que usa el almacén del sistema operativo. Verificado compilando contra `reqwest 0.13.4` (la versión ya presente en `Cargo.lock`). `default-features = false` evita arrastrar `charset`, `http2` y `system-proxy`, que no necesitamos para dos endpoints JSON. Si en el futuro hace falta HTTP/2 o proxy del sistema, añadir `"http2"` / `"system-proxy"` explícitamente.

No hace falta `lru` (64 entradas se resuelven con 30 líneas), ni `sha2`/`blake3` (FNV-1a de 64 bits para el nombre de archivo), ni `dirs` (tres `env::var`), ni `urlencoding` (`reqwest::Url::query_pairs_mut` codifica). No hace falta ningún permiso nuevo en `capabilities/default.json`: la petición sale de Rust, no del WebView.

> Verificar con `cargo tree -i reqwest` antes de fijar la versión. Si el árbol ya lo trae con otra feature, unificar para no compilar dos variantes.

### 4.2 Modelos — `src-tauri/src/models/lyrics.rs`

```rust
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
```

Añadir `pub mod lyrics;` a `models/mod.rs`.

**Por qué `time_ms: u64` y no `f64`:** evita acumulación de error y hace la búsqueda binaria exacta. El frontend convierte a segundos solo al hacer seek.

**Por qué `track_id` dentro del doc:** el backend y el frontend pueden descartar respuestas tardías sin coordinación adicional. Es la barrera más barata contra el “me salté de track y apareció la letra anterior”.

### 4.3 Parser LRC — `src-tauri/src/services/lyrics/parser.rs`

Soporta: `[mm:ss]`, `[mm:ss.xx]`, `[mm:ss.xxx]`, `[hh:mm:ss.xx]`, múltiples timestamps por línea, `[offset:]`, metadatos (`ti`, `ar`, `al`, `by`, `length`), LRC enriquecido con `<mm:ss.xx>`, CRLF, BOM UTF-8 y letra plana sin timestamps.

```rust
use crate::models::lyrics::{LyricLine, LyricWord};

pub const MAX_LINES: usize = 2_000;

#[derive(Debug, Default, PartialEq)]
pub struct ParsedLrc {
    pub lines: Vec<LyricLine>,
    pub offset_ms: i32,
    pub synced: bool,
    pub title: Option<String>,
    pub artist: Option<String>,
}

pub fn parse_lrc(raw: &str) -> ParsedLrc {
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let mut parsed = ParsedLrc::default();
    let mut offset: i64 = 0;
    let mut timed: Vec<(u64, LyricLine)> = Vec::new();
    let mut plain: Vec<String> = Vec::new();

    for raw_line in raw.lines().take(MAX_LINES) {
        let line = raw_line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }

        let mut cursor = line;
        let mut times: Vec<u64> = Vec::new();
        let mut metadata_here = false;

        // Consume todos los `[...]` iniciales: timestamps y metadatos.
        while let Some(after_open) = cursor.strip_prefix('[') {
            let Some(close) = after_open.find(']') else { break };
            let inner = &after_open[..close];

            if let Some(ms) = parse_timestamp(inner) {
                times.push(ms);
                cursor = &after_open[close + 1..];
                continue;
            }

            // Solo tratamos como metadato si aún no hemos visto un timestamp.
            // Si no, `[Chorus: ...]` dentro de una línea se perdería.
            if times.is_empty() {
                if let Some((key, value)) = inner.split_once(':') {
                    if key.chars().all(|c| c.is_ascii_alphabetic()) {
                        match key.to_ascii_lowercase().as_str() {
                            "offset" => offset = value.trim().parse().unwrap_or(0),
                            "ti" => parsed.title = Some(value.trim().to_string()),
                            "ar" => parsed.artist = Some(value.trim().to_string()),
                            _ => {}
                        }
                        metadata_here = true;
                        cursor = &after_open[close + 1..];
                        continue;
                    }
                }
            }
            break;
        }

        let text = cursor.trim();
        if times.is_empty() {
            if !metadata_here && !text.is_empty() {
                plain.push(text.to_string());
            }
            continue;
        }
        if text.is_empty() {
            continue;
        }

        // Los tags `<mm:ss.xx>` son absolutos; el primer timestamp de la línea
        // es la referencia para el texto que va antes del primer tag.
        let (display, absolute_words) = parse_words(text, times[0]);
        let base = times[0];

        for &time in &times {
            // Varias timestamps por línea (raro): desplaza las palabras para
            // que cada copia conserve su sincronía relativa.
            let shift = time as i64 - base as i64;
            let words = absolute_words.as_ref().map(|words| {
                words
                    .iter()
                    .map(|word| LyricWord {
                        time_ms: (word.time_ms as i64 + shift).max(0) as u64,
                        end_ms: word
                            .end_ms
                            .map(|end| (end as i64 + shift).max(0) as u64),
                        text: word.text.clone(),
                    })
                    .collect()
            });

            timed.push((
                time,
                LyricLine {
                    time_ms: time,
                    text: display.clone(),
                    words,
                },
            ));
        }
    }

    if !timed.is_empty() {
        for (time, mut line) in timed {
            let adjusted = (time as i64 + offset).max(0) as u64;
            line.time_ms = adjusted;
            if let Some(words) = line.words.as_mut() {
                for word in words {
                    word.time_ms = (word.time_ms as i64 + offset).max(0) as u64;
                    word.end_ms = word
                        .end_ms
                        .map(|end| (end as i64 + offset).max(0) as u64);
                }
            }
            parsed.lines.push(line);
        }
        parsed.lines.sort_by_key(|line| line.time_ms);
        parsed
            .lines
            .dedup_by(|a, b| a.time_ms == b.time_ms && a.text == b.text);
        parsed.synced = true;
    } else if !plain.is_empty() {
        parsed.lines = plain
            .into_iter()
            .map(|text| LyricLine {
                time_ms: 0,
                text,
                words: None,
            })
            .collect();
        parsed.synced = false;
    }

    parsed.offset_ms = offset.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    parsed
}

/// Acepta `mm:ss`, `mm:ss.xx`, `mm:ss,xxx`, `hh:mm:ss.xx`.
fn parse_timestamp(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    let parts: Vec<&str> = raw.split(':').collect();

    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => (0u64, minutes.parse::<u64>().ok()?, *seconds),
        [hours, minutes, seconds] => (
            hours.parse::<u64>().ok()?,
            minutes.parse::<u64>().ok()?,
            *seconds,
        ),
        _ => return None,
    };

    let (whole, fraction) = match seconds.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => match seconds.split_once(',') {
            Some((whole, fraction)) => (whole, fraction),
            None => (seconds, ""),
        },
    };

    let secs = whole.parse::<u64>().ok()?;
    let millis = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<u64>().ok()? * 100,
        2 => fraction.parse::<u64>().ok()? * 10,
        _ => fraction.get(..3)?.parse::<u64>().ok()?,
    };

    Some((hours * 3_600 + minutes * 60 + secs) * 1_000 + millis)
}

/// Devuelve el texto sin etiquetas y las palabras con tiempos absolutos.
/// `line_time` es el timestamp de la línea, usado para el texto que precede
/// al primer tag `<mm:ss.xx>`.
fn parse_words(text: &str, line_time: u64) -> (String, Option<Vec<LyricWord>>) {
    if !text.contains('<') {
        return (text.trim().to_string(), None);
    }

    let mut plain = String::with_capacity(text.len());
    let mut words: Vec<LyricWord> = Vec::new();
    let mut current: u64 = line_time;
    let mut any_tag = false;

    for (index, chunk) in text.split('<').enumerate() {
        if index == 0 {
            plain.push_str(chunk);
            push_word(&mut words, current, chunk);
            continue;
        }

        let Some((tag, body)) = chunk.split_once('>') else {
            plain.push('<');
            plain.push_str(chunk);
            continue;
        };

        let Some(ms) = parse_timestamp(tag) else {
            plain.push('<');
            plain.push_str(chunk);
            continue;
        };

        any_tag = true;
        current = ms;
        plain.push_str(body);
        push_word(&mut words, current, body);
    }

    if !any_tag {
        return (text.trim().to_string(), None);
    }

    for index in 0..words.len().saturating_sub(1) {
        words[index].end_ms = Some(words[index + 1].time_ms);
    }

    (plain.trim().to_string(), Some(words))
}

fn push_word(words: &mut Vec<LyricWord>, time_ms: u64, raw: &str) {
    if raw.trim().is_empty() {
        return;
    }
    words.push(LyricWord {
        time_ms,
        end_ms: None,
        text: raw.to_string(),
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_and_sorts() {
        let parsed = parse_lrc("[00:10.50]Second\n[00:02.00]First\n");
        assert!(parsed.synced);
        assert_eq!(parsed.lines.len(), 2);
        assert_eq!(parsed.lines[0].time_ms, 2_000);
        assert_eq!(parsed.lines[1].text, "Second");
    }

    #[test]
    fn applies_offset_and_clamps_at_zero() {
        let parsed = parse_lrc("[offset:-500]\n[00:00.20]Early\n[00:01.00]Later\n");
        assert_eq!(parsed.offset_ms, -500);
        assert_eq!(parsed.lines[0].time_ms, 0);
        assert_eq!(parsed.lines[1].time_ms, 500);
    }

    #[test]
    fn expands_multiple_timestamps() {
        let parsed = parse_lrc("[00:01.00][00:05.00]Chorus\n");
        assert_eq!(parsed.lines.len(), 2);
        assert_eq!(parsed.lines[1].time_ms, 5_000);
        assert_eq!(parsed.lines[1].text, "Chorus");
    }

    #[test]
    fn parses_hours_and_comma_fraction() {
        let parsed = parse_lrc("[1:02:03,500]Long\n");
        assert_eq!(parsed.lines[0].time_ms, 3_723_500);
    }

    #[test]
    fn parses_word_sync() {
        let parsed = parse_lrc("[00:10.00]<00:10.00>Hello <00:10.50>world\n");
        let words = parsed.lines[0].words.as_ref().expect("words");
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].time_ms, 10_000);
        assert_eq!(words[0].end_ms, Some(10_500));
        assert_eq!(words[1].time_ms, 10_500);
        assert_eq!(parsed.lines[0].text, "Hello world");
    }

    #[test]
    fn plain_lyrics_have_no_timestamps() {
        let parsed = parse_lrc("Line one\nLine two\n");
        assert!(!parsed.synced);
        assert_eq!(parsed.lines.len(), 2);
        assert_eq!(parsed.lines[0].time_ms, 0);
    }

    #[test]
    fn metadata_only_is_empty() {
        let parsed = parse_lrc("[ti:Title]\n[ar:Artist]\n");
        assert!(parsed.lines.is_empty());
    }

    #[test]
    fn bracket_text_after_timestamp_is_kept() {
        let parsed = parse_lrc("[00:01.00][Chorus: loud]\n");
        assert_eq!(parsed.lines[0].text, "[Chorus: loud]");
    }
}
```

**Detalles deliberados:**

- `[Chorus: ...]` al principio de una línea se interpreta como metadato; después de un timestamp, como texto. Es la heurística que evita perder la letra en temas con secciones.
- Las líneas vacías con timestamp se descartan: no aportan texto y romperían el resaltado. Durante un interludio, la última línea queda activa (comportamiento Apple Music).
- `dedup_by` después de ordenar elimina duplicados exactos de tiempo+texto, comunes en LRC generados automáticamente.
- `MAX_LINES = 2000` acota el peor caso; el límite de bytes (512 KB) se aplica antes de llamar al parser.

### 4.4 Caché — `src-tauri/src/services/lyrics/cache.rs`

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::models::lyrics::{LyricsCacheStats, LyricsDoc};

const MEMORY_CAPACITY: usize = 64;
const POSITIVE_TTL_SECS: u64 = 30 * 24 * 3_600;
const NEGATIVE_TTL_SECS: u64 = 24 * 3_600;
const DISK_BUDGET_BYTES: u64 = 50 * 1024 * 1024;
const DISK_MAX_FILES: usize = 5_000;

/// LRU mínimo: 64 entradas, evicción O(n) sobre 64 elementos = irrelevante.
/// Evita la dependencia `lru` y su `hashbrown`.
struct TinyLru<V> {
    map: HashMap<String, (Arc<V>, u64)>,
    tick: u64,
    capacity: usize,
}

impl<V> TinyLru<V> {
    fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::with_capacity(capacity),
            tick: 0,
            capacity: capacity.max(1),
        }
    }

    fn get(&mut self, key: &str) -> Option<Arc<V>> {
        self.tick += 1;
        let (value, stamp) = self.map.get_mut(key)?;
        *stamp = self.tick;
        Some(Arc::clone(value))
    }

    fn put(&mut self, key: String, value: Arc<V>) {
        self.tick += 1;
        if self.map.len() >= self.capacity && !self.map.contains_key(&key) {
            if let Some(oldest) = self
                .map
                .iter()
                .min_by_key(|(_, (_, stamp))| *stamp)
                .map(|(key, _)| key.clone())
            {
                self.map.remove(&oldest);
            }
        }
        self.map.insert(key, (value, self.tick));
    }

    fn len(&self) -> usize {
        self.map.len()
    }
}

#[derive(Serialize, Deserialize)]
struct DiskEntry {
    schema: u8,
    key: String,
    stored_at: u64,
    doc: LyricsDoc,
}

impl DiskEntry {
    fn ttl(&self) -> u64 {
        if self.doc.source == crate::models::lyrics::LyricsSource::None {
            NEGATIVE_TTL_SECS
        } else {
            POSITIVE_TTL_SECS
        }
    }

    fn is_fresh(&self) -> bool {
        now_secs().saturating_sub(self.stored_at) < self.ttl()
    }
}

pub struct LyricsCache {
    memory: Mutex<TinyLru<LyricsDoc>>,
    dir: PathBuf,
}

impl LyricsCache {
    pub fn new() -> Self {
        let dir = cache_root().join("lyrics");
        let _ = std::fs::create_dir_all(&dir);
        Self {
            memory: Mutex::new(TinyLru::new(MEMORY_CAPACITY)),
            dir,
        }
    }

    /// Lee memoria y, si falla, disco. `spawn_blocking` evita bloquear el runtime.
    pub async fn get(&self, key: &str) -> Option<Arc<LyricsDoc>> {
        if let Ok(mut memory) = self.memory.lock() {
            if let Some(hit) = memory.get(key) {
                return Some(hit);
            }
        }

        let path = self.path_for(key);
        let key_owned = key.to_string();
        let entry = tokio::task::spawn_blocking(move || {
            let bytes = std::fs::read(&path).ok()?;
            serde_json::from_slice::<DiskEntry>(&bytes).ok()
        })
        .await
        .ok()??;

        if entry.key != key_owned || !entry.is_fresh() {
            let _ = tokio::fs::remove_file(self.path_for(key)).await;
            return None;
        }

        let doc = Arc::new(entry.doc);
        if let Ok(mut memory) = self.memory.lock() {
            memory.put(key_owned, Arc::clone(&doc));
        }
        Some(doc)
    }

    pub async fn put(&self, key: String, doc: Arc<LyricsDoc>) {
        if let Ok(mut memory) = self.memory.lock() {
            memory.put(key.clone(), Arc::clone(&doc));
        }

        let path = self.path_for(&key);
        let entry = DiskEntry {
            schema: 1,
            key,
            stored_at: now_secs(),
            doc: (*doc).clone(),
        };

        let _ = tokio::task::spawn_blocking(move || {
            let Ok(bytes) = serde_json::to_vec(&entry) else {
                return;
            };
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, &bytes).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        })
        .await;
    }

    pub async fn stats(&self) -> LyricsCacheStats {
        let memory_entries = self.memory.lock().map(|m| m.len()).unwrap_or(0);
        let dir = self.dir.clone();
        let (disk_entries, disk_bytes) = tokio::task::spawn_blocking(move || scan_dir(&dir))
            .await
            .unwrap_or((0, 0));
        LyricsCacheStats {
            memory_entries,
            disk_entries,
            disk_bytes,
        }
    }

    pub async fn clear(&self) -> u64 {
        if let Ok(mut memory) = self.memory.lock() {
            *memory = TinyLru::new(MEMORY_CAPACITY);
        }
        let dir = self.dir.clone();
        tokio::task::spawn_blocking(move || {
            let mut removed = 0u64;
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    if entry.path().extension().and_then(|e| e.to_str()) == Some("json")
                        && std::fs::remove_file(entry.path()).is_ok()
                    {
                        removed += 1;
                    }
                }
            }
            removed
        })
        .await
        .unwrap_or(0)
    }

    /// Borra expirados y aplica el presupuesto. Llamar una vez al arrancar.
    /// Un hilo OS de un solo uso evita depender de que `new()` se llame dentro
    /// de un runtime Tokio (Tauri llama a `setup` fuera de `block_on`).
    pub fn spawn_prune(&self) {
        let dir = self.dir.clone();
        std::thread::spawn(move || prune(&dir));
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{:016x}.json", fnv1a64(key.as_bytes())))
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// FNV-1a 64: estable entre versiones de Rust, 10 líneas, sin dependencias.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn scan_dir(dir: &Path) -> (u64, u64) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    files += 1;
                    bytes += meta.len();
                }
            }
        }
    }
    (files, bytes)
}

fn prune(dir: &Path) {
    let mut entries: Vec<(PathBuf, u64, u64)> = Vec::new();
    let Ok(read_dir) = std::fs::read_dir(dir) else { return };

    for entry in read_dir.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let modified = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        entries.push((path, modified, meta.len()));
    }

    let now = now_secs();
    for (path, modified, _) in &entries {
        if now.saturating_sub(*modified) > POSITIVE_TTL_SECS {
            let _ = std::fs::remove_file(path);
        }
    }

    entries.retain(|(path, _, _)| path.exists());
    entries.sort_by_key(|(_, modified, _)| *modified);

    let mut total: u64 = entries.iter().map(|(_, _, len)| *len).sum();
    let mut count = entries.len();
    for (path, _, len) in entries {
        if total <= DISK_BUDGET_BYTES && count <= DISK_MAX_FILES {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
            count = count.saturating_sub(1);
        }
    }
}

/// `~/.cache/cadence` en Linux, `%LOCALAPPDATA%\Cadence\cache` en Windows,
/// `~/Library/Caches/com.xalejo.cadence` en macOS.
fn cache_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(dir).join("cadence");
    }

    #[cfg(target_os = "windows")]
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(dir).join("Cadence").join("cache");
    }

    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join("Library")
            .join("Caches")
            .join("com.xalejo.cadence");
    }

    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".cache").join("cadence");
    }

    std::env::temp_dir().join("cadence-cache")
}
```

**Por qué no `/tmp`:** `/tmp` se limpia al reiniciar y en Linux multiusuario es un vector de interferencia (audit §2.4). La caché de letras es datos de usuario, no efímeros.

**Por qué FNV y no SHA:** el nombre de archivo solo necesita dispersión, no criptografía. La entrada guarda la clave completa y se compara al leer, así una colisión no devuelve la letra equivocada.

### 4.5 Cliente LRCLIB — `src-tauri/src/services/lyrics/lrclib.rs`

```rust
use std::time::{Duration, Instant};

use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;
use tokio::sync::{Mutex, Semaphore};

use crate::errors::CadenceError;
use crate::models::lyrics::{
    LyricsCandidate, LyricsDoc, LyricsRequest, LyricsSource,
};

use super::parser::{parse_lrc, MAX_LINES};

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
            .filter(|(score, _)| *score >= 70)
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
        let mut url = self.endpoint("api/search")?;
        {
            let mut query = url.query_pairs_mut();
            let title = clean_title(&request.title, request.artist.as_deref());
            match request.artist.as_deref().filter(|a| !a.trim().is_empty()) {
                Some(artist) => {
                    query.append_pair("track_name", &title);
                    query.append_pair("artist_name", &clean_artist(artist));
                }
                None => {
                    query.append_pair("q", &title);
                }
            }
        }

        let response = self.request(url).await?;
        let tracks: Vec<LrclibTrack> = response
            .json()
            .await
            .map_err(|error| CadenceError::Lyrics {
                message: format!("lrclib search parse: {error}"),
            })?;

        Ok(tracks)
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
        // Un fallo de red se propaga: el servicio lo convierte en "reintentar",
        // no en "no hay letra" (eso solo lo decide un 404).
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

    /// Una petición cada 200 ms como máximo y 2 en vuelo. Cortesía con la API.
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

fn normalize(raw: &str) -> String {
    raw.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Quita ruido de títulos de YouTube: "(Official Video)", "[HD]", " - Topic"…
/// Solo elimina grupos entre paréntesis cuyo contenido sea ruido conocido.
pub fn clean_title(raw: &str, artist: Option<&str>) -> String {
    const NOISE: &[&str] = &[
        "official", "video", "audio", "lyric", "lyrics", "visualizer", "hd", "hq", "4k",
        "remaster", "remastered", "m/v", "mv", "explicit", "clean", "topic",
    ];

    let mut output = String::with_capacity(raw.len());
    let mut depth = 0usize;
    let mut buffer = String::new();

    for character in raw.chars() {
        match character {
            '(' | '[' => {
                depth += 1;
                buffer.clear();
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let lower = buffer.to_lowercase();
                    if !NOISE.iter().any(|noise| lower.contains(noise)) {
                        output.push(' ');
                        output.push_str(&buffer);
                        output.push(' ');
                    }
                }
                buffer.clear();
            }
            _ if depth > 0 => buffer.push(character),
            _ => output.push(character),
        }
    }

    let mut cleaned = output
        .replace(" - Topic", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if let Some(artist) = artist {
        let prefix = format!("{} - ", artist.trim());
        // `get` evita panic si el corte no cae en un límite de carácter.
        if let (Some(head), Some(tail)) = (cleaned.get(..prefix.len()), cleaned.get(prefix.len()..)) {
            if head.eq_ignore_ascii_case(&prefix) {
                cleaned = tail.trim().to_string();
            }
        }
    }

    cleaned
}

pub fn clean_artist(raw: &str) -> String {
    raw.replace(" - Topic", "")
        .replace("VEVO", "")
        .trim()
        .to_string()
}

fn score(candidate: &LrclibTrack, request: &LyricsRequest) -> i32 {
    let wanted_title = normalize(&clean_title(&request.title, request.artist.as_deref()));
    let candidate_title = normalize(&candidate.track_name);

    let mut score = 0;
    if !wanted_title.is_empty() && candidate_title == wanted_title {
        score += 100;
    } else if !wanted_title.is_empty()
        && (candidate_title.contains(&wanted_title) || wanted_title.contains(&candidate_title))
    {
        score += 45;
    }

    if let Some(artist) = request.artist.as_deref() {
        let wanted_artist = normalize(&clean_artist(artist));
        let candidate_artist = normalize(&candidate.artist_name);
        if candidate_artist == wanted_artist {
            score += 60;
        } else if candidate_artist.contains(&wanted_artist)
            || wanted_artist.contains(&candidate_artist)
        {
            score += 25;
        }
    }

    if request.duration_seconds >= 1.0 && candidate.duration >= 1.0 {
        let difference = (candidate.duration - request.duration_seconds).abs();
        score -= (difference.min(30.0) as i32) * 5;
        if difference <= 2.0 {
            score += 30;
        }
    }

    if candidate.synced_lyrics.is_some() {
        score += 15;
    }
    if candidate.instrumental {
        score -= 5;
    }

    score
}
```

**Notas de robustez:**

- `send()` solo devuelve `Ok(None)` ante un 404. Un timeout o un 5xx se propaga como error y la UI ofrece reintentar; nunca se cachea como “sin letra”.
- `get_exact` exige artista. Sin artista (típico en archivos locales) se va directo a `search`.
- `clean_title` solo elimina paréntesis que contengan ruido conocido; “(Reprise)” o “(Live)” se conservan porque cambian la versión.
- El `User-Agent` es obligatorio en la práctica: sin él, Cloudflare puede devolver 403 (comprobado en `/docs`). Incluye contacto para que LRCLIB pueda identificar el cliente.
- No se sigue ningún redirect fuera de `lrclib.net`; `base` está fijado y el path se construye con `Url::join`.

### 4.6 Servicio y single-flight — `src-tauri/src/services/lyrics/mod.rs`

```rust
pub mod cache;
pub mod id3;
pub mod lrclib;
pub mod parser;

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
use lrclib::{clean_artist, clean_title, LrclibClient};
use parser::parse_lrc;

type LyricsOutcome = Result<Arc<LyricsDoc>, String>;

pub struct LyricsService {
    cache: LyricsCache,
    lrclib: LrclibClient,
    in_flight: Mutex<HashMap<String, Arc<OnceCell<LyricsOutcome>>>>,
    enabled: AtomicBool,
    events: broadcast::Sender<LyricsEvent>,
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

            // 2. Letra embebida en ID3 (USLT/SYLT). Ver §4.7.
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
        let doc = match self.lrclib.get_exact(request).await? {
            Some(doc) => doc,
            None => match self.lrclib.search_best(request).await? {
                Some(doc) => doc,
                None => LyricsDoc::empty(&request.track_id),
            },
        };

        let doc = Arc::new(doc);
        self.cache.put(key, Arc::clone(&doc)).await;
        Ok(doc)
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

/// Clave estable y normalizada. Incluye duración redondeada para distinguir
/// versiones de estudio / live / extended.
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

/// UTF-8 con BOM, UTF-16 LE/BE con BOM, o UTF-8 lossy. Límite 512 KB.
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
```

Añadir a `services/mod.rs`:

```rust
pub mod lyrics;
```

**El detalle que evita duplicados:** `OnceCell` se crea por clave y se elimina al terminar. Si dos componentes piden la misma letra a la vez, el segundo espera la misma inicialización; si llega después, la caché ya está caliente. No hay `Mutex` retenido durante I/O.

### 4.7 Local: sidecar `.lrc` e ID3

**Sidecar (v1, recomendado).** Es el 90 % del valor local: el usuario puede editar la letra con cualquier editor y Cadence la respeta. Se busca `<audio>.lrc` y `<audio>.LRC` junto al archivo.

**ID3 (v1.5).** Leer `USLT` (letra sin sincronizar) y `SYLT` (sincronizada) a mano es posible pero frágil: ID3v2.3 y v2.4 cambian el tamaño de frame (entero vs. syncsafe), hay *unsynchronisation*, extended headers y codificaciones UTF-16 con BOM. Para no meter 200 líneas de parser binario en v1:

```rust
// src-tauri/src/services/lyrics/id3.rs
pub async fn read_embedded(path: &Path) -> Option<String> {
    // v1: no-op. Devuelve None y deja que LRCLIB haga el trabajo.
    // v1.5: `lofty` (una sola dependencia, ~400 KB) o parser propio de USLT/SYLT.
    let _ = path;
    None
}
```

**Decisión:** si el soporte de biblioteca local (audit §3.1) entra en el roadmap, añadir `lofty` en ese momento y conectar `read_embedded`. No añadirlo ahora solo por letras: el sidecar cubre el caso real y no engorda el binario.

### 4.8 Integración con el orquestador

`PlayerOrchestrator` gana una dependencia explícita (no un global):

```rust
pub struct PlayerOrchestrator {
    // …
    lyrics: Arc<LyricsService>,
}

impl PlayerOrchestratorBuilder {
    pub fn with_lyrics(mut self, lyrics: Arc<LyricsService>) -> Self {
        self.lyrics = Some(lyrics);
        self
    }
}
```

En `play()`, después de fijar `current_track` y antes de `loadfile`:

```rust
// No await: la letra no puede retrasar el arranque del audio.
self.lyrics.request(LyricsRequest::from_track(&track));
```

`LyricsRequest::from_track` (en `services/lyrics/mod.rs`, junto a `use lrclib::clean_title;`):

```rust
impl LyricsRequest {
    pub fn from_track(track: &crate::models::search_result::SearchResult) -> Self {
        let path = std::path::Path::new(&track.id);
        let is_local = path.is_absolute() && path.is_file();

        let (title, artist) = match track.artist.as_deref() {
            Some(artist) if artist != "Archivo Local" => {
                (clean_title(&track.title, Some(artist)), Some(artist.to_string()))
            }
            _ => split_artist_title(&track.title),
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

/// "Daft Punk - Get Lucky" → ("Get Lucky", Some("Daft Punk")).
pub(crate) fn split_artist_title(raw: &str) -> (String, Option<String>) {
    match raw.split_once(" - ") {
        Some((artist, title)) => (title.trim().to_string(), Some(artist.trim().to_string())),
        None => (raw.trim().to_string(), None),
    }
}
```

`clean_title` vive en `services::lyrics::lrclib`; para no crear una dependencia invertida (`models` → `services`), moverlo a `models/lyrics.rs` o a `services/lyrics/text.rs` y reexportarlo. **Recomendado:** `models/lyrics.rs` solo structs; `services/lyrics/text.rs` con `clean_title`, `clean_artist`, `split_artist_title`, `normalize`. `LyricsRequest::from_track` se mueve a `services/lyrics/mod.rs` como constructor.

**Por qué no emitir `LyricsReady` en `OrchestratorEvent`:** el canal del orquestador es de 64 y transporta `Progress` a 2 Hz. Un doc de 20 KB ahí puede provocar `Lagged` bajo carga. El canal propio de 16 es suficiente y aísla el fallo.

### 4.9 Comandos Tauri — `src-tauri/src/commands/lyrics.rs`

```rust
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
```

`commands/mod.rs`: `pub mod lyrics;`.

`lib.rs`:

```rust
let lyrics = Arc::new(services::lyrics::LyricsService::new());
app.manage(Arc::clone(&lyrics));

// Puente del canal propio a Tauri.
let mut lyrics_rx = lyrics.subscribe();
let lyrics_handle = app.handle().clone();
tauri::async_runtime::spawn(async move {
    use tauri::Emitter;
    loop {
        match lyrics_rx.recv().await {
            Ok(event) => {
                let _ = lyrics_handle.emit("lyrics-event", event);
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                tracing::warn!(skipped, "lyrics event listener lagged");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
});

let orchestrator = tauri::async_runtime::block_on(
    PlayerOrchestratorBuilder::new(socket_path.clone())
        .with_lyrics(Arc::clone(&lyrics))
        .build(),
)?;
```

Y en `invoke_handler`:

```rust
commands::lyrics::get_lyrics,
commands::lyrics::search_lyrics,
commands::lyrics::get_lyrics_by_id,
commands::lyrics::lyrics_cache_stats,
commands::lyrics::clear_lyrics_cache,
commands::lyrics::set_lyrics_enabled,
```

**No añadir `player_seek`:** ya existe `seek` (`commands/playback.rs:108-114`) y `src/lib/tauri.ts:98-100` lo expone como `seek(positionSeconds)`. Duplicar el nombre crea dos contratos para la misma acción. El click de una línea llama a `seek(line.time_ms / 1000)`.

### 4.10 Errores

`errors.rs`:

```rust
#[error("lyrics error: {message}")]
Lyrics { message: String },
```

Actualizar los dos `match` exhaustivos:

```rust
// user_message()
CadenceError::Lyrics { .. } => "No pudimos cargar la letra de este tema.",

// From<CadenceError> for FrontendError
CadenceError::Lyrics { message } => FrontendError::Unknown { message },
```

Si no se actualizan, el compilador falla — es una red de seguridad, no un olvido silencioso.

---

## 5. Frontend React

### 5.1 Tipos y API — `src/lib/tauri.ts`

```ts
export type LyricsSource = "none" | "lrclib" | "local_lrc" | "id3";

export interface LyricWord {
  time_ms: number;
  end_ms?: number;
  text: string;
}

export interface LyricLine {
  time_ms: number;
  text: string;
  words?: LyricWord[];
}

export interface LyricsDoc {
  track_id: string;
  source: LyricsSource;
  synced: boolean;
  instrumental: boolean;
  offset_ms: number;
  lines: LyricLine[];
  title?: string;
  artist?: string;
}

export interface LyricsRequest {
  track_id: string;
  title: string;
  artist?: string | null;
  album?: string | null;
  duration_seconds?: number;
  local_path?: string | null;
  force?: boolean;
}

export interface LyricsCandidate {
  id: number;
  title: string;
  artist: string;
  album: string | null;
  duration_seconds: number;
  has_synced: boolean;
  instrumental: boolean;
}

export type LyricsEvent =
  | { type: "Ready"; data: LyricsDoc }
  | { type: "Unavailable"; data: { track_id: string; reason: string } };

export function getLyrics(request: LyricsRequest): Promise<LyricsDoc> {
  return invoke("get_lyrics", { request });
}

export function searchLyrics(request: LyricsRequest): Promise<LyricsCandidate[]> {
  return invoke("search_lyrics", { request });
}

export function getLyricsById(id: number, trackId: string): Promise<LyricsDoc> {
  return invoke("get_lyrics_by_id", { id, trackId });
}

export function setLyricsEnabled(enabled: boolean): Promise<void> {
  return invoke("set_lyrics_enabled", { enabled });
}

export function listenLyricsEvents(
  callback: (event: LyricsEvent) => void,
): Promise<UnlistenFn> {
  return listen<LyricsEvent>("lyrics-event", (event) => callback(event.payload));
}
```

**Atención al naming de argumentos:** Tauri convierte `snake_case` de Rust a `camelCase` en JS solo si el comando usa `#[tauri::command(rename_all = "camelCase")]`; en este repo los comandos existentes usan el nombre tal cual (`positionSeconds` se declara así en Rust). Para evitar ambigüedad, declarar el parámetro Rust como `track_id` y enviarlo como `trackId` funciona porque Tauri v2 aplica camelCase por defecto a los argumentos de comando; el objeto `request` interior conserva `snake_case` porque es un struct serde. Verificar con una prueba manual antes de dar por hecho el contrato.

### 5.2 Reloj — `src/components/lyrics/useLyricClock.ts`

Objetivo: **cero `setState` por frame**. El hook llama `onFrame(ms)` a 60 fps; el consumidor decide si eso cambia la línea activa o solo actualiza una CSS variable.

```ts
import { useEffect, useRef } from "react";

export interface LyricClockOptions {
  positionSeconds: number;
  playing: boolean;
  speed: number;
  durationSeconds: number;
  offsetMs: number;
  onFrame: (timeMs: number) => void;
}

export function useLyricClock({
  positionSeconds,
  playing,
  speed,
  durationSeconds,
  offsetMs,
  onFrame,
}: LyricClockOptions): void {
  const anchor = useRef({ posMs: positionSeconds * 1000, at: performance.now() });
  const positionRef = useRef(positionSeconds);
  const speedRef = useRef(speed);
  const durationRef = useRef(durationSeconds);
  const offsetRef = useRef(offsetMs);
  const onFrameRef = useRef(onFrame);
  const frameRef = useRef(0);

  positionRef.current = positionSeconds;
  speedRef.current = speed;
  durationRef.current = durationSeconds;
  offsetRef.current = offsetMs;
  onFrameRef.current = onFrame;

  // Cada muestra autoritativa (2 Hz) re-ancla el reloj. El error entre muestras
  // es < 25 ms y el ojo no lo percibe; un seek grande se corrige en el acto.
  useEffect(() => {
    anchor.current = { posMs: positionSeconds * 1000, at: performance.now() };
  }, [positionSeconds, playing, speed]);

  // Pausa: un solo frame, sin rAF.
  useEffect(() => {
    if (!playing) {
      onFrameRef.current(positionSeconds * 1000 + offsetMs);
    }
  }, [playing, positionSeconds, offsetMs]);

  useEffect(() => {
    if (!playing) return;

    let alive = true;
    const tick = (now: number) => {
      if (!alive) return;
      const current = anchor.current;
      // mpv reporta tiempo de media; a 0.85x avanza 0.85x. Multiplicar por
      // `speed` es obligatorio o la letra se adelanta 9 s por minuto.
      const mediaMs = current.posMs + (now - current.at) * speedRef.current;
      const clamped = Math.min(mediaMs, durationRef.current * 1000);
      onFrameRef.current(clamped + offsetRef.current);
      frameRef.current = requestAnimationFrame(tick);
    };

    frameRef.current = requestAnimationFrame(tick);
    return () => {
      alive = false;
      cancelAnimationFrame(frameRef.current);
    };
  }, [playing]);

  // Al volver de background, el `now` del rAF ha saltado. Re-anclar evita
  // un salto visual antes de que llegue el siguiente `Progress`.
  useEffect(() => {
    const onVisibility = () => {
      if (!document.hidden) {
        anchor.current = { posMs: positionRef.current * 1000, at: performance.now() };
      }
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => document.removeEventListener("visibilitychange", onVisibility);
  }, []);
}
```

**Por qué no usar `positionSeconds` directamente en cada render:** porque solo llega a 2 Hz y la línea cambiaría a saltos. La extrapolación es lo que da la sensación Apple Music.

**Deriva:** si `Progress` llega con retraso, el ancla se corrige en el siguiente evento. No se hace “smoothing” del error porque un seek del usuario debe ser inmediato; el umbral de 150 ms que se consideró inicialmente añadía latencia perceptible en seeks pequeños.

### 5.3 Auto-scroll — `src/components/lyrics/useLyricsScroll.ts`

```ts
import { useCallback, useEffect, useRef } from "react";

const REDUCED_MOTION =
  typeof window !== "undefined" &&
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

export function useLyricsScroll(
  activeIndex: number,
  lineCount: number,
  enabled: boolean,
) {
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const trackRef = useRef<HTMLDivElement | null>(null);
  const centers = useRef<Float32Array>(new Float32Array(0));
  const translate = useRef(0);
  const activeRef = useRef(activeIndex);
  const manualUntil = useRef(0);
  const recenterTimer = useRef(0);

  activeRef.current = activeIndex;

  // Se mide una vez por cambio de letra/resize, NUNCA dentro del rAF.
  const measure = useCallback(() => {
    const track = trackRef.current;
    if (!track) return;
    const nodes = track.querySelectorAll<HTMLElement>("[data-lyric-line]");
    const next = new Float32Array(nodes.length);
    for (let index = 0; index < nodes.length; index += 1) {
      const node = nodes[index];
      next[index] = node.offsetTop + node.offsetHeight / 2;
    }
    centers.current = next;
  }, []);

  const moveTo = useCallback((y: number, animate: boolean) => {
    const track = trackRef.current;
    if (!track) return;
    track.style.transition =
      animate && !REDUCED_MOTION
        ? "transform 620ms cubic-bezier(.22,.61,.36,1)"
        : "none";
    track.style.transform = `translate3d(0, ${y}px, 0)`;
    translate.current = y;
  }, []);

  const recenter = useCallback(() => {
    const viewport = viewportRef.current;
    const index = activeRef.current;
    if (!viewport || index < 0 || index >= centers.current.length) return;
    const center = centers.current[index];
    moveTo(viewport.clientHeight / 2 - center, true);
  }, [moveTo]);

  useEffect(() => {
    measure();
    const track = trackRef.current;
    if (!track) return;
    const observer = new ResizeObserver(measure);
    observer.observe(track);
    void document.fonts?.ready.then(measure);
    return () => observer.disconnect();
  }, [measure, lineCount]);

  useEffect(() => {
    if (!enabled || activeIndex < 0) return;
    if (performance.now() < manualUntil.current) return;
    recenter();
  }, [activeIndex, enabled, recenter]);

  // Navegación manual: 3 s de gracia antes de devolver el control a la canción.
  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;

    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      manualUntil.current = performance.now() + 3000;
      window.clearTimeout(recenterTimer.current);
      recenterTimer.current = window.setTimeout(() => {
        manualUntil.current = 0;
        recenter();
      }, 3000);

      const track = trackRef.current;
      if (!track) return;
      const min = -(track.scrollHeight - viewport.clientHeight);
      const next = Math.min(0, Math.max(min, translate.current - event.deltaY));
      moveTo(next, false);
    };

    viewport.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      viewport.removeEventListener("wheel", onWheel);
      window.clearTimeout(recenterTimer.current);
    };
  }, [moveTo, recenter]);

  return { viewportRef, trackRef };
}
```

**Regla de oro:** no se anima `font-size` ni `line-height` de la línea activa. Eso cambia la altura, invalida `centers` y provoca reflow de todas las líneas siguientes. El énfasis se hace con `transform: scale()` y `text-shadow`, que no afectan al layout. Este es el error más común en implementaciones “bonitas” de letras.

### 5.4 Componentes — `src/components/lyrics/LyricsPanel.tsx`

```tsx
import React, { useCallback, useMemo, useRef, useState } from "react";
import { Minus, Plus, RefreshCw, Search, X } from "lucide-react";

import type { LyricLine as LyricLineModel, LyricsDoc, PlaybackStatus } from "@/lib/tauri";
import { findActiveLine, formatTimestamp } from "@/lib/lyrics";
import { useAlbumPalette } from "@/lib/palette";
import { useLyricClock } from "./useLyricClock";
import { useLyricsScroll } from "./useLyricsScroll";

interface LyricsPanelProps {
  thumbnailUrl?: string;
  lyrics: LyricsDoc | null;
  status: PlaybackStatus;
  positionSeconds: number;
  durationSeconds: number;
  speed: number;
  offsetMs: number;
  onSeek: (seconds: number) => void;
  onOffsetChange: (offsetMs: number) => void;
  onClose: () => void;
  onManualSearch: () => void;
  onReload: () => void;
}

export const LyricsPanel = React.memo(function LyricsPanel({
  thumbnailUrl,
  lyrics,
  status,
  positionSeconds,
  durationSeconds,
  speed,
  offsetMs,
  onSeek,
  onOffsetChange,
  onClose,
  onManualSearch,
  onReload,
}: LyricsPanelProps) {
  const lines = lyrics?.lines ?? [];
  const [activeIndex, setActiveIndex] = useState(-1);
  const activeRef = useRef(-1);
  const lineNodes = useRef(new Map<number, HTMLButtonElement>());
  const linesRef = useRef<LyricLineModel[]>(lines);
  const palette = useAlbumPalette(thumbnailUrl);

  linesRef.current = lines;

  const registerLine = useCallback((index: number, node: HTMLButtonElement | null) => {
    if (node) lineNodes.current.set(index, node);
    else lineNodes.current.delete(index);
  }, []);

  // 60 fps aquí, pero React solo se entera cuando cambia la línea.
  const onFrame = useCallback((timeMs: number) => {
    const currentLines = linesRef.current;
    if (currentLines.length === 0) return;

    const index = findActiveLine(currentLines, timeMs);
    if (index !== activeRef.current) {
      activeRef.current = index;
      setActiveIndex(index);
    }

    const node = lineNodes.current.get(index);
    const line = currentLines[index];
    if (!node || !line) return;

    const end =
      line.words?.[line.words.length - 1]?.end_ms ??
      currentLines[index + 1]?.time_ms ??
      line.time_ms + 6000;
    const span = Math.max(1, end - line.time_ms);
    const progress = Math.min(1, Math.max(0, (timeMs - line.time_ms) / span));
    node.style.setProperty("--p", progress.toFixed(3));
  }, []);

  useLyricClock({
    positionSeconds,
    playing: status === "Playing",
    speed,
    durationSeconds,
    offsetMs,
    onFrame,
  });

  const { viewportRef, trackRef } = useLyricsScroll(
    activeIndex,
    lines.length,
    status === "Playing" && lyrics?.synced === true,
  );

  const seekLine = useCallback(
    (timeMs: number) => onSeek((timeMs + offsetMs) / 1000),
    [offsetMs, onSeek],
  );

  const emptyState = useMemo(() => {
    if (!lyrics) return "loading" as const;
    if (lyrics.instrumental) return "instrumental" as const;
    if (lyrics.lines.length === 0) return "missing" as const;
    if (!lyrics.synced) return "plain" as const;
    return null;
  }, [lyrics]);

  return (
    <section
      className="lyrics-panel"
      aria-label="Letras"
      style={
        {
          "--aura-1": palette[0],
          "--aura-2": palette[1],
        } as unknown as React.CSSProperties
      }
    >
      <div className="lyrics-aura" aria-hidden="true" />

      <header className="lyrics-header">
        <div className="min-w-0">
          <p className="truncate text-xs font-semibold text-white/90">
            {lyrics?.title ?? "Letras"}
          </p>
          <p className="truncate text-[10px] text-white/45">
            {lyrics?.artist ?? ""}
            {lyrics?.source === "lrclib" ? " · LRCLIB" : ""}
            {lyrics?.source === "local_lrc" ? " · LRC local" : ""}
          </p>
        </div>
        <div className="flex items-center gap-1">
          <button
            type="button"
            className="lyrics-icon-button"
            onClick={() => onOffsetChange(offsetMs - 250)}
            aria-label="Adelantar letra 250 ms"
            title="-0.25 s"
          >
            <Minus className="size-3.5" />
          </button>
          <span className="w-10 text-center text-[10px] tabular-nums text-white/50">
            {offsetMs > 0 ? "+" : ""}
            {(offsetMs / 1000).toFixed(2)}s
          </span>
          <button
            type="button"
            className="lyrics-icon-button"
            onClick={() => onOffsetChange(offsetMs + 250)}
            aria-label="Retrasar letra 250 ms"
            title="+0.25 s"
          >
            <Plus className="size-3.5" />
          </button>
          <button
            type="button"
            className="lyrics-icon-button"
            onClick={onReload}
            aria-label="Recargar letra"
            title="Recargar"
          >
            <RefreshCw className="size-3.5" />
          </button>
          <button
            type="button"
            className="lyrics-icon-button"
            onClick={onClose}
            aria-label="Cerrar letras"
          >
            <X className="size-4" />
          </button>
        </div>
      </header>

      {emptyState ? (
        <div className="lyrics-empty" role="status">
          {emptyState === "loading" && (
            <>
              <span className="lyrics-skeleton" />
              <span className="lyrics-skeleton w-2/3" />
              <span className="lyrics-skeleton w-1/2" />
            </>
          )}
          {emptyState === "instrumental" && (
            <>
              <p className="text-sm font-medium text-white/80">Instrumental</p>
              <p className="text-xs text-white/50">
                Este tema no tiene letra. Disfruta del silencio (o del bajo).
              </p>
            </>
          )}
          {emptyState === "missing" && (
            <>
              <p className="text-sm font-medium text-white/80">Sin letra todavía</p>
              <p className="text-xs text-white/50">
                LRCLIB no tiene este tema. Puedes buscar otra versión.
              </p>
              <button type="button" className="lyrics-secondary-button" onClick={onManualSearch}>
                <Search className="size-3.5" /> Buscar letra
              </button>
            </>
          )}
          {emptyState === "plain" && (
            <p className="text-xs text-white/50">
              Letra sin sincronizar. Se muestra completa, sin resaltado.
            </p>
          )}
        </div>
      ) : (
        <div ref={viewportRef} className="lyrics-viewport">
          <div ref={trackRef} className="lyrics-track">
            {lines.map((line, index) => (
              <LyricLine
                key={`${line.time_ms}-${index}`}
                line={line}
                index={index}
                active={index === activeIndex}
                distance={Math.abs(index - activeIndex)}
                onSeek={seekLine}
                register={registerLine}
              />
            ))}
          </div>
        </div>
      )}

      {lyrics?.synced && (
        <footer className="lyrics-footer">
          <span className="tabular-nums">{formatTimestamp(Math.max(0, positionSeconds * 1000 + offsetMs))}</span>
          <span className="text-white/30">toca una línea para saltar</span>
        </footer>
      )}
    </section>
  );
});

interface LyricLineProps {
  line: LyricLineModel;
  index: number;
  active: boolean;
  distance: number;
  onSeek: (timeMs: number) => void;
  register: (index: number, node: HTMLButtonElement | null) => void;
}

const LyricLine = React.memo(function LyricLine({
  line,
  index,
  active,
  distance,
  onSeek,
  register,
}: LyricLineProps) {
  const ref = useCallback(
    (node: HTMLButtonElement | null) => register(index, node),
    [index, register],
  );
  const handleClick = useCallback(() => onSeek(line.time_ms), [line.time_ms, onSeek]);

  return (
    <button
      ref={ref}
      type="button"
      data-lyric-line
      data-active={active ? "true" : undefined}
      data-distance={Math.min(distance, 4)}
      aria-current={active ? "true" : undefined}
      className="lyric-line"
      onClick={handleClick}
      title={`Ir a ${formatTimestamp(line.time_ms)}`}
    >
      <span className="lyric-text-wrap">
        <span className="lyric-text">{line.text}</span>
        {active && (
          <span className="lyric-text lyric-fill" aria-hidden="true">
            {line.text}
          </span>
        )}
      </span>
    </button>
  );
});
```

`src/lib/lyrics.ts`:

```ts
import type { LyricLine } from "@/lib/tauri";

/** Última línea con `time_ms <= timeMs`. -1 si aún no empieza. */
export function findActiveLine(lines: LyricLine[], timeMs: number): number {
  let low = 0;
  let high = lines.length - 1;
  let answer = -1;

  while (low <= high) {
    const middle = (low + high) >> 1;
    if (lines[middle].time_ms <= timeMs) {
      answer = middle;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return answer;
}

export function formatTimestamp(timeMs: number): string {
  const totalSeconds = Math.max(0, Math.floor(timeMs / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}
```

Test (`src/lib/lyrics.test.ts`):

```ts
import { describe, expect, it } from "vitest";
import { findActiveLine } from "./lyrics";

const lines = [1000, 3000, 7000].map((time_ms) => ({ time_ms, text: String(time_ms) }));

describe("findActiveLine", () => {
  it("returns -1 before the first line", () => {
    expect(findActiveLine(lines, 0)).toBe(-1);
  });
  it("finds the exact line and the previous one between lines", () => {
    expect(findActiveLine(lines, 1000)).toBe(0);
    expect(findActiveLine(lines, 2999)).toBe(0);
    expect(findActiveLine(lines, 3000)).toBe(1);
  });
  it("returns the last line after the end", () => {
    expect(findActiveLine(lines, 999999)).toBe(2);
  });
  it("handles an empty list", () => {
    expect(findActiveLine([], 5000)).toBe(-1);
  });
});
```

### 5.5 CSS — `src/styles/lyrics.css`

```css
.lyrics-panel {
  position: absolute;
  inset: 0;
  z-index: 30;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border-radius: 1.5rem;
  background: rgba(8, 11, 20, 0.82);
  contain: layout paint style;
}

/* Una sola capa de aura, estática. No añadir backdrop-filter aquí. */
.lyrics-aura {
  position: absolute;
  inset: -25%;
  z-index: -1;
  pointer-events: none;
  opacity: 0.3;
  transform: translateZ(0);
  background:
    radial-gradient(55% 45% at 30% 35%, var(--aura-1, #2dd4bf) 0%, transparent 70%),
    radial-gradient(50% 45% at 70% 62%, var(--aura-2, #7c3aed) 0%, transparent 70%);
  filter: blur(48px);
  transition: background 800ms ease;
}

.lyrics-header,
.lyrics-footer {
  position: relative;
  z-index: 1;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
  padding: 0.75rem 0.9rem;
}

.lyrics-footer {
  font-size: 10px;
  color: rgba(255, 255, 255, 0.4);
}

.lyrics-icon-button {
  display: grid;
  place-items: center;
  width: 1.75rem;
  height: 1.75rem;
  border-radius: 9999px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.04);
  color: rgba(255, 255, 255, 0.65);
}
.lyrics-icon-button:hover {
  background: rgba(255, 255, 255, 0.1);
  color: #fff;
}

.lyrics-viewport {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow: hidden;
  mask-image: linear-gradient(to bottom, transparent 0, #000 14%, #000 86%, transparent 100%);
  -webkit-mask-image: linear-gradient(to bottom, transparent 0, #000 14%, #000 86%, transparent 100%);
  contain: layout paint;
}

.lyrics-track {
  will-change: transform;
  transform: translate3d(0, 0, 0);
  padding-block: 1.25rem;
}

.lyric-line {
  display: block;
  width: 100%;
  padding: 0.35rem 1rem;
  border: 0;
  background: none;
  text-align: left;
  color: inherit;
  cursor: pointer;
  opacity: 0.34;
  filter: blur(0.4px);
  transform: scale(0.985);
  transform-origin: left center;
  transition:
    opacity 380ms ease,
    transform 380ms ease,
    filter 380ms ease;
}

/* El énfasis NO toca font-size/line-height: eso invalidaría los offsets. */
.lyric-line[data-distance="0"] {
  opacity: 1;
  filter: none;
  transform: scale(1.045);
}
.lyric-line[data-distance="1"] {
  opacity: 0.62;
  filter: blur(0.6px);
}
.lyric-line[data-distance="2"] {
  opacity: 0.46;
  filter: blur(1.1px);
}
.lyric-line[data-distance="3"] {
  opacity: 0.34;
  filter: blur(1.6px);
}
.lyric-line[data-distance="4"] {
  opacity: 0.24;
  filter: blur(2.2px);
}

.lyric-text-wrap {
  position: relative;
  display: block;
}

.lyric-text {
  display: block;
  font-size: 1.08rem;
  line-height: 1.34;
  font-weight: 620;
  letter-spacing: -0.01em;
  text-wrap: pretty;
}

.lyric-line[data-distance="0"] .lyric-text {
  text-shadow:
    0 0 26px color-mix(in srgb, var(--aura-1, #2dd4bf) 55%, transparent),
    0 0 8px rgba(255, 255, 255, 0.22);
}

/* Wipe de karaoke: se actualiza con una CSS var por frame, sin re-render. */
.lyric-fill {
  position: absolute;
  inset: 0;
  color: #fff;
  pointer-events: none;
  clip-path: inset(0 calc(100% - var(--p, 0) * 100%) 0 0);
}

.lyrics-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 0.6rem;
  padding: 1.5rem;
  text-align: center;
}

.lyrics-skeleton {
  display: block;
  width: 80%;
  height: 0.9rem;
  border-radius: 9999px;
  background: rgba(255, 255, 255, 0.07);
  animation: lyrics-pulse 1.6s ease-in-out infinite;
}

.lyrics-secondary-button {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  margin-top: 0.5rem;
  padding: 0.4rem 0.75rem;
  border-radius: 9999px;
  border: 1px solid rgba(255, 255, 255, 0.12);
  background: rgba(255, 255, 255, 0.06);
  font-size: 11px;
  color: rgba(255, 255, 255, 0.8);
}

@keyframes lyrics-pulse {
  0%, 100% { opacity: 0.35; }
  50% { opacity: 0.8; }
}

@media (prefers-reduced-motion: reduce) {
  .lyrics-track { transition: none !important; }
  .lyric-line { transition: none; }
  .lyrics-skeleton { animation: none; }
  .lyrics-aura { filter: none; opacity: 0.22; }
}
```

Importar en `src/index.css` después de `liquid-glass.css`:

```css
@import "@/styles/lyrics.css";
```

**Por qué el blur no se anima por frame:** `filter` no es una propiedad de compositor barata; animarla obliga a repintar el texto. Se transiciona solo al cambiar de línea (380 ms), no en cada frame. El único cambio por frame es `--p` en **una** línea activa (clip-path), que no reflowea.

### 5.6 Paleta y aura — `src/lib/palette.ts`

Se mejora `album-color.ts` sin romper su API. El promedio simple da colores grises; una paleta ponderada por saturación da el “aura” de Apple Music.

```ts
const cache = new Map<string, [string, string]>();
const MAX_CACHE = 64;

export async function extractPalette(url: string): Promise<[string, string]> {
  const cached = cache.get(url);
  if (cached) return cached;

  const palette = await new Promise<[string, string]>((resolve) => {
    const image = new Image();
    image.crossOrigin = "anonymous";
    image.onload = () => {
      try {
        const size = 24;
        const canvas = document.createElement("canvas");
        canvas.width = size;
        canvas.height = size;
        const context = canvas.getContext("2d", { willReadFrequently: true });
        if (!context) return resolve(fallback(url));
        context.drawImage(image, 0, 0, size, size);
        const data = context.getImageData(0, 0, size, size).data;

        const buckets = new Map<number, { count: number; r: number; g: number; b: number; saturation: number }>();
        for (let index = 0; index < data.length; index += 4) {
          const r = data[index];
          const g = data[index + 1];
          const b = data[index + 2];
          const alpha = data[index + 3];
          if (alpha < 128) continue;
          const max = Math.max(r, g, b);
          if (max < 24) continue; // casi negro
          const saturation = max === 0 ? 0 : (max - Math.min(r, g, b)) / max;
          const key = ((r >> 4) << 8) | ((g >> 4) << 4) | (b >> 4);
          const bucket = buckets.get(key) ?? { count: 0, r: 0, g: 0, b: 0, saturation: 0 };
          bucket.count += 1;
          bucket.r += r;
          bucket.g += g;
          bucket.b += b;
          bucket.saturation += saturation;
          buckets.set(key, bucket);
        }

        const ranked = [...buckets.values()]
          .sort((a, b) => {
            const scoreA = a.count * (0.35 + a.saturation / a.count);
            const scoreB = b.count * (0.35 + b.saturation / b.count);
            return scoreB - scoreA;
          })
          .slice(0, 2);

        if (ranked.length === 0) return resolve(fallback(url));
        const first = toHsl(ranked[0]);
        const second = ranked[1] ? toHsl(ranked[1]) : first;
        resolve([first, second]);
      } catch {
        resolve(fallback(url));
      }
    };
    image.onerror = () => resolve(fallback(url));
    image.src = url;
  });

  if (cache.size >= MAX_CACHE) {
    const oldest = cache.keys().next().value;
    if (oldest) cache.delete(oldest);
  }
  cache.set(url, palette);
  return palette;
}

function toHsl(color: { r: number; g: number; b: number; count: number }): string {
  const r = color.r / color.count / 255;
  const g = color.g / color.count / 255;
  const b = color.b / color.count / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  let hue = 0;
  const lightness = (max + min) / 2;
  const delta = max - min;

  if (delta > 0) {
    if (max === r) hue = ((g - b) / delta) % 6;
    else if (max === g) hue = (b - r) / delta + 2;
    else hue = (r - g) / delta + 4;
    hue *= 60;
    if (hue < 0) hue += 360;
  }

  const saturation = delta === 0 ? 0 : delta / (1 - Math.abs(2 * lightness - 1));
  return `hsl(${Math.round(hue)} ${Math.round(Math.min(0.85, Math.max(0.45, saturation)) * 100)}% ${Math.round(Math.min(0.62, Math.max(0.45, lightness)) * 100)}%)`;
}

function fallback(url: string): [string, string] {
  let hash = 0;
  for (let index = 0; index < url.length; index += 1) {
    hash = url.charCodeAt(index) + ((hash << 5) - hash);
  }
  const hue = Math.abs(hash) % 360;
  return [`hsl(${hue} 65% 52%)`, `hsl(${(hue + 48) % 360} 60% 45%)`];
}
```

El hook (mismo archivo `src/lib/palette.ts`; añadir `import { useEffect, useState } from "react";` al inicio):

```ts
export function useAlbumPalette(url?: string | null): [string, string] {
  const [palette, setPalette] = useState<[string, string]>(["#2dd4bf", "#7c3aed"]);

  useEffect(() => {
    if (!url) {
      setPalette(["#2dd4bf", "#7c3aed"]);
      return;
    }
    let alive = true;
    void extractPalette(url).then((next) => {
      if (alive) setPalette(next);
    });
    return () => {
      alive = false;
    };
  }, [url]);

  return palette;
}
```

`i.ytimg.com` envía `access-control-allow-origin: *` (comprobado), así que el canvas no se contamina. Para portadas locales sin URL, el fallback determinista mantiene un tono coherente.

### 5.7 Integración en `App.tsx`

1. Añadir estado y suscripción única:

```tsx
const [lyrics, setLyrics] = useState<LyricsDoc | null>(null);
const [lyricsOpen, setLyricsOpen] = useState(false);
const currentTrackIdRef = useRef<string | null>(null);
currentTrackIdRef.current = playerState?.current?.id ?? null;

useEffect(() => {
  let unlisten: UnlistenFn | undefined;
  void listenLyricsEvents((event) => {
    if (event.type === "Ready") {
      if (event.data.track_id === currentTrackIdRef.current) setLyrics(event.data);
    } else if (event.data.track_id === currentTrackIdRef.current) {
      setLyrics(null);
    }
  }).then((fn) => {
    unlisten = fn;
  });
  return () => unlisten?.();
}, []);
```

2. Limpiar al cambiar de track y recuperar si el evento se perdió:

```tsx
useEffect(() => {
  setLyrics(null);
  if (!lyricsOpen || !playerState?.current) return;
  let cancelled = false;
  const track = playerState.current;

  void getLyrics({
    track_id: track.id,
    title: track.title,
    artist: track.artist,
    duration_seconds: track.duration_seconds,
  })
    .then((doc) => {
      if (!cancelled && doc.track_id === currentTrackIdRef.current) setLyrics(doc);
    })
    .catch(() => {
      /* el estado vacío ya muestra "buscar letra" */
    });

  return () => {
    cancelled = true;
  };
}, [lyricsOpen, playerState?.current?.id]);
```

3. Offset persistido por track (localStorage; migrar a `state.json` cuando exista):

```tsx
const offsetKey = playerState?.current ? `cadence.lyrics.offset.${playerState.current.id}` : null;
const [offsetMs, setOffsetMs] = useState(0);

useEffect(() => {
  if (!offsetKey) return setOffsetMs(0);
  const stored = Number(localStorage.getItem(offsetKey) ?? "0");
  setOffsetMs(Number.isFinite(stored) ? Math.max(-5000, Math.min(5000, stored)) : 0);
}, [offsetKey]);

const changeOffset = useCallback(
  (next: number) => {
    const clamped = Math.max(-5000, Math.min(5000, next));
    setOffsetMs(clamped);
    if (offsetKey) localStorage.setItem(offsetKey, String(clamped));
  },
  [offsetKey],
);
```

4. Render del panel dentro del contenedor relativo del contenido:

```tsx
{lyricsOpen && playerState?.current && (
  <LyricsPanel
    thumbnailUrl={playerState.current.thumbnail_url}
    lyrics={lyrics}
    status={playerState.status}
    positionSeconds={playerState.position_seconds}
    durationSeconds={playerState.duration_seconds}
    speed={playerState.speed ?? 1}
    offsetMs={offsetMs}
    onSeek={handleSeekCommit}
    onOffsetChange={changeOffset}
    onClose={() => setLyricsOpen(false)}
    onManualSearch={() => setManualSearchOpen(true)}
    onReload={() => {
      const track = playerState?.current;
      if (!track) return;
      setLyrics(null);
      void getLyrics({
        track_id: track.id,
        title: track.title,
        artist: track.artist,
        duration_seconds: track.duration_seconds,
        force: true,
      }).then(setLyrics);
    }}
  />
)}
```

`handleSeekCommit` ya existe (`App.tsx:1366-1378`) y aplica el `seekGuardRef`; reutilizarlo evita que un `Progress` atrasado devuelva el slider. **No** crear un `seek` paralelo para letras.

5. Botón en `NowPlaying`: añadir `onToggleLyrics` / `lyricsOpen` a `NowPlayingProps` y un `GlassButton` con icono `Mic2` o `AlignLeft`. El componente ya es `React.memo`; las props nuevas deben ser estables (`useCallback`) para no romper la memoización (audit F2).

**Sobre el re-render a 2 Hz:** `LyricsPanel` recibe `positionSeconds`, así que se re-renderiza a 2 Hz. `LyricsList`/`LyricLine` están memoizados y solo reciben `activeIndex`/`distance`; el coste a 2 Hz es un render del header y del footer, no de las 100 líneas. Si se quiere eliminar también eso, mover `positionSeconds` a un store externo y leerlo con `useSyncExternalStore` solo en el footer. No es necesario para v1.

### 5.8 Accesibilidad

- Cada línea es un `<button>` real: foco, Enter y Space funcionan.
- `aria-current="true"` en la línea activa; **no** usar `aria-live` en cada cambio (un lector de pantalla leería la canción entera sin parar). Ofrecer un toggle “anunciar línea” que active `aria-live="polite"` en un nodo oculto, solo para quien lo pida.
- Contraste: la línea activa es blanca sobre fondo oscuro; las inactivas bajan a 34 % de opacidad, suficiente para contexto pero no para lectura principal. Si se quiere cumplir WCAG AA en todas, subir el mínimo a 0.55.
- `prefers-reduced-motion`: sin transición de scroll, sin scale, sin animación de skeleton.
- El viewport tiene `tabIndex={0}` y soporta `ArrowUp`/`ArrowDown` para navegación manual (añadir ~15 líneas; no incluido arriba para no alargar).
- El botón de cerrar tiene `aria-label`; el panel tiene `aria-label="Letras"`.

---

## 6. Rendimiento: presupuesto y técnicas

| Métrica | Objetivo | Cómo se consigue |
|---|---|---|
| `get_lyrics` cache hit (memoria) | **< 1 ms** | `TinyLru` + `Arc`, sin serialización hasta el borde |
| `get_lyrics` cache hit (disco) | **< 5 ms** | JSON pequeño + `spawn_blocking` |
| LRCLIB p50 / p95 | **200–400 ms / < 900 ms** | timeout 6 s, pool, single-flight |
| Parse 300 líneas | **< 0,5 ms** | una pasada, `Vec` con capacidad, sin regex |
| Memoria por doc | **8–20 KB** | `String` + `Vec`, sin `serde_json::Value` |
| Memoria total caché | **< 1,5 MB** | 64 docs LRU |
| Disco | **< 50 MB** | presupuesto + poda por mtime |
| IPC durante playback | **0 msg/s** | reloj local; `Progress` ya existente |
| Renders de React | **≈ 0,25/s** (una por línea) | `setActiveIndex` solo al cambiar |
| Escrituras de estilo por frame | **1** (`--p`) | una línea activa |
| Capas de compositing nuevas | **1** (aura estática) | sin `backdrop-filter` |
| CPU con panel visible reproduciendo | **< 0,5 %** en iGPU | transform + un clip-path |
| CPU con panel visible pausado | **0 %** | `rAF` cancelado |

**Las cinco reglas que no se negocian:**

1. **Nunca leer layout dentro del `rAF`.** `offsetTop`, `getBoundingClientRect`, `scrollHeight` solo en `measure()` (montaje, resize, cambio de letra).
2. **Nunca animar `font-size`, `line-height`, `padding` o `margin` de la línea activa.** Solo `transform`, `opacity`, `filter` (al cambiar de línea) y `clip-path` (por frame).
3. **Nunca renderizar 100 líneas por frame.** El estado `activeIndex` cambia ~0,25 veces/s.
4. **Nunca poner la letra en `PlayerState`.** Se clona a 2 Hz.
5. **Nunca añadir `backdrop-filter` al panel.** El audit F1 ya está al límite.

**Perfilado recomendado:**

```bash
# CPU del proceso principal y del WebView
pidstat -p $(pgrep -f 'cadence$') 1 30
# Long tasks en DevTools > Performance: ninguna > 16 ms al cambiar de línea
# Memoria: heap snapshot antes/después de 20 tracks; debe estabilizarse
```

---

## 7. Innovaciones de alto impacto y bajo consumo

Ordenadas por relación impacto/esfuerzo. Todas respetan el principio del audit: **nada que añada un segundo backend de audio, un segundo mpv o un blur nuevo.**

| # | Innovación | Impacto | Esfuerzo | Consumo | Por qué |
|---|---|---|---|---|---|
| 1 | **Karaoke palabra a palabra** (LRC enriquecido / `hasWordSync`) | Muy alto | Medio | ~0 | El wipe por palabra es la diferencia percibida más grande frente a “resaltar línea”. LRCLIB ya expone `hasWordSync`; el parser ya soporta `<mm:ss.xx>`. |
| 2 | **Calibración de offset por track (±5 s, pasos de 250 ms)** | Muy alto | Bajo | 0 | Las versiones de YouTube casi nunca coinciden al milisegundo con el álbum. Sin esto, el 20 % de las letras “van mal” y el usuario culpa a la app. |
| 3 | **Búsqueda manual en LRCLIB con selector de candidatos** | Alto | Bajo | 0 | Rescata el 10–15 % de tracks que el match automático falla (live, remix, covers). El comando `search_lyrics` ya está diseñado. |
| 4 | **Prefetch de letras de la cola** | Medio | Muy bajo | 1 request/track | El orquestador ya resuelve el audio del siguiente; añadir `lyrics.request(next)` en la misma tarea. Cambio de track con letra instantánea. |
| 5 | **Modo Focus “una línea”** | Alto (producto) | Muy bajo | 0 | Durante una sesión Pomodoro, mostrar solo la línea actual + la siguiente, sin lista. Reduce carga cognitiva y encaja con la filosofía de Cadence. |
| 6 | **Ventana flotante de letras always-on-top** | Alto | Medio | ~0 (misma UI) | Segunda `WebviewWindow` de 420×120, sin blur, que escucha `lyrics-event` y `Progress`. Para quien trabaja con la música de fondo. Reutiliza el 90 % del componente. |
| 7 | **Editor “tap to sync” para LRC local** | Alto (local) | Medio | 0 | Reproduces el tema, pulsas un botón al inicio de cada línea y se escribe un `.lrc`. Convierte a Cadence en la herramienta de letras para tu biblioteca. |
| 8 | **Letra bilingüe con sidecar `.es.lrc`** | Medio | Bajo | 0 | Sin APIs de traducción (privacidad/coste): si existe `<track>.es.lrc`, se muestra la segunda línea en pequeño bajo la original. Ideal para aprender idiomas. |
| 9 | **“¿Letra incorrecta?” → reemplazar por ID** | Medio | Bajo | 0 | Un click guarda el `id` elegido en `state.json` y salta el matching automático la próxima vez. Cierra el círculo de calidad. |
| 10 | **Línea actual en MPRIS / `cadence-cli`** | Bajo | Bajo | 0 | `playerctl` y la status bar pueden mostrar la letra; el backend ya tiene la posición y el doc. Un `watch` de línea activa y listo. |
| 11 | **Copiar línea / compartir como texto** | Bajo | Muy bajo | 0 | `navigator.clipboard.writeText`; útil para citas. Sin imagen, sin canvas. |
| 12 | **“Instrumental” con aura reactiva al estado** | Bajo | Muy bajo | 0 | Si `instrumental`, en vez de un empty state aburrido, un pulso lento de opacidad (no FFT). Honesto y barato. |

### Las tres que haría primero

1. **Offset por track** (2 h): es la que más percepciones de “esto funciona” genera por línea de código.
2. **Búsqueda manual** (4 h): elimina la clase de fallo “no hay letra” que más soporte genera.
3. **Prefetch de cola** (1 h): hace que la letra aparezca antes de que el usuario abra el panel.

### Lo que **no** haría

- **Visualizador FFT “real”**: el audio está en mpv, otro proceso. Necesitarías loopback (PipeWire/PulseAudio monitor), un segundo consumidor de audio y sincronización; +complejidad y +CPU. Un aura que respira con el estado da el 80 % de la sensación con el 0 % del coste.
- **Traducción automática en la nube**: latencia, coste, privacidad y términos de servicio. Sidecar bilingüe es superior para este producto.
- **`backdrop-filter` por línea** o blur animado: mata la GPU en iGPU, justo lo que el audit F1 intenta arreglar.
- **Guardar la letra dentro de `PlayerState`** o emitirla en `orchestrator-event`: acopla el canal caliente a un payload grande.
- **`dangerouslySetInnerHTML`**: las letras son texto de terceros; React escapa por defecto. No lo rompas.
- **Un segundo mpv para “karaoke”**: +58 MB medidos en el audit.

---

## 8. Roadmap, tests y criterios de aceptación

### Fases

| Fase | Contenido | Estimación | Gate |
|---|---|---|---|
| **L0** | `models/lyrics.rs`, parser completo, tests unitarios | 0,5 día | `cargo test` verde; 8 fixtures |
| **L1** | Caché (memoria/disco/negative), cliente LRCLIB, single-flight | 1 día | test de caché + `#[ignore]` live test |
| **L2** | `LyricsService`, comandos, evento `lyrics-event`, integración en `play()` | 1 día | `cargo clippy --all-targets` limpio; no bloquea `play()` |
| **L3** | Panel, reloj, scroll, seek, aura, CSS | 1,5–2 días | 60 fps; sin long tasks > 16 ms |
| **L4** | Offset, búsqueda manual, prefetch, word-sync | 1–2 días | pruebas manuales con 20 tracks |
| **L5** | Sidecar `.lrc` (ya en L1), ID3 con `lofty`, editor tap-to-sync | 2–3 días | solo si entra la biblioteca local |

### Tests

**Rust**

```bash
cargo test -p cadence_lib lyrics
cargo clippy --all-targets -- -D warnings
```

- Parser: básico, offset, múltiples timestamps, horas, coma decimal, word-sync, letra plana, metadatos, `[Chorus: ...]`, BOM, CRLF, límite de líneas.
- `line_at`: antes de la primera, exacta, entre líneas, después de la última, lista vacía.
- Caché: put/get, TTL positivo y negativo, evicción, colisión de hash (clave distinta, mismo archivo imposible por comparación), poda por presupuesto.
- LRCLIB: parseo de fixture real (sin red) + un test `#[ignore]` que golpea la API una vez.
- Servicio: single-flight con `tokio::join!` de dos `load()`; solo una petición (contador en un mock).

**Frontend**

```bash
npm test -- lyrics
```

- `findActiveLine` (4 casos).
- `formatTimestamp`.
- `toHsl`/`fallback` (opcional).

**Manual (checklist de 10 minutos)**

1. Track con letra sincronizada: la línea activa coincide al abrir el panel.
2. Seek a mitad: la línea salta en < 100 ms y no vuelve atrás.
3. Pausa: el rAF se detiene; la línea no se mueve.
4. Cambio de velocidad 0.85x/1.25x: la letra sigue sincronizada tras 30 s.
5. Skip rápido entre 3 tracks: nunca aparece la letra del anterior.
6. Track sin letra: empty state + búsqueda manual.
7. Track instrumental: badge, sin lista.
8. Archivo local con `.lrc`: se usa el sidecar, no LRCLIB.
9. Offline: track ya cacheado abre letra; track nuevo muestra error recuperable.
10. Rueda del ratón: navegación manual 3 s y recentrado suave.

### Criterios de aceptación

- `play()` con URL cacheada sigue < 150 ms (medir antes/después).
- `get_lyrics` cache hit < 1 ms (bench local).
- Panel abierto reproduciendo: CPU < 0,5 % en iGPU; sin frames > 16 ms al cambiar de línea.
- Memoria: +< 2 MB tras 20 tracks.
- Cero peticiones LRCLIB repetidas para el mismo track dentro del TTL (verificar con `tracing`).
- `cargo clippy --all-targets -- -D warnings` y `npm run build` limpios.
- Sin `unwrap`/`expect` en código no-test (lint del crate).

---

## 9. Riesgos, privacidad y licencias

| Riesgo | Probabilidad | Mitigación |
|---|---|---|
| LRCLIB caído o lento | Media | Timeout 6 s, caché, negative cache, UI con reintento; la reproducción nunca depende de ello |
| Match incorrecto (live/remix) | Alta | `clean_title`, tolerancia de duración, búsqueda manual, offset, “reemplazar por ID” |
| Letra desincronizada por intro/outro | Alta | Offset por track; si `duration` difiere > 5 s, avisar en el footer (“versión distinta”) |
| API cambia el schema | Baja | `#[serde(default)]` en campos opcionales; fixture test; logs |
| Copyright de las letras | — | No empaquetar letras; caché local personal; atribución “Letras por LRCLIB” con enlace; el usuario puede desactivar |
| Privacidad | Media | Ajuste “letras online” (opt-out) que evita cualquier petición; no se envía nada más que título/artista/duración |
| WebView sin red | Baja | La petición es Rust; no depende de CORS ni de `connect-src` |
| Ruta local no UTF-8 | Baja | Se usa `PathBuf`; el `track_id` ya es la ruta tal cual; el sidecar se resuelve con `with_extension` |

**Atribución:** añadir en el footer del panel “Letras por LRCLIB” cuando `source === "lrclib"`, con un enlace que abra el navegador (`tauri-plugin-opener` ya está, aunque el audit sugiere quitarlo si no se usa; aquí pasaría a tener un uso real). Si se decide no usar el plugin, un texto sin enlace es suficiente para v1.

**Opt-out:** `set_lyrics_enabled(false)` hace que `LyricsService::load` devuelva un doc vacío sin tocar la red. Persistir en `state.json` cuando exista; mientras tanto, `localStorage` + llamada al comando al arrancar.

---

## 10. Lo que no haría (y por qué)

- **`scrollIntoView`**: ver ADR-06. Es la fuente clásica de jank y de scrolls inesperados del contenedor padre.
- **Un `setState` por frame con la posición**: 60 renders/s de 100 líneas. Inviable.
- **`position_seconds` en el contexto global del panel**: el audit F2 ya documenta el coste de los re-renders a 2 Hz; no añadir otra víctima.
- **Un `LyricsProvider` con `useEffect` que hace polling cada segundo**: la letra no cambia; el evento es suficiente.
- **Guardar letras en SQLite**: son documentos pequeños y efímeros; JSON + LRU es más simple y más rápido. SQLite entra cuando haya biblioteca local, no antes (ADR del repo).
- **Dependencia `moka`/`cacache`**: LRU de 64 entradas y 30 líneas propias; no hay razón.
- **`lofty` solo para letras**: 400 KB para un caso que el sidecar cubre. Entra con la biblioteca local, no antes.
- **Renderizar la letra con `innerHTML`**: riesgo de inyección y de romper el layout con HTML inesperado.

---

## Anexo A — Mapa de archivos

```text
src-tauri/src/
├── models/
│   ├── lyrics.rs                  ← NUEVO: structs + line_at
│   └── mod.rs                     ← + pub mod lyrics;
├── services/
│   ├── lyrics/
│   │   ├── mod.rs                 ← NUEVO: LyricsService, single-flight, local
│   │   ├── parser.rs              ← NUEVO: LRC parser + tests
│   │   ├── cache.rs               ← NUEVO: TinyLru + disco + poda
│   │   ├── lrclib.rs              ← NUEVO: cliente HTTP + scoring + clean_title
│   │   ├── id3.rs                 ← NUEVO (v1.5): USLT/SYLT
│   │   └── text.rs                ← NUEVO: clean_title/clean_artist/normalize
│   ├── orchestrator.rs            ← + lyrics: Arc<LyricsService>, request() en play()
│   └── mod.rs                     ← + pub mod lyrics;
├── commands/
│   ├── lyrics.rs                  ← NUEVO: 6 comandos
│   └── mod.rs                     ← + pub mod lyrics;
├── errors.rs                      ← + Lyrics { message }
└── lib.rs                         ← manage service, puente de eventos, handlers

src/
├── lib/
│   ├── tauri.ts                   ← + tipos y wrappers
│   ├── lyrics.ts                  ← NUEVO: findActiveLine, formatTimestamp
│   ├── lyrics.test.ts             ← NUEVO
│   └── palette.ts                 ← NUEVO: extractPalette + hook
├── components/lyrics/
│   ├── LyricsPanel.tsx            ← NUEVO
│   ├── useLyricClock.ts           ← NUEVO
│   └── useLyricsScroll.ts         ← NUEVO
├── styles/lyrics.css              ← NUEVO
├── index.css                      ← + @import lyrics.css
└── App.tsx                        ← + estado, suscripción, panel, botón
```

## Anexo B — Checklist de PRs (orden del ADR del repo)

1. **Model** — `models/lyrics.rs` + `line_at` + tests. Sin lógica de red.
2. **Parser** — `parser.rs` + 8 fixtures. `cargo test lyrics` verde.
3. **Cache** — `cache.rs` + tests con `tempdir`. Sin `unwrap`.
4. **Provider** — `lrclib.rs` + fixture + test `#[ignore]` live.
5. **Service** — `mod.rs` + single-flight + local. Test de concurrencia.
6. **Command** — `commands/lyrics.rs`, thin delegation, registrar en `lib.rs`.
7. **Orchestrator** — `with_lyrics`, `request()` en `play()`, comprobar que `play()` no se ralentiza.
8. **Frontend lib** — tipos, wrappers, `findActiveLine` + vitest.
9. **UI** — panel, reloj, scroll, CSS, botón en `NowPlaying`.
10. **Perf** — 10 tracks, DevTools, `pidstat`, memoria. Adjuntar números al PR.

**Definición de “hecho”:** un track de YouTube con letra sincronizada se abre en < 1 s (red) o < 5 ms (caché), la línea activa sigue al audio a 0.85x y 1.25x, el seek por click responde en < 100 ms, y el panel no añade frames > 16 ms ni supera el 0,5 % de CPU en iGPU.


## Anexo C — Validación ejecutada

No basta con que el código del documento "parezca" correcto. Cada bloque con riesgo se extrajo a un crate temporal y se compiló contra las versiones reales del proyecto.

| Qué se validó | Cómo | Resultado |
|---|---|---|
| Parser LRC (`parser.rs`) | `cargo test` en crate aislado | **8/8 tests en verde** (básico, orden, horas, coma decimal, múltiples timestamps, word-sync, offset, metadatos) |
| Caché (`cache.rs`) | `cargo check` con stubs | Compila limpio |
| Cliente LRCLIB (`lrclib.rs`) | `cargo check` contra `reqwest 0.13.4` | Compila limpio; solo un `use` sin usar, corregido en el documento |
| Servicio + single-flight (`mod.rs`) | `cargo check` con todo el stack junto | Compila limpio, **0 warnings** |
| Stack backend completo | `models` + `parser` + `cache` + `lrclib` + `mod` + `id3` | Compila y pasa tests |
| Frontend TS (hooks + componentes) | `tsc --noEmit` | Sin errores (exit 0) |
| Features de `reqwest` | Inspección de `Cargo.toml` de `reqwest 0.13.4` + compilación | `["json", "rustls"]` es el conjunto válido; `rustls-tls` **no existe** en 0.13 |
| Fences Markdown | Conteo de vallas de código | 70 líneas de valla = balanceado |
| `unwrap`/`expect` | `grep` sobre todo el Rust del documento | Solo 1, dentro de `#[cfg(test)]` y con `#[allow]`, como exige el crate |

**Lo que esta validación no cubre:** no se ha ejecutado contra LRCLIB en vivo desde este crate (solo se verificó la API con peticiones manuales), ni se ha medido el coste real de composición en la GPU del usuario. Esos dos puntos quedan como criterio de aceptación del PR de rendimiento (Anexo B, paso 10).
