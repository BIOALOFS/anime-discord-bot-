//! The requests behind people: search across sources (partial results when one fails), a person's card, the crew of
//! a work and the performers of a character. Every document goes through the same safety judgement as works.

use super::{anilist, igdb, rank, tmdb, vndb, CrewEntry, Hit, PSource, Person};
use crate::{anilist::ContentPolicy, providers::Providers, tmdb::model as tm, tmdb::Tmdb, Data};
use std::sync::Arc;

/// The clients a people lookup needs (cheap to clone, so a background lookup can own them).
#[derive(Clone)]
pub struct Sources {
    pub anilist: Arc<crate::anilist::SafeAniList>,
    pub tmdb: Arc<Tmdb>,
    pub providers: Arc<Providers>,
}

impl Sources {
    pub fn of(data: &Data) -> Self {
        Self {
            anilist: data.anilist.clone(),
            tmdb: data.tmdb.clone(),
            providers: data.providers.clone(),
        }
    }
}

/// Which sources a command searches: the category fixes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// `/anime /manga /novel`: AniList staff.
    AnimeManga,
    /// `/drama /series /movie`: TMDB persons.
    Screen,
    /// `/game`: IGDB companies and VNDB staff.
    Games,
    /// `/search` without a category.
    All,
}

impl Scope {
    fn anilist(self) -> bool {
        matches!(self, Self::AnimeManga | Self::All)
    }
    fn tmdb(self) -> bool {
        matches!(self, Self::Screen | Self::All)
    }
    fn games(self) -> bool {
        matches!(self, Self::Games | Self::All)
    }
}

/// Hits plus the sources that did not answer (their results are simply missing).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hits {
    pub hits: Vec<Hit>,
    pub failed: Vec<PSource>,
}

impl Hits {
    /// The Arabic note for a partial answer.
    pub fn partial_note(&self) -> Option<String> {
        if self.failed.is_empty() {
            return None;
        }
        let names: Vec<&str> = self.failed.iter().map(|s| s.credit()).collect();
        Some(format!(
            "⚠️ لم يصل رد من {} الآن، فالنتائج ناقصة.",
            names.join(" و")
        ))
    }
}

/// Search people by name across the sources of `scope`. Safe sources are searched in parallel.
pub async fn search(data: &Sources, scope: Scope, query: &str, mature: bool) -> Hits {
    let _ = mature;
    let q = query.trim();
    let ani = async {
        if !scope.anilist() {
            return None;
        }
        Some(
            data.anilist
                .staff_search_doc(q, 10)
                .await
                .map(|d| d.map(|d| anilist::parse_hits(&d)).unwrap_or_default()),
        )
    };
    let tm = async {
        if !scope.tmdb() || !data.tmdb.enabled() || q.is_empty() {
            return None;
        }
        Some(Ok::<_, ()>(
            data.tmdb
                .person_search_doc(q)
                .await
                .map(|d| tmdb::parse_hits(&d))
                .unwrap_or_default(),
        ))
    };
    let vn = async {
        if !scope.games() || q.is_empty() {
            return None;
        }
        data.providers
            .vndb_doc("staff", vndb::search_body(q))
            .await
            .map(|r| r.map(|d| vndb::parse_hits(&d)))
    };
    let ig = async {
        if !scope.games() || q.is_empty() {
            return None;
        }
        data.providers
            .igdb_doc("companies", igdb::search_body(q))
            .await
            .map(|r| r.map(|d| igdb::parse_hits(&d)))
    };
    let (ani, tm, vn, ig) = futures::join!(ani, tm, vn, ig);
    let mut out = Hits::default();
    let mut take = |source: PSource, r: Option<Result<Vec<Hit>, ()>>| match r {
        Some(Ok(h)) => out.hits.extend(h),
        Some(Err(())) => out.failed.push(source),
        None => {}
    };
    take(
        PSource::AniList,
        ani.map(|r| r.map_err(|e| tracing::debug!("anilist staff search failed: {e}"))),
    );
    take(PSource::Tmdb, tm);
    take(
        PSource::Vndb,
        vn.map(|r| r.map_err(|e| tracing::debug!("vndb staff search failed: {e}"))),
    );
    take(
        PSource::Igdb,
        ig.map(|r| r.map_err(|e| tracing::debug!("igdb company search failed: {e}"))),
    );
    out.hits = rank(out.hits, q, 25);
    out
}

/// What a card request found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Card {
    Found(Box<Person>),
    /// Missing, or hidden by the safety rules (the two look the same on purpose).
    Unavailable,
    /// The source did not answer.
    Down,
}

