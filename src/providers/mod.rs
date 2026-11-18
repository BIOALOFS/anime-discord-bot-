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
    /// For characters: the provider key of the main game / novel (`igdb:55`, `vndb:v17`), for "أظهر العمل".
    pub work_key: Option<String>,
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

/// One provider's answer: `None` when it was not asked.
pub type Asked = Option<Result<Vec<Work>, ProviderError>>;

/// Results of a query that goes to several game providers: what succeeded, and who did not answer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    pub works: Vec<Work>,
    /// Providers that were asked.
    pub tried: Vec<Source>,
    /// Providers that failed (their results are simply missing).
    pub failed: Vec<Source>,
}

impl Found {
    /// Combine per-provider results (`None` = not asked).
    pub fn from_results<const N: usize>(results: [(Source, Asked); N]) -> Self {
        let mut f = Self::default();
        for (source, r) in results {
            match r {
                None => {}
                Some(Ok(list)) => {
                    f.tried.push(source);
                    f.works.extend(list);
                }
                Some(Err(_)) => {
                    f.tried.push(source);
                    f.failed.push(source);
                }
            }
        }
        f
    }

    /// Every provider that was asked failed: only then is "the source does not respond" the right message.
    pub fn all_failed(&self) -> bool {
        !self.tried.is_empty() && self.failed.len() == self.tried.len()
    }

    /// A short Arabic note when some, but not all, providers failed.
    pub fn partial_note(&self) -> Option<String> {
        if self.failed.is_empty() || self.all_failed() {
            return None;
        }
        let names = |v: &[Source]| {
            v.iter()
                .map(|s| s.prefix().to_uppercase())
                .collect::<Vec<_>>()
                .join(" و")
        };
        let ok: Vec<Source> = self
            .tried
            .iter()
            .copied()
            .filter(|s| !self.failed.contains(s))
            .collect();
        Some(format!(
            "لم أستطع جلب نتائج من {} الآن، هذه نتائج {} فقط.",
            names(&self.failed),
            names(&ok)
        ))
    }

