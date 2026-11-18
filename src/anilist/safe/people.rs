//! The AniList requests behind the people cards: staff search, a person with their works, the crew of a work and the
//! voice actors of a character. They return the raw documents; `crate::people::anilist` parses and judges them.

use super::SafeAniList;
use crate::anilist::{client::Which, AniError};
use serde_json::{json, Value};
use std::sync::Arc;

impl SafeAniList {
    /// Staff by name, or the most-loved staff when `query` is empty.
    pub async fn staff_search_doc(
        &self,
        query: &str,
        per_page: u8,
    ) -> Result<Option<Arc<Value>>, AniError> {
        let q = query.trim();
        if q.is_empty() {
            return self
                .client
                .get(
                    Which::Search,
                    format!("pst:{per_page}"),
                    crate::people::anilist::POPULAR_STAFF,
                    json!({"perPage": per_page.min(10)}),
                )
                .await;
        }
        self.client
            .get(
                Which::Search,
                format!("ss:{}:{per_page}", q.to_lowercase()),
                crate::people::anilist::STAFF_SEARCH,
                json!({"search": q, "perPage": per_page.min(10)}),
            )
            .await
    }

    pub async fn staff_doc(&self, id: i64) -> Result<Option<Arc<Value>>, AniError> {
        self.client
            .get(
                Which::Details,
                format!("st:{id}"),
                crate::people::anilist::STAFF_BY_ID,
                json!({ "id": id }),
            )
            .await
    }

    pub async fn media_crew_doc(&self, id: i64) -> Result<Option<Arc<Value>>, AniError> {
        self.client
            .get(
                Which::Details,
                format!("mcrew:{id}"),
                crate::people::anilist::MEDIA_CREW,
                json!({ "id": id }),
            )
            .await
    }

    pub async fn character_performers_doc(&self, id: i64) -> Result<Option<Arc<Value>>, AniError> {
        self.client
            .get(
                Which::Details,
                format!("cperf:{id}"),
                crate::people::anilist::CHARACTER_PERFORMERS,
                json!({ "id": id }),
            )
            .await
    }
}
