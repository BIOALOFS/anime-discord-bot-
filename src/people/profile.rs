//! People inside the profile. A person is a character there: the same list, the same controls, an id in the
//! character tables (see `PERSON_ID_BASE`) and a cache row for the name, picture and role line.

use super::{encode_person_id, role_line, Person};
use crate::db::profile::{self as pdb, AddOutcome, CacheRow};
use sqlx::SqlitePool;

/// Pictures a profile may show for a person (the hosts that allow embedding).
pub fn image_ok(url: &str) -> bool {
    url.starts_with("https://s4.anilist.co/") || url.starts_with("https://image.tmdb.org/")
}

/// The cache row of a person: the role line plays the part of the main work.
pub fn cache_row(p: &Person, id: i64, now: i64) -> CacheRow {
    CacheRow {
        character_id: id,
        name_romaji: p.name.clone(),
        name_native: p.native.clone(),
        image_url: p.image.clone().filter(|u| image_ok(u)),
        is_safe: true,
        main_work: Some(role_line(&p.roles)),
        main_work_url: p.url.clone(),
        favourites: p.popularity.filter(|n| *n > 0),
        fetched_at: now,
    }
}

/// Add a person (already judged by the safety rules when it was fetched) to a member's profile. `None` when the
/// person cannot be added (an IGDB company).
pub async fn add(
    pool: &SqlitePool,
    user: i64,
    p: &Person,
    now: i64,
) -> Result<Option<AddOutcome>, pdb::ProfileError> {
    let Some(id) = encode_person_id(&p.key) else {
        return Ok(None);
    };
    pdb::create_profile(pool, user).await?;
    pdb::upsert_cache(pool, &cache_row(p, id, now)).await?;
    Ok(Some(pdb::add_character(pool, user, id).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(key: &str, roles: &[&str]) -> Person {
        Person {
            key: key.into(),
            name: "Hiroshi Kamiya".into(),
            native: Some("神谷浩史".into()),
            roles: roles.iter().map(|r| r.to_string()).collect(),
            popularity: Some(1200),
            image: Some("https://s4.anilist.co/x.png".into()),
            url: Some("https://anilist.co/staff/95".into()),
            ..Person::default()
        }
    }

    #[tokio::test]
    async fn each_provider_person_is_added_listed_and_removed_like_a_character() {
        let pool = crate::db::connect_memory().await;
        for (n, key) in ["anilist-staff:95", "tmdb-person:17419", "vndb-staff:s42"]
            .iter()
            .enumerate()
        {
            let p = person(key, &["مؤدي أصوات"]);
            assert_eq!(
                add(&pool, 1, &p, 100).await.expect("add"),
                Some(AddOutcome::Added(n as i64 + 1))
            );
            assert_eq!(
                add(&pool, 1, &p, 100).await.expect("again"),
                Some(AddOutcome::Duplicate),
                "same limits as characters"
            );
        }
        // they are in the characters list (one list, positions 1..3), with the role line where the work would be
        let page = pdb::list_page(&pool, 1, None, 0, 10).await.expect("list");
        assert_eq!(page.len(), 3);
        assert!(page.iter().all(|e| e.visible()));
        assert_eq!(page[0].main_work.as_deref(), Some("شخص حقيقي · مؤدي أصوات"));
        assert_eq!(page[0].name(), "Hiroshi Kamiya");
        assert_eq!(
            pdb::count(&pool, 1).await.expect("count"),
            3,
            "the characters count includes people"
        );
        // the autocomplete of /edit character finds them
        assert_eq!(
            pdb::search_own(&pool, 1, "kamiya", 10)
                .await
                .expect("search")
                .len(),
            3
        );
        // and they can be removed
        let id = encode_person_id("tmdb-person:17419").expect("id");
        assert!(pdb::remove_character(&pool, 1, id).await.expect("remove"));
        assert_eq!(pdb::count(&pool, 1).await.expect("count"), 2);
    }

    #[tokio::test]
    async fn a_company_is_not_addable_and_other_members_are_untouched() {
        let pool = crate::db::connect_memory().await;
        let c = person("igdb-company:70", &["شركة"]);
        assert_eq!(add(&pool, 1, &c, 1).await.expect("add"), None);
        assert_eq!(pdb::count(&pool, 1).await.unwrap_or(0), 0);
        pdb::create_profile(&pool, 2).await.expect("profile");
        add(&pool, 2, &person("anilist-staff:1", &[]), 1)
            .await
            .expect("add");
        assert_eq!(pdb::count(&pool, 2).await.expect("count"), 1);
    }

    #[test]
    fn only_embeddable_pictures_are_kept() {
        let mut p = person("vndb-staff:s1", &[]);
        p.image = Some("https://example.org/x.png".into());
        assert_eq!(cache_row(&p, 5, 1).image_url, None);
        p.image = Some("https://image.tmdb.org/t/p/w500/a.jpg".into());
        assert!(cache_row(&p, 5, 1).image_url.is_some());
        assert_eq!(cache_row(&p, 5, 1).main_work.as_deref(), Some("شخص حقيقي"));
    }
}
