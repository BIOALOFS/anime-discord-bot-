//! MediaWiki sources (Fandom, wiki.gg, Wikipedia-style sites), global for every server and managed by the bot owner.
//! Only the official `api.php` is used, never HTML. `mod.rs` holds the shared types, the name normalisation used for
//! de-duplication and the address rules (HTTPS, public hosts only); the other modules parse, fetch and store.

pub mod admin;
pub mod clean;
pub mod client;
pub mod fill;
pub mod msgs;
pub mod parse;
pub mod profile;
pub mod resolve;
pub mod safety;
pub mod service;
pub mod store;

use serde::{Deserialize, Serialize};

/// How long a card waits for a wiki to fill its gaps; the lookup goes on in the background after that.
pub const FILL_BUDGET: std::time::Duration = std::time::Duration::from_millis(1200);

/// What a wiki page is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Character,
    /// A work whose medium is a game.
    Game,
    Anime,
    /// Manga, manhwa, manhua and webtoons.
    Manga,
    Novel,
    /// Dramas and series.
    Series,
    Movie,
    /// A work whose medium the categories do not say: shown with the series name as written, never assumed a game.
    Work,
    Other,
}

impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Character => "character",
            Self::Game => "game",
            Self::Anime => "anime",
            Self::Manga => "manga",
            Self::Novel => "novel",
            Self::Series => "series",
            Self::Movie => "movie",
            Self::Work => "work",
            Self::Other => "other",
        }
    }

    pub fn from_key(k: &str) -> Self {
        match k {
            "character" => Self::Character,
            "game" => Self::Game,
            "anime" => Self::Anime,
            "manga" => Self::Manga,
            "novel" => Self::Novel,
            "series" => Self::Series,
            "movie" => Self::Movie,
            "work" => Self::Work,
            _ => Self::Other,
        }
    }

    /// Every kind that is a work (whatever its medium).
    pub const WORKS: [Kind; 6] = [
        Self::Game,
        Self::Anime,
        Self::Manga,
        Self::Novel,
        Self::Series,
        Self::Movie,
    ];

    /// Does a filter of this kind accept a page of kind `page`? `Work` accepts every medium.
    pub fn matches(self, page: Kind) -> bool {
        self == page || (self == Self::Work && page.is_work())
    }

    pub fn is_work(self) -> bool {
        self == Self::Work || Self::WORKS.contains(&self)
    }

    /// The medium named by a word of a category or a hint ("Manhwa", "Light novels", "TV series", "Films"...).
    pub fn medium_of(text: &str) -> Option<Kind> {
        let padded = format!(" {} ", normalize(text));
        let has = |words: &[&str]| words.iter().any(|w| padded.contains(&format!(" {w} ")));
        if has(&[
            "games",
            "game",
            "video games",
            "visual novels",
            "visual novel",
        ]) {
            Some(Self::Game)
        } else if has(&["manhwa", "manhua", "webtoon", "webtoons", "manga"]) {
            Some(Self::Manga)
        } else if has(&["light novels", "light novel", "novels", "novel"]) {
            Some(Self::Novel)
        } else if has(&["anime", "ova", "ona"]) {
            Some(Self::Anime)
        } else if has(&[
            "dramas",
            "drama",
            "kdrama",
            "tv series",
            "tv shows",
            "series",
        ]) {
            Some(Self::Series)
        } else if has(&["films", "film", "movies", "movie"]) {
            Some(Self::Movie)
        } else {
            None
        }
    }
}

/// One page of a wiki, as stored in the cache and shown on cards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub wiki_id: i64,
    pub page_id: i64,
    pub title: String,
    pub kind: Kind,
    pub extract: Option<String>,
    pub image_url: Option<String>,
    pub aliases: Vec<String>,
    /// The game or series the page belongs to.
    pub parent_title: Option<String>,
    pub categories: Vec<String>,
}

impl Page {
    /// The provider-style key that opens this page: `wiki:{wiki id}:{page id}`.
    pub fn key(&self) -> String {
        key(self.wiki_id, self.page_id)
    }

    pub fn normalized(&self) -> String {
        normalize(&self.title)
    }
}

pub fn key(wiki_id: i64, page_id: i64) -> String {
    format!("wiki:{wiki_id}:{page_id}")
}

