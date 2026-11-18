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

// ---- games (IGDB): judged by CONTENT only -------------------------------------------------------------
// Age ratings are unreliable in IGDB (missing, regional, inconsistent), so they are never requested and never
// used: not to allow and not to block. Only positive content signals hide a game.

/// IGDB theme ids.
pub const THEME_EROTIC: i64 = 42;
pub const THEME_HORROR: i64 = 19;
pub const THEME_WARFARE: i64 = 39;

/// Keywords that mean explicit sexual content (matched as parts of a lower-cased keyword). Extend freely.
pub const EXPLICIT_KEYWORD_PARTS: &[&str] = &[
    "hentai",
    "porn",
    "eroge",
    "erotic",
    "nsfw",
    "xxx",
    "lewd",
    "explicit sex",
    "sex game",
    "sex simulation",
    "sexual intercourse",
    "adults only",
    "adult game",
];
/// Keywords that are explicit only when they are the whole keyword.
pub const EXPLICIT_KEYWORDS_EXACT: &[&str] = &["sex", "sexual", "adult", "18+"];
/// Genres that mark an adult game.
pub const ADULT_GENRES: &[&str] = &["hentai", "adult", "erotic", "erotica", "pornographic"];
/// Extra keywords hidden only when `mature-themes` is off (survival-horror style content).
pub const HORROR_KEYWORD_PARTS: &[&str] = &[
    "survival horror",
    "psychological horror",
    "body horror",
    "gore",
    "splatter",
    "slasher",
];

/// What IGDB tells us about a game's content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameFacts {
    /// Theme "Erotic" (id 42).
    pub erotic: bool,
    /// A keyword from the explicit-sexual blacklist.
    pub explicit_keyword: bool,
    /// An adult genre.
    pub adult_genre: bool,
    /// Theme Horror (19), Warfare (39) or a survival-horror style keyword: shown unless `mature-themes` is off.
    pub mature_theme: bool,
}

fn lower(v: &[String]) -> Vec<String> {
    v.iter().map(|x| x.trim().to_lowercase()).collect()
}

/// Build the facts from the theme ids/names, genre names and keyword names the query returned (empty lists are fine:
/// IGDB leaves a field out when a game has nothing in it).
pub fn game_facts(
    theme_ids: &[i64],
    theme_names: &[String],
    genres: &[String],
    keywords: &[String],
) -> GameFacts {
    let themes = lower(theme_names);
    let keywords = lower(keywords);
    let has_theme =
        |id: i64, name: &str| theme_ids.contains(&id) || themes.iter().any(|t| t == name);
    GameFacts {
        erotic: has_theme(THEME_EROTIC, "erotic"),
        explicit_keyword: keywords.iter().any(|k| {
            EXPLICIT_KEYWORD_PARTS.iter().any(|p| k.contains(p))
                || EXPLICIT_KEYWORDS_EXACT.contains(&k.as_str())
        }),
        adult_genre: lower(genres)
            .iter()
            .any(|g| ADULT_GENRES.contains(&g.as_str())),
        mature_theme: has_theme(THEME_HORROR, "horror")
            || has_theme(THEME_WARFARE, "warfare")
            || keywords
                .iter()
                .any(|k| HORROR_KEYWORD_PARTS.iter().any(|p| k.contains(p))),
    }
}

/// Does the game carry a sexual-content signal (never allowed, whatever the settings)?
pub fn is_sexual(f: &GameFacts) -> bool {
    f.erotic || f.explicit_keyword || f.adult_genre
}

/// May a game with these facts be shown? Sexual signals hide it; violence and horror are allowed unless the server
/// turned `mature-themes` off. A game with no signals at all is allowed.
pub fn game_allowed(f: &GameFacts, allow_mature_themes: bool) -> bool {
    !is_sexual(f) && (allow_mature_themes || !f.mature_theme)
}

// ---- visual novels (VNDB) ----------------------------------------------------------------------------

/// VNDB tag categories that are sexual content ("ero").
pub const SEXUAL_TAG_CATEGORIES: &[&str] = &["ero"];
/// VNDB tag ids that are sexual content, on top of the category. Add ids here as they are found.
pub const SEXUAL_TAG_IDS: &[&str] = &[];
/// Tag names (lower-cased) treated as sexual content even when the category is missing.
pub const SEXUAL_TAG_NAMES: &[&str] = &[
    "sexual content",
    "explicit sex",
    "hentai",
    "ero",
    "nudity",
    "rape",
    "incest",
    "sex toys",
    "prostitution",
];
/// Character trait groups / names that are explicitly sexual.
pub const SEXUAL_TRAIT_GROUPS: &[&str] = &["sexual"];
pub const SEXUAL_TRAIT_NAMES: &[&str] = &[
    "sexual content",
    "explicit",
    "genitals",
    "pubic hair",
    "nipples",
];

/// What VNDB tells us about a visual novel or character.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VnFacts {
    /// Highest `sexual` flag over the cover and screenshots (0 safe, 1 suggestive, 2 explicit); `None` without images.
    pub image_sexual: Option<i64>,
    /// Highest `violence` flag over the cover and screenshots (0 tame, 1 violent, 2 brutal).
    pub image_violence: Option<i64>,
    /// A sexual-content tag (category, id or name).
    pub sexual_tag: bool,
    /// The tag list was part of the response (empty is a real answer; absence means no verdict is possible).
    pub tags_known: bool,
}

/// May a visual novel be shown? Hidden on a sexual tag or an image flagged above safe (an 18+ release is only judged
/// together with sexual tags, which already hide it, so release ages are not requested). Violence is allowed unless `mature-themes` is off, which also hides brutal images.
pub fn vn_allowed(f: &VnFacts, allow_mature_themes: bool) -> bool {
    f.tags_known
        && !f.sexual_tag
        && f.image_sexual.is_none_or(|s| s < 1)
        && (allow_mature_themes || f.image_violence.is_none_or(|v| v < 2))
}

/// Is this tag (category, id, lower-cased name) a sexual-content tag?
pub fn is_sexual_tag(category: Option<&str>, id: Option<&str>, name: Option<&str>) -> bool {
    category.is_some_and(|c| SEXUAL_TAG_CATEGORIES.contains(&c))
        || id.is_some_and(|i| SEXUAL_TAG_IDS.contains(&i))
        || name.is_some_and(|n| SEXUAL_TAG_NAMES.contains(&n.trim().to_lowercase().as_str()))
}

/// Is this character trait explicitly sexual?
pub fn is_sexual_trait(group: Option<&str>, name: Option<&str>) -> bool {
    group.is_some_and(|g| {
        SEXUAL_TRAIT_GROUPS
            .iter()
            .any(|s| g.to_lowercase().contains(s))
    }) || name.is_some_and(|n| SEXUAL_TRAIT_NAMES.contains(&n.trim().to_lowercase().as_str()))
}

/// Characters: hidden on an explicitly sexual trait or a picture above safe (and, with `mature-themes` off, brutal
/// pictures); they need at least one appearance (whose novels are vetted separately).
pub fn vn_character_allowed(
    image_sexual: Option<i64>,
    image_violence: Option<i64>,
    sexual_trait: bool,
    appearances: usize,
    allow_mature_themes: bool,
) -> bool {
    appearances > 0
        && !sexual_trait
        && image_sexual.is_none_or(|s| s < 1)
        && (allow_mature_themes || image_violence.is_none_or(|v| v < 2))
}
