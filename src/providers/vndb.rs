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
const VN_FIELDS: &str = "id,title,alttitle,image.url,image.sexual,image.violence,rating,votecount,released,devstatus,description,screenshots.sexual,screenshots.violence,tags.id,tags.name,tags.category,tags.rating";
/// What the verdict of a novel needs (used when characters are vetted through their novels).
const VERDICT_FIELDS: &str = "id,image.sexual,image.violence,screenshots.sexual,screenshots.violence,tags.id,tags.name,tags.category,tags.rating";
const CHARACTER_FIELDS: &str = "id,name,original,image.url,image.sexual,image.violence,vns.id,vns.title,traits.name,traits.group_name";

// ---- request bodies (pure, so the exact JSON is tested) ------------------------------------------------
// Every field and filter below comes from the Kana API documentation (https://api.vndb.org/kana). A field that does
// not exist on an endpoint answers 400 Bad Request: `releases.minage`, for example, belongs to /release, not /vn.

pub(super) fn vn_search_body(query: &str) -> Value {
    json!({"filters": ["search", "=", query], "fields": VN_FIELDS, "results": 20})
}

pub(super) fn vn_by_id_body(id: &str) -> Value {
    json!({"filters": ["id", "=", id], "fields": VN_FIELDS, "results": 1})
}

/// One page of the most voted novels (sorted by votes, so no `search` filter is used with it).
pub(super) fn vn_popular_body(page: u32) -> Value {
    json!({"filters": ["votecount", ">=", 100], "fields": VN_FIELDS, "sort": "votecount", "reverse": true, "results": 20, "page": page.max(1)})
}

/// Novels by id, only the fields the verdict needs (at most 100 results).
pub(super) fn vn_verdict_body(ids: &[String]) -> Value {
    let mut filter = vec![json!("or")];
    filter.extend(ids.iter().take(45).map(|id| json!(["id", "=", id])));
    json!({"filters": filter, "fields": VERDICT_FIELDS, "results": 100})
}

pub(super) fn character_search_body(query: &str) -> Value {
    json!({"filters": ["search", "=", query], "fields": CHARACTER_FIELDS, "results": 20})
}

pub(super) fn character_by_id_body(n: i64) -> Value {
    json!({"filters": ["id", "=", format!("c{n}")], "fields": CHARACTER_FIELDS, "results": 1})
}

/// The characters of one visual novel (`v17`).
pub(super) fn characters_of_vn_body(vn: &str) -> Value {
    json!({"filters": ["vn", "=", ["id", "=", vn]], "fields": CHARACTER_FIELDS, "results": 50})
}

pub(super) fn character_popular_body(page: u32) -> Value {
    json!({"filters": ["id", ">=", "c1"], "fields": CHARACTER_FIELDS, "results": 40, "page": page.max(1)})
}

#[cfg(test)]
pub(super) fn all_bodies_for_tests() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        ("vn", "search", vn_search_body("silent hill")),
        ("vn", "by id", vn_by_id_body("v17")),
        ("vn", "popular", vn_popular_body(3)),
        (
            "vn",
            "verdict",
            vn_verdict_body(&["v1".into(), "v2".into()]),
        ),
        ("character", "search", character_search_body("alice")),
        ("character", "by id", character_by_id_body(5)),
        ("character", "of a novel", characters_of_vn_body("v17")),
        ("character", "popular", character_popular_body(2)),
    ]
}

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
        crate::ops::stats::bump("api", "vndb");
        let resp = self
            .http
            .post(format!("{API}/{endpoint}"))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::USER_AGENT,
                concat!(
                    "anpot-discord-bot/",
                    env!("CARGO_PKG_VERSION"),
                    " (+https://github.com/BIOALOFS/anpot)"
                ),
            )
            .json(&body)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.without_url().to_string()))?;
        if !resp.status().is_success() {
            return Err(ProviderError::Http(
                crate::http_log::failed("vndb", endpoint, resp).await,
            ));
        }
        crate::ops::health::ok("vndb");
        resp.json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))
    }

    pub async fn search_vns(
        &self,
        query: &str,
        allow_mature: bool,
    ) -> Result<Vec<Work>, ProviderError> {
        let doc = self.post("vn", vn_search_body(query)).await?;
        let mut out = parse_vns(&doc, allow_mature);
        out.truncate(10);
        Ok(out)
    }

    /// One page (20) of the most voted visual novels.
    pub async fn popular_vns(
        &self,
        page: u32,
        allow_mature: bool,
    ) -> Result<Vec<Work>, ProviderError> {
        let doc = self.post("vn", vn_popular_body(page)).await?;
        Ok(parse_vns(&doc, allow_mature))
    }

    /// The characters of one visual novel (`v17`).
    pub async fn characters_of(
        &self,
        vn: &str,
        allow_mature: bool,
    ) -> Result<Vec<Work>, ProviderError> {
        let doc = self.post("character", characters_of_vn_body(vn)).await?;
        self.vet_characters(&doc, allow_mature).await
    }

    pub async fn popular_characters(
        &self,
        allow_mature: bool,
        page: u32,
    ) -> Result<Vec<Work>, ProviderError> {
        let doc = self.post("character", character_popular_body(page)).await?;
        self.vet_characters(&doc, allow_mature).await
    }

    pub async fn vn_by_id(
        &self,
        id: &str,
        allow_mature: bool,
    ) -> Result<Option<Work>, ProviderError> {
        let doc = self.post("vn", vn_by_id_body(id)).await?;
        Ok(parse_vns(&doc, allow_mature).into_iter().next())
    }

    pub async fn search_characters(
        &self,
        query: &str,
        allow_mature: bool,
    ) -> Result<Vec<Work>, ProviderError> {
        let doc = self.post("character", character_search_body(query)).await?;
        self.vet_characters(&doc, allow_mature).await
    }

    pub async fn character_by_id(
        &self,
        n: i64,
        allow_mature: bool,
    ) -> Result<Option<Work>, ProviderError> {
        let doc = self.post("character", character_by_id_body(n)).await?;
        Ok(self
            .vet_characters(&doc, allow_mature)
            .await?
            .into_iter()
            .next())
    }

    /// Characters carry no tags, so the visual novels they appear in are checked in one extra request.
    async fn vet_characters(
        &self,
        doc: &Value,
        allow_mature: bool,
    ) -> Result<Vec<Work>, ProviderError> {
        let ids = character_vn_ids(doc);
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let vns = self.post("vn", vn_verdict_body(&ids)).await?;
        Ok(parse_characters(
            doc,
            &vn_verdicts(&vns, allow_mature),
            allow_mature,
        ))
    }
}