/// `wiki:3:1234` → `(3, 1234)`.
pub fn parse_key(k: &str) -> Option<(i64, i64)> {
    let mut p = k.strip_prefix("wiki:")?.split(':');
    let (w, page) = (p.next()?.parse().ok()?, p.next()?.parse().ok()?);
    (p.next().is_none() && w > 0 && page > 0).then_some((w, page))
}

/// A wiki as defined by the owner.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Source {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub api_url: String,
    pub lang: String,
    pub license_name: Option<String>,
    pub license_url: Option<String>,
    pub character_category: Option<String>,
    pub work_category: Option<String>,
    /// Optional manual override of the medium of this wiki's works (game, anime, manga, novel, series, movie).
    pub kind_hint: Option<String>,
    pub enabled: bool,
    pub added_by: Option<i64>,
    pub created_at: i64,
    pub last_checked_at: Option<i64>,
    pub last_error: Option<String>,
}

impl Source {
    /// Does this wiki take part in searches for works/characters of this medium? A wiki with a known medium (its hint,
    /// or the name of its work category: Manhwa, Games...) serves that medium; a mixed wiki serves them all.
    pub fn serves(&self, want: Kind) -> bool {
        match parse::wiki_medium(self) {
            Some(m) => !want.is_work() || m == want || want == Kind::Work,
            None => true,
        }
    }
}

/// May a page of kind `page` of this wiki answer a search for `want` (a medium, `Work` for any, or `Character`)? A page
/// whose own categories name its medium must match it; anything else follows the wiki.
pub fn fits(src: &Source, page: Kind, want: Kind) -> bool {
    if want == Kind::Character {
        return page == Kind::Character && src.serves(Kind::Work);
    }
    if page.is_work() && page != Kind::Work {
        return want.matches(page);
    }
    page.is_work() && src.serves(want)
}

// ---- normalisation ---------------------------------------------------------------------------------

/// Lower-case, without accents or Arabic marks, with hyphens, apostrophes and punctuation read as spaces and spaces
/// collapsed: "Pyramid-Head" and "pyramid  head" are the same key, "Pyramid Head (Silent Hill 2)" is not.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let c = fold(c);
        match c {
            None => {}
            Some(c) if c.is_alphanumeric() => out.extend(c.to_lowercase()),
            Some(_) => out.push(' '),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A letter without its accent; combining marks and Arabic vowel signs disappear (`None`).
fn fold(c: char) -> Option<char> {
    if matches!(c, '\u{0300}'..='\u{036F}' | '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0640}') {
        return None;
    }
    let base = match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' | 'Ă' | 'Ą' => 'A',
        'ç' | 'ć' | 'č' => 'c',
        'Ç' | 'Ć' | 'Č' => 'C',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => 'e',
        'È' | 'É' | 'Ê' | 'Ë' | 'Ē' | 'Ę' | 'Ě' => 'E',
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'ı' => 'i',
        'Ì' | 'Í' | 'Î' | 'Ï' | 'Ī' | 'İ' => 'I',
        'ñ' | 'ń' | 'ň' => 'n',
        'Ñ' | 'Ń' | 'Ň' => 'N',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => 'o',
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'Ō' | 'Ő' => 'O',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => 'u',
        'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ū' | 'Ů' | 'Ű' => 'U',
        'ý' | 'ÿ' => 'y',
        'Ý' | 'Ÿ' => 'Y',
        'ś' | 'š' | 'ş' => 's',
        'Ś' | 'Š' | 'Ş' => 'S',
        'ž' | 'ź' | 'ż' => 'z',
        'Ž' | 'Ź' | 'Ż' => 'Z',
        'ł' => 'l',
        'Ł' => 'L',
        'ğ' => 'g',
        'Ğ' => 'G',
        'ß' => 's',
        'أ' | 'إ' | 'آ' => 'ا',
        'ى' => 'ي',
        'ة' => 'ه',
        other => other,
    };
    Some(base)
}

/// "Pyramid Head (Silent Hill 2)" → "Pyramid Head": the title without a trailing parenthetical.
pub fn base_title(t: &str) -> &str {
    match t.rfind(" (") {
        Some(i) if t.ends_with(')') => t[..i].trim_end(),
        _ => t.trim(),
    }
}

