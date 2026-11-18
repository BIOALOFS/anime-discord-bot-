//! The messages of people: the person card, the list of a person's works, the crew of a work and the performers of a
//! character. They are answered as NEW messages by the global handlers; page buttons edit their own message.

use super::fetch::{self, Card, Sources};
use super::{CrewEntry, Hit, Person};
use crate::{
    components,
    global::Msg,
    i18n,
    ui::embeds::{fit, Emb},
    Data,
};
use poise::serenity_prelude as serenity;

/// Entries per page (one select menu holds 25).
pub const PAGE: usize = 25;

/// The slice of `items` for `page` (clamped), and the page count.
pub fn paged<T>(items: &[T], page: usize) -> (&[T], usize, usize) {
    let pages = items.len().div_ceil(PAGE).max(1);
    let page = page.min(pages - 1);
    (
        &items[page * PAGE..((page + 1) * PAGE).min(items.len())],
        page,
        pages,
    )
}

fn nav_row(id: impl Fn(usize) -> String, page: usize, pages: usize) -> serenity::CreateActionRow {
    let btn = |label: &str, to: usize, off: bool| {
        serenity::CreateButton::new(id(to))
            .label(label)
            .style(serenity::ButtonStyle::Secondary)
            .disabled(off)
    };
    serenity::CreateActionRow::Buttons(vec![
        btn("◀", page.saturating_sub(1), page == 0),
        serenity::CreateButton::new("nav:indicator")
            .label(format!("{}/{}", page + 1, pages))
            .style(serenity::ButtonStyle::Secondary)
            .disabled(true),
        btn("▶", (page + 1).min(pages - 1), page + 1 >= pages),
    ])
}

fn unavailable(card: Card) -> Msg {
    match card {
        Card::Down => Msg::text(i18n::PERSON_SOURCE_DOWN),
        _ => Msg::text(i18n::PERSON_UNAVAILABLE),
    }
}

/// The details lines of a card (roles, birth and age, place, popularity).
pub fn info_lines(p: &Person) -> Vec<String> {
    let mut lines = Vec::new();
    if !p.roles.is_empty() {
        lines.push(format!("🎙️ {}", p.roles.join(" · ")));
    }
    let mut when = Vec::new();
    if let Some(b) = &p.birth {
        when.push(format!("🎂 {}: {b}", i18n::F_BORN));
    }
    match (p.age, p.deceased) {
        (Some(a), false) => when.push(i18n::years_old(a)),
        (_, true) => when.push("🕊️".to_string()),
        _ => {}
    }
    if let Some(place) = &p.place {
        when.push(format!("📍 {place}"));
    }
    if !when.is_empty() {
        lines.push(when.join(" · "));
    }
    if let Some(n) = p.popularity.filter(|n| *n > 0) {
        let label = if p.key.starts_with("tmdb-") {
            i18n::PERSON_POPULARITY
        } else {
            i18n::PERSON_FAVOURITES
        };
        lines.push(format!("❤️ {n} {label}"));
    }
    lines
}

/// The embed of a person card.
pub fn person_embed(p: &Person) -> Emb {
    let mut text = info_lines(p).join("\n");
    if let Some(a) = &p.about {
        text.push_str(&format!("\n\n{a}"));
    }
    if !p.credits.is_empty() {
        let preview: Vec<String> = p
            .credits
            .iter()
            .take(5)
            .map(|c| format!("• {}", c.line()))
            .collect();
        text.push_str(&format!(
            "\n\n**{}**\n{}",
            i18n::F_NOTABLE_WORKS,
            preview.join("\n")
        ));
    }
    let title = match &p.native {
        Some(n) if n != &p.name => format!("{} ({n})", p.name),
        _ => p.name.clone(),
    };
    let credit = super::parse_key(&p.key).map_or("", |(s, _)| s.credit());
    Emb::new()
        .title(fit(&title, 256))
        .url(p.url.clone())
        .description(fit(&text, 3800))
        .image(p.image.clone())
        .footer(credit)
}

