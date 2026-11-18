//! The TMDB layer (movies, TV, dramas, people): a rate-limited, cached client plus the pure model in `model` and the
//! safety rules in `safety`. Disabled gracefully (every call returns `None`) when no key is configured.
//!
//! This product uses the TMDB API but is not endorsed or certified by TMDB. Watch-provider data is by JustWatch.

pub mod card;
pub mod find;
pub mod lookup;
pub mod model;
pub mod safety;

pub use model::{Kind, Work};

use governor::{clock::DefaultClock, state::InMemoryState, state::NotKeyed, Quota, RateLimiter};
use moka::future::Cache;
use serde_json::Value;
use std::{num::NonZeroU32, sync::Arc, time::Duration};

const API: &str = "https://api.themoviedb.org/3";

/// Required attribution lines.
pub const ATTRIBUTION_AR: &str = "هذا المنتج يستخدم TMDB API لكنه غير معتمد أو مصدّق من TMDB.";
pub const ATTRIBUTION_EN: &str =
    "This product uses the TMDB API but is not endorsed or certified by TMDB.";
pub const JUSTWATCH_CREDIT: &str = "Streaming data by JustWatch";
pub const FOOTER_CREDIT: &str = "المصدر: TMDB";

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub api_key: Option<String>,
    pub read_token: Option<String>,
    pub region: String,
    pub rate_per_10s: u32,
}

impl Config {
    pub fn enabled(&self) -> bool {
        self.api_key.is_some() || self.read_token.is_some()
    }
}

/// How long a response is kept.
#[derive(Debug, Clone, Copy)]
pub enum Ttl {
    Search,
    Details,
    Lists,
}

impl Ttl {
    pub fn secs(self) -> u64 {
        match self {
            Self::Search => 600,
            Self::Details => 21_600,
            Self::Lists => 3_600,
        }
    }
}

/// Seconds to wait after a 429: `Retry-After` when sane, else 2 seconds, never more than 10.
pub fn retry_wait(header: Option<&str>) -> u64 {
    header
        .and_then(|h| h.trim().parse::<u64>().ok())
        .unwrap_or(2)
        .clamp(1, 10)
}

pub struct Tmdb {
    http: reqwest::Client,
    cfg: Config,
    limit: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
    cache: Cache<String, Arc<Value>>,
    /// Misses are remembered briefly too (5 minutes) so a missing id is not asked again and again.
    negative: Cache<String, ()>,
}

