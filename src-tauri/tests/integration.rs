//! Tests de integración end-to-end: orquestador + mpv real + yt-dlp (stub).
//!
//! Estos tests no tocan la red: `yt-dlp` se reemplaza por un script que
//! devuelve un WAV local generado por el propio test. Así se cubre el camino
//! completo (resolver → cargar → reproducir → pausar → buscar → loop → fin →
//! recuperación de URL caducada → reinicio de mpv) sin depender de YouTube.
//!
//! Ejecución:
//! ```text
//! cargo test --test integration -- --include-ignored --test-threads=1
//! ```
//! Son `#[ignore]` porque necesitan el binario `mpv` instalado.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use cadence_lib::models::player_state::PlaybackStatus;
use cadence_lib::models::search_result::SearchResult;
use cadence_lib::services::mpv_event::{MpvEvent, MpvPlayer};
use cadence_lib::services::orchestrator::{OrchestratorEvent, PlayerOrchestratorBuilder};

/// Serializa los tests: comparten el directorio de trabajo y mutan `PATH`.
static LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

/// Reintenta un bloque asíncrono hasta que devuelva `true` o se agote el
/// tiempo. Se hace macro (y no función) para que cada iteración cree un
/// futuro nuevo y no aparezcan problemas de lifetimes con las capturas.
macro_rules! wait_until {
    ($what:literal, $timeout:expr, $cond:expr) => {{
        let deadline = tokio::time::Instant::now() + $timeout;
        loop {
            if ($cond).await {
                break;
            }
            if tokio::time::Instant::now() > deadline {
                panic!("timed out waiting for: {}", $what);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }};
}

// ===== Helpers =====

/// Escribe un WAV PCM de `seconds` segundos con silencio (sin ruido en el
/// equipo de audio del que se ejecute el test).
fn write_silence_wav(path: &Path, seconds: u32) {
    const SAMPLE_RATE: u32 = 8000;
    let samples = SAMPLE_RATE * seconds;
    let data_len = samples * 2; // 16 bits mono
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes()); // tamaño del bloque fmt
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // bytes/segundo
    bytes.extend_from_slice(&2u16.to_le_bytes()); // block align
    bytes.extend_from_slice(&16u16.to_le_bytes()); // bits por muestra
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    bytes.extend(std::iter::repeat_n(0u8, data_len as usize));
    std::fs::write(path, bytes).expect("write wav");
}

/// Cómo debe comportarse el stub de yt-dlp al resolver audio.
#[derive(Clone, Copy, PartialEq)]
enum ResolveMode {
    /// Siempre devuelve el WAV válido.
    AlwaysOk,
    /// La primera resolución devuelve un path inexistente (simula una URL
    /// firmada caducada que mpv no puede abrir) y a partir de ahí válida.
    FailFirst,
}

impl ResolveMode {
    fn as_str(self) -> &'static str {
        match self {
            ResolveMode::AlwaysOk => "always_ok",
            ResolveMode::FailFirst => "fail_first",
        }
    }
}

struct Fixture {
    dir: PathBuf,
    socket: PathBuf,
    wav: PathBuf,
    original_path: String,
    resolve_calls: PathBuf,
}