/// The buttons under a person card: "أظهر الأعمال" (when there are works) and the source link.
pub fn person_rows(p: &Person, in_profile: bool) -> Vec<serenity::CreateActionRow> {
    let mut buttons = Vec::new();
    if !p.credits.is_empty() {
        buttons.push(components::works_button(&p.key));
    }
    // Actors, voice actors, staff: the same add-to-profile button as a character (companies have none).
    if super::addable(&p.key) {
        buttons.push(crate::global::add_button_for(&p.key, in_profile));
    }
    if let Some(u) = p.url.as_deref().filter(|u| u.starts_with("https://")) {
        let credit = super::parse_key(&p.key).map_or("", |(s, _)| s.credit());
        buttons.push(serenity::CreateButton::new_link(u).label(format!("🔗 {credit}")));
    }
    if buttons.is_empty() {
        Vec::new()
    } else {
        components::card_rows(buttons).0
    }
}

pub async fn person_msg(data: &Data, key: &str, mature: bool, user: i64) -> Msg {
    let card = fetch::person(&Sources::of(data), key, mature).await;
    let Card::Found(p) = card else {
        return unavailable(card);
    };
    let mine = crate::global::in_profile(data, user, &p.key).await;
    let rows = person_rows(&p, mine);
    Msg::card(person_embed(&p), rows)
}

/// The works of a person, one page: the list, a menu of the works and (when they played characters) a menu of them.
pub async fn works_msg(data: &Data, key: &str, page: usize, mature: bool) -> Msg {
    let card = fetch::person(&Sources::of(data), key, mature).await;
    let Card::Found(p) = card else {
        return unavailable(card);
    };
    works_page(&p, page)
}

pub fn works_page(p: &Person, page: usize) -> Msg {
    if p.credits.is_empty() {
        return Msg::text(i18n::PERSON_NO_WORKS);
    }
    let (slice, page, pages) = paged(&p.credits, page);
    let lines: Vec<String> = slice
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}. {}", page * PAGE + i + 1, c.line()))
        .collect();
    let works: Vec<(String, String)> = slice
        .iter()
        .filter_map(|c| {
            let key = c.work_key.clone()?;
            let label = match c.year {
                Some(y) => format!("{} ({y})", c.title),
                None => c.title.clone(),
            };
            Some((fit(&label, 100), key))
        })
        .collect();
    let chars: Vec<(String, String)> = slice
        .iter()
        .filter_map(|c| {
            let (key, name) = c.character.clone()?;
            Some((fit(&format!("{name} — {}", c.title), 100), key))
        })
        .collect();
    let mut rows: Vec<_> = components::work_menu(i18n::PICK_WORK_OF_PERSON, &works)
        .into_iter()
        .collect();
    rows.extend(components::character_menu(
        i18n::PICK_CHAR_OF_PERSON,
        &chars,
    ));
    if pages > 1 {
        let key = p.key.clone();
        rows.push(nav_row(
            move |to| components::works_nav_id(&key, to),
            page,
            pages,
        ));
    }
    let credit = super::parse_key(&p.key).map_or("", |(s, _)| s.credit());
    let e = Emb::new()
        .title(fit(&i18n::works_of(&p.name), 256))
        .url(p.url.clone())
        .description(fit(&lines.join("\n"), 4000))
        .footer(format!("{credit} · {}/{pages}", page + 1));
    Msg::card(e, rows)
}

/// The crew of a work, one page, with a menu that opens each person's card.
pub async fn crew_msg(data: &Data, work_key: &str, page: usize, mature: bool) -> Msg {
    let Some(entries) = fetch::crew(&Sources::of(data), work_key, mature).await else {
        return Msg::text(i18n::PERSON_SOURCE_DOWN);
    };
    crew_page(work_key, &entries, page)
}

pub fn crew_page(work_key: &str, entries: &[CrewEntry], page: usize) -> Msg {
    if entries.is_empty() {
        return Msg::text(i18n::NO_CREW);
    }
    let (slice, page, pages) = paged(entries, page);
    let lines: Vec<String> = slice
        .iter()
        .map(|e| format!("**{}** — {}", e.person.name, e.role))
        .collect();
    let items: Vec<(String, String)> = slice
        .iter()
        .map(|e| {
            (
                fit(&format!("{} — {}", e.person.name, e.role), 100),
                e.person.key.clone(),
            )
        })
        .collect();
    let mut rows: Vec<_> = components::person_menu(i18n::PICK_PERSON_PLACEHOLDER, &items)
        .into_iter()
        .collect();
    if pages > 1 {
        let key = work_key.to_string();
        rows.push(nav_row(
            move |to| components::crew_nav_id(&key, to),
            page,
            pages,
        ));
    }
    let e = Emb::new()
        .title(i18n::crew_of(""))
        .description(fit(&lines.join("\n"), 4000))
        .footer(format!("{}/{pages}", page + 1));
    Msg::card(e, rows)
}

