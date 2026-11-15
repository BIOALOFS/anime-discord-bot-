//! VNDB HTTP API v2 ("Kana", no key). Limits: ~200 requests per 5 minutes, 1 s of execution time per minute,
//! and requests running over 3 s are aborted, so requests are slow-paced and results are cached by the caller.
//! The card footer credits "Data from VNDB"; the data is under the VNDB data license (vndb.org/d17).

use super::safety::{self, VnFacts};
use super::{CatalogProvider, Kind, ProviderError, Source, Work};
use governor::{clock::DefaultClock, state::InMemoryState, state::NotKeyed, Quota, RateLimiter};
use serde_json::{json, Value};
use std::{collections::HashMap, time::Duration};
use tokio::sync::Semaphore;

const API: &str = "https://api.vndb.org/kana";
const VN_FIELDS: &str =
    "title,alttitle,released,rating,devstatus,image.url,image.sexual,tags.category,tags.rating";

type Limiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

pub struct Vndb {
    http: reqwest::Client,
    limit: Limiter,
    open: Semaphore,
}

impl Vndb {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            // One request per 1.5 s = 200 per 5 minutes.
            limit: RateLimiter::direct(
                Quota::with_period(Duration::from_millis(1500))
                    .unwrap_or_else(|| Quota::per_minute(std::num::NonZeroU32::MIN)),
            ),
            open: Semaphore::new(2),
        }
    }

    async fn post(&self, endpoint: &str, body: Value) -> Result<Value, ProviderError> {
        let _open = self
            .open
            .acquire()
            .await
            .map_err(|_| ProviderError::NotConfigured)?;
        self.limit.until_ready().await;
        let resp = self
            .http
            .post(format!("{API}/{endpoint}"))
            .json(&body)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.without_url().to_string()))?;
        if !resp.status().is_success() {
            return Err(ProviderError::Http(resp.status().to_string()));
        }
        resp.json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))
    }

    pub async fn search_vns(&self, query: &str) -> Result<Vec<Work>, ProviderError> {
        let doc = self
            .post("vn", json!({"filters": ["search", "=", query], "fields": VN_FIELDS, "sort": "searchrank", "results": 15}))
            .await?;
        let mut out = parse_vns(&doc);
        out.truncate(10);
        Ok(out)
    }

    pub async fn search_characters(&self, query: &str) -> Result<Vec<Work>, ProviderError> {
        let doc = self
            .post(
                "character",
                character_request(json!(["search", "=", query]), 12),
            )
            .await?;
        self.vet_characters(&doc).await
    }

    pub async fn character_by_id(&self, n: i64) -> Result<Option<Work>, ProviderError> {
        let doc = self
            .post(
                "character",
                character_request(json!(["id", "=", format!("c{n}")]), 1),
            )
            .await?;
        Ok(self.vet_characters(&doc).await?.into_iter().next())
    }

    /// Characters carry no tags, so the visual novels they appear in are checked in one extra request.
    async fn vet_characters(&self, doc: &Value) -> Result<Vec<Work>, ProviderError> {
        let ids = character_vn_ids(doc);
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut filter = vec![json!("or")];
        filter.extend(ids.iter().map(|id| json!(["id", "=", id])));
        let vns = self.post("vn", json!({"filters": filter, "fields": "image.sexual,tags.category,tags.rating", "results": 100})).await?;
        Ok(parse_characters(doc, &vn_verdicts(&vns)))
    }
}

impl CatalogProvider for Vndb {
    fn source(&self) -> Source {
        Source::Vndb
    }
    async fn search(&self, query: &str) -> Result<Vec<Work>, ProviderError> {
        self.search_vns(query).await
    }
}

fn character_request(filter: Value, results: u32) -> Value {
    json!({"filters": filter, "fields": "name,original,image.url,image.sexual,vns.id,vns.title", "results": results})
}

