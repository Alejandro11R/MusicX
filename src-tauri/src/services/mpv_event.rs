use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::time::sleep;

use crate::errors::CadenceError;

const SOCKET_CONNECT_RETRY_DELAY: Duration = Duration::from_millis(100);
const SOCKET_CONNECT_MAX_ATTEMPTS: u32 = 50;
const CMD_CHANNEL_CAPACITY: usize = 32;
const EVENT_CHANNEL_CAPACITY: usize = 64;
const RECONNECT_BACKOFF_MIN: Duration = Duration::from_millis(250);
const RECONNECT_BACKOFF_MAX: Duration = Duration::from_secs(5);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

// IDs estables para `observe_property` (mpv los devuelve en `property-change`).
const OBS_PAUSE: u64 = 1;
const OBS_VOLUME: u64 = 2;
const OBS_TIME_POS: u64 = 3;
const OBS_DURATION: u64 = 4;
const OBS_LOOP_FILE: u64 = 5;

/// Eventos no solicitados que mpv emite por el socket IPC.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MpvEvent {
    /// La reproducción se pausó/reanudó.
    PauseChanged { paused: bool },
    /// Cambió la posición de reproducción (evento periódico).
    PositionChanged { position_seconds: f64 },
    /// Cambió la duración del track actual.
    DurationChanged { duration_seconds: f64 },
    /// Cambió el volumen.
    VolumeChanged { volume: f64 },
    /// Llegó al final del archivo. `reason` puede ser "eof", "stop", "error",
    /// "redirect" o "quit"; `error` trae el detalle si mpv no pudo abrir el
    /// stream (por ejemplo HTTP 403 por una URL firmada expirada).
    EndFile {
        reason: String,
        position_seconds: Option<f64>,
        error: Option<String>,
    },
    /// Cambió la propiedad loop-file.
    LoopChanged { looping: bool },
    /// Se perdió la conexión con mpv; el supervisor intentará reconectar.
    Disconnected,
    /// La conexión con mpv se restableció (proceso reiniciado).
    Reconnected,
    /// Evento genérico no manejado específicamente.
    Other(Value),
}

/// Estado actual de mpv, actualizado por el event loop.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MpvStatus {
    /// Si el socket con mpv está actualmente conectado.
    pub connected: bool,
    pub paused: bool,
    pub volume: f64,
    pub position_seconds: Option<f64>,
    pub duration_seconds: Option<f64>,
    pub looping: bool,
}

/// Comando interno para el event loop de mpv.
enum MpvCommand {
    /// Setea una propiedad y espera confirmación.
    SetProperty {
        name: String,
        value: Value,
        reply_tx: oneshot::Sender<Result<(), CadenceError>>,
    },
    /// Carga un archivo (loadfile).
    LoadFile {
        url: String,
        reply_tx: oneshot::Sender<Result<(), CadenceError>>,
    },
    /// Detiene la reproducción.
    Stop {
        reply_tx: oneshot::Sender<Result<(), CadenceError>>,
    },
    /// Apaga mpv.
    Shutdown,
}

/// Comando enviado a mpv que aún no tiene respuesta.
struct PendingCommand {
    reply: oneshot::Sender<Result<(), CadenceError>>,
    /// Propiedad a reflejar en el status cuando mpv confirme el comando.
    /// Se aplica en la respuesta (y no al enviar) para no pisar con un
    /// `property-change` atrasado de un comando anterior.
    optimistic: Option<(String, Value)>,
}

enum ConnectionOutcome {
    /// El usuario pidió apagar mpv; no reconectar.
    Shutdown,
    /// La conexión se perdió; reconectar con backoff.
    Lost,
}

/// Handle público para interactuar con mpv sin bloquear el event loop.
/// Es estable a través de reconexiones: `cmd_tx`/`event_tx` sobreviven al
/// reinicio del proceso hijo y del socket.
#[derive(Clone)]
pub struct MpvPlayer {
    cmd_tx: mpsc::Sender<MpvCommand>,
    status_rx: watch::Receiver<MpvStatus>,
    event_tx: broadcast::Sender<MpvEvent>,
}

