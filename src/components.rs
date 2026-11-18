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
/// People: a menu of persons (opens a person card), the works of a person, the crew of a work, the performer of a
/// character, and the overflow menu "المزيد" that holds optional buttons when a row would be too full.
pub const PICK_PERSON: &str = "pickperson";
pub const SHOW_WORKS: &str = "showworks";
pub const SHOW_CREW: &str = "showcrew";
pub const SHOW_PERFORMER: &str = "showperf";
pub const PICK_MORE: &str = "pickmore";
/// "أظهر العمل" of a wiki character: the work is looked up when the button is pressed.
pub const SHOW_WORK_OF: &str = "showworkof";
/// The general search: its result menu (any kind of item) and its page buttons.
pub const PICK_ANY: &str = "pickany";
pub const SEARCH_PAGE: &str = "searchpage";
/// "رجوع": back to the previous view of the same message.
pub const BACK: &str = "navback";
pub const LABEL_BACK: &str = "رجوع";
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
pub const LABEL_SHOW_WORKS: &str = "أظهر الأعمال";
pub const LABEL_SHOW_CREW: &str = "أظهر الطاقم";
pub const LABEL_SHOW_PERFORMER: &str = "أظهر المؤدي";
pub const LABEL_MORE: &str = "المزيد";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Work,
    Character,
}

/// Any id handled by the global handler: command sessions must leave these presses alone.
pub fn is_global_id(id: &str) -> bool {
    id == PICK_WORK
        || id == PICK_CHAR
        || id == PICK_PERSON
        || id == PICK_MORE
        || id == PICK_ANY
        || id == BACK
        || [
            SEARCH_PAGE,
            SHOW_WORK,
            SHOW_CHARS,
            SHOW_WORKS,
            SHOW_CREW,
            SHOW_PERFORMER,
            SHOW_WORK_OF,
            REMIND,
            ADD_PROFILE,
            SET_STATUS,
        ]
        .iter()
        .any(|p| id.starts_with(&format!("{p}:")))
}

