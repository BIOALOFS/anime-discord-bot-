use super::*;
use crate::db::connect_memory;

const MAIN: i64 = 10;
const OTHER: i64 = 20;

fn settings(main: Option<i64>, extra: &[i64], bypass: bool, exists: bool) -> ChannelSettings {
    ChannelSettings {
        command_channel: main,
        extra: extra.to_vec(),
        admin_bypass: bypass,
        channel_exists: exists,
    }
}

#[test]
fn no_channel_set_means_everywhere() {
    let s = settings(None, &[], true, true);
    assert_eq!(is_allowed(OTHER, &s, false), Decision::Allowed);
    assert_eq!(is_allowed(MAIN, &s, false), Decision::Allowed);
}

#[test]
fn command_channel_is_allowed_and_others_denied() {
    let s = settings(Some(MAIN), &[], false, true);
    assert_eq!(is_allowed(MAIN, &s, false), Decision::Allowed);
    assert_eq!(
        is_allowed(OTHER, &s, false),
        Decision::Denied { channel: MAIN }
    );
}

#[test]
fn admins_bypass_only_when_enabled() {
    let on = settings(Some(MAIN), &[], true, true);
    assert_eq!(is_allowed(OTHER, &on, true), Decision::Allowed);
    assert_eq!(
        is_allowed(OTHER, &on, false),
        Decision::Denied { channel: MAIN },
        "members are still locked"
    );
    let off = settings(Some(MAIN), &[], false, true);
    assert_eq!(
        is_allowed(OTHER, &off, true),
        Decision::Denied { channel: MAIN }
    );
}

#[test]
fn extra_channels_are_allowed() {
    let s = settings(Some(MAIN), &[30, 40], false, true);
    assert_eq!(is_allowed(30, &s, false), Decision::Allowed);
    assert_eq!(is_allowed(40, &s, false), Decision::Allowed);
    assert_eq!(
        is_allowed(50, &s, false),
        Decision::Denied { channel: MAIN }
    );
}

#[test]
fn a_deleted_command_channel_disables_the_lock() {
    let s = settings(Some(MAIN), &[], false, false);
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
    let g = 1;
    let fresh = store.settings(g).await.expect("fresh");
    assert_eq!(*fresh, ChannelSettings::default());

    store.set_command_channel(g, MAIN).await.expect("set");
    assert_eq!(
        store.settings(g).await.expect("s").command_channel,
        Some(MAIN)
    );

    assert_eq!(
        store.add_extra(g, 30).await.expect("add"),
        ExtraResult::Added
    );
    assert_eq!(
        store.add_extra(g, 30).await.expect("again"),
        ExtraResult::AlreadyThere
    );
    assert_eq!(
        store.add_extra(g, MAIN).await.expect("main"),
        ExtraResult::AlreadyThere
    );
    for c in 31..=34 {
        assert_eq!(
            store.add_extra(g, c).await.expect("add"),
            ExtraResult::Added
        );
    }
    assert_eq!(
        store.add_extra(g, 99).await.expect("full"),
        ExtraResult::Full,
        "at most 5 extras"
    );
    assert!(store.remove_extra(g, 30).await.expect("remove"));
    assert!(!store.remove_extra(g, 30).await.expect("gone"));
    assert_eq!(store.settings(g).await.expect("s").extra.len(), 4);

    store.set_admin_bypass(g, false).await.expect("bypass");
    assert!(!store.settings(g).await.expect("s").admin_bypass);

    // Making an extra the command channel removes it from the extras.
    store.set_command_channel(g, 31).await.expect("set");
    assert!(!store.settings(g).await.expect("s").extra.contains(&31));

    store.reset(g).await.expect("reset");
    let s = store.settings(g).await.expect("s");
    assert_eq!((s.command_channel, s.extra.len()), (None, 0));
    assert!(!s.admin_bypass, "reset keeps the admin-bypass choice");
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
