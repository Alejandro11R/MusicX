use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, watch, Mutex, RwLock};
use tokio::time::sleep;

use crate::errors::CadenceError;
use crate::models::player_state::{PlaybackStatus, PlayerState};
use crate::models::search_result::SearchResult;
use crate::services::mpv_event::{MpvEvent, MpvPlayer, MpvStatus};
use crate::services::yt_dlp::YtDlpService;

/// Cuántas veces intentamos resolver de nuevo la URL de un track cuando
/// mpv falla al abrirlo (típicamente un 403 por URL firmada expirada).
const MAX_URL_RETRIES: u32 = 2;
const URL_RETRY_DELAY: Duration = Duration::from_millis(350);

/// Controlador de playback rápido (pause, resume, seek, volume, loop).
/// No hace operaciones pesadas (yt-dlp).
#[derive(Clone)]
pub struct PlaybackController {
    mpv: Arc<MpvPlayer>,
}

impl PlaybackController {
    pub fn new(mpv: Arc<MpvPlayer>) -> Self {
        Self { mpv }
    }

    pub async fn pause(&self) -> Result<(), CadenceError> {
        self.mpv.pause().await
    }

    pub async fn resume(&self) -> Result<(), CadenceError> {
        self.mpv.resume().await
    }

    pub async fn stop(&self) -> Result<(), CadenceError> {
        self.mpv.stop().await
    }

    pub async fn set_volume(&self, level: f64) -> Result<(), CadenceError> {
        self.mpv.set_volume(level).await
    }

    pub async fn seek(&self, position_seconds: f64) -> Result<(), CadenceError> {
        self.mpv.seek(position_seconds).await
    }

    pub async fn set_loop(&self, enabled: bool) -> Result<(), CadenceError> {
        self.mpv.set_loop(enabled).await
    }

    pub async fn set_audio_filter(&self, filter: &str) -> Result<(), CadenceError> {
        self.mpv.set_audio_filter(filter).await
    }

    pub async fn set_speed(&self, speed: f64) -> Result<(), CadenceError> {
        self.mpv.set_speed(speed).await
    }

    pub async fn set_media_title(&self, title: &str) -> Result<(), CadenceError> {
        self.mpv.set_media_title(title).await
    }

    /// Obtiene estado instantáneo (sin IPC).
    pub fn status(&self) -> MpvStatus {
        self.mpv.status()
    }

    /// Suscribe a eventos de mpv.
    pub fn subscribe_events(&self) -> broadcast::Receiver<MpvEvent> {
        self.mpv.subscribe_events()
    }

    /// Suscribe a cambios de estado (watch channel).
    pub fn subscribe_status(&self) -> watch::Receiver<MpvStatus> {
        self.mpv.subscribe_status()
    }
}

/// Resolvedor de tracks con yt-dlp (operaciones pesadas).
#[derive(Clone)]
pub struct TrackResolver {
    yt_dlp: Arc<YtDlpService>,
    #[cfg(test)]
    gate: Option<Arc<ResolutionGate>>,
}

#[cfg(test)]
struct ResolutionGate {
    entered: tokio::sync::Notify,
    release: tokio::sync::Semaphore,
}

impl TrackResolver {
    pub fn new(yt_dlp: Arc<YtDlpService>) -> Self {
        Self {
            yt_dlp,
            #[cfg(test)]
            gate: None,
        }
    }

    pub async fn resolve_audio(&self, video_id: &str) -> Result<String, CadenceError> {
        #[cfg(test)]
        if video_id == "local:/blocked.wav" {
            if let Some(gate) = &self.gate {
                gate.entered.notify_one();
                gate.release.acquire().await.expect("gate open").forget();
            }
        }
        if video_id.starts_with("local:") {
            let path = video_id.trim_start_matches("local:");
            return Ok(path.to_string());
        }
        if video_id.starts_with("file://") {
            let path = video_id.trim_start_matches("file://");
            return Ok(path.to_string());
        }
        if std::path::Path::new(video_id).is_file() {
            return Ok(video_id.to_string());
        }
        self.yt_dlp.resolve_audio(video_id).await
    }

    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<SearchResult>, CadenceError> {
        self.yt_dlp.search(query, limit).await
    }

    /// Descarta la URL cacheada de un video para forzar una resolución nueva.
    pub fn invalidate(&self, video_id: &str) {
        self.yt_dlp.invalidate(video_id);
    }
}

/// Estado de la máquina de estados del reproductor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum PlayerMode {
    Stopped,
    Loading,
    Playing,
    Paused,
}

/// Elemento dentro de la cola de reproducción.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct QueueItem {
    pub id: String,
    pub track: SearchResult,
}

/// Eventos internos del orquestador.
///
/// Se serializan como `{ "type": "...", "data": ... }` para que el frontend
/// pueda hacer un switch discriminado sin mirar el nombre del variant.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", content = "data")]
pub enum OrchestratorEvent {
    TrackStarted(SearchResult),
    TrackEnded {
        reason: String,
    },
    PlaybackError(String),
    StateChanged(PlayerMode),
    ConnectionChanged {
        connected: bool,
    },
    QueueUpdated(Vec<QueueItem>),
    AudioPresetChanged(crate::models::player_state::AudioPreset),
    SpeedChanged(f64),
    /// Tick periódico para mover la barra de progreso sin polling desde el
    /// frontend (2 Hz).
    Progress {
        position_seconds: f64,
        duration_seconds: f64,
    },
}

fn store_f64(atomic: &AtomicU64, value: f64) {
    atomic.store(value.to_bits(), Ordering::SeqCst);
}

fn load_f64(atomic: &AtomicU64) -> f64 {
    f64::from_bits(atomic.load(Ordering::SeqCst))
}

/// `loadfile` y `stop` abortan el archivo anterior y mpv emite `end-file` con
/// reason `"stop"`; actuar sobre ese evento borraría el track que acabamos de
/// cargar. `"quit"` llega al apagar mpv y tampoco es un final real.
fn is_abort_reason(reason: &str) -> bool {
    matches!(reason, "stop" | "quit")
}

