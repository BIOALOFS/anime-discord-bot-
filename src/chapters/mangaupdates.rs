//! MangaUpdates release information (keyless, polite: one request at a time with a pause). It reports that a
//! chapter was released by a group; the link points to the MangaUpdates release list for the series.

use super::{
    normalize_number, normalize_title, Chapter, ChapterSource, SourceError, SourceKind, Tracked,
};
use serde_json::{json, Value};
use std::time::Duration;

const API: &str = "https://api.mangaupdates.com/v1";

pub struct MangaUpdates {
    http: reqwest::Client,
    gate: tokio::sync::Mutex<()>,
}

impl MangaUpdates {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            gate: tokio::sync::Mutex::new(()),
        }
    }
}

impl ChapterSource for MangaUpdates {
    async fn fetch(&self, work: &Tracked, _lang: &str) -> Result<Vec<Chapter>, SourceError> {
        // One request at a time, one per second at most.
        let _turn = self.gate.lock().await;
        let Some(title) = work.titles.first() else {
            return Ok(Vec::new());
        };
        let resp = self
            .http
            .post(format!("{API}/releases/search"))
            .json(&json!({"search": title, "stype": "title", "perpage": 15}))
            .timeout(Duration::from_secs(8))
            .send()
            .await
            .map_err(|e| SourceError::Http(e.without_url().to_string()))?;
        tokio::time::sleep(Duration::from_secs(1)).await;
        if !resp.status().is_success() {
            return Err(SourceError::Http(resp.status().to_string()));
        }
        let doc: Value = resp
            .json()
            .await
            .map_err(|e| SourceError::Parse(e.to_string()))?;
        parse_releases(&doc, work)
    }
}

fn parse_date(s: &str) -> Option<i64> {
    chrono::NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|d| d.and_utc().timestamp())
}

/// Releases whose series title equals one of the work's titles.
pub fn parse_releases(doc: &Value, work: &Tracked) -> Result<Vec<Chapter>, SourceError> {
    let results = doc
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| SourceError::Parse("missing results".into()))?;
    let wanted: Vec<String> = work.titles.iter().map(|t| normalize_title(t)).collect();
    Ok(results
        .iter()
        .filter_map(|r| {
            let rec = r.get("record")?;
            let title = rec.get("title")?.as_str()?;
            if !wanted.contains(&normalize_title(title)) {
                return None;
            }
            let number = rec
                .get("chapter")
                .and_then(Value::as_str)
                .and_then(|c| normalize_number(c.split(['-', '+']).next_back().unwrap_or(c)));
            let id = rec.get("id").and_then(Value::as_i64)?;
            Some(Chapter {
                source: SourceKind::MangaUpdates,
                work_id: work.work_id,
                key: number.clone().unwrap_or_else(|| format!("mu:{id}")),
                number,
                title: None,
                url: format!(
                    "https://www.mangaupdates.com/releases?search={}",
                    title.replace(' ', "+")
                ),
                lang: None,
                published: rec
                    .get("release_date")
                    .and_then(Value::as_str)
                    .and_then(parse_date),
            })
        })
        .collect())
}
