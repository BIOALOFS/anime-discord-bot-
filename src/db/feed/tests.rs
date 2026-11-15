use super::*;
use crate::db::connect_memory;

const G: i64 = 1;

#[tokio::test]
async fn config_defaults_and_setters() {
    let pool = connect_memory().await;
    let c = get_config(&pool, G).await.expect("config");
    assert!(!c.enabled && c.channel_id.is_none());
    assert_eq!(c.min_popularity, 1500);
    assert_eq!(c.formats, vec!["TV", "TV_SHORT", "ONA", "MOVIE"]);
    assert_eq!(
        (c.filter, c.kinds.len()),
        (FilterMode::All, 5),
        "every release of every work by default"
    );
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
    set_role(&pool, G, Some(77)).await.expect("role");
    set_filter_mode(&pool, G, FilterMode::Profiles)
        .await
        .expect("mode");
    set_kinds(&pool, G, &["anime".into(), "bogus".into(), "novel".into()])
        .await
        .expect("kinds");
    let c = get_config(&pool, G).await.expect("config");
    assert_eq!(c.channel_id, Some(55));
    assert_eq!(c.min_popularity, 0, "never negative");
    assert_eq!(
        c.formats,
        vec!["TV", "MOVIE"],
        "unknown formats are dropped"
    );
    assert_eq!(c.kinds, vec!["anime", "novel"], "unknown kinds are dropped");
    assert_eq!((c.role_id, c.filter), (Some(77), FilterMode::Profiles));
    assert_eq!(active_guilds(&pool).await.expect("a").len(), 1);
    set_role(&pool, G, None).await.expect("role");
    assert_eq!(get_config(&pool, G).await.expect("c").role_id, None);
}
