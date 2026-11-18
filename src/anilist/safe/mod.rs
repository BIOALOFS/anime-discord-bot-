//! The only AniList API the rest of the bot may use. Every method returns filtered data only,
//! so no command can bypass the safety rules (see the architecture test in `commands`).

use super::{
    client::{parse, AniListClient, Which},
    models::*,
    queries, safety, AniError,
};
use crate::config::Config;
use moka::future::Cache;
use serde_json::{json, Value};
use std::time::Duration;

/// Per-guild content policy. Always `SAFE` until `/settings` is wired up in milestone 8.
#[derive(Debug, Clone, Copy, Default)]
pub struct ContentPolicy {
    pub allow_adult: bool,
}

impl ContentPolicy {
    pub const SAFE: Self = Self { allow_adult: false };
}

/// What the user searches for. AniList files manhwa/manhua/novels under MANGA, told apart by
/// `countryOfOrigin` (JP manga, KR manhwa, CN manhua) and `format` (NOVEL).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaKind {
    Anime,
    Manga,
    Manhwa,
    Manhua,
    Novel,
}

impl MediaKind {
    pub const ALL: [MediaKind; 5] = [
        Self::Anime,
        Self::Manga,
        Self::Manhwa,
        Self::Manhua,
        Self::Novel,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Anime => "anime",
            Self::Manga => "manga",
            Self::Manhwa => "manhwa",
            Self::Manhua => "manhua",
            Self::Novel => "novel",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }

    fn type_str(self) -> &'static str {
        if self == Self::Anime {
            "ANIME"
        } else {
            "MANGA"
        }
    }

    fn country(self) -> Option<&'static str> {
        match self {
            Self::Manga => Some("JP"),
            Self::Manhwa => Some("KR"),
            Self::Manhua => Some("CN"),
            _ => None,
        }
    }

    fn formats(self) -> Option<&'static [&'static str]> {
        match self {
            Self::Novel => Some(&["NOVEL"]),
            Self::Manga | Self::Manhwa | Self::Manhua => Some(&["MANGA", "ONE_SHOT"]),
            Self::Anime => None,
        }
    }

    /// Chapters/volumes instead of episodes.
    pub fn is_written(self) -> bool {
        self != Self::Anime
    }
}

/// The kind a media belongs to, from its format and country.
pub fn kind_of(m: &Media) -> MediaKind {
    match (m.format.as_deref(), m.country_of_origin.as_deref()) {
        (Some("NOVEL"), _) => MediaKind::Novel,
        (Some("MANGA" | "ONE_SHOT"), Some("KR")) => MediaKind::Manhwa,
        (Some("MANGA" | "ONE_SHOT"), Some("CN" | "TW")) => MediaKind::Manhua,
        (Some("MANGA" | "ONE_SHOT"), _) => MediaKind::Manga,
        _ => MediaKind::Anime,
    }
}

/// Optional narrowing of a category search: AniList format (`TV`, `MOVIE`...), year and status (`FINISHED`...).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaFilter {
    pub format: Option<String>,
    pub year: Option<i32>,
    pub status: Option<String>,
    /// Country of origin (`JP`, `KR`, `CN`), for categories that do not fix one (novels).
    pub country: Option<String>,
}

impl MediaFilter {
    pub fn is_empty(&self) -> bool {
        self.format.is_none()
            && self.year.is_none()
            && self.status.is_none()
            && self.country.is_none()
    }

    pub fn cache_key(&self) -> String {
        format!(
            "{}/{}/{}/{}",
            self.format.as_deref().unwrap_or(""),
            self.year.unwrap_or(0),
            self.status.as_deref().unwrap_or(""),
            self.country.as_deref().unwrap_or("")
        )
    }

    /// Does a work found by name pass the filter? (Searches cannot be narrowed server-side, so they are checked here.)
    pub fn matches(&self, m: &Media) -> bool {
        self.format.as_deref().is_none_or(|f| {
            m.format
                .as_deref()
                .is_some_and(|mf| mf.eq_ignore_ascii_case(f))
        }) && self.year.is_none_or(|y| m.year() == Some(y))
            && self.status.as_deref().is_none_or(|s| {
                m.status
                    .as_deref()
                    .is_some_and(|ms| ms.eq_ignore_ascii_case(s))
            })
            && self.country.as_deref().is_none_or(|c| {
                m.country_of_origin
                    .as_deref()
                    .is_some_and(|mc| mc.eq_ignore_ascii_case(c))
            })
    }
}

