use super::*;
use crate::db::{connect_memory, profile};

const U: i64 = 1;
const V: i64 = 2;

async fn setup() -> SqlitePool {
    let pool = connect_memory().await;
    profile::create_profile(&pool, U).await.expect("profile");
    pool
}

fn cache(id: &str, safe: bool) -> MediaCacheRow {
    MediaCacheRow {
        work_id: id.into(),
        title_romaji: format!("Title {id}"),
        title_english: None,
        cover_url: Some("https://s4.anilist.co/c.png".into()),
        kind: "anime".into(),
        format: Some("TV".into()),
        country: Some("JP".into()),
        year: Some(2020),
        score: Some(80),
        status: Some("FINISHED".into()),
        is_safe: safe,
        source: "anilist".into(),
        fetched_at: 1,
    }
}

async fn positions(pool: &SqlitePool, user: i64) -> Vec<i64> {
    sqlx::query_scalar("SELECT position FROM profile_works WHERE user_id = ?1 ORDER BY position")
        .bind(user)
        .fetch_all(pool)
        .await
        .expect("rows")
}

#[tokio::test]
async fn add_duplicate_remove_and_positions() {
    let pool = setup().await;
    assert!(matches!(
        add_work(&pool, 99, "anilist:1", "anime").await,
        Err(ProfileError::NoProfile)
    ));
    assert_eq!(
        add_work(&pool, U, "anilist:1", "anime").await.expect("a"),
        AddOutcome::Added(1)
    );
    assert_eq!(
        add_work(&pool, U, "anilist:2", "manhwa").await.expect("a"),
        AddOutcome::Added(2)
    );
    assert_eq!(
        add_work(&pool, U, "anilist:1", "anime").await.expect("dup"),
        AddOutcome::Duplicate
    );
    assert!(has_work(&pool, U, "anilist:2").await.expect("has"));
    assert!(remove_work(&pool, U, "anilist:1").await.expect("rm"));
    assert!(!remove_work(&pool, U, "anilist:1").await.expect("again"));
    assert_eq!(positions(&pool, U).await, vec![1], "gap closed");
    assert_eq!(count(&pool, U).await.expect("n"), 1);
}

#[tokio::test]
async fn move_clamps_and_keeps_positions_contiguous() {
    let pool = setup().await;
    for i in 1..=5 {
        add_work(&pool, U, &format!("anilist:{i}"), "manga")
            .await
            .expect("add");
    }
    assert_eq!(move_work(&pool, U, "anilist:5", 1).await.expect("top"), 1);
    assert_eq!(
        move_work(&pool, U, "anilist:5", 99).await.expect("bottom"),
        5
    );
    assert_eq!(move_work(&pool, U, "anilist:2", 4).await.expect("down"), 4);
    assert_eq!(positions(&pool, U).await, vec![1, 2, 3, 4, 5]);
    assert!(matches!(
        move_work(&pool, U, "anilist:99", 1).await,
        Err(ProfileError::NotFound)
    ));
}

#[tokio::test]
async fn more_than_500_works_paginate() {
    let pool = setup().await;
    for i in 1..=620 {
        add_work(&pool, U, &format!("anilist:{i}"), "anime")
            .await
            .expect("add");
    }
    assert_eq!(count(&pool, U).await.expect("n"), 620);
    let page = list_page(&pool, U, None, None, 600, 20)
        .await
        .expect("page");
    assert_eq!(page.len(), 20);
    assert_eq!(page[0].position, 601);
    remove_work(&pool, U, "anilist:100").await.expect("rm");
    assert_eq!(positions(&pool, U).await.len(), 619);
    assert_eq!(positions(&pool, U).await.last(), Some(&619));
}

#[tokio::test]
async fn hidden_until_verified_and_render_from_cache() {
    let pool = setup().await;
    add_work(&pool, U, "anilist:1", "anime").await.expect("add");
    add_work(&pool, U, "anilist:2", "anime").await.expect("add");
    upsert_media_cache(&pool, &cache("anilist:1", true))
        .await
        .expect("cache");
    upsert_media_cache(&pool, &cache("anilist:2", false))
        .await
        .expect("cache");
    let page = list_page(&pool, U, None, None, 0, 10).await.expect("page");
    assert!(page[0].visible() && page[0].title() == "Title anilist:1");
    assert!(!page[1].visible(), "adult-flagged works are hidden");
    assert_eq!(page[0].anilist_id(), Some(1));
}

#[tokio::test]
async fn export_import_round_trip_with_works() {
    let pool = setup().await;
    add_work(&pool, U, "anilist:1", "anime").await.expect("add");
    add_work(&pool, U, "anilist:2", "manhua")
        .await
        .expect("add");
    set_tier(&pool, U, "anilist:2", Some("A"))
        .await
        .expect("tier");
    set_list(&pool, U, "anilist:1", Some("watched"))
        .await
        .expect("list");
    let groups = profile::list_groups(&pool, U).await.expect("g");
    let out = export_works(&pool, U, &groups).await.expect("export");
    assert_eq!(out.len(), 2);

    profile::create_profile(&pool, V).await.expect("v");
    let (added, dups, invalid) = import_works(&pool, V, &out).await.expect("import");
    assert_eq!((added, dups, invalid), (2, 0, 0));
    let again = import_works(&pool, V, &out).await.expect("again");
    assert_eq!(again, (0, 2, 0));
    let bad = vec![ExportWork {
        id: "evil:1".into(),
        kind: "anime".into(),
        group: None,
        tier: None,
        note: None,
        list: None,
    }];
    assert_eq!(import_works(&pool, V, &bad).await.expect("bad"), (0, 0, 1));
    let e = get_entry(&pool, V, "anilist:2")
        .await
        .expect("e")
        .expect("some");
    assert_eq!(e.tier.as_deref(), Some("A"));
    assert!(!e.visible(), "imported works stay hidden until verified");
}

#[tokio::test]
async fn id_helpers() {
    assert_eq!(anilist_id("anilist:42"), Some(42));
    assert_eq!(anilist_id("mdex:abc"), None);
    assert_eq!(work_key(7), "anilist:7");
}
