//! Wiki pages inside profiles. A wiki character is a character there (an integer id above 4×10^15 in the character
//! tables), a wiki game or other page is a work with the key `wiki:{wiki}:{page}`. A snapshot (name, picture, summary,
//! series, source, address, license) is kept so that the item outlives the cache, the wiki being disabled and the wiki
//! being deleted. Member edits are separate columns of the profile rows and are never touched by any of this.

use super::{Kind, Page, Source};
use crate::db::{
    profile::{self as pdb, AddOutcome, CacheRow},
    works,
};
use sqlx::{Row, SqlitePool};

/// Wiki characters live in the character tables above this base: clear of AniList ids, provider characters, TMDB cast
/// entries (below 3.4×10^14 above 3×10^12) and people (below 4×10^15).
pub const ID_BASE: i64 = 4_000_000_000_000_000;

/// The integer id of a wiki character (21 bits of wiki, 31 bits of page: far below SQLite's limit).
pub fn encode_id(wiki: i64, page: i64) -> Option<i64> {
    ((1..1 << 20).contains(&wiki) && (1..1 << 31).contains(&page))
        .then(|| ID_BASE + ((wiki << 31) | page))
}

pub fn decode_id(id: i64) -> Option<(i64, i64)> {
    let n = id
        .checked_sub(ID_BASE)
        .filter(|n| (0..1 << 51).contains(n))?;
    Some((n >> 31, n & ((1 << 31) - 1))).filter(|(w, p)| *w > 0 && *p > 0)
}

/// The key a stored wiki character id opens.
pub fn key_of_id(id: i64) -> Option<String> {
    decode_id(id).map(|(w, p)| super::key(w, p))
}

/// What was saved of a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub wiki_id: i64,
    pub page_id: i64,
    pub kind: Kind,
    pub name: String,
    pub image_url: Option<String>,
    pub summary: Option<String>,
    pub parent: Option<String>,
    pub source_name: String,
    pub page_url: String,
    pub license_name: Option<String>,
    pub license_url: Option<String>,
}

impl Snapshot {
    pub fn of(p: &Page, w: &Source) -> Self {
        Self {
            wiki_id: p.wiki_id,
            page_id: p.page_id,
            kind: p.kind,
            name: p.title.clone(),
            image_url: p
                .image_url
                .clone()
                .filter(|u| super::safety::image_url_ok(u)),
            summary: p.extract.clone(),
            parent: p.parent_title.clone(),
            source_name: w.name.clone(),
            page_url: super::msgs::page_url(&w.host, &p.title),
            license_name: w.license_name.clone(),
            license_url: w.license_url.clone(),
        }
    }
}

