//! Safety verdicts for the extra providers. Sexual content is always hidden; violence/horror is not a reason
//! to hide a game (Silent Hill stays visible) unless the server turned `mature-games` off. If a provider cannot
//! give a reliable verdict, the item is hidden.

use super::Work;

/// Re-check on results of the written-work fallbacks.
pub fn fallback_safe(w: &Work) -> bool {
    const BLOCKED: &[&str] = &[
        "adult",
        "hentai",
        "smut",
        "erotica",
        "pornographic",
        "lolicon",
        "shotacon",
    ];
    // Without any genre information a verdict is impossible, so the work stays hidden.
    !w.genres.is_empty()
        && !w
            .genres
            .iter()
            .any(|g| BLOCKED.contains(&g.to_lowercase().as_str()))
}

/// What IGDB tells us about a game, reduced to what the verdict needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameFacts {
    /// A theme named "Erotic" (id 42).
    pub erotic: bool,
    /// ESRB AO (adults only).
    pub esrb_ao: bool,
    /// ESRB M, PEGI 18 or CERO Z.
    pub mature: bool,
    /// At least one age rating was present.
    pub rated: bool,
}

const ESRB: i64 = 1;
const PEGI: i64 = 2;
const CERO: i64 = 3;
const ESRB_M: i64 = 11;
const ESRB_AO: i64 = 12;
const PEGI_18: i64 = 5;
const CERO_Z: i64 = 5;

/// `(category, rating)` pairs as IGDB returns them (legacy `age_ratings` fields).
pub fn facts_from(theme_ids: &[i64], theme_names: &[String], ratings: &[(i64, i64)]) -> GameFacts {
    let mut f = GameFacts {
        erotic: theme_ids.contains(&42)
            || theme_names.iter().any(|n| n.eq_ignore_ascii_case("erotic")),
        rated: !ratings.is_empty(),
        ..GameFacts::default()
    };
    for (category, rating) in ratings {
        match (*category, *rating) {
            (ESRB, ESRB_AO) => f.esrb_ao = true,
            (ESRB, ESRB_M) | (PEGI, PEGI_18) | (CERO, CERO_Z) => f.mature = true,
            _ => {}
        }
    }
    f
}

/// May a game with these facts be shown?
pub fn game_allowed(f: &GameFacts, allow_mature: bool) -> bool {
    if f.erotic || f.esrb_ao || !f.rated {
        return false;
    }
    allow_mature || !f.mature
}

/// What VNDB tells us about a visual novel or character.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VnFacts {
    /// Highest image `sexual` flag (0 safe, 1 suggestive, 2 explicit); `None` when there is no image.
    pub image_sexual: Option<i64>,
    /// A sexual-content ("ero" category) tag with a real rating.
    pub sexual_tag: bool,
    /// The tag list was part of the response (its absence means no verdict is possible).
    pub tags_known: bool,
}

/// VNDB flags: hide explicit/erotic images and sexual-content tags; unknown tags -> hide.
pub fn vn_allowed(f: &VnFacts) -> bool {
    f.tags_known && !f.sexual_tag && f.image_sexual.is_none_or(|s| s < 1)
}

/// Characters have no tags; judge by the picture and require known appearances.
pub fn vn_character_allowed(image_sexual: Option<i64>, appearances: usize) -> bool {
    appearances > 0 && image_sexual.is_none_or(|s| s < 1)
}
