//! Integration tests against an in-memory SQLite database (real migrations).

use super::*;
use crate::db::connect_memory;

const U: i64 = 100;
const V: i64 = 200;

async fn db_with_profile() -> SqlitePool {
    let pool = connect_memory().await;
    assert!(create_profile(&pool, U).await.expect("create"));
    pool
}

async fn positions(pool: &SqlitePool, user: i64) -> Vec<(i64, i64)> {
    sqlx::query("SELECT character_id, position FROM profile_characters WHERE user_id = ?1 ORDER BY position")
        .bind(user)
        .fetch_all(pool)
        .await
        .expect("rows")
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect()
}

async fn assert_contiguous(pool: &SqlitePool, user: i64) {
    let pos: Vec<i64> = positions(pool, user).await.iter().map(|p| p.1).collect();
    let expected: Vec<i64> = (1..=pos.len() as i64).collect();
    assert_eq!(
        pos, expected,
        "positions must be 1..n without gaps or duplicates"
    );
}

fn cache(id: i64, safe: bool) -> CacheRow {
    CacheRow {
        character_id: id,
        name_romaji: format!("Char {id}"),
        name_native: Some("名".into()),
        image_url: Some("https://img".into()),
        is_safe: safe,
        main_work: Some("Work".into()),
        main_work_url: Some("https://anilist.co/anime/1".into()),
        favourites: Some(10),
        fetched_at: 1,
    }
}

#[tokio::test]
async fn add_and_duplicate() {
    let pool = db_with_profile().await;
    assert_eq!(
        add_character(&pool, U, 10).await.expect("add"),
        AddOutcome::Added(1)
    );
    assert_eq!(
        add_character(&pool, U, 20).await.expect("add"),
        AddOutcome::Added(2)
    );
    assert_eq!(
        add_character(&pool, U, 10).await.expect("dup"),
        AddOutcome::Duplicate
    );
    assert_eq!(count(&pool, U).await.expect("count"), 2);
    assert!(has_character(&pool, U, 10).await.expect("has"));
    assert_contiguous(&pool, U).await;
}

#[tokio::test]
async fn remove_closes_the_gap() {
    let pool = db_with_profile().await;
    for id in [1, 2, 3, 4] {
        add_character(&pool, U, id).await.expect("add");
    }
    assert!(remove_character(&pool, U, 2).await.expect("remove"));
    assert!(!remove_character(&pool, U, 2).await.expect("again"));
    assert_eq!(positions(&pool, U).await, vec![(1, 1), (3, 2), (4, 3)]);
}

#[tokio::test]
async fn move_and_reorder() {
    let pool = db_with_profile().await;
    for id in [1, 2, 3, 4, 5] {
        add_character(&pool, U, id).await.expect("add");
    }
    assert_eq!(move_character(&pool, U, 5, 1).await.expect("top"), 1);
    assert_eq!(
        all_character_ids(&pool, U).await.expect("ids"),
        vec![5, 1, 2, 3, 4]
    );
    move_character(&pool, U, 5, 99)
        .await
        .expect("bottom clamps");
    assert_eq!(
        all_character_ids(&pool, U).await.expect("ids"),
        vec![1, 2, 3, 4, 5]
    );
    move_character(&pool, U, 2, 4).await.expect("down");
    assert_eq!(
        all_character_ids(&pool, U).await.expect("ids"),
        vec![1, 3, 4, 2, 5]
    );
    move_character(&pool, U, 4, 2).await.expect("up");
    assert_eq!(
        all_character_ids(&pool, U).await.expect("ids"),
        vec![1, 4, 3, 2, 5]
    );
    move_character(&pool, U, 3, 3).await.expect("same place");
    assert_eq!(
        all_character_ids(&pool, U).await.expect("ids"),
        vec![1, 4, 3, 2, 5]
    );
    assert!(matches!(
        move_character(&pool, U, 999, 1).await,
        Err(ProfileError::NotFound)
    ));
    assert_contiguous(&pool, U).await;
}

