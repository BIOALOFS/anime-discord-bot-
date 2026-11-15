//! Fallback catalogs for works AniList does not have (MangaDex, MangaUpdates).
//!
//! Disabled unless `ENABLE_FALLBACK_PROVIDERS=true`. Rules: only `safe`/`suggestive` content (anything that cannot
//! be verified is hidden), the source is credited on the card, and images are **never hotlinked**: the bot
//! downloads the cover itself (allow-listed hosts, size cap, cached) and attaches it.

mod mangadex;
mod mangaupdates;

use governor::{clock::DefaultClock, state::InMemoryState, state::NotKeyed, Quota, RateLimiter};
use moka::future::Cache;
use std::{future::Future, num::NonZeroU32, sync::Arc, time::Duration};

pub use mangadex::MangaDex;
pub use mangaupdates::MangaUpdates;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    MangaDex,
    MangaUpdates,
}

impl Source {
    /// Credit shown in the card footer.
    pub fn credit(&self) -> &'static str {
        match self {
            Self::MangaDex => "Source: MangaDex",
            Self::MangaUpdates => "Source: MangaUpdates",
        }
    }

    pub fn prefix(&self) -> &'static str {
        match self {
            Self::MangaDex => "mdex",
            Self::MangaUpdates => "mu",
        }
    }
}

/// A work from a fallback provider, normalized.
#[derive(Debug, Clone, PartialEq)]
pub struct Work {
    /// Provider-prefixed: `mdex:<uuid>` or `mu:<id>`.
    pub id: String,
    pub source: Source,
    pub title: String,
    pub year: Option<i32>,
    pub status: Option<String>,
    pub rating: Option<f32>,
    pub url: Option<String>,
    pub cover_url: Option<String>,
    pub genres: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("http: {0}")]
    Http(String),
    #[error("bad response: {0}")]
    Parse(String),
}

pub trait CatalogProvider: Send + Sync {
    fn source(&self) -> Source;
    fn search(&self, query: &str) -> impl Future<Output = Result<Vec<Work>, ProviderError>> + Send;
}

/// Hosts the bot will download covers from.
const COVER_HOSTS: &[&str] = &["uploads.mangadex.org", "cdn.mangaupdates.com"];
const MAX_COVER_BYTES: usize = 3 * 1024 * 1024;

pub fn cover_host_allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.contains('@') && COVER_HOSTS.contains(&host)
}

type Limiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

pub struct Providers {
    http: reqwest::Client,
    enabled: bool,
    mangadex: MangaDex,
    mangaupdates: MangaUpdates,
    dex_limit: Limiter,
    mu_limit: Limiter,
    searches: Cache<String, Arc<Vec<Work>>>,
    covers: Cache<String, Arc<Vec<u8>>>,
}

fn per_second(n: u32) -> Limiter {
    RateLimiter::direct(Quota::per_second(
        NonZeroU32::new(n.max(1)).unwrap_or(NonZeroU32::MIN),
    ))
}

impl Providers {
    pub fn new(http: reqwest::Client, enabled: bool) -> Self {
        Self {
            mangadex: MangaDex::new(http.clone()),
            mangaupdates: MangaUpdates::new(http.clone()),
            http,
            enabled,
            // MangaDex allows about 5 requests/second per IP; stay below it.
            dex_limit: per_second(3),
            mu_limit: per_second(2),
            searches: Cache::builder()
                .max_capacity(500)
                .time_to_live(Duration::from_secs(600))
                .build(),
            covers: Cache::builder()
                .weigher(|_: &String, v: &Arc<Vec<u8>>| u32::try_from(v.len()).unwrap_or(u32::MAX))
                .max_capacity(24 * 1024 * 1024)
                .time_to_live(Duration::from_secs(24 * 3600))
                .build(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Search every provider; failures of one never hide results of the other. Empty when disabled.
    pub async fn search(&self, query: &str) -> Vec<Work> {
        if !self.enabled || query.trim().chars().count() < 2 {
            return Vec::new();
        }
        let key = query.trim().to_lowercase();
        if let Some(hit) = self.searches.get(&key).await {
            return (*hit).clone();
        }
        self.dex_limit.until_ready().await;
        let dex = self.mangadex.search(query).await;
        self.mu_limit.until_ready().await;
        let mu = self.mangaupdates.search(query).await;
        let mut out = Vec::new();
        for (source, res) in [
            (self.mangadex.source(), dex),
            (self.mangaupdates.source(), mu),
        ] {
            match res {
                Ok(list) => out.extend(list.into_iter().filter(is_safe)),
                Err(e) => {
                    tracing::debug!(provider = source.prefix(), "fallback search failed: {e}")
                }
            }
        }
        out.truncate(10);
        self.searches.insert(key, Arc::new(out.clone())).await;
        out
    }

    /// Cover bytes fetched by the bot itself (never a hotlink).
    pub async fn cover(&self, work: &Work) -> Option<Arc<Vec<u8>>> {
        let url = work.cover_url.as_deref()?;
        if let Some(hit) = self.covers.get(url).await {
            return Some(hit);
        }
        if !cover_host_allowed(url) {
            return None;
        }
        let resp = self
            .http
            .get(url)
            .timeout(Duration::from_secs(8))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success()
            || resp
                .content_length()
                .is_some_and(|n| n > MAX_COVER_BYTES as u64)
        {
            return None;
        }
        let bytes = resp.bytes().await.ok()?;
        if bytes.len() > MAX_COVER_BYTES || !looks_like_image(&bytes) {
            return None;
        }
        let bytes = Arc::new(bytes.to_vec());
        self.covers.insert(url.to_string(), bytes.clone()).await;
        Some(bytes)
    }
}

fn looks_like_image(b: &[u8]) -> bool {
    b.starts_with(&[0x89, b'P', b'N', b'G'])
        || b.starts_with(&[0xFF, 0xD8, 0xFF])
        || b.starts_with(b"GIF8")
        || (b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP")
}

/// The last line of defence: providers already filter, this re-checks what they return.
pub fn is_safe(w: &Work) -> bool {
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

#[cfg(test)]
mod tests;
