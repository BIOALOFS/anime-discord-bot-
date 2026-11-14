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

pub fn appearance_adult(r: &MediaRef) -> bool {
    r.is_adult || genres_adult(&r.genres)
}

/// AniList has no adult flag on characters, so judge by appearances: hidden if ANY
/// fetched appearance is adult or Hentai. A character with no known appearances
/// cannot be verified and is hidden. When more appearances exist than were fetched
/// (`hasNextPage`) and none of the fetched ones is adult, the character is still safe.
pub fn character_allowed(c: &Character, allow_adult: bool) -> bool {
    if allow_adult {
        return true;
    }
    match &c.media {
        Some(m) if !m.nodes.is_empty() => !m.nodes.iter().any(appearance_adult),
        _ => false,
    }
}

pub fn contains_explicit_term(text: &str) -> bool {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| EXPLICIT_TERMS.contains(&w))
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
    fn zero_appearances_hidden() {
        let none: Character = serde_json::from_value(json!({"id": 1})).expect("c");
        assert!(!character_allowed(&none, false));
        assert!(!character_allowed(&character(json!([])), false));
    }

    #[test]
    fn more_than_fifty_appearances() {
        let safe_nodes: Vec<_> = (0..50)
            .map(|_| json!({"isAdult": false, "genres": ["Action"]}))
            .collect();
        let paged = |nodes: Vec<serde_json::Value>| -> Character {
            serde_json::from_value(
                json!({"id": 9, "media": {"nodes": nodes, "pageInfo": {"hasNextPage": true}}}),
            )
            .expect("character")
        };
        // 50 fetched, none adult, more exist: still safe.
        assert!(character_allowed(&paged(safe_nodes.clone()), false));
        // An adult appearance anywhere in the fetched 50: unsafe.
        let mut with_adult = safe_nodes;
        with_adult[49] = json!({"isAdult": true, "genres": []});
        assert!(!character_allowed(&paged(with_adult), false));
    }

    #[test]
    fn explicit_terms_match_whole_words() {
        assert!(contains_explicit_term("a sexual scene"));
        assert!(!contains_explicit_term("Essex is a county"));
    }
}