    /// The message for an empty answer: unavailable when everything failed, otherwise "nothing found".
    pub fn empty_message(&self, nothing: &'static str) -> &'static str {
        if self.all_failed() {
            crate::i18n::GAMES_DOWN
        } else {
            nothing
        }
    }
}

/// Tries per provider for a random pick (at least ten different offsets).
pub const RANDOM_TRIES: usize = 12;

/// `n` distinct random offsets inside the first 500 results: IGDB pages of 40 (offsets 0..=460), VNDB pages (1..=25).
pub fn offset_plan(n: usize, provider: Source) -> Vec<u32> {
    let mut all: Vec<u32> = if provider == Source::Igdb {
        (0..12).map(|i| i * 40).collect()
    } else {
        (1..=25).collect()
    };
    for i in (1..all.len()).rev() {
        all.swap(i, rand::random_range(0..=i));
    }
    all.truncate(n.min(all.len()));
    all
}

/// A random candidate that is not one of the member's last five picks (remembered once chosen).
pub fn pick_unseen(mut list: Vec<Work>, user: i64, scope: &str) -> Option<Work> {
    for i in (1..list.len()).rev() {
        list.swap(i, rand::random_range(0..=i));
    }
    let pos = list
        .iter()
        .position(|w| !crate::commands::picks::seen(user, scope, &w.id))?;
    let w = list.swap_remove(pos);
    crate::commands::picks::remember(user, scope, &w.id);
    Some(w)
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
    "image.tmdb.org",
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

    /// A real lightweight IGDB request for the status panel (`false` when IGDB is off or failing).
    pub async fn ping_igdb(&self) -> bool {
        match &self.igdb {
            Some(igdb) => igdb.ping().await,
            None => false,
        }
    }

    /// Like `cached`, but a failure is passed on (and never cached) so the caller can tell "nothing found" from "the
    /// provider did not answer".
    async fn cached_result<F, Fut>(&self, key: String, f: F) -> Result<Vec<Work>, ProviderError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Vec<Work>, ProviderError>>,
    {
        if let Some(hit) = self.searches.get(&key).await {
            return Ok((*hit).clone());
        }
        let out = f().await?;
        if !out.is_empty() {
            self.searches.insert(key, Arc::new(out.clone())).await;
        }
        Ok(out)
    }

    /// Games from IGDB, judged by content: a sexual signal always hides a game; horror and war only when
    /// `allow_mature` (the `mature-themes` setting) is off. Age ratings play no part.
    async fn igdb_games(
        &self,
        query: &str,
        platform: Option<&str>,
        mature: bool,
    ) -> Result<Vec<Work>, ProviderError> {
        let Some(igdb) = &self.igdb else {
            return Ok(Vec::new());
        };
        let key = format!(
            "igdb:g:{mature}:{}:{}",
            platform.unwrap_or("").to_lowercase(),
            query.trim().to_lowercase()
        );
        self.cached_result(key, || async {
            match platform {
                Some(p) => igdb.search_games_on(query, p, mature).await,
                None => igdb.search_games(query, mature).await,
            }
        })
        .await
    }

    async fn vndb_novels(&self, query: &str, mature: bool) -> Result<Vec<Work>, ProviderError> {
        self.cached_result(
            format!("vndb:v:{mature}:{}", query.trim().to_lowercase()),
            || self.vndb.search_vns(query, mature),
        )
        .await
    }

    async fn igdb_characters(&self, query: &str, mature: bool) -> Result<Vec<Work>, ProviderError> {
        let Some(igdb) = &self.igdb else {
            return Ok(Vec::new());
        };
        self.cached_result(
            format!("igdb:c:{mature}:{}", query.trim().to_lowercase()),
            || igdb.search_characters(query, mature),
        )
        .await
    }

    async fn vndb_characters(&self, query: &str, mature: bool) -> Result<Vec<Work>, ProviderError> {
        self.cached_result(
            format!("vndb:c:{mature}:{}", query.trim().to_lowercase()),
            || self.vndb.search_characters(query, mature),
        )
        .await
    }

    /// Which providers a query goes to: `source` is `igdb`, `vndb` or `None` for both; a platform is an IGDB notion.
    fn targets(&self, source: Option<&str>, platform: Option<&str>) -> (bool, bool) {
        (
            self.igdb.is_some() && source != Some("vndb"),
            self.cfg.vndb && source != Some("igdb") && platform.is_none(),
        )
    }

    /// Search games and visual novels. One provider failing never blocks the other: its name is in `failed`.
    pub async fn search_games_found(
        &self,
        query: &str,
        platform: Option<&str>,
        mature: bool,
        source: Option<&str>,
    ) -> Found {
        if query.trim().chars().count() < 2 {
            return Found::default();
        }
        let (igdb, vndb) = self.targets(source, platform);
        let (g, v) = futures::join!(
            async {
                if igdb {
                    Some(self.igdb_games(query, platform, mature).await)
                } else {
                    None
                }
            },
            async {
                if vndb {
                    Some(self.vndb_novels(query, mature).await)
                } else {
                    None
                }
            }
        );
        Found::from_results([(Source::Igdb, g), (Source::Vndb, v)])
    }

    /// Search game and visual-novel characters (same partial-result rule).
    pub async fn search_characters_found(&self, query: &str, mature: bool) -> Found {
        if query.trim().chars().count() < 2 {
            return Found::default();
        }
        let (g, v) = futures::join!(
            async {
                if self.igdb.is_some() {
                    Some(self.igdb_characters(query, mature).await)
                } else {
                    None
                }
            },
            async {
                if self.cfg.vndb {
                    Some(self.vndb_characters(query, mature).await)
                } else {
                    None
                }
            }
        );
        Found::from_results([(Source::Igdb, g), (Source::Vndb, v)])
    }

    /// Games for suggestions and autocomplete: whatever succeeded (a failure only shortens the list).
    pub async fn search_games(&self, query: &str, mature: bool) -> Vec<Work> {
        self.search_games_found(query, None, mature, Some("igdb"))
            .await
            .works
    }

    pub async fn search_visual_novels(&self, query: &str, mature: bool) -> Vec<Work> {
        self.search_games_found(query, None, mature, Some("vndb"))
            .await
            .works
    }

    pub async fn search_characters(&self, query: &str, mature: bool) -> Vec<Work> {
        self.search_characters_found(query, mature).await.works
    }

    /// A random well-known main game (optionally on a platform) or visual novel that passes the safety filter, is not one of
    /// the member's last five picks and fits the filters. Candidate pages are popularity-sorted, inside the first 500
    /// results, and cached; when a page has nothing usable the next of at least ten different offsets is tried before
    /// giving up. A provider that fails is skipped and reported, never fatal for the other.
    pub async fn random_work_found(
        &self,
        source: Option<&str>,
        platform: Option<&str>,
        mature: bool,
        user: i64,
    ) -> Found {
        let (igdb, vndb) = self.targets(source, platform);
        let mut order: Vec<Source> = [(igdb, Source::Igdb), (vndb, Source::Vndb)]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, s)| s)
            .collect();
        if order.len() == 2 && rand::random::<bool>() {
            order.swap(0, 1);
        }
        let mut found = Found::default();
        for provider in order {
            found.tried.push(provider);
            let mut ok = true;
            for (n, offset) in offset_plan(RANDOM_TRIES, provider).into_iter().enumerate() {
                // The first six attempts want well-known games; later ones accept anything rated.
                let floor = if n < 6 { 5 } else { 0 };
                let page = match provider {
                    Source::Igdb => match &self.igdb {
                        Some(igdb) => {
                            let key = format!(
                                "igdb:rand:{mature}:{}:{offset}:{floor}",
                                platform.unwrap_or("").to_lowercase()
                            );
                            self.cached_result(key, || {
                                igdb.popular_games(offset, floor, platform, mature)
                            })
                            .await
                        }
                        None => Ok(Vec::new()),
                    },
                    _ => {
                        self.cached_result(format!("vndb:rand:{mature}:{offset}"), || {
                            self.vndb.popular_vns(offset, mature)
                        })
                        .await
                    }
                };
                match page {
                    Ok(list) => {
                        if let Some(w) = pick_unseen(list, user, "game") {
                            found.works.push(w);
                            return found;
                        }
                    }
                    Err(_) => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                found.failed.push(provider);
            }
        }
        found
    }

