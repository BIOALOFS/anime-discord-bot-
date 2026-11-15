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
const GAME_FIELDS: &str = "name,url,first_release_date,total_rating,total_rating_count,cover.image_id,genres.name,themes.id,themes.name,age_ratings.category,age_ratings.rating";
const CHARACTER_FIELDS: &str = "name,url,mug_shot.image_id,games.name,games.total_rating_count,games.themes.id,games.themes.name,games.age_ratings.category,games.age_ratings.rating";

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
            return Err(ProviderError::Http(format!("token {}", resp.status())));
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

    async fn query(&self, endpoint: &str, body: String) -> Result<Value, ProviderError> {
        for attempt in 0..2 {
            let token = self.token().await?;
            let _open = self
                .open
                .acquire()
                .await
                .map_err(|_| ProviderError::NotConfigured)?;
            self.limit.until_ready().await;
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
                return Err(ProviderError::Http(resp.status().to_string()));
            }
            return resp
                .json()
                .await
                .map_err(|e| ProviderError::Parse(e.to_string()));
        }
        Err(ProviderError::Http("unauthorized".into()))
    }

    pub async fn search_games(
        &self,
        query: &str,
        allow_mature: bool,
    ) -> Result<Vec<Work>, ProviderError> {
        let body = format!(
            "search {}; fields {GAME_FIELDS}; where version_parent = null; limit 25;",
            quote(query)
        );
        let doc = self.query("games", body).await?;
        Ok(rank(parse_games(&doc, allow_mature), 10))
    }

    pub async fn game_by_id(
        &self,
        id: i64,
        allow_mature: bool,
    ) -> Result<Option<Work>, ProviderError> {
        let body = format!("fields {GAME_FIELDS}; where id = {id}; limit 1;");
        let doc = self.query("games", body).await?;
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
        let body = format!(
            "search {}; fields {CHARACTER_FIELDS}; limit 25;",
            quote(query)
        );
        let doc = self.query("characters", body).await?;
        Ok(rank(parse_characters(&doc, allow_mature), 10))
    }

    pub async fn character_by_id(
        &self,
        id: i64,
        allow_mature: bool,
    ) -> Result<Option<Work>, ProviderError> {
        let body = format!("fields {CHARACTER_FIELDS}; where id = {id}; limit 1;");
        let doc = self.query("characters", body).await?;
        Ok(parse_characters(&doc, allow_mature)
            .into_iter()
            .next()
            .map(|(w, _)| w))
    }
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

fn facts_of(game: &Value) -> GameFacts {
    let ids: Vec<i64> = arr(game, "themes")
        .iter()
        .filter_map(|t| t.get("id").and_then(Value::as_i64))
        .collect();
    let names: Vec<String> = arr(game, "themes")
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str).map(str::to_string))
        .collect();
    let ratings: Vec<(i64, i64)> = arr(game, "age_ratings")
        .iter()
        .filter_map(|r| Some((r.get("category")?.as_i64()?, r.get("rating")?.as_i64()?)))
        .collect();
    safety::facts_from(&ids, &names, &ratings)
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

pub(super) fn parse_games(doc: &Value, allow_mature: bool) -> Vec<(Work, u64)> {
    let Some(list) = doc.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|g| {
            let id = g.get("id")?.as_i64()?;
            // Verdict first: sexual content, AO and (optionally) mature games never get through.
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

/// A character is shown only when it has appearances, none of them is erotic/AO, and at least one is allowed.
pub(super) fn parse_characters(doc: &Value, allow_mature: bool) -> Vec<(Work, u64)> {
    let Some(list) = doc.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|c| {
            let id = c.get("id")?.as_i64()?;
            let games = arr(c, "games");
            let facts: Vec<GameFacts> = games.iter().map(facts_of).collect();
            if facts.iter().any(|f| f.erotic || f.esrb_ao) {
                return None;
            }
            let mut main: Option<(&Value, u64)> = None;
            let mut weight = 0;
            for (g, f) in games.iter().zip(&facts) {
                if !safety::game_allowed(f, allow_mature) {
                    continue;
                }
                let n = g
                    .get("total_rating_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                weight += n;
                if main.is_none_or(|(_, best)| n > best) {
                    main = Some((g, n));
                }
            }
            let (main, _) = main?;
            let work = Work {
                id: format!("igdb:c{id}"),
                source: Source::Igdb,
                kind: Kind::Character,
                title: c.get("name")?.as_str()?.to_string(),
                native: None,
                subtitle: main.get("name").and_then(Value::as_str).map(str::to_string),
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
            };
            Some((work, weight))
        })
        .collect()
}