impl MpvPlayer {
    /// Conecta a mpv, spawnea el proceso y arranca el supervisor que lo
    /// reinicia automáticamente si se cae.
    pub async fn connect(socket_path: PathBuf) -> Result<Self, CadenceError> {
        let (cmd_tx, cmd_rx) = mpsc::channel(CMD_CHANNEL_CAPACITY);
        let (status_tx, status_rx) = watch::channel(MpvStatus::default());
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        let supervisor_socket = socket_path.clone();
        let supervisor_status = status_tx.clone();
        let supervisor_events = event_tx.clone();
        tokio::spawn(async move {
            Self::run_supervisor(
                supervisor_socket,
                cmd_rx,
                supervisor_status,
                supervisor_events,
            )
            .await;
        });

        // Esperar la primera conexión (o fallar como antes si mpv no existe).
        let mut rx = status_rx.clone();
        let connected = tokio::time::timeout(Duration::from_secs(8), async move {
            loop {
                if rx.borrow_and_update().connected {
                    return true;
                }
                if rx.changed().await.is_err() {
                    return false;
                }
            }
        })
        .await
        .unwrap_or(false);

        if !connected {
            return Err(CadenceError::MpvSocketTimeout(socket_path));
        }

        Ok(Self {
            cmd_tx,
            status_rx,
            event_tx,
        })
    }

    /// Supervisa el proceso mpv: arranca, escucha, y si la conexión se cae
    /// reinicia con backoff exponencial conservando el canal de comandos.
    async fn run_supervisor(
        socket_path: PathBuf,
        mut cmd_rx: mpsc::Receiver<MpvCommand>,
        status_tx: watch::Sender<MpvStatus>,
        event_tx: broadcast::Sender<MpvEvent>,
    ) {
        let mut backoff = RECONNECT_BACKOFF_MIN;

        loop {
            let mut process = match Self::spawn_mpv(&socket_path) {
                Ok(child) => child,
                Err(err) => {
                    tracing::error!(error = %err, "Failed to spawn mpv, will retry");
                    let _ = status_tx.send(MpvStatus::default());
                    let _ = event_tx.send(MpvEvent::Disconnected);
                    if !Self::sleep_or_shutdown(&mut cmd_rx, backoff).await {
                        break;
                    }
                    backoff = Self::next_backoff(backoff);
                    continue;
                }
            };

            let stream = match Self::wait_for_socket(&socket_path).await {
                Ok(stream) => stream,
                Err(err) => {
                    tracing::error!(error = %err, "mpv IPC socket never appeared, restarting");
                    let _ = process.kill().await;
                    let _ = status_tx.send(MpvStatus::default());
                    let _ = event_tx.send(MpvEvent::Disconnected);
                    if !Self::sleep_or_shutdown(&mut cmd_rx, backoff).await {
                        break;
                    }
                    backoff = Self::next_backoff(backoff);
                    continue;
                }
            };

            let (reader, writer) = stream.into_split();

            // Estado inicial hasta que `observe_property` reporte los reales.
            let mut status = MpvStatus {
                connected: true,
                volume: 100.0,
                ..MpvStatus::default()
            };
            let _ = status_tx.send(status.clone());
            let _ = event_tx.send(MpvEvent::Reconnected);
            backoff = RECONNECT_BACKOFF_MIN;

            let outcome = Self::run_connection(
                BufReader::new(reader),
                writer,
                &mut cmd_rx,
                &status_tx,
                &event_tx,
                &mut status,
            )
            .await;

            status.connected = false;
            let _ = status_tx.send(status);
            let _ = process.kill().await;
            let _ = std::fs::remove_file(&socket_path);

            match outcome {
                ConnectionOutcome::Shutdown => break,
                ConnectionOutcome::Lost => {
                    tracing::warn!("mpv connection lost; reconnecting");
                    let _ = event_tx.send(MpvEvent::Disconnected);
                    if !Self::sleep_or_shutdown(&mut cmd_rx, backoff).await {
                        break;
                    }
                    backoff = Self::next_backoff(backoff);
                }
            }
        }

        tracing::info!("mpv supervisor stopped");
    }

    fn next_backoff(current: Duration) -> Duration {
        (current * 2).min(RECONNECT_BACKOFF_MAX)
    }

