//! MangaDex official API (no key): about 5 requests per second per IP (3 are used), `safe` and `suggestive` content
//! ratings only, chapters filtered by the server's preferred language. Credit "MangaDex" with a link to the chapter
//! on MangaDex; no pages are fetched or hosted.

use super::{
    normalize_number, normalize_title, Chapter, ChapterSource, SourceError, SourceKind, Tracked,
};
use governor::{clock::DefaultClock, state::InMemoryState, state::NotKeyed, Quota, RateLimiter};
use serde_json::Value;
use std::{num::NonZeroU32, time::Duration};

const API: &str = "https://api.mangadex.org";

pub struct MangaDex {
    http: reqwest::Client,
    limit: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
}

impl MangaDex {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            limit: RateLimiter::direct(Quota::per_second(
                NonZeroU32::new(3).unwrap_or(NonZeroU32::MIN),
            )),
        }
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, SourceError> {
        self.limit.until_ready().await;
        let resp = self
            .http
            .get(format!("{API}{path}"))
            .query(query)
            .timeout(Duration::from_secs(8))
            .send()
            .await
            .map_err(|e| SourceError::Http(e.without_url().to_string()))?;
        if !resp.status().is_success() {
            return Err(SourceError::Http(
                crate::http_log::failed("mangadex", "chapter feed", resp).await,
            ));
        }
        crate::ops::health::ok("mangadex");
        resp.json()
            .await
            .map_err(|e| SourceError::Parse(e.to_string()))
    }

    /// The MangaDex id of a work, found by title (the caller remembers it).
    pub async fn find_id(&self, work: &Tracked) -> Result<Option<String>, SourceError> {
        for title in work.titles.iter().take(2) {
            let doc = self
                .get(
                    "/manga",
                    &[
                        ("title", title.as_str()),
                        ("limit", "5"),
                        ("contentRating[]", "safe"),
                        ("contentRating[]", "suggestive"),
                    ],
                )
                .await?;
            if let Some(id) = match_manga(&doc, work) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    pub async fn feed(
        &self,
        manga_id: &str,
        work_id: i64,
        lang: &str,
    ) -> Result<Vec<Chapter>, SourceError> {
        let doc = self
            .get(
                &format!("/manga/{manga_id}/feed"),
                &[
                    ("translatedLanguage[]", lang),
                    ("order[publishAt]", "desc"),
                    ("limit", "10"),
                    ("contentRating[]", "safe"),
                    ("contentRating[]", "suggestive"),
                ],
            )
            .await?;
        parse_feed(&doc, work_id)
    }
}

impl ChapterSource for MangaDex {
    async fn fetch(&self, work: &Tracked, lang: &str) -> Result<Vec<Chapter>, SourceError> {
        match self.find_id(work).await? {
            Some(id) => self.feed(&id, work.work_id, lang).await,
            None => Ok(Vec::new()),
        }
    }
}

/// The search result whose title (or an alternative title) equals one of the work's titles.
pub fn match_manga(doc: &Value, work: &Tracked) -> Option<String> {
    let wanted: Vec<String> = work
        .titles
        .iter()
        .map(|t| normalize_title(t))
        .filter(|t| !t.is_empty())
        .collect();
    for item in doc.get("data")?.as_array()? {
        let attrs = item.get("attributes")?;
        // Only the allowed ratings are ever matched, even if the filter were ignored.
        if !matches!(
            attrs.get("contentRating").and_then(Value::as_str),
            Some("safe" | "suggestive")
        ) {
            continue;
        }
        let mut names: Vec<String> = attrs
            .get("title")
            .and_then(Value::as_object)
            .map(|o| {
                o.values()
                    .filter_map(Value::as_str)
                    .map(normalize_title)
                    .collect()
            })
            .unwrap_or_default();
        for alt in attrs
            .get("altTitles")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            names.extend(
                alt.as_object()
                    .into_iter()
                    .flat_map(|o| o.values().filter_map(Value::as_str).map(normalize_title)),
            );
        }
        if names.iter().any(|n| wanted.contains(n)) {
            return item.get("id")?.as_str().map(str::to_string);
        }
    }
    None
}

fn parse_time(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp())
}

/// Chapters of a manga feed. Chapters that only point to an external site, and anything without an id, are skipped.
pub fn parse_feed(doc: &Value, work_id: i64) -> Result<Vec<Chapter>, SourceError> {
    let data = doc
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| SourceError::Parse("missing data".into()))?;
    Ok(data
        .iter()
        .filter_map(|c| {
            let id = c.get("id")?.as_str()?;
            let a = c.get("attributes")?;
            if a.get("externalUrl").is_some_and(|u| !u.is_null()) {
                return None;
            }
            let number = a
                .get("chapter")
                .and_then(Value::as_str)
                .and_then(normalize_number);
            Some(Chapter {
                source: SourceKind::MangaDex,
                work_id,
                key: number.clone().unwrap_or_else(|| format!("mdex:{id}")),
                number,
                title: a
                    .get("title")
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string),
                url: format!("https://mangadex.org/chapter/{id}"),
                lang: a
                    .get("translatedLanguage")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                published: a
                    .get("publishAt")
                    .and_then(Value::as_str)
                    .and_then(parse_time),
            })
        })
        .collect())
}
