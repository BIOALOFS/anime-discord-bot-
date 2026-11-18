//! Navigation and lifetimes of interactive messages: how long components stay active (per message class, sliding with a
//! hard cap), the session rows that let a sweeper grey out components silently, and the Back stack of a message.

pub mod live;
pub mod overlay;
pub mod private;
pub mod stack;
pub mod store;
pub mod sweeper;

use serde_json::Value;
use std::sync::OnceLock;

/// Which lifetime a message gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Search, results, rosters, `/top`, cards, ephemeral views…
    Default,
    /// The profile message.
    Profile,
    /// Notifications and digests: fixed from the moment of sending.
    Notification,
}

impl Class {
    pub fn key(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Profile => "profile",
            Self::Notification => "notification",
        }
    }

    pub fn from_key(k: &str) -> Self {
        match k {
            "profile" => Self::Profile,
            "notification" => Self::Notification,
            _ => Self::Default,
        }
    }
}

/// The lifetime table, in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lifetimes {
    pub default_idle: u64,
    pub default_max: u64,
    pub profile_idle: u64,
    pub profile_max: u64,
    pub notification: u64,
}

impl Default for Lifetimes {
    fn default() -> Self {
        Self {
            default_idle: 60,
            default_max: 600,
            profile_idle: 300,
            profile_max: 900,
            notification: 3600,
        }
    }
}

impl Lifetimes {
    pub fn from_config(c: &crate::config::Config) -> Self {
        let d = Self::default();
        let pos = |v: Option<u64>, default: u64| v.filter(|n| *n > 0).unwrap_or(default);
        Self {
            default_idle: pos(c.view_timeout_default_secs, d.default_idle),
            default_max: pos(c.view_timeout_max_secs, d.default_max),
            profile_idle: pos(c.view_timeout_profile_secs, d.profile_idle),
            profile_max: pos(c.view_timeout_profile_max_secs, d.profile_max),
            notification: pos(c.view_timeout_notification_secs, d.notification),
        }
    }

    /// `(idle, hard cap)` of a class; a notification has no sliding part (idle = cap = its fixed hour).
    pub fn of(&self, class: Class) -> (u64, u64) {
        match class {
            Class::Default => (self.default_idle, self.default_max.max(self.default_idle)),
            Class::Profile => (self.profile_idle, self.profile_max.max(self.profile_idle)),
            Class::Notification => (self.notification, self.notification),
        }
    }

    /// When a message first sent at `now` stops being active, and the hard cap.
    pub fn first_expiry(&self, class: Class, now: i64) -> (i64, i64) {
        let (idle, cap) = self.of(class);
        (now + idle as i64, now + cap as i64)
    }

    /// The expiry after a press at `now` (never beyond the hard cap). A notification is not extended.
    pub fn extended(&self, class: Class, now: i64, current: i64, hard: i64) -> i64 {
        if class == Class::Notification {
            return current;
        }
        let (idle, _) = self.of(class);
        (now + idle as i64).min(hard).max(current.min(hard))
    }
}

static LIFETIMES: OnceLock<Lifetimes> = OnceLock::new();

/// Set once at startup from the configuration.
pub fn init(l: Lifetimes) {
    let _ = LIFETIMES.set(l);
}

pub fn lifetimes() -> Lifetimes {
    LIFETIMES.get().copied().unwrap_or_default()
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Register a notification the bot just sent (a digest, a reminder, a daily post): its buttons and menus stay active for
/// a fixed hour from now (never extended by presses) and are greyed out by the sweeper afterwards. A message without
/// buttons has nothing to expire and is not registered.
pub async fn register_notification(pool: &sqlx::SqlitePool, m: &poise::serenity_prelude::Message) {
    let components: Vec<Value> = serde_json::to_value(&m.components)
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    if components.is_empty() {
        return;
    }
    let json = serde_json::to_string(&components).unwrap_or_else(|_| "[]".into());
    let s = store::NewSession {
        message_id: m.id.get() as i64,
        channel_id: m.channel_id.get() as i64,
        guild_id: m.guild_id.map(|g| g.get() as i64),
        owner_id: None,
        kind: "notification",
        class: Class::Notification,
        components_json: &json,
        v2: false,
    };
    if let Err(e) = store::register(pool, &s, &lifetimes(), now()).await {
        tracing::warn!("could not register a notification session: {e}");
    }
}

/// What a press on a navigation button does to the message it sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// The owner: the message itself is edited (and gets a Back button).
    EditInPlace,
    /// Anyone else, or a shared or profile message: the message does not change, the presser gets a private view.
    Private,
}