    /// Espera `delay`, pero aborta si llega un `Shutdown` por el canal.
    /// Devuelve false si se pidió apagar.
    async fn sleep_or_shutdown(
        cmd_rx: &mut mpsc::Receiver<MpvCommand>,
        delay: Duration,
    ) -> bool {
        let sleep = tokio::time::sleep(delay);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                _ = &mut sleep => return true,
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(MpvCommand::Shutdown) | None => return false,
                        Some(MpvCommand::SetProperty { reply_tx, .. })
                        | Some(MpvCommand::LoadFile { reply_tx, .. })
                        | Some(MpvCommand::Stop { reply_tx }) => {
                            // Sin conexión: rechazar comandos en cola en vez de
                            // dejarlos colgados hasta la reconexión.
                            let _ = reply_tx.send(Err(CadenceError::MpvConnectionClosed));
                        }
                    }
                }
            }
        }
    }

    fn spawn_mpv(socket_path: &Path) -> Result<Child, CadenceError> {
        // Si la app murió sin shutdown (SIGKILL, crash, `tauri dev`
        // recompilando), mpv queda huérfano ocupando el socket.
        kill_stale_mpv(socket_path);
        let _ = std::fs::remove_file(socket_path);

        let mut cmd = Command::new("mpv");
        cmd.arg("--idle=yes")
            .arg("--no-video")
            .arg("--really-quiet")
            .arg("--af=dynaudnorm")
            .arg(format!("--input-ipc-server={}", socket_path.display()));

        if !cfg!(test) {
            if Path::new("/usr/lib/mpv-mpris/mpris.so").exists() {
                cmd.arg("--script=/usr/lib/mpv-mpris/mpris.so");
            } else if Path::new("/etc/mpv/scripts/mpris.so").exists() {
                cmd.arg("--script=/etc/mpv/scripts/mpris.so");
            }
        }

        let child = cmd
            .kill_on_drop(true)
            .spawn()
            .map_err(CadenceError::MpvSpawn)?;
        // Dejamos constancia del hijo para poder limpiarlo en el próximo
        // arranque si el proceso muere sin shutdown.
        if let Some(pid) = child.id() {
            let _ = std::fs::write(stale_pid_file(socket_path), pid.to_string());
        }
        Ok(child)
    }

    async fn wait_for_socket(socket_path: &Path) -> Result<UnixStream, CadenceError> {
        for _ in 0..SOCKET_CONNECT_MAX_ATTEMPTS {
            if let Ok(stream) = UnixStream::connect(socket_path).await {
                return Ok(stream);
            }
            sleep(SOCKET_CONNECT_RETRY_DELAY).await;
        }
        Err(CadenceError::MpvSocketTimeout(socket_path.to_path_buf()))
    }

    /// Una conexión concreta: registra observers, procesa comandos y eventos
    /// hasta que el socket se cierra o se pide shutdown.
    async fn run_connection(
        mut reader: BufReader<OwnedReadHalf>,
        mut writer: OwnedWriteHalf,
        cmd_rx: &mut mpsc::Receiver<MpvCommand>,
        status_tx: &watch::Sender<MpvStatus>,
        event_tx: &broadcast::Sender<MpvEvent>,
        status: &mut MpvStatus,
    ) -> ConnectionOutcome {
        let mut pending_replies: std::collections::HashMap<u64, PendingCommand> =
            std::collections::HashMap::new();
        let mut request_id_counter: u64 = 0;
        let mut line_buffer = String::new();

        // Observar las propiedades que la UI necesita. mpv envía un
        // `property-change` inicial con el valor actual por cada observer,
        // lo que además inicializa `status` con datos reales.
        for (id, name) in [
            (OBS_PAUSE, "pause"),
            (OBS_VOLUME, "volume"),
            (OBS_TIME_POS, "time-pos"),
            (OBS_DURATION, "duration"),
            (OBS_LOOP_FILE, "loop-file"),
        ] {
            let observe = json!({ "command": ["observe_property", id, name] });
            if let Err(err) = send_json(&mut writer, &observe).await {
                tracing::warn!(error = %err, property = name, "failed to observe property");
                return ConnectionOutcome::Lost;
            }
        }

        loop {
            tokio::select! {
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(MpvCommand::SetProperty { name, value, reply_tx }) => {
                            request_id_counter += 1;
                            let request_id = request_id_counter;
                            let payload = json!({
                                "command": ["set_property", name, value],
                                "request_id": request_id,
                            });
                            pending_replies.insert(
                                request_id,
                                PendingCommand {
                                    reply: reply_tx,
                                    optimistic: Some((name.clone(), value.clone())),
                                },
                            );
                            if let Err(err) = send_json(&mut writer, &payload).await {
                                if let Some(pending) = pending_replies.remove(&request_id) {
                                    let _ = pending.reply.send(Err(err));
                                }
                                return ConnectionOutcome::Lost;
                            }
                        }
                        Some(MpvCommand::LoadFile { url, reply_tx }) => {
                            request_id_counter += 1;
                            let request_id = request_id_counter;
                            let payload = json!({
                                "command": ["loadfile", url],
                                "request_id": request_id,
                            });
                            pending_replies.insert(
                                request_id,
                                PendingCommand {
                                    reply: reply_tx,
                                    optimistic: None,
                                },
                            );
                            if let Err(err) = send_json(&mut writer, &payload).await {
                                if let Some(pending) = pending_replies.remove(&request_id) {
                                    let _ = pending.reply.send(Err(err));
                                }
                                return ConnectionOutcome::Lost;
                            }
                            // Un archivo nuevo arranca sin posición/duración.
                            set_position(status, None, status_tx, event_tx);
                            set_duration(status, None, status_tx, event_tx);
                            set_paused(status, false, status_tx, event_tx);
                        }
                        Some(MpvCommand::Stop { reply_tx }) => {
                            request_id_counter += 1;
                            let request_id = request_id_counter;
                            let payload = json!({
                                "command": ["stop"],
                                "request_id": request_id,
                            });
                            pending_replies.insert(
                                request_id,
                                PendingCommand {
                                    reply: reply_tx,
                                    optimistic: None,
                                },
                            );
                            if let Err(err) = send_json(&mut writer, &payload).await {
                                if let Some(pending) = pending_replies.remove(&request_id) {
                                    let _ = pending.reply.send(Err(err));
                                }
                                return ConnectionOutcome::Lost;
                            }
                            set_position(status, None, status_tx, event_tx);
                            set_duration(status, None, status_tx, event_tx);
                        }
                        Some(MpvCommand::Shutdown) => {
                            resolve_pending(&mut pending_replies);
                            return ConnectionOutcome::Shutdown;
                        }
                        None => {
                            // Handle dropped (app shutting down).
                            resolve_pending(&mut pending_replies);
                            return ConnectionOutcome::Shutdown;
                        }
                    }
                }

                read_result = reader.read_line(&mut line_buffer) => {
                    let bytes_read = match read_result {
                        Ok(n) => n,
                        Err(err) => {
                            tracing::error!(error = %err, "error reading from mpv socket");
                            break;
                        }
                    };
                    if bytes_read == 0 {
                        tracing::warn!("mpv socket closed (EOF)");
                        break;
                    }

                    let message: Value = match serde_json::from_str(&line_buffer) {
                        Ok(value) => value,
                        Err(err) => {
                            tracing::warn!(error = %err, line = %line_buffer, "failed to parse mpv message");
                            line_buffer.clear();
                            continue;
                        }
                    };
                    line_buffer.clear();

                    if let Some(request_id) = message.get("request_id").and_then(Value::as_u64) {
                        if let Some(mut pending) = pending_replies.remove(&request_id) {
                            let result = match message.get("error").and_then(Value::as_str) {
                                Some("success") | None => Ok(()),
                                Some(error) => Err(CadenceError::MpvCommand {
                                    command: format!("request_id={request_id}"),
                                    message: error.to_string(),
                                }),
                            };
                            // mpv ya emitió los `property-change` de este
                            // comando, así que reflejar aquí el estado pedido
                            // no puede pisar cambios de un comando anterior.
                            if result.is_ok() {
                                if let Some((name, value)) = pending.optimistic.take() {
                                    apply_property(status, &name, &value, status_tx, event_tx);
                                }
                            }
                            let _ = pending.reply.send(result);
                        }
                    } else if let Some(event_name) =
                        message.get("event").and_then(Value::as_str)
                    {
                        match Self::handle_mpv_event(
                            event_name,
                            &message,
                            status,
                            status_tx,
                            event_tx,
                        ) {
                            Some(ConnectionOutcome::Shutdown) => {
                                resolve_pending(&mut pending_replies);
                                return ConnectionOutcome::Shutdown;
                            }
                            Some(ConnectionOutcome::Lost) => break,
                            None => {}
                        }
                    }
                }
            }
        }

        resolve_pending(&mut pending_replies);
        ConnectionOutcome::Lost
    }

    /// Procesa un evento de mpv. Devuelve `Some(outcome)` si debe terminar la
    /// conexión.
    fn handle_mpv_event(
        event_name: &str,
        message: &Value,
        status: &mut MpvStatus,
        status_tx: &watch::Sender<MpvStatus>,
        event_tx: &broadcast::Sender<MpvEvent>,
    ) -> Option<ConnectionOutcome> {
        match event_name {
            "property-change" => {
                let name = message
                    .get("name")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        message
                            .get("id")
                            .and_then(Value::as_u64)
                            .and_then(observed_name)
                    });
                let name = name?;
                let data = message.get("data").cloned().unwrap_or(Value::Null);
                apply_property(status, name, &data, status_tx, event_tx);
                None
            }
            "end-file" => {
                // mpv 0.40+ envía el detalle en el propio evento
                // (`{"event":"end-file","reason":"eof","file_error":"..."}`);
                // versiones anteriores lo traían dentro de `data`.
                let reason = end_file_field(message, "reason")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string();
                let error = end_file_error(message);
                let position = status.position_seconds;
                set_position(status, None, status_tx, event_tx);
                set_duration(status, None, status_tx, event_tx);
                let _ = event_tx.send(MpvEvent::EndFile {
                    reason,
                    position_seconds: position,
                    error,
                });
                None
            }
            "idle" => {
                set_position(status, None, status_tx, event_tx);
                set_duration(status, None, status_tx, event_tx);
                None
            }
            "shutdown" => Some(ConnectionOutcome::Shutdown),
            _ => {
                let _ = event_tx.send(MpvEvent::Other(message.clone()));
                None
            }
        }
    }

    // ===== API Pública =====

    /// Carga un archivo en mpv.
    pub async fn load(&self, url: &str) -> Result<(), CadenceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(MpvCommand::LoadFile {
                url: url.to_string(),
                reply_tx,
            })
            .await
            .map_err(|_| CadenceError::MpvConnectionClosed)?;
        reply_rx.await.map_err(|_| CadenceError::MpvConnectionClosed)?
    }

    pub async fn pause(&self) -> Result<(), CadenceError> {
        self.set_property("pause", json!(true)).await
    }

    pub async fn resume(&self) -> Result<(), CadenceError> {
        self.set_property("pause", json!(false)).await
    }

    /// Detiene la reproducción y descarga el archivo.
    pub async fn stop(&self) -> Result<(), CadenceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(MpvCommand::Stop { reply_tx })
            .await
            .map_err(|_| CadenceError::MpvConnectionClosed)?;
        reply_rx.await.map_err(|_| CadenceError::MpvConnectionClosed)?
    }

    pub async fn set_volume(&self, level: f64) -> Result<(), CadenceError> {
        self.set_property("volume", json!(level.clamp(0.0, 100.0)))
            .await
    }

    pub async fn seek(&self, position_seconds: f64) -> Result<(), CadenceError> {
        self.set_property("time-pos", json!(position_seconds.max(0.0)))
            .await
    }

    pub async fn set_loop(&self, enabled: bool) -> Result<(), CadenceError> {
        let value = if enabled { "inf" } else { "no" };
        self.set_property("loop-file", json!(value)).await
    }

    /// Lectura instantánea del estado (watch channel, sin IPC).
    pub fn status(&self) -> MpvStatus {
        self.status_rx.borrow().clone()
    }

    /// Suscribe a eventos de mpv (pause, position, eof, etc.).
    pub fn subscribe_events(&self) -> broadcast::Receiver<MpvEvent> {
        self.event_tx.subscribe()
    }

    /// Suscribe a cambios de estado (watch channel).
    pub fn subscribe_status(&self) -> watch::Receiver<MpvStatus> {
        self.status_rx.clone()
    }

    async fn set_property(&self, name: &str, value: Value) -> Result<(), CadenceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(MpvCommand::SetProperty {
                name: name.to_string(),
                value,
                reply_tx,
            })
            .await
            .map_err(|_| CadenceError::MpvConnectionClosed)?;
        reply_rx.await.map_err(|_| CadenceError::MpvConnectionClosed)?
    }

    /// Fuerza el apagado de mpv (para shutdown de app).
    ///
    /// Espera a que el supervisor termine: es quien hace `kill` del hijo. Sin
    /// esa espera, la app sale del `run()` antes de que mpv muera y deja un
    /// proceso huérfano.
    pub async fn shutdown(&self) {
        let _ = self.cmd_tx.send(MpvCommand::Shutdown).await;
        let _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, self.cmd_tx.closed()).await;
    }
}

