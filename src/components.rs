//! The universal button rule (every message the bot sends):
//! 1. a message that shows or mentions a CHARACTER carries "أظهر العمل";
//! 2. a message that shows or mentions a WORK carries "أظهر الشخصيات";
//! 3. the profile is the one exception (it keeps exactly its own controls);
//! 4. lists use a select menu of their items instead of one button per item;
//! 5. event rounds show these buttons only after the answer is revealed.
//!
//! Both buttons send a NEW message (never an edit of the source message) and work for every member, so their ids are
//! handled globally in `events`, never by a command session. This module is the single place that builds them.

use poise::serenity_prelude as serenity;

pub const SHOW_WORK: &str = "showwork";
pub const SHOW_CHARS: &str = "showchars";
pub const PICK_WORK: &str = "pickwork";
pub const PICK_CHAR: &str = "pickchar";
pub const REMIND: &str = "remind";
pub const ADD_PROFILE: &str = "addprofile";
/// The watch / read status menu of a work card: `setstatus:<work key>` (the chosen status is the menu value).
pub const SET_STATUS: &str = "setstatus";
/// Suffix of the pagination buttons of a roster message (these edit that new message, not the source).
pub const NAV: &str = "nav";

pub const LABEL_SHOW_WORK: &str = "أظهر العمل";
pub const LABEL_SHOW_CHARS: &str = "أظهر الشخصيات";
pub const LABEL_REMIND: &str = "ذكّرني";
pub const LABEL_ADD: &str = "أضف لبروفايلي";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Work,
    Character,
}

/// Any id handled by the global handler: command sessions must leave these presses alone.
pub fn is_global_id(id: &str) -> bool {
    id == PICK_WORK
        || id == PICK_CHAR
        || [SHOW_WORK, SHOW_CHARS, REMIND, ADD_PROFILE, SET_STATUS]
            .iter()
            .any(|p| id.starts_with(&format!("{p}:")))
}

/// Provider-prefixed key of a work: `anilist:1`, `tmdb-tv:2`, `tmdb-movie:3`, `igdb:4`, `vndb:v5`.
pub fn is_work_key(key: &str) -> bool {
    ["anilist:", "tmdb-tv:", "tmdb-movie:", "igdb:", "vndb:v"]
        .iter()
        .any(|p| {
            key.strip_prefix(p)
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
        })
}

pub fn show_work_id(work_key: &str) -> String {
    format!("{SHOW_WORK}:{work_key}")
}

pub fn show_chars_id(work_key: &str) -> String {
    format!("{SHOW_CHARS}:{work_key}")
}

/// A roster page button: `showchars:<work key>:<page>:nav`.
pub fn roster_nav_id(work_key: &str, page: usize) -> String {
    format!("{SHOW_CHARS}:{work_key}:{page}:{NAV}")
}

pub fn remind_id(work_key: &str) -> String {
    format!("{REMIND}:{work_key}")
}

pub fn add_id(key: &str) -> String {
    format!("{ADD_PROFILE}:{key}")
}

/// What a global press asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Press {
    ShowWork(String),
    /// The work key, and the page when it is a roster page button (`None`: the first press, a new message).
    ShowChars(String, Option<usize>),
    PickWork(String),
    PickChar(String),
    Remind(String),
    Add(String),
    /// Work key and the chosen status (`current`, `completed`, `planned`, `paused`, `dropped`, or `none` to clear).
    Status(String, String),
}

/// Parse a custom id (and the selected value of a menu).
pub fn parse(id: &str, values: &[String]) -> Option<Press> {
    if id == PICK_WORK {
        return values.first().map(|v| Press::PickWork(v.clone()));
    }
    if id == PICK_CHAR {
        return values.first().map(|v| Press::PickChar(v.clone()));
    }
    let (head, rest) = id.split_once(':')?;
    match head {
        SHOW_WORK => is_work_key(rest).then(|| Press::ShowWork(rest.to_string())),
        REMIND => is_work_key(rest).then(|| Press::Remind(rest.to_string())),
        ADD_PROFILE => (!rest.is_empty()).then(|| Press::Add(rest.to_string())),
        SET_STATUS => {
            let value = values.first()?;
            (is_work_key(rest) && (value == "none" || crate::db::status::is_valid(value)))
                .then(|| Press::Status(rest.to_string(), value.clone()))
        }
        SHOW_CHARS => {
            if let Some(body) = rest.strip_suffix(&format!(":{NAV}")) {
                let (key, page) = body.rsplit_once(':')?;
                return is_work_key(key)
                    .then_some(())
                    .and(page.parse().ok())
                    .map(|p| Press::ShowChars(key.to_string(), Some(p)));
            }
            is_work_key(rest).then(|| Press::ShowChars(rest.to_string(), None))
        }
        _ => None,
    }
}