impl Fixture {
    fn new(name: &str, wav_seconds: u32, mode: ResolveMode) -> Self {
        let dir =
            std::env::temp_dir().join(format!("cadence-integ-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create fixture dir");

        let wav = dir.join("tone.wav");
        write_silence_wav(&wav, wav_seconds);

        // Stub de yt-dlp: resuelve al WAV local o responde una búsqueda fake.
        let stub = dir.join("yt-dlp");
        std::fs::write(
            &stub,
            r#"#!/bin/sh
# Stub de yt-dlp para tests de integración de Cadence.
dir="$CADENCE_TEST_DIR"
for arg in "$@"; do
  if [ "$arg" = "-g" ]; then
    calls="$dir/resolve_calls"
    n=$(cat "$calls" 2>/dev/null || echo 0)
    n=$((n + 1))
    echo "$n" > "$calls"
    if [ "$(cat "$dir/resolve_mode" 2>/dev/null)" = "fail_first" ] && [ "$n" = "1" ]; then
      # Path inexistente: mpv no puede abrirlo y dispara la recuperación.
      echo "$dir/tone.expired.wav"
      exit 0
    fi
    echo "$dir/tone.wav"
    exit 0
  fi
done
cat "$dir/search_result.json"
"#,
        )
        .expect("write yt-dlp stub");
        set_executable(&stub);

        std::fs::write(dir.join("search_result.json"), SEARCH_RESULT_JSON).expect("write search json");
        std::fs::write(dir.join("resolve_mode"), mode.as_str()).expect("write resolve mode");
        let resolve_calls = dir.join("resolve_calls");
        std::fs::write(&resolve_calls, "0").expect("init counter");

        let original_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{}", dir.display(), original_path));
        std::env::set_var("CADENCE_TEST_DIR", dir.as_os_str());

        let socket = dir.join("mpv.sock");
        Self {
            dir,
            socket,
            wav,
            original_path,
            resolve_calls,
        }
    }

    fn resolve_calls(&self) -> u32 {
        std::fs::read_to_string(&self.resolve_calls)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    }

    /// Corta el proceso mpv por su socket IPC, como si se hubiera caído.
    async fn kill_mpv(&self) {
        use tokio::io::AsyncWriteExt;
        let mut stream = tokio::net::UnixStream::connect(&self.socket)
            .await
            .expect("connect to mpv socket to kill it");
        stream
            .write_all(b"{\"command\":[\"quit\"]}\n")
            .await
            .expect("send quit to mpv");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::env::set_var("PATH", &self.original_path);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(unix)]
fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).unwrap();
}

const SEARCH_RESULT_JSON: &str =
    r#"{"id":"cadence-test-track","title":"Tono de prueba","artist":"Cadence Tests","duration":8.0,"ie_key":"Youtube"}"#;

fn sample_track(seconds: u64) -> SearchResult {
    SearchResult {
        id: "cadence-test-track".into(),
        title: "Tono de prueba".into(),
        artist: Some("Cadence Tests".into()),
        duration_seconds: seconds,
        thumbnail_url: "https://i.ytimg.com/vi/cadence-test-track/hqdefault.jpg".into(),
    }
}

/// Espera el siguiente evento que cumpla `pred`, tolerando `Lagged`.
async fn next_event<T, F>(
    rx: &mut tokio::sync::broadcast::Receiver<T>,
    what: &str,
    timeout: Duration,
    mut pred: F,
) -> T
where
    T: Clone + std::fmt::Debug,
    F: FnMut(&T) -> bool,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!("timed out waiting for event: {what}");
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(event)) => {
                if pred(&event) {
                    return event;
                }
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                panic!("event bus closed while waiting for: {what}")
            }
            Err(_) => panic!("timed out waiting for event: {what}"),
        }
    }
}

fn is_track_started(event: &OrchestratorEvent) -> bool {
    matches!(event, OrchestratorEvent::TrackStarted(_))
}

// ===== mpv a nivel de IPC =====