async fn send_json(writer: &mut OwnedWriteHalf, value: &Value) -> Result<(), CadenceError> {
    let mut payload = serde_json::to_vec(value).map_err(CadenceError::MpvSerialize)?;
    payload.push(b'\n');
    writer
        .write_all(&payload)
        .await
        .map_err(CadenceError::MpvIo)?;
    writer.flush().await.map_err(CadenceError::MpvIo)?;
    Ok(())
}

fn resolve_pending(pending: &mut std::collections::HashMap<u64, PendingCommand>) {
    for (_, command) in pending.drain() {
        let _ = command.reply.send(Err(CadenceError::MpvConnectionClosed));
    }
}

/// Archivo auxiliar junto al socket con el PID de mpv. Se usa para limpiar un
/// mpv huérfano dejado por una ejecución anterior que no llegó a apagarlo.
fn stale_pid_file(socket_path: &Path) -> PathBuf {
    let mut file = socket_path.as_os_str().to_owned();
    file.push(".pid");
    PathBuf::from(file)
}

/// Mata el mpv que quedó colgado del arranque anterior, si existe.
///
/// Solo se mata si el proceso sigue vivo **y** su línea de comando contiene
/// nuestro socket, para no tocar procesos ajenos que reutilicen el PID.
fn kill_stale_mpv(socket_path: &Path) {
    let pid_file = stale_pid_file(socket_path);
    let Ok(content) = std::fs::read_to_string(&pid_file) else {
        return;
    };
    let _ = std::fs::remove_file(&pid_file);

    let Ok(pid) = content.trim().parse::<u32>() else {
        return;
    };
    if pid == std::process::id() || !process_is_our_mpv(pid, socket_path) {
        return;
    }

    tracing::warn!(pid, "killing stale mpv process left by a previous run");
    let _ = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status();
}