/// The card of one person by key.
pub async fn person(data: &Sources, key: &str, mature: bool) -> Card {
    let Some((source, id)) = super::parse_key(key) else {
        return Card::Unavailable;
    };
    match source {
        PSource::AniList => match data.anilist.staff_doc(id.parse().unwrap_or(0)).await {
            Ok(Some(d)) => {
                anilist::parse_person(&d).map_or(Card::Unavailable, |p| Card::Found(Box::new(p)))
            }
            Ok(None) => Card::Unavailable,
            Err(e) => {
                tracing::debug!("anilist staff failed: {e}");
                Card::Down
            }
        },
        PSource::Tmdb => {
            if !data.tmdb.enabled() {
                return Card::Unavailable;
            }
            match data.tmdb.person_doc(id.parse().unwrap_or(0)).await {
                Some(d) => {
                    tmdb::parse_person(&d).map_or(Card::Unavailable, |p| Card::Found(Box::new(p)))
                }
                None => Card::Unavailable,
            }
        }
        PSource::Vndb => {
            let (staff, works) = futures::join!(
                data.providers.vndb_doc("staff", vndb::staff_body(&id)),
                data.providers.vndb_doc("vn", vndb::works_body(&id))
            );
            let (Some(Ok(staff)), Some(Ok(works))) = (staff, works) else {
                return Card::Down;
            };
            let raw = works
                .get("results")
                .and_then(|r| r.as_array())
                .map_or(0, Vec::len);
            let allowed = crate::providers::vndb::parse_vns(&works, mature);
            vndb::parse_person(&staff, raw, &allowed)
                .map_or(Card::Unavailable, |p| Card::Found(Box::new(p)))
        }
        PSource::Igdb => match data
            .providers
            .igdb_doc("companies", igdb::company_body(id.parse().unwrap_or(0)))
            .await
        {
            Some(Ok(d)) => igdb::parse_company(&d, mature)
                .map_or(Card::Unavailable, |p| Card::Found(Box::new(p))),
            Some(Err(e)) => {
                tracing::debug!("igdb company failed: {e}");
                Card::Down
            }
            None => Card::Unavailable,
        },
    }
}

/// The crew of a work by key. `None` when the source did not answer; an empty list when the work is hidden or has none.
pub async fn crew(data: &Sources, work_key: &str, mature: bool) -> Option<Vec<CrewEntry>> {
    if let Some(id) = work_key
        .strip_prefix("anilist:")
        .and_then(|n| n.parse::<i64>().ok())
    {
        let doc = data.anilist.media_crew_doc(id).await.ok()?;
        // The work itself must pass the shared policy as well.
        if !matches!(
            data.anilist.media_by_id(id, ContentPolicy::SAFE).await,
            Ok(Some(_))
        ) {
            return Some(Vec::new());
        }
        return Some(doc.map(|d| anilist::parse_crew(&d)).unwrap_or_default());
    }
    if let Some((kind, id)) = tm::parse_work_key(work_key) {
        // `details` hides blocked works.
        data.tmdb.details(kind, id).await?;
        let doc = data.tmdb.credits_doc(kind, id).await?;
        return Some(tmdb::parse_crew(&doc));
    }
    if let Some(rest) = work_key.strip_prefix("vndb:") {
        data.providers.work_by_key(work_key, mature).await?;
        let doc = data
            .providers
            .vndb_doc("vn", vndb::crew_body(rest))
            .await?
            .ok()?;
        return Some(vndb::parse_crew(&doc));
    }
    if let Some(id) = work_key
        .strip_prefix("igdb:")
        .and_then(|n| n.parse::<i64>().ok())
    {
        data.providers.work_by_key(work_key, mature).await?;
        let doc = data
            .providers
            .igdb_doc("games", igdb::crew_body(id))
            .await?
            .ok()?;
        return Some(igdb::parse_crew(&doc));
    }
    Some(Vec::new())
}

/// The performer(s) of a character by key (an AniList id, a `tmdb-cast:` key or a `vndb:c` key).
pub async fn performers(data: &Sources, character_key: &str, mature: bool) -> Vec<Hit> {
    if let Ok(id) = character_key.parse::<i64>() {
        return match data.anilist.character_performers_doc(id).await {
            Ok(Some(d)) => anilist::parse_performers(&d),
            _ => Vec::new(),
        };
    }
    if let Some((kind, work, person)) = tm::parse_cast_key(character_key) {
        let Some(w) = data.tmdb.details(kind, work).await else {
            return Vec::new();
        };
        return w
            .cast
            .iter()
            .filter(|c| c.person_id == person)
            .map(|c| Hit {
                key: PSource::Tmdb.key(c.person_id),
                name: c.actor.clone(),
                native: None,
                hint: "ممثل".into(),
                popularity: c.popularity.map_or(0, |p| (p * 100.0) as i64),
            })
            .collect();
    }
    if let Some(c) = character_key
        .strip_prefix("vndb:")
        .filter(|c| c.starts_with('c'))
    {
        let Some(Ok(chars)) = data
            .providers
            .vndb_doc("character", vndb::character_vns_body(c))
            .await
        else {
            return Vec::new();
        };
        for vn in vndb::character_vns(&chars) {
            // The novel itself must be allowed before anything about it is listed.
            if data
                .providers
                .work_by_key(&format!("vndb:{vn}"), mature)
                .await
                .is_none()
            {
                continue;
            }
            if let Some(Ok(doc)) = data.providers.vndb_doc("vn", vndb::crew_body(&vn)).await {
                let found = vndb::parse_performers(&doc, c);
                if !found.is_empty() {
                    return found;
                }
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_pick_their_sources() {
        assert!(
            Scope::AnimeManga.anilist() && !Scope::AnimeManga.tmdb() && !Scope::AnimeManga.games()
        );
        assert!(Scope::Screen.tmdb() && !Scope::Screen.anilist());
        assert!(Scope::Games.games() && !Scope::Games.tmdb());
        assert!(Scope::All.anilist() && Scope::All.tmdb() && Scope::All.games());
    }

    #[test]
    fn a_partial_answer_says_who_is_missing() {
        let h = Hits {
            hits: vec![],
            failed: vec![PSource::Vndb],
        };
        assert!(h.partial_note().is_some_and(|n| n.contains("VNDB")));
        assert!(Hits::default().partial_note().is_none());
    }
}
