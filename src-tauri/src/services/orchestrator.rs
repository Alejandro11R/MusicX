use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, watch, RwLock};
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
}

impl TrackResolver {
    pub fn new(yt_dlp: Arc<YtDlpService>) -> Self {
        Self { yt_dlp }
    }

    pub async fn resolve_audio(&self, video_id: &str) -> Result<String, CadenceError> {
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
    TrackEnded { reason: String },
    PlaybackError(String),
    StateChanged(PlayerMode),
    ConnectionChanged { connected: bool },
    QueueUpdated(Vec<QueueItem>),
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
    queue_id_counter: AtomicU64,
    mpv_events: broadcast::Receiver<MpvEvent>,
    _mpv_status_rx: watch::Receiver<MpvStatus>,
}

impl PlayerOrchestrator {
    pub fn new(
        controller: PlaybackController,
        resolver: TrackResolver,
        mpv_events: broadcast::Receiver<MpvEvent>,
        mpv_status_rx: watch::Receiver<MpvStatus>,
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
            queue_id_counter: AtomicU64::new(1),
            mpv_events,
            _mpv_status_rx: mpv_status_rx,
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
                let _ = self.event_tx.send(OrchestratorEvent::ConnectionChanged {
                    connected: false,
                });
                tracing::warn!("mpv disconnected");
            }
            Reconnected => {
                let _ = self.event_tx.send(OrchestratorEvent::ConnectionChanged {
                    connected: true,
                });
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

        let failed = reason == "error" || error.is_some();
        let track = self.current_track.read().await.clone();

        if failed {
            tracing::warn!(reason = %reason, error = ?error, "mpv failed to play current track");
            if let Some(track) = track {
                let position = load_f64(&self.last_position);
                self.recover_track(track, position).await;
                return;
            }
        }

        tracing::info!(reason = %reason, "Track ended");
        if track.is_some() {
            let _ = self
                .event_tx
                .send(OrchestratorEvent::TrackEnded { reason: reason.clone() });
        }

        // Si el tema terminó normalmente y no estamos en bucle, intentamos auto-avanzar la cola
        if reason == "eof" && !self.desired_loop.load(Ordering::SeqCst) {
            if let Ok(()) = self.next().await {
                if self.current_track.read().await.is_some() {
                    return;
                }
            }
        }

        *self.current_track.write().await = None;
        self.desired_paused.store(false, Ordering::SeqCst);
        store_f64(&self.last_position, 0.0);
        self.set_mode(PlayerMode::Stopped).await;
    }

