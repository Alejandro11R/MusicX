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

    #[error("YouTube requires sign-in to resolve video {video_id}")]
    YtDlpAuthRequired { video_id: String },
}

impl CadenceError {
    /// A message safe to show the user: no raw process stderr, socket
    /// paths, or protocol internals — just enough to know what happened
    /// and whether trying again makes sense. `Display` (used for logs,
    /// e.g. the `eprintln!` in yt_dlp::resolve_audio) keeps the full detail;
    /// this is deliberately a separate, shorter message.
    fn user_message(&self) -> &'static str {
        match self {
            CadenceError::YtDlpAuthRequired { .. } => {
                "This video requires YouTube sign-in or has restrictions. Try another result."
            }
            CadenceError::YtDlpNoAudioStream { .. } => {
                "Couldn't find an audio stream for this track. Try another result."
            }
            CadenceError::YtDlpExecution { .. } => "Couldn't play this track. Try another result.",
            CadenceError::YtDlpTimeout => {
                "yt-dlp took too long to respond. Check your connection and try again."
            }
            CadenceError::YtDlpSpawn(_) | CadenceError::YtDlpIo(_) => {
                "yt-dlp isn't available. Make sure it's installed."
            }
            CadenceError::YtDlpParse(_) => "Got an unexpected response while searching.",
            CadenceError::MpvSpawn(_)
            | CadenceError::MpvIo(_)
            | CadenceError::MpvConnectionClosed
            | CadenceError::MpvSocketTimeout(_) => {
                "Lost connection to the audio player. Try restarting Cadence."
            }
            CadenceError::MpvSerialize(_) | CadenceError::MpvCommand { .. } => {
                "The audio player rejected that action."
            }
        }
    }
}

// Tauri serializes a command's `Err` to send it to the frontend, so this
// carries the user-facing message rather than the full Display output,
// which can include raw yt-dlp/mpv internals not fit for the UI.
impl serde::Serialize for CadenceError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.user_message())
    }
}