pub async fn save_snapshot(pool: &SqlitePool, s: &Snapshot, now: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO wiki_snapshots (wiki_id, page_id, kind, name, image_url, summary, parent, source_name, page_url, license_name, license_url, saved_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
         ON CONFLICT(wiki_id, page_id) DO UPDATE SET kind = excluded.kind, name = excluded.name, image_url = excluded.image_url, \
         summary = excluded.summary, parent = excluded.parent, source_name = excluded.source_name, page_url = excluded.page_url, \
         license_name = excluded.license_name, license_url = excluded.license_url, saved_at = excluded.saved_at",
    )
    .bind(s.wiki_id)
    .bind(s.page_id)
    .bind(s.kind.key())
    .bind(&s.name)
    .bind(&s.image_url)
    .bind(&s.summary)
    .bind(&s.parent)
    .bind(&s.source_name)
    .bind(&s.page_url)
    .bind(&s.license_name)
    .bind(&s.license_url)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn snapshot(pool: &SqlitePool, wiki: i64, page: i64) -> Option<Snapshot> {
    let r = sqlx::query("SELECT kind, name, image_url, summary, parent, source_name, page_url, license_name, license_url FROM wiki_snapshots WHERE wiki_id = ?1 AND page_id = ?2")
        .bind(wiki)
        .bind(page)
        .fetch_optional(pool)
        .await
        .ok()??;
    Some(Snapshot {
        wiki_id: wiki,
        page_id: page,
        kind: Kind::from_key(&r.get::<String, _>("kind")),
        name: r.get("name"),
        image_url: r.get("image_url"),
        summary: r.get("summary"),
        parent: r.get("parent"),
        source_name: r.get("source_name"),
        page_url: r.get("page_url"),
        license_name: r.get("license_name"),
        license_url: r.get("license_url"),
    })
}

/// Is this page a character (it goes to the characters list) or a work?
pub fn is_character(p: &Page) -> bool {
    p.kind == Kind::Character
}

/// Add a wiki character to a member's profile (with its snapshot). A duplicate is reported, not added twice.
pub async fn add_character(
    pool: &SqlitePool,
    user: i64,
    p: &Page,
    w: &Source,
    now: i64,
) -> Result<Option<AddOutcome>, pdb::ProfileError> {
    let Some(id) = encode_id(p.wiki_id, p.page_id) else {
        return Ok(None);
    };
    let snap = Snapshot::of(p, w);
    save_snapshot(pool, &snap, now).await?;
    pdb::create_profile(pool, user).await?;
    pdb::upsert_cache(
        pool,
        &CacheRow {
            character_id: id,
            name_romaji: snap.name.clone(),
            name_native: None,
            image_url: snap.image_url.clone(),
            is_safe: true,
            main_work: snap.parent.clone(),
            main_work_url: Some(snap.page_url.clone()),
            favourites: None,
            fetched_at: now,
        },
    )
    .await?;
    Ok(Some(pdb::add_character(pool, user, id).await?))
}

/// Add a wiki game (or other page) as a work.
pub async fn add_work(
    pool: &SqlitePool,
    user: i64,
    p: &Page,
    w: &Source,
    now: i64,
) -> Result<AddOutcome, pdb::ProfileError> {
    let key = p.key();
    let snap = Snapshot::of(p, w);
    save_snapshot(pool, &snap, now).await?;
    pdb::create_profile(pool, user).await?;
    works::upsert_media_cache(
        pool,
        &works::MediaCacheRow {
            work_id: key.clone(),
            title_romaji: snap.name.clone(),
            title_english: None,
            cover_url: snap.image_url.clone(),
            kind: "wiki".into(),
            format: None,
            country: None,
            year: None,
            score: None,
            status: None,
            is_safe: true,
            source: "wiki".into(),
            fetched_at: now,
        },
    )
    .await?;
    works::add_work(pool, user, &key, "wiki").await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki::store;

    fn source(id: i64) -> Source {
        Source {
            id,
            name: "Mudae Wiki".into(),
            host: "mudae.fandom.com".into(),
            api_url: "https://mudae.fandom.com/api.php".into(),
            lang: "en".into(),
            license_name: Some("CC BY-SA 3.0".into()),
            license_url: Some("https://creativecommons.org/licenses/by-sa/3.0/".into()),
            character_category: Some("Category:Characters".into()),
            game_category: None,
            enabled: true,
            added_by: None,
            created_at: 0,
            last_checked_at: None,
            last_error: None,
        }
    }

    fn page(wiki: i64, id: i64, title: &str, kind: Kind) -> Page {
        Page {
            wiki_id: wiki,
            page_id: id,
            title: title.into(),
            kind,
            extract: Some("She is the trial captain.".into()),
            image_url: Some(
                "https://static.wikia.nocookie.net/mudae/images/a/a1/Acerola.png/revision/latest"
                    .into(),
            ),
            aliases: vec![],
            parent_title: Some("Pokémon Sun/Moon".into()),
            categories: vec![],
        }
    }

    #[test]
    fn wiki_ids_are_stable_round_trip_and_clear_of_every_other_character_id() {
        let id = encode_id(3, 12_345).expect("id");
        assert_eq!(decode_id(id), Some((3, 12_345)));
        assert_eq!(key_of_id(id).as_deref(), Some("wiki:3:12345"));
        assert_eq!(encode_id(3, 12_345), Some(id), "stable");
        let top = encode_id((1 << 20) - 1, (1 << 31) - 1).expect("largest");
        assert!(top < i64::MAX / 100);
        // below: people (below 4e15), cast entries, provider characters, AniList ids
        let person = crate::people::encode_person_id("vndb-staff:s999999").expect("person");
        for other in [
            person,
            1234,
            1_000_000_000_042,
            2_000_000_000_007,
            crate::tmdb::model::encode_cast_id(
                crate::tmdb::model::Kind::Movie,
                (1 << 22) - 1,
                (1 << 25) - 1,
            )
            .expect("cast"),
        ] {
            assert!(other < ID_BASE, "{other}");
            assert_eq!(decode_id(other), None);
        }
        assert_eq!(encode_id(0, 5), None);
        assert_eq!(encode_id(5, 0), None);
        assert_eq!(encode_id(1 << 20, 5), None);
    }

    #[tokio::test]
    async fn a_wiki_character_is_added_listed_snapshotted_and_removed_like_any_character() {
        let pool = crate::db::connect_memory().await;
        let (p, w) = (page(2, 77, "Acerola", Kind::Character), source(2));
        assert_eq!(
            add_character(&pool, 1, &p, &w, 100).await.expect("add"),
            Some(AddOutcome::Added(1))
        );
        assert_eq!(
            add_character(&pool, 1, &p, &w, 101).await.expect("again"),
            Some(AddOutcome::Duplicate)
        );
        let list = pdb::list_page(&pool, 1, None, 0, 10).await.expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(
            (list[0].name(), list[0].main_work.as_deref()),
            ("Acerola", Some("Pokémon Sun/Moon"))
        );
        assert!(list[0].visible());
        let id = encode_id(2, 77).expect("id");
        assert_eq!(list[0].character_id, id);
        assert_eq!(pdb::count(&pool, 1).await.expect("count"), 1);
        assert!(pdb::remove_character(&pool, 1, id).await.expect("remove"));
        assert_eq!(pdb::count(&pool, 1).await.expect("count"), 0);
    }

    #[tokio::test]
    async fn the_snapshot_keeps_everything_the_item_needs_and_survives_the_wiki() {
        let pool = crate::db::connect_memory().await;
        let id = match store::insert(
            &pool,
            &store::NewSource {
                name: "Mudae Wiki".into(),
                host: "mudae.fandom.com".into(),
                api_url: "https://mudae.fandom.com/api.php".into(),
                lang: "en".into(),
                license_name: Some("CC BY-SA 3.0".into()),
                license_url: None,
                character_category: None,
                game_category: None,
                added_by: None,
            },
        )
        .await
        .expect("insert")
        {
            store::Added::New(id) => id,
            other => panic!("{other:?}"),
        };
        let (p, w) = (page(id, 77, "Acerola", Kind::Character), source(id));
        add_character(&pool, 1, &p, &w, 100).await.expect("add");
        let before = crate::db::user_data_counts(&pool).await.expect("counts");
        // the wiki is disabled, then deleted, and its cache emptied
        store::set_enabled(&pool, id, false).await.expect("off");
        store::delete(&pool, id).await.expect("delete");
        store::clear_cache(&pool).await.expect("clear");
        let after = crate::db::user_data_counts(&pool).await.expect("counts");
        for (table, n) in &before {
            if table != "wiki_sources" {
                assert_eq!(after.get(table), Some(n), "{table} is untouched");
            }
        }
        let s = snapshot(&pool, id, 77)
            .await
            .expect("the snapshot is still there");
        assert_eq!(
            (s.name.as_str(), s.parent.as_deref(), s.source_name.as_str()),
            ("Acerola", Some("Pokémon Sun/Moon"), "Mudae Wiki")
        );
        assert_eq!(s.summary.as_deref(), Some("She is the trial captain."));
        assert_eq!(s.page_url, "https://mudae.fandom.com/wiki/Acerola");
        assert_eq!(s.license_name.as_deref(), Some("CC BY-SA 3.0"));
        assert!(s.image_url.is_some());
        // and the profile item is still listed
        assert_eq!(pdb::count(&pool, 1).await.expect("count"), 1);
        assert_eq!(
            pdb::list_page(&pool, 1, None, 0, 5).await.expect("list")[0].name(),
            "Acerola"
        );
    }

    #[tokio::test]
    async fn a_wiki_game_is_a_work_with_the_stable_key_and_a_cache_row() {
        let pool = crate::db::connect_memory().await;
        let (p, w) = (page(2, 5, "Silent Hill 2", Kind::Game), source(2));
        assert_eq!(
            add_work(&pool, 1, &p, &w, 10).await.expect("add"),
            AddOutcome::Added(1)
        );
        assert_eq!(
            add_work(&pool, 1, &p, &w, 11).await.expect("again"),
            AddOutcome::Duplicate
        );
        let list = works::list_page(&pool, 1, None, None, 0, 10)
            .await
            .expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].work_id, "wiki:2:5");
        assert_eq!(list[0].title(), "Silent Hill 2");
        assert!(list[0].visible());
        assert!(works::remove_work(&pool, 1, "wiki:2:5")
            .await
            .expect("remove"));
    }

    #[tokio::test]
    async fn the_profile_stays_paginated_with_far_more_than_a_hundred_wiki_items() {
        let pool = crate::db::connect_memory().await;
        let w = source(2);
        for i in 1..=130 {
            let p = page(2, i, &format!("Character {i}"), Kind::Character);
            add_character(&pool, 1, &p, &w, 10).await.expect("add");
        }
        assert_eq!(
            pdb::count(&pool, 1).await.expect("count"),
            130,
            "no limit on the number of items"
        );
        let first = pdb::list_page(&pool, 1, None, 0, 25).await.expect("page");
        let last = pdb::list_page(&pool, 1, None, 125, 25).await.expect("page");
        assert_eq!((first.len(), last.len()), (25, 5), "read 25 at a time");
        assert_eq!(first[0].name(), "Character 1");
        assert_eq!(last[4].name(), "Character 130");
    }

    #[tokio::test]
    async fn member_edits_are_not_touched_when_the_page_is_added_again_or_the_cache_is_refreshed() {
        let pool = crate::db::connect_memory().await;
        let w = source(2);
        let mut p = page(2, 77, "Acerola", Kind::Character);
        add_character(&pool, 1, &p, &w, 100).await.expect("add");
        let id = encode_id(2, 77).expect("id");
        crate::db::custom::set_title(&pool, 1, id, Some("Captain"))
            .await
            .expect("nickname");
        crate::db::custom::set_description(&pool, 1, id, Some("My favourite"))
            .await
            .expect("description");
        // the wiki changed the page, and the same page is refreshed into the cache (a second add attempt)
        p.title = "Acerola (Pokémon)".into();
        p.extract = Some("Rewritten.".into());
        add_character(&pool, 1, &p, &w, 200).await.expect("refresh");
        let entry = pdb::get_entry(&pool, 1, id)
            .await
            .expect("entry")
            .expect("row");
        assert_eq!(entry.custom_title.as_deref(), Some("Captain"));
        assert_eq!(entry.custom_description.as_deref(), Some("My favourite"));
        assert_eq!(
            entry.name(),
            "Acerola (Pokémon)",
            "the source data follows the wiki"
        );
    }
}
