use crate::models::lyrics::{LyricLine, LyricWord};

pub const MAX_LINES: usize = 2_000;

#[derive(Debug, Default, PartialEq)]
pub struct ParsedLrc {
    pub lines: Vec<LyricLine>,
    pub offset_ms: i32,
    pub synced: bool,
    pub title: Option<String>,
    pub artist: Option<String>,
}

pub fn parse_lrc(raw: &str) -> ParsedLrc {
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let mut parsed = ParsedLrc::default();
    let mut offset: i64 = 0;
    let mut timed: Vec<(u64, LyricLine)> = Vec::new();
    let mut plain: Vec<String> = Vec::new();

    for raw_line in raw.lines().take(MAX_LINES) {
        let line = raw_line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }

        let mut cursor = line;
        let mut times: Vec<u64> = Vec::new();
        let mut metadata_here = false;

        // Consume todos los `[...]` iniciales: timestamps y metadatos.
        while let Some(after_open) = cursor.strip_prefix('[') {
            let Some(close) = after_open.find(']') else { break };
            let inner = &after_open[..close];

            if let Some(ms) = parse_timestamp(inner) {
                times.push(ms);
                cursor = &after_open[close + 1..];
                continue;
            }

            // Solo tratamos como metadato si aún no hemos visto un timestamp.
            if times.is_empty() {
                if let Some((key, value)) = inner.split_once(':') {
                    if key.chars().all(|c| c.is_ascii_alphabetic()) {
                        match key.to_ascii_lowercase().as_str() {
                            "offset" => offset = value.trim().parse().unwrap_or(0),
                            "ti" => parsed.title = Some(value.trim().to_string()),
                            "ar" => parsed.artist = Some(value.trim().to_string()),
                            _ => {}
                        }
                        metadata_here = true;
                        cursor = &after_open[close + 1..];
                        continue;
                    }
                }
            }
            break;
        }

        let text = cursor.trim();
        if times.is_empty() {
            if !metadata_here && !text.is_empty() {
                plain.push(text.to_string());
            }
            continue;
        }
        if text.is_empty() {
            continue;
        }

        let (display, absolute_words) = parse_words(text, times[0]);
        let base = times[0];

        for &time in &times {
            let shift = time as i64 - base as i64;
            let words = absolute_words.as_ref().map(|words| {
                words
                    .iter()
                    .map(|word| LyricWord {
                        time_ms: (word.time_ms as i64 + shift).max(0) as u64,
                        end_ms: word
                            .end_ms
                            .map(|end| (end as i64 + shift).max(0) as u64),
                        text: word.text.clone(),
                    })
                    .collect()
            });

            timed.push((
                time,
                LyricLine {
                    time_ms: time,
                    text: display.clone(),
                    words,
                },
            ));
        }
    }

    if !timed.is_empty() {
        for (time, mut line) in timed {
            let adjusted = (time as i64 + offset).max(0) as u64;
            line.time_ms = adjusted;
            if let Some(words) = line.words.as_mut() {
                for word in words {
                    word.time_ms = (word.time_ms as i64 + offset).max(0) as u64;
                    word.end_ms = word
                        .end_ms
                        .map(|end| (end as i64 + offset).max(0) as u64);
                }
            }
            parsed.lines.push(line);
        }
        parsed.lines.sort_by_key(|line| line.time_ms);
        parsed
            .lines
            .dedup_by(|a, b| a.time_ms == b.time_ms && a.text == b.text);
        parsed.synced = true;
    } else if !plain.is_empty() {
        parsed.lines = plain
            .into_iter()
            .map(|text| LyricLine {
                time_ms: 0,
                text,
                words: None,
            })
            .collect();
        parsed.synced = false;
    }

    parsed.offset_ms = offset.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    parsed
}

/// Acepta `mm:ss`, `mm:ss.xx`, `mm:ss,xxx`, `hh:mm:ss.xx`.
fn parse_timestamp(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    let parts: Vec<&str> = raw.split(':').collect();

    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => (0u64, minutes.parse::<u64>().ok()?, *seconds),
        [hours, minutes, seconds] => (
            hours.parse::<u64>().ok()?,
            minutes.parse::<u64>().ok()?,
            *seconds,
        ),
        _ => return None,
    };

    let (whole, fraction) = match seconds.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => match seconds.split_once(',') {
            Some((whole, fraction)) => (whole, fraction),
            None => (seconds, ""),
        },
    };

    let secs = whole.parse::<u64>().ok()?;
    let millis = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<u64>().ok()? * 100,
        2 => fraction.parse::<u64>().ok()? * 10,
        _ => fraction.get(..3)?.parse::<u64>().ok()?,
    };

    Some((hours * 3_600 + minutes * 60 + secs) * 1_000 + millis)
}