/// Verifica en `/proc` que `pid` es, en efecto, nuestro mpv. Sin `/proc`
/// (macOS) devolvemos false: ante la duda, no matamos.
fn process_is_our_mpv(pid: u32, socket_path: &Path) -> bool {
    let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let text = String::from_utf8_lossy(&cmdline).replace('\0', " ");
    text.contains("mpv") && text.contains(&format!("--input-ipc-server={}", socket_path.display()))
}

/// Lee un campo de `end-file` sea donde sea: mpv 0.40+ lo pone en el nivel
/// del evento y las versiones anteriores dentro de `data`.
fn end_file_field<'a>(message: &'a Value, field: &str) -> Option<&'a Value> {
    message.get(field).or_else(|| {
        message
            .get("data")
            .filter(|data| !data.is_null())
            .and_then(|data| data.get(field))
    })
}

/// Extrae el detalle del fallo de un `end-file`.
///
/// mpv actual envía `file_error: "loading failed"` (string); el formato
/// antiguo traía `error: "..."` dentro de `data` o `file_error: true`.
/// Un `error: ""` significa "sin error" (fin de archivo normal).
fn end_file_error(message: &Value) -> Option<String> {
    let non_empty = |value: Option<&Value>| match value {
        Some(Value::String(text)) if !text.is_empty() => Some(text.clone()),
        _ => None,
    };

    non_empty(end_file_field(message, "error")).or_else(|| match end_file_field(message, "file_error")
    {
        Some(Value::Bool(true)) => Some("file could not be opened".to_string()),
        other => non_empty(other),
    })
}

