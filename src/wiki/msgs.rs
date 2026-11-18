//! How wiki pages are shown: a card for a character or a game, a list when several match, and the roster of a game.
//! Wiki data carries no rating or popularity, so none is shown (never invented); the footer names the wiki and its
//! license, and the title links to the page (CC BY-SA attribution).

use super::{service::Wikis, Kind, Page, Source};
use crate::{
    components,
    global::Msg,
    i18n,
    ui::embeds::{fit, Emb},
};
use poise::serenity_prelude as serenity;

/// The address of a page on its wiki: `https://host/wiki/Title_With_Underscores`.
pub fn page_url(host: &str, title: &str) -> String {
    let mut u = reqwest::Url::parse(&format!("https://{host}/"))
        .unwrap_or_else(|_| reqwest::Url::parse("https://invalid.example/").expect("static url"));
    if let Ok(mut seg) = u.path_segments_mut() {
        seg.push("wiki").push(&title.replace(' ', "_"));
    }
    u.to_string()
}

/// "Silent Hill Wiki · CC BY-SA 3.0": what the footer says when wiki data is used.
pub fn credit(w: &Source) -> String {
    match &w.license_name {
        Some(l) => format!("{} · {l}", w.name),
        None => w.name.clone(),
    }
}

/// The card of a wiki page. Characters follow the simplified game-character card (the game, then the name and text);
/// there is no rating or popularity line.
pub fn card(p: &Page, w: &Source) -> Emb {
    let mut lines = Vec::new();
    if let Some(parent) = &p.parent_title {
        lines.push(format!("📚 {parent}"));
    }
    if let Some(e) = &p.extract {
        lines.push(fit(e, 700));
    }
    if let Some(l) = &w.license_url {
        lines.push(format!("📄 {l}"));
    }
    let mut e = Emb::new()
        .title(fit(&p.title, 256))
        .url(Some(page_url(&w.host, &p.title)))
        .description(lines.join("\n\n"))
        .footer(credit(w));
    e.image = p.image_url.clone();
    e
}

/// The wiki page of the game a character belongs to, in the same wiki, if it is known.
async fn game_key(wikis: &Wikis, p: &Page) -> Option<String> {
    let parent = p.parent_title.as_deref()?;
    let norm = super::normalize(super::base_title(parent));
    let now = chrono::Utc::now().timestamp();
    super::store::by_title(&wikis.pool, &norm, Some(Kind::Game), now)
        .await
        .ok()?
        .into_iter()
        .find(|g| g.wiki_id == p.wiki_id)
        .map(|g| g.key())
}

/// The buttons under a card: a character carries "أظهر العمل" when its game page is known, a game carries
/// "أظهر الشخصيات"; the page link comes last (it is the first thing to go in a crowded row).
pub fn rows(p: &Page, w: &Source, game: Option<&str>) -> Vec<serenity::CreateActionRow> {
    let mut buttons = Vec::new();
    match p.kind {
        Kind::Character => buttons.extend(components::standard_actions(
            components::ItemKind::Character,
            game,
        )),
        Kind::Game => {
            if w.character_category.is_some() {
                // The crew button is not offered: a wiki has no crew list.
                buttons.extend(
                    components::standard_actions(components::ItemKind::Work, Some(&p.key()))
                        .into_iter()
                        .take(1),
                );
            }
        }
        Kind::Other => {}
    }
    buttons.push(
        serenity::CreateButton::new_link(page_url(&w.host, &p.title))
            .label(format!("🔗 {}", fit(&w.name, 60))),
    );
    components::card_rows(buttons).0
}

pub async fn page_msg(wikis: &Wikis, wiki_id: i64, page_id: i64) -> Msg {
    let Some((p, w)) = wikis.page(wiki_id, page_id).await else {
        return Msg::text(i18n::SHOW_WORK_UNAVAILABLE);
    };
    let game = game_key(wikis, &p).await;
    Msg::card(card(&p, &w), rows(&p, &w, game.as_deref()))
}

