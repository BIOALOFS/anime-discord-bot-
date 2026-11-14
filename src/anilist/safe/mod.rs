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

#[derive(Debug, Clone, Copy)]
pub enum MediaKind {
    Anime,
    Manga,
}

impl MediaKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Anime => "ANIME",
            Self::Manga => "MANGA",
        }
    }
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
pub struct SeasonalQuery {
    pub season: &'static str,
    pub year: i32,
    pub sort: &'static str,
    /// Empty means all formats.
    pub formats: &'static [&'static str],
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
            json!({"search": query, "type": kind.as_str(), "perPage": per_page.min(10)}),
            policy,
        );
        let key = format!(
            "sm:{}:{}:{per_page}:{}",
            kind.as_str(),
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

    pub async fn seasonal(
        &self,
        q: &SeasonalQuery,
        page: u32,
        policy: ContentPolicy,
    ) -> Result<Page<Media>, AniError> {
        let mut vars = json!({
            "season": q.season, "year": q.year, "page": page, "perPage": 10, "sort": [q.sort],
        });
        if !q.formats.is_empty() {
            vars["formatIn"] = json!(q.formats);
        }
        let vars = adult_vars(vars, policy);
        let key = format!(
            "se:{}:{}:{page}:{}:{}:{}",
            q.season,
            q.year,
            q.sort,
            q.formats.join(","),
            policy.allow_adult
        );
        let Some(doc) = self
            .client
            .get(Which::Search, key, queries::SEASONAL, vars)
            .await?
        else {
            return Ok(Page {
                items: Vec::new(),
                has_next: false,
                last_page: None,
            });
        };
        let list: Vec<Media> = parse(&doc, "/Page/media")?;
        let (has_next, last_page) = page_flags(&doc, "/Page");
        Ok(Page {
            items: filter_media(list, policy),
            has_next,
            last_page,
        })
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
        let mut max_page: usize = 50;
        for _ in 0..5 {
            let page = rand::random_range(1..=max_page.max(1));
            let mut vars = json!({"page": page, "type": kind.as_str()});
            if let Some(g) = genre.filter(|g| !g.is_empty()) {
                vars["genre"] = json!(g);
            }
            if let Some(s) = min_score {
                vars["minScore"] = json!(s);
            }
            let vars = adult_vars(vars, policy);
            let key = format!(
                "rm:{}:{}:{}:{page}:{}",
                kind.as_str(),
                genre.unwrap_or(""),
                min_score.unwrap_or(0),
                policy.allow_adult
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
