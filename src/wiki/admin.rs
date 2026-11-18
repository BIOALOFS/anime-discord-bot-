//! The owner's wiki panel inside `/admin`: the list, adding a wiki with a preview, editing, switching, deleting, testing,
//! the emergency switch and the cache. Everything here is owner-only (the caller refuses anyone else) and global: a
//! change applies to every server at once.

use super::{
    client::Transport,
    service::{AddError, Preview, Stats, Wikis},
    store, AddressError, Source,
};
use crate::{
    i18n,
    ui::{
        embeds::{fit, Emb},
        pagination::{close_button, BACK},
        select::{select_row, Choice},
    },
};
use poise::serenity_prelude as serenity;

pub const ADD: &str = "wk:add";
pub const KILL: &str = "wk:kill";
pub const CLEAR: &str = "wk:clear";
pub const PICK: &str = "wk:pick";
pub const TOGGLE: &str = "wk:toggle";
pub const RENAME: &str = "wk:rename";
pub const CHAR_CAT: &str = "wk:charcat";
pub const GAME_CAT: &str = "wk:gamecat";
pub const TEST: &str = "wk:test";
pub const DELETE: &str = "wk:del";
pub const DELETE_YES: &str = "wk:delyes";
pub const CONFIRM: &str = "wk:confirm";
pub const CANCEL: &str = "wk:cancel";

/// The ids this panel answers.
pub fn handles(id: &str) -> bool {
    id.starts_with("wk:")
}

/// A wiki the owner is adding: what was discovered and the categories as edited so far.
#[derive(Debug, Clone)]
pub struct Pending {
    pub preview: Preview,
    pub characters: Option<String>,
    pub games: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Panel {
    pub sources: Vec<Source>,
    pub selected: Option<i64>,
    pub pending: Option<Pending>,
    pub confirm_delete: bool,
    pub stats: Option<Stats>,
}

impl Panel {
    pub async fn refresh<T: Transport + 'static>(&mut self, w: &Wikis<T>) {
        self.sources = store::list(&w.pool).await.unwrap_or_default();
        self.stats = Some(w.stats().await);
        if self
            .selected
            .is_some_and(|id| !self.sources.iter().any(|s| s.id == id))
        {
            self.selected = None;
            self.confirm_delete = false;
        }
    }

    pub fn selected_source(&self) -> Option<&Source> {
        self.sources.iter().find(|s| Some(s.id) == self.selected)
    }
}

/// The one-line read-only statistics.
pub fn stats_line(s: &Stats) -> String {
    let mut t = format!(
        "• الويكيات: {} (مفعّلة {}) · صفحات مخزنة: {} · طلبات اليوم: {}",
        s.wikis, s.enabled, s.pages, s.requests_today
    );
    if !s.last_errors.is_empty() {
        t.push_str(&format!("\n• آخر الأخطاء: {}", s.last_errors.join(" | ")));
    }
    t
}

pub fn embed(p: &Panel) -> Emb {
    if let Some(pending) = &p.pending {
        let d = &pending.preview;
        let found = |c: &Option<(String, u64)>| c.as_ref().map(|(t, n)| format!("{t} ({n})"));
        let mut lines = vec![
            format!("**{}** — {}", d.name, d.host),
            match &d.detection.character {
                Some((_, n)) => i18n::wiki_preview_found(*n, &d.detection.samples),
                None => "لم أجد تصنيف شخصيات تلقائيًا؛ يمكنك كتابته بنفسك أو الحفظ للبحث فقط."
                    .to_string(),
            },
            format!(
                "• تصنيف الشخصيات: {}",
                pending
                    .characters
                    .clone()
                    .or_else(|| found(&d.detection.character))
                    .unwrap_or_else(|| "بحث فقط".into())
            ),
            format!(
                "• تصنيف الألعاب: {}",
                pending
                    .games
                    .clone()
                    .or_else(|| found(&d.detection.game))
                    .unwrap_or_else(|| "بحث فقط".into())
            ),
        ];
        if let Some(l) = &d.license_name {
            lines.push(format!("• الرخصة: {l}"));
        }
        return Emb::new()
            .title(i18n::WIKI_ADD_TITLE)
            .description(fit(&lines.join("\n"), 3800));
    }
    let mut lines = vec![i18n::WIKI_ADMIN_INTRO.to_string(), String::new()];
    if p.sources.is_empty() {
        lines.push(i18n::WIKI_NONE.to_string());
    }
    for s in &p.sources {
        let mark = if p.selected == Some(s.id) { "▶ " } else { "" };
        lines.push(format!(
            "{mark}{} **{}** — {}{}",
            if s.enabled { "✅" } else { "⏸️" },
            s.name,
            s.host,
            s.last_error
                .as_ref()
                .map(|e| format!(" ⚠️ {}", fit(e, 60)))
                .unwrap_or_default()
        ));
    }
    if let Some(s) = &p.stats {
        lines.push(String::new());
        lines.push(format!(
            "• الويكيات {}",
            if s.global {
                "تعمل"
            } else {
                "**موقوفة** (مفتاح الطوارئ)"
            }
        ));
        lines.push(stats_line(s));
    }
    if p.confirm_delete {
        lines.push(String::new());
        lines.push(i18n::WIKI_CONFIRM_DELETE.to_string());
    }
    Emb::new()
        .title(i18n::WIKI_ADMIN_TITLE)
        .description(fit(&lines.join("\n"), 3900))
}

