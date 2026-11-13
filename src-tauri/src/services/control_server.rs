use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(windows)]
use tokio::net::windows::named_pipe::ServerOptions;

use crate::models::player_state::AudioPreset;
use crate::services::focus::FocusEngine;
use crate::services::orchestrator::PlayerOrchestrator;

pub const MAX_CONTROL_LINE_BYTES: usize = 16 * 1024;

pub fn control_socket_path() -> PathBuf {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        crate::services::local_transport::runtime_root().join(format!(
            "cadence-{}/control.sock",
            crate::services::local_transport::effective_uid()
        ))
    }
    #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
    {
        let user = std::env::var("USER").unwrap_or_else(|_| "default".to_string());
        std::env::temp_dir().join(format!("cadence-control-{}.sock", user))
    }
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "default".to_string());
        PathBuf::from(format!(r"\\.\pipe\cadence-control-{}", user))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub async fn bind_control_at(
    root: &std::path::Path,
) -> std::io::Result<(
    UnixListener,
    crate::services::local_transport::EndpointLease,
)> {
    let path = crate::services::local_transport::control_path_at(root, true)?;
    let lease = crate::services::local_transport::EndpointLease::acquire(&path)?;
    let listener = lease.bind().await?;
    Ok((listener, lease))
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod transport_tests {
    use super::*;
    use crate::services::local_transport::{connect_control_at, resolve_at, validate_socket};
    use crate::services::mpv_event::MpvPlayer;
    use crate::services::orchestrator::PlayerOrchestrator;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cadence-consumer-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[tokio::test]
    async fn startup_rejects_collision_without_mutating_fixture() {
        let f = Fixture::new();
        let private = resolve_at(&f.0, true).unwrap();
        let path = private.join("control.sock");
        std::fs::write(&path, b"preserve").unwrap();
        let result = bind_control_at(&f.0).await;
        assert!(result.is_err(), "startup must reject a regular file");
        assert_eq!(std::fs::read(&path).unwrap(), b"preserve");
    }

    #[tokio::test]
    async fn startup_retains_lock_and_recovers_only_safe_stale_socket() {
        let f = Fixture::new();
        let (listener, lease) = bind_control_at(&f.0).await.unwrap();
        let path = resolve_at(&f.0, false).unwrap().join("control.sock");
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        validate_socket(&path).unwrap();
        assert!(bind_control_at(&f.0).await.is_err());
        let client = tokio::net::UnixStream::connect(&path).await.unwrap();
        drop(client);
        drop(listener);
        assert!(
            bind_control_at(&f.0).await.is_err(),
            "lease outlives listener"
        );
        drop(lease);
        let (listener, lease) = bind_control_at(&f.0).await.unwrap();
        drop(listener);
        lease.remove_socket().unwrap();
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn client_server_parity_and_rejects_loose_socket_permissions() {
        let f = Fixture::new();
        let (_listener, lease) = bind_control_at(&f.0).await.unwrap();
        let stream = connect_control_at(&f.0).await.unwrap();
        drop(stream);

        let path = resolve_at(&f.0, false).unwrap().join("control.sock");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(connect_control_at(&f.0).await.is_err());

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        drop(_listener);
        lease.remove_socket().unwrap();
    }

    #[tokio::test]
    async fn bounded_framing_rejects_oversized_line_without_oom_and_allows_normal_commands() {
        let mpv = Arc::new(MpvPlayer::fake());
        let orch = Arc::new(PlayerOrchestrator::new(
            crate::services::orchestrator::PlaybackController::new(mpv.clone()),
            crate::services::orchestrator::TrackResolver::new(Arc::new(
                crate::services::yt_dlp::YtDlpService::new(),
            )),
            mpv.subscribe_events(),
            mpv.subscribe_status(),
            None,
        ));
        let focus = crate::services::focus::FocusEngine::without_ticker(orch.clone());

        let (client, server) = tokio::io::duplex(64 * 1024);
        let (server_read, server_write) = tokio::io::split(server);
        let handle = tokio::spawn(async move {
            ControlServer::handle_client(server_read, server_write, orch, focus).await;
        });

        let (client_read, mut client_write) = tokio::io::split(client);
        let mut reader = BufReader::new(client_read);

        // Normal command works
        client_write
            .write_all(b"{\"cmd\":\"status\"}\n")
            .await
            .unwrap();
        client_write.flush().await.unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let val: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(val["ok"], true);

        // Oversized command (> MAX_CONTROL_LINE_BYTES) is rejected and closes connection
        let mut oversized = vec![b'a'; MAX_CONTROL_LINE_BYTES + 128];
        oversized.push(b'\n');
        client_write.write_all(&oversized).await.unwrap();
        client_write.flush().await.unwrap();

        line.clear();
        reader.read_line(&mut line).await.unwrap();
        let val: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(val["ok"], false);
        assert!(val["error"]
            .as_str()
            .unwrap_or("")
            .contains("maximum length"));

        // Subsequent read hits EOF because server closed stream
        line.clear();
        let n = reader.read_line(&mut line).await.unwrap();
        assert_eq!(n, 0);

        handle.await.unwrap();
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ControlRequest {
    Status,
    Play { query: String },
    Pause,
    Resume,
    Toggle,
    Next,
    Prev,
    Volume { value: f64 },
    Preset { value: AudioPreset },
    Speed { value: f64 },
    Focus { action: String },
    QueueList,
    QueueClear,
}

#[derive(Debug, Serialize)]
pub struct ControlResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ControlResponse {
    pub fn success(data: Value) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(message.into()),
        }
    }
}

pub struct ControlServer;

impl ControlServer {
    pub fn spawn(orchestrator: Arc<PlayerOrchestrator>, focus: Arc<FocusEngine>) {
        tauri::async_runtime::spawn(async move {
            let socket_path = control_socket_path();
            tracing::info!(path = %socket_path.display(), "Starting Cadence Control Server");

            #[cfg(any(target_os = "linux", target_os = "macos"))]
            {
                let (listener, lease) =
                    match bind_control_at(crate::services::local_transport::runtime_root()).await {
                        Ok(pair) => pair,
                        Err(err) => {
                            tracing::error!(error = %err, "Failed to bind hardened control socket");
                            return;
                        }
                    };

                loop {
                    match listener.accept().await {
                        Ok((stream, _)) => {
                            let orch = orchestrator.clone();
                            let foc = focus.clone();
                            tauri::async_runtime::spawn(async move {
                                let (reader, writer) = stream.into_split();
                                Self::handle_client(reader, writer, orch, foc).await;
                            });
                        }
                        Err(err) => {
                            tracing::warn!(error = %err, "Error accepting control connection");
                            break;
                        }
                    }
                }

                let _ = lease.remove_socket();
            }

            #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
            {
                let _ = std::fs::remove_file(&socket_path);
                let listener = match UnixListener::bind(&socket_path) {
                    Ok(l) => l,
                    Err(err) => {
                        tracing::error!(error = %err, "Failed to bind control socket");
                        return;
                    }
                };

                loop {
                    match listener.accept().await {
                        Ok((stream, _)) => {
                            let orch = orchestrator.clone();
                            let foc = focus.clone();
                            tauri::async_runtime::spawn(async move {
                                let (reader, writer) = stream.into_split();
                                Self::handle_client(reader, writer, orch, foc).await;
                            });
                        }
                        Err(err) => {
                            tracing::warn!(error = %err, "Error accepting control connection");
                            break;
                        }
                    }
                }
            }

            #[cfg(windows)]
            {
                let pipe_name = socket_path.to_string_lossy().to_string();
                loop {
                    let server = match ServerOptions::new()
                        .first_pipe_instance(true)
                        .create(&pipe_name)
                    {
                        Ok(s) => s,
                        Err(err) => {
                            tracing::error!(error = %err, "Failed to create named pipe server");
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            continue;
                        }
                    };

                    if server.connect().await.is_ok() {
                        let orch = orchestrator.clone();
                        let foc = focus.clone();
                        tauri::async_runtime::spawn(async move {
                            let (reader, writer) = tokio::io::split(server);
                            Self::handle_client(reader, writer, orch, foc).await;
                        });
                    }
                }
            }
        });
    }

    async fn handle_client<R, W>(
        reader: R,
        mut writer: W,
        orchestrator: Arc<PlayerOrchestrator>,
        focus: Arc<FocusEngine>,
    ) where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        let mut reader = BufReader::new(reader);
        loop {
            match read_bounded_line(&mut reader, MAX_CONTROL_LINE_BYTES).await {
                Ok(BoundedLine::Line(line)) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    let response = match serde_json::from_str::<ControlRequest>(trimmed) {
                        Ok(req) => Self::dispatch_command(req, &orchestrator, &focus).await,
                        Err(err) => ControlResponse::err(format!("Invalid command: {}", err)),
                    };

                    if let Ok(mut bytes) = serde_json::to_vec(&response) {
                        bytes.push(b'\n');
                        if writer.write_all(&bytes).await.is_err() || writer.flush().await.is_err() {
                            break;
                        }
                    }
                }
                Ok(BoundedLine::TooLong) => {
                    let response = ControlResponse::err("Command exceeds maximum length");
                    if let Ok(mut bytes) = serde_json::to_vec(&response) {
                        bytes.push(b'\n');
                        let _ = writer.write_all(&bytes).await;
                        let _ = writer.flush().await;
                    }
                    break;
                }
                Ok(BoundedLine::Eof) | Err(_) => break,
            }
        }
    }

    async fn dispatch_command(
        request: ControlRequest,
        orchestrator: &Arc<PlayerOrchestrator>,
        focus: &Arc<FocusEngine>,
    ) -> ControlResponse {
        match request {
            ControlRequest::Status => {
                let state = orchestrator.get_state().await;
                let focus_state = focus.get_state().await;
                ControlResponse::success(json!({
                    "player": state,
                    "focus": focus_state,
                }))
            }
            ControlRequest::Play { query } => {
                match orchestrator.search(&query, 1).await {
                    Ok(results) => {
                        if let Some(first) = results.into_iter().next() {
                            match orchestrator.play(first.clone()).await {
                                Ok(()) => ControlResponse::success(json!({ "playing": first })),
                                Err(err) => ControlResponse::err(err.to_string()),
                            }
                        } else {
                            ControlResponse::err("No tracks found for query")
                        }
                    }
                    Err(err) => ControlResponse::err(err.to_string()),
                }
            }
            ControlRequest::Pause => match orchestrator.pause().await {
                Ok(()) => ControlResponse::success(json!({ "paused": true })),
                Err(err) => ControlResponse::err(err.to_string()),
            },
            ControlRequest::Resume => match orchestrator.resume().await {
                Ok(()) => ControlResponse::success(json!({ "resumed": true })),
                Err(err) => ControlResponse::err(err.to_string()),
            },
            ControlRequest::Toggle => {
                let state = orchestrator.get_state().await;
                if state.status == crate::models::player_state::PlaybackStatus::Playing {
                    let _ = orchestrator.pause().await;
                    ControlResponse::success(json!({ "status": "paused" }))
                } else {
                    let _ = orchestrator.resume().await;
                    ControlResponse::success(json!({ "status": "playing" }))
                }
            }
            ControlRequest::Next => match orchestrator.next().await {
                Ok(()) => ControlResponse::success(json!({ "next": true })),
                Err(err) => ControlResponse::err(err.to_string()),
            },
            ControlRequest::Prev => match orchestrator.previous().await {
                Ok(()) => ControlResponse::success(json!({ "previous": true })),
                Err(err) => ControlResponse::err(err.to_string()),
            },
            ControlRequest::Volume { value } => match orchestrator.set_volume(value).await {
                Ok(()) => ControlResponse::success(json!({ "volume": value })),
                Err(err) => ControlResponse::err(err.to_string()),
            },
            ControlRequest::Preset { value } => match orchestrator.set_audio_preset(value).await {
                Ok(()) => ControlResponse::success(json!({ "preset": value })),
                Err(err) => ControlResponse::err(err.to_string()),
            },
            ControlRequest::Speed { value } => match orchestrator.set_speed(value).await {
                Ok(()) => ControlResponse::success(json!({ "speed": value })),
                Err(err) => ControlResponse::err(err.to_string()),
            },
            ControlRequest::Focus { action } => match action.as_str() {
                "start" => {
                    focus.start().await;
                    ControlResponse::success(json!({ "focus": "started" }))
                }
                "pause" => {
                    focus.pause().await;
                    ControlResponse::success(json!({ "focus": "paused" }))
                }
                "reset" => {
                    focus.reset().await;
                    ControlResponse::success(json!({ "focus": "reset" }))
                }
                "skip" => {
                    focus.skip().await;
                    ControlResponse::success(json!({ "focus": "skipped" }))
                }
                _ => ControlResponse::err("Unknown focus action (use start, pause, reset, skip)"),
            },
            ControlRequest::QueueList => {
                let queue = orchestrator.get_queue().await;
                ControlResponse::success(json!({ "queue": queue }))
            }
            ControlRequest::QueueClear => {
                orchestrator.clear_queue().await;
                ControlResponse::success(json!({ "cleared": true }))
            }
        }
    }
}

enum BoundedLine {
    Line(String),
    TooLong,
    Eof,
}

async fn read_bounded_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    limit: usize,
) -> std::io::Result<BoundedLine> {
    let mut buf = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if buf.is_empty() {
                return Ok(BoundedLine::Eof);
            }
            return Ok(BoundedLine::Line(
                String::from_utf8_lossy(&buf).into_owned(),
            ));
        }
        if let Some(newline_pos) = available.iter().position(|&b| b == b'\n') {
            let take_len = newline_pos + 1;
            if buf.len() + take_len > limit {
                reader.consume(take_len);
                return Ok(BoundedLine::TooLong);
            }
            buf.extend_from_slice(&available[..take_len]);
            reader.consume(take_len);
            return Ok(BoundedLine::Line(
                String::from_utf8_lossy(&buf).into_owned(),
            ));
        }
        if buf.len() + available.len() > limit {
            let len = available.len();
            reader.consume(len);
            return Ok(BoundedLine::TooLong);
        }
        buf.extend_from_slice(available);
        let len = available.len();
        reader.consume(len);
    }
}