/// The parenthetical of a title, if any: "Pyramid Head (Silent Hill 2)" → "Silent Hill 2".
pub fn title_qualifier(t: &str) -> Option<&str> {
    let i = t.rfind(" (")?;
    t.ends_with(')')
        .then(|| t[i + 2..t.len() - 1].trim())
        .filter(|q| !q.is_empty())
}

/// Are these two titles about the same thing? The names must match after normalisation AND the game or series must
/// agree (both known and equal, after normalisation). Name-only similarity is not enough: rare duplicates are better
/// than wrong merges.
pub fn same_identity<'a>(
    a_title: &'a str,
    a_parent: Option<&'a str>,
    b_title: &'a str,
    b_parent: Option<&'a str>,
) -> bool {
    if normalize(base_title(a_title)) != normalize(base_title(b_title))
        || normalize(base_title(a_title)).is_empty()
    {
        return false;
    }
    let known = |p: Option<&'a str>| p.filter(|p| !normalize(p).is_empty());
    let pa = known(a_parent.or_else(|| title_qualifier(a_title)));
    let pb = known(b_parent.or_else(|| title_qualifier(b_title)));
    // The game or series must agree, compared flexibly ("Sun/Moon" = "Sun and Moon"); when unsure, they differ.
    matches!((pa, pb), (Some(x), Some(y)) if clean::series_matches(x, y))
}

// ---- addresses -------------------------------------------------------------------------------------

/// Why an address cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressError {
    Empty,
    NotHttps,
    NotPublic,
    Invalid,
}

/// A wiki address as the owner typed it: a page URL, a bare host or an `api.php` URL, normalised to the host and the
/// API addresses to try (`/api.php`, then `/w/api.php`). Only HTTPS and public host names are accepted.
pub fn normalize_input(input: &str) -> Result<(String, Vec<String>), AddressError> {
    let t = input.trim();
    if t.is_empty() {
        return Err(AddressError::Empty);
    }
    let with_scheme = if t.contains("://") {
        t.to_string()
    } else {
        format!("https://{t}")
    };
    let url = reqwest::Url::parse(&with_scheme).map_err(|_| AddressError::Invalid)?;
    if url.scheme() != "https" {
        return Err(AddressError::NotHttps);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AddressError::Invalid);
    }
    let host = url.host_str().ok_or(AddressError::Invalid)?.to_lowercase();
    if !public_host(&host) || url.port().is_some_and(|p| p != 443) {
        return Err(AddressError::NotPublic);
    }
    let path = url.path();
    let mut apis = Vec::new();
    if path.ends_with("/api.php") {
        apis.push(format!("https://{host}{path}"));
    }
    for p in ["/api.php", "/w/api.php"] {
        let a = format!("https://{host}{p}");
        if !apis.contains(&a) {
            apis.push(a);
        }
    }
    Ok((host, apis))
}