/// Provider-prefixed key of a work: `anilist:1`, `tmdb-tv:2`, `tmdb-movie:3`, `igdb:4`, `vndb:v5`.
pub fn is_work_key(key: &str) -> bool {
    // A page of a wiki is `wiki:{wiki}:{page}`: a game when shown as a work, a character when opened from a roster.
    if crate::wiki::parse_key(key).is_some() {
        return true;
    }
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

/// A person key: `anilist-staff:1`, `tmdb-person:2`, `vndb-staff:s3`, `igdb-company:4`.
pub fn is_person_key(key: &str) -> bool {
    crate::people::parse_key(key).is_some()
}

pub fn show_works_id(person_key: &str) -> String {
    format!("{SHOW_WORKS}:{person_key}")
}

pub fn works_nav_id(person_key: &str, page: usize) -> String {
    format!("{SHOW_WORKS}:{person_key}:{page}:{NAV}")
}

pub fn show_crew_id(work_key: &str) -> String {
    format!("{SHOW_CREW}:{work_key}")
}

pub fn crew_nav_id(work_key: &str, page: usize) -> String {
    format!("{SHOW_CREW}:{work_key}:{page}:{NAV}")
}

pub fn show_work_of_id(character_key: &str) -> String {
    format!("{SHOW_WORK_OF}:{character_key}")
}

pub fn show_performer_id(character_key: &str) -> String {
    format!("{SHOW_PERFORMER}:{character_key}")
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
    PickPerson(String),
    /// Back to the previous view of this message.
    Back,
    /// Any item of the general search, by key: opens a work, character or person card.
    PickAny(String),
    /// A page button of a general-search result: category, type, page, text.
    SearchPage(crate::search::Cat, crate::search::Ty, usize, String),
    /// The works of a person, and the page when it is a page button of that message.
    ShowWorks(String, Option<usize>),
    /// The crew of a work, and the page when it is a page button of that message.
    ShowCrew(String, Option<usize>),
    /// The performer(s) of a character.
    ShowPerformer(String),
    /// The work of a wiki character, resolved on the press.
    ShowWorkOf(String),
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
    if id == PICK_PERSON {
        return values
            .first()
            .filter(|v| is_person_key(v))
            .map(|v| Press::PickPerson(v.clone()));
    }
    if id == BACK {
        return Some(Press::Back);
    }
    if id == PICK_ANY {
        return values
            .first()
            .filter(|v| crate::search::is_key(v))
            .map(|v| Press::PickAny(v.clone()));
    }
    if id.starts_with(&format!("{SEARCH_PAGE}:")) {
        return crate::search::parse_page_id(id).map(|(c, t, p, q)| Press::SearchPage(c, t, p, q));
    }
    if id == PICK_MORE {
        // The overflow menu carries the id of the button it stands for.
        let v = values.first()?;
        return (v != PICK_MORE).then(|| parse(v, &[])).flatten();
    }
    let (head, rest) = id.split_once(':')?;
    match head {
        SHOW_WORKS | SHOW_CREW => {
            let ok = |k: &str| {
                if head == SHOW_WORKS {
                    is_person_key(k)
                } else {
                    is_work_key(k)
                }
            };
            let make = |k: String, p: Option<usize>| {
                if head == SHOW_WORKS {
                    Press::ShowWorks(k, p)
                } else {
                    Press::ShowCrew(k, p)
                }
            };
            if let Some(body) = rest.strip_suffix(&format!(":{NAV}")) {
                let (key, page) = body.rsplit_once(':')?;
                return ok(key)
                    .then_some(())
                    .and(page.parse().ok())
                    .map(|p| make(key.to_string(), Some(p)));
            }
            ok(rest).then(|| make(rest.to_string(), None))
        }
        SHOW_PERFORMER => (!rest.is_empty()).then(|| Press::ShowPerformer(rest.to_string())),
        SHOW_WORK_OF => crate::wiki::parse_key(rest).map(|_| Press::ShowWorkOf(rest.to_string())),
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
        (ItemKind::Work, Some(k)) => vec![
            serenity::CreateButton::new(show_chars_id(k))
                .label(LABEL_SHOW_CHARS)
                .emoji('👥')
                .style(secondary),
            serenity::CreateButton::new(show_crew_id(k))
                .label(LABEL_SHOW_CREW)
                .emoji('🎬')
                .style(secondary),
        ],
        _ => Vec::new(),
    }
}

/// "أظهر المؤدي" for a character (the voice actor or actor).
pub fn performer_button(character_key: &str) -> serenity::CreateButton {
    serenity::CreateButton::new(show_performer_id(character_key))
        .label(LABEL_SHOW_PERFORMER)
        .emoji('🎙')
        .style(serenity::ButtonStyle::Secondary)
}

/// "أظهر الأعمال" for a person.
pub fn works_button(person_key: &str) -> serenity::CreateButton {
    serenity::CreateButton::new(show_works_id(person_key))
        .label(LABEL_SHOW_WORKS)
        .emoji('🎞')
        .style(serenity::ButtonStyle::Secondary)
}

/// A menu of persons: choosing one opens the person card as a new message.
pub fn person_menu(
    placeholder: &str,
    items: &[(String, String)],
) -> Option<serenity::CreateActionRow> {
    menu(PICK_PERSON, placeholder, items)
}

/// Buttons of one card as action rows within Discord's limits (five buttons per row). Never dropped: the buttons
/// every card must carry (show work / characters, add). When there are too many, the optional ones go in this order:
/// link buttons first (the caller keeps the links as text), then the crew / performer buttons move into a single
/// "المزيد" select under the row. Returns the rows and the links that were dropped.
pub fn card_rows(
    buttons: Vec<serenity::CreateButton>,
) -> (Vec<serenity::CreateActionRow>, Vec<String>) {
    const MAX: usize = 5;
    let json = |b: &serenity::CreateButton| serde_json::to_value(b).unwrap_or_default();
    if buttons.len() <= MAX {
        return (
            vec![serenity::CreateActionRow::Buttons(buttons)],
            Vec::new(),
        );
    }
    let is_link = |b: &serenity::CreateButton| json(b)["style"] == 5;
    let optional = |b: &serenity::CreateButton| {
        json(b)["custom_id"]
            .as_str()
            .is_some_and(|id| id.starts_with(SHOW_CREW) || id.starts_with(SHOW_PERFORMER))
    };
    let mut kept = buttons;
    let mut links = Vec::new();
    while kept.len() > MAX {
        let Some(pos) = kept.iter().rposition(is_link) else {
            break;
        };
        links.push(
            json(&kept.remove(pos))["url"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        );
    }
    if kept.len() <= MAX {
        return (vec![serenity::CreateActionRow::Buttons(kept)], links);
    }
    let (more, main): (Vec<_>, Vec<_>) = kept.into_iter().partition(|b| optional(b));
    let mut rows = vec![serenity::CreateActionRow::Buttons(main)];
    let choices: Vec<crate::ui::select::Choice> = more
        .iter()
        .map(|b| {
            let j = json(b);
            crate::ui::select::Choice {
                label: j["label"].as_str().unwrap_or("").to_string(),
                description: None,
                value: j["custom_id"].as_str().unwrap_or("").to_string(),
            }
        })
        .collect();
    rows.extend(crate::ui::select::select_row(
        PICK_MORE, LABEL_MORE, choices, true,
    ));
    (rows, links)
}

/// "رجوع": every view but the first of a message carries it.
pub fn back_button() -> serenity::CreateButton {
    serenity::CreateButton::new(BACK)
        .label(LABEL_BACK)
        .emoji('↩')
        .style(serenity::ButtonStyle::Secondary)
}

/// Add the Back button to a view's rows. It joins the first row of buttons that has room; otherwise it gets a row of
/// its own. It is never dropped (the builders keep their rows within four so that this always fits).
pub fn with_back(mut rows: Vec<serenity::CreateActionRow>) -> Vec<serenity::CreateActionRow> {
    let json = |r: &serenity::CreateActionRow| serde_json::to_value(r).unwrap_or_default();
    let room = rows.iter().position(|r| {
        let j = json(r);
        j["components"][0]["type"] == 2 && j["components"].as_array().is_some_and(|c| c.len() < 5)
    });
    match room {
        Some(i) => {
            if let serenity::CreateActionRow::Buttons(b) = &mut rows[i] {
                b.push(back_button());
            }
        }
        None => rows.push(serenity::CreateActionRow::Buttons(vec![back_button()])),
    }
    rows
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

/// A menu of items of any kind (the general search): choosing one opens its card as a new message.
pub fn any_menu(
    placeholder: &str,
    items: &[(String, String)],
) -> Option<serenity::CreateActionRow> {
    menu(PICK_ANY, placeholder, items)
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
    ("NotifyView", ViewKind::None),
    ("AiringView", ViewKind::List),
    ("Arena", ViewKind::Event),
    ("Versus", ViewKind::Event),
    ("PickAddView", ViewKind::List),
    // A built view (person card, search results…) sent as the reply of a command: it carries its own buttons.
    ("MsgView", ViewKind::None),
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
            vec!["أظهر الشخصيات", "أظهر الطاقم"]
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

    #[test]
    fn people_ids_parse_back_and_are_global() {
        let person = "anilist-staff:95";
        assert_eq!(
            parse(&show_works_id(person), &[]),
            Some(Press::ShowWorks(person.into(), None))
        );
        assert_eq!(
            parse(&works_nav_id(person, 2), &[]),
            Some(Press::ShowWorks(person.into(), Some(2)))
        );
        assert_eq!(
            parse(&show_crew_id("tmdb-tv:5"), &[]),
            Some(Press::ShowCrew("tmdb-tv:5".into(), None))
        );
        assert_eq!(
            parse(&crew_nav_id("anilist:1", 1), &[]),
            Some(Press::ShowCrew("anilist:1".into(), Some(1)))
        );
        assert_eq!(
            parse(&show_performer_id("12"), &[]),
            Some(Press::ShowPerformer("12".into()))
        );
        assert_eq!(
            parse(PICK_PERSON, &["tmdb-person:7".into()]),
            Some(Press::PickPerson("tmdb-person:7".into()))
        );
        assert_eq!(
            parse(PICK_PERSON, &["anilist:7".into()]),
            None,
            "only person keys open a person"
        );
        assert_eq!(
            parse(&show_works_id("anilist:1"), &[]),
            None,
            "a work key is not a person"
        );
        assert_eq!(
            parse(&show_crew_id("anilist-staff:1"), &[]),
            None,
            "a person key is not a work"
        );
        for id in [
            show_works_id(person),
            show_crew_id("igdb:4"),
            show_performer_id("9"),
            PICK_PERSON.to_string(),
            PICK_MORE.to_string(),
        ] {
            assert!(is_global_id(&id), "{id}");
            assert!(id.len() <= 100);
        }
    }

    fn row_buttons(rows: &[serenity::CreateActionRow]) -> Vec<Vec<serde_json::Value>> {
        rows.iter()
            .filter_map(|r| {
                let j = serde_json::to_value(r).ok()?;
                let c = j["components"].as_array()?.clone();
                (j["components"][0]["type"] == 2).then_some(c)
            })
            .collect()
    }

    #[test]
    fn a_crowded_card_drops_links_first_then_moves_optional_buttons_into_the_more_menu() {
        let link = serenity::CreateButton::new_link("https://example.org").label("🔗 مصدر");
        let plain = |id: &str, label: &str| serenity::CreateButton::new(id).label(label);
        // six buttons: only the link has to go
        let six = vec![
            plain(&show_chars_id("anilist:1"), LABEL_SHOW_CHARS),
            plain(&show_crew_id("anilist:1"), LABEL_SHOW_CREW),
            plain("add", "add"),
            plain("remind", "remind"),
            link.clone(),
            plain("close", "close"),
        ];
        let (rows, links) = card_rows(six);
        assert_eq!(links, vec!["https://example.org".to_string()]);
        let b = row_buttons(&rows);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].len(), 5);
        assert!(b[0].iter().any(|x| x["label"] == LABEL_SHOW_CREW));
        // seven without a link: the crew button moves into the "المزيد" menu, the required ones stay
        let seven = vec![
            plain(&show_chars_id("anilist:1"), LABEL_SHOW_CHARS),
            plain(&show_crew_id("anilist:1"), LABEL_SHOW_CREW),
            plain("a", "a"),
            plain("b", "b"),
            performer_button("7"),
            plain("c", "c"),
            plain("d", "d"),
        ];
        let (rows, links) = card_rows(seven);
        assert!(links.is_empty());
        let b = row_buttons(&rows);
        assert!(b[0].len() <= 5);
        assert!(
            b[0].iter().any(|x| x["label"] == LABEL_SHOW_CHARS),
            "show characters is never moved"
        );
        assert!(b[0].iter().all(|x| x["label"] != LABEL_SHOW_CREW));
        let menu = serde_json::to_value(rows.last().expect("menu row")).expect("json");
        assert_eq!(menu["components"][0]["custom_id"], PICK_MORE);
        assert_eq!(menu["components"][0]["placeholder"], LABEL_MORE);
        let value = menu["components"][0]["options"][0]["value"]
            .as_str()
            .unwrap_or("")
            .to_string();
        assert_eq!(
            parse(PICK_MORE, &[value]),
            Some(Press::ShowCrew("anilist:1".into(), None))
        );
        // five or fewer: untouched
        let (rows, links) = card_rows(vec![plain("x", "x")]);
        assert!(links.is_empty() && rows.len() == 1);
    }

    #[test]
    fn the_universal_buttons_cover_people() {
        // a work: characters and crew; a character: its work (and the performer where known); a person: works
        assert_eq!(
            labels(standard_actions(ItemKind::Work, Some("igdb:1"))),
            vec![LABEL_SHOW_CHARS, LABEL_SHOW_CREW]
        );
        assert_eq!(
            labels(vec![performer_button("5")]),
            vec![LABEL_SHOW_PERFORMER]
        );
        assert_eq!(
            labels(vec![works_button("tmdb-person:1")]),
            vec![LABEL_SHOW_WORKS]
        );
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