#[tokio::test]
async fn view_pages_with_cached_metadata() {
    let pool = db_with_profile().await;
    for id in 1..=25 {
        add_character(&pool, U, id).await.expect("add");
    }
    upsert_cache(&pool, &cache(1, true)).await.expect("cache");
    upsert_cache(&pool, &cache(2, false)).await.expect("cache");
    let page1 = list_page(&pool, U, None, 0, 10).await.expect("page");
    assert_eq!(page1.len(), 10);
    assert!(page1[0].visible() && page1[0].name() == "Char 1");
    assert!(!page1[1].visible(), "unsafe is hidden");
    assert!(
        !page1[2].visible(),
        "unverified (no cache) is hidden too: fail-closed"
    );
    assert_eq!(
        list_page(&pool, U, None, 20, 10).await.expect("last").len(),
        5
    );
    assert!(list_page(&pool, U, None, 100, 10)
        .await
        .expect("beyond")
        .is_empty());
}

#[tokio::test]
async fn profile_with_more_than_500_characters() {
    let pool = db_with_profile().await;
    let started = std::time::Instant::now();
    for id in 1..=650 {
        add_character(&pool, U, id).await.expect("add");
    }
    assert_eq!(count(&pool, U).await.expect("count"), 650);
    assert_eq!(
        list_page(&pool, U, None, 640, 20)
            .await
            .expect("tail")
            .len(),
        10
    );
    move_character(&pool, U, 650, 1)
        .await
        .expect("move last to first");
    move_character(&pool, U, 1, 650)
        .await
        .expect("move first to last");
    remove_character(&pool, U, 300)
        .await
        .expect("remove middle");
    assert_eq!(count(&pool, U).await.expect("count"), 649);
    assert_contiguous(&pool, U).await;
    let page = list_page(&pool, U, None, 0, 25).await.expect("page");
    assert_eq!(
        page.iter().map(|e| e.position).collect::<Vec<_>>(),
        (1..=25).collect::<Vec<_>>()
    );
    assert!(
        started.elapsed().as_secs() < 30,
        "500+ characters must stay fast"
    );
}

#[tokio::test]
async fn privacy_levels() {
    let pool = db_with_profile().await;
    create_profile(&pool, V).await.expect("v");
    let mut p = get_profile(&pool, U).await.expect("get").expect("some");
    assert!(can_view(&pool, V, &p).await.expect("public"));
    set_privacy(&pool, U, Privacy::Private)
        .await
        .expect("private");
    p = get_profile(&pool, U).await.expect("get").expect("some");
    assert!(!can_view(&pool, V, &p).await.expect("private hides"));
    assert!(can_view(&pool, U, &p).await.expect("owner sees own"));
    set_privacy(&pool, U, Privacy::Servers)
        .await
        .expect("servers");
    p = get_profile(&pool, U).await.expect("get").expect("some");
    assert!(!can_view(&pool, V, &p).await.expect("no shared guild"));
    touch_user_guild(&pool, 1, U).await.expect("guild");
    touch_user_guild(&pool, 1, V).await.expect("guild");
    assert!(can_view(&pool, V, &p).await.expect("shared guild"));
}

#[tokio::test]
async fn cache_upsert_round_trip() {
    let pool = connect_memory().await;
    upsert_cache(&pool, &cache(5, true)).await.expect("insert");
    let mut changed = cache(5, false);
    changed.name_romaji = "Renamed".into();
    upsert_cache(&pool, &changed).await.expect("update");
    let got = cache_get(&pool, 5).await.expect("get").expect("some");
    assert_eq!((got.name_romaji.as_str(), got.is_safe), ("Renamed", false));
    assert!(cache_get(&pool, 6).await.expect("get").is_none());
}