/// Orquestador principal: coordina controller, resolver, estado y eventos.
pub struct PlayerOrchestrator {
    controller: PlaybackController,
    resolver: TrackResolver,
    current_track: RwLock<Option<SearchResult>>,
    mode: RwLock<PlayerMode>,
    event_tx: broadcast::Sender<OrchestratorEvent>,
    /// Volumen deseado por el usuario (se re-aplica en cada load/reconexión).
    desired_volume: AtomicU64,
    /// Loop deseado por el usuario (no dependemos del estado de mpv).
    desired_loop: AtomicBool,
    /// Si el usuario pausó; se usa para restaurar tras una reconexión.
    desired_paused: AtomicBool,
    /// Última posición conocida, para reanudar tras reconectar.
    last_position: AtomicU64,
    /// Evita recuperaciones/reconexiones concurrentes.
    recovering: AtomicBool,
    /// `subscribe_events` es idempotente.
    events_subscribed: AtomicBool,
    queue: RwLock<Vec<QueueItem>>,
    current_queue_index: RwLock<Option<usize>>,
    /// Navigation follows the latest selected entry while its URL resolves.
    pending_queue_entry: RwLock<Option<(u64, String)>>,
    /// A removed active entry leaves a gap immediately before its successor.
    queue_gap: AtomicBool,
    intent: AtomicU64,
    loaded_intent: AtomicU64,
    effects: Mutex<()>,
    queue_id_counter: AtomicU64,
    desired_audio_preset: RwLock<crate::models::player_state::AudioPreset>,
    desired_speed: AtomicU64,
    mpv_events: broadcast::Receiver<MpvEvent>,
    _mpv_status_rx: watch::Receiver<MpvStatus>,
    lyrics: Option<Arc<crate::services::lyrics::LyricsService>>,
}

impl PlayerOrchestrator {
    pub fn new(
        controller: PlaybackController,
        resolver: TrackResolver,
        mpv_events: broadcast::Receiver<MpvEvent>,
        mpv_status_rx: watch::Receiver<MpvStatus>,
        lyrics: Option<Arc<crate::services::lyrics::LyricsService>>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        let initial_status = controller.status();
        Self {
            controller,
            resolver,
            current_track: RwLock::new(None),
            mode: RwLock::new(PlayerMode::Stopped),
            event_tx,
            desired_volume: AtomicU64::new(
                (if initial_status.volume > 0.0 {
                    initial_status.volume
                } else {
                    100.0
                })
                .to_bits(),
            ),
            desired_loop: AtomicBool::new(initial_status.looping),
            desired_paused: AtomicBool::new(false),
            last_position: AtomicU64::new(0.0f64.to_bits()),
            recovering: AtomicBool::new(false),
            events_subscribed: AtomicBool::new(false),
            queue: RwLock::new(Vec::new()),
            current_queue_index: RwLock::new(None),
            pending_queue_entry: RwLock::new(None),
            queue_gap: AtomicBool::new(false),
            intent: AtomicU64::new(0),
            loaded_intent: AtomicU64::new(0),
            effects: Mutex::new(()),
            queue_id_counter: AtomicU64::new(1),
            desired_audio_preset: RwLock::new(crate::models::player_state::AudioPreset::Normal),
            desired_speed: AtomicU64::new(1.0f64.to_bits()),
            mpv_events,
            _mpv_status_rx: mpv_status_rx,
            lyrics,
        }
    }

    /// Inicia el loop de eventos de mpv -> orquestador.
    pub fn spawn_event_loop(self: Arc<Self>) {
        let this = self.clone();
        tokio::spawn(async move {
            this.run_event_loop().await;
        });
    }

