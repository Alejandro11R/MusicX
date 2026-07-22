use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CadenceError {
    #[error("failed to start mpv: {0}")]
    MpvSpawn(#[source] std::io::Error),

    #[error("timed out waiting for mpv's IPC socket at {0}")]
    MpvSocketTimeout(PathBuf),

    #[error("lost connection to mpv")]
    MpvConnectionClosed,

    #[error("I/O error communicating with mpv: {0}")]
    MpvIo(#[source] std::io::Error),

    #[error("failed to (de)serialize an mpv IPC message: {0}")]
    MpvSerialize(#[source] serde_json::Error),

    #[error("mpv rejected command {command}: {message}")]
    MpvCommand { command: String, message: String },

    #[error("failed to start yt-dlp: {0}")]
    YtDlpSpawn(#[source] std::io::Error),

    #[error("I/O error running yt-dlp: {0}")]
    YtDlpIo(#[source] std::io::Error),

    #[error("yt-dlp did not finish within the timeout")]
    YtDlpTimeout,

    #[error("yt-dlp exited with an error: {message}")]
    YtDlpExecution { message: String },

    #[error("failed to parse yt-dlp's output: {0}")]
    YtDlpParse(#[source] serde_json::Error),

    #[error("yt-dlp found no audio stream for video {video_id}")]
    YtDlpNoAudioStream { video_id: String },
}

// Tauri serializes a command's `Err` to send it to the frontend. There's no
// structured data the UI needs beyond the message, so this just serializes
// the Display output rather than mirroring the variant structure.
impl serde::Serialize for CadenceError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
