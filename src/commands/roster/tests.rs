use super::*;
use serde_json::{json, Value};

const TOTAL: usize = 230;
const HIDDEN_INDEX: usize = 7;

/// A 230-character cast; index 7 is linked to an adult work; AniList pages hold 25.
fn api() -> Arc<SafeAniList> {
    Arc::new(SafeAniList::with_fixture(|_, vars| {
        if let Some(page) = vars["page"].as_u64() {
            let base = (page as usize - 1) * CAST_PAGE_SIZE;
            let edges: Vec<Value> = (base..(base + CAST_PAGE_SIZE).min(TOTAL))
                .map(|i| {
                    json!({
                        "role": if i < 20 { "MAIN" } else { "SUPPORTING" },
                        "node": {
                            "id": 1000 + i, "name": {"full": format!("Char {i}")},
                            "media": {"nodes": [{"isAdult": i == HIDDEN_INDEX, "genres": []}]}
                        }
                    })
                })
                .collect();
            return Ok(json!({"Media": {"characters": {
                "pageInfo": {"total": TOTAL, "lastPage": 10, "hasNextPage": page < 10}, "edges": edges
            }}}));
        }
        // character_by_id
        let id = vars["id"].as_i64().unwrap_or(0);
        Ok(json!({"Character": {
            "id": id, "name": {"full": format!("Full {id}"), "native": "名"},
            "media": {"nodes": [{"id": 1, "title": {"romaji": "Show"}, "isAdult": false, "genres": []}]}
        }}))
    }))
}

fn character(id: i64, works: usize) -> Character {
    let nodes: Vec<Value> = (0..works)
        .map(|w| json!({"id": 10 + w, "title": {"romaji": format!("Work {w}")}, "isAdult": false, "genres": []}))
        .collect();
    serde_json::from_value(json!({"id": id, "name": {"full": "Origin"}, "media": {"nodes": nodes}}))
        .expect("c")
}

async fn roster() -> Roster {
    Roster::start(api(), ContentPolicy::SAFE, &character(1005, 1))
        .await
        .expect("ok")
        .expect("some")
}

#[test]
fn page_math() {
    assert_eq!(pages_of(0), 1);
    assert_eq!(pages_of(10), 1);
    assert_eq!(pages_of(11), 2);
    assert_eq!(pages_of(230), 23);
    assert_eq!(anilist_pages(0), 1..=1);
    // Roster page 2 = entries 20..29 -> AniList page 1 (entries 0-24) and page 2 (25-49).
    assert_eq!(anilist_pages(2), 1..=2);
    assert_eq!(anilist_pages(5), 3..=3);
    assert_eq!(range_label(0, 230), "1–10");
    assert_eq!(range_label(22, 230), "221–230");
}

#[test]
fn ranges_split_into_groups_of_25() {
    assert_eq!(group_count(23), 1, "a 230-character cast fits one menu");
    assert_eq!(group_count(25), 1);
    assert_eq!(group_count(26), 2);
    assert_eq!(group_count(200), 8);
    assert_eq!(pages_in_group(0, 60), 0..25);
    assert_eq!(pages_in_group(2, 60), 50..60);
    assert_eq!(group_of_page(24), 0);
    assert_eq!(group_of_page(25), 1);
    assert_eq!(group_label(1, 2000), "251–500");
    assert_eq!(group_label(7, 2000), "1751–2000");
}

#[test]
fn page_input_is_forgiving_but_bounded() {
    assert_eq!(parse_page_input("12", 23), Some(11));
    assert_eq!(parse_page_input(" ٥ ", 23), Some(4), "Arabic-Indic digits");
    assert_eq!(parse_page_input("۲۳", 23), Some(22), "Persian digits");
    assert_eq!(parse_page_input("1", 23), Some(0));
    assert_eq!(parse_page_input("24", 23), None);
    assert_eq!(parse_page_input("0", 23), None);
    assert_eq!(parse_page_input("abc", 23), None);
    assert_eq!(parse_page_input("", 23), None);
}