    /// Emite `Progress` a 2 Hz mientras hay un track cargado. Así la barra de
    /// progreso es fluida sin que el frontend haga polling de `state()`.
    pub fn spawn_progress_ticker(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(500));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                if self.current_track.read().await.is_none() {
                    continue;
                }
                let state = self.get_state().await;
                let _ = self.event_tx.send(OrchestratorEvent::Progress {
                    position_seconds: state.position_seconds,
                    duration_seconds: state.duration_seconds,
                });
            }
        });
    }

    async fn run_event_loop(self: Arc<Self>) {
        let mut events = self.mpv_events.resubscribe();
        loop {
            match events.recv().await {
                Ok(event) => self.handle_mpv_event(event).await,
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    tracing::warn!("Orchestrator event loop lagged, catching up");
                    continue;
                }
            }
        }
    }

    async fn handle_mpv_event(self: &Arc<Self>, event: MpvEvent) {
        use MpvEvent::*;
        match event {
            PositionChanged { position_seconds } => {
                if position_seconds.is_finite() && position_seconds >= 0.0 {
                    store_f64(&self.last_position, position_seconds);
                }
            }
            PauseChanged { paused } => {
                if self.current_track.read().await.is_some() {
                    let new_mode = if paused {
                        PlayerMode::Paused
                    } else {
                        PlayerMode::Playing
                    };
                    self.set_mode(new_mode).await;
                }
            }
            EndFile {
                reason,
                position_seconds,
                error,
            } => {
                if let Some(position) = position_seconds {
                    if position.is_finite() && position >= 0.0 {
                        store_f64(&self.last_position, position);
                    }
                }
                self.handle_end_file(reason, error).await;
            }
            Disconnected => {
                let _ = self
                    .event_tx
                    .send(OrchestratorEvent::ConnectionChanged { connected: false });
                tracing::warn!("mpv disconnected");
            }
            Reconnected => {
                let _ = self
                    .event_tx
                    .send(OrchestratorEvent::ConnectionChanged { connected: true });
                tracing::info!("mpv reconnected; restoring playback state");
                let this = self.clone();
                tokio::spawn(async move {
                    this.restore_after_reconnect().await;
                });
            }
            // position/volume/loop se leen del watch channel; no hay nada que
            // propagar por el bus de eventos del orquestador.
            _ => {}
        }
    }

    async fn handle_end_file(&self, reason: String, error: Option<String>) {
        if is_abort_reason(&reason) {
            tracing::debug!(reason = %reason, "ignoring aborted file (loadfile/stop)");
            return;
        }

        let intent = self.intent.load(Ordering::SeqCst);
        if self.loaded_intent.load(Ordering::SeqCst) != intent {
            return;
        }
        let failed = reason == "error" || error.is_some();
        let track = self.current_track.read().await.clone();

        if failed {
            tracing::warn!(reason = %reason, error = ?error, "mpv failed to play current track");
            if let Some(track) = track {
                let position = load_f64(&self.last_position);
                self.recover_track(track, position, intent).await;
                return;
            }
        }

        tracing::info!(reason = %reason, "Track ended");
        if track.is_some() {
            let _ = self.event_tx.send(OrchestratorEvent::TrackEnded {
                reason: reason.clone(),
            });
        }

        // Si el tema terminó normalmente y no estamos en bucle, intentamos auto-avanzar la cola
        if reason == "eof" && !self.desired_loop.load(Ordering::SeqCst) {
            if let Ok(()) = self.next_owned(Some(intent)).await {
                if self.current_track.read().await.is_some() {
                    return;
                }
            }
        }

        let _effects = self.effects.lock().await;
        if !self.owns(intent) {
            return;
        }
        *self.current_track.write().await = None;
        self.desired_paused.store(false, Ordering::SeqCst);
        store_f64(&self.last_position, 0.0);
        self.set_mode(PlayerMode::Stopped).await;
    }

    /// Una URL firmada caducó o el stream falló: invalidamos caché, resolvemos
    /// de nuevo y reanudamos desde la última posición conocida.
    async fn recover_track(&self, track: SearchResult, position: f64, intent: u64) {
        if self.loaded_intent.load(Ordering::SeqCst) != intent {
            return;
        }
        if self.recovering.swap(true, Ordering::SeqCst) {
            return;
        }

        let mut last_error: Option<String> = None;
        for attempt in 0..=MAX_URL_RETRIES {
            if attempt > 0 {
                sleep(URL_RETRY_DELAY * attempt).await;
            }

            if !self.owns(intent) {
                break;
            }
            self.resolver.invalidate(&track.id);
            let resolved = self.resolver.resolve_audio(&track.id).await;
            let _effects = self.effects.lock().await;
            if !self.owns(intent) {
                break;
            }
            match resolved {
                Ok(url) => match self.controller.mpv.load(&url).await {
                    Ok(()) => {
                        if !self.owns(intent) {
                            self.recovering.store(false, Ordering::SeqCst);
                            return;
                        }
                        if let Err(err) = self.apply_desired_output().await {
                            tracing::warn!(error = %err, "failed to restore volume/loop after retry");
                        }
                        if position > 1.0 {
                            if let Err(err) = self.controller.seek(position).await {
                                tracing::warn!(error = %err, "failed to resume position after retry");
                            }
                        }
                        let paused = self.desired_paused.load(Ordering::SeqCst);
                        let _ = if paused {
                            self.controller.pause().await
                        } else {
                            self.controller.resume().await
                        };
                        if !self.owns(intent) {
                            self.recovering.store(false, Ordering::SeqCst);
                            return;
                        }
                        self.set_mode(if paused {
                            PlayerMode::Paused
                        } else {
                            PlayerMode::Playing
                        })
                        .await;
                        let _ = self
                            .event_tx
                            .send(OrchestratorEvent::TrackStarted(track.clone()));
                        tracing::info!(
                            video_id = %track.id,
                            attempt,
                            "recovered playback after expired URL"
                        );
                        self.recovering.store(false, Ordering::SeqCst);
                        return;
                    }
                    Err(err) => last_error = Some(err.to_string()),
                },
                Err(err) => last_error = Some(err.to_string()),
            }
            tracing::warn!(
                video_id = %track.id,
                attempt,
                error = ?last_error,
                "retry to resolve audio URL failed"
            );
        }

        self.recovering.store(false, Ordering::SeqCst);
        let _effects = self.effects.lock().await;
        if !self.owns(intent) {
            return;
        }
        let message = last_error.unwrap_or_else(|| "Couldn't recover this track".to_string());
        let _ = self
            .event_tx
            .send(OrchestratorEvent::PlaybackError(message));
        *self.current_track.write().await = None;
        self.set_mode(PlayerMode::Stopped).await;
    }

    /// mpv se reinició: re-aplicamos volumen/loop y, si había un track,
    /// volvemos a cargarlo en la posición donde estaba.
    async fn restore_after_reconnect(&self) {
        let intent = self.intent.load(Ordering::SeqCst);
        if self.recovering.swap(true, Ordering::SeqCst) {
            return;
        }

        let track = self.current_track.read().await.clone();
        if self.loaded_intent.load(Ordering::SeqCst) != intent {
            self.recovering.store(false, Ordering::SeqCst);
            return;
        }
        if let Some(track) = track {
            let position = load_f64(&self.last_position);
            let resolved = self.resolver.resolve_audio(&track.id).await;
            let _effects = self.effects.lock().await;
            if !self.owns(intent) {
                self.recovering.store(false, Ordering::SeqCst);
                return;
            }
            match resolved {
                Ok(url) => match self.controller.mpv.load(&url).await {
                    Ok(()) => {
                        if !self.owns(intent) {
                            self.recovering.store(false, Ordering::SeqCst);
                            return;
                        }
                        let _ = self.apply_desired_output().await;
                        if position > 1.0 {
                            let _ = self.controller.seek(position).await;
                        }
                        let paused = self.desired_paused.load(Ordering::SeqCst);
                        let _ = if paused {
                            self.controller.pause().await
                        } else {
                            self.controller.resume().await
                        };
                        if !self.owns(intent) {
                            self.recovering.store(false, Ordering::SeqCst);
                            return;
                        }
                        self.set_mode(if paused {
                            PlayerMode::Paused
                        } else {
                            PlayerMode::Playing
                        })
                        .await;
                        tracing::info!(
                            video_id = %track.id,
                            position,
                            "playback restored after reconnect"
                        );
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "could not reload track after reconnect");
                    }
                },
                Err(err) => {
                    tracing::warn!(error = %err, "could not resolve track after reconnect");
                }
            }
        } else {
            let _effects = self.effects.lock().await;
            if self.owns(intent) {
                let _ = self.apply_desired_output().await;
            }
        }

        self.recovering.store(false, Ordering::SeqCst);
    }

    async fn apply_desired_output(&self) -> Result<(), CadenceError> {
        self.controller
            .set_volume(load_f64(&self.desired_volume))
            .await?;
        self.controller
            .set_loop(self.desired_loop.load(Ordering::SeqCst))
            .await?;
        let preset = *self.desired_audio_preset.read().await;
        let _ = self.apply_preset_filter(preset).await;
        let speed = load_f64(&self.desired_speed);
        if (speed - 1.0).abs() > f64::EPSILON {
            let _ = self.controller.set_speed(speed).await;
        }
        Ok(())
    }

    async fn set_mode(&self, new_mode: PlayerMode) {
        let mut mode = self.mode.write().await;
        if *mode != new_mode {
            *mode = new_mode;
            let _ = self
                .event_tx
                .send(OrchestratorEvent::StateChanged(new_mode));
        }
    }

    /// Carga y reproduce un track.
    pub async fn play(&self, track: SearchResult) -> Result<(), CadenceError> {
        self.play_entry(track, None).await
    }

    async fn play_entry(
        &self,
        track: SearchResult,
        entry_id: Option<String>,
    ) -> Result<(), CadenceError> {
        self.play_entry_owned(track, entry_id, None).await
    }

    async fn play_entry_owned(
        &self,
        track: SearchResult,
        entry_id: Option<String>,
        expected: Option<u64>,
    ) -> Result<(), CadenceError> {
        let Some(intent) = self.begin_intent(expected) else {
            return Ok(());
        };
        {
            let _effects = self.effects.lock().await;
            if !self.owns(intent) {
                return Ok(());
            }
            *self.pending_queue_entry.write().await = entry_id.clone().map(|id| (intent, id));
            self.set_mode(PlayerMode::Loading).await;
        }

        let audio_url = match self.resolver.resolve_audio(&track.id).await {
            Ok(url) => url,
            Err(err) => {
                let _effects = self.effects.lock().await;
                if !self.owns(intent) {
                    return Ok(());
                }
                *self.pending_queue_entry.write().await = None;
                self.set_mode(PlayerMode::Stopped).await;
                return Err(err);
            }
        };

        let _effects = self.effects.lock().await;
        if !self.owns(intent) {
            return Ok(());
        }
        // Mantener sincronizada la cola de reproducción
        let selected = {
            let queue = self.queue.read().await;
            queue
                .iter()
                .find(|item| match &entry_id {
                    Some(id) => item.id == *id,
                    None => item.track.id == track.id,
                })
                .cloned()
        };
        let selected = match selected {
            Some(item) => item,
            None if entry_id.is_none() => self.add_to_queue(track.clone()).await,
            None => {
                *self.pending_queue_entry.write().await = None;
                self.restore_loaded_mode(intent).await;
                return Ok(());
            }
        };
        *self.pending_queue_entry.write().await = None;
        {
            let queue = self.queue.read().await;
            *self.current_queue_index.write().await =
                queue.iter().position(|i| i.id == selected.id);
            self.queue_gap.store(false, Ordering::SeqCst);
        }

        *self.current_track.write().await = Some(track.clone());
        store_f64(&self.last_position, 0.0);
        self.desired_paused.store(false, Ordering::SeqCst);

        // `loadfile` reemplaza el archivo actual; no hace falta stop previo
        // (un stop extra generaría end-file y podría disparar una recuperación).
        self.loaded_intent.store(intent, Ordering::SeqCst);
        let loaded = self.controller.mpv.load(&audio_url).await;
        if !self.owns(intent) {
            return Ok(());
        }
        if let Err(err) = loaded {
            *self.current_track.write().await = None;
            self.set_mode(PlayerMode::Stopped).await;
            return Err(err);
        }

        if let Err(err) = self.apply_desired_output().await {
            tracing::warn!(error = %err, "failed to apply volume/loop on play");
        }

        if !self.owns(intent) {
            return Ok(());
        }
        // Formatear título limpio en mpv (para MPRIS, controles de medios en Windows y lockscreens)
        let display_title = if let Some(ref artist) = track.artist {
            format!("{} - {}", artist, track.title)
        } else {
            track.title.clone()
        };
        let _ = self.controller.set_media_title(&display_title).await;

        if !self.owns(intent) {
            return Ok(());
        }
        self.set_mode(PlayerMode::Playing).await;
        let _ = self
            .event_tx
            .send(OrchestratorEvent::TrackStarted(track.clone()));

        // Auto-fetch de letras en segundo plano (fire-and-forget, nunca bloquea el playback)
        if let Some(ref lyrics) = self.lyrics {
            let req = crate::models::lyrics::LyricsRequest::from_track(&track);
            lyrics.request(req);
        }

        // Queue Prefetch: Resolver el audio del siguiente tema en segundo plano
        self.prefetch_next_track().await;

        Ok(())
    }

    /// Cancel a selection without touching the audio that is already loaded.
    /// The caller holds `effects` and owns this intent.
    async fn restore_loaded_mode(&self, intent: u64) {
        self.loaded_intent.store(intent, Ordering::SeqCst);
        let mode = if self.current_track.read().await.is_none() {
            PlayerMode::Stopped
        } else if self.desired_paused.load(Ordering::SeqCst) {
            PlayerMode::Paused
        } else {
            PlayerMode::Playing
        };
        self.set_mode(mode).await;
    }

    fn begin_intent(&self, expected: Option<u64>) -> Option<u64> {
        match expected {
            Some(old) => self
                .intent
                .compare_exchange(old, old + 1, Ordering::SeqCst, Ordering::SeqCst)
                .ok()
                .map(|v| v + 1),
            None => Some(self.intent.fetch_add(1, Ordering::SeqCst) + 1),
        }
    }

    fn owns(&self, intent: u64) -> bool {
        self.intent.load(Ordering::SeqCst) == intent
    }

    /// Pausa la reproducción.
    pub async fn pause(&self) -> Result<(), CadenceError> {
        self.desired_paused.store(true, Ordering::SeqCst);
        self.controller.pause().await
    }

    /// Reanuda la reproducción.
    pub async fn resume(&self) -> Result<(), CadenceError> {
        self.desired_paused.store(false, Ordering::SeqCst);
        self.controller.resume().await
    }

    /// Detiene la reproducción.
    pub async fn stop(&self) -> Result<(), CadenceError> {
        self.stop_owned(None).await
    }

    async fn stop_owned(&self, expected: Option<u64>) -> Result<(), CadenceError> {
        let Some(intent) = self.begin_intent(expected) else {
            return Ok(());
        };
        let _effects = self.effects.lock().await;
        if !self.owns(intent) {
            return Ok(());
        }
        *self.pending_queue_entry.write().await = None;
        self.desired_paused.store(false, Ordering::SeqCst);
        store_f64(&self.last_position, 0.0);
        *self.current_track.write().await = None;
        self.controller.stop().await?;
        self.set_mode(PlayerMode::Stopped).await;
        Ok(())
    }

    pub async fn set_volume(&self, level: f64) -> Result<(), CadenceError> {
        let level = level.clamp(0.0, 100.0);
        store_f64(&self.desired_volume, level);
        self.controller.set_volume(level).await
    }

    pub async fn seek(&self, position_seconds: f64) -> Result<(), CadenceError> {
        let intent = self.intent.load(Ordering::SeqCst);
        let position = position_seconds.max(0.0);
        store_f64(&self.last_position, position);
        match self.controller.seek(position).await {
            Ok(()) => Ok(()),
            Err(err) => {
                // Si mpv perdió el stream (URL expirada), intentamos recuperar
                // la pista en la posición solicitada antes de rendirnos.
                let track = self.current_track.read().await.clone();
                if let Some(track) = track {
                    self.recover_track(track, position, intent).await;
                    if self.current_track.read().await.is_some() {
                        return Ok(());
                    }
                }
                Err(err)
            }
        }
    }

    pub async fn set_loop(&self, enabled: bool) -> Result<(), CadenceError> {
        self.desired_loop.store(enabled, Ordering::SeqCst);
        self.controller.set_loop(enabled).await
    }

    /// Salta al siguiente tema en la cola.
    pub async fn next(&self) -> Result<(), CadenceError> {
        self.next_owned(None).await
    }

    async fn next_owned(&self, expected: Option<u64>) -> Result<(), CadenceError> {
        let next_track = {
            let queue = self.queue.read().await;
            if queue.is_empty() {
                None
            } else {
                let current_idx = self.current_queue_index.read().await;
                let pending = self.pending_queue_entry.read().await;
                let pending_idx = pending
                    .as_ref()
                    .and_then(|(_, id)| queue.iter().position(|item| item.id == *id));
                let next_idx = match pending_idx.or(*current_idx) {
                    Some(idx) => {
                        let candidate =
                            if pending_idx.is_none() && self.queue_gap.load(Ordering::SeqCst) {
                                idx
                            } else {
                                idx + 1
                            };
                        if candidate < queue.len() {
                            candidate
                        } else if self.desired_loop.load(Ordering::SeqCst) {
                            0
                        } else {
                            drop(pending);
                            drop(current_idx);
                            drop(queue);
                            return self.stop_owned(expected).await;
                        }
                    }
                    None => 0,
                };
                Some(queue[next_idx].clone())
            }
        };

        if let Some(item) = next_track {
            self.play_entry_owned(item.track, Some(item.id), expected)
                .await
        } else {
            self.stop_owned(expected).await
        }
    }

    /// Retrocede al tema anterior en la cola o reinicia si lleva más de 3 segundos.
    pub async fn previous(&self) -> Result<(), CadenceError> {
        let position = load_f64(&self.last_position);
        if position > 3.0 {
            return self.seek(0.0).await;
        }

        let prev_track = {
            let queue = self.queue.read().await;
            if queue.is_empty() {
                None
            } else {
                let current_idx = self.current_queue_index.read().await;
                let prev_idx = match *current_idx {
                    Some(idx) if idx > 0 => idx - 1,
                    Some(_) | None => 0,
                };
                Some(queue[prev_idx].clone())
            }
        };

        if let Some(item) = prev_track {
            self.play_entry(item.track, Some(item.id)).await
        } else {
            self.seek(0.0).await
        }
    }

    pub async fn get_queue(&self) -> Vec<QueueItem> {
        self.queue.read().await.clone()
    }

    pub async fn add_to_queue(&self, track: SearchResult) -> QueueItem {
        let mut queue = self.queue.write().await;
        let counter = self.queue_id_counter.fetch_add(1, Ordering::SeqCst);
        let id = format!(
            "q_{}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            counter
        );
        let item = QueueItem { id, track };
        queue.push(item.clone());
        let _ = self
            .event_tx
            .send(OrchestratorEvent::QueueUpdated(queue.clone()));
        drop(queue);
        self.prefetch_next_track().await;
        item
    }

    pub async fn add_to_queue_next(&self, track: SearchResult) -> QueueItem {
        let mut queue = self.queue.write().await;
        let counter = self.queue_id_counter.fetch_add(1, Ordering::SeqCst);
        let id = format!(
            "q_{}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            counter
        );
        let item = QueueItem { id, track };
        let insert_idx = match *self.current_queue_index.read().await {
            Some(idx) if self.queue_gap.load(Ordering::SeqCst) => idx.min(queue.len()),
            Some(idx) => (idx + 1).min(queue.len()),
            None => queue.len(),
        };
        queue.insert(insert_idx, item.clone());
        let _ = self
            .event_tx
            .send(OrchestratorEvent::QueueUpdated(queue.clone()));
        drop(queue);
        self.prefetch_next_track().await;
        item
    }

    /// Precarga la URL del siguiente tema en segundo plano si existe en la cola.
    pub async fn prefetch_next_track(&self) {
        let (next_track_id, next_track) = {
            let queue = self.queue.read().await;
            let current_idx = *self.current_queue_index.read().await;
            if let Some(idx) = current_idx {
                let next = if self.queue_gap.load(Ordering::SeqCst) {
                    idx
                } else {
                    idx + 1
                };
                if next < queue.len() {
                    let t = queue[next].track.clone();
                    (Some(t.id.clone()), Some(t))
                } else if self.desired_loop.load(Ordering::SeqCst) && !queue.is_empty() {
                    let t = queue[0].track.clone();
                    (Some(t.id.clone()), Some(t))
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            }
        };

        if let Some(next_id) = next_track_id {
            if !(next_id.starts_with("local:")
                || next_id.starts_with("file://")
                || std::path::Path::new(&next_id).is_file())
            {
                let resolver = self.resolver.clone();
                tauri::async_runtime::spawn(async move {
                    tracing::debug!(next_id = %next_id, "[prefetch] pre-resolving next track audio stream");
                    let _ = resolver.resolve_audio(&next_id).await;
                });
            }
        }

        if let (Some(next_track), Some(lyrics)) = (next_track, &self.lyrics) {
            let req = crate::models::lyrics::LyricsRequest::from_track(&next_track);
            lyrics.prefetch(req);
        }
    }

    pub async fn remove_from_queue(&self, item_id: &str) -> bool {
        let _effects = self.effects.lock().await;
        let mut queue = self.queue.write().await;
        if let Some(pos) = queue.iter().position(|i| i.id == item_id) {
            queue.remove(pos);
            let pending = self.pending_queue_entry.read().await.clone();
            if let Some((pending_intent, id)) = pending {
                if id == item_id {
                    *self.pending_queue_entry.write().await = None;
                    if let Some(intent) = self.begin_intent(Some(pending_intent)) {
                        self.restore_loaded_mode(intent).await;
                    }
                }
            }
            let mut current_idx = self.current_queue_index.write().await;
            if let Some(idx) = *current_idx {
                if pos < idx {
                    *current_idx = Some(idx.saturating_sub(1));
                } else if pos == idx {
                    self.queue_gap.store(true, Ordering::SeqCst);
                }
            }
            let _ = self
                .event_tx
                .send(OrchestratorEvent::QueueUpdated(queue.clone()));
            true
        } else {
            false
        }
    }

    pub async fn clear_queue(&self) {
        let mut queue = self.queue.write().await;
        queue.clear();
        *self.current_queue_index.write().await = None;
        self.queue_gap.store(false, Ordering::SeqCst);
        let _ = self.event_tx.send(OrchestratorEvent::QueueUpdated(vec![]));
    }

    pub async fn play_queue_index(&self, index: usize) -> Result<(), CadenceError> {
        let track = {
            let queue = self.queue.read().await;
            if index >= queue.len() {
                return Err(CadenceError::QueueError {
                    message: format!("Queue index {} out of bounds", index),
                });
            }
            queue[index].clone()
        };
        self.play_entry(track.track, Some(track.id)).await
    }

    /// Obtiene estado completo para el frontend.
    pub async fn get_state(&self) -> PlayerState {
        let mpv_status = self.controller.status();
        let current = self.current_track.read().await.clone();

        let status = match (&current, mpv_status.paused) {
            (None, _) => PlaybackStatus::Stopped,
            (Some(_), true) => PlaybackStatus::Paused,
            (Some(_), false) => PlaybackStatus::Playing,
        };

        let position_seconds = if current.is_some() {
            mpv_status
                .position_seconds
                .filter(|position| position.is_finite() && *position >= 0.0)
                .unwrap_or_else(|| load_f64(&self.last_position))
        } else {
            0.0
        };

        let duration_seconds = mpv_status
            .duration_seconds
            .filter(|duration| duration.is_finite() && *duration > 0.0)
            .unwrap_or(0.0);

        PlayerState {
            status,
            current,
            position_seconds,
            duration_seconds,
            volume: load_f64(&self.desired_volume),
            looping: self.desired_loop.load(Ordering::SeqCst),
            connected: mpv_status.connected,
            audio_preset: *self.desired_audio_preset.read().await,
            speed: load_f64(&self.desired_speed),
        }
    }

    pub async fn set_audio_preset(
        &self,
        preset: crate::models::player_state::AudioPreset,
    ) -> Result<(), CadenceError> {
        *self.desired_audio_preset.write().await = preset;
        self.apply_preset_filter(preset).await?;
        let _ = self
            .event_tx
            .send(OrchestratorEvent::AudioPresetChanged(preset));
        Ok(())
    }

    pub async fn set_speed(&self, speed: f64) -> Result<(), CadenceError> {
        let speed = speed.clamp(0.5, 2.0);
        store_f64(&self.desired_speed, speed);
        self.controller.set_speed(speed).await?;
        let _ = self.event_tx.send(OrchestratorEvent::SpeedChanged(speed));
        Ok(())
    }

    pub async fn get_audio_preset(&self) -> crate::models::player_state::AudioPreset {
        *self.desired_audio_preset.read().await
    }

    pub fn get_speed(&self) -> f64 {
        load_f64(&self.desired_speed)
    }

    async fn apply_preset_filter(
        &self,
        preset: crate::models::player_state::AudioPreset,
    ) -> Result<(), CadenceError> {
        use crate::models::player_state::AudioPreset;
        let filter = match preset {
            AudioPreset::Normal => "",
            AudioPreset::Lofi => "lavfi=[highpass=f=200,lowpass=f=4000]",
            AudioPreset::BassBoost => "lavfi=[equalizer=f=60:width_type=h:width=50:g=10]",
            AudioPreset::Loudnorm => "lavfi=[loudnorm=I=-16:TP=-1.5:LRA=11]",
            AudioPreset::SlowedReverb => "lavfi=[aecho=0.8:0.88:60:0.4]",
            AudioPreset::Nightcore => "lavfi=[asetrate=48000*1.2,atempo=1.05]",
        };
        self.controller.set_audio_filter(filter).await
    }

    /// Suscribe a eventos del orquestador.
    pub fn subscribe_events(&self) -> broadcast::Receiver<OrchestratorEvent> {
        self.event_tx.subscribe()
    }

    /// Suscripción idempotente: solo el primer llamador recibe el receiver.
    /// Evita registrar N tareas emisoras si el frontend re-monta.
    pub fn take_events_subscription(&self) -> Option<broadcast::Receiver<OrchestratorEvent>> {
        if self.events_subscribed.swap(true, Ordering::SeqCst) {
            None
        } else {
            Some(self.event_tx.subscribe())
        }
    }

    pub fn is_connected(&self) -> bool {
        self.controller.status().connected
    }

    /// Estado crudo de mpv (diagnóstico y tests).
    pub fn mpv_status(&self) -> MpvStatus {
        self.controller.status()
    }

    /// Busca tracks (delegado al resolver).
    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<SearchResult>, CadenceError> {
        self.resolver.search(query, limit).await
    }

    /// Apaga el orquestador y mpv.
    pub async fn shutdown(&self) {
        let _ = self.stop().await;
        self.controller.mpv.shutdown().await;
    }
}