/// The standard buttons of a message about one item: a character message gets "أظهر العمل" (needs the work's key),
/// a work message gets "أظهر الشخصيات".
pub fn standard_actions(kind: ItemKind, work_key: Option<&str>) -> Vec<serenity::CreateButton> {
    let secondary = serenity::ButtonStyle::Secondary;
    match (kind, work_key) {
        (ItemKind::Character, Some(k)) => vec![serenity::CreateButton::new(show_work_id(k))
            .label(LABEL_SHOW_WORK)
            .emoji('📖')
            .style(secondary)],
        (ItemKind::Work, Some(k)) => vec![serenity::CreateButton::new(show_chars_id(k))
            .label(LABEL_SHOW_CHARS)
            .emoji('👥')
            .style(secondary)],
        _ => Vec::new(),
    }
}

/// "ذكّرني" for a work.
pub fn remind_button(work_key: &str) -> serenity::CreateButton {
    serenity::CreateButton::new(remind_id(work_key))
        .label(LABEL_REMIND)
        .emoji('🔔')
        .style(serenity::ButtonStyle::Secondary)
}

/// "أضف لبروفايلي" for a work or a character key.
pub fn add_button(key: &str) -> serenity::CreateButton {
    serenity::CreateButton::new(add_id(key))
        .label(LABEL_ADD)
        .emoji('➕')
        .style(serenity::ButtonStyle::Secondary)
}

/// The watch / read status select of a work card (a "none" option clears it).
pub fn status_menu(
    work_key: &str,
    written: bool,
    current: Option<&str>,
) -> Option<serenity::CreateActionRow> {
    let mut choices: Vec<crate::ui::select::Choice> = crate::db::status::STATUSES
        .iter()
        .map(|s| crate::ui::select::Choice {
            label: crate::db::status::label(s, written).to_string(),
            description: None,
            value: (*s).to_string(),
        })
        .collect();
    choices.push(crate::ui::select::Choice {
        label: "بدون حالة".into(),
        description: None,
        value: "none".into(),
    });
    let placeholder = match current {
        Some(c) if crate::db::status::is_valid(c) => {
            format!("الحالة: {}", crate::db::status::label(c, written))
        }
        _ => "حدّد حالتك مع هذا العمل".to_string(),
    };
    crate::ui::select::select_row(
        &format!("{SET_STATUS}:{work_key}"),
        &placeholder,
        choices,
        true,
    )
}

/// A menu of works for lists (`/top`, `/recommend`, `/schedule`, digests): choosing one opens its card as a new message.
pub fn work_menu(
    placeholder: &str,
    items: &[(String, String)],
) -> Option<serenity::CreateActionRow> {
    menu(PICK_WORK, placeholder, items)
}

/// A menu of characters (rosters, lists of characters).
pub fn character_menu(
    placeholder: &str,
    items: &[(String, String)],
) -> Option<serenity::CreateActionRow> {
    menu(PICK_CHAR, placeholder, items)
}

fn menu(
    id: &str,
    placeholder: &str,
    items: &[(String, String)],
) -> Option<serenity::CreateActionRow> {
    let choices = items
        .iter()
        .map(|(label, value)| crate::ui::select::Choice {
            label: label.clone(),
            description: None,
            value: value.clone(),
        })
        .collect();
    crate::ui::select::select_row(id, placeholder, choices, true)
}