/// A host name that is not local or private: no `localhost`, no `.local`/`.internal`, no IP address at all (IPs are
/// refused outright, which also covers every private, loopback and link-local range), and a dot in the name.
pub fn public_host(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_lowercase();
    if h.is_empty() || h.parse::<std::net::IpAddr>().is_ok() || h.starts_with('[') {
        return false;
    }
    if !h.contains('.') || h == "localhost" {
        return false;
    }
    const LOCAL_SUFFIXES: &[&str] = &[
        ".localhost",
        ".local",
        ".internal",
        ".lan",
        ".home",
        ".intranet",
        ".corp",
        ".private",
        ".localdomain",
    ];
    if LOCAL_SUFFIXES.iter().any(|s| h.ends_with(s)) {
        return false;
    }
    h.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label.chars().all(|c| {
                c.is_ascii_alphanumeric() || c == '-' || (!c.is_ascii() && c.is_alphanumeric())
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_reject_everything_else() {
        assert_eq!(key(3, 1234), "wiki:3:1234");
        assert_eq!(parse_key("wiki:3:1234"), Some((3, 1234)));
        for bad in [
            "wiki:3",
            "wiki:3:x",
            "wiki:0:5",
            "wiki:3:1234:9",
            "anilist:3",
            "",
        ] {
            assert_eq!(parse_key(bad), None, "{bad}");
        }
    }

    #[test]
    fn normalisation_unifies_case_hyphens_apostrophes_accents_and_spaces() {
        assert_eq!(normalize("Pyramid-Head"), normalize("pyramid head"));
        assert_eq!(normalize("  Pyramid   Head "), "pyramid head");
        assert_eq!(normalize("O'Brien"), normalize("o brien"));
        assert_eq!(normalize("Pokémon"), "pokemon");
        assert_eq!(normalize("Ōkami"), "okami");
        assert_eq!(normalize("مُحَمَّد"), normalize("محمد"));
        assert_eq!(normalize("أحمد"), normalize("احمد"));
        assert_eq!(normalize("..."), "");
        assert_ne!(
            normalize("Pyramid Head (Silent Hill 2)"),
            normalize("Pyramid Head")
        );
    }

    #[test]
    fn titles_split_into_a_base_and_a_qualifier() {
        assert_eq!(base_title("Pyramid Head (Silent Hill 2)"), "Pyramid Head");
        assert_eq!(base_title("Heather"), "Heather");
        assert_eq!(
            title_qualifier("Pyramid Head (Silent Hill 2)"),
            Some("Silent Hill 2")
        );
        assert_eq!(title_qualifier("Heather"), None);
        assert_eq!(base_title("Dr. (weird"), "Dr. (weird");
    }

    #[test]
    fn identity_needs_the_same_name_and_the_same_game() {
        // the same name in the same game, written differently: one entry
        assert!(same_identity(
            "Pyramid Head",
            Some("Silent Hill 2"),
            "Pyramid-Head",
            Some("silent hill 2")
        ));
        assert!(same_identity(
            "Pyramid Head (Silent Hill 2)",
            None,
            "Pyramid Head",
            Some("Silent Hill 2")
        ));
        // the same name in different games stays separate
        assert!(!same_identity(
            "Pyramid Head",
            Some("Silent Hill 2"),
            "Pyramid Head",
            Some("Silent Hill: Homecoming")
        ));
        // name-only similarity (a game unknown on either side) stays separate
        assert!(!same_identity(
            "Pyramid Head",
            None,
            "Pyramid Head",
            Some("Silent Hill 2")
        ));
        assert!(!same_identity("Pyramid Head", None, "Pyramid Head", None));
        // different names never merge
        assert!(!same_identity(
            "Pyramid Head",
            Some("Silent Hill 2"),
            "Red Pyramid Thing",
            Some("Silent Hill 2")
        ));
        assert!(!same_identity("", Some("x"), "", Some("x")));
    }

    #[test]
    fn addresses_are_normalised_to_a_host_and_api_candidates() {
        let (host, apis) =
            normalize_input("https://silenthill.fandom.com/wiki/Pyramid_Head").expect("page url");
        assert_eq!(host, "silenthill.fandom.com");
        assert_eq!(
            apis,
            vec![
                "https://silenthill.fandom.com/api.php",
                "https://silenthill.fandom.com/w/api.php"
            ]
        );
        assert_eq!(
            normalize_input("silenthill.fandom.com")
                .expect("bare host")
                .0,
            "silenthill.fandom.com"
        );
        let (_, apis) =
            normalize_input("https://x.wiki.gg/w/api.php?action=query").expect("api url");
        assert_eq!(
            apis[0], "https://x.wiki.gg/w/api.php",
            "a given api.php is tried first"
        );
        assert_eq!(apis.len(), 2);
        assert_eq!(
            normalize_input("HTTPS://Wiki.GG/").expect("case").0,
            "wiki.gg"
        );
    }

    #[test]
    fn dangerous_addresses_are_refused() {
        assert_eq!(normalize_input(""), Err(AddressError::Empty));
        assert_eq!(
            normalize_input("http://silenthill.fandom.com"),
            Err(AddressError::NotHttps)
        );
        for bad in [
            "https://localhost/api.php",
            "https://127.0.0.1/api.php",
            "https://10.0.0.5/api.php",
            "https://192.168.1.1/",
            "https://169.254.169.254/latest/meta-data",
            "https://[::1]/api.php",
            "https://intranet/api.php",
            "https://wiki.internal/api.php",
            "https://printer.local/",
            "https://example.com:8443/api.php",
            "https://user:pass@example.com/",
            "https://2130706433/",
        ] {
            assert!(normalize_input(bad).is_err(), "{bad} must be refused");
        }
        assert!(normalize_input("https://a b.com/").is_err());
        assert!(!public_host("fandom"));
        assert!(public_host("silenthill.fandom.com"));
    }
}