/// `owner` is `None` for shared messages (game rounds). The profile (`is_v2`) is never navigated in place.
pub fn route(owner: Option<u64>, presser: u64, is_v2: bool) -> Route {
    if !is_v2 && owner == Some(presser) {
        Route::EditInPlace
    } else {
        Route::Private
    }
}

/// What the global event handler does with a press that reaches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventRoute {
    /// A running session owns the message and answers.
    Session,
    /// A notification: the presser gets a private view, the notification is never edited.
    Private,
    /// Anything else (a message from before a restart, a session that ended): acknowledged, nothing more.
    Stale,
}

pub fn event_route(live: bool, notification: bool) -> EventRoute {
    if live {
        EventRoute::Session
    } else if notification {
        EventRoute::Private
    } else {
        EventRoute::Stale
    }
}

/// Set `disabled: true` on every button and select menu of a components tree (classic action rows and Components V2
/// containers alike). Everything else in the JSON is left exactly as it is.
pub fn disable_components(v: &mut Value) {
    match v {
        Value::Array(a) => a.iter_mut().for_each(disable_components),
        Value::Object(o) => {
            // 2 button, 3 string select, 5 user select, 6 role select, 7 mentionable select, 8 channel select
            if matches!(o.get("type").and_then(Value::as_i64), Some(2 | 3 | 5..=8)) {
                o.insert("disabled".into(), Value::Bool(true));
            }
            if let Some(children) = o.get_mut("components") {
                disable_components(children);
            }
            // V2 sections hold an accessory button.
            if let Some(accessory) = o.get_mut("accessory") {
                disable_components(accessory);
            }
        }
        _ => {}
    }
}

