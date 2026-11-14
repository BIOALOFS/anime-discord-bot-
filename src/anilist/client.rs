use super::{models::*, queries, safety};
use governor::{clock::DefaultClock, state::InMemoryState, state::NotKeyed, Quota, RateLimiter};
use moka::future::Cache;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{num::NonZeroU32, sync::Arc, time::Duration};

use crate::config::Config;

const ENDPOINT: &str = "https://graphql.anilist.co";
const MAX_RETRIES: u32 = 2;
const MAX_RETRY_AFTER_SECS: u64 = 5;

#[derive(Debug, Clone, thiserror::Error)]
pub enum AniError {
    /// Rate limited by AniList (429) or by us for too long.
    #[error("anilist is busy")]
    Busy,
    #[error("not found")]
    NotFound,
    #[error("http error: {0}")]
    Http(String),
    #[error("api error: {0}")]
    Api(String),
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

/// Result of looking up a character by id; `Hidden` lets callers show the
/// "inappropriate items hidden" notice instead of pretending it does not exist.
#[derive(Debug, Clone)]
pub enum CharacterLookup {
    Found(Character),
    Hidden,
    NotFound,
}

type Limiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;
type JsonCache = Cache<String, Arc<Value>>;

fn cache(ttl_secs: u64, cap: u64) -> JsonCache {
    Cache::builder()
        .max_capacity(cap)
        .time_to_live(Duration::from_secs(ttl_secs))
        .build()
}

pub struct AniListClient {
    http: reqwest::Client,
    limiter: Limiter,
    search: JsonCache,
    details: JsonCache,
    profile: JsonCache,
    airing: JsonCache,
    /// Keys that returned "not found" recently.
    negative: Cache<String, ()>,
    /// Safety verdict per character id (true = safe to show).
    verdicts: Cache<i64, bool>,
}

impl AniListClient {
    pub fn new(cfg: &Config) -> anyhow::Result<Self> {
        let per_min =
            NonZeroU32::new(cfg.anilist_rate_limit_per_min.max(1)).unwrap_or(NonZeroU32::MIN);
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent(concat!("anpot-discord-bot/", env!("CARGO_PKG_VERSION")))
                .build()?,
            limiter: RateLimiter::direct(Quota::per_minute(per_min)),
            search: cache(cfg.cache_ttl_search_secs, 5_000),
            details: cache(cfg.cache_ttl_details_secs, 5_000),
            profile: cache(cfg.cache_ttl_profile_secs, 1_000),
            airing: cache(cfg.cache_ttl_airing_secs, 200),
            negative: Cache::builder()
                .max_capacity(5_000)
                .time_to_live(Duration::from_secs(cfg.cache_ttl_negative_secs))
                .build(),
            verdicts: Cache::builder()
                .max_capacity(50_000)
                .time_to_live(Duration::from_secs(cfg.cache_ttl_details_secs))
                .build(),
        })
    }

    // ---- transport -------------------------------------------------------

    async fn post(&self, query: &str, vars: &Value) -> Result<Value, AniError> {
        let body = json!({ "query": query, "variables": vars });
        for attempt in 0..=MAX_RETRIES {
            self.limiter.until_ready().await;
            let resp = match self.http.post(ENDPOINT).json(&body).send().await {
                Ok(r) => r,
                Err(e) if attempt < MAX_RETRIES && (e.is_timeout() || e.is_connect()) => {
                    backoff(attempt).await;
                    continue;
                }
                Err(e) => return Err(AniError::Http(e.to_string())),
            };
            let status = resp.status();
            if status.as_u16() == 429 {
                let wait = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(2);
                if wait <= MAX_RETRY_AFTER_SECS && attempt < MAX_RETRIES {
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                    continue;
                }
                return Err(AniError::Busy);
            }
            if status.is_server_error() && attempt < MAX_RETRIES {
                backoff(attempt).await;
                continue;
            }
            // AniList answers "not found" with HTTP 404 and a JSON error body.
            let mut doc: Value = resp
                .json()
                .await
                .map_err(|e| AniError::Http(e.to_string()))?;
            if let Some(err) = doc.get("errors").and_then(|e| e.get(0)) {
                if status.as_u16() == 404 || err.get("status").and_then(Value::as_u64) == Some(404)
                {
                    return Err(AniError::NotFound);
                }
                let msg = err
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                return Err(AniError::Api(msg.to_string()));
            }
            if !status.is_success() {
                return Err(AniError::Http(status.to_string()));
            }
            return Ok(doc["data"].take());
        }
        Err(AniError::Busy)
    }

    /// Cached + coalesced request: concurrent identical keys share one network call
    /// (moka runs the init future once per key). `Ok(None)` means not found.
    async fn fetch(
        &self,
        cache: &JsonCache,
        key: String,
        query: &'static str,
        vars: Value,
    ) -> Result<Option<Arc<Value>>, AniError> {
        if self.negative.contains_key(&key) {
            return Ok(None);
        }
        let result = cache
            .try_get_with(key.clone(), async {
                self.post(query, &vars).await.map(Arc::new)
            })
            .await;
        match result {
            Ok(v) => Ok(Some(v)),
            Err(e) => match &*e {
                AniError::NotFound => {
                    self.negative.insert(key, ()).await;
                    Ok(None)
                }
                other => Err(other.clone()),
            },
        }
    }

    // ---- media -----------------------------------------------------------

    pub async fn search_media(
        &self,
        query: &str,
        kind: MediaKind,
        per_page: u8,
        allow_adult: bool,
    ) -> Result<Vec<Media>, AniError> {
        let vars = with_adult_vars(
            json!({"search": query, "type": kind.as_str(), "perPage": per_page}),
            allow_adult,
        );
        let key = format!(
            "sm:{}:{}:{per_page}:{allow_adult}:{}",
            kind.as_str(),
            query.to_lowercase(),
            ""
        );
        let list: Vec<Media> = self
            .fetch(&self.search, key, queries::SEARCH_MEDIA, vars)
            .await?
            .map(|v| parse(&v, "/Page/media"))
            .transpose()?
            .unwrap_or_default();
        Ok(list
            .into_iter()
            .filter(|m| safety::media_allowed(m, allow_adult))
            .collect())
    }

    /// `None` when missing or hidden by the safety filter.
    pub async fn media_by_id(&self, id: i64, allow_adult: bool) -> Result<Option<Media>, AniError> {
        let media: Option<Media> = self
            .fetch(
                &self.details,
                format!("m:{id}"),
                queries::MEDIA_BY_ID,
                json!({ "id": id }),
            )
            .await?
            .map(|v| parse(&v, "/Media"))
            .transpose()?;
        Ok(media.filter(|m| safety::media_allowed(m, allow_adult)))
    }

    pub async fn seasonal(
        &self,
        season: &str,
        year: i32,
        page: u32,
        per_page: u8,
        allow_adult: bool,
    ) -> Result<Vec<Media>, AniError> {
        let vars = with_adult_vars(
            json!({"season": season, "year": year, "page": page, "perPage": per_page}),
            allow_adult,
        );
        let key = format!("se:{season}:{year}:{page}:{per_page}:{allow_adult}");
        let list: Vec<Media> = self
            .fetch(&self.search, key, queries::SEASONAL, vars)
            .await?
            .map(|v| parse(&v, "/Page/media"))
            .transpose()?
            .unwrap_or_default();
        Ok(list
            .into_iter()
            .filter(|m| safety::media_allowed(m, allow_adult))
            .collect())
    }

    /// Episodes airing between two unix timestamps, safe media only.
    pub async fn airing_between(
        &self,
        from: i64,
        to: i64,
        allow_adult: bool,
    ) -> Result<Vec<(i64, i32, Media)>, AniError> {
        // Round to the cache TTL window so identical requests share a key.
        let key = format!("ai:{}:{}:{allow_adult}", from / 300, to / 300);
        let vars = json!({"from": from, "to": to, "perPage": 50});
        let Some(doc) = self
            .fetch(&self.airing, key, queries::AIRING_SCHEDULE, vars)
            .await?
        else {
            return Ok(Vec::new());
        };
        let rows: Vec<Value> = parse(&doc, "/Page/airingSchedules")?;
        let mut out = Vec::new();
        for row in rows {
            let at = row
                .get("airingAt")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let ep = row
                .get("episode")
                .and_then(Value::as_i64)
                .unwrap_or_default() as i32;
            if let Some(m) = row
                .get("media")
                .and_then(|m| serde_json::from_value::<Media>(m.clone()).ok())
                .filter(|m| safety::media_allowed(m, allow_adult))
            {
                out.push((at, ep, m));
            }
        }
        Ok(out)
    }

    // ---- characters ------------------------------------------------------

    pub async fn search_characters(
        &self,
        query: &str,
        per_page: u8,
        allow_adult: bool,
    ) -> Result<Vec<Character>, AniError> {
        let key = format!("sc:{}:{per_page}", query.to_lowercase());
        let vars = json!({"search": query, "perPage": per_page});
        let list: Vec<Character> = self
            .fetch(&self.search, key, queries::SEARCH_CHARACTER, vars)
            .await?
            .map(|v| parse(&v, "/Page/characters"))
            .transpose()?
            .unwrap_or_default();
        let mut out = Vec::with_capacity(list.len());
        for c in list {
            if self.character_visible(&c, allow_adult).await {
                out.push(c);
            }
        }
        Ok(out)
    }

    pub async fn character_by_id(
        &self,
        id: i64,
        allow_adult: bool,
    ) -> Result<CharacterLookup, AniError> {
        let c: Option<Character> = self
            .fetch(
                &self.details,
                format!("c:{id}"),
                queries::CHARACTER_BY_ID,
                json!({ "id": id }),
            )
            .await?
            .map(|v| parse(&v, "/Character"))
            .transpose()?;
        Ok(match c {
            None => CharacterLookup::NotFound,
            Some(c) if self.character_visible(&c, allow_adult).await => CharacterLookup::Found(c),
            Some(_) => CharacterLookup::Hidden,
        })
    }

    /// Judge once per id and remember the verdict; adult-allowed callers bypass it.
    async fn character_visible(&self, c: &Character, allow_adult: bool) -> bool {
        if allow_adult {
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
        self.fetch(
            &self.profile,
            format!("u:{}", name.to_lowercase()),
            queries::USER_PROFILE,
            json!({ "name": name }),
        )
        .await?
        .map(|v| parse(&v, "/User"))
        .transpose()
    }
}

