use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};
use tokio::time::sleep;

use crate::errors::CadenceError;

const SOCKET_CONNECT_RETRY_DELAY: Duration = Duration::from_millis(100);
const SOCKET_CONNECT_MAX_ATTEMPTS: u32 = 50;

/// A live mpv playback status snapshot. `position_seconds` and
/// `duration_seconds` are `None` while mpv is idle with nothing loaded.
#[derive(Debug, Clone, PartialEq)]
pub struct MpvStatus {
    pub paused: bool,
    pub volume: f64,
    pub position_seconds: Option<f64>,
    pub duration_seconds: Option<f64>,
}

/// Owns an mpv child process and its IPC socket connection. All mpv
/// process management and JSON IPC framing is contained here.
pub struct MpvPlayer {
    socket_path: PathBuf,
    process: Child,
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
}

impl MpvPlayer {
    pub async fn connect(socket_path: PathBuf) -> Result<Self, CadenceError> {
        let _ = std::fs::remove_file(&socket_path);

        let process = Command::new("mpv")
            .arg("--idle=yes")
            .arg("--no-video")
            .arg("--really-quiet")
            .arg(format!("--input-ipc-server={}", socket_path.display()))
            .kill_on_drop(true)
            .spawn()
            .map_err(CadenceError::MpvSpawn)?;

        let stream = Self::wait_for_socket(&socket_path).await?;
        let (reader, writer) = stream.into_split();

        Ok(Self {
            socket_path,
            process,
            reader: BufReader::new(reader),
            writer,
        })
    }

    async fn wait_for_socket(socket_path: &PathBuf) -> Result<UnixStream, CadenceError> {
        for _ in 0..SOCKET_CONNECT_MAX_ATTEMPTS {
            if let Ok(stream) = UnixStream::connect(socket_path).await {
                return Ok(stream);
            }
            sleep(SOCKET_CONNECT_RETRY_DELAY).await;
        }
        Err(CadenceError::MpvSocketTimeout(socket_path.clone()))
    }

    pub async fn load(&mut self, url: &str) -> Result<(), CadenceError> {
        self.request(json!({ "command": ["loadfile", url] })).await?;
        Ok(())
    }

    pub async fn pause(&mut self) -> Result<(), CadenceError> {
        self.set_property("pause", json!(true)).await
    }

    pub async fn resume(&mut self) -> Result<(), CadenceError> {
        self.set_property("pause", json!(false)).await
    }

    /// Halts playback and unloads the current file, returning mpv to idle.
    pub async fn stop(&mut self) -> Result<(), CadenceError> {
        self.request(json!({ "command": ["stop"] })).await?;
        Ok(())
    }

    pub async fn set_volume(&mut self, level: f64) -> Result<(), CadenceError> {
        self.set_property("volume", json!(level)).await
    }

    /// Seeks to an absolute position in the current track.
    pub async fn seek(&mut self, position_seconds: f64) -> Result<(), CadenceError> {
        self.set_property("time-pos", json!(position_seconds)).await
    }

    /// Forcefully terminates the mpv process. `kill_on_drop` alone is not
    /// enough for app shutdown: Tauri's normal exit path does not
    /// guarantee `Drop` runs (e.g. `std::process::exit`), so callers must
    /// invoke this explicitly on `RunEvent::ExitRequested`.
    pub async fn kill(&mut self) {
        let _ = self.process.kill().await;
    }

    pub async fn state(&mut self) -> Result<MpvStatus, CadenceError> {
        let paused = self
            .get_property("pause")
            .await?
            .as_bool()
            .unwrap_or(false);
        let volume = self.get_property("volume").await?.as_f64().unwrap_or(0.0);
        let position_seconds = self.get_optional_property_f64("time-pos").await?;
        let duration_seconds = self.get_optional_property_f64("duration").await?;

        Ok(MpvStatus {
            paused,
            volume,
            position_seconds,
            duration_seconds,
        })
    }

    async fn set_property(&mut self, name: &str, value: Value) -> Result<(), CadenceError> {
        self.request(json!({ "command": ["set_property", name, value] }))
            .await?;
        Ok(())
    }

