use std::path::PathBuf;

use crate::errors::CadenceError;
use crate::models::player_state::{PlaybackStatus, PlayerState};
use crate::models::search_result::SearchResult;
use crate::services::mpv::MpvPlayer;
use crate::services::yt_dlp;

/// Coordinates the yt-dlp and mpv services into the playback operations the
/// rest of the app needs. Holds the one live mpv connection plus the
/// currently loaded track — mpv itself has no notion of *which*
/// `SearchResult` is playing, only a raw stream URL.
pub struct PlayerService {
    mpv: MpvPlayer,
    current: Option<SearchResult>,
}

impl PlayerService {
    pub async fn connect(socket_path: PathBuf) -> Result<Self, CadenceError> {
        let mpv = MpvPlayer::connect(socket_path).await?;
        Ok(Self { mpv, current: None })
    }

    /// Resolves `track`'s audio stream and starts playing it.
    pub async fn play(&mut self, track: SearchResult) -> Result<(), CadenceError> {
        let audio_url = yt_dlp::resolve_audio(&track.id).await?;
        self.mpv.load(&audio_url).await?;
        self.current = Some(track);
        Ok(())
    }

    pub async fn pause(&mut self) -> Result<(), CadenceError> {
        self.mpv.pause().await
    }

    pub async fn resume(&mut self) -> Result<(), CadenceError> {
        self.mpv.resume().await
    }

    pub async fn stop(&mut self) -> Result<(), CadenceError> {
        self.mpv.stop().await?;
        self.current = None;
        Ok(())
    }

    pub async fn set_volume(&mut self, level: f64) -> Result<(), CadenceError> {
        self.mpv.set_volume(level).await
    }

    pub async fn seek(&mut self, position_seconds: f64) -> Result<(), CadenceError> {
        self.mpv.seek(position_seconds).await
    }

    /// Terminates the underlying mpv process. Call this on app shutdown —
    /// see `MpvPlayer::kill` for why `Drop` alone isn't sufficient.
    pub async fn shutdown(&mut self) {
        self.mpv.kill().await;
    }

    pub async fn state(&mut self) -> Result<PlayerState, CadenceError> {
        let mpv_status = self.mpv.state().await?;

        let status = match (&self.current, mpv_status.paused) {
            (None, _) => PlaybackStatus::Stopped,
            (Some(_), true) => PlaybackStatus::Paused,
            (Some(_), false) => PlaybackStatus::Playing,
        };

        Ok(PlayerState {
            status,
            current: self.current.clone(),
            position_seconds: mpv_status.position_seconds.unwrap_or(0.0),
            duration_seconds: mpv_status.duration_seconds.unwrap_or(0.0),
            volume: mpv_status.volume,
        })
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
        std::env::temp_dir().join(format!("cadence-player-test-{}-{id}.sock", std::process::id()))
    }

    async fn sample_track() -> SearchResult {
        let mut results = yt_dlp::search("Imagine Dragons Believer", 1)
            .await
            .expect("search should succeed");
        results.remove(0)
    }

    /// Polls state() until mpv reports a known duration (i.e. has actually
    /// opened the stream, not just accepted the loadfile command), up to
    /// 10s. Panics if it never does — that's a real failure, not something
    /// to silently tolerate.
    async fn wait_for_stream_to_open(player: &mut PlayerService) {
        for _ in 0..20 {
            if player.state().await.expect("state").duration_seconds > 0.0 {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        panic!("stream never reported a duration within 10s");
    }

    #[tokio::test]
    async fn starts_stopped_with_no_current_track() {
        let mut player = PlayerService::connect(unique_socket_path())
            .await
            .expect("connect");

        let state = player.state().await.expect("state");

        assert_eq!(state.status, PlaybackStatus::Stopped);
        assert!(state.current.is_none());
    }

    #[tokio::test]
    async fn play_resolves_and_loads_the_track() {
        let mut player = PlayerService::connect(unique_socket_path())
            .await
            .expect("connect");
        let track = sample_track().await;
        let track_id = track.id.clone();

        player.play(track).await.expect("play");
        let state = player.state().await.expect("state");

        assert_eq!(state.status, PlaybackStatus::Playing);
        assert_eq!(state.current.map(|t| t.id), Some(track_id));
    }

    #[tokio::test]
    async fn pause_and_resume_are_reflected_in_state() {
        let mut player = PlayerService::connect(unique_socket_path())
            .await
            .expect("connect");
        player.play(sample_track().await).await.expect("play");

        player.pause().await.expect("pause");
        assert_eq!(
            player.state().await.expect("state").status,
            PlaybackStatus::Paused
        );

        player.resume().await.expect("resume");
        assert_eq!(
            player.state().await.expect("state").status,
            PlaybackStatus::Playing
        );
    }

    #[tokio::test]
    async fn stop_clears_the_current_track() {
        let mut player = PlayerService::connect(unique_socket_path())
            .await
            .expect("connect");
        player.play(sample_track().await).await.expect("play");

        player.stop().await.expect("stop");
        let state = player.state().await.expect("state");

        assert_eq!(state.status, PlaybackStatus::Stopped);
        assert!(state.current.is_none());
    }

    #[tokio::test]
    async fn seek_moves_the_playback_position() {
        let mut player = PlayerService::connect(unique_socket_path())
            .await
            .expect("connect");
        player.play(sample_track().await).await.expect("play");

        // Right after play() returns, only the loadfile command is
        // confirmed — mpv may not have opened the stream yet, and seeking
        // fails with "property unavailable" until it has. A fixed sleep
        // guessed wrong once already (500ms wasn't enough), so poll for
        // the actual signal (a known duration) instead of a fixed delay.
        wait_for_stream_to_open(&mut player).await;

        player.seek(30.0).await.expect("seek");

        // set_property("time-pos", ...) reports success as soon as mpv
        // queues the seek, not once it's actually landed — on a remote
        // stream that means fetching the new byte range first. Poll for
        // the real outcome instead of asserting immediately.
        let mut last_position = 0.0;
        for _ in 0..20 {
            last_position = player.state().await.expect("state").position_seconds;
            if (last_position - 30.0).abs() < 2.0 {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        panic!("expected position near 30s within 6s, got {last_position}");
    }
}