/// Every message view the bot has and what it shows. A new `impl View for X` must be listed here (a test scans the
/// sources), so nothing can ship without declaring its kind. `Exempt` is only the profile.
#[cfg(test)]
pub const VIEW_KINDS: &[(&str, ViewKind)] = &[
    ("ProfileView", ViewKind::Exempt),
    ("CharacterView", ViewKind::Character),
    ("MediaView", ViewKind::Work),
    ("FallbackView", ViewKind::Work),
    ("TmdbView", ViewKind::Work),
    ("TopView", ViewKind::List),
    ("ScheduleView", ViewKind::List),
    ("RecView", ViewKind::List),
    ("SuggestView", ViewKind::List),
    ("MineView", ViewKind::None),
    ("RemindersView", ViewKind::None),
    ("Picker", ViewKind::None),
    ("AdminView", ViewKind::None),
    ("HelpView", ViewKind::None),
    ("DeleteView", ViewKind::None),
    ("Confirm", ViewKind::None),
    ("NotifyView", ViewKind::None),
    ("AiringView", ViewKind::List),
    ("Arena", ViewKind::Event),
    ("Versus", ViewKind::Event),
    ("PickAddView", ViewKind::List),
];

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    Work,
    Character,
    /// Lists use select menus (rule 4).
    List,
    /// Rounds show the buttons only after the reveal (rule 5).
    Event,
    /// The profile.
    Exempt,
    None,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(buttons: Vec<serenity::CreateButton>) -> Vec<String> {
        buttons
            .iter()
            .map(|b| {
                serde_json::to_value(b).expect("json")["label"]
                    .as_str()
                    .unwrap_or("")
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn a_character_message_gets_show_work_and_a_work_message_gets_show_characters() {
        assert_eq!(
            labels(standard_actions(ItemKind::Character, Some("anilist:1"))),
            vec!["أظهر العمل"]
        );
        assert_eq!(
            labels(standard_actions(ItemKind::Work, Some("tmdb-tv:5"))),
            vec!["أظهر الشخصيات"]
        );
        assert!(
            standard_actions(ItemKind::Work, None).is_empty(),
            "no key, no button"
        );
    }

    #[test]
    fn ids_are_stable_provider_prefixed_and_parse_back() {
        for key in [
            "anilist:12",
            "tmdb-tv:1396",
            "tmdb-movie:5",
            "igdb:77",
            "vndb:v17",
        ] {
            assert!(is_work_key(key), "{key}");
            assert_eq!(
                parse(&show_work_id(key), &[]),
                Some(Press::ShowWork(key.into()))
            );
            assert_eq!(
                parse(&show_chars_id(key), &[]),
                Some(Press::ShowChars(key.into(), None))
            );
            assert_eq!(
                parse(&roster_nav_id(key, 3), &[]),
                Some(Press::ShowChars(key.into(), Some(3)))
            );
            assert_eq!(parse(&remind_id(key), &[]), Some(Press::Remind(key.into())));
            assert!(show_work_id(key).len() <= 100);
        }
        assert_eq!(
            parse(PICK_WORK, &["anilist:9".into()]),
            Some(Press::PickWork("anilist:9".into()))
        );
        assert_eq!(
            parse(PICK_CHAR, &["ac:9".into()]),
            Some(Press::PickChar("ac:9".into()))
        );
        assert_eq!(parse(PICK_CHAR, &[]), None);
        assert_eq!(
            parse(&add_id("tmdb-cast:tv:1:2"), &[]),
            Some(Press::Add("tmdb-cast:tv:1:2".into()))
        );
        for bad in [
            "showwork:",
            "showwork:anilist:x",
            "showwork:mdex:abc",
            "showchars:anilist:1:x:nav",
            "close",
            "back",
            "showwork",
        ] {
            assert_eq!(parse(bad, &[]), None, "{bad}");
        }
    }

    #[test]
    fn global_ids_are_recognised_so_sessions_leave_them_alone() {
        for id in [
            show_work_id("anilist:1"),
            show_chars_id("tmdb-tv:2"),
            PICK_WORK.to_string(),
            PICK_CHAR.to_string(),
            remind_id("anilist:3"),
            add_id("anilist:4"),
        ] {
            assert!(is_global_id(&id), "{id}");
        }
        for id in ["close", "back", "nav:next", "item", "showworkers", "ans:1"] {
            assert!(!is_global_id(id), "{id}");
        }
    }

    #[test]
    fn menus_respect_discords_limits() {
        let items: Vec<(String, String)> = (0..40)
            .map(|i| ("w".repeat(150), format!("anilist:{}", i % 30)))
            .collect();
        let row = work_menu("اختر", &items).expect("menu");
        let j = serde_json::to_value(&row).expect("json");
        let opts = j["components"][0]["options"].as_array().expect("options");
        assert_eq!(opts.len(), 25, "25 options at most, duplicates removed");
        assert!(opts
            .iter()
            .all(|o| o["label"].as_str().unwrap_or("").chars().count() <= 100));
        assert!(work_menu("اختر", &[]).is_none());
    }

    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).expect("dir").flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("tests.rs") {
                out.push(p);
            }
        }
    }

    /// Every `impl View for X` must be declared in `VIEW_KINDS`; the declared kind decides what the source must use.
    #[test]
    fn every_view_declares_its_kind_and_uses_the_central_builders() {
        let mut files = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let mut found = 0;
        for f in files {
            let text = std::fs::read_to_string(&f).expect("read");
            let prod = text.split("#[cfg(test)]").next().unwrap_or(&text);
            for line in prod.lines() {
                let Some(name) = line.trim().strip_prefix("impl View for ") else {
                    continue;
                };
                let name = name.trim_end_matches(" {").trim();
                found += 1;
                let kind = VIEW_KINDS.iter().find(|(n, _)| *n == name).map(|(_, k)| *k);
                let kind = kind.unwrap_or_else(|| {
                    panic!(
                        "view {name} ({}) is not declared in VIEW_KINDS",
                        f.display()
                    )
                });
                match kind {
                    ViewKind::Work | ViewKind::Character => assert!(
                        prod.contains("standard_actions(") || prod.contains("components::"),
                        "{name} shows a {kind:?} but does not use the central standard_actions builder"
                    ),
                    ViewKind::List => assert!(
                        prod.contains("work_menu(") || prod.contains("select_row(") || prod.contains("character_choices("),
                        "{name} lists items but has no select menu"
                    ),
                    ViewKind::Exempt => assert_eq!(name, "ProfileView", "only the profile is exempt"),
                    ViewKind::Event | ViewKind::None => {}
                }
            }
        }
        assert!(found >= 12, "the scan found only {found} views");
        let mut seen = std::collections::HashSet::new();
        assert!(
            VIEW_KINDS.iter().all(|(n, _)| seen.insert(*n)),
            "no duplicate entries"
        );
        assert_eq!(
            VIEW_KINDS
                .iter()
                .filter(|(_, k)| *k == ViewKind::Exempt)
                .count(),
            1
        );
    }
}