pub fn components(p: &Panel, enabled: bool) -> Vec<serenity::CreateActionRow> {
    let b = |id: &str, label: &str, style: serenity::ButtonStyle| {
        serenity::CreateButton::new(id)
            .label(label)
            .style(style)
            .disabled(!enabled)
    };
    let sec = serenity::ButtonStyle::Secondary;
    if p.pending.is_some() {
        return vec![serenity::CreateActionRow::Buttons(vec![
            b(CONFIRM, "تأكيد وحفظ", serenity::ButtonStyle::Success),
            b(CHAR_CAT, "تصنيف الشخصيات", sec),
            b(GAME_CAT, "تصنيف الألعاب", sec),
            b(CANCEL, "إلغاء", sec),
        ])];
    }
    let global_on = p.stats.as_ref().is_none_or(|s| s.global);
    let mut rows = vec![serenity::CreateActionRow::Buttons(vec![
        b(ADD, "إضافة ويكي", serenity::ButtonStyle::Primary),
        b(
            KILL,
            if global_on {
                "إيقاف كل الويكيات"
            } else {
                "تشغيل الويكيات"
            },
            sec,
        ),
        b(CLEAR, "مسح الكاش", sec),
        b(BACK, "رجوع", sec),
        close_button(enabled),
    ])];
    let choices: Vec<Choice> = p
        .sources
        .iter()
        .take(25)
        .map(|s| Choice {
            label: format!(
                "{} {}",
                if s.enabled { "✅" } else { "⏸️" },
                fit(&s.name, 90)
            ),
            description: Some(fit(&s.host, 100)),
            value: s.id.to_string(),
        })
        .collect();
    rows.extend(select_row(
        PICK,
        i18n::WIKI_PICK_PLACEHOLDER,
        choices,
        enabled,
    ));
    if let Some(s) = p.selected_source() {
        if p.confirm_delete {
            rows.push(serenity::CreateActionRow::Buttons(vec![
                b(DELETE_YES, "تأكيد الحذف", serenity::ButtonStyle::Danger),
                b(CANCEL, "إلغاء", sec),
            ]));
        } else {
            rows.push(serenity::CreateActionRow::Buttons(vec![
                b(
                    TOGGLE,
                    if s.enabled {
                        "تعطيل"
                    } else {
                        "تفعيل"
                    },
                    sec,
                ),
                b(RENAME, "الاسم", sec),
                b(CHAR_CAT, "تصنيف الشخصيات", sec),
                b(TEST, "اختبار الاتصال", sec),
                b(DELETE, "حذف", serenity::ButtonStyle::Danger),
            ]));
        }
    }
    rows
}