/// Does a components tree use Components V2 (a container, section or text display at the top level)?
pub fn is_v2(components: &[Value]) -> bool {
    components.iter().any(|c| {
        matches!(
            c.get("type").and_then(Value::as_i64),
            Some(9 | 10 | 12 | 13 | 14 | 17)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_sent_message_serialises_its_components_in_discords_own_shape() {
        // What Discord sends back for a message with a button and a menu must come out as the same kind of JSON that
        // the sweeper edits with: numeric `type`s, `custom_id`, and the options.
        let raw = json!([{"type": 1, "components": [{"type": 2, "style": 2, "label": "x", "custom_id": "showwork:anilist:1"}]},
            {"type": 1, "components": [{"type": 3, "custom_id": "pickwork", "options": [{"label": "a", "value": "anilist:1"}]}]}]);
        let rows: Vec<poise::serenity_prelude::ActionRow> =
            serde_json::from_value(raw).expect("rows");
        let mut back = serde_json::to_value(&rows).expect("json");
        assert_eq!(back[0]["type"], 1);
        assert_eq!(back[0]["components"][0]["type"], 2);
        assert_eq!(back[0]["components"][0]["custom_id"], "showwork:anilist:1");
        assert_eq!(back[1]["components"][0]["type"], 3);
        disable_components(&mut back);
        assert_eq!(back[0]["components"][0]["disabled"], true);
        assert_eq!(back[1]["components"][0]["disabled"], true);
    }

    #[test]
    fn only_the_owner_edits_in_place_everyone_else_gets_a_private_view() {
        assert_eq!(route(Some(1), 1, false), Route::EditInPlace);
        assert_eq!(route(Some(1), 2, false), Route::Private, "another member");
        assert_eq!(
            route(None, 1, false),
            Route::Private,
            "a shared message (a game round)"
        );
        assert_eq!(
            route(Some(1), 1, true),
            Route::Private,
            "the profile is not navigated in place"
        );
    }

    #[test]
    fn notifications_always_answer_privately_and_stale_presses_do_nothing() {
        assert_eq!(event_route(true, false), EventRoute::Session);
        assert_eq!(event_route(true, true), EventRoute::Session);
        assert_eq!(event_route(false, true), EventRoute::Private);
        assert_eq!(event_route(false, false), EventRoute::Stale);
    }

    #[test]
    fn defaults_are_the_documented_table() {
        let l = Lifetimes::default();
        assert_eq!(l.of(Class::Default), (60, 600));
        assert_eq!(l.of(Class::Profile), (300, 900));
        assert_eq!(l.of(Class::Notification), (3600, 3600));
    }

    #[test]
    fn config_overrides_and_blank_or_zero_fall_back() {
        let mut c = crate::config::Config::for_tests();
        assert_eq!(Lifetimes::from_config(&c), Lifetimes::default());
        c.view_timeout_default_secs = Some(30);
        c.view_timeout_profile_secs = Some(120);
        c.view_timeout_notification_secs = Some(0);
        c.view_timeout_max_secs = Some(200);
        c.view_timeout_profile_max_secs = Some(400);
        let l = Lifetimes::from_config(&c);
        assert_eq!(l.of(Class::Default), (30, 200));
        assert_eq!(l.of(Class::Profile), (120, 400));
        assert_eq!(
            l.of(Class::Notification),
            (3600, 3600),
            "zero means the default"
        );
    }

    #[test]
    fn a_press_slides_the_expiry_but_never_past_the_hard_cap() {
        let l = Lifetimes::default();
        let (exp, hard) = l.first_expiry(Class::Default, 1000);
        assert_eq!((exp, hard), (1060, 1600));
        assert_eq!(
            l.extended(Class::Default, 1050, exp, hard),
            1110,
            "a press at 1050 keeps it active until 1110"
        );
        assert_eq!(
            l.extended(Class::Default, 1590, 1500, hard),
            1600,
            "capped at ten minutes in total"
        );
        let (exp, hard) = l.first_expiry(Class::Profile, 0);
        assert_eq!((exp, hard), (300, 900));
        assert_eq!(l.extended(Class::Profile, 200, exp, hard), 500);
        assert_eq!(l.extended(Class::Profile, 800, 700, hard), 900);
    }

    #[test]
    fn a_notification_is_fixed_for_an_hour_whatever_is_pressed() {
        let l = Lifetimes::default();
        let (exp, hard) = l.first_expiry(Class::Notification, 5000);
        assert_eq!((exp, hard), (8600, 8600));
        assert_eq!(l.extended(Class::Notification, 6000, exp, hard), 8600);
    }

    #[test]
    fn disabling_covers_classic_rows_selects_and_v2_containers_and_changes_nothing_else() {
        let mut rows = json!([
            {"type": 1, "components": [
                {"type": 2, "style": 2, "label": "a", "custom_id": "a"},
                {"type": 2, "style": 5, "label": "link", "url": "https://x.org"}]},
            {"type": 1, "components": [{"type": 3, "custom_id": "menu", "options": [{"label": "o", "value": "v"}]}]}]);
        let before = rows.clone();
        disable_components(&mut rows);
        for (i, j) in [(0, 0), (0, 1), (1, 0)] {
            assert_eq!(rows[i]["components"][j]["disabled"], true, "{i}/{j}");
        }
        // only `disabled` was added
        let mut undone = rows.clone();
        for (i, j) in [(0, 0), (0, 1), (1, 0)] {
            undone[i]["components"][j]
                .as_object_mut()
                .expect("object")
                .remove("disabled");
        }
        assert_eq!(undone, before);
        let mut v2 = json!([{"type": 17, "components": [
            {"type": 12, "items": []}, {"type": 10, "content": "hello"},
            {"type": 1, "components": [{"type": 2, "style": 2, "custom_id": "x"}]},
            {"type": 1, "components": [{"type": 3, "custom_id": "s", "options": []}]}]}]);
        disable_components(&mut v2);
        assert_eq!(v2[0]["components"][2]["components"][0]["disabled"], true);
        assert_eq!(v2[0]["components"][3]["components"][0]["disabled"], true);
        assert_eq!(v2[0]["components"][1]["content"], "hello", "the text stays");
        assert!(v2[0]["components"][1].get("disabled").is_none());
        assert!(is_v2(v2.as_array().expect("array")));
        assert!(!is_v2(rows.as_array().expect("array")));
    }
}