impl CatalogProvider for Vndb {
    fn source(&self) -> Source {
        Source::Vndb
    }
    async fn search(&self, query: &str) -> Result<Vec<Work>, ProviderError> {
        self.search_vns(query, true).await
    }
}

fn max_flag(v: &Value, key: &str) -> Option<i64> {
    let mut flags: Vec<i64> = Vec::new();
    flags.extend(v.pointer(&format!("/image/{key}")).and_then(Value::as_i64));
    flags.extend(
        v.get("screenshots")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|x| x.get(key).and_then(Value::as_i64)),
    );
    flags.into_iter().max()
}

fn vn_facts(v: &Value) -> VnFacts {
    let tags = v.get("tags").and_then(Value::as_array);
    let sexual_tag = tags.is_some_and(|t| {
        t.iter().any(|x| {
            safety::is_sexual_tag(
                x.get("category").and_then(Value::as_str),
                x.get("id").and_then(Value::as_str),
                x.get("name").and_then(Value::as_str),
            )
        })
    });
    VnFacts {
        // The cover and every screenshot count: the worst one decides.
        image_sexual: max_flag(v, "sexual"),
        image_violence: max_flag(v, "violence"),
        sexual_tag,
        // An empty tag list is a real answer (nothing sexual is tagged); only a missing list gives no verdict.
        tags_known: tags.is_some(),
    }
}

pub(super) fn parse_vns(doc: &Value, allow_mature: bool) -> Vec<Work> {
    let Some(list) = doc.get("results").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|v| {
            if !safety::vn_allowed(&vn_facts(v), allow_mature) {
                return None;
            }
            let id = v.get("id")?.as_str()?;
            let year = v
                .get("released")
                .and_then(Value::as_str)
                .and_then(|d| d.get(..4))
                .and_then(|y| y.parse().ok());
            Some(Work {
                work_key: None,
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
pub(super) fn vn_verdicts(doc: &Value, allow_mature: bool) -> HashMap<String, bool> {
    doc.get("results")
        .and_then(Value::as_array)
        .map(|l| {
            l.iter()
                .filter_map(|v| {
                    Some((
                        v.get("id")?.as_str()?.to_string(),
                        safety::vn_allowed(&vn_facts(v), allow_mature),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A character is hidden on a sexual trait or picture, or unless every checked appearance is verified safe.
pub(super) fn parse_characters(
    doc: &Value,
    verdicts: &HashMap<String, bool>,
    allow_mature: bool,
) -> Vec<Work> {
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
            let sexual_trait = c.get("traits").and_then(Value::as_array).is_some_and(|t| {
                t.iter().any(|x| {
                    safety::is_sexual_trait(
                        x.get("group_name").and_then(Value::as_str),
                        x.get("name").and_then(Value::as_str),
                    )
                })
            });
            if !safety::vn_character_allowed(
                c.pointer("/image/sexual").and_then(Value::as_i64),
                c.pointer("/image/violence").and_then(Value::as_i64),
                sexual_trait,
                vns.len(),
                allow_mature,
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
                work_key: vns
                    .first()
                    .and_then(|v| v.get("id"))
                    .and_then(Value::as_str)
                    .map(|id| format!("vndb:{id}")),
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
