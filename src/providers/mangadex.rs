//! MangaDex public API: only `safe` and `suggestive` content ratings are requested and accepted.

use super::{CatalogProvider, Kind, ProviderError, Source, Work};
use serde_json::Value;

const API: &str = "https://api.mangadex.org";
const ALLOWED_RATINGS: [&str; 2] = ["safe", "suggestive"];

pub struct MangaDex {
    http: reqwest::Client,
}

impl MangaDex {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }
}

impl CatalogProvider for MangaDex {
    fn source(&self) -> Source {
        Source::MangaDex
    }

    async fn search(&self, query: &str) -> Result<Vec<Work>, ProviderError> {
        let resp = self
            .http
            .get(format!("{API}/manga"))
            .query(&[
                ("title", query),
                ("limit", "10"),
                ("includes[]", "cover_art"),
                ("contentRating[]", "safe"),
                ("contentRating[]", "suggestive"),
                ("order[relevance]", "desc"),
            ])
            .timeout(std::time::Duration::from_secs(8))
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(ProviderError::Http(
                crate::http_log::failed("mangadex", "manga search", resp).await,
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
    let data = doc
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderError::Parse("missing data".into()))?;
    Ok(data.iter().filter_map(parse_one).collect())
}

fn parse_one(item: &Value) -> Option<Work> {
    let id = item.get("id")?.as_str()?;
    let attrs = item.get("attributes")?;
    // Anything but an explicitly allowed rating is dropped.
    let rating = attrs.get("contentRating")?.as_str()?;
    if !ALLOWED_RATINGS.contains(&rating) {
        return None;
    }
    let titles = attrs.get("title")?.as_object()?;
    let title = titles
        .get("en")
        .or_else(|| titles.values().next())?
        .as_str()?
        .to_string();
    let genres: Vec<String> = attrs
        .get("tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter(|t| t.pointer("/attributes/group").and_then(Value::as_str) == Some("genre"))
                .filter_map(|t| {
                    t.pointer("/attributes/name/en")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default();
    // The content rating is the verdict here; tags only decorate the card.
    let cover = item
        .get("relationships")
        .and_then(Value::as_array)
        .and_then(|rels| {
            rels.iter()
                .find(|r| r.get("type").and_then(Value::as_str) == Some("cover_art"))
        })
        .and_then(|r| r.pointer("/attributes/fileName").and_then(Value::as_str))
        .map(|f| format!("https://uploads.mangadex.org/covers/{id}/{f}.256.jpg"));
    Some(Work {
        id: format!("mdex:{id}"),
        source: Source::MangaDex,
        kind: Kind::Manga,
        native: None,
        subtitle: None,
        title,
        year: attrs.get("year").and_then(Value::as_i64).map(|y| y as i32),
        status: attrs
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_string),
        rating: None,
        url: Some(format!("https://mangadex.org/title/{id}")),
        cover_url: cover,
        genres: if genres.is_empty() {
            vec!["Unspecified".to_string()]
        } else {
            genres
        },
        work_key: None,
    })
}