fn observed_name(id: u64) -> Option<&'static str> {
    match id {
        OBS_PAUSE => Some("pause"),
        OBS_VOLUME => Some("volume"),
        OBS_TIME_POS => Some("time-pos"),
        OBS_DURATION => Some("duration"),
        OBS_LOOP_FILE => Some("loop-file"),
        _ => None,
    }
}

/// Aplica un valor de propiedad al estado y emite el evento correspondiente
/// solo si cambió. Sirve tanto para replies optimistas como para eventos.
fn apply_property(
    status: &mut MpvStatus,
    name: &str,
    value: &Value,
    status_tx: &watch::Sender<MpvStatus>,
    event_tx: &broadcast::Sender<MpvEvent>,
) {
    match name {
        "pause" => {
            if let Some(paused) = value.as_bool() {
                set_paused(status, paused, status_tx, event_tx);
            }
        }
        "volume" => {
            if let Some(volume) = value.as_f64() {
                set_volume(status, volume, status_tx, event_tx);
            }
        }
        "time-pos" => {
            if let Some(position) = value.as_f64() {
                set_position(status, Some(position.max(0.0)), status_tx, event_tx);
            }
        }
        "duration" => {
            if let Some(duration) = value.as_f64() {
                let duration = if duration.is_finite() && duration > 0.0 {
                    Some(duration)
                } else {
                    None
                };
                set_duration(status, duration, status_tx, event_tx);
            }
        }
        "loop-file" => {
            let looping = match value {
                Value::String(text) => text == "inf",
                Value::Bool(flag) => *flag,
                Value::Number(number) => number.as_u64() == Some(0),
                _ => false,
            };
            set_loop(status, looping, status_tx, event_tx);
        }
        _ => {}
    }
}

