use serde::{Deserialize, Serialize};

use super::search_result::SearchResult;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum PlaybackStatus {
    #[default]
    Stopped,
    Loading,
    Playing,
    Paused,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PlayerState {
    pub status: PlaybackStatus,
    pub current: Option<SearchResult>,
    pub position_seconds: f64,
    pub duration_seconds: f64,
    pub volume: f64,
    pub looping: bool,
    /// Si el socket con mpv está conectado. El frontend muestra un aviso de
    /// reconexión cuando pasa a `false`.
    #[serde(default = "default_connected")]
    pub connected: bool,
}

fn default_connected() -> bool {
    true
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn sample_track() -> SearchResult {
        SearchResult {
            id: "abc123".into(),
            title: "Numb".into(),
            artist: Some("Linkin Park".into()),
            duration_seconds: 187,
            thumbnail_url: "https://example.com/numb.jpg".into(),
        }
    }

    #[test]
    fn round_trips_through_json_with_current_track() {
        let state = PlayerState {
            status: PlaybackStatus::Playing,
            current: Some(sample_track()),
            position_seconds: 42.5,
            duration_seconds: 187.0,
            volume: 60.0,
            looping: true,
            connected: true,
        };

        let json = serde_json::to_string(&state).expect("serialize PlayerState");
        let restored: PlayerState = serde_json::from_str(&json).expect("deserialize PlayerState");

        assert_eq!(state, restored);
    }

    #[test]
    fn stopped_state_has_no_current_track() {
        let state = PlayerState {
            status: PlaybackStatus::Stopped,
            current: None,
            position_seconds: 0.0,
            duration_seconds: 0.0,
            volume: 100.0,
            looping: false,
            connected: true,
        };

        let json = serde_json::to_string(&state).expect("serialize PlayerState");
        let restored: PlayerState = serde_json::from_str(&json).expect("deserialize PlayerState");

        assert_eq!(restored.current, None);
        assert_eq!(restored.status, PlaybackStatus::Stopped);
    }

    #[test]
    fn default_impl_works() {
        let state = PlayerState::default();
        assert_eq!(state.status, PlaybackStatus::Stopped);
        assert_eq!(state.current, None);
        assert_eq!(state.position_seconds, 0.0);
        assert_eq!(state.duration_seconds, 0.0);
        assert_eq!(state.volume, 0.0);
        assert!(!state.looping);
        assert!(!state.connected);
    }

    #[test]
    fn missing_connected_field_defaults_to_true_for_old_payloads() {
        let json = r#"{
            "status": "Playing",
            "current": null,
            "position_seconds": 1.0,
            "duration_seconds": 2.0,
            "volume": 50.0,
            "looping": false
        }"#;
        let state: PlayerState = serde_json::from_str(json).expect("deserialize legacy state");
        assert!(state.connected);
    }
}
