//! `/settings setting:notifications`: a private panel for the member's own notification preferences — a switch,
//! a filter mode and a digest interval per category, the time zone and hour, the spoiler guard, and the list of
//! things they never want to hear about.

use super::buttons;
use crate::{
    db::notify::{self, Prefs, Setting},
    error::Error,
    i18n,
    ui::{
        embeds::{fit, Emb},
        pagination::{close_button, CLOSE},
        select::{select_row, Choice},
        session::{ModalSpec, Outcome, Page, View},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;

const CAT: &str = "np_cat";
const MODE: &str = "np_mode";
const DIGEST: &str = "np_digest";
const TOGGLE: &str = "np_toggle";
const SWITCH: &str = "np_switch";
const TZ: &str = "np_tz";
const HOUR: &str = "np_hour";
const SPOILER: &str = "np_spoiler";
const EXCLUDE: &str = "np_exclude";
const UNEXCLUDE: &str = "np_unexclude";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Categories,
    General,
}

pub fn mode_label(m: &str) -> &'static str {
    match m {
        "all" => "كل شيء",
        "mine" => "أعمالي وتذكيراتي فقط",
        "popular" => "الأعمال الشائعة فقط",
        _ => "—",
    }
}

pub fn digest_label(h: i64) -> String {
    match h {
        1 => "كل ساعة".into(),
        24 => "مرة في اليوم".into(),
        n => format!("كل {n} ساعات"),
    }
}

/// Only the chapter categories have digests.
pub fn has_digest(category: &str) -> bool {
    matches!(category, "manga" | "novel")
}

/// What a typed exclusion means: a category (by Arabic or English name), an anime format, or otherwise a genre.
pub fn parse_exclusion(input: &str) -> Option<(&'static str, String)> {
    let t = input.trim();
    if t.is_empty() || t.chars().count() > 40 {
        return None;
    }
    if let Some(c) = notify::CATEGORIES.iter().find(|c| {
        c.eq_ignore_ascii_case(t)
            || notify::category_label(c) == t
            || notify::category_label(c).trim_start_matches("ال") == t
    }) {
        return Some(("category", (*c).to_string()));
    }
    if let Some((f, _)) = super::category::ANIME_FORMATS
        .iter()
        .find(|(f, l)| f.eq_ignore_ascii_case(t) || *l == t)
    {
        return Some(("format", (*f).to_lowercase()));
    }
    Some(("genre", t.to_lowercase()))
}

pub fn exclusion_text(kind: &str, value: &str) -> String {
    match kind {
        "item" => format!("عمل: {value}"),
        "category" => format!("فئة: {}", notify::category_label(value)),
        "format" => format!("صيغة: {}", value.to_uppercase()),
        _ => format!("نوع: {value}"),
    }
}

pub struct NotifyView {
    pub pool: SqlitePool,
    pub user: i64,
    screen: Screen,
    cat: usize,
    setting: Setting,
    prefs: Prefs,
    excl: Vec<(String, String)>,
    notice: Option<String>,
}

impl NotifyView {
    pub async fn open(pool: SqlitePool, user: i64) -> Result<Self, Error> {
        let mut v = Self {
            pool,
            user,
            screen: Screen::Categories,
            cat: 0,
            setting: Setting::default(),
            prefs: Prefs::default(),
            excl: Vec::new(),
            notice: None,
        };
        v.reload().await?;
        Ok(v)
    }

