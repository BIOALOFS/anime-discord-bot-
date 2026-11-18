//! MediaWiki sources (Fandom, wiki.gg, Wikipedia-style sites), global for every server and managed by the bot owner.
//! Only the official `api.php` is used, never HTML. `mod.rs` holds the shared types, the name normalisation used for
//! de-duplication and the address rules (HTTPS, public hosts only); the other modules parse, fetch and store.

pub mod admin;
pub mod client;
pub mod parse;
pub mod safety;
pub mod service;
pub mod store;

use serde::{Deserialize, Serialize};

/// What a wiki page is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Character,
    Game,
    Other,
}

impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Character => "character",
            Self::Game => "game",
            Self::Other => "other",
        }
    }

    pub fn from_key(k: &str) -> Self {
        match k {
            "character" => Self::Character,
            "game" => Self::Game,
            _ => Self::Other,
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
    pub game_category: Option<String>,
    pub enabled: bool,
    pub added_by: Option<i64>,
    pub created_at: i64,
    pub last_checked_at: Option<i64>,
    pub last_error: Option<String>,
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
pub fn same_identity(
    a_title: &str,
    a_parent: Option<&str>,
    b_title: &str,
    b_parent: Option<&str>,
) -> bool {
    if normalize(base_title(a_title)) != normalize(base_title(b_title))
        || normalize(base_title(a_title)).is_empty()
    {
        return false;
    }
    let pa = a_parent
        .or_else(|| title_qualifier(a_title))
        .map(normalize)
        .filter(|p| !p.is_empty());
    let pb = b_parent
        .or_else(|| title_qualifier(b_title))
        .map(normalize)
        .filter(|p| !p.is_empty());
    matches!((pa, pb), (Some(x), Some(y)) if x == y)
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
