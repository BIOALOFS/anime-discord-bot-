//! IGDB (games and game characters). Authentication is the Twitch client-credentials flow; the token is cached
//! and refreshed shortly before it expires. IGDB allows 4 requests/second and 8 open requests, and returns at
//! most 500 results per request. Non-commercial use only; the card footer credits "Data from IGDB.com".

use super::safety::{self, GameFacts};
use super::{CatalogProvider, Kind, ProviderError, Source, Work};
use chrono::Datelike;
use governor::{clock::DefaultClock, state::InMemoryState, state::NotKeyed, Quota, RateLimiter};
use serde_json::Value;
use std::{
    num::NonZeroU32,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Semaphore};

const API: &str = "https://api.igdb.com/v4";
const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
const IMAGE: &str = "https://images.igdb.com/igdb/image/upload/t_cover_big";
pub(crate) const GAME_FIELDS: &str = "name,summary,url,first_release_date,total_rating,total_rating_count,cover.image_id,genres.name,themes.id,themes.name,keywords.name";
pub(super) const CHARACTER_FIELDS: &str = "name,akas,url,mug_shot.image_id,games.name,games.cover.image_id,games.total_rating_count,games.genres.name,games.themes.id,games.themes.name,games.keywords.name";

type Limiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

struct Token {
    value: String,
    expires: Instant,
}

pub struct Igdb {
    http: reqwest::Client,
    id: String,
    secret: String,
    token: Mutex<Option<Token>>,
    limit: Limiter,
    open: Semaphore,
}

impl Igdb {
    pub fn new(http: reqwest::Client, id: String, secret: String) -> Self {
        Self {
            http,
            id,
            secret,
            token: Mutex::new(None),
            limit: RateLimiter::direct(Quota::per_second(
                NonZeroU32::new(4).unwrap_or(NonZeroU32::MIN),
            )),
            open: Semaphore::new(8),
        }
    }

    async fn token(&self) -> Result<String, ProviderError> {
        let mut guard = self.token.lock().await;
        if let Some(t) = guard.as_ref() {
            if t.expires > Instant::now() {
                return Ok(t.value.clone());
            }
        }
        let resp = self
            .http
            .post(TOKEN_URL)
            .query(&[
                ("client_id", self.id.as_str()),
                ("client_secret", self.secret.as_str()),
                ("grant_type", "client_credentials"),
            ])
            .timeout(Duration::from_secs(8))
            .send()
            .await
            // The error text never contains the request URL (it carries the secret).
            .map_err(|e| ProviderError::Http(e.without_url().to_string()))?;
        if !resp.status().is_success() {
            return Err(ProviderError::Http(format!(
                "token {}",
                crate::http_log::failed("igdb", "twitch token", resp).await
            )));
        }
        let doc: Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;
        let value = doc
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or_else(|| ProviderError::Parse("no token".into()))?
            .to_string();
        let ttl = doc
            .get("expires_in")
            .and_then(Value::as_u64)
            .unwrap_or(3600)
            .saturating_sub(120)
            .max(60);
        *guard = Some(Token {
            value: value.clone(),
            expires: Instant::now() + Duration::from_secs(ttl),
        });
        Ok(value)
    }