fn set_paused(
    status: &mut MpvStatus,
    paused: bool,
    status_tx: &watch::Sender<MpvStatus>,
    event_tx: &broadcast::Sender<MpvEvent>,
) {
    if status.paused != paused {
        status.paused = paused;
        let _ = status_tx.send(status.clone());
        let _ = event_tx.send(MpvEvent::PauseChanged { paused });
    }
}

fn set_volume(
    status: &mut MpvStatus,
    volume: f64,
    status_tx: &watch::Sender<MpvStatus>,
    event_tx: &broadcast::Sender<MpvEvent>,
) {
    let volume = volume.clamp(0.0, 100.0);
    if (status.volume - volume).abs() > f64::EPSILON {
        status.volume = volume;
        let _ = status_tx.send(status.clone());
        let _ = event_tx.send(MpvEvent::VolumeChanged { volume });
    }
}

fn set_position(
    status: &mut MpvStatus,
    position: Option<f64>,
    status_tx: &watch::Sender<MpvStatus>,
    event_tx: &broadcast::Sender<MpvEvent>,
) {
    if status.position_seconds != position {
        status.position_seconds = position;
        let _ = status_tx.send(status.clone());
        if let Some(position_seconds) = position {
            let _ = event_tx.send(MpvEvent::PositionChanged { position_seconds });
        }
    }
}

fn set_duration(
    status: &mut MpvStatus,
    duration: Option<f64>,
    status_tx: &watch::Sender<MpvStatus>,
    event_tx: &broadcast::Sender<MpvEvent>,
) {
    if status.duration_seconds != duration {
        status.duration_seconds = duration;
        let _ = status_tx.send(status.clone());
        if let Some(duration_seconds) = duration {
            let _ = event_tx.send(MpvEvent::DurationChanged { duration_seconds });
        }
    }
}

fn set_loop(
    status: &mut MpvStatus,
    looping: bool,
    status_tx: &watch::Sender<MpvStatus>,
    event_tx: &broadcast::Sender<MpvEvent>,
) {
    if status.looping != looping {
        status.looping = looping;
        let _ = status_tx.send(status.clone());
        let _ = event_tx.send(MpvEvent::LoopChanged { looping });
    }
}