/// Several pages matched: a short list with a menu that opens each one in place.
pub fn list_msg(
    pages: &[Page],
    names: &std::collections::HashMap<i64, String>,
    note: Option<String>,
) -> Msg {
    let label = |p: &Page| {
        let wiki = names.get(&p.wiki_id).map(String::as_str).unwrap_or("");
        match (&p.parent_title, p.kind) {
            (Some(g), Kind::Character) => format!("{} — {g} ({wiki})", p.title),
            _ => format!("{} ({wiki})", p.title),
        }
    };
    let shown: Vec<&Page> = pages.iter().take(25).collect();
    let lines: Vec<String> = shown.iter().map(|p| format!("• {}", label(p))).collect();
    let items: Vec<(String, String)> = shown
        .iter()
        .map(|p| (fit(&label(p), 100), p.key()))
        .collect();
    let mut text = lines.join("\n");
    if let Some(n) = note {
        text.push_str(&format!("\n\n{n}"));
    }
    Msg::card(
        Emb::new()
            .title(i18n::WIKI_PICK_TITLE)
            .description(fit(&text, 3800)),
        components::any_menu(i18n::PICK_ANY_PLACEHOLDER, &items)
            .into_iter()
            .collect(),
    )
}

/// What to answer for the pages a search found: nothing (`None`), the card when one page matched or the typed name is
/// exactly one title, otherwise a list. A note names the wikis that did not answer.
pub async fn show(wikis: &Wikis, found: super::service::Found, typed: &str) -> Option<Msg> {
    let note = (!found.failed.is_empty()).then(|| {
        format!(
            "⚠️ لم يصل رد من {} الآن، فالنتائج ناقصة.",
            found.failed.join(" و")
        )
    });
    let pages = found.pages;
    let exact: Vec<&Page> = pages
        .iter()
        .filter(|p| super::normalize(&p.title) == super::normalize(typed))
        .collect();
    let one = match (pages.as_slice(), exact.as_slice()) {
        ([], _) => return None,
        ([only], _) => Some(only),
        (_, [only]) => Some(*only),
        _ => None,
    };
    if let Some(p) = one {
        let mut m = page_msg(wikis, p.wiki_id, p.page_id).await;
        if let (Some(n), None) = (note, m.text.as_ref()) {
            m.text = Some(n);
        }
        return Some(m);
    }
    let names = wikis
        .sources()
        .await
        .iter()
        .map(|w| (w.id, w.name.clone()))
        .collect();
    Some(list_msg(&pages, &names, note))
}