    fn category(&self) -> &'static str {
        notify::CATEGORIES[self.cat]
    }

    async fn reload(&mut self) -> Result<(), Error> {
        self.setting = notify::setting(&self.pool, self.user, self.category()).await?;
        self.prefs = notify::prefs(&self.pool, self.user).await?;
        self.excl = notify::exclusions(&self.pool, self.user).await?;
        Ok(())
    }

    fn embed(&self) -> Emb {
        match self.screen {
            Screen::Categories => Emb::new().title(format!("🔔 الإشعارات — {}", notify::category_label(self.category()))).description(format!(
                "• الحالة: {}\n• ما يصلك: **{}**{}\n\nاختر فئة من القائمة الأولى لتعديل إعداداتها. التذكيرات التي تضيفها بنفسك تصلك دائمًا ما لم تُطفئ الفئة أو تستثنِ العمل.",
                if self.setting.enabled { "✅ مفعّلة" } else { "❌ مطفأة" },
                mode_label(&self.setting.mode),
                if has_digest(self.category()) { format!("\n• ملخص الفصول: **{}**", digest_label(self.setting.digest_hours)) } else { String::new() },
            )),
            Screen::General => {
                let excl = if self.excl.is_empty() { "لا شيء".to_string() } else { self.excl.iter().map(|(k, v)| exclusion_text(k, v)).collect::<Vec<_>>().join("، ") };
                Emb::new().title("⚙️ إعدادات عامة").description(format!(
                    "• المنطقة الزمنية: **{}**\n• ساعة الملخص الأسبوعي: **{:02}:00**\n• حماية الحرق (إخفاء الملخصات خلف ||سبويلر||): {}\n• لا أريد إشعارات عن: {}",
                    super::admin::TIMEZONES.iter().find(|(_, z)| *z == self.prefs.timezone).map_or(self.prefs.timezone.as_str(), |(l, _)| *l),
                    self.prefs.summary_hour,
                    if self.prefs.spoiler_guard { "✅" } else { "❌" },
                    fit(&excl, 900),
                ))
            }
        }
    }
}

fn choices(items: impl IntoIterator<Item = (String, String)>) -> Vec<Choice> {
    items
        .into_iter()
        .map(|(label, value)| Choice {
            label,
            description: None,
            value,
        })
        .collect()
}

impl View for NotifyView {
    fn ephemeral(&self) -> bool {
        true
    }

    fn render(&self, enabled: bool) -> Page {
        let mut rows = Vec::new();
        let btn = |id: &str, label: &str, on: Option<bool>| {
            let b = serenity::CreateButton::new(id)
                .label(label)
                .disabled(!enabled);
            match on {
                Some(true) => b.style(serenity::ButtonStyle::Success),
                _ => b.style(serenity::ButtonStyle::Secondary),
            }
        };
        match self.screen {
            Screen::Categories => {
                rows.extend(select_row(
                    CAT,
                    "اختر الفئة",
                    choices(
                        notify::CATEGORIES
                            .iter()
                            .map(|c| (notify::category_label(c).to_string(), (*c).to_string())),
                    ),
                    enabled,
                ));
                rows.extend(select_row(
                    MODE,
                    "ما الذي يصلني؟",
                    choices(
                        notify::MODES
                            .iter()
                            .map(|m| (mode_label(m).to_string(), (*m).to_string())),
                    ),
                    enabled,
                ));
                if has_digest(self.category()) {
                    rows.extend(select_row(
                        DIGEST,
                        "ملخص الفصول",
                        choices(
                            notify::DIGEST_HOURS
                                .iter()
                                .map(|h| (digest_label(*h), h.to_string())),
                        ),
                        enabled,
                    ));
                }
                rows.push(buttons(vec![
                    btn(
                        TOGGLE,
                        if self.setting.enabled {
                            "أطفئ هذه الفئة"
                        } else {
                            "شغّل هذه الفئة"
                        },
                        Some(!self.setting.enabled),
                    ),
                    btn(SWITCH, "إعدادات عامة", None),
                    close_button(enabled),
                ]));
            }
            Screen::General => {
                rows.extend(select_row(
                    TZ,
                    "المنطقة الزمنية",
                    choices(
                        super::admin::TIMEZONES
                            .iter()
                            .map(|(l, z)| ((*l).to_string(), (*z).to_string())),
                    ),
                    enabled,
                ));
                rows.extend(select_row(
                    HOUR,
                    "ساعة الملخص الأسبوعي",
                    choices((0..24).map(|h| (format!("{h:02}:00"), h.to_string()))),
                    enabled,
                ));
                if !self.excl.is_empty() {
                    rows.extend(select_row(
                        UNEXCLUDE,
                        "ألغِ استثناءً",
                        choices(
                            self.excl.iter().take(25).map(|(k, v)| {
                                (fit(&exclusion_text(k, v), 100), format!("{k}|{v}"))
                            }),
                        ),
                        enabled,
                    ));
                }
                rows.push(buttons(vec![
                    btn(SPOILER, "حماية الحرق", Some(self.prefs.spoiler_guard)),
                    btn(EXCLUDE, "أضف استثناءً", None),
                    btn(SWITCH, "الفئات", None),
                    close_button(enabled),
                ]));
            }
        }
        Page::embed(self.embed().build(), rows).notice(self.notice.clone())
    }