#[tokio::test]
async fn search_own_matches_names_safely() {
    let pool = db_with_profile().await;
    for (id, name, safe) in [
        (1, "Naruto Uzumaki", true),
        (2, "Sasuke", true),
        (3, "Naruto Hidden", false),
        (4, "100%_odd", true),
    ] {
        add_character(&pool, U, id).await.expect("add");
        let mut c = cache(id, safe);
        c.name_romaji = name.into();
        upsert_cache(&pool, &c).await.expect("cache");
    }
    let hits = search_own(&pool, U, "naru", 10).await.expect("search");
    assert_eq!(
        hits,
        vec![(1, "Naruto Uzumaki".to_string())],
        "unsafe entries never show up"
    );
    assert_eq!(search_own(&pool, U, "", 10).await.expect("all").len(), 3);
    assert_eq!(
        search_own(&pool, U, "%", 10)
            .await
            .expect("wildcard is literal")
            .len(),
        1
    );
    assert_eq!(
        search_own(&pool, U, "_", 10)
            .await
            .expect("underscore is literal")
            .len(),
        1
    );
}

#[tokio::test]
async fn delete_user_data_removes_everything_about_the_user() {
    use crate::db::{custom, game, works};
    let pool = db_with_profile().await;
    create_profile(&pool, V).await.expect("v");
    add_character(&pool, U, 1).await.expect("add");
    add_character(&pool, V, 1).await.expect("add");
    works::add_work(&pool, U, "anilist:5", "anime")
        .await
        .expect("w");
    create_group(&pool, U, "G").await.expect("g");
    touch_user_guild(&pool, 9, U).await.expect("ug");
    game::record_result(&pool, 9, U, true).await.expect("game");
    game::hide_recommendation(&pool, U, 7).await.expect("hide");
    let image = custom::NewImage {
        hash: "h".into(),
        mime: "image/png".into(),
        bytes: vec![1],
        width: 1,
        height: 1,
        animated: false,
    };
    let custom::StoreOutcome::Stored(id) =
        custom::store_image(&pool, U, &image, 5).await.expect("img")
    else {
        panic!()
    };
    custom::set_character_image(&pool, U, 1, id)
        .await
        .expect("attach");

    delete_user_data(&pool, U).await.expect("delete");
    assert!(get_profile(&pool, U).await.expect("p").is_none());
    assert_eq!(works::count(&pool, U).await.expect("n"), 0);
    assert_eq!(custom::image_count(&pool, U).await.expect("n"), 0);
    assert_eq!(game::total_correct(&pool, U).await.expect("n"), 0);
    assert!(game::hidden_recommendations(&pool, U)
        .await
        .expect("h")
        .is_empty());
    assert!(list_groups(&pool, U).await.expect("g").is_empty());
    assert!(!shares_guild(&pool, U, V).await.expect("s"));
    // Other users are untouched.
    assert!(get_profile(&pool, V).await.expect("p").is_some());
    assert!(has_character(&pool, V, 1).await.expect("c"));
}

#[tokio::test]
async fn create_is_idempotent_and_required_first() {
    let pool = connect_memory().await;
    assert!(matches!(
        add_character(&pool, U, 1).await,
        Err(ProfileError::NoProfile)
    ));
    assert!(create_profile(&pool, U).await.expect("first"));
    assert!(
        !create_profile(&pool, U).await.expect("second"),
        "second create changes nothing"
    );
    let p = get_profile(&pool, U).await.expect("get").expect("some");
    assert_eq!(p.privacy.as_str(), "public");
    assert_eq!(p.language, "ar");
    assert!(p.display_title.is_none() && p.banner.is_none());
}

