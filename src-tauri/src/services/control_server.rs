use std::path::PathBuf;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(windows)]
use tokio::net::windows::named_pipe::ServerOptions;

use crate::models::player_state::AudioPreset;
use crate::services::focus::FocusEngine;
use crate::services::orchestrator::PlayerOrchestrator;

pub fn control_socket_path() -> PathBuf {
    #[cfg(unix)]
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
        tokio::spawn(async move {
            let socket_path = control_socket_path();
            tracing::info!(path = %socket_path.display(), "Starting Cadence Control Server");

            #[cfg(unix)]
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
                            tokio::spawn(async move {
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
                        tokio::spawn(async move {
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
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
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
                let _ = writer.write_all(&bytes).await;
                let _ = writer.flush().await;
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
