use super::*;
use crate::db::{connect_memory, profile};

const U: i64 = 1;
const V: i64 = 2;

async fn setup() -> SqlitePool {
    let pool = connect_memory().await;
    profile::create_profile(&pool, U).await.expect("profile");
    profile::create_profile(&pool, V).await.expect("profile");
    profile::add_character(&pool, U, 10).await.expect("add");
    pool
}

fn img(tag: &str) -> NewImage {
    NewImage {
        hash: tag.into(),
        mime: "image/png".into(),
        bytes: vec![1, 2, 3],
        width: 10,
        height: 20,
        animated: false,
    }
}

#[test]
fn text_is_sanitised() {
    assert_eq!(
        sanitize_text("  my   favourite\nwaifu ", 40).as_deref(),
        Some("my favourite waifu")
    );
    assert_eq!(
        sanitize_text("hi @everyone and @here <@123> <@&456> <#789> bye", 100).as_deref(),
        Some("hi and bye")
    );
    assert_eq!(
        sanitize_text("join discord.gg/abc now", 100).as_deref(),
        Some("join now")
    );
    assert_eq!(
        sanitize_text("see https://evil.example/x and www.evil.example ok", 100).as_deref(),
        Some("see and ok")
    );
    assert_eq!(
        sanitize_text("<script>alert</script>", 100).as_deref(),
        Some("scriptalert/script"),
        "angle brackets are stripped"
    );
    assert_eq!(
        sanitize_text("a sexual scene", 100),
        None,
        "explicit text is refused"
    );
    assert_eq!(sanitize_text("   \n  ", 100), None);
    assert_eq!(sanitize_text("@everyone", 100), None);
    assert_eq!(
        sanitize_text(&"word ".repeat(50), 40).map(|t| t.chars().count()),
        Some(39)
    );
}

#[tokio::test]
async fn title_and_description_only_for_own_characters() {
    let pool = setup().await;
    set_title(&pool, U, 10, Some("The Best"))
        .await
        .expect("title");
    set_description(&pool, U, 10, Some("My personal take @everyone"))
        .await
        .expect("desc");
    let e = profile::get_entry(&pool, U, 10)
        .await
        .expect("e")
        .expect("some");
    assert_eq!(e.custom_title.as_deref(), Some("The Best"));
    assert_eq!(e.custom_description.as_deref(), Some("My personal take"));
    // Not in V's profile -> refused, and U's data is untouched.
    assert!(matches!(
        set_title(&pool, V, 10, Some("Hijack")).await,
        Err(ProfileError::NotFound)
    ));
    assert!(matches!(
        set_title(&pool, U, 99, Some("x")).await,
        Err(ProfileError::NotFound)
    ));
    assert!(matches!(
        set_title(&pool, U, 10, Some("https://x.y")).await,
        Err(ProfileError::Invalid(_))
    ));
    assert_eq!(
        profile::get_entry(&pool, U, 10)
            .await
            .expect("e")
            .expect("s")
            .custom_title
            .as_deref(),
        Some("The Best")
    );
    reset_character(&pool, U, 10).await.expect("reset");
    let e = profile::get_entry(&pool, U, 10)
        .await
        .expect("e")
        .expect("some");
    assert!(
        e.custom_title.is_none() && e.custom_description.is_none() && e.custom_image_id.is_none()
    );
}

