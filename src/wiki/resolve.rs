//! Finding the work a wiki character belongs to, when its button is pressed: first in the same wiki, then in the other
//! sources by flexible name matching. Only a strong match counts.

use super::{clean::series_matches, Kind, Page};
use crate::{anilist::ContentPolicy, tmdb::model as tm, Data};
use std::time::Duration;

/// The whole look-up may take this long (it runs on a button press, after an immediate acknowledgement).
const BUDGET: Duration = Duration::from_millis(3500);

/// The key of the work a wiki character belongs to: the same wiki's page for it, else a work of another source whose
/// title matches the series strongly. `None` when nothing matches (or the page names no series).
pub async fn work_key_for(data: &Data, p: &Page, mature: bool) -> Option<String> {
    let parent = p.parent_title.as_deref()?.trim();
    if parent.is_empty() {
        return None;
    }
    tokio::time::timeout(BUDGET, async {
        // 1. the same wiki, from the cache and then by search
        let now = chrono::Utc::now().timestamp();
        let mut pages = super::store::by_title(
            &data.wikis.pool,
            &super::normalize(super::base_title(parent)),
            None,
            now,
        )
        .await
        .unwrap_or_default();
        pages.extend(
            data.wikis
                .search(super::base_title(parent), None, Some(p.wiki_id))
                .await
                .pages,
        );
        if let Some(own) = pages.iter().find(|g| {
            g.wiki_id == p.wiki_id && g.kind != Kind::Character && series_matches(&g.title, parent)
        }) {
            return Some(own.key());
        }
        // 2. the other sources, all at once, by title
        let (ani, shows, games) = futures::join!(
            async {
                data.anilist
                    .search_any(parent, 5, ContentPolicy::SAFE)
                    .await
                    .unwrap_or_default()
            },
            async {
                let (tv, movie) = futures::join!(
                    data.tmdb.search(tm::Kind::Tv, parent, 1),
                    data.tmdb.search(tm::Kind::Movie, parent, 1)
                );
                tv.into_iter().chain(movie).collect::<Vec<_>>()
            },
            async {
                data.providers
                    .search_games_found(parent, None, mature, None)
                    .await
                    .works
            }
        );
        ani.iter()
            .find(|m| {
                series_matches(m.title.main(), parent)
                    || m.title
                        .english
                        .as_deref()
                        .is_some_and(|e| series_matches(e, parent))
            })
            .map(|m| format!("anilist:{}", m.id))
            .or_else(|| {
                shows
                    .iter()
                    .find(|w| !w.blocked() && series_matches(&w.title, parent))
                    .map(|w| w.key())
            })
            .or_else(|| {
                games
                    .iter()
                    .find(|g| series_matches(&g.title, parent))
                    .map(|g| g.id.clone())
            })
    })
    .await
    .ok()
    .flatten()
}
