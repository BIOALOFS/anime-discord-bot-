//! MangaUpdates v1 API. It has no content rating, so a work is shown only when its genres are known and none
//! of them is an adult genre.

use super::{CatalogProvider, Kind, ProviderError, Source, Work};
use serde_json::{json, Value};

const API: &str = "https://api.mangaupdates.com/v1";

pub struct MangaUpdates {
    http: reqwest::Client,
}

impl MangaUpdates {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }
}

impl CatalogProvider for MangaUpdates {
    fn source(&self) -> Source {
        Source::MangaUpdates
    }

    async fn search(&self, query: &str) -> Result<Vec<Work>, ProviderError> {
        let resp = self
            .http
            .post(format!("{API}/series/search"))
            .json(&json!({"search": query, "perpage": 10}))
            .timeout(std::time::Duration::from_secs(8))
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(ProviderError::Http(
                crate::http_log::failed("mangaupdates", "series search", resp).await,
            ));
        }
        let doc: Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;
        parse_search(&doc)
    }
}

pub(super) fn parse_search(doc: &Value) -> Result<Vec<Work>, ProviderError> {
    let results = doc
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderError::Parse("missing results".into()))?;
    Ok(results
        .iter()
        .filter_map(|r| parse_one(r.get("record")?))
        .collect())
}

fn parse_one(rec: &Value) -> Option<Work> {
    let id = rec.get("series_id")?.as_i64()?;
    let genres: Vec<String> = rec
        .get("genres")
        .and_then(Value::as_array)
        .map(|g| {
            g.iter()
                .filter_map(|x| x.get("genre").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Some(Work {
        work_key: None,
        id: format!("mu:{id}"),
        source: Source::MangaUpdates,
        kind: Kind::Manga,
        native: None,
        subtitle: None,
        title: rec.get("title")?.as_str()?.to_string(),
        year: rec
            .get("year")
            .and_then(Value::as_str)
            .and_then(|y| y.trim().parse().ok()),
        status: None,
        rating: rec
            .get("bayesian_rating")
            .and_then(Value::as_f64)
            .map(|r| r as f32),
        url: rec.get("url").and_then(Value::as_str).map(str::to_string),
        cover_url: rec
            .pointer("/image/url/original")
            .and_then(Value::as_str)
            .map(str::to_string),
        genres,
    })
}