#[tokio::test]
async fn custom_images_are_deduplicated_capped_and_private() {
    let pool = setup().await;
    let StoreOutcome::Stored(id) = store_image(&pool, U, &img("a"), 2).await.expect("store") else {
        panic!("stored")
    };
    assert_eq!(
        store_image(&pool, U, &img("a"), 2).await.expect("dup"),
        StoreOutcome::Existing(id)
    );
    assert!(matches!(
        store_image(&pool, U, &img("b"), 2).await.expect("b"),
        StoreOutcome::Stored(_)
    ));
    assert_eq!(
        store_image(&pool, U, &img("c"), 2).await.expect("cap"),
        StoreOutcome::LimitReached
    );
    assert!(matches!(
        store_image(&pool, V, &img("a"), 2)
            .await
            .expect("other user"),
        StoreOutcome::Stored(_)
    ));
    assert_eq!(image_count(&pool, U).await.expect("n"), 2);

    set_character_image(&pool, U, 10, id).await.expect("attach");
    let got = image_of_character(&pool, U, 10)
        .await
        .expect("get")
        .expect("some");
    assert_eq!((got.id, got.bytes.as_slice()), (id, &[1u8, 2, 3][..]));
    // Another user can neither use U's image nor see it on their own profile.
    assert!(matches!(
        set_character_image(&pool, V, 10, id).await,
        Err(ProfileError::NotFound)
    ));
    profile::add_character(&pool, V, 10).await.expect("v add");
    assert!(
        image_of_character(&pool, V, 10)
            .await
            .expect("get")
            .is_none(),
        "the image only shows in its owner's profile"
    );
    // A character that is not in the caller's profile cannot get an image.
    assert!(matches!(
        set_character_image(&pool, U, 77, id).await,
        Err(ProfileError::NotFound)
    ));

    clear_character_image(&pool, U, 10).await.expect("clear");
    assert!(
        get_image(&pool, id).await.expect("g").is_none(),
        "orphan images are deleted"
    );
}

#[tokio::test]
async fn replacing_an_image_drops_the_old_one() {
    let pool = setup().await;
    let StoreOutcome::Stored(a) = store_image(&pool, U, &img("a"), 5).await.expect("a") else {
        panic!()
    };
    let StoreOutcome::Stored(b) = store_image(&pool, U, &img("b"), 5).await.expect("b") else {
        panic!()
    };
    set_character_image(&pool, U, 10, a).await.expect("a");
    set_character_image(&pool, U, 10, b).await.expect("b");
    assert!(get_image(&pool, a).await.expect("g").is_none());
    assert!(get_image(&pool, b).await.expect("g").is_some());
}

#[tokio::test]
async fn reports_bans_and_audit() {
    let pool = setup().await;
    let StoreOutcome::Stored(id) = store_image(&pool, U, &img("a"), 5).await.expect("a") else {
        panic!()
    };
    set_character_image(&pool, U, 10, id).await.expect("attach");
    assert!(report_image(&pool, id, V, Some(5)).await.expect("report"));
    assert!(!report_image(&pool, id, V, Some(5)).await.expect("again"));
    assert!(matches!(
        report_image(&pool, 999, V, Some(5)).await,
        Err(ProfileError::NotFound)
    ));
    assert_eq!(open_reports(&pool, 5).await.expect("r"), vec![(id, U, 1)]);

    assert!(remove_image_as_mod(&pool, id, 99, 5).await.expect("remove"));
    assert!(open_reports(&pool, 5).await.expect("r").is_empty());
    assert!(image_of_character(&pool, U, 10).await.expect("g").is_none());

    assert!(!is_banned(&pool, 5, U).await.expect("b"));
    ban_uploads(&pool, 5, U, 99).await.expect("ban");
    assert!(is_banned(&pool, 5, U).await.expect("b"));
    assert!(!is_banned(&pool, 6, U).await.expect("other guild"));
    assert!(unban_uploads(&pool, 5, U, 99).await.expect("unban"));
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE guild_id = 5")
        .fetch_one(&pool)
        .await
        .expect("n");
    assert_eq!(n, 3, "remove, ban and unban are audited");
}

#[tokio::test]
async fn server_switches_default_to_safe_values() {
    let pool = connect_memory().await;
    assert!(
        !custom_images_enabled(&pool, 1).await.expect("ci"),
        "custom images are off until an admin enables them"
    );
    assert!(games_enabled(&pool, 1).await.expect("g"));
    assert!(!nsfw_mode(&pool, 1).await.expect("n"));
    assert!(
        mature_games(&pool, 1).await.expect("m"),
        "mature-rated games are allowed by default"
    );
    set_mature_games(&pool, 1, false).await.expect("off");
    assert!(!mature_games(&pool, 1).await.expect("m"));
    assert!(mature_games(&pool, 2).await.expect("other guild"));
    set_custom_images(&pool, 1, true).await.expect("on");
    set_games(&pool, 1, false).await.expect("off");
    assert!(custom_images_enabled(&pool, 1).await.expect("ci"));
    assert!(!games_enabled(&pool, 1).await.expect("g"));
    assert!(!custom_images_enabled(&pool, 2).await.expect("other guild"));
}