/// Builder para crear el orquestador con todas sus dependencias.
pub struct PlayerOrchestratorBuilder {
    mpv_socket_path: std::path::PathBuf,
    lyrics: Option<Arc<crate::services::lyrics::LyricsService>>,
}

impl PlayerOrchestratorBuilder {
    pub fn new(socket_path: std::path::PathBuf) -> Self {
        Self {
            mpv_socket_path: socket_path,
            lyrics: None,
        }
    }

    pub fn with_lyrics(mut self, lyrics: Arc<crate::services::lyrics::LyricsService>) -> Self {
        self.lyrics = Some(lyrics);
        self
    }

    pub async fn build(self) -> Result<Arc<PlayerOrchestrator>, CadenceError> {
        let mpv = Arc::new(MpvPlayer::connect(self.mpv_socket_path).await?);
        let yt_dlp = Arc::new(YtDlpService::new());

        let controller = PlaybackController::new(mpv.clone());
        let resolver = TrackResolver::new(yt_dlp);

        let mpv_events = mpv.subscribe_events();
        let mpv_status = mpv.subscribe_status();

        let orchestrator = Arc::new(PlayerOrchestrator::new(
            controller,
            resolver,
            mpv_events,
            mpv_status,
            self.lyrics,
        ));

        orchestrator.clone().spawn_event_loop();
        orchestrator.clone().spawn_progress_ticker();

        Ok(orchestrator)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn duplicate_gap_and_rejection_boundaries() {
        let player = fixture();
        let first = player.add_to_queue(track("first")).await;
        let active = player.add_to_queue(track("active")).await;
        player.add_to_queue(track("successor")).await;
        player.play_queue_index(1).await.unwrap();

        let before = player.get_queue().await;
        let error = player.play_queue_index(3).await.unwrap_err();
        assert!(matches!(error, CadenceError::QueueError { message }
            if message == "Queue index 3 out of bounds"));
        assert_eq!(player.get_queue().await, before);
        assert_eq!(player.get_state().await.current.unwrap().title, "active");

        assert!(player.remove_from_queue(&active.id).await);
        assert!(player.remove_from_queue(&first.id).await);
        assert_eq!(player.get_state().await.current.unwrap().title, "active");
        player.next().await.unwrap();
        assert_eq!(player.get_state().await.current.unwrap().title, "successor");

        let last = player.get_queue().await[0].id.clone();
        assert!(player.remove_from_queue(&last).await);
        assert_eq!(player.get_state().await.current.unwrap().title, "successor");
        player.next().await.unwrap();
        assert!(player.get_state().await.current.is_none());
        assert!(player.get_queue().await.is_empty());
    }

    fn fixture() -> Arc<PlayerOrchestrator> {
        let mpv = Arc::new(MpvPlayer::fake());
        Arc::new(PlayerOrchestrator::new(
            PlaybackController::new(mpv.clone()),
            TrackResolver::new(Arc::new(YtDlpService::new())),
            mpv.subscribe_events(),
            mpv.subscribe_status(),
            None,
        ))
    }

    fn track(title: &str) -> SearchResult {
        SearchResult {
            id: "local:/fake.wav".into(),
            title: title.into(),
            artist: None,
            duration_seconds: 10,
            thumbnail_url: String::new(),
        }
    }

    #[tokio::test]
    async fn duplicates_navigate_by_entry_and_previous_is_preserved() {
        let player = fixture();
        let a = player.add_to_queue(track("first")).await;
        let b = player.add_to_queue(track("second")).await;
        player.add_to_queue(track("third")).await;
        assert_ne!(a.id, b.id);
        player.play_queue_index(1).await.unwrap();
        player.next().await.unwrap();
        assert_eq!(player.get_state().await.current.unwrap().title, "third");
        player.previous().await.unwrap();
        assert_eq!(player.get_state().await.current.unwrap().title, "second");
    }

    #[tokio::test]
    async fn removing_active_keeps_audio_and_next_uses_former_successor() {
        let player = fixture();
        let a = player.add_to_queue(track("first")).await;
        player.add_to_queue(track("second")).await;
        player.add_to_queue(track("third")).await;
        player.play_queue_index(0).await.unwrap();
        assert!(player.remove_from_queue(&a.id).await);
        assert_eq!(player.get_state().await.current.unwrap().title, "first");
        player.next().await.unwrap();
        assert_eq!(player.get_state().await.current.unwrap().title, "second");
        player.stop().await.unwrap();
        assert!(player.get_state().await.current.is_none());
        assert_eq!(player.get_queue().await.len(), 2);
    }

    #[tokio::test]
    async fn slow_play_cannot_override_new_play_or_stop() {
        for stop in [false, true] {
            let mut player = fixture();
            let gate = Arc::new(ResolutionGate {
                entered: tokio::sync::Notify::new(),
                release: tokio::sync::Semaphore::new(0),
            });
            Arc::get_mut(&mut player).unwrap().resolver.gate = Some(gate.clone());
            let old = player.clone();
            let pending = tokio::spawn(async move {
                let mut t = track("old");
                t.id = "local:/blocked.wav".into();
                old.play(t).await.unwrap();
            });
            gate.entered.notified().await;
            if stop {
                player.stop().await.unwrap();
            } else {
                player.play(track("new")).await.unwrap();
            }
            gate.release.add_permits(1);
            pending.await.unwrap();
            let state = player.get_state().await;
            assert_eq!(
                state.current.map(|t| t.title),
                if stop { None } else { Some("new".into()) }
            );
            assert_eq!(player.get_queue().await.len(), usize::from(!stop));
        }
    }

    #[tokio::test]
    async fn slow_recovery_cannot_override_new_play_or_stop() {
        for stop in [false, true] {
            let mut player = fixture();
            let gate = Arc::new(ResolutionGate {
                entered: tokio::sync::Notify::new(),
                release: tokio::sync::Semaphore::new(0),
            });
            Arc::get_mut(&mut player).unwrap().resolver.gate = Some(gate.clone());
            player.play(track("original")).await.unwrap();
            let intent = player.intent.load(Ordering::SeqCst);
            let old = player.clone();
            let pending = tokio::spawn(async move {
                let mut t = track("old");
                t.id = "local:/blocked.wav".into();
                old.recover_track(t, 8.0, intent).await;
            });
            gate.entered.notified().await;
            if stop {
                player.stop().await.unwrap();
            } else {
                player.play(track("new")).await.unwrap();
            }
            gate.release.add_permits(1);
            pending.await.unwrap();
            assert_eq!(
                player.get_state().await.current.map(|t| t.title),
                if stop { None } else { Some("new".into()) }
            );
        }
    }

    #[tokio::test]
    async fn direct_play_enqueues_and_queue_edits_preserve_navigation() {
        let player = fixture();
        player.play(track("direct")).await.unwrap();
        assert_eq!(player.get_queue().await.len(), 1);
        let next = player.add_to_queue_next(track("inserted")).await;
        player.next().await.unwrap();
        assert_eq!(player.get_state().await.current.unwrap().title, "inserted");
        assert!(!player.remove_from_queue("missing").await);
        assert!(player.remove_from_queue(&next.id).await);
        player.set_loop(true).await.unwrap();
        player.next().await.unwrap();
        assert_eq!(player.get_state().await.current.unwrap().title, "direct");
        player.clear_queue().await;
        assert!(player.get_queue().await.is_empty());
        assert!(player.get_state().await.current.is_some());
        player.next().await.unwrap();
        assert!(player.get_state().await.current.is_none());
        assert!(player.get_state().await.looping);
    }

    #[tokio::test]
    async fn stop_serializes_after_an_already_sent_load() {
        let (mpv, mut commands) = MpvPlayer::controlled_fake();
        let mpv = Arc::new(mpv);
        let player = Arc::new(PlayerOrchestrator::new(
            PlaybackController::new(mpv.clone()),
            TrackResolver::new(Arc::new(YtDlpService::new())),
            mpv.subscribe_events(),
            mpv.subscribe_status(),
            None,
        ));
        let mut events = player.subscribe_events();
        let old = player.clone();
        let playing = tokio::spawn(async move {
            old.play(track("old")).await.unwrap();
        });
        let (command, load_reply) = commands.recv().await.unwrap();
        assert_eq!(command, "load:/fake.wav");
        let stopping = player.stop();
        tokio::pin!(stopping);
        tokio::select! {
            biased;
            _ = &mut stopping => panic!("stop must wait for in-flight load"),
            _ = std::future::ready(()) => {}
        }
        assert!(commands.try_recv().is_err());
        load_reply.send(Ok(())).unwrap();
        let acknowledge = async {
            while let Some((command, reply)) = commands.recv().await {
                let stopped = command == "stop";
                reply.send(Ok(())).unwrap();
                if stopped {
                    break;
                }
            }
        };
        let (result, ()) = tokio::join!(stopping, acknowledge);
        result.unwrap();
        playing.await.unwrap();
        while let Ok(event) = events.try_recv() {
            assert!(!matches!(event, OrchestratorEvent::TrackStarted(_)));
        }
        assert!(commands.try_recv().is_err());
        assert!(player.get_state().await.current.is_none());
    }

    #[tokio::test]
    async fn eof_advance_cannot_supersede_a_newer_user_intent() {
        let player = fixture();
        player.play(track("old")).await.unwrap();
        let ended_intent = player.intent.load(Ordering::SeqCst);
        player.add_to_queue(track("successor")).await;
        player.play(track("new")).await.unwrap();
        player.next_owned(Some(ended_intent)).await.unwrap();
        assert_eq!(player.get_state().await.current.unwrap().title, "new");
        player.stop().await.unwrap();
        player.next_owned(Some(ended_intent)).await.unwrap();
        assert!(player.get_state().await.current.is_none());
    }

    #[tokio::test]
    async fn playback_preserves_output_preferences_and_previous_restart() {
        use crate::models::player_state::AudioPreset;
        let player = fixture();
        player.set_volume(37.0).await.unwrap();
        player.set_loop(true).await.unwrap();
        player.set_speed(1.25).await.unwrap();
        player
            .set_audio_preset(AudioPreset::BassBoost)
            .await
            .unwrap();
        player.play(track("first")).await.unwrap();
        player.add_to_queue(track("second")).await;
        player.next().await.unwrap();
        player.seek(5.0).await.unwrap();
        player.previous().await.unwrap();
        let state = player.get_state().await;
        assert_eq!(state.current.unwrap().title, "second");
        assert_eq!(state.position_seconds, 0.0);
        assert_eq!(state.volume, 37.0);
        assert!(state.looping);
        assert_eq!(state.speed, 1.25);
        assert_eq!(state.audio_preset, AudioPreset::BassBoost);
    }

    fn gated_fixture() -> (Arc<PlayerOrchestrator>, Arc<ResolutionGate>) {
        let mut player = fixture();
        let gate = Arc::new(ResolutionGate {
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Semaphore::new(0),
        });
        Arc::get_mut(&mut player).unwrap().resolver.gate = Some(gate.clone());
        (player, gate)
    }

    fn last_mode(events: &mut broadcast::Receiver<OrchestratorEvent>) -> Option<PlayerMode> {
        let mut mode = None;
        while let Ok(event) = events.try_recv() {
            match event {
                OrchestratorEvent::StateChanged(value) => mode = Some(value),
                OrchestratorEvent::TrackStarted(track) => {
                    assert_ne!(track.title, "pending", "removed selection must not start");
                }
                _ => {}
            }
        }
        mode
    }

    #[tokio::test]
    async fn removing_pending_selection_restores_loaded_or_idle_mode() {
        for expected in [PlayerMode::Playing, PlayerMode::Paused, PlayerMode::Stopped] {
            let (player, gate) = gated_fixture();
            if expected != PlayerMode::Stopped {
                player.play(track("original")).await.unwrap();
                if expected == PlayerMode::Paused {
                    player.pause().await.unwrap();
                }
            }
            let mut blocked = track("pending");
            blocked.id = "local:/blocked.wav".into();
            let item = player.add_to_queue(blocked).await;
            let index = player.get_queue().await.len() - 1;
            let mut events = player.subscribe_events();
            let old = player.clone();
            let pending = tokio::spawn(async move { old.play_queue_index(index).await });
            gate.entered.notified().await;
            assert_eq!(last_mode(&mut events), Some(PlayerMode::Loading));
            assert!(player.remove_from_queue(&item.id).await);
            let restored = last_mode(&mut events);
            gate.release.add_permits(1);
            pending.await.unwrap().unwrap();
            assert_eq!(
                restored,
                Some(expected),
                "removal must restore mode immediately"
            );
            assert_eq!(
                last_mode(&mut events),
                None,
                "stale resolution must stay silent"
            );
            assert_eq!(
                player.get_state().await.current.map(|t| t.title),
                if expected == PlayerMode::Stopped {
                    None
                } else {
                    Some("original".into())
                }
            );
            assert!(!player
                .get_queue()
                .await
                .iter()
                .any(|entry| entry.id == item.id));
        }
    }

    #[tokio::test]
    async fn removed_pending_completion_cannot_override_successor_or_stop() {
        for stop in [false, true] {
            let (player, gate) = gated_fixture();
            player.play(track("original")).await.unwrap();
            let mut blocked = track("pending");
            blocked.id = "local:/blocked.wav".into();
            let item = player.add_to_queue(blocked).await;
            player.add_to_queue(track("successor")).await;
            let old = player.clone();
            let pending = tokio::spawn(async move { old.next().await });
            gate.entered.notified().await;
            assert!(player.remove_from_queue(&item.id).await);
            if stop {
                player.stop().await.unwrap();
            } else {
                player.next().await.unwrap();
            }
            let mut events = player.subscribe_events();
            gate.release.add_permits(1);
            pending.await.unwrap().unwrap();
            assert_eq!(last_mode(&mut events), None);
            assert_eq!(
                player.get_state().await.current.map(|t| t.title),
                if stop { None } else { Some("successor".into()) }
            );
            assert_eq!(player.get_queue().await.len(), 2);
        }
    }

    #[tokio::test]
    async fn repeated_next_advances_past_the_pending_entry() {
        let (player, gate) = gated_fixture();
        player.play(track("original")).await.unwrap();
        let mut blocked = track("pending");
        blocked.id = "local:/blocked.wav".into();
        player.add_to_queue(blocked).await;
        player.add_to_queue(track("successor")).await;
        let old = player.clone();
        let first = tokio::spawn(async move { old.next().await });
        gate.entered.notified().await;
        let mut events = player.subscribe_events();
        let second = player.next();
        tokio::pin!(second);
        let finished = tokio::select! {
            result = &mut second => Some(result),
            _ = gate.entered.notified() => None,
        };
        gate.release.add_permits(2);
        match finished {
            Some(result) => result.unwrap(),
            None => second.await.unwrap(),
        }
        first.await.unwrap().unwrap();
        assert_eq!(player.get_state().await.current.unwrap().title, "successor");
        assert_eq!(last_mode(&mut events), Some(PlayerMode::Playing));
        assert_eq!(player.get_queue().await.len(), 3);
    }

    #[test]
    fn f64_atomics_round_trip() {
        let atomic = AtomicU64::new(0);
        store_f64(&atomic, 42.5);
        assert!((load_f64(&atomic) - 42.5).abs() < f64::EPSILON);
    }

    #[test]
    fn events_serialize_with_type_and_data() {
        let ended = serde_json::to_value(OrchestratorEvent::TrackEnded {
            reason: "eof".to_string(),
        })
        .expect("serialize TrackEnded");
        assert_eq!(ended["type"], "TrackEnded");
        assert_eq!(ended["data"]["reason"], "eof");

        let connection =
            serde_json::to_value(OrchestratorEvent::ConnectionChanged { connected: false })
                .expect("serialize ConnectionChanged");
        assert_eq!(connection["type"], "ConnectionChanged");
        assert_eq!(connection["data"]["connected"], false);

        let state = serde_json::to_value(OrchestratorEvent::StateChanged(PlayerMode::Paused))
            .expect("serialize StateChanged");
        assert_eq!(state["data"], "Paused");

        let progress = serde_json::to_value(OrchestratorEvent::Progress {
            position_seconds: 12.5,
            duration_seconds: 187.0,
        })
        .expect("serialize Progress");
        assert_eq!(progress["type"], "Progress");
        assert_eq!(progress["data"]["position_seconds"], 12.5);
        assert_eq!(progress["data"]["duration_seconds"], 187.0);
    }

    #[test]
    fn bounded_retry_budget_is_small() {
        assert_eq!(MAX_URL_RETRIES, 2);
        assert!(URL_RETRY_DELAY > Duration::ZERO);
    }

    #[test]
    fn aborted_end_file_reasons_are_ignored() {
        // `loadfile` aborta el track anterior con "stop"; si lo tratáramos
        // como fin real, borraríamos el track recién cargado.
        assert!(is_abort_reason("stop"));
        assert!(is_abort_reason("quit"));
        assert!(!is_abort_reason("eof"));
        assert!(!is_abort_reason("error"));
    }
}