#[tokio::test]
async fn first_page_hides_the_adult_linked_character_and_keeps_numbers() {
    let r = roster().await;
    assert_eq!(r.total, TOTAL);
    let numbers: Vec<usize> = r.entries.iter().map(|(i, ..)| i + 1).collect();
    assert_eq!(
        numbers,
        vec![1, 2, 3, 4, 5, 6, 7, 9, 10],
        "number 8 is hidden, numbering has a gap"
    );
    let d = r.list_embed().description.expect("desc");
    assert!(
        d.contains("⭐ **6. Char 5**"),
        "the origin character (index 5) is highlighted: {d}"
    );
    assert!(d.contains("**1.** Char 0 — رئيسية"));
    assert!(!d.contains("Char 7"));
}

#[tokio::test]
async fn navigation_covers_a_200_plus_cast() {
    let mut r = roster().await;
    assert_eq!(r.pages(), 23);
    assert_eq!(r.handle(NAV_LAST, &[]).await, Step::Update);
    assert_eq!(r.page, 22);
    assert_eq!(r.entries.len(), 10);
    assert_eq!(r.entries.last().map(|e| e.0 + 1), Some(230));
    assert_eq!(
        r.handle(NAV_NEXT, &[]).await,
        Step::Keep,
        "no page past the end"
    );
    assert_eq!(r.handle(NAV_PREV, &[]).await, Step::Update);
    assert_eq!(r.page, 21);
    assert_eq!(r.handle(NAV_FIRST, &[]).await, Step::Update);
    assert_eq!(r.page, 0);
    // A page that straddles two AniList pages (entries 20-29).
    assert_eq!(r.handle(ID_RANGE, &["2".into()]).await, Step::Update);
    let numbers: Vec<usize> = r.entries.iter().map(|(i, ..)| i + 1).collect();
    assert_eq!(numbers, (21..=30).collect::<Vec<_>>());
}

#[tokio::test]
async fn page_jump_modal() {
    let mut r = roster().await;
    let spec = r.modal_for(ID_JUMP).expect("modal on the list");
    assert!(spec.label.contains("23"));
    assert!(r.modal_for("other").is_none());
    assert_eq!(r.on_modal(ID_JUMP, "12".into()).await, Step::Update);
    assert_eq!(r.page, 11);
    assert_eq!(r.on_modal(ID_JUMP, "99".into()).await, Step::Update);
    assert_eq!(r.page, 11, "invalid input keeps the page");
    assert_eq!(r.notice.as_deref(), Some(i18n::ROSTER_BAD_PAGE));
    assert_eq!(
        r.on_modal(ID_JUMP, "12".into()).await,
        Step::Keep,
        "same page"
    );
}

#[tokio::test]
async fn character_card_and_back_return_to_the_same_page() {
    let mut r = roster().await;
    r.handle(ID_RANGE, &["3".into()]).await;
    assert_eq!(r.handle(ID_CHAR, &["1031".into()]).await, Step::Update);
    assert!(matches!(r.mode, Mode::Card(_)));
    assert_eq!(r.handle(BACK, &[]).await, Step::Update);
    assert!(matches!(r.mode, Mode::List));
    assert_eq!(r.page, 3, "back returns to the same page");
    assert_eq!(
        r.handle(BACK, &[]).await,
        Step::Exit,
        "back from the list leaves the roster"
    );
    assert_eq!(r.handle(CLOSE, &[]).await, Step::Close);
}

#[tokio::test]
async fn several_works_ask_which_one_first() {
    let mut r = Roster::start(api(), ContentPolicy::SAFE, &character(1005, 3))
        .await
        .expect("ok")
        .expect("some");
    assert!(matches!(r.mode, Mode::WorkPick));
    assert!(r.work.is_none());
    assert_eq!(r.handle(ID_WORK, &["11".into()]).await, Step::Update);
    assert_eq!(r.title(), "Work 1");
    assert!(matches!(r.mode, Mode::List));
    assert_eq!(r.total, TOTAL);
    let none = Roster::start(api(), ContentPolicy::SAFE, &character(1, 0))
        .await
        .expect("ok");
    assert!(none.is_none(), "no known work, no roster");
}

#[tokio::test]
async fn render_respects_discord_component_limits() {
    let r = roster().await;
    let page = r.render(true);
    assert!(page.components.len() <= 5);
    assert!(!page.embeds.is_empty());
    let disabled = r.render(false);
    assert!(disabled.components.len() <= 5);
}