/// The modal a button opens: `(title, field label)`.
pub fn modal_spec(id: &str) -> Option<(&'static str, &'static str)> {
    match id {
        ADD => Some((i18n::WIKI_ADD_TITLE, i18n::WIKI_ADD_LABEL)),
        RENAME => Some((i18n::WIKI_RENAME_TITLE, i18n::WIKI_RENAME_LABEL)),
        CHAR_CAT => Some((i18n::WIKI_CHAR_CAT_TITLE, i18n::WIKI_CHAR_CAT_LABEL)),
        GAME_CAT => Some((i18n::WIKI_GAME_CAT_TITLE, i18n::WIKI_GAME_CAT_LABEL)),
        _ => None,
    }
}

/// The answer to a refused address or an unreachable site.
pub fn add_error_text(e: &AddError) -> String {
    match e {
        AddError::Address(
            AddressError::Empty
            | AddressError::Invalid
            | AddressError::NotHttps
            | AddressError::NotPublic,
        ) => i18n::WIKI_BAD_ADDRESS.to_string(),
        AddError::Unreachable(_) => i18n::WIKI_NOT_MEDIAWIKI.to_string(),
    }
}

/// A button press. Returns the notice to show.
pub async fn press<T: Transport + 'static>(
    p: &mut Panel,
    w: &Wikis<T>,
    user: i64,
    id: &str,
    values: &[String],
) -> String {
    let notice = press_inner(p, w, user, id, values).await;
    // What the list shows follows the change at once.
    p.refresh(w).await;
    notice
}

async fn press_inner<T: Transport + 'static>(
    p: &mut Panel,
    w: &Wikis<T>,
    user: i64,
    id: &str,
    values: &[String],
) -> String {
    match id {
        PICK => {
            p.selected = values.first().and_then(|v| v.parse().ok());
            p.confirm_delete = false;
            String::new()
        }
        KILL => {
            let on = !store::global_enabled(&w.pool).await;
            let _ = store::set_global_enabled(&w.pool, on).await;
            w.invalidate().await;
            if on {
                i18n::WIKI_KILL_ON
            } else {
                i18n::WIKI_KILL_OFF
            }
            .to_string()
        }
        CLEAR => {
            let _ = store::clear_cache(&w.pool).await;
            w.invalidate().await;
            i18n::WIKI_CACHE_CLEARED.to_string()
        }
        CANCEL => {
            let was_adding = p.pending.take().is_some();
            p.confirm_delete = false;
            if was_adding {
                i18n::WIKI_CANCELLED.to_string()
            } else {
                String::new()
            }
        }
        CONFIRM => {
            let Some(pending) = p.pending.take() else {
                return String::new();
            };
            let d = &pending.preview;
            let chars = pending
                .characters
                .clone()
                .or_else(|| d.detection.character.as_ref().map(|c| c.0.clone()));
            let games = pending
                .games
                .clone()
                .or_else(|| d.detection.game.as_ref().map(|c| c.0.clone()));
            match w
                .save(d, None, chars.as_deref(), games.as_deref(), user)
                .await
            {
                Ok(store::Added::New(_)) => i18n::WIKI_SAVED.to_string(),
                Ok(store::Added::Duplicate(_)) => i18n::WIKI_DUPLICATE.to_string(),
                Err(e) => {
                    tracing::warn!("saving a wiki failed: {e}");
                    i18n::ERR_GENERIC.to_string()
                }
            }
        }
        TOGGLE | TEST | DELETE | DELETE_YES => {
            let Some(s) = p.selected_source().cloned() else {
                return i18n::WIKI_NO_SELECTION.to_string();
            };
            match id {
                TOGGLE => {
                    let _ = store::set_enabled(&w.pool, s.id, !s.enabled).await;
                    w.invalidate().await;
                    if s.enabled {
                        i18n::WIKI_DISABLED
                    } else {
                        i18n::WIKI_ENABLED
                    }
                    .to_string()
                }
                TEST => match w.test(&s).await {
                    Ok(()) => i18n::WIKI_CONNECTION_OK.to_string(),
                    Err(e) => i18n::wiki_connection_failed(&e.to_string()),
                },
                DELETE => {
                    p.confirm_delete = true;
                    String::new()
                }
                _ => {
                    // Only the source definition goes; cached pages expire, member data is never involved.
                    let _ = store::delete(&w.pool, s.id).await;
                    w.invalidate().await;
                    p.selected = None;
                    p.confirm_delete = false;
                    i18n::WIKI_DELETED.to_string()
                }
            }
        }
        _ => String::new(),
    }
}