fn kind_vars(mut vars: Value, kind: MediaKind) -> Value {
    vars["type"] = json!(kind.type_str());
    if let Some(c) = kind.country() {
        vars["country"] = json!(c);
    }
    if let Some(f) = kind.formats() {
        vars["formatIn"] = json!(f);
    }
    vars
}

#[derive(Debug, Clone)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub has_next: bool,
    pub last_page: Option<usize>,
}

/// Result of looking up a character by id; `Hidden` lets callers show the
/// "inappropriate items hidden" notice instead of pretending it does not exist.
#[derive(Debug, Clone)]
pub enum CharacterLookup {
    Found(Box<Character>),
    Hidden,
    NotFound,
}

#[derive(Debug, Clone)]
pub struct AiringEntry {
    pub airing_at: i64,
    pub episode: i32,
    pub media: Media,
}

#[derive(Debug, Clone)]
pub struct AiringInfo {
    pub media: Media,
    pub previous_at: Option<i64>,
}

/// One AniList page (25) of a work's cast. `index` is the character's rank in the full cast (0-based);
/// hidden characters leave gaps so numbering stays stable across pages.
#[derive(Debug, Clone)]
pub struct CastPage {
    pub total: usize,
    pub entries: Vec<(usize, Character, String)>,
}

/// AniList page size used for the cast query.
pub const CAST_PAGE_SIZE: usize = 25;

pub struct SafeAniList {
    client: AniListClient,
    /// Safety verdict per character id (true = safe to show), from the 50-appearance window.
    verdicts: Cache<i64, bool>,
}

fn adult_vars(mut vars: Value, policy: ContentPolicy) -> Value {
    if !policy.allow_adult {
        vars["isAdult"] = json!(false);
        vars["genreNotIn"] = json!(["Hentai"]);
    }
    vars
}

fn filter_media(list: Vec<Media>, policy: ContentPolicy) -> Vec<Media> {
    list.into_iter()
        .filter(|m| safety::media_allowed(m, policy.allow_adult))
        .collect()
}

