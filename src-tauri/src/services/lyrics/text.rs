pub fn normalize(raw: &str) -> String {
    raw.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Quita ruido de títulos de YouTube: "(Official Video)", "[HD]", " - Topic"…
/// Solo elimina grupos entre paréntesis cuyo contenido sea ruido conocido.
pub fn clean_title(raw: &str, artist: Option<&str>) -> String {
    const NOISE: &[&str] = &[
        "official", "oficial", "video", "videoclip", "audio", "lyric", "lyrics", "letra", "letras",
        "visualizer", "hd", "hq", "4k", "remaster", "remastered", "m/v", "mv", "explicit",
        "clean", "topic", "en vivo", "live",
    ];

    let mut output = String::with_capacity(raw.len());
    let mut depth = 0usize;
    let mut buffer = String::new();

    for character in raw.chars() {
        match character {
            '(' | '[' => {
                depth += 1;
                buffer.clear();
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let lower = buffer.to_lowercase();
                    if !NOISE.iter().any(|noise| lower.contains(noise)) {
                        output.push(' ');
                        output.push_str(&buffer);
                        output.push(' ');
                    }
                }
                buffer.clear();
            }
            _ if depth > 0 => buffer.push(character),
            _ => output.push(character),
        }
    }

    let mut cleaned = output
        .replace(" - Topic", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if let Some(artist) = artist {
        let prefix = format!("{} - ", artist.trim());
        if let (Some(head), Some(tail)) = (cleaned.get(..prefix.len()), cleaned.get(prefix.len()..)) {
            if head.eq_ignore_ascii_case(&prefix) {
                cleaned = tail.trim().to_string();
            }
        }
    }

    cleaned
}

pub fn clean_artist(raw: &str) -> String {
    raw.replace(" - Topic", "")
        .replace("VEVO", "")
        .trim()
        .to_string()
}

/// "Daft Punk - Get Lucky" → ("Get Lucky", Some("Daft Punk")).
pub fn split_artist_title(raw: &str) -> (String, Option<String>) {
    match raw.split_once(" - ") {
        Some((artist, title)) => (title.trim().to_string(), Some(artist.trim().to_string())),
        None => (raw.trim().to_string(), None),
    }
}