    fn modal_for(&self, id: &str) -> Option<ModalSpec> {
        (id == EXCLUDE).then(|| ModalSpec {
            title: "استثناء من الإشعارات".into(),
            label: "فئة (مثل المانجا) أو صيغة (مثل OVA) أو نوع (مثل Ecchi)".into(),
        })
    }

    async fn on_modal(&mut self, id: &str, input: String) -> Outcome {
        if id != EXCLUDE {
            return Outcome::Keep;
        }
        self.notice = Some(match parse_exclusion(&input) {
            Some((kind, value)) => match notify::exclude(&self.pool, self.user, kind, &value).await
            {
                Ok(()) => i18n::SETTINGS_SAVED.to_string(),
                Err(_) => i18n::ERR_GENERIC.to_string(),
            },
            None => "لم أفهم الاستثناء، اكتب اسم فئة أو صيغة أو نوع.".to_string(),
        });
        let _ = self.reload().await;
        Outcome::Update
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        self.notice = None;
        let first = values.first().map(String::as_str);
        let (pool, user) = (self.pool.clone(), self.user);
        let result: Result<(), sqlx::Error> = async {
            match id {
                CLOSE => {}
                SWITCH => {
                    self.screen = if self.screen == Screen::Categories {
                        Screen::General
                    } else {
                        Screen::Categories
                    }
                }
                CAT => {
                    if let Some(i) =
                        first.and_then(|c| notify::CATEGORIES.iter().position(|x| *x == c))
                    {
                        self.cat = i;
                    }
                }
                TOGGLE => {
                    let s = Setting {
                        enabled: !self.setting.enabled,
                        ..self.setting.clone()
                    };
                    notify::set_setting(&pool, user, self.category(), &s).await?;
                }
                MODE => {
                    if let Some(m) = first.filter(|m| notify::MODES.contains(m)) {
                        notify::set_setting(
                            &pool,
                            user,
                            self.category(),
                            &Setting {
                                mode: m.to_string(),
                                ..self.setting.clone()
                            },
                        )
                        .await?;
                    }
                }
                DIGEST => {
                    if let Some(h) = first
                        .and_then(|h| h.parse::<i64>().ok())
                        .filter(|h| notify::DIGEST_HOURS.contains(h))
                    {
                        notify::set_setting(
                            &pool,
                            user,
                            self.category(),
                            &Setting {
                                digest_hours: h,
                                ..self.setting.clone()
                            },
                        )
                        .await?;
                    }
                }
                TZ => {
                    if let Some(z) =
                        first.filter(|z| super::admin::TIMEZONES.iter().any(|(_, t)| t == z))
                    {
                        notify::set_prefs(
                            &pool,
                            user,
                            &Prefs {
                                timezone: z.to_string(),
                                ..self.prefs.clone()
                            },
                        )
                        .await?;
                    }
                }
                HOUR => {
                    if let Some(h) = first.and_then(|h| h.parse::<i64>().ok()) {
                        notify::set_prefs(
                            &pool,
                            user,
                            &Prefs {
                                summary_hour: h,
                                ..self.prefs.clone()
                            },
                        )
                        .await?;
                    }
                }
                SPOILER => {
                    notify::set_prefs(
                        &pool,
                        user,
                        &Prefs {
                            spoiler_guard: !self.prefs.spoiler_guard,
                            ..self.prefs.clone()
                        },
                    )
                    .await?
                }
                UNEXCLUDE => {
                    if let Some((k, v)) = first.and_then(|s| s.split_once('|')) {
                        notify::unexclude(&pool, user, k, v).await?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        .await;
        if id == CLOSE {
            return Outcome::Close;
        }
        if result.is_err() || self.reload().await.is_err() {
            self.notice = Some(i18n::ERR_GENERIC.to_string());
        } else if !matches!(id, SWITCH | CAT) {
            self.notice = Some(i18n::SETTINGS_SAVED.to_string());
        }
        Outcome::Update
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn view() -> NotifyView {
        let p = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("db");
        sqlx::migrate!("./migrations")
            .run(&p)
            .await
            .expect("migrate");
        NotifyView::open(p, 5).await.expect("open")
    }

    #[test]
    fn typed_exclusions_are_read_as_category_format_or_genre() {
        assert_eq!(
            parse_exclusion("المانجا"),
            Some(("category", "manga".into()))
        );
        assert_eq!(parse_exclusion("drama"), Some(("category", "drama".into())));
        assert_eq!(parse_exclusion("OVA"), Some(("format", "ova".into())));
        assert_eq!(parse_exclusion(" Ecchi "), Some(("genre", "ecchi".into())));
        assert_eq!(parse_exclusion("   "), None);
        assert_eq!(parse_exclusion(&"x".repeat(41)), None);
    }

    #[test]
    fn only_chapter_categories_have_digests_and_labels_are_arabic() {
        assert!(
            has_digest("manga")
                && has_digest("novel")
                && !has_digest("anime")
                && !has_digest("movie")
        );
        assert_eq!(digest_label(1), "كل ساعة");
        assert_eq!(digest_label(2), "كل 2 ساعات");
        assert_eq!(digest_label(24), "مرة في اليوم");
        assert!(notify::CATEGORIES
            .iter()
            .all(|c| notify::category_label(c) != "—"));
        assert!(
            exclusion_text("category", "movie").contains("الأفلام")
                && exclusion_text("genre", "ecchi").contains("ecchi")
        );
    }

    #[tokio::test]
    async fn the_panel_changes_a_category_and_the_general_settings() {
        let mut v = view().await;
        assert!(
            v.on_interaction(CAT, &["manga".into()]).await == Outcome::Update
                && v.category() == "manga"
        );
        v.on_interaction(TOGGLE, &[]).await;
        v.on_interaction(MODE, &["popular".into()]).await;
        v.on_interaction(DIGEST, &["8".into()]).await;
        let s = notify::setting(&v.pool, 5, "manga").await.expect("s");
        assert_eq!(
            (s.enabled, s.mode.as_str(), s.digest_hours),
            (false, "popular", 8)
        );
        assert!(
            notify::setting(&v.pool, 5, "anime")
                .await
                .expect("s")
                .enabled,
            "other categories are untouched"
        );
        v.on_interaction(SWITCH, &[]).await;
        v.on_interaction(TZ, &["Asia/Riyadh".into()]).await;
        v.on_interaction(HOUR, &["21".into()]).await;
        v.on_interaction(SPOILER, &[]).await;
        let p = notify::prefs(&v.pool, 5).await.expect("p");
        assert_eq!(
            (p.timezone.as_str(), p.summary_hour, p.spoiler_guard),
            ("Asia/Riyadh", 21, true)
        );
    }

    #[tokio::test]
    async fn exclusions_are_added_through_the_modal_and_removed_from_the_menu() {
        let mut v = view().await;
        v.on_modal(EXCLUDE, "Ecchi".into()).await;
        v.on_modal(EXCLUDE, "الأفلام".into()).await;
        assert_eq!(notify::exclusions(&v.pool, 5).await.expect("e").len(), 2);
        v.on_interaction(UNEXCLUDE, &["genre|ecchi".into()]).await;
        assert_eq!(
            notify::exclusions(&v.pool, 5).await.expect("e"),
            vec![("category".to_string(), "movie".to_string())]
        );
        assert!(v.modal_for(EXCLUDE).is_some() && v.modal_for(TOGGLE).is_none());
    }

    #[tokio::test]
    async fn every_screen_fits_five_rows_and_has_a_close_button() {
        let mut v = view().await;
        for screen in [Screen::Categories, Screen::General] {
            v.screen = screen;
            for cat in 0..notify::CATEGORIES.len() {
                v.cat = cat;
                let json = serde_json::to_string(&v.render(true).components).expect("json");
                assert!(
                    v.render(true).components.len() <= 5 && json.contains("\"close\""),
                    "{screen:?} {cat}"
                );
            }
        }
    }
}