fn page_flags(doc: &Value, base: &str) -> (bool, Option<usize>) {
    let has_next = doc
        .pointer(&format!("{base}/pageInfo/hasNextPage"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let last = doc
        .pointer(&format!("{base}/pageInfo/lastPage"))
        .and_then(Value::as_u64)
        .map(|n| n as usize);
    (has_next, last)
}

fn pick<T>(mut v: Vec<T>) -> Option<T> {
    if v.is_empty() {
        return None;
    }
    let i = rand::random_range(0..v.len());
    Some(v.swap_remove(i))
}

/// Most recent episode that already aired at `now`.
pub fn previous_airing(nodes: &[AiringNode], now: i64) -> Option<i64> {
    nodes
        .iter()
        .map(|n| n.airing_at)
        .filter(|t| *t <= now)
        .max()
}

impl SafeAniList {
    pub fn new(cfg: &Config) -> anyhow::Result<Self> {
        Ok(Self::from_client(AniListClient::new(cfg)?, cfg))
    }

    /// Test constructor for other modules: answers every query from `f` instead of the network.
    #[cfg(test)]
    pub fn with_fixture(
        f: impl Fn(&str, &Value) -> Result<Value, AniError> + Send + Sync + 'static,
    ) -> Self {
        Self::with_mock(Box::new(f))
    }

    #[cfg(test)]
    pub(super) fn with_mock(mock: super::client::Mock) -> Self {
        let cfg = Config::for_tests();
        let client = AniListClient::with_mock(&cfg, mock).expect("test client");
        Self::from_client(client, &cfg)
    }

    fn from_client(client: AniListClient, cfg: &Config) -> Self {
        Self {
            client,
            verdicts: Cache::builder()
                .max_capacity(50_000)
                .time_to_live(Duration::from_secs(cfg.cache_ttl_details_secs))
                .build(),
        }
    }

    // ---- media -----------------------------------------------------------

    pub async fn search_media(
        &self,
        query: &str,
        kind: MediaKind,
        per_page: u8,
        policy: ContentPolicy,
    ) -> Result<Vec<Media>, AniError> {
        let vars = adult_vars(
            kind_vars(json!({"search": query, "perPage": per_page.min(10)}), kind),
            policy,
        );
        let key = format!(
            "sm:{}:{}:{per_page}:{}",
            kind.key(),
            query.to_lowercase(),
            policy.allow_adult
        );
        let list: Vec<Media> = match self
            .client
            .get(Which::Search, key, queries::SEARCH_MEDIA, vars)
            .await?
        {
            Some(doc) => parse(&doc, "/Page/media")?,
            None => Vec::new(),
        };
        Ok(filter_media(list, policy))
    }

    /// The most popular safe works of `kind` (shared cache with nothing but the kind in its key).
    pub async fn popular_media(
        &self,
        kind: MediaKind,
        per_page: u8,
        policy: ContentPolicy,
    ) -> Result<Vec<Media>, AniError> {
        let vars = adult_vars(
            kind_vars(json!({"perPage": per_page.min(10)}), kind),
            policy,
        );
        let key = format!("pm:{}:{per_page}:{}", kind.key(), policy.allow_adult);
        let list: Vec<Media> = match self
            .client
            .get(Which::Search, key, queries::POPULAR_MEDIA, vars)
            .await?
        {
            Some(doc) => parse(&doc, "/Page/media")?,
            None => Vec::new(),
        };
        Ok(filter_media(list, policy))
    }

    /// The most-loved safe characters.
    pub async fn popular_characters(
        &self,
        per_page: u8,
        policy: ContentPolicy,
    ) -> Result<Vec<Character>, AniError> {
        let key = format!("pc:{per_page}");
        let list: Vec<Character> = match self
            .client
            .get(
                Which::Search,
                key,
                queries::POPULAR_CHARACTERS,
                json!({"perPage": per_page.min(10)}),
            )
            .await?
        {
            Some(doc) => parse(&doc, "/Page/characters")?,
            None => Vec::new(),
        };
        Ok(self.visible(list, policy).await)
    }

    /// Search across anime and every written kind at once (profile favourites).
    pub async fn search_any(
        &self,
        query: &str,
        per_page: u8,
        policy: ContentPolicy,
    ) -> Result<Vec<Media>, AniError> {
        let vars = adult_vars(
            json!({"search": query, "perPage": per_page.min(10)}),
            policy,
        );
        let key = format!(
            "sa:{}:{per_page}:{}",
            query.to_lowercase(),
            policy.allow_adult
        );
        let list: Vec<Media> = match self
            .client
            .get(Which::Search, key, queries::SEARCH_MEDIA, vars)
            .await?
        {
            Some(doc) => parse(&doc, "/Page/media")?,
            None => Vec::new(),
        };
        Ok(filter_media(list, policy))
    }

    /// `None` when missing or hidden by the safety filter.
    pub async fn media_by_id(
        &self,
        id: i64,
        policy: ContentPolicy,
    ) -> Result<Option<Media>, AniError> {
        let media: Option<Media> = match self
            .client
            .get(
                Which::Details,
                format!("m:{id}"),
                queries::MEDIA_BY_ID,
                json!({ "id": id }),
            )
            .await?
        {
            Some(doc) => Some(parse(&doc, "/Media")?),
            None => None,
        };
        Ok(media.filter(|m| safety::media_allowed(m, policy.allow_adult)))
    }

    /// Main/supporting characters of a media, 8 per page, each judged by its own appearances.
    pub async fn media_characters(
        &self,
        id: i64,
        page: u32,
        policy: ContentPolicy,
    ) -> Result<Page<(Character, String)>, AniError> {
        let key = format!("mc:{id}:{page}");
        let vars = json!({"id": id, "page": page});
        let Some(doc) = self
            .client
            .get(Which::Details, key, queries::MEDIA_CHARACTERS, vars)
            .await?
        else {
            return Ok(Page {
                items: Vec::new(),
                has_next: false,
                last_page: None,
            });
        };
        let edges: Vec<CharacterEdge> = parse(&doc, "/Media/characters/edges")?;
        let (has_next, last_page) = page_flags(&doc, "/Media/characters");
        let items = edges
            .into_iter()
            .filter(|e| safety::character_allowed(&e.node, policy.allow_adult))
            .map(|e| (e.node, e.role.unwrap_or_default()))
            .collect();
        Ok(Page {
            items,
            has_next,
            last_page,
        })
    }

    /// One page of the cast, each character judged by its own appearances (cached 6 h per page).
    pub async fn work_cast_page(
        &self,
        id: i64,
        page: usize,
        policy: ContentPolicy,
    ) -> Result<CastPage, AniError> {
        let key = format!("wc:{id}:{page}");
        let vars = json!({"id": id, "page": page});
        let Some(doc) = self
            .client
            .get(Which::Details, key, queries::WORK_CAST, vars)
            .await?
        else {
            return Ok(CastPage {
                total: 0,
                entries: Vec::new(),
            });
        };
        let edges: Vec<CharacterEdge> = parse(&doc, "/Media/characters/edges")?;
        let total = doc
            .pointer("/Media/characters/pageInfo/total")
            .and_then(Value::as_u64)
            .map_or(edges.len(), |n| n as usize);
        let base = page.saturating_sub(1) * CAST_PAGE_SIZE;
        let entries = edges
            .into_iter()
            .enumerate()
            .filter(|(_, e)| safety::character_allowed(&e.node, policy.allow_adult))
            .map(|(i, e)| (base + i, e.node, e.role.unwrap_or_default()))
            .collect();
        Ok(CastPage { total, entries })
    }

    pub async fn recommendations(
        &self,
        id: i64,
        page: u32,
        policy: ContentPolicy,
    ) -> Result<Page<Media>, AniError> {
        let key = format!("rec:{id}:{page}");
        let Some(doc) = self
            .client
            .get(
                Which::Details,
                key,
                queries::RECOMMENDATIONS,
                json!({"id": id, "page": page}),
            )
            .await?
        else {
            return Ok(Page {
                items: Vec::new(),
                has_next: false,
                last_page: None,
            });
        };
        let nodes: Vec<Value> = parse(&doc, "/Media/recommendations/nodes")?;
        let list = nodes
            .into_iter()
            .filter_map(|n| {
                serde_json::from_value::<Media>(n.get("mediaRecommendation")?.clone()).ok()
            })
            .collect();
        let (has_next, last_page) = page_flags(&doc, "/Media/recommendations");
        Ok(Page {
            items: filter_media(list, policy),
            has_next,
            last_page,
        })
    }

    /// Related works as `(relation type, media)`, safe only.
    pub async fn relations(
        &self,
        id: i64,
        policy: ContentPolicy,
    ) -> Result<Vec<(String, Media)>, AniError> {
        let Some(doc) = self
            .client
            .get(
                Which::Details,
                format!("rel:{id}"),
                queries::RELATIONS,
                json!({ "id": id }),
            )
            .await?
        else {
            return Ok(Vec::new());
        };
        let edges: Vec<RelationEdge> = parse(&doc, "/Media/relations/edges")?;
        Ok(edges
            .into_iter()
            .filter(|e| safety::media_allowed(&e.node, policy.allow_adult))
            .map(|e| (e.relation_type.unwrap_or_default(), e.node))
            .collect())
    }

    pub async fn airing_for_media(
        &self,
        id: i64,
        now: i64,
        policy: ContentPolicy,
    ) -> Result<Option<AiringInfo>, AniError> {
        let Some(doc) = self
            .client
            .get(
                Which::Airing,
                format!("am:{id}"),
                queries::AIRING_FOR_MEDIA,
                json!({ "id": id }),
            )
            .await?
        else {
            return Ok(None);
        };
        let media: Media = parse(&doc, "/Media")?;
        if !safety::media_allowed(&media, policy.allow_adult) {
            return Ok(None);
        }
        let nodes: Vec<AiringNode> = parse(&doc, "/Media/airingSchedule/nodes").unwrap_or_default();
        Ok(Some(AiringInfo {
            previous_at: previous_airing(&nodes, now),
            media,
        }))
    }

    /// Episodes airing in `[from, to]`, ascending, safe media only (up to 4 pages of 50).
    pub async fn airing_between(
        &self,
        from: i64,
        to: i64,
        policy: ContentPolicy,
    ) -> Result<Vec<AiringEntry>, AniError> {
        let mut out = Vec::new();
        for page in 1..=4u32 {
            // Round to the cache window so identical requests share a key.
            let key = format!("ai:{}:{}:{page}", from / 300, to / 300);
            let vars = json!({"from": from, "to": to, "page": page, "perPage": 50});
            let Some(doc) = self
                .client
                .get(Which::Airing, key, queries::AIRING_SCHEDULE, vars)
                .await?
            else {
                break;
            };
            let rows: Vec<Value> = parse(&doc, "/Page/airingSchedules")?;
            for row in rows {
                let at = row
                    .get("airingAt")
                    .and_then(Value::as_i64)
                    .unwrap_or_default();
                let episode = row
                    .get("episode")
                    .and_then(Value::as_i64)
                    .unwrap_or_default() as i32;
                let media = row
                    .get("media")
                    .and_then(|m| serde_json::from_value::<Media>(m.clone()).ok())
                    .filter(|m| safety::media_allowed(m, policy.allow_adult));
                if let Some(media) = media {
                    out.push(AiringEntry {
                        airing_at: at,
                        episode,
                        media,
                    });
                }
            }
            if !page_flags(&doc, "/Page").0 {
                break;
            }
        }
        out.sort_by_key(|e| e.airing_at);
        Ok(out)
    }

    /// Random safe media; retries up to 5 times when a page yields nothing visible.
    pub async fn random_media(
        &self,
        kind: MediaKind,
        genre: Option<&str>,
        min_score: Option<u8>,
        policy: ContentPolicy,
    ) -> Result<Option<Media>, AniError> {
        self.random_media_with(kind, genre, min_score, &MediaFilter::default(), policy)
            .await
    }

    /// Same, narrowed by format / year / status (sent to AniList, so a narrow filter still finds something).
    pub async fn random_media_with(
        &self,
        kind: MediaKind,
        genre: Option<&str>,
        min_score: Option<u8>,
        filter: &MediaFilter,
        policy: ContentPolicy,
    ) -> Result<Option<Media>, AniError> {
        let mut max_page: usize = 50;
        for _ in 0..5 {
            let page = rand::random_range(1..=max_page.max(1));
            let mut vars = kind_vars(json!({"page": page}), kind);
            if let Some(f) = filter.format.as_deref() {
                vars["formatIn"] = json!([f]);
            }
            if let Some(c) = filter
                .country
                .as_deref()
                .filter(|_| kind.country().is_none())
            {
                vars["country"] = json!(c);
            }
            if let Some(y) = filter.year {
                vars["seasonYear"] = json!(y);
            }
            if let Some(st) = filter.status.as_deref() {
                vars["status"] = json!(st);
            }
            if let Some(g) = genre.filter(|g| !g.is_empty()) {
                vars["genre"] = json!(g);
            }
            if let Some(s) = min_score {
                vars["minScore"] = json!(s);
            }
            let vars = adult_vars(vars, policy);
            let key = format!(
                "rm:{}:{}:{}:{page}:{}:{}",
                kind.key(),
                genre.unwrap_or(""),
                min_score.unwrap_or(0),
                policy.allow_adult,
                filter.cache_key()
            );
            let Some(doc) = self
                .client
                .get(Which::Search, key, queries::RANDOM_MEDIA, vars)
                .await?
            else {
                continue;
            };
            let list: Vec<Media> = parse(&doc, "/Page/media")?;
            if let (_, Some(last)) = page_flags(&doc, "/Page") {
                max_page = last.clamp(1, 100);
            }
            if let Some(m) = pick(filter_media(list, policy)) {
                return Ok(Some(m));
            }
        }
        Ok(None)
    }

    pub async fn genres(&self) -> Result<Vec<String>, AniError> {
        let Some(doc) = self
            .client
            .get(Which::Daily, "genres".into(), queries::GENRES, json!({}))
            .await?
        else {
            return Ok(Vec::new());
        };
        let all: Vec<String> = parse(&doc, "/GenreCollection")?;
        Ok(all
            .into_iter()
            .filter(|g| !g.eq_ignore_ascii_case("Hentai"))
            .collect())
    }

    // ---- characters ------------------------------------------------------

    /// Safe characters, most favourited first.
    pub async fn search_characters(
        &self,
        query: &str,
        per_page: u8,
        policy: ContentPolicy,
    ) -> Result<Vec<Character>, AniError> {
        let key = format!("sc:{}:{per_page}", query.to_lowercase());
        let vars = json!({"search": query, "perPage": per_page.min(10)});
        let list: Vec<Character> = match self
            .client
            .get(Which::Search, key, queries::SEARCH_CHARACTER, vars)
            .await?
        {
            Some(doc) => parse(&doc, "/Page/characters")?,
            None => Vec::new(),
        };
        let mut out = self.visible(list, policy).await;
        out.sort_by_key(|c| std::cmp::Reverse(c.favourites.unwrap_or(0)));
        Ok(out)
    }

    pub async fn character_by_id(
        &self,
        id: i64,
        policy: ContentPolicy,
    ) -> Result<CharacterLookup, AniError> {
        let c: Option<Character> = match self
            .client
            .get(
                Which::Details,
                format!("c:{id}"),
                queries::CHARACTER_BY_ID,
                json!({ "id": id }),
            )
            .await?
        {
            Some(doc) => Some(parse(&doc, "/Character")?),
            None => None,
        };
        Ok(match c {
            None => CharacterLookup::NotFound,
            Some(c) if self.is_visible(&c, policy).await => CharacterLookup::Found(Box::new(c)),
            Some(_) => CharacterLookup::Hidden,
        })
    }

    /// Random safe character from the 1000 most favourited.
    pub async fn random_character(
        &self,
        policy: ContentPolicy,
    ) -> Result<Option<Character>, AniError> {
        let mut max_page: usize = 100;
        for _ in 0..5 {
            let page = rand::random_range(1..=max_page.max(1));
            let key = format!("rc:{page}");
            let Some(doc) = self
                .client
                .get(
                    Which::Search,
                    key,
                    queries::RANDOM_CHARACTER,
                    json!({ "page": page }),
                )
                .await?
            else {
                continue;
            };
            let list: Vec<Character> = parse(&doc, "/Page/characters")?;
            if let (_, Some(last)) = page_flags(&doc, "/Page") {
                max_page = last.clamp(1, 100);
            }
            if let Some(c) = pick(self.visible(list, policy).await) {
                return Ok(Some(c));
            }
        }
        Ok(None)
    }

    /// Safe characters whose birthday is today, most favourited first (one page of up to 10).
    pub async fn birthdays_today(&self, policy: ContentPolicy) -> Result<Vec<Character>, AniError> {
        Ok(self.birthdays_page(1, policy).await?.0)
    }

    /// One page (10) of today's birthdays and whether another page exists.
    pub async fn birthdays_page(
        &self,
        page: u32,
        policy: ContentPolicy,
    ) -> Result<(Vec<Character>, bool), AniError> {
        // The key carries the UTC day so the cache never serves yesterday's birthdays.
        let day = chrono::Utc::now().format("%Y-%m-%d");
        let key = format!("bd:{day}:{page}");
        let (list, more): (Vec<Character>, bool) = match self
            .client
            .get(
                Which::Search,
                key,
                queries::BIRTHDAYS,
                json!({"page": page}),
            )
            .await?
        {
            Some(doc) => (
                parse(&doc, "/Page/characters")?,
                page_flags(&doc, "/Page").0,
            ),
            None => (Vec::new(), false),
        };
        Ok((self.visible(list, policy).await, more))
    }

    /// The same character for everyone on a given day: `seed` (e.g. a hash of date and server) picks
    /// a page of the 200 most favourited characters and an entry on it.
    pub async fn daily_character(
        &self,
        seed: u64,
        policy: ContentPolicy,
    ) -> Result<Option<Character>, AniError> {
        let page = 1 + (seed % 20) as usize;
        let key = format!("rc:{page}");
        let Some(doc) = self
            .client
            .get(
                Which::Search,
                key,
                queries::RANDOM_CHARACTER,
                json!({ "page": page }),
            )
            .await?
        else {
            return Ok(None);
        };
        let list: Vec<Character> = parse(&doc, "/Page/characters")?;
        let mut visible = self.visible(list, policy).await;
        if visible.is_empty() {
            return Ok(None);
        }
        let i = ((seed / 20) as usize) % visible.len();
        Ok(Some(visible.swap_remove(i)))
    }

    /// Four distinct safe characters of similar popularity (one page): the answer plus three decoys.
    pub async fn random_character_set(
        &self,
        policy: ContentPolicy,
    ) -> Result<Option<(Character, Vec<Character>)>, AniError> {
        for _ in 0..5 {
            let page = rand::random_range(1..=100usize);
            let key = format!("rc:{page}");
            let Some(doc) = self
                .client
                .get(
                    Which::Search,
                    key,
                    queries::RANDOM_CHARACTER,
                    json!({ "page": page }),
                )
                .await?
            else {
                continue;
            };
            let list: Vec<Character> = parse(&doc, "/Page/characters")?;
            let mut visible = self.visible(list, policy).await;
            visible.retain(|c| c.image_url().is_some());
            if visible.len() >= 4 {
                let answer = visible.swap_remove(rand::random_range(0..visible.len()));
                let decoys = (0..3)
                    .map(|_| visible.swap_remove(rand::random_range(0..visible.len())))
                    .collect();
                return Ok(Some((answer, decoys)));
            }
        }
        Ok(None)
    }

    /// Four distinct safe works of one kind and similar popularity: the answer plus three decoys.
    pub async fn random_media_set(
        &self,
        kind: MediaKind,
        policy: ContentPolicy,
    ) -> Result<Option<(Media, Vec<Media>)>, AniError> {
        for _ in 0..5 {
            let page = rand::random_range(1..=50usize);
            let vars = adult_vars(kind_vars(json!({"page": page}), kind), policy);
            let key = format!("rm:{}::{page}:{}", kind.key(), policy.allow_adult);
            let Some(doc) = self
                .client
                .get(Which::Search, key, queries::RANDOM_MEDIA, vars)
                .await?
            else {
                continue;
            };
            let list: Vec<Media> = parse(&doc, "/Page/media")?;
            let mut safe = filter_media(list, policy);
            if safe.len() >= 4 {
                let answer = safe.swap_remove(rand::random_range(0..safe.len()));
                let decoys = (0..3)
                    .map(|_| safe.swap_remove(rand::random_range(0..safe.len())))
                    .collect();
                return Ok(Some((answer, decoys)));
            }
        }
        Ok(None)
    }

    async fn visible(&self, list: Vec<Character>, policy: ContentPolicy) -> Vec<Character> {
        let mut out = Vec::with_capacity(list.len());
        for c in list {
            if self.is_visible(&c, policy).await {
                out.push(c);
            }
        }
        out
    }

    /// Judge once per id and remember the verdict; adult-allowed callers bypass it.
    async fn is_visible(&self, c: &Character, policy: ContentPolicy) -> bool {
        if policy.allow_adult {
            return true;
        }
        if let Some(v) = self.verdicts.get(&c.id).await {
            return v;
        }
        let verdict = safety::character_allowed(c, false);
        self.verdicts.insert(c.id, verdict).await;
        verdict
    }

    // ---- users -----------------------------------------------------------

    pub async fn user_profile(&self, name: &str) -> Result<Option<UserProfile>, AniError> {
        match self
            .client
            .get(
                Which::Profile,
                format!("u:{}", name.to_lowercase()),
                queries::USER_PROFILE,
                json!({ "name": name }),
            )
            .await?
        {
            Some(doc) => Ok(Some(parse(&doc, "/User")?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests;