fn vn_facts(v: &Value) -> VnFacts {
    let tags = v.get("tags").and_then(Value::as_array);
    VnFacts {
        image_sexual: v.pointer("/image/sexual").and_then(Value::as_i64),
        // Any content tag in the "ero" category counts, whatever its score.
        sexual_tag: tags.is_some_and(|t| {
            t.iter()
                .any(|x| x.get("category").and_then(Value::as_str) == Some("ero"))
        }),
        // An empty or absent tag list gives no way to judge the novel.
        tags_known: tags.is_some_and(|t| !t.is_empty()),
    }
}

pub(super) fn parse_vns(doc: &Value) -> Vec<Work> {
    let Some(list) = doc.get("results").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|v| {
            if !safety::vn_allowed(&vn_facts(v)) {
                return None;
            }
            let id = v.get("id")?.as_str()?;
            let year = v
                .get("released")
                .and_then(Value::as_str)
                .and_then(|d| d.get(..4))
                .and_then(|y| y.parse().ok());
            Some(Work {
                id: format!("vndb:{id}"),
                source: Source::Vndb,
                kind: Kind::VisualNovel,
                title: v.get("title")?.as_str()?.to_string(),
                native: v
                    .get("alttitle")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                subtitle: None,
                year,
                status: v.get("devstatus").and_then(Value::as_i64).map(|s| {
                    ["finished", "in development", "cancelled"]
                        .get(s as usize)
                        .copied()
                        .unwrap_or("finished")
                        .to_string()
                }),
                rating: v
                    .get("rating")
                    .and_then(Value::as_f64)
                    .map(|r| (r / 10.0) as f32),
                url: Some(format!("https://vndb.org/{id}")),
                cover_url: v
                    .pointer("/image/url")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                genres: Vec::new(),
            })
        })
        .collect()
}

/// Up to three visual novels per character, deduplicated, for the verdict query.
fn character_vn_ids(doc: &Value) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for c in doc
        .get("results")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        for vn in c
            .get("vns")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .take(3)
        {
            if let Some(id) = vn.get("id").and_then(Value::as_str) {
                if !ids.iter().any(|x| x == id) {
                    ids.push(id.to_string());
                }
            }
        }
    }
    ids.truncate(45);
    ids
}

/// `vn id -> allowed`.
pub(super) fn vn_verdicts(doc: &Value) -> HashMap<String, bool> {
    doc.get("results")
        .and_then(Value::as_array)
        .map(|l| {
            l.iter()
                .filter_map(|v| {
                    Some((
                        v.get("id")?.as_str()?.to_string(),
                        safety::vn_allowed(&vn_facts(v)),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A character is hidden unless it has a safe picture and every checked appearance is verified safe.
pub(super) fn parse_characters(doc: &Value, verdicts: &HashMap<String, bool>) -> Vec<Work> {
    let Some(list) = doc.get("results").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|c| {
            let vns = c
                .get("vns")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if !safety::vn_character_allowed(
                c.pointer("/image/sexual").and_then(Value::as_i64),
                vns.len(),
            ) {
                return None;
            }
            let checked: Vec<&str> = vns
                .iter()
                .take(3)
                .filter_map(|v| v.get("id").and_then(Value::as_str))
                .collect();
            if checked.is_empty()
                || !checked
                    .iter()
                    .all(|id| verdicts.get(*id).copied().unwrap_or(false))
            {
                return None;
            }
            let id = c.get("id")?.as_str()?;
            Some(Work {
                id: format!("vndb:{id}"),
                source: Source::Vndb,
                kind: Kind::Character,
                title: c.get("name")?.as_str()?.to_string(),
                native: c
                    .get("original")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                subtitle: vns
                    .first()
                    .and_then(|v| v.get("title"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                year: None,
                status: None,
                rating: None,
                url: Some(format!("https://vndb.org/{id}")),
                cover_url: c
                    .pointer("/image/url")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                genres: Vec::new(),
            })
        })
        .collect()
}
