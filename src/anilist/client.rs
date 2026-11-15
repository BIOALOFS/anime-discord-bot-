//! Raw transport: rate limiting, retries, caching, request coalescing.
//! Private to the `anilist` module on purpose; everything else goes through `SafeAniList`.

use governor::{clock::DefaultClock, state::InMemoryState, state::NotKeyed, Quota, RateLimiter};
use moka::future::Cache;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{num::NonZeroU32, sync::Arc, time::Duration};

use crate::config::Config;

const ENDPOINT: &str = "https://graphql.anilist.co";
const MAX_RETRIES: u32 = 2;
const MAX_RETRY_AFTER_SECS: u64 = 5;
const MAX_GATE_WAIT_SECS: u64 = 20;

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

/// Which cache (and therefore TTL) a request belongs to.
#[derive(Debug, Clone, Copy)]
pub(super) enum Which {
    Search,
    Details,
    Profile,
    Airing,
    Daily,
}

type Limiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;
type JsonCache = Cache<String, Arc<Value>>;

#[cfg(test)]
pub(super) type Mock = Box<dyn Fn(&str, &Value) -> Result<Value, AniError> + Send + Sync>;

fn cache(ttl_secs: u64, cap: u64) -> JsonCache {
    Cache::builder()
        .max_capacity(cap)
        .time_to_live(Duration::from_secs(ttl_secs))
        .build()
}

pub(super) struct AniListClient {
    http: reqwest::Client,
    /// Set from `Retry-After` / `X-RateLimit-*`: no request is sent before this instant.
    paused_until: std::sync::Mutex<Option<tokio::time::Instant>>,
    limiter: Limiter,
    search: JsonCache,
    details: JsonCache,
    profile: JsonCache,
    airing: JsonCache,
    daily: JsonCache,
    /// Keys that returned "not found" recently.
    negative: Cache<String, ()>,
    #[cfg(test)]
    mock: Option<Mock>,
}

impl AniListClient {
    pub(super) fn new(cfg: &Config) -> anyhow::Result<Self> {
        let per_min =
            NonZeroU32::new(cfg.anilist_rate_limit_per_min.max(1)).unwrap_or(NonZeroU32::MIN);
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent(concat!("anpot-discord-bot/", env!("CARGO_PKG_VERSION")))
                .build()?,
            paused_until: std::sync::Mutex::new(None),
            limiter: RateLimiter::direct(Quota::per_minute(per_min)),
            search: cache(cfg.cache_ttl_search_secs, 5_000),
            details: cache(cfg.cache_ttl_details_secs, 5_000),
            profile: cache(cfg.cache_ttl_profile_secs, 1_000),
            airing: cache(cfg.cache_ttl_airing_secs, 500),
            daily: cache(86_400, 50),
            negative: Cache::builder()
                .max_capacity(5_000)
                .time_to_live(Duration::from_secs(cfg.cache_ttl_negative_secs))
                .build(),
            #[cfg(test)]
            mock: None,
        })
    }

    #[cfg(test)]
    pub(super) fn with_mock(cfg: &Config, mock: Mock) -> anyhow::Result<Self> {
        let mut c = Self::new(cfg)?;
        c.mock = Some(mock);
        Ok(c)
    }

    fn pause(&self, d: Duration) {
        if let Ok(mut g) = self.paused_until.lock() {
            let until = tokio::time::Instant::now() + d;
            if g.is_none_or(|cur| until > cur) {
                *g = Some(until);
            }
        }
    }

    /// Wait out a rate-limit pause; give up (Busy) if it is too long to be worth holding a command for.
    async fn wait_gate(&self) -> Result<(), AniError> {
        let until = self.paused_until.lock().ok().and_then(|g| *g);
        if let Some(until) = until {
            let wait = until.saturating_duration_since(tokio::time::Instant::now());
            if wait > Duration::from_secs(MAX_GATE_WAIT_SECS) {
                return Err(AniError::Busy);
            }
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
        }
        Ok(())
    }

    async fn post(&self, query: &str, vars: &Value) -> Result<Value, AniError> {
        #[cfg(test)]
        if let Some(mock) = &self.mock {
            return mock(query, vars);
        }
        let body = json!({ "query": query, "variables": vars });
        for attempt in 0..=MAX_RETRIES {
            self.wait_gate().await?;
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
            let retry_after: Option<u64> = header_num(&resp, "retry-after");
            let now_unix = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64);
            let pause = pause_for(
                header_num(&resp, "x-ratelimit-remaining"),
                retry_after.filter(|_| status.as_u16() == 429 || status.is_server_error()),
                header_num(&resp, "x-ratelimit-reset"),
                now_unix,
            );
            if let Some(p) = pause {
                self.pause(p);
            }
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
    pub(super) async fn get(
        &self,
        which: Which,
        key: String,
        query: &'static str,
        vars: Value,
    ) -> Result<Option<Arc<Value>>, AniError> {
        let cache = match which {
            Which::Search => &self.search,
            Which::Details => &self.details,
            Which::Profile => &self.profile,
            Which::Airing => &self.airing,
            Which::Daily => &self.daily,
        };
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
}

/// How long to stop sending requests, from AniList's rate-limit headers.
/// `Retry-After` wins; otherwise pause until the window resets when the budget is (nearly) spent.
pub(super) fn pause_for(
    remaining: Option<u32>,
    retry_after_secs: Option<u64>,
    reset_unix: Option<i64>,
    now_unix: i64,
) -> Option<Duration> {
    if let Some(s) = retry_after_secs {
        return Some(Duration::from_secs(s.max(1)));
    }
    match (remaining, reset_unix) {
        (Some(r), Some(reset)) if r <= 1 => {
            Some(Duration::from_secs((reset - now_unix).clamp(1, 60) as u64))
        }
        _ => None,
    }
}

fn header_num<T: std::str::FromStr>(resp: &reqwest::Response, name: &str) -> Option<T> {
    resp.headers().get(name)?.to_str().ok()?.trim().parse().ok()
}

pub(super) fn parse<T: DeserializeOwned>(doc: &Value, pointer: &str) -> Result<T, AniError> {
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
    fn retry_after_wins() {
        assert_eq!(
            pause_for(Some(50), Some(7), Some(1_000), 900),
            Some(Duration::from_secs(7))
        );
        assert_eq!(
            pause_for(None, Some(0), None, 0),
            Some(Duration::from_secs(1)),
            "never a zero pause"
        );
    }

    #[test]
    fn pauses_until_reset_when_the_budget_is_spent() {
        assert_eq!(
            pause_for(Some(0), None, Some(1_030), 1_000),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            pause_for(Some(1), None, Some(1_005), 1_000),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            pause_for(Some(0), None, Some(5_000), 1_000),
            Some(Duration::from_secs(60)),
            "capped at a minute"
        );
        assert_eq!(
            pause_for(Some(0), None, Some(900), 1_000),
            Some(Duration::from_secs(1)),
            "reset in the past"
        );
    }

    #[test]
    fn no_pause_while_there_is_budget_or_no_headers() {
        assert_eq!(pause_for(Some(20), None, Some(1_030), 1_000), None);
        assert_eq!(pause_for(None, None, None, 0), None);
        assert_eq!(
            pause_for(Some(0), None, None, 0),
            None,
            "no reset time to wait for"
        );
    }
}