    /// Una URL firmada caducó o el stream falló: invalidamos caché, resolvemos
    /// de nuevo y reanudamos desde la última posición conocida.
    async fn recover_track(&self, track: SearchResult, position: f64) {
        if self
            .recovering
            .swap(true, Ordering::SeqCst)
        {
            return;
        }

        let mut last_error: Option<String> = None;
        for attempt in 0..=MAX_URL_RETRIES {
            if attempt > 0 {
                sleep(URL_RETRY_DELAY * attempt).await;
            }

            self.resolver.invalidate(&track.id);
            match self.resolver.resolve_audio(&track.id).await {
                Ok(url) => match self.controller.mpv.load(&url).await {
                    Ok(()) => {
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
        let message = last_error.unwrap_or_else(|| "Couldn't recover this track".to_string());
        let _ = self.event_tx.send(OrchestratorEvent::PlaybackError(message));
        *self.current_track.write().await = None;
        self.set_mode(PlayerMode::Stopped).await;
    }

    /// mpv se reinició: re-aplicamos volumen/loop y, si había un track,
    /// volvemos a cargarlo en la posición donde estaba.
    async fn restore_after_reconnect(&self) {
        if self.recovering.swap(true, Ordering::SeqCst) {
            return;
        }

        let track = self.current_track.read().await.clone();
        if let Some(track) = track {
            let position = load_f64(&self.last_position);
            match self.resolver.resolve_audio(&track.id).await {
                Ok(url) => match self.controller.mpv.load(&url).await {
                    Ok(()) => {
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
            let _ = self.apply_desired_output().await;
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
        Ok(())
    }

    async fn set_mode(&self, new_mode: PlayerMode) {
        let mut mode = self.mode.write().await;
        if *mode != new_mode {
            *mode = new_mode;
            let _ = self.event_tx.send(OrchestratorEvent::StateChanged(new_mode));
        }
    }

    /// Carga y reproduce un track.
    pub async fn play(&self, track: SearchResult) -> Result<(), CadenceError> {
        self.set_mode(PlayerMode::Loading).await;

        let audio_url = match self.resolver.resolve_audio(&track.id).await {
            Ok(url) => url,
            Err(err) => {
                self.set_mode(PlayerMode::Stopped).await;
                return Err(err);
            }
        };

        *self.current_track.write().await = Some(track.clone());
        store_f64(&self.last_position, 0.0);
        self.desired_paused.store(false, Ordering::SeqCst);

        // Mantener sincronizada la cola de reproducción
        {
            let mut queue = self.queue.write().await;
            let mut current_idx = self.current_queue_index.write().await;
            if let Some(pos) = queue.iter().position(|i| i.track.id == track.id) {
                *current_idx = Some(pos);
            } else {
                let counter = self.queue_id_counter.fetch_add(1, Ordering::SeqCst);
                let id = format!(
                    "q_{}_{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    counter
                );
                let item = QueueItem {
                    id,
                    track: track.clone(),
                };
                queue.push(item);
                *current_idx = Some(queue.len() - 1);
                let _ = self.event_tx.send(OrchestratorEvent::QueueUpdated(queue.clone()));
            }
        }

        // `loadfile` reemplaza el archivo actual; no hace falta stop previo
        // (un stop extra generaría end-file y podría disparar una recuperación).
        if let Err(err) = self.controller.mpv.load(&audio_url).await {
            *self.current_track.write().await = None;
            self.set_mode(PlayerMode::Stopped).await;
            return Err(err);
        }

        if let Err(err) = self.apply_desired_output().await {
            tracing::warn!(error = %err, "failed to apply volume/loop on play");
        }

        self.set_mode(PlayerMode::Playing).await;
        let _ = self.event_tx.send(OrchestratorEvent::TrackStarted(track));
        Ok(())
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
        let position = position_seconds.max(0.0);
        store_f64(&self.last_position, position);
        match self.controller.seek(position).await {
            Ok(()) => Ok(()),
            Err(err) => {
                // Si mpv perdió el stream (URL expirada), intentamos recuperar
                // la pista en la posición solicitada antes de rendirnos.
                let track = self.current_track.read().await.clone();
                if let Some(track) = track {
                    self.recover_track(track, position).await;
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
        let next_track = {
            let queue = self.queue.read().await;
            if queue.is_empty() {
                None
            } else {
                let mut current_idx = self.current_queue_index.write().await;
                let next_idx = match *current_idx {
                    Some(idx) => {
                        if idx + 1 < queue.len() {
                            idx + 1
                        } else if self.desired_loop.load(Ordering::SeqCst) {
                            0
                        } else {
                            return self.stop().await;
                        }
                    }
                    None => 0,
                };
                *current_idx = Some(next_idx);
                Some(queue[next_idx].track.clone())
            }
        };

        if let Some(track) = next_track {
            self.play(track).await
        } else {
            self.stop().await
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
                let mut current_idx = self.current_queue_index.write().await;
                let prev_idx = match *current_idx {
                    Some(idx) if idx > 0 => idx - 1,
                    Some(_) | None => 0,
                };
                *current_idx = Some(prev_idx);
                Some(queue[prev_idx].track.clone())
            }
        };

        if let Some(track) = prev_track {
            self.play(track).await
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
        let _ = self.event_tx.send(OrchestratorEvent::QueueUpdated(queue.clone()));
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
            Some(idx) => (idx + 1).min(queue.len()),
            None => queue.len(),
        };
        queue.insert(insert_idx, item.clone());
        let _ = self.event_tx.send(OrchestratorEvent::QueueUpdated(queue.clone()));
        item
    }

    pub async fn remove_from_queue(&self, item_id: &str) -> bool {
        let mut queue = self.queue.write().await;
        if let Some(pos) = queue.iter().position(|i| i.id == item_id) {
            queue.remove(pos);
            let mut current_idx = self.current_queue_index.write().await;
            if let Some(idx) = *current_idx {
                if pos < idx {
                    *current_idx = Some(idx.saturating_sub(1));
                } else if pos == idx && pos >= queue.len() {
                    *current_idx = if queue.is_empty() { None } else { Some(queue.len() - 1) };
                }
            }
            let _ = self.event_tx.send(OrchestratorEvent::QueueUpdated(queue.clone()));
            true
        } else {
            false
        }
    }

    pub async fn clear_queue(&self) {
        let mut queue = self.queue.write().await;
        queue.clear();
        *self.current_queue_index.write().await = None;
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
            *self.current_queue_index.write().await = Some(index);
            queue[index].track.clone()
        };
        self.play(track).await
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
        }
    }

    /// Suscribe a eventos del orquestador.
    pub fn subscribe_events(&self) -> broadcast::Receiver<OrchestratorEvent> {
        self.event_tx.subscribe()
    }

    /// Suscripción idempotente: solo el primer llamador recibe el receiver.
    /// Evita registrar N tareas emisoras si el frontend re-monta.
    pub fn take_events_subscription(
        &self,
    ) -> Option<broadcast::Receiver<OrchestratorEvent>> {
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
        let _ = self.controller.stop().await;
        self.controller.mpv.shutdown().await;
    }
}

/// Builder para crear el orquestador con todas sus dependencias.
pub struct PlayerOrchestratorBuilder {
    mpv_socket_path: std::path::PathBuf,
}

impl PlayerOrchestratorBuilder {
    pub fn new(socket_path: std::path::PathBuf) -> Self {
        Self {
            mpv_socket_path: socket_path,
        }
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

        let state =
            serde_json::to_value(OrchestratorEvent::StateChanged(PlayerMode::Paused))
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
