use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub duration_seconds: u64,
    pub thumbnail_url: String,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let result = SearchResult {
            id: "7wtfhZwyrcc".into(),
            title: "Believer".into(),
            artist: Some("Imagine Dragons".into()),
            duration_seconds: 217,
            thumbnail_url: "https://i.ytimg.com/vi/7wtfhZwyrcc/maxresdefault.jpg".into(),
        };

        let json = serde_json::to_string(&result).expect("serialize SearchResult");
        let restored: SearchResult = serde_json::from_str(&json).expect("deserialize SearchResult");

        assert_eq!(result, restored);
    }

    #[test]
    fn artist_is_optional() {
        let json = r#"{
            "id": "abc123",
            "title": "Some Mix",
            "artist": null,
            "duration_seconds": 3600,
            "thumbnail_url": "https://example.com/thumb.jpg"
        }"#;

        let result: SearchResult = serde_json::from_str(json).expect("deserialize SearchResult");
        assert_eq!(result.artist, None);
    }
}
