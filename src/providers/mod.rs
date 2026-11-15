//! Catalogs beyond AniList: MangaDex/MangaUpdates (fallback for written works), IGDB (games and game
//! characters) and VNDB (visual novels and their characters).
//!
//! Everything is opt-in: the fallback needs `ENABLE_FALLBACK_PROVIDERS=true`, IGDB needs
//! `IGDB_CLIENT_ID` + `IGDB_CLIENT_SECRET`, VNDB needs `ENABLE_VNDB=true`. Rules for all of them: sexual content
//! is hidden, an item without a reliable verdict is hidden, the source is credited, and images are **never
//! hotlinked**: the bot downloads them itself (allow-listed hosts, size cap, cache) and attaches them.

mod igdb;
mod mangadex;
mod mangaupdates;
pub mod safety;
mod vndb;

use governor::{clock::DefaultClock, state::InMemoryState, state::NotKeyed, Quota, RateLimiter};
use moka::future::Cache;
use std::{future::Future, num::NonZeroU32, sync::Arc, time::Duration};

pub use igdb::Igdb;
pub use mangadex::MangaDex;
pub use mangaupdates::MangaUpdates;
pub use vndb::Vndb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    MangaDex,
    MangaUpdates,
    Igdb,
    Vndb,
}

impl Source {
    /// Credit shown in the card footer.
    pub fn credit(&self) -> &'static str {
        match self {
            Self::MangaDex => "Source: MangaDex",
            Self::MangaUpdates => "Source: MangaUpdates",
            Self::Igdb => "Data from IGDB.com",
            Self::Vndb => "Data from VNDB",
        }
    }

    pub fn prefix(&self) -> &'static str {
        match self {
            Self::MangaDex => "mdex",
            Self::MangaUpdates => "mu",
            Self::Igdb => "igdb",
            Self::Vndb => "vndb",
        }
    }

    /// Base added to a provider character id to keep ids from different providers apart in the integer
    /// character tables (AniList ids stay far below it).
    fn character_base(&self) -> Option<i64> {
        match self {
            Self::Vndb => Some(1_000_000_000_000),
            Self::Igdb => Some(2_000_000_000_000),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Manga,
    Game,
    VisualNovel,
    Character,
}

impl Kind {
    /// Stored in `profile_works.kind` for works.
    pub fn work_key(&self) -> &'static str {
        match self {
            Self::Game => "game",
            Self::VisualNovel => "vn",
            _ => "manga",
        }
    }
}

/// Smallest id that belongs to a non-AniList character (see `Source::character_base`).
pub const PROVIDER_CHARACTER_MIN: i64 = 1_000_000_000_000;

/// `vndb` + 123 -> an integer id usable in the character tables. Reversible with `decode_character_id`.
pub fn encode_character_id(source: Source, n: i64) -> Option<i64> {
    source.character_base().map(|b| b + n)
}

/// A work or character from one of these providers, normalized.
#[derive(Debug, Clone, PartialEq)]
pub struct Work {
    /// Provider-prefixed: `mdex:<uuid>`, `mu:<id>`, `igdb:<id>`, `vndb:v17`, or for characters `igdb:c<id>` / `vndb:c<id>`.
    pub id: String,
    pub source: Source,
    pub kind: Kind,
    pub title: String,
    /// Original-language name (characters).
    pub native: Option<String>,
    /// For characters: the main work.
    pub subtitle: Option<String>,
    pub year: Option<i32>,
    pub status: Option<String>,
    pub rating: Option<f32>,
    pub url: Option<String>,
    pub cover_url: Option<String>,
    pub genres: Vec<String>,
}

impl Work {
    /// Numeric provider id (`v17` -> 17, `c123` -> 123, `igdb:55` -> 55).
    pub fn numeric_id(&self) -> Option<i64> {
        let tail = self.id.split_once(':')?.1;
        tail.trim_start_matches(['v', 'c']).parse().ok()
    }

    /// Integer id for the character tables (`None` for works).
    pub fn character_id(&self) -> Option<i64> {
        (self.kind == Kind::Character)
            .then(|| encode_character_id(self.source, self.numeric_id()?))
            .flatten()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("http: {0}")]
    Http(String),
    #[error("bad response: {0}")]
    Parse(String),
    #[error("not configured")]
    NotConfigured,
}

pub trait CatalogProvider: Send + Sync {
    fn source(&self) -> Source;
    fn search(&self, query: &str) -> impl Future<Output = Result<Vec<Work>, ProviderError>> + Send;
}

/// Hosts the bot will download pictures from.
const COVER_HOSTS: &[&str] = &[
    "uploads.mangadex.org",
    "cdn.mangaupdates.com",
    "images.igdb.com",
    "t.vndb.org",
];
const MAX_COVER_BYTES: usize = 3 * 1024 * 1024;

pub fn cover_host_allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.contains('@') && COVER_HOSTS.contains(&host)
}

/// Which providers are switched on.
#[derive(Debug, Clone, Default)]
pub struct ProviderConfig {
    pub fallback: bool,
    pub igdb: Option<(String, String)>,
    pub vndb: bool,
}

/// Where `/character` looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterSource {
    All,
    AnimeManga,
    Games,
}

type Limiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

pub struct Providers {
    http: reqwest::Client,
    cfg: ProviderConfig,
    mangadex: MangaDex,
    mangaupdates: MangaUpdates,
    igdb: Option<Igdb>,
    vndb: Vndb,
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
    pub fn new(http: reqwest::Client, cfg: ProviderConfig) -> Self {
        Self {
            mangadex: MangaDex::new(http.clone()),
            mangaupdates: MangaUpdates::new(http.clone()),
            igdb: cfg
                .igdb
                .clone()
                .map(|(id, secret)| Igdb::new(http.clone(), id, secret)),
            vndb: Vndb::new(http.clone()),
            http,
            cfg,
            // MangaDex allows about 5 requests/second per IP; stay below it.
            dex_limit: per_second(3),
            mu_limit: per_second(2),
            searches: Cache::builder()
                .max_capacity(1_000)
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
        self.cfg.fallback
    }
    pub fn igdb_enabled(&self) -> bool {
        self.igdb.is_some()
    }
    pub fn vndb_enabled(&self) -> bool {
        self.cfg.vndb
    }
    pub fn games_enabled(&self) -> bool {
        self.igdb_enabled() || self.vndb_enabled()
    }

    async fn cached<F, Fut>(&self, key: String, f: F) -> Vec<Work>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Vec<Work>>,
    {
        if let Some(hit) = self.searches.get(&key).await {
            return (*hit).clone();
        }
        let out = f().await;
        // Failures are not cached (an empty list would hide a recovered provider for ten minutes).
        if !out.is_empty() {
            self.searches.insert(key, Arc::new(out.clone())).await;
        }
        out
    }

    /// Search the written-work fallbacks; failures of one never hide results of the other.
    pub async fn search(&self, query: &str) -> Vec<Work> {
        if !self.cfg.fallback || query.trim().chars().count() < 2 {
            return Vec::new();
        }
        self.cached(format!("fb:{}", query.trim().to_lowercase()), || async {
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
                    Ok(list) => out.extend(list.into_iter().filter(safety::fallback_safe)),
                    Err(e) => {
                        tracing::debug!(provider = source.prefix(), "fallback search failed: {e}")
                    }
                }
            }
            out.truncate(10);
            out
        })
        .await
    }

    /// Games from IGDB; sexual content is always hidden, mature-rated games only when `allow_mature`.
    pub async fn search_games(&self, query: &str, allow_mature: bool) -> Vec<Work> {
        let (Some(igdb), true) = (&self.igdb, query.trim().chars().count() >= 2) else {
            return Vec::new();
        };
        self.cached(
            format!("igdb:g:{allow_mature}:{}", query.trim().to_lowercase()),
            || async {
                match igdb.search_games(query, allow_mature).await {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::debug!("igdb game search failed: {e}");
                        Vec::new()
                    }
                }
            },
        )
        .await
    }

    pub async fn search_visual_novels(&self, query: &str) -> Vec<Work> {
        if !self.cfg.vndb || query.trim().chars().count() < 2 {
            return Vec::new();
        }
        self.cached(
            format!("vndb:v:{}", query.trim().to_lowercase()),
            || async {
                self.vndb.search_vns(query).await.unwrap_or_else(|e| {
                    tracing::debug!("vndb search failed: {e}");
                    Vec::new()
                })
            },
        )
        .await
    }

    /// Game and visual-novel characters.
    pub async fn search_characters(&self, query: &str, allow_mature: bool) -> Vec<Work> {
        if query.trim().chars().count() < 2 {
            return Vec::new();
        }
        let mut out = Vec::new();
        if let Some(igdb) = &self.igdb {
            let found = self
                .cached(
                    format!("igdb:c:{allow_mature}:{}", query.trim().to_lowercase()),
                    || async {
                        igdb.search_characters(query, allow_mature)
                            .await
                            .unwrap_or_else(|e| {
                                tracing::debug!("igdb character search failed: {e}");
                                Vec::new()
                            })
                    },
                )
                .await;
            out.extend(found);
        }
        if self.cfg.vndb {
            let found = self
                .cached(
                    format!("vndb:c:{}", query.trim().to_lowercase()),
                    || async {
                        self.vndb
                            .search_characters(query)
                            .await
                            .unwrap_or_else(|e| {
                                tracing::debug!("vndb character search failed: {e}");
                                Vec::new()
                            })
                    },
                )
                .await;
            out.extend(found);
        }
        out
    }

    /// Re-find one character from an autocomplete value like `igdb:c55` / `vndb:c123`.
    pub async fn character_by_key(&self, key: &str, allow_mature: bool) -> Option<Work> {
        let (prefix, tail) = key.split_once(':')?;
        let n: i64 = tail.trim_start_matches('c').parse().ok()?;
        match prefix {
            "igdb" => self
                .igdb
                .as_ref()?
                .character_by_id(n, allow_mature)
                .await
                .ok()
                .flatten(),
            "vndb" if self.cfg.vndb => self.vndb.character_by_id(n).await.ok().flatten(),
            _ => None,
        }
    }

    /// Picture bytes fetched by the bot itself (never a hotlink).
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

#[cfg(test)]
mod tests;
