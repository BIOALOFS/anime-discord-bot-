use super::*;
use crate::db::connect_memory;

const G: i64 = 1;

#[tokio::test]
async fn config_defaults_and_setters() {
    let pool = connect_memory().await;
    let c = get_config(&pool, G).await.expect("config");
    assert!(!c.enabled && c.channel_id.is_none());
    assert_eq!(
        (c.min_popularity, c.digest_hours, c.daily_cap),
        (1500, 0, 0)
    );
    assert_eq!(c.formats, vec!["TV", "TV_SHORT", "ONA", "MOVIE"]);
    assert!(!c.only_listed);
    assert!(
        active_guilds(&pool).await.expect("a").is_empty(),
        "off by default"
    );

    set_channel(&pool, G, 55).await.expect("ch");
    set_enabled(&pool, G, true).await.expect("on");
    set_min_popularity(&pool, G, -5).await.expect("pop");
    set_formats(&pool, G, &["TV".into(), "BOGUS".into(), "MOVIE".into()])
        .await
        .expect("fmt");
    set_excluded_genres(&pool, G, &["Ecchi".into(), "Horror".into()])
        .await
        .expect("genres");
    set_digest_hours(&pool, G, 500).await.expect("digest");
    set_daily_cap(&pool, G, 10).await.expect("cap");
    set_role(&pool, G, Some(77)).await.expect("role");
    set_only_listed(&pool, G, true).await.expect("mode");
    let c = get_config(&pool, G).await.expect("config");
    assert_eq!(c.channel_id, Some(55));
    assert_eq!(c.min_popularity, 0, "never negative");
    assert_eq!(
        c.formats,
        vec!["TV", "MOVIE"],
        "unknown formats are dropped"
    );
    assert_eq!(c.excluded_genres, vec!["Ecchi", "Horror"]);
    assert_eq!(c.digest_hours, 72, "clamped");
    assert_eq!(
        (c.daily_cap, c.role_id, c.only_listed),
        (10, Some(77), true)
    );
    assert_eq!(active_guilds(&pool).await.expect("a").len(), 1);
    set_role(&pool, G, None).await.expect("role");
    assert_eq!(get_config(&pool, G).await.expect("c").role_id, None);
}

#[tokio::test]
async fn notification_list_modes() {
    let pool = connect_memory().await;
    set_list_entry(&pool, G, 100, "Show A", ListMode::Listed)
        .await
        .expect("add");
    set_list_entry(&pool, G, 200, "Show B", ListMode::Muted)
        .await
        .expect("add");
    set_list_entry(&pool, G, 100, "Show A", ListMode::Pinned)
        .await
        .expect("update");
    let pinned = list_entries(&pool, G)
        .await
        .expect("list")
        .into_iter()
        .find(|e| e.0 == 100)
        .map(|e| e.2);
    assert_eq!(pinned, Some(ListMode::Pinned));
    let all = list_entries(&pool, G).await.expect("list");
    assert_eq!(all.len(), 2);
    assert!(remove_list_entry(&pool, G, 200).await.expect("rm"));
    assert!(!remove_list_entry(&pool, G, 200).await.expect("again"));
    assert_eq!(ListMode::parse("pinned"), Some(ListMode::Pinned));
    assert_eq!(ListMode::parse("x"), None);
}

#[tokio::test]
async fn requests_are_deduplicated_and_cleared_on_approval() {
    let pool = connect_memory().await;
    assert!(add_request(&pool, G, 100, "Show", 1).await.expect("r"));
    assert!(!add_request(&pool, G, 100, "Show", 1).await.expect("dup"));
    assert!(add_request(&pool, G, 100, "Show", 2)
        .await
        .expect("other user"));
    assert_eq!(
        pending_requests(&pool, G).await.expect("p"),
        vec![(100, "Show".to_string(), 2)]
    );
    set_list_entry(&pool, G, 100, "Show", ListMode::Listed)
        .await
        .expect("approve");
    assert!(pending_requests(&pool, G).await.expect("p").is_empty());
    assert!(add_request(&pool, G, 200, "Other", 1).await.expect("r"));
    assert!(deny_request(&pool, G, 200).await.expect("deny"));
}

#[tokio::test]
async fn dedupe_cap_digest_and_state() {
    let pool = connect_memory().await;
    assert!(!already_posted(&pool, G, 1, 5).await.expect("p"));
    mark_posted(&pool, G, 1, 5, 1_000).await.expect("m");
    mark_posted(&pool, G, 1, 5, 2_000)
        .await
        .expect("idempotent");
    assert!(already_posted(&pool, G, 1, 5).await.expect("p"));
    assert!(!already_posted(&pool, 2, 1, 5).await.expect("other guild"));
    mark_posted(&pool, G, 2, 1, 3_000).await.expect("m");
    assert_eq!(posted_since(&pool, G, 2_500).await.expect("n"), 1);
    assert_eq!(prune_posted(&pool, 1_500).await.expect("prune"), 1);

    enqueue_digest(&pool, G, 1, 6, "A", 20).await.expect("q");
    enqueue_digest(&pool, G, 1, 6, "A", 20)
        .await
        .expect("idempotent");
    enqueue_digest(&pool, G, 2, 1, "B", 10).await.expect("q");
    let d = drain_digest(&pool, G).await.expect("drain");
    assert_eq!(
        d.iter().map(|x| x.0).collect::<Vec<_>>(),
        vec![2, 1],
        "ordered by air time"
    );
    assert!(drain_digest(&pool, G).await.expect("drain").is_empty());

    assert_eq!(last_checked(&pool).await.expect("lc"), 0);
    set_last_checked(&pool, 123).await.expect("set");
    assert_eq!(last_checked(&pool).await.expect("lc"), 123);
}

#[tokio::test]
async fn birthday_targets_need_both_a_channel_and_the_switch() {
    let pool = connect_memory().await;
    assert!(birthday_targets(&pool).await.expect("t").is_empty());
    set_birthday_channel(&pool, G, 9).await.expect("c");
    assert!(
        birthday_targets(&pool).await.expect("t").is_empty(),
        "channel alone is not enough"
    );
    set_birthdays_enabled(&pool, G, true).await.expect("on");
    assert_eq!(birthday_targets(&pool).await.expect("t"), vec![(G, 9)]);
    set_birthdays_enabled(&pool, G, false).await.expect("off");
    assert!(birthday_targets(&pool).await.expect("t").is_empty());
}