/// The characters of a wiki game as a menu that opens each one.
pub async fn roster_msg(wikis: &Wikis, wiki_id: i64, page_id: i64) -> Msg {
    let Some((game, w)) = wikis.page(wiki_id, page_id).await else {
        return Msg::text(i18n::SHOW_WORK_UNAVAILABLE);
    };
    let list = wikis.characters_of(wiki_id, &game.title).await;
    if list.is_empty() {
        return Msg::text(i18n::game_without_characters(&game.title));
    }
    let lines: Vec<String> = list
        .iter()
        .take(25)
        .map(|p| format!("**{}**", p.title))
        .collect();
    let items: Vec<(String, String)> = list
        .iter()
        .take(25)
        .map(|p| (fit(&p.title, 100), p.key()))
        .collect();
    Msg::card(
        Emb::new()
            .title(format!("{} — {}", i18n::TITLE_CHARACTERS, game.title))
            .description(lines.join("\n"))
            .footer(credit(&w)),
        components::character_menu(i18n::PICK_CHARACTER_PLACEHOLDER, &items)
            .into_iter()
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wiki() -> Source {
        Source {
            id: 1,
            name: "Silent Hill Wiki".into(),
            host: "silenthill.fandom.com".into(),
            api_url: "https://silenthill.fandom.com/api.php".into(),
            lang: "en".into(),
            license_name: Some("CC BY-SA 3.0".into()),
            license_url: Some("https://creativecommons.org/licenses/by-sa/3.0/".into()),
            character_category: Some("Category:Characters".into()),
            game_category: None,
            enabled: true,
            added_by: None,
            created_at: 0,
            last_checked_at: None,
            last_error: None,
        }
    }

    fn page(kind: Kind) -> Page {
        Page {
            wiki_id: 1,
            page_id: 10,
            title: "Pyramid Head".into(),
            kind,
            extract: Some("A monster that carries a great knife.".into()),
            image_url: Some("https://static.wikia.nocookie.net/silent/a.png".into()),
            aliases: vec![],
            parent_title: Some("Silent Hill 2".into()),
            categories: vec![],
        }
    }

    fn json<T: serde::Serialize>(v: &T) -> serde_json::Value {
        serde_json::to_value(v).expect("json")
    }

    #[test]
    fn page_addresses_use_underscores_and_escape_what_needs_escaping() {
        assert_eq!(
            page_url("silenthill.fandom.com", "Pyramid Head"),
            "https://silenthill.fandom.com/wiki/Pyramid_Head"
        );
        assert_eq!(
            page_url("x.wiki.gg", "A/B?c"),
            "https://x.wiki.gg/wiki/A%2FB%3Fc"
        );
    }

    #[test]
    fn a_wiki_card_names_the_game_credits_the_wiki_and_invents_no_numbers() {
        let e = json(&card(&page(Kind::Character), &wiki()).build());
        let text = e["description"].as_str().unwrap_or("");
        assert!(
            text.contains("📚 Silent Hill 2") && text.contains("A monster"),
            "{text}"
        );
        assert_eq!(e["footer"]["text"], "Silent Hill Wiki · CC BY-SA 3.0");
        assert_eq!(e["url"], "https://silenthill.fandom.com/wiki/Pyramid_Head");
        assert_eq!(
            e["image"]["url"],
            "https://static.wikia.nocookie.net/silent/a.png"
        );
        assert!(
            text.contains("creativecommons.org"),
            "the license is linked"
        );
        for number in ["⭐", "❤", "★", "/10", "%"] {
            assert!(!text.contains(number), "no rating or popularity: {number}");
        }
    }

    #[test]
    fn a_character_carries_show_work_when_its_game_is_known_and_a_game_carries_show_characters() {
        let labels = |rows: &[serenity::CreateActionRow]| -> Vec<String> {
            rows.iter()
                .flat_map(|r| {
                    json(r)["components"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                })
                .map(|c| c["label"].as_str().unwrap_or("").to_string())
                .collect()
        };
        let with_game = labels(&rows(&page(Kind::Character), &wiki(), Some("wiki:1:5")));
        assert_eq!(with_game, vec!["أظهر العمل", "🔗 Silent Hill Wiki"]);
        let game_page = Page {
            kind: Kind::Game,
            ..page(Kind::Game)
        };
        assert_eq!(
            labels(&rows(&game_page, &wiki(), None)),
            vec!["أظهر الشخصيات", "🔗 Silent Hill Wiki"],
            "no crew button for a wiki"
        );
        let mut search_only = wiki();
        search_only.character_category = None;
        assert_eq!(
            labels(&rows(&game_page, &search_only, None)),
            vec!["🔗 Silent Hill Wiki"],
            "no roster without a character category"
        );
        assert_eq!(
            json(&rows(&page(Kind::Character), &wiki(), Some("wiki:1:5"))[0])["components"][0]
                ["custom_id"],
            "showwork:wiki:1:5"
        );
    }

    #[test]
    fn the_list_has_a_menu_of_keys_and_says_which_wiki_each_page_is_from() {
        let pages: Vec<Page> = (0..30)
            .map(|i| Page {
                page_id: i + 1,
                title: format!("Page {i}"),
                ..page(Kind::Character)
            })
            .collect();
        let names = std::collections::HashMap::from([(1, "Silent Hill Wiki".to_string())]);
        let m = list_msg(&pages, &names, Some("⚠️ note".into()));
        let menu = json(&m.rows[0]);
        assert_eq!(menu["components"][0]["custom_id"], components::PICK_ANY);
        let options = menu["components"][0]["options"]
            .as_array()
            .expect("options");
        assert_eq!(options.len(), 25);
        assert_eq!(options[0]["value"], "wiki:1:1");
        assert!(options[0]["label"]
            .as_str()
            .unwrap_or("")
            .contains("(Silent Hill Wiki)"));
        assert!(json(&m.embed.expect("embed"))["description"]
            .as_str()
            .unwrap_or("")
            .contains("note"));
    }
}