/// Several people matched a typed name: a short list with a menu that opens each card.
pub fn hits_list(hits: &[Hit], note: Option<String>) -> Msg {
    let lines: Vec<String> = hits
        .iter()
        .take(PAGE)
        .map(|h| format!("**{}** — {}", h.name, h.hint))
        .collect();
    let items: Vec<(String, String)> = hits
        .iter()
        .take(PAGE)
        .map(|h| (fit(&h.label(), 100), h.key.clone()))
        .collect();
    let rows = components::person_menu(i18n::PICK_PERSON_PLACEHOLDER, &items)
        .into_iter()
        .collect();
    let mut text = lines.join("\n");
    if let Some(n) = note {
        text.push_str(&format!("\n\n{n}"));
    }
    Msg::card(
        Emb::new().title(i18n::PICK_PERSON_TITLE).description(text),
        rows,
    )
}

/// The performer(s) of a character: one opens the person card directly, several get a menu.
pub async fn performers_msg(data: &Data, character_key: &str, mature: bool, user: i64) -> Msg {
    let hits = fetch::performers(&Sources::of(data), character_key, mature).await;
    match hits.as_slice() {
        [] => Msg::text(i18n::NO_PERFORMER),
        [one] => person_msg(data, &one.key, mature, user).await,
        many => performers_list(many),
    }
}