impl Tmdb {
    pub fn new(http: reqwest::Client, cfg: Config) -> Self {
        let n = cfg.rate_per_10s.clamp(1, 40);
        let quota = Quota::with_period(Duration::from_secs(10) / n)
            .map(|q| q.allow_burst(NonZeroU32::new(n).unwrap_or(NonZeroU32::MIN)))
            .unwrap_or_else(|| Quota::per_second(NonZeroU32::MIN));
        Self {
            http,
            cfg,
            limit: RateLimiter::direct(quota),
            cache: Cache::builder()
                .max_capacity(3_000)
                .time_to_live(Duration::from_secs(6 * 3600))
                .build(),
            negative: Cache::builder()
                .max_capacity(3_000)
                .time_to_live(Duration::from_secs(300))
                .build(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.cfg.enabled()
    }

    pub fn region(&self) -> &str {
        &self.cfg.region
    }

    /// One GET with `include_adult=false` always on, the key as a query parameter or a bearer token, one retry after a
    /// 429. Cached by path and query; `ttl` is checked against the entry's age through separate keys.
    async fn get(&self, path: &str, query: &[(&str, String)], ttl: Ttl) -> Option<Arc<Value>> {
        if !self.enabled() {
            return None;
        }
        let mut q: Vec<(&str, String)> = query.to_vec();
        q.push(("include_adult", "false".into()));
        q.push(("language", "en-US".into()));
        let key = format!(
            "{path}?{}#{}",
            q.iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("&"),
            ttl.secs()
        );
        if let Some(hit) = self.cache.get(&key).await {
            crate::ops::stats::bump("cache", "hit");
            return Some(hit);
        }
        crate::ops::stats::bump("cache", "miss");
        if self.negative.get(&key).await.is_some() {
            return None;
        }
        for attempt in 0..2 {
            self.limit.until_ready().await;
            crate::ops::stats::bump("api", "tmdb");
            let mut req = self
                .http
                .get(format!("{API}{path}"))
                .query(&q)
                .timeout(Duration::from_secs(8));
            match (&self.cfg.read_token, &self.cfg.api_key) {
                (Some(t), _) => req = req.bearer_auth(t),
                (None, Some(k)) => req = req.query(&[("api_key", k.as_str())]),
                _ => return None,
            }
            let Ok(resp) = req.send().await else {
                return None;
            };
            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt == 0 {
                let wait = retry_wait(
                    resp.headers()
                        .get("retry-after")
                        .and_then(|h| h.to_str().ok()),
                );
                tokio::time::sleep(Duration::from_secs(wait)).await;
                continue;
            }
            if !resp.status().is_success() {
                // The path is logged without its query (the key travels in the query or a header).
                crate::http_log::failed("tmdb", path, resp).await;
                self.negative.insert(key, ()).await;
                return None;
            }
            let doc: Value = resp.json().await.ok()?;
            crate::ops::health::ok("tmdb");
            let doc = Arc::new(doc);
            // Entries live for the cache's maximum; short TTL classes expire by being re-keyed on a time bucket.
            self.cache.insert(key, doc.clone()).await;
            return Some(doc);
        }
        None
    }

    fn bucket(ttl: Ttl) -> String {
        // A new bucket every `ttl` seconds makes older cached entries unreachable (and they age out of the cache).
        (chrono::Utc::now().timestamp() as u64 / ttl.secs()).to_string()
    }

    pub async fn search(&self, kind: Kind, query: &str, page: u32) -> Vec<Work> {
        let path = if kind == Kind::Tv {
            "/search/tv"
        } else {
            "/search/movie"
        };
        let q = [
            ("query", query.to_string()),
            ("page", page.to_string()),
            ("b", Self::bucket(Ttl::Search)),
        ];
        self.get(path, &q, Ttl::Search)
            .await
            .map(|d| model::parse_list(&d, kind))
            .unwrap_or_default()
    }

    pub async fn search_people(&self, query: &str) -> Vec<model::Cast> {
        let q = [
            ("query", query.to_string()),
            ("b", Self::bucket(Ttl::Search)),
        ];
        self.get("/search/person", &q, Ttl::Search)
            .await
            .map(|d| model::parse_people(&d))
            .unwrap_or_default()
    }

    /// Full details with credits, ratings, keywords, providers (one request). `None` when missing, blocked or off.
    pub async fn details(&self, kind: Kind, id: i64) -> Option<Work> {
        let (path, append) = match kind {
            Kind::Tv => (
                format!("/tv/{id}"),
                "aggregate_credits,content_ratings,keywords,watch/providers,external_ids",
            ),
            Kind::Movie => (
                format!("/movie/{id}"),
                "credits,release_dates,keywords,watch/providers",
            ),
        };
        let q = [
            ("append_to_response", append.to_string()),
            ("b", Self::bucket(Ttl::Details)),
        ];
        let doc = self.get(&path, &q, Ttl::Details).await?;
        let work = match kind {
            Kind::Tv => model::parse_tv(&doc, &self.cfg.region),
            Kind::Movie => model::parse_movie(&doc, &self.cfg.region),
        };
        (!work.blocked() && work.id > 0).then_some(work)
    }

    /// `discover/tv` or `discover/movie`, sorted by popularity. `filters` are discover parameters.
    pub async fn discover(&self, kind: Kind, filters: &[(&str, String)], page: u32) -> Vec<Work> {
        let path = if kind == Kind::Tv {
            "/discover/tv"
        } else {
            "/discover/movie"
        };
        let mut q: Vec<(&str, String)> = filters.to_vec();
        q.push(("sort_by", "popularity.desc".into()));
        q.push(("page", page.to_string()));
        q.push(("vote_count.gte", "20".into()));
        q.push(("b", Self::bucket(Ttl::Lists)));
        self.get(path, &q, Ttl::Lists)
            .await
            .map(|d| model::parse_list(&d, kind))
            .unwrap_or_default()
    }

    /// A real (uncached) lightweight request for the status panel: is TMDB answering with our credentials?
    pub async fn ping(&self) -> bool {
        if !self.enabled() {
            return false;
        }
        let mut req = self
            .http
            .get(format!("{API}/configuration"))
            .timeout(Duration::from_secs(8));
        match (&self.cfg.read_token, &self.cfg.api_key) {
            (Some(t), _) => req = req.bearer_auth(t),
            (None, Some(k)) => req = req.query(&[("api_key", k.as_str())]),
            _ => return false,
        }
        req.send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// What TMDB recommends after a show or film (the "because you liked it" list), most relevant first.
    pub async fn recommendations(&self, kind: Kind, id: i64) -> Vec<Work> {
        let path = if kind == Kind::Tv {
            format!("/tv/{id}/recommendations")
        } else {
            format!("/movie/{id}/recommendations")
        };
        let q = [("page", "1".to_string()), ("b", Self::bucket(Ttl::Lists))];
        self.get(&path, &q, Ttl::Lists)
            .await
            .map(|d| model::parse_list(&d, kind))
            .unwrap_or_default()
    }

    /// Shows with an episode airing on `date` (`YYYY-MM-DD`, in the time zone `tz`), most popular first. New shows with
    /// few votes are included (unlike `discover`).
    pub async fn airing_on(&self, date: &str, tz: &str, page: u32) -> Vec<Work> {
        let q = [
            ("air_date.gte", date.to_string()),
            ("air_date.lte", date.to_string()),
            ("timezone", tz.to_string()),
            ("sort_by", "popularity.desc".to_string()),
            ("page", page.to_string()),
            ("b", Self::bucket(Ttl::Lists)),
        ];
        self.get("/discover/tv", &q, Ttl::Lists)
            .await
            .map(|d| model::parse_list(&d, Kind::Tv))
            .unwrap_or_default()
    }

    /// `tv/on_the_air`, `tv/airing_today`, `movie/upcoming`, `movie/now_playing`.
    pub async fn list(&self, path: &str, kind: Kind, page: u32) -> Vec<Work> {
        let q = [
            ("page", page.to_string()),
            ("region", self.cfg.region.clone()),
            ("b", Self::bucket(Ttl::Lists)),
        ];
        self.get(path, &q, Ttl::Lists)
            .await
            .map(|d| model::parse_list(&d, kind))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
