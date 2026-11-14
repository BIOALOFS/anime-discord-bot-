//! Pure adult-content filtering. Fail-closed: anything we cannot verify as safe is hidden.

use super::models::{Character, Media, MediaRef};

const ADULT_GENRE: &str = "Hentai";

/// Terms that make a description unfit to show (matched as whole lowercase words).
const EXPLICIT_TERMS: &[&str] = &[
    "sex",
    "sexual",
    "sexually",
    "rape",
    "raped",
    "rapist",
    "porn",
    "porno",
    "pornographic",
    "hentai",
    "nude",
    "naked",
    "erotic",
    "erotica",
    "orgasm",
    "genitals",
    "penis",
    "vagina",
    "incest",
    "fetish",
    "bdsm",
    "molest",
    "molested",
    "lewd",
    "masturbation",
    "nsfw",
];

fn genres_adult(genres: &[String]) -> bool {
    genres.iter().any(|g| g.eq_ignore_ascii_case(ADULT_GENRE))
}

pub fn media_allowed(m: &Media, allow_adult: bool) -> bool {
    allow_adult || !(m.is_adult || genres_adult(&m.genres))
}

fn appearance_adult(r: &MediaRef) -> bool {
    r.is_adult || genres_adult(&r.genres)
}

/// AniList has no adult flag on characters, so judge by appearances: hidden if ANY
/// appearance is adult. A character with no known appearances cannot be verified
/// and is hidden. Only the 25 most popular appearances are inspected.
pub fn character_allowed(c: &Character, allow_adult: bool) -> bool {
    if allow_adult {
        return true;
    }
    match &c.media {
        Some(m) if !m.nodes.is_empty() => !m.nodes.iter().any(appearance_adult),
        _ => false,
    }
}

/// Strip HTML, turn AniList spoilers into Discord spoilers, drop explicit text, truncate.
/// Returns `None` when there is nothing safe to show.
pub fn clean_description(raw: &str, max_chars: usize) -> Option<String> {
    let text = strip_html(raw).replace("~!", "||").replace("!~", "||");
    let text = collapse_newlines(text.trim());
    if text.is_empty() || has_explicit_term(&text) {
        return None;
    }
    Some(close_spoilers(truncate_words(&text, max_chars)))
}

fn has_explicit_term(text: &str) -> bool {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| EXPLICIT_TERMS.contains(&w))
}

fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut tag = String::new();
    let mut in_tag = false;
    for ch in s.chars() {
        match (in_tag, ch) {
            (false, '<') => {
                in_tag = true;
                tag.clear();
            }
            (true, '>') => {
                in_tag = false;
                if tag.trim_start_matches('/').to_lowercase().starts_with("br") {
                    out.push('\n');
                }
            }
            (true, c) => tag.push(c),
            (false, c) => out.push(c),
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&amp;", "&")
}

fn collapse_newlines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut run = 0;
    for ch in s.chars() {
        if ch == '\n' {
            run += 1;
            if run <= 2 {
                out.push(ch);
            }
        } else {
            run = 0;
            out.push(ch);
        }
    }
    out
}

/// Truncate to `max` chars at a word boundary, adding an ellipsis when cut.
pub fn truncate_words(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    let cut = head.rfind(char::is_whitespace).unwrap_or(head.len());
    format!("{}…", head[..cut].trim_end())
}

/// A cut can leave a spoiler open, which would spoil the rest of the embed.
fn close_spoilers(s: String) -> String {
    if s.matches("||").count() % 2 == 1 {
        format!("{s}||")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn media(adult: bool, genres: &[&str]) -> Media {
        serde_json::from_value(json!({"id": 1, "isAdult": adult, "genres": genres})).expect("media")
    }

    fn character(media: serde_json::Value) -> Character {
        serde_json::from_value(json!({"id": 7, "media": {"nodes": media}})).expect("character")
    }

    #[test]
    fn media_filter() {
        assert!(media_allowed(&media(false, &["Action"]), false));
        assert!(!media_allowed(&media(true, &["Action"]), false));
        assert!(!media_allowed(&media(false, &["Hentai"]), false));
        assert!(!media_allowed(&media(false, &["hentai"]), false));
        assert!(media_allowed(&media(true, &["Hentai"]), true));
    }

    #[test]
    fn safe_character() {
        let c = character(json!([{"isAdult": false, "genres": ["Comedy"]}]));
        assert!(character_allowed(&c, false));
    }

    #[test]
    fn adult_linked_character() {
        let c = character(json!([{"isAdult": true, "genres": []}]));
        assert!(!character_allowed(&c, false));
        assert!(character_allowed(&c, true));
        let h = character(json!([{"isAdult": false, "genres": ["Hentai"]}]));
        assert!(!character_allowed(&h, false));
    }

    #[test]
    fn mixed_appearances_hidden() {
        let c = character(json!([
            {"isAdult": false, "genres": ["Action"]},
            {"isAdult": true, "genres": ["Ecchi"]},
            {"isAdult": false, "genres": ["Drama"]}
        ]));
        assert!(!character_allowed(&c, false));
    }

    #[test]
    fn unverifiable_character_hidden() {
        let none: Character = serde_json::from_value(json!({"id": 1})).expect("c");
        assert!(!character_allowed(&none, false));
        assert!(!character_allowed(&character(json!([])), false));
    }

    #[test]
    fn description_cleaning() {
        let d = clean_description("Hello<br>world &amp; ~!secret!~", 300).expect("d");
        assert_eq!(d, "Hello\nworld & ||secret||");
        assert!(clean_description("a sexual scene", 300).is_none());
        assert!(clean_description("Essex is a county", 300).is_some());
        assert!(clean_description("   ", 300).is_none());
    }

    #[test]
    fn truncation_keeps_spoilers_balanced() {
        let raw = format!("start ~!{}!~ end", "word ".repeat(100));
        let d = clean_description(&raw, 50).expect("d");
        assert!(d.chars().count() <= 52);
        assert_eq!(d.matches("||").count() % 2, 0);
        assert!(d.contains('…'));
    }
}