/// mpv real: carga un WAV, reporta duración/posición, pausa, busca y avisa
/// cuando el archivo termina (`end-file` reason "eof").
#[tokio::test]
#[ignore = "requires the mpv binary"]
async fn mpv_plays_a_local_wav_end_to_end() {
    let _guard = LOCK.lock().await;
    let fixture = Fixture::new("mpv-wav", 3, ResolveMode::AlwaysOk);

    let player = MpvPlayer::connect(fixture.socket.clone())
        .await
        .expect("connect to mpv");
    assert!(player.status().connected);

    let mut events = player.subscribe_events();
    player
        .load(&fixture.wav.display().to_string())
        .await
        .expect("load wav");

    wait_until!("mpv reports duration", Duration::from_secs(5), async {
        player
            .status()
            .duration_seconds
            .map(|d| d > 2.0 && d < 4.0)
            .unwrap_or(false)
    });

    wait_until!("mpv reports playback position", Duration::from_secs(5), async {
        player
            .status()
            .position_seconds
            .map(|p| p > 0.2)
            .unwrap_or(false)
    });

    player.pause().await.expect("pause");
    wait_until!("mpv reports pause", Duration::from_secs(3), async {
        player.status().paused
    });

    let paused_at = player.status().position_seconds.unwrap_or(0.0);
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(
        (player.status().position_seconds.unwrap_or(0.0) - paused_at).abs() < 0.25,
        "position moved while paused"
    );

    player.resume().await.expect("resume");
    player.seek(1.5).await.expect("seek");
    wait_until!("seek lands near 1.5s", Duration::from_secs(5), async {
        player
            .status()
            .position_seconds
            .map(|p| (p - 1.5).abs() < 1.0)
            .unwrap_or(false)
    });

    // El archivo dura 3s: debe llegar `end-file` con reason "eof".
    next_event(&mut events, "end-file eof", Duration::from_secs(20), |e| {
        matches!(e, MpvEvent::EndFile { reason, .. } if reason == "eof")
    })
    .await;

    player.shutdown().await;
}

/// mpv muere (crash, `kill`, etc.): el supervisor debe relanzarlo con el
/// mismo handle público y reportar `Reconnected`.
#[tokio::test]
#[ignore = "requires the mpv binary"]
async fn mpv_supervisor_restarts_after_crash() {
    let _guard = LOCK.lock().await;
    let fixture = Fixture::new("mpv-restart", 8, ResolveMode::AlwaysOk);

    let player = MpvPlayer::connect(fixture.socket.clone())
        .await
        .expect("connect to mpv");
    let mut events = player.subscribe_events();

    fixture.kill_mpv().await;

    next_event(&mut events, "reconnected", Duration::from_secs(15), |e| {
        matches!(e, MpvEvent::Reconnected)
    })
    .await;

    wait_until!("status reports connected again", Duration::from_secs(5), async {
        player.status().connected
    });

    // El handle sigue siendo utilizable tras el reinicio.
    player
        .set_loop(true)
        .await
        .expect("set loop after reconnect");
    assert!(player.status().looping);
    player.shutdown().await;
}

// ===== Orquestador end-to-end =====

/// Camino feliz completo: buscar → play → posición → seek → pause/resume →
/// loop → stop, todo sin red y con mpv real.
#[tokio::test]
#[ignore = "requires the mpv binary"]
async fn orchestrator_playback_cycle_works() {
    let _guard = LOCK.lock().await;
    let fixture = Fixture::new("orch-cycle", 8, ResolveMode::AlwaysOk);

    let orchestrator = PlayerOrchestratorBuilder::new(fixture.socket.clone())
        .build()
        .await
        .expect("build orchestrator");
    let mut events = orchestrator.subscribe_events();

    // Búsqueda resuelta por el stub (sin YouTube).
    let results = orchestrator
        .search("tono de prueba", 5)
        .await
        .expect("search via stub");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "cadence-test-track");
    assert_eq!(results[0].duration_seconds, 8);

    orchestrator.play(sample_track(8)).await.expect("play");
    next_event(&mut events, "TrackStarted", Duration::from_secs(10), |e| {
        is_track_started(e)
    })
    .await;

    let state = orchestrator.get_state().await;
    assert!(state.current.is_some(), "current track must be set");
    assert!(state.connected);
    assert!(state.volume > 0.0);

    // La duración llega por `observe_property` y puede tardar unos ms tras
    // el `loadfile`.
    wait_until!("duration comes from mpv", Duration::from_secs(5), async {
        orchestrator.get_state().await.duration_seconds > 7.0
    });

    wait_until!("playback advances", Duration::from_secs(8), async {
        orchestrator.get_state().await.position_seconds > 0.3
    });

    orchestrator.seek(4.0).await.expect("seek");
    wait_until!("seek lands", Duration::from_secs(6), async {
        (orchestrator.get_state().await.position_seconds - 4.0).abs() < 1.5
    });

    orchestrator.pause().await.expect("pause");
    wait_until!("orchestrator reports paused", Duration::from_secs(4), async {
        orchestrator.get_state().await.status == PlaybackStatus::Paused
    });

    orchestrator.resume().await.expect("resume");
    wait_until!("orchestrator reports playing", Duration::from_secs(4), async {
        orchestrator.get_state().await.status == PlaybackStatus::Playing
    });

    // Loop: estado deseado + reflejo real en mpv.
    orchestrator.set_loop(true).await.expect("loop on");
    assert!(orchestrator.get_state().await.looping);
    wait_until!("mpv loop-file is on", Duration::from_secs(4), async {
        orchestrator.mpv_status().looping
    });

    orchestrator.stop().await.expect("stop");
    let state = orchestrator.get_state().await;
    assert!(state.current.is_none(), "stop clears the current track");
    assert_eq!(state.status, PlaybackStatus::Stopped);
    // El loop es una preferencia del usuario: stop no la resetea.
    assert!(state.looping, "desired loop survives stop");

    orchestrator.shutdown().await;
}

