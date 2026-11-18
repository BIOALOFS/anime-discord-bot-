//! Edit-in-place navigation: opening a work, character, person or list replaces the view on the same message and adds a
//! "رجوع" button that returns to the previous view. `NavState` is the logic of that for one message (the Back stack and
//! what a press means); the session engine and the private sessions of other members both use it.

use super::stack::{Desc, Trail};
use crate::{
    components::{self, Press},
    global::{self, Msg},
    ui::session::{Outcome, Page, View},
    Data,
};

/// What a press on a navigation component does to the message.
pub enum Step {
    /// Show this view (with Back when the state says so).
    Show(Box<Msg>),
    /// Show the first view of the message again (Back from the first overlay).
    Root,
    /// The press acted for the presser (reminder, add, status): answer privately with this text, change nothing.
    Act(String),
    /// Nothing to do (stale, unknown or nothing to go back to): acknowledge silently.
    Ignore,
}

pub struct NavState {
    pub trail: Trail,
    /// Views that are not left by Back: 0 for a public message (its own first view is the root), 1 for a private view
    /// whose first view is the thing that was asked for.
    floor: usize,
}

impl NavState {
    pub fn new() -> Self {
        Self {
            trail: Trail::default(),
            floor: 0,
        }
    }

    /// A private view of another member: the requested view is its root.
    pub fn private(first: Desc) -> Self {
        let mut trail = Trail::default();
        trail.open(first);
        Self { trail, floor: 1 }
    }

    pub fn has_back(&self) -> bool {
        self.trail.depth() > self.floor
    }

    /// Is a view other than the message's first one on screen?
    pub fn in_overlay(&self) -> bool {
        self.trail.depth() > 0
    }

    pub async fn current_msg(&self, data: &Data, user: i64, guild: Option<i64>) -> Option<Msg> {
        let d = self.trail.current()?;
        let press = components::parse(&d.id, &d.values)?;
        global::view_msg(data, &press, user, guild).await
    }

    pub async fn press(
        &mut self,
        data: &Data,
        id: &str,
        values: &[String],
        user: i64,
        guild: Option<i64>,
    ) -> Step {
        let Some(press) = components::parse(id, values) else {
            return Step::Ignore;
        };
        if press == Press::Back {
            if !self.has_back() {
                return Step::Ignore;
            }
            self.trail.back();
            if self.trail.current().is_none() {
                return Step::Root;
            }
            return match self.current_msg(data, user, guild).await {
                Some(m) => Step::Show(Box::new(m)),
                None => Step::Ignore,
            };
        }
        if let Some(text) = global::act_text(data, &press, user, guild).await {
            return Step::Act(text);
        }
        let Some(msg) = global::view_msg(data, &press, user, guild).await else {
            return Step::Ignore;
        };
        let desc = Desc::new(id, values);
        if global::replaces_view(&press) {
            self.trail.replace(desc);
        } else {
            self.trail.open(desc);
        }
        Step::Show(Box::new(msg))
    }
}

impl Default for NavState {
    fn default() -> Self {
        Self::new()
    }
}

/// A view as a page of the engine; `back` adds the "رجوع" button.
pub fn page_of(msg: Msg, back: bool) -> Page {
    let rows = if back {
        components::with_back(msg.rows)
    } else {
        msg.rows
    };
    Page {
        content: msg.text,
        embeds: msg.embed.into_iter().collect(),
        components: rows,
        files: msg.files,
        v2: None,
    }
}

/// A message that is only a built view (a person card, a search result…): the engine gives it the same navigation as
/// any other message.
pub struct MsgView(pub Msg);

impl View for MsgView {
    fn render(&self, _enabled: bool) -> Page {
        page_of(self.0.clone(), false)
    }