fn parse_words(text: &str, line_time: u64) -> (String, Option<Vec<LyricWord>>) {
    if !text.contains('<') {
        return (text.trim().to_string(), None);
    }

    let mut plain = String::with_capacity(text.len());
    let mut words: Vec<LyricWord> = Vec::new();
    let mut current: u64 = line_time;
    let mut any_tag = false;

    for (index, chunk) in text.split('<').enumerate() {
        if index == 0 {
            plain.push_str(chunk);
            push_word(&mut words, current, chunk);
            continue;
        }

        let Some((tag, body)) = chunk.split_once('>') else {
            plain.push('<');
            plain.push_str(chunk);
            continue;
        };

        let Some(ms) = parse_timestamp(tag) else {
            plain.push('<');
            plain.push_str(chunk);
            continue;
        };

        any_tag = true;
        current = ms;
        plain.push_str(body);
        push_word(&mut words, current, body);
    }

    if !any_tag {
        return (text.trim().to_string(), None);
    }

    for index in 0..words.len().saturating_sub(1) {
        words[index].end_ms = Some(words[index + 1].time_ms);
    }

    (plain.trim().to_string(), Some(words))
}

fn push_word(words: &mut Vec<LyricWord>, time_ms: u64, raw: &str) {
    if raw.trim().is_empty() {
        return;
    }
    words.push(LyricWord {
        time_ms,
        end_ms: None,
        text: raw.to_string(),
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_and_sorts() {
        let parsed = parse_lrc("[00:10.50]Second\n[00:02.00]First\n");
        assert!(parsed.synced);
        assert_eq!(parsed.lines.len(), 2);
        assert_eq!(parsed.lines[0].time_ms, 2_000);
        assert_eq!(parsed.lines[1].text, "Second");
    }

    #[test]
    fn applies_offset_and_clamps_at_zero() {
        let parsed = parse_lrc("[offset:-500]\n[00:00.20]Early\n[00:01.00]Later\n");
        assert_eq!(parsed.offset_ms, -500);
        assert_eq!(parsed.lines[0].time_ms, 0);
        assert_eq!(parsed.lines[1].time_ms, 500);
    }

    #[test]
    fn expands_multiple_timestamps() {
        let parsed = parse_lrc("[00:01.00][00:05.00]Chorus\n");
        assert_eq!(parsed.lines.len(), 2);
        assert_eq!(parsed.lines[1].time_ms, 5_000);
        assert_eq!(parsed.lines[1].text, "Chorus");
    }

    #[test]
    fn parses_hours_and_comma_fraction() {
        let parsed = parse_lrc("[1:02:03,500]Long\n");
        assert_eq!(parsed.lines[0].time_ms, 3_723_500);
    }

    #[test]
    fn parses_word_sync() {
        let parsed = parse_lrc("[00:10.00]<00:10.00>Hello <00:10.50>world\n");
        let words = parsed.lines[0].words.as_ref().expect("words");
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].time_ms, 10_000);
        assert_eq!(words[0].end_ms, Some(10_500));
        assert_eq!(words[1].time_ms, 10_500);
        assert_eq!(parsed.lines[0].text, "Hello world");
    }

    #[test]
    fn plain_lyrics_have_no_timestamps() {
        let parsed = parse_lrc("Line one\nLine two\n");
        assert!(!parsed.synced);
        assert_eq!(parsed.lines.len(), 2);
        assert_eq!(parsed.lines[0].time_ms, 0);
    }

    #[test]
    fn metadata_only_is_empty() {
        let parsed = parse_lrc("[ti:Title]\n[ar:Artist]\n");
        assert!(parsed.lines.is_empty());
    }

    #[test]
    fn bracket_text_after_timestamp_is_kept() {
        let parsed = parse_lrc("[00:01.00][Chorus: loud]\n");
        assert_eq!(parsed.lines[0].text, "[Chorus: loud]");
    }
}
