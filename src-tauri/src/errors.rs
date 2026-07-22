use std::path::PathBuf;

use thiserror::Error;

// The `Mpv` prefix is redundant while mpv is the only subsystem with error
// variants; it stops being redundant once yt-dlp errors join this enum.
#[allow(clippy::enum_variant_names)]
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
}