    async fn on_interaction(&mut self, _id: &str, _values: &[String]) -> Outcome {
        Outcome::Keep
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poise::serenity_prelude as serenity;

    fn msg_with_buttons(n: usize) -> Msg {
        let buttons = (0..n)
            .map(|i| serenity::CreateButton::new(format!("showwork:anilist:{i}")).label("x"))
            .collect();
        Msg {
            text: Some("hello".into()),
            rows: vec![serenity::CreateActionRow::Buttons(buttons)],
            ..Msg::default()
        }
    }

    fn ids(page: &Page) -> Vec<String> {
        serde_json::to_value(&page.components)
            .expect("json")
            .as_array()
            .expect("rows")
            .iter()
            .flat_map(|r| r["components"].as_array().cloned().unwrap_or_default())
            .map(|c| c["custom_id"].as_str().unwrap_or("").to_string())
            .collect()
    }

    #[test]
    fn a_non_root_view_gets_back_and_the_root_does_not() {
        let root = page_of(msg_with_buttons(2), false);
        assert!(!ids(&root).contains(&components::BACK.to_string()));
        let view = page_of(msg_with_buttons(2), true);
        assert!(ids(&view).contains(&components::BACK.to_string()));
        assert_eq!(
            view.content.as_deref(),
            Some("hello"),
            "the content is kept"
        );
    }

    #[test]
    fn back_is_never_dropped_even_when_the_buttons_row_is_full() {
        let page = page_of(msg_with_buttons(5), true);
        assert!(ids(&page).contains(&components::BACK.to_string()));
        let j = serde_json::to_value(&page.components).expect("json");
        assert!(j.as_array().expect("rows").len() <= 5);
        assert!(j
            .as_array()
            .expect("rows")
            .iter()
            .all(|r| r["components"].as_array().is_some_and(|c| c.len() <= 5)));
        // with only a select menu there is no row of buttons: Back gets a row of its own
        let menu = Msg {
            rows: components::work_menu("p", &[("t".into(), "anilist:1".into())])
                .into_iter()
                .collect(),
            ..Msg::default()
        };
        let page = page_of(menu, true);
        assert!(ids(&page).contains(&components::BACK.to_string()));
        assert_eq!(
            serde_json::to_value(&page.components)
                .expect("json")
                .as_array()
                .map(Vec::len),
            Some(2)
        );
    }

    #[test]
    fn a_private_view_starts_without_back_and_gains_it_after_opening_something() {
        let mut s = NavState::private(Desc::new("showwork:anilist:1", &[]));
        assert!(
            !s.has_back(),
            "the view that was asked for is the root of a private message"
        );
        s.trail.open(Desc::new("showchars:anilist:1", &[]));
        assert!(s.has_back());
        s.trail.back();
        assert!(!s.has_back());
        let public = NavState::new();
        assert!(!public.has_back() && !public.in_overlay());
    }

    fn anilist(
        query: &str,
        _vars: &serde_json::Value,
    ) -> Result<serde_json::Value, crate::anilist::AniError> {
        let load = |name: &str| -> serde_json::Value {
            let path = format!(
                "{}/tests/fixtures/anilist/{name}.json",
                env!("CARGO_MANIFEST_DIR")
            );
            serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("json")
        };
        Ok(if query.contains("Character(id") {
            load("character_safe")["data"].clone()
        } else if query.contains("characters(page") {
            load("media_characters")["data"].clone()
        } else {
            serde_json::json!({"Media": load("search_media")["data"]["Page"]["media"][0].clone()})
        })
    }

    #[tokio::test]
    async fn opening_views_edits_in_place_and_back_returns_through_the_stack_to_the_first_view() {
        let data = Data::for_tests(anilist).await;
        let mut nav = NavState::new();
        let none: Vec<String> = Vec::new();
        // the first view is a work card (not in the stack); "أظهر الشخصيات" opens its roster in the same message
        let Step::Show(roster) = nav
            .press(&data, "showchars:anilist:1", &none, 7, None)
            .await
        else {
            panic!("a roster view");
        };
        assert!(nav.has_back() && nav.in_overlay());
        let page = page_of(*roster, nav.has_back());
        assert!(
            ids(&page).contains(&components::BACK.to_string()),
            "every non-root view has Back"
        );
        // a character picked from the roster: a second level
        let Step::Show(_) = nav
            .press(&data, components::PICK_CHAR, &["10".to_string()], 7, None)
            .await
        else {
            panic!("a character view");
        };
        assert_eq!(nav.trail.depth(), 2);
        // Back, Back: down to the roster, then the first view
        assert!(matches!(
            nav.press(&data, components::BACK, &none, 7, None).await,
            Step::Show(_)
        ));
        assert!(matches!(
            nav.press(&data, components::BACK, &none, 7, None).await,
            Step::Root
        ));
        assert!(!nav.has_back());
        assert!(
            matches!(
                nav.press(&data, components::BACK, &none, 7, None).await,
                Step::Ignore
            ),
            "nothing to go back to"
        );
    }

    #[tokio::test]
    async fn a_page_button_replaces_the_list_on_screen() {
        let data = Data::for_tests(anilist).await;
        let mut nav = NavState::new();
        let none: Vec<String> = Vec::new();
        assert!(matches!(
            nav.press(&data, "showchars:anilist:1", &none, 7, None)
                .await,
            Step::Show(_)
        ));
        assert!(matches!(
            nav.press(&data, "showchars:anilist:1:1:nav", &none, 7, None)
                .await,
            Step::Show(_)
        ));
        assert_eq!(nav.trail.depth(), 1, "paging does not add a Back level");
    }

    #[tokio::test]
    async fn presses_that_act_answer_privately_and_change_nothing_and_unknown_ids_are_ignored() {
        let data = Data::for_tests(anilist).await;
        let mut nav = NavState::new();
        let none: Vec<String> = Vec::new();
        assert!(matches!(
            nav.press(&data, "remind:igdb:5", &none, 7, None).await,
            Step::Act(_)
        ));
        assert_eq!(nav.trail.depth(), 0, "an action never navigates");
        assert!(matches!(
            nav.press(&data, "close", &none, 7, None).await,
            Step::Ignore
        ));
        assert!(
            matches!(
                nav.press(&data, "showwork:broken", &none, 7, None).await,
                Step::Ignore
            ),
            "a malformed id is a silent no-op"
        );
    }

    #[tokio::test]
    async fn a_private_view_cannot_go_back_past_its_first_view() {
        let data = Data::for_tests(anilist).await;
        let none: Vec<String> = Vec::new();
        let mut nav = NavState::private(Desc::new("showwork:anilist:1", &none));
        assert!(matches!(
            nav.press(&data, components::BACK, &none, 7, None).await,
            Step::Ignore
        ));
        assert!(matches!(
            nav.press(&data, "showchars:anilist:1", &none, 7, None)
                .await,
            Step::Show(_)
        ));
        assert!(nav.has_back());
        assert!(
            matches!(
                nav.press(&data, components::BACK, &none, 7, None).await,
                Step::Show(_)
            ),
            "back to the requested view"
        );
        assert!(!nav.has_back());
    }

    /// Every view a navigation press can open is a non-root view: with Back it always fits Discord's limits (five rows,
    /// five buttons a row, 25 options a menu) and the required buttons stay.
    #[tokio::test]
    async fn every_navigation_view_gets_back_and_stays_within_discords_limits() {
        let data = Data::for_tests(anilist).await;
        let none: Vec<String> = Vec::new();
        let presses: Vec<(&str, Vec<String>)> = vec![
            ("showwork:anilist:1", none.clone()),
            ("showchars:anilist:1", none.clone()),
            ("showchars:anilist:1:1:nav", none.clone()),
            (components::PICK_CHAR, vec!["10".to_string()]),
            (components::PICK_WORK, vec!["anilist:1".to_string()]),
            (components::PICK_ANY, vec!["anilist:1".to_string()]),
        ];
        let mut msgs: Vec<Msg> = Vec::new();
        for (id, values) in &presses {
            let press = components::parse(id, values).expect("a press");
            msgs.push(
                global::view_msg(&data, &press, 7, None)
                    .await
                    .expect("a view"),
            );
        }
        // the people views
        let credits: Vec<crate::people::Credit> = (0..60)
            .map(|i| crate::people::Credit {
                work_key: Some(format!("anilist:{}", i + 1)),
                title: format!("Work {i}"),
                year: Some(2000),
                role: None,
                character: Some((format!("{}", 100 + i), format!("Char {i}"))),
            })
            .collect();
        let person = crate::people::Person {
            key: "anilist-staff:95".into(),
            name: "Kamiya".into(),
            credits,
            url: Some("https://anilist.co/staff/95".into()),
            ..crate::people::Person::default()
        };
        msgs.push(crate::people::msgs::works_page(&person, 1));
        msgs.push(Msg {
            embed: None,
            rows: crate::people::msgs::person_rows(&person, false),
            text: Some("person".into()),
            files: Vec::new(),
        });
        let entries: Vec<crate::people::CrewEntry> = (0..40)
            .map(|i| crate::people::CrewEntry {
                person: crate::people::Hit {
                    key: format!("tmdb-person:{i}"),
                    name: format!("P{i}"),
                    native: None,
                    hint: String::new(),
                    popularity: 0,
                },
                role: "ممثل".into(),
            })
            .collect();
        msgs.push(crate::people::msgs::crew_page("tmdb-tv:1", &entries, 0));
        for msg in msgs {
            let page = page_of(msg, true);
            let rows = serde_json::to_value(&page.components).expect("json");
            let rows = rows.as_array().expect("rows");
            assert!(rows.len() <= 5, "{} rows", rows.len());
            let mut has_back = false;
            for r in rows {
                let c = r["components"].as_array().expect("components");
                assert!(c.len() <= 5);
                has_back |= c.iter().any(|x| x["custom_id"] == components::BACK);
                for x in c {
                    if let Some(o) = x["options"].as_array() {
                        assert!(o.len() <= 25);
                    }
                }
            }
            assert!(has_back, "a non-root view without Back");
        }
    }
}