/// Media filters: safe by default; adult content only when explicitly allowed.
fn with_adult_vars(mut vars: Value, allow_adult: bool) -> Value {
    if !allow_adult {
        vars["isAdult"] = json!(false);
        vars["genreNotIn"] = json!(["Hentai"]);
    }
    vars
}

fn parse<T: DeserializeOwned>(doc: &Value, pointer: &str) -> Result<T, AniError> {
    let part = doc
        .pointer(pointer)
        .ok_or_else(|| AniError::Api(format!("missing {pointer}")))?;
    serde_json::from_value(part.clone()).map_err(|e| AniError::Api(e.to_string()))
}

async fn backoff(attempt: u32) {
    // Cheap jitter without an extra dependency.
    let jitter = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::from(d.subsec_millis()) % 250)
        .unwrap_or(0);
    tokio::time::sleep(Duration::from_millis(
        300 * (u64::from(attempt) + 1) + jitter,
    ))
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_by_default() {
        let v = with_adult_vars(json!({"search": "x"}), false);
        assert_eq!(v["isAdult"], json!(false));
        assert_eq!(v["genreNotIn"], json!(["Hentai"]));
        let v = with_adult_vars(json!({"search": "x"}), true);
        assert!(v.get("isAdult").is_none());
    }

    #[test]
    fn queries_accept_adult_vars() {
        for q in [queries::SEARCH_MEDIA, queries::SEASONAL] {
            assert!(q.contains("$isAdult") && q.contains("$genreNotIn"));
        }
        assert!(queries::SEARCH_CHARACTER.contains("isAdult genres"));
        assert!(queries::CHARACTER_BY_ID.contains("isAdult genres"));
    }

    #[test]
    fn parse_reports_missing_path() {
        let doc = json!({"Page": {}});
        assert!(matches!(
            parse::<Vec<Media>>(&doc, "/Page/media"),
            Err(AniError::Api(_))
        ));
    }
}