    /// A random game or visual-novel character (same widening, nothing to filter by platform).
    pub async fn random_character_found(
        &self,
        source: Option<&str>,
        mature: bool,
        user: i64,
    ) -> Found {
        let (igdb, vndb) = self.targets(source, None);
        let mut order: Vec<Source> = [(igdb, Source::Igdb), (vndb, Source::Vndb)]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, s)| s)
            .collect();
        if order.len() == 2 && rand::random::<bool>() {
            order.swap(0, 1);
        }
        let mut found = Found::default();
        for provider in order {
            found.tried.push(provider);
            let mut ok = true;
            for offset in offset_plan(RANDOM_TRIES, provider) {
                let page = match provider {
                    Source::Igdb => match &self.igdb {
                        Some(igdb) => {
                            self.cached_result(format!("igdb:randc:{mature}:{offset}"), || {
                                igdb.popular_characters(offset, mature)
                            })
                            .await
                        }
                        None => Ok(Vec::new()),
                    },
                    _ => {
                        self.cached_result(format!("vndb:randc:{mature}:{offset}"), || {
                            self.vndb.popular_characters(mature, offset)
                        })
                        .await
                    }
                };
                match page {
                    Ok(list) => {
                        if let Some(w) = pick_unseen(list, user, "game-character") {
                            found.works.push(w);
                            return found;
                        }
                    }
                    Err(_) => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                found.failed.push(provider);
            }
        }
        found
    }

    /// The characters of a game or visual novel from its provider key (`igdb:55`, `vndb:v17`). A game that is hidden by
    /// the safety rules has no roster; a failed request is an error, never an empty roster.
    pub async fn characters_of(&self, key: &str, mature: bool) -> Result<Vec<Work>, ProviderError> {
        let Some((prefix, tail)) = key.split_once(':') else {
            return Ok(Vec::new());
        };
        self.cached_result(format!("roster:{key}:{mature}"), || async {
            match prefix {
                "igdb" => match (&self.igdb, tail.parse::<i64>()) {
                    (Some(igdb), Ok(id)) => igdb.characters_of(id, mature).await,
                    _ => Ok(Vec::new()),
                },
                "vndb" if self.cfg.vndb && tail.starts_with('v') => {
                    self.vndb.characters_of(tail, mature).await
                }
                _ => Ok(Vec::new()),
            }
        })
        .await
    }

    /// Re-find one game or visual novel from a provider-prefixed key (`igdb:55`, `vndb:v17`); `None` when the
    /// provider is off, the item is unknown or it fails the safety rules.
    pub async fn work_by_key(&self, key: &str, mature: bool) -> Option<Work> {
        let (prefix, tail) = key.split_once(':')?;
        match prefix {
            "igdb" => self
                .igdb
                .as_ref()?
                .game_by_id(tail.parse().ok()?, mature)
                .await
                .map_err(|e| tracing::warn!("igdb game lookup failed: {e}"))
                .ok()
                .flatten(),
            "vndb"
                if self.cfg.vndb && tail.starts_with('v') && tail[1..].parse::<u64>().is_ok() =>
            {
                self.vndb
                    .vn_by_id(tail, mature)
                    .await
                    .map_err(|e| tracing::warn!("vndb lookup failed: {e}"))
                    .ok()
                    .flatten()
            }
            _ => None,
        }
    }

    /// Re-find one character from an autocomplete value like `igdb:c55` / `vndb:c123`.
    pub async fn character_by_key(&self, key: &str, mature: bool) -> Option<Work> {
        let (prefix, tail) = key.split_once(':')?;
        let n: i64 = tail.trim_start_matches('c').parse().ok()?;
        match prefix {
            "igdb" => self
                .igdb
                .as_ref()?
                .character_by_id(n, mature)
                .await
                .map_err(|e| tracing::warn!("igdb character lookup failed: {e}"))
                .ok()
                .flatten(),
            "vndb" if self.cfg.vndb => self
                .vndb
                .character_by_id(n, mature)
                .await
                .map_err(|e| tracing::warn!("vndb character lookup failed: {e}"))
                .ok()
                .flatten(),
            _ => None,
        }
    }

    /// Picture bytes fetched by the bot itself (never a hotlink).
    pub async fn cover(&self, work: &Work) -> Option<Arc<Vec<u8>>> {
        self.image_bytes(work.cover_url.as_deref()?).await
    }

    /// Same, for a stored picture URL.
    pub async fn image_bytes(&self, url: &str) -> Option<Arc<Vec<u8>>> {
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