#[tokio::test]
async fn bio_color_banner_style_privacy() {
    let pool = db_with_profile().await;
    set_bio(&pool, U, Some(&"x".repeat(900)))
        .await
        .expect("bio");
    set_color(&pool, U, Some(0x3DB4F2)).await.expect("colour");
    set_banner(&pool, U, Some("https://img/banner.png"))
        .await
        .expect("banner");
    set_display_title(&pool, U, Some(&"t".repeat(90)))
        .await
        .expect("title");
    set_language(&pool, U, "en").await.expect("language");
    assert!(
        set_language(&pool, U, "xx").await.is_err(),
        "unknown languages are refused"
    );
    set_privacy(&pool, U, Privacy::Servers)
        .await
        .expect("privacy");
    let p = get_profile(&pool, U).await.expect("get").expect("some");
    assert_eq!(p.bio.as_deref().map(|b| b.chars().count()), Some(MAX_BIO));
    assert_eq!(p.accent_color, Some(0x3DB4F2));
    assert_eq!((p.language.as_str(), p.privacy()), ("en", Privacy::Servers));
    assert_eq!(
        p.display_title.as_deref().map(|t| t.chars().count()),
        Some(MAX_TITLE)
    );
    reset_profile(&pool, U).await.expect("reset");
    let p = get_profile(&pool, U).await.expect("get").expect("some");
    assert!(
        p.bio.is_none()
            && p.banner.is_none()
            && p.accent_color.is_none()
            && p.display_title.is_none()
    );
    set_bio(&pool, U, Some("again")).await.expect("bio again");
    set_bio(&pool, U, Some("   ")).await.expect("blank clears");
    assert!(get_profile(&pool, U)
        .await
        .expect("get")
        .expect("some")
        .bio
        .is_none());
    assert!(matches!(
        set_bio(&pool, 12345, Some("x")).await,
        Err(ProfileError::NoProfile)
    ));
}

#[tokio::test]
async fn export_import_round_trip_and_validation() {
    let pool = db_with_profile().await;
    for id in [10, 20, 30] {
        add_character(&pool, U, id).await.expect("add");
    }
    let g = create_group(&pool, U, "Best").await.expect("group");
    assign_group(&pool, U, 20, Some(g)).await.expect("assign");
    set_tier(&pool, U, 10, Some("S")).await.expect("tier");
    set_note(&pool, U, 30, Some("note")).await.expect("note");
    set_bio(&pool, U, Some("hello")).await.expect("bio");
    set_color(&pool, U, Some(0xff0000)).await.expect("colour");
    let doc = export(&pool, U).await.expect("export");
    let json = serde_json::to_string(&doc).expect("json");
    let parsed: ExportDoc = serde_json::from_str(&json).expect("parse");
    assert_eq!(parsed, doc);

    create_profile(&pool, V).await.expect("v");
    let report = import(&pool, V, &parsed).await.expect("import");
    assert_eq!(
        report,
        ImportReport {
            added: 3,
            duplicates: 0,
            invalid: 0
        }
    );
    let again = import(&pool, V, &parsed).await.expect("again");
    assert_eq!((again.added, again.duplicates), (0, 3));
    assert_eq!(
        all_character_ids(&pool, V).await.expect("ids"),
        vec![10, 20, 30]
    );
    let e = get_entry(&pool, V, 20).await.expect("entry").expect("some");
    assert!(e.group_id.is_some(), "group restored");
    assert_eq!(
        get_entry(&pool, V, 10)
            .await
            .expect("e")
            .expect("s")
            .tier
            .as_deref(),
        Some("S")
    );
    let imported = get_entry(&pool, V, 10).await.expect("e").expect("s");
    assert!(
        !imported.visible(),
        "imported characters stay hidden until verified safe"
    );

    let mut bad = parsed.clone();
    bad.version = 3;
    assert!(matches!(
        import(&pool, V, &bad).await,
        Err(ProfileError::Invalid(_))
    ));
    let mut junk = parsed;
    junk.characters.push(ExportChar {
        id: -5,
        group: None,
        tier: None,
        note: None,
    });
    junk.banner = Some("http://insecure".into());
    junk.privacy = "weird".into();
    let r = import(&pool, V, &junk).await.expect("junk handled");
    assert_eq!(r.invalid, 1);
    let p = get_profile(&pool, V).await.expect("get").expect("some");
    assert_ne!(p.banner.as_deref(), Some("http://insecure"));
}