/// A modal answer. Returns the notice to show.
pub async fn modal<T: Transport + 'static>(
    p: &mut Panel,
    w: &Wikis<T>,
    id: &str,
    input: String,
) -> String {
    let notice = modal_inner(p, w, id, input).await;
    p.refresh(w).await;
    notice
}

async fn modal_inner<T: Transport + 'static>(
    p: &mut Panel,
    w: &Wikis<T>,
    id: &str,
    input: String,
) -> String {
    let text = input.trim().to_string();
    match id {
        ADD => match w.preview(&text).await {
            Ok(preview) => {
                p.pending = Some(Pending {
                    preview,
                    characters: None,
                    games: None,
                });
                String::new()
            }
            Err(e) => add_error_text(&e),
        },
        CHAR_CAT | GAME_CAT => {
            let category = (!text.is_empty()).then(|| super::parse::category_title(&text));
            if let Some(pending) = p.pending.as_mut() {
                // While adding: the choice is kept for the confirmation (an empty text means search-only).
                let slot = if id == CHAR_CAT {
                    &mut pending.characters
                } else {
                    &mut pending.games
                };
                *slot = Some(category.clone().unwrap_or_default()).filter(|c| !c.is_empty());
                if id == CHAR_CAT {
                    if let Some(c) = &category {
                        let n = w
                            .api
                            .count_category(&pending.preview.api_url, c)
                            .await
                            .unwrap_or(0);
                        pending.preview.detection.character = Some((c.clone(), n));
                        pending.preview.detection.samples.clear();
                    } else {
                        pending.preview.detection.character = None;
                    }
                }
                return String::new();
            }
            let Some(s) = p.selected_source().cloned() else {
                return i18n::WIKI_NO_SELECTION.to_string();
            };
            let (chars, games) = if id == CHAR_CAT {
                (category, s.game_category.clone())
            } else {
                (s.character_category.clone(), category)
            };
            let _ = store::update(&w.pool, s.id, &s.name, chars.as_deref(), games.as_deref()).await;
            w.invalidate().await;
            i18n::WIKI_RENAMED.to_string()
        }
        RENAME => {
            let Some(s) = p.selected_source().cloned() else {
                return i18n::WIKI_NO_SELECTION.to_string();
            };
            if text.is_empty() {
                return String::new();
            }
            let name: String = text.chars().take(80).collect();
            let _ = store::update(
                &w.pool,
                s.id,
                &name,
                s.character_category.as_deref(),
                s.game_category.as_deref(),
            )
            .await;
            w.invalidate().await;
            i18n::WIKI_RENAMED.to_string()
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki::client::fake::FakeWiki;
    use std::{collections::HashMap, sync::Arc};

    fn fake() -> FakeWiki {
        let mut members = HashMap::new();
        members.insert(
            "Category:Characters".to_string(),
            vec![(1, "Pyramid Head".to_string()), (2, "Heather".to_string())],
        );
        FakeWiki {
            categories: vec![
                ("Category:Characters".into(), 2),
                ("Category:Games".into(), 1),
            ],
            members,
            batch: 10,
            ..FakeWiki::default()
        }
    }

    async fn setup() -> (Panel, Arc<Wikis<FakeWiki>>) {
        let pool = crate::db::connect_memory().await;
        (
            Panel::default(),
            Arc::new(Wikis::new(pool, Arc::new(fake()))),
        )
    }

    fn ids(rows: &[serenity::CreateActionRow]) -> Vec<String> {
        serde_json::to_value(rows)
            .expect("json")
            .as_array()
            .expect("rows")
            .iter()
            .flat_map(|r| r["components"].as_array().cloned().unwrap_or_default())
            .map(|c| c["custom_id"].as_str().unwrap_or("").to_string())
            .collect()
    }

    #[tokio::test]
    async fn adding_shows_a_preview_and_saves_only_on_confirm_and_is_live_everywhere() {
        let (mut p, w) = setup().await;
        let notice = modal(
            &mut p,
            &w,
            ADD,
            "https://silenthill.fandom.com/wiki/Main_Page".into(),
        )
        .await;
        assert!(notice.is_empty());
        let pending = p.pending.as_ref().expect("a preview");
        assert_eq!(
            pending.preview.detection.character.as_ref().map(|c| c.1),
            Some(2)
        );
        let text = serde_json::to_value(embed(&p).build()).expect("json")["description"]
            .as_str()
            .unwrap_or("")
            .to_string();
        assert!(
            text.contains("وجدت 2 شخصية، مثال: Pyramid Head، Heather."),
            "{text}"
        );
        assert!(
            store::list(&w.pool).await.expect("list").is_empty(),
            "nothing is saved before confirming"
        );
        assert_eq!(
            ids(&components(&p, true)),
            vec![CONFIRM, CHAR_CAT, GAME_CAT, CANCEL]
        );
        assert_eq!(press(&mut p, &w, 7, CONFIRM, &[]).await, i18n::WIKI_SAVED);
        assert!(p.pending.is_none());
        let live = w.sources().await;
        assert_eq!(live.len(), 1);
        assert!(
            live[0].enabled,
            "enabled on creation, in every server, without a restart"
        );
        assert_eq!(
            live[0].character_category.as_deref(),
            Some("Category:Characters")
        );
        assert_eq!(live[0].added_by, Some(7));
    }

    #[tokio::test]
    async fn cancelling_the_preview_saves_nothing_and_a_bad_address_says_why() {
        let (mut p, w) = setup().await;
        modal(&mut p, &w, ADD, "silenthill.fandom.com".into()).await;
        assert_eq!(
            press(&mut p, &w, 1, CANCEL, &[]).await,
            i18n::WIKI_CANCELLED
        );
        assert!(store::list(&w.pool).await.expect("list").is_empty());
        for bad in [
            "http://a.fandom.com",
            "https://127.0.0.1",
            "https://localhost/api.php",
            "",
        ] {
            assert_eq!(
                modal(&mut p, &w, ADD, bad.into()).await,
                i18n::WIKI_BAD_ADDRESS,
                "{bad}"
            );
        }
        assert!(p.pending.is_none());
        let pool = crate::db::connect_memory().await;
        let none = Wikis::new(
            pool,
            Arc::new(FakeWiki {
                not_mediawiki: true,
                ..FakeWiki::default()
            }),
        );
        assert_eq!(
            modal(&mut p, &none, ADD, "example.com".into()).await,
            i18n::WIKI_NOT_MEDIAWIKI
        );
    }

    #[tokio::test]
    async fn the_owner_can_set_a_category_by_hand_or_save_search_only() {
        let (mut p, w) = setup().await;
        modal(&mut p, &w, ADD, "x.fandom.com".into()).await;
        modal(&mut p, &w, CHAR_CAT, "Characters".into()).await;
        assert_eq!(
            p.pending
                .as_ref()
                .and_then(|x| x.characters.clone())
                .as_deref(),
            Some("Category:Characters")
        );
        assert_eq!(
            p.pending
                .as_ref()
                .and_then(|x| x.preview.detection.character.clone())
                .map(|c| c.1),
            Some(2),
            "the count of the typed category is shown"
        );
        modal(&mut p, &w, CHAR_CAT, "".into()).await;
        modal(&mut p, &w, GAME_CAT, "".into()).await;
        assert!(p
            .pending
            .as_ref()
            .expect("pending")
            .preview
            .detection
            .character
            .is_none());
        let text = serde_json::to_value(embed(&p).build()).expect("json")["description"]
            .as_str()
            .unwrap_or("")
            .to_string();
        assert!(text.contains("بحث فقط"));
        // confirming with nothing found still saves (search-only), as the owner chose
        p.pending.as_mut().expect("pending").preview.detection.game = None;
        press(&mut p, &w, 1, CONFIRM, &[]).await;
        let saved = &w.sources().await[0];
        assert_eq!(
            (
                saved.character_category.clone(),
                saved.game_category.clone()
            ),
            (None, None)
        );
    }

    #[tokio::test]
    async fn toggling_renaming_testing_and_deleting_are_global_and_delete_asks_first() {
        let (mut p, w) = setup().await;
        modal(&mut p, &w, ADD, "a.fandom.com".into()).await;
        press(&mut p, &w, 1, CONFIRM, &[]).await;
        p.refresh(&w).await;
        let id = p.sources[0].id;
        assert_eq!(
            press(&mut p, &w, 1, TOGGLE, &[]).await,
            i18n::WIKI_NO_SELECTION,
            "nothing selected yet"
        );
        press(&mut p, &w, 1, PICK, &[id.to_string()]).await;
        assert!(ids(&components(&p, true)).contains(&DELETE.to_string()));
        assert_eq!(
            press(&mut p, &w, 1, TEST, &[]).await,
            i18n::WIKI_CONNECTION_OK
        );
        assert_eq!(press(&mut p, &w, 1, TOGGLE, &[]).await, i18n::WIKI_DISABLED);
        assert!(
            w.sources().await.is_empty(),
            "disabled for every server at once"
        );
        assert_eq!(press(&mut p, &w, 1, TOGGLE, &[]).await, i18n::WIKI_ENABLED);
        assert_eq!(w.sources().await.len(), 1);
        modal(&mut p, &w, RENAME, "Silent Hill Wiki".into()).await;
        assert_eq!(w.sources().await[0].name, "Silent Hill Wiki");
        // deleting asks first
        press(&mut p, &w, 1, DELETE, &[]).await;
        assert!(p.confirm_delete);
        assert_eq!(
            ids(&components(&p, true))
                .iter()
                .filter(|i| *i == DELETE_YES || *i == CANCEL)
                .count(),
            2
        );
        assert_eq!(w.sources().await.len(), 1, "not yet");
        assert_eq!(
            press(&mut p, &w, 1, DELETE_YES, &[]).await,
            i18n::WIKI_DELETED
        );
        assert!(w.sources().await.is_empty());
        assert!(p.selected.is_none());
    }

    #[tokio::test]
    async fn the_kill_switch_and_the_cache_clear_do_not_touch_the_definitions_or_member_data() {
        let (mut p, w) = setup().await;
        modal(&mut p, &w, ADD, "a.fandom.com".into()).await;
        press(&mut p, &w, 1, CONFIRM, &[]).await;
        crate::db::profile::create_profile(&w.pool, 5)
            .await
            .expect("profile");
        assert_eq!(press(&mut p, &w, 1, KILL, &[]).await, i18n::WIKI_KILL_OFF);
        assert!(w.sources().await.is_empty());
        p.refresh(&w).await;
        assert_eq!(p.sources.len(), 1, "the definition is still there");
        assert!(ids(&components(&p, true)).len() >= 5);
        let label = serde_json::to_value(components(&p, true)).expect("json")[0]["components"][1]
            ["label"]
            .as_str()
            .unwrap_or("")
            .to_string();
        assert_eq!(label, "تشغيل الويكيات");
        assert_eq!(press(&mut p, &w, 1, KILL, &[]).await, i18n::WIKI_KILL_ON);
        assert_eq!(w.sources().await.len(), 1);
        assert_eq!(
            press(&mut p, &w, 1, CLEAR, &[]).await,
            i18n::WIKI_CACHE_CLEARED
        );
        assert!(crate::db::profile::get_profile(&w.pool, 5)
            .await
            .expect("profile")
            .is_some());
    }

    #[tokio::test]
    async fn the_list_shows_the_stats_the_errors_and_the_kill_switch_state() {
        let (mut p, w) = setup().await;
        modal(&mut p, &w, ADD, "a.fandom.com".into()).await;
        press(&mut p, &w, 1, CONFIRM, &[]).await;
        let id = w.sources().await[0].id;
        store::mark_checked(&w.pool, id, 1, Some("timeout"))
            .await
            .expect("mark");
        p.refresh(&w).await;
        let text = serde_json::to_value(embed(&p).build()).expect("json")["description"]
            .as_str()
            .unwrap_or("")
            .to_string();
        assert!(
            text.contains("الويكيات: 1 (مفعّلة 1)")
                && text.contains("طلبات اليوم")
                && text.contains("timeout"),
            "{text}"
        );
        assert!(text.contains("تعمل"));
        assert_eq!(modal_spec(ADD).map(|m| m.0), Some("إضافة ويكي"));
        assert!(modal_spec("wk:other").is_none());
        assert!(handles("wk:add") && !handles("o:backup"));
    }
}