    async fn get_property(&mut self, name: &str) -> Result<Value, CadenceError> {
        let response = self
            .request(json!({ "command": ["get_property", name] }))
            .await?;
        Ok(response.get("data").cloned().unwrap_or(Value::Null))
    }

    /// Like `get_property`, but treats mpv's "property unavailable" error
    /// (e.g. `time-pos` while idle with nothing loaded) as `None` rather
    /// than a failure.
    async fn get_optional_property_f64(&mut self, name: &str) -> Result<Option<f64>, CadenceError> {
        match self.request(json!({ "command": ["get_property", name] })).await {
            Ok(response) => Ok(response.get("data").and_then(Value::as_f64)),
            Err(CadenceError::MpvCommand { message, .. }) if message == "property unavailable" => {
                Ok(None)
            }
            Err(other) => Err(other),
        }
    }

    /// Sends one JSON IPC command and returns mpv's reply, skipping any
    /// unsolicited events mpv emits on the same socket in the meantime.
    async fn request(&mut self, command: Value) -> Result<Value, CadenceError> {
        let mut payload = serde_json::to_vec(&command).map_err(CadenceError::MpvSerialize)?;
        payload.push(b'\n');
        self.writer
            .write_all(&payload)
            .await
            .map_err(CadenceError::MpvIo)?;

        loop {
            let mut line = String::new();
            let bytes_read = self
                .reader
                .read_line(&mut line)
                .await
                .map_err(CadenceError::MpvIo)?;
            if bytes_read == 0 {
                return Err(CadenceError::MpvConnectionClosed);
            }

            let reply: Value = serde_json::from_str(&line).map_err(CadenceError::MpvSerialize)?;

            // mpv interleaves unsolicited events (e.g. "pause") with command
            // replies on the same socket; only replies carry an "error" key.
            let Some(error) = reply.get("error").and_then(Value::as_str) else {
                continue;
            };

            if error != "success" {
                return Err(CadenceError::MpvCommand {
                    command: command.to_string(),
                    message: error.to_string(),
                });
            }

            return Ok(reply);
        }
    }
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_socket_path() -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("cadence-test-{}-{id}.sock", std::process::id()))
    }

    #[tokio::test]
    async fn connects_and_reports_idle_state() {
        let mut player = MpvPlayer::connect(unique_socket_path())
            .await
            .expect("connect to mpv");

        let status = player.state().await.expect("query state");

        assert!(!status.paused);
        assert!((status.volume - 100.0).abs() < f64::EPSILON);
        assert_eq!(status.position_seconds, None);
        assert_eq!(status.duration_seconds, None);
    }

    #[tokio::test]
    async fn set_volume_is_reflected_in_state() {
        let mut player = MpvPlayer::connect(unique_socket_path())
            .await
            .expect("connect to mpv");

        player.set_volume(42.0).await.expect("set volume");
        let status = player.state().await.expect("query state");

        assert!((status.volume - 42.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn pause_and_resume_toggle_paused_state() {
        let mut player = MpvPlayer::connect(unique_socket_path())
            .await
            .expect("connect to mpv");

        player.pause().await.expect("pause");
        assert!(player.state().await.expect("query state").paused);

        player.resume().await.expect("resume");
        assert!(!player.state().await.expect("query state").paused);
    }

    #[tokio::test]
    async fn load_command_is_accepted_by_mpv() {
        let mut player = MpvPlayer::connect(unique_socket_path())
            .await
            .expect("connect to mpv");

        player
            .load("https://example.invalid/does-not-exist.mp3")
            .await
            .expect("mpv should acknowledge the loadfile command");
    }

    #[tokio::test]
    async fn stop_is_accepted_by_mpv() {
        let mut player = MpvPlayer::connect(unique_socket_path())
            .await
            .expect("connect to mpv");

        player.stop().await.expect("mpv should acknowledge stop");
    }

    #[tokio::test]
    async fn kill_terminates_the_mpv_process() {
        let mut player = MpvPlayer::connect(unique_socket_path())
            .await
            .expect("connect to mpv");
        let pid = player.process.id().expect("mpv should have a pid");

        player.kill().await;
        sleep(Duration::from_millis(200)).await;

        let still_running = std::path::Path::new(&format!("/proc/{pid}")).exists();
        assert!(!still_running, "mpv process {pid} should have been killed");
    }
}