    pub(crate) async fn query(&self, endpoint: &str, body: String) -> Result<Value, ProviderError> {
        for attempt in 0..2 {
            let token = self.token().await?;
            let _open = self
                .open
                .acquire()
                .await
                .map_err(|_| ProviderError::NotConfigured)?;
            self.limit.until_ready().await;
            crate::ops::stats::bump("api", "igdb");
            let resp = self
                .http
                .post(format!("{API}/{endpoint}"))
                .header("Client-ID", &self.id)
                .bearer_auth(token)
                .body(body.clone())
                .timeout(Duration::from_secs(8))
                .send()
                .await
                .map_err(|e| ProviderError::Http(e.without_url().to_string()))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                *self.token.lock().await = None;
                continue;
            }
            if !resp.status().is_success() {
                return Err(ProviderError::Http(
                    crate::http_log::failed("igdb", endpoint, resp).await,
                ));
            }
            let doc: Value = resp
                .json()
                .await
                .map_err(|e| ProviderError::Parse(e.to_string()))?;
            // An error body is a failure, not an empty answer: nothing may be judged from it.
            ensure_games(&doc)?;
            crate::ops::health::ok("igdb");
            return Ok(doc);
        }
        Err(ProviderError::Http("unauthorized".into()))
    }

    /// The smallest possible request (the status panel): does IGDB answer with our token?
    pub async fn ping(&self) -> bool {
        self.query("games", "fields name; limit 1;".to_string())
            .await
            .is_ok()
    }

    pub async fn search_games(
        &self,
        query: &str,
        allow_mature: bool,
    ) -> Result<Vec<Work>, ProviderError> {
        let doc = self.query("games", games_search_body(query, None)).await?;
        Ok(rank(parse_games(&doc, allow_mature), 10))
    }

    /// Search narrowed to a platform (a case-insensitive part of its name, e.g. "playstation").
    pub async fn search_games_on(
        &self,
        query: &str,
        platform: &str,
        allow_mature: bool,
    ) -> Result<Vec<Work>, ProviderError> {
        let doc = self
            .query("games", games_search_body(query, Some(platform)))
            .await?;
        Ok(rank(parse_games(&doc, allow_mature), 10))
    }

    /// One page (40) of well-known main games, most rated first; `offset` picks the neighbourhood and `min_votes` the
    /// popularity floor. `platform` narrows it.
    pub async fn popular_games(
        &self,
        offset: u32,
        min_votes: u32,
        platform: Option<&str>,
        allow_mature: bool,
    ) -> Result<Vec<Work>, ProviderError> {
        let doc = self
            .query("games", popular_games_body(offset, min_votes, platform))
            .await?;
        Ok(parse_games(&doc, allow_mature)
            .into_iter()
            .map(|(w, _)| w)
            .collect())
    }

    /// The characters of one game (the game itself is judged through the same fields).
    pub async fn characters_of(
        &self,
        game: i64,
        allow_mature: bool,
    ) -> Result<Vec<Work>, ProviderError> {
        let doc = self
            .query("characters", characters_of_game_body(game))
            .await?;
        Ok(rank(parse_characters(&doc, allow_mature), 50))
    }

    pub async fn popular_characters(
        &self,
        offset: u32,
        allow_mature: bool,
    ) -> Result<Vec<Work>, ProviderError> {
        let doc = self
            .query("characters", popular_characters_body(offset))
            .await?;
        Ok(parse_characters(&doc, allow_mature)
            .into_iter()
            .map(|(w, _)| w)
            .collect())
    }

    pub async fn game_by_id(
        &self,
        id: i64,
        allow_mature: bool,
    ) -> Result<Option<Work>, ProviderError> {
        let doc = self.query("games", game_by_id_body(id)).await?;
        Ok(parse_games(&doc, allow_mature)
            .into_iter()
            .next()
            .map(|(w, _)| w))
    }

    pub async fn search_characters(
        &self,
        query: &str,
        allow_mature: bool,
    ) -> Result<Vec<Work>, ProviderError> {
        let doc = self
            .query("characters", characters_search_body(query))
            .await?;
        Ok(rank(parse_characters(&doc, allow_mature), 10))
    }

    pub async fn character_by_id(
        &self,
        id: i64,
        allow_mature: bool,
    ) -> Result<Option<Work>, ProviderError> {
        let doc = self.query("characters", character_by_id_body(id)).await?;
        Ok(parse_characters(&doc, allow_mature)
            .into_iter()
            .next()
            .map(|(w, _)| w))
    }
}

// ---- request bodies (Apicalypse, pure so the exact text is tested) ------------------------------------
// Fields come from the IGDB documentation. Main games are `version_parent = null & parent_game = null` (no editions,
// DLC, expansions or bundle parts) so the deprecated `category` field is not needed.

/// The "main games only" condition.
pub(super) const MAIN_GAMES: &str = "version_parent = null & parent_game = null";

fn platform_clause(platform: Option<&str>) -> String {
    platform
        .map(|p| format!(" & platforms.name ~ *{}*", quote(p)))
        .unwrap_or_default()
}

pub(super) fn games_search_body(query: &str, platform: Option<&str>) -> String {
    format!(
        "search {}; fields {GAME_FIELDS}; where {MAIN_GAMES}{}; limit 50;",
        quote(query),
        platform_clause(platform)
    )
}

pub(super) fn game_by_id_body(id: i64) -> String {
    format!("fields {GAME_FIELDS}; where id = {id}; limit 1;")
}

pub(super) fn popular_games_body(offset: u32, min_votes: u32, platform: Option<&str>) -> String {
    format!(
        "fields {GAME_FIELDS}; where {MAIN_GAMES} & total_rating_count > {min_votes}{}; sort total_rating_count desc; limit 40; offset {offset};",
        platform_clause(platform)
    )
}

pub(super) fn characters_search_body(query: &str) -> String {
    format!(
        "search {}; fields {CHARACTER_FIELDS}; limit 20;",
        quote(query)
    )
}

pub(super) fn characters_of_game_body(game: i64) -> String {
    format!("fields {CHARACTER_FIELDS}; where games = ({game}); limit 50;")
}

pub(super) fn character_by_id_body(id: i64) -> String {
    format!("fields {CHARACTER_FIELDS}; where id = {id}; limit 1;")
}

pub(super) fn popular_characters_body(offset: u32) -> String {
    format!("fields {CHARACTER_FIELDS}; where mug_shot != null; limit 40; offset {offset};")
}

impl CatalogProvider for Igdb {
    fn source(&self) -> Source {
        Source::Igdb
    }
    async fn search(&self, query: &str) -> Result<Vec<Work>, ProviderError> {
        self.search_games(query, true).await
    }
}