/// Una URL firmada caducada (mpv no puede abrir el stream) debe provocar un
/// reintento automático con una resolución nueva, no un fallo visible.
#[tokio::test]
#[ignore = "requires the mpv binary"]
async fn orchestrator_recovers_from_expired_url() {
    let _guard = LOCK.lock().await;
    let fixture = Fixture::new("orch-expired", 8, ResolveMode::FailFirst);

    let orchestrator = PlayerOrchestratorBuilder::new(fixture.socket.clone())
        .build()
        .await
        .expect("build orchestrator");

    orchestrator.play(sample_track(8)).await.expect("play");

    // La recuperación debe terminar con el mismo track reproduciéndose.
    wait_until!("recovered playback with the same track", Duration::from_secs(30), async {
        let state = orchestrator.get_state().await;
        state.current.is_some() && state.duration_seconds > 7.0 && state.position_seconds > 0.0
    });

    assert!(
        fixture.resolve_calls() >= 2,
        "the URL must have been resolved twice (expired then fresh), got {}",
        fixture.resolve_calls()
    );

    orchestrator.shutdown().await;
}

/// mpv muere en mitad de la canción: la app debe reconectar, recargar el
/// track y reanudar cerca de donde estaba.
#[tokio::test]
#[ignore = "requires the mpv binary"]
async fn orchestrator_restores_playback_after_mpv_restart() {
    let _guard = LOCK.lock().await;
    let fixture = Fixture::new("orch-restart", 20, ResolveMode::AlwaysOk);

    let orchestrator = PlayerOrchestratorBuilder::new(fixture.socket.clone())
        .build()
        .await
        .expect("build orchestrator");
    let mut events = orchestrator.subscribe_events();

    orchestrator.play(sample_track(20)).await.expect("play");
    wait_until!("playback reached ~2s", Duration::from_secs(15), async {
        orchestrator.get_state().await.position_seconds > 2.0
    });

    let before = orchestrator.get_state().await.position_seconds;

    // Caída simulada de mpv.
    fixture.kill_mpv().await;

    next_event(
        &mut events,
        "ConnectionChanged disconnected",
        Duration::from_secs(10),
        |e| matches!(e, OrchestratorEvent::ConnectionChanged { connected: false }),
    )
    .await;

    wait_until!("connection is back", Duration::from_secs(20), async {
        orchestrator.is_connected()
    });

    // El track se recarga y se reanuda la posición.
    wait_until!("track reloaded after reconnect", Duration::from_secs(20), async {
        orchestrator.get_state().await.current.is_some()
    });

    wait_until!("position restored near where it was", Duration::from_secs(20), async {
        let position = orchestrator.get_state().await.position_seconds;
        position > 0.5 && (position - before).abs() < 6.0
    });

    orchestrator.shutdown().await;
}