/// Tests de integración (requieren mpv real).
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_socket_path() -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("cadence-mpv-test-{}-{id}.sock", std::process::id()))
    }

    #[test]
    fn observed_property_ids_are_mapped() {
        assert_eq!(observed_name(OBS_PAUSE), Some("pause"));
        assert_eq!(observed_name(OBS_VOLUME), Some("volume"));
        assert_eq!(observed_name(OBS_TIME_POS), Some("time-pos"));
        assert_eq!(observed_name(OBS_DURATION), Some("duration"));
        assert_eq!(observed_name(OBS_LOOP_FILE), Some("loop-file"));
        assert_eq!(observed_name(99), None);
    }

    #[test]
    fn apply_property_updates_status_and_emits_events() {
        let (status_tx, status_rx) = watch::channel(MpvStatus::default());
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let mut status = MpvStatus::default();

        apply_property(&mut status, "volume", &json!(42.0), &status_tx, &event_tx);
        assert!((status.volume - 42.0).abs() < f64::EPSILON);
        assert!((status_rx.borrow().volume - 42.0).abs() < f64::EPSILON);
        assert!(matches!(
            event_rx.try_recv(),
            Ok(MpvEvent::VolumeChanged { volume }) if (volume - 42.0).abs() < f64::EPSILON
        ));

        apply_property(&mut status, "loop-file", &json!("inf"), &status_tx, &event_tx);
        assert!(status.looping);

        apply_property(&mut status, "pause", &json!(true), &status_tx, &event_tx);
        assert!(status.paused);

        apply_property(
            &mut status,
            "duration",
            &json!(f64::INFINITY),
            &status_tx,
            &event_tx,
        );
        assert_eq!(status.duration_seconds, None);
    }

    #[test]
    fn retry_backoff_is_capped() {
        let mut backoff = RECONNECT_BACKOFF_MIN;
        for _ in 0..20 {
            backoff = MpvPlayer::next_backoff(backoff);
        }
        assert_eq!(backoff, RECONNECT_BACKOFF_MAX);
    }

    /// Capturado de mpv 0.41 reproduciendo un WAV local hasta el final:
    /// el detalle va en el nivel del evento, no en `data`.
    #[test]
    fn end_file_from_modern_mpv_reports_eof() {
        let message: Value = serde_json::from_str(
            r#"{"event":"end-file","reason":"eof","playlist_entry_id":2}"#,
        )
        .expect("parse end-file");

        let (status_tx, _status_rx) = watch::channel(MpvStatus::default());
        let (event_tx, mut event_rx) = broadcast::channel(8);
        let mut status = MpvStatus::default();

        let outcome = MpvPlayer::handle_mpv_event(
            "end-file",
            &message,
            &mut status,
            &status_tx,
            &event_tx,
        );
        assert!(outcome.is_none(), "end-file must not close the connection");

        match event_rx.try_recv() {
            Ok(MpvEvent::EndFile {
                reason,
                error,
                ..
            }) => {
                assert_eq!(reason, "eof");
                assert_eq!(error, None);
            }
            other => panic!("expected EndFile, got {other:?}"),
        }
    }

    /// Capturado de mpv 0.41 con un path inexistente (lo que pasa cuando la
    /// URL firmada de YouTube ya caducó): reason "error" + detalle.
    #[test]
    fn end_file_with_loading_failure_reports_error() {
        let message: Value = serde_json::from_str(
            r#"{"event":"end-file","reason":"error","playlist_entry_id":2,"file_error":"loading failed"}"#,
        )
        .expect("parse end-file");

        let (status_tx, _status_rx) = watch::channel(MpvStatus::default());
        let (event_tx, mut event_rx) = broadcast::channel(8);
        let mut status = MpvStatus::default();

        MpvPlayer::handle_mpv_event("end-file", &message, &mut status, &status_tx, &event_tx);

        match event_rx.try_recv() {
            Ok(MpvEvent::EndFile { reason, error, .. }) => {
                assert_eq!(reason, "error");
                assert_eq!(error.as_deref(), Some("loading failed"));
            }
            other => panic!("expected EndFile, got {other:?}"),
        }
    }

    /// Compatibilidad con el formato antiguo de mpv (detalle dentro de `data`).
    #[test]
    fn end_file_from_legacy_mpv_uses_the_data_object() {
        let message: Value = serde_json::from_str(
            r#"{"event":"end-file","data":{"reason":"error","error":"HTTP error 403"}}"#,
        )
        .expect("parse end-file");

        assert_eq!(end_file_field(&message, "reason").and_then(Value::as_str), Some("error"));
        assert_eq!(
            end_file_error(&message).as_deref(),
            Some("HTTP error 403")
        );
    }

    /// Un `error: ""` (string vacío) no es un error: es un fin normal.
    #[test]
    fn empty_error_string_is_not_treated_as_failure() {
        let message: Value =
            serde_json::from_str(r#"{"event":"end-file","reason":"eof","error":""}"#)
                .expect("parse end-file");
        assert_eq!(end_file_error(&message), None);
    }

    #[test]
    fn stale_pid_file_lives_next_to_the_socket() {
        assert_eq!(
            stale_pid_file(Path::new("/tmp/a.sock")),
            PathBuf::from("/tmp/a.sock.pid")
        );
    }

    #[test]
    fn a_pid_that_is_not_our_mpv_is_never_killed() {
        // Nuestro propio proceso de test no es mpv → nunca lo tocamos.
        assert!(!process_is_our_mpv(std::process::id(), Path::new("/tmp/a.sock")));
        // PID imposible: sin /proc no podemos verificar, así que no matamos.
        assert!(!process_is_our_mpv(u32::MAX, Path::new("/tmp/a.sock")));
    }

    #[tokio::test]
    #[ignore = "requires mpv binary and can hang in CI"]
    async fn connects_and_reports_idle_state() {
        let player = MpvPlayer::connect(unique_socket_path())
            .await
            .expect("connect to mpv");
        let status = player.status();
        assert!(status.connected);
        assert!(!status.paused);
        assert!((status.volume - 100.0).abs() < f64::EPSILON);
        player.shutdown().await;
    }

    #[tokio::test]
    #[ignore = "requires mpv binary and can hang in CI"]
    async fn set_loop_is_reflected_without_polling() {
        let player = MpvPlayer::connect(unique_socket_path())
            .await
            .expect("connect to mpv");
        player.set_loop(true).await.expect("set loop");
        assert!(player.status().looping);
        player.set_loop(false).await.expect("unset loop");
        assert!(!player.status().looping);
        player.shutdown().await;
    }

    #[tokio::test]
    #[ignore = "requires mpv binary and can hang in CI"]
    async fn status_reads_are_instant() {
        let player = MpvPlayer::connect(unique_socket_path())
            .await
            .expect("connect to mpv");
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            let _ = player.status();
        }
        assert!(start.elapsed() < Duration::from_millis(10));
        player.shutdown().await;
    }
}