/// An Apicalypse string literal.
pub(super) fn quote(q: &str) -> String {
    let clean: String = q.chars().filter(|c| !c.is_control()).collect();
    format!("\"{}\"", clean.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Most popular first, then cut.
fn rank(mut v: Vec<(Work, u64)>, n: usize) -> Vec<Work> {
    v.sort_by_key(|(_, weight)| std::cmp::Reverse(*weight));
    v.into_iter().take(n).map(|(w, _)| w).collect()
}

fn names(v: &Value, key: &str) -> Vec<String> {
    arr(v, key)
        .iter()
        .filter_map(|x| x.get("name").and_then(Value::as_str).map(str::to_string))
        .collect()
}

/// The content facts of one game: themes, genres and keywords only (age ratings are never read).
fn facts_of(game: &Value) -> GameFacts {
    let ids: Vec<i64> = arr(game, "themes")
        .iter()
        .filter_map(|t| t.get("id").and_then(Value::as_i64))
        .collect();
    safety::game_facts(
        &ids,
        &names(game, "themes"),
        &names(game, "genres"),
        &names(game, "keywords"),
    )
}

/// IGDB reports a bad request as an array of error objects (`status`, `title`, `cause`) instead of games: that is
/// "the fields were not retrieved", never "no games", and must not be judged.
pub(super) fn ensure_games(doc: &Value) -> Result<(), ProviderError> {
    let Some(list) = doc.as_array() else {
        return Err(ProviderError::Parse("not a list".into()));
    };
    if list.iter().any(|g| {
        g.get("id").is_none()
            && (g.get("status").is_some() || g.get("cause").is_some() || g.get("message").is_some())
    }) {
        return Err(ProviderError::Parse("IGDB error response".into()));
    }
    Ok(())
}

fn arr<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn image(v: &Value, key: &str) -> Option<String> {
    let id = v.pointer(&format!("/{key}/image_id"))?.as_str()?;
    // `image_id` is alphanumeric; anything else would let a response steer the URL.
    id.chars()
        .all(|c| c.is_ascii_alphanumeric())
        .then(|| format!("{IMAGE}/{id}.jpg"))
}

pub(crate) fn parse_games(doc: &Value, allow_mature: bool) -> Vec<(Work, u64)> {
    let Some(list) = doc.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|g| {
            let id = g.get("id")?.as_i64()?;
            // Verdict first: a sexual-content signal never gets through; horror and war only when allowed.
            if !safety::game_allowed(&facts_of(g), allow_mature) {
                return None;
            }
            let year = g
                .get("first_release_date")
                .and_then(Value::as_i64)
                .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
                .map(|d| d.year());
            let work = Work {
                id: format!("igdb:{id}"),
                source: Source::Igdb,
                kind: Kind::Game,
                title: g.get("name")?.as_str()?.to_string(),
                native: None,
                subtitle: None,
                year,
                status: None,
                rating: g
                    .get("total_rating")
                    .and_then(Value::as_f64)
                    .map(|r| (r / 10.0) as f32),
                url: g
                    .get("url")
                    .and_then(Value::as_str)
                    .filter(|u| u.starts_with("https://www.igdb.com/"))
                    .map(str::to_string),
                cover_url: image(g, "cover"),
                genres: arr(g, "genres")
                    .iter()
                    .filter_map(|x| x.get("name").and_then(Value::as_str).map(str::to_string))
                    .collect(),
                work_key: None,
            };
            Some((
                work,
                g.get("total_rating_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            ))
        })
        .collect()
}

/// A character is shown only when it has appearances, none of them carries a sexual signal, and at least one is allowed.
pub(super) fn parse_characters(doc: &Value, allow_mature: bool) -> Vec<(Work, u64)> {
    let Some(list) = doc.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|c| {
            let id = c.get("id")?.as_i64()?;
            let games = arr(c, "games");
            let facts: Vec<GameFacts> = games.iter().map(facts_of).collect();
            if facts.iter().any(safety::is_sexual) {
                return None;
            }
            // The games the character appears in that may be shown, most rated first.
            let mut shown: Vec<(&Value, u64)> = Vec::new();
            for (g, f) in games.iter().zip(&facts) {
                if safety::game_allowed(f, allow_mature) {
                    shown.push((
                        g,
                        g.get("total_rating_count")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                    ));
                }
            }
            shown.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            let weight: u64 = shown.iter().map(|(_, n)| n).sum();
            let (main, _) = *shown.first()?;
            let titles: Vec<&str> = shown
                .iter()
                .filter_map(|(g, _)| g.get("name").and_then(Value::as_str))
                .take(3)
                .collect();
            let work = Work {
                id: format!("igdb:c{id}"),
                source: Source::Igdb,
                kind: Kind::Character,
                title: c.get("name")?.as_str()?.to_string(),
                native: None,
                subtitle: Some(titles.join(" · ")).filter(|t| !t.is_empty()),
                year: None,
                status: None,
                rating: None,
                url: c
                    .get("url")
                    .and_then(Value::as_str)
                    .filter(|u| u.starts_with("https://www.igdb.com/"))
                    .map(str::to_string),
                cover_url: image(c, "mug_shot"),
                genres: Vec::new(),
                work_key: main
                    .get("id")
                    .and_then(Value::as_i64)
                    .map(|g| format!("igdb:{g}")),
            };
            Some((work, weight))
        })
        .collect()
}
