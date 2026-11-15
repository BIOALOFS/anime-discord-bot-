use super::*;
use crate::db::connect_memory;

const MAIN: i64 = 10;
const OTHER: i64 = 20;

fn settings(main: Option<i64>, exists: bool) -> ChannelSettings {
    ChannelSettings {
        command_channel: main,
        channel_exists: exists,
    }
}

#[test]
fn no_channel_set_means_everywhere() {
    let s = settings(None, true);
    assert_eq!(is_allowed(OTHER, &s, false), Decision::Allowed);
    assert_eq!(is_allowed(MAIN, &s, false), Decision::Allowed);
}

#[test]
fn command_channel_is_allowed_and_others_denied() {
    let s = settings(Some(MAIN), true);
    assert_eq!(is_allowed(MAIN, &s, false), Decision::Allowed);
    assert_eq!(
        is_allowed(OTHER, &s, false),
        Decision::Denied { channel: MAIN }
    );
}

#[test]
fn admins_always_bypass_but_members_stay_locked() {
    let s = settings(Some(MAIN), true);
    assert_eq!(is_allowed(OTHER, &s, true), Decision::Allowed);
    assert_eq!(
        is_allowed(OTHER, &s, false),
        Decision::Denied { channel: MAIN },
        "members are still locked"
    );
}

#[test]
fn a_deleted_command_channel_disables_the_lock() {
    let s = settings(Some(MAIN), false);
    assert_eq!(is_allowed(OTHER, &s, false), Decision::Allowed);
}

#[test]
fn settings_and_help_are_always_exempt() {
    assert!(is_exempt("settings"));
    assert!(is_exempt("settings channel"));
    assert!(
        !is_exempt("setup"),
        "the old /setup command no longer exists"
    );
    assert!(is_exempt("help"));
    assert!(!is_exempt("profile view"));
    assert!(!is_exempt("anime"));
    assert!(!is_exempt("settingsx"));
}

#[tokio::test]
async fn store_round_trip() {
    let store = GuildStore::new(connect_memory().await);
    let g = 5;
    assert_eq!(
        store.settings(g).await.expect("fresh").command_channel,
        None
    );
    store.set_command_channel(g, MAIN).await.expect("set");
    assert_eq!(
        store.settings(g).await.expect("s").command_channel,
        Some(MAIN)
    );
    store.reset(g).await.expect("reset");
    assert_eq!(store.settings(g).await.expect("s").command_channel, None);
}

#[tokio::test]
async fn deleted_channel_marker_survives_until_settings_change() {
    let store = GuildStore::new(connect_memory().await);
    store.set_command_channel(1, MAIN).await.expect("set");
    let s = store.settings(1).await.expect("s");
    store.mark_channel_deleted(1, &s).await;
    assert!(!store.settings(1).await.expect("s").channel_exists);
    assert_eq!(
        is_allowed(OTHER, &store.settings(1).await.expect("s"), false),
        Decision::Allowed
    );
    store.set_command_channel(1, 77).await.expect("reconfigure");
    assert!(
        store.settings(1).await.expect("s").channel_exists,
        "a new setting re-enables the lock"
    );
}

#[test]
fn admin_detection() {
    use poise::serenity_prelude::Permissions;
    assert!(is_admin(Some(Permissions::MANAGE_GUILD)));
    assert!(is_admin(Some(Permissions::ADMINISTRATOR)));
    assert!(!is_admin(Some(Permissions::SEND_MESSAGES)));
    assert!(!is_admin(None));
}

#[tokio::test]
async fn server_cooldown_blocks_a_second_command_per_user_only() {
    let store = GuildStore::new(connect_memory().await);
    assert_eq!(store.cooldown_left(1, 7, 0), None, "0 means no cooldown");
    assert_eq!(
        store.cooldown_left(1, 7, 30),
        None,
        "the first command is free"
    );
    let wait = store.cooldown_left(1, 7, 30).expect("second command waits");
    assert!(wait.as_secs() <= 30 && wait.as_secs() >= 28);
    assert_eq!(
        store.cooldown_left(1, 8, 30),
        None,
        "another user is unaffected"
    );
    assert_eq!(
        store.cooldown_left(2, 7, 30),
        None,
        "another server is unaffected"
    );
}

#[test]
fn admin_commands_work_in_every_channel() {
    for name in [
        "admin",
        "set-bot-channel",
        "set-notifications-channel",
        "settings",
        "help",
    ] {
        assert!(is_exempt(name), "{name}");
    }
    assert!(!is_exempt("edit") && !is_exempt("profile"));
}