pub fn performers_list(hits: &[Hit]) -> Msg {
    let lines: Vec<String> = hits.iter().map(|h| format!("**{}**", h.name)).collect();
    let items: Vec<(String, String)> = hits
        .iter()
        .map(|h| (fit(&h.name, 100), h.key.clone()))
        .collect();
    let rows = components::person_menu(i18n::PICK_PERFORMER_PLACEHOLDER, &items)
        .into_iter()
        .collect();
    Msg::card(
        Emb::new()
            .title(i18n::performers_of(""))
            .description(lines.join("\n")),
        rows,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::people::Credit;

    fn person(n: usize) -> Person {
        Person {
            key: "anilist-staff:95".into(),
            name: "Hiroshi Kamiya".into(),
            native: Some("神谷浩史".into()),
            roles: vec!["مؤدي أصوات".into()],
            birth: Some("1975-01-28".into()),
            age: Some(51),
            popularity: Some(12000),
            about: Some("A voice actor.".into()),
            url: Some("https://anilist.co/staff/95".into()),
            place: Some("Chiba".into()),
            credits: (0..n)
                .map(|i| Credit {
                    work_key: Some(format!("anilist:{}", i + 1)),
                    title: format!("Work {i}"),
                    year: Some(2000 + i as i32 % 20),
                    role: None,
                    character: Some((format!("{}", 1000 + i), format!("Char {i}"))),
                })
                .collect(),
            ..Person::default()
        }
    }

    fn json(row: &serenity::CreateActionRow) -> serde_json::Value {
        serde_json::to_value(row).expect("json")
    }

    #[test]
    fn the_person_card_shows_role_birth_age_popularity_works_and_the_image_slot() {
        let e = serde_json::to_value(person_embed(&person(8)).build()).expect("json");
        let text = e["description"].as_str().unwrap_or("");
        for want in [
            "مؤدي أصوات",
            "1975-01-28",
            "51 سنة",
            "Chiba",
            "12000",
            "A voice actor.",
            "Work 0",
            "Char 0",
        ] {
            assert!(text.contains(want), "{want} in {text}");
        }
        assert!(e["title"].as_str().unwrap_or("").contains("神谷浩史"));
        assert_eq!(text.matches("• ").count(), 5, "five works are previewed");
        let mut p = person(1);
        p.image = Some("https://s4.anilist.co/x.png".into());
        let e = serde_json::to_value(person_embed(&p).build()).expect("json");
        assert_eq!(e["image"]["url"], "https://s4.anilist.co/x.png");
    }

    #[test]
    fn the_works_list_pages_with_menus_and_stays_within_discords_limits() {
        let p = person(60);
        let m = works_page(&p, 1);
        assert_eq!(m.rows.len(), 3, "work menu, character menu, page buttons");
        let work_menu = json(&m.rows[0]);
        assert_eq!(
            work_menu["components"][0]["custom_id"],
            components::PICK_WORK
        );
        assert!(work_menu["components"][0]["options"]
            .as_array()
            .is_some_and(|o| o.len() <= 25));
        assert_eq!(
            json(&m.rows[1])["components"][0]["custom_id"],
            components::PICK_CHAR
        );
        let nav = json(&m.rows[2]);
        assert_eq!(nav["components"][1]["label"], "2/3");
        assert_eq!(
            nav["components"][0]["custom_id"],
            "showworks:anilist-staff:95:0:nav"
        );
        assert_eq!(
            nav["components"][2]["custom_id"],
            "showworks:anilist-staff:95:2:nav"
        );
        // a page past the end is clamped
        let last = works_page(&p, 99);
        assert_eq!(json(&last.rows[2])["components"][1]["label"], "3/3");
        // no credits: a polite line
        assert_eq!(
            works_page(&person(0), 0).text.as_deref(),
            Some(i18n::PERSON_NO_WORKS)
        );
    }

    #[test]
    fn menu_values_are_stable_keys_within_100_characters() {
        let m = works_page(&person(30), 0);
        for row in &m.rows[..2] {
            for o in json(row)["components"][0]["options"]
                .as_array()
                .expect("options")
            {
                assert!(o["value"].as_str().unwrap_or("").chars().count() <= 100);
                assert!(o["label"].as_str().unwrap_or("").chars().count() <= 100);
            }
        }
    }

    fn entry(i: usize) -> CrewEntry {
        CrewEntry {
            person: Hit {
                key: format!("tmdb-person:{i}"),
                name: format!("Person {i}"),
                native: None,
                hint: String::new(),
                popularity: 0,
            },
            role: "ممثل".into(),
        }
    }

    #[test]
    fn the_crew_list_pages_and_opens_person_cards_through_a_menu() {
        let entries: Vec<CrewEntry> = (0..40).map(entry).collect();
        let m = crew_page("tmdb-tv:1396", &entries, 0);
        assert_eq!(m.rows.len(), 2);
        assert_eq!(
            json(&m.rows[0])["components"][0]["custom_id"],
            components::PICK_PERSON
        );
        assert_eq!(
            json(&m.rows[1])["components"][1]["custom_id"],
            "nav:indicator"
        );
        assert_eq!(
            json(&m.rows[1])["components"][2]["custom_id"],
            "showcrew:tmdb-tv:1396:1:nav"
        );
        assert_eq!(
            crew_page("tmdb-tv:1", &[], 0).text.as_deref(),
            Some(i18n::NO_CREW)
        );
        let one_page = crew_page("tmdb-tv:1", &entries[..3], 0);
        assert_eq!(one_page.rows.len(), 1, "no page buttons for one page");
    }

    #[test]
    fn several_performers_get_a_menu() {
        let hits: Vec<Hit> = (0..3).map(|i| entry(i).person).collect();
        let m = performers_list(&hits);
        assert_eq!(m.rows.len(), 1);
        assert_eq!(
            json(&m.rows[0])["components"][0]["custom_id"],
            components::PICK_PERSON
        );
    }

    #[test]
    fn the_person_card_carries_show_works_and_the_source_link() {
        let rows = person_rows(&person(3), false);
        let j = json(&rows[0]);
        let labels: Vec<&str> = j["components"]
            .as_array()
            .expect("buttons")
            .iter()
            .map(|b| b["label"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(labels, vec!["أظهر الأعمال", "أضف لبروفايلي", "🔗 AniList"]);
        assert_eq!(
            j["components"][0]["custom_id"],
            "showworks:anilist-staff:95"
        );
        // a person without works has no works button
        let none = person_rows(&person(0), false);
        assert!(json(&none[0])["components"]
            .as_array()
            .expect("buttons")
            .iter()
            .all(|b| b["label"] != "أظهر الأعمال"));
    }

    #[test]
    fn paging_helper_clamps() {
        let v: Vec<u32> = (0..60).collect();
        assert_eq!(paged(&v, 0).2, 3);
        assert_eq!(paged(&v, 9).1, 2);
        assert_eq!(paged(&v[..0], 5), (&v[..0], 0, 1));
    }
}
