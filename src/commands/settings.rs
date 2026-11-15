//! `/settings`: personal preferences only (language, privacy and your release reminders). Server administration
//! lives in `/admin`.

use super::{buttons, profile::option_str, say_ephemeral};
use crate::{
    db::{
        profile::{self as pdb, Privacy},
        reminders::{self, Reminder},
    },
    error::{Context, Error},
    i18n,
    ui::{
        autocomplete::to_choices,
        embeds::{fit, Emb},
        pagination::{apply_nav, close_button, nav_row, page_count, parse_nav, CLOSE},
        select::{normalize_choices, select_row, Choice},
        session::{self, Outcome, Page, View},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;

/// `(key, Arabic label)` of the personal settings.
pub const SETTINGS: &[(&str, &str)] = &[
    ("language", "اللغة"),
    ("privacy", "الخصوصية"),
    ("reminders", "التذكيرات"),
];

pub const REMINDER_VALUES: &[(&str, &str)] = &[("list", "عرض تذكيراتي وحذفها")];

pub const LANGUAGES: &[(&str, &str)] = &[("ar", "العربية"), ("en", "English")];
pub const PRIVACY_LEVELS: &[(&str, &str)] = &[
    ("public", "عام"),
    ("servers", "السيرفرات المشتركة"),
    ("private", "خاص"),
];

/// Options of one setting as `(value, label)` pairs.
pub fn values_for(setting: &str) -> &'static [(&'static str, &'static str)] {
    match setting {
        "language" => LANGUAGES,
        "privacy" => PRIVACY_LEVELS,
        "reminders" => REMINDER_VALUES,
        _ => &[],
    }
}

fn matching(items: Vec<(String, String)>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let needle = partial.trim().to_lowercase();
    to_choices(
        items
            .into_iter()
            .filter(|(l, v)| {
                needle.is_empty() || l.to_lowercase().contains(&needle) || v.contains(&needle)
            })
            .collect(),
    )
}

async fn ac_setting(_ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    matching(
        SETTINGS
            .iter()
            .map(|(k, l)| ((*l).to_string(), (*k).to_string()))
            .collect(),
        partial,
    )
}

async fn ac_value(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let Some(setting) = option_str(ctx, "setting") else {
        return Vec::new();
    };
    matching(
        values_for(&setting)
            .iter()
            .map(|(v, l)| ((*l).to_string(), (*v).to_string()))
            .collect(),
        partial,
    )
}

/// Your personal preferences: language, privacy and reminders.
#[poise::command(
    slash_command,
    name_localized("ar", "إعداداتي"),
    description_localized("ar", "تفضيلاتك الشخصية: اللغة والخصوصية والتذكيرات")
)]
pub async fn settings(
    ctx: Context<'_>,
    #[description = "Setting: language or privacy"]
    #[description_localized("ar", "الإعداد: اللغة أو الخصوصية")]
    #[name_localized("ar", "الإعداد")]
    #[autocomplete = "ac_setting"]
    setting: String,
    #[description = "New value"]
    #[description_localized("ar", "القيمة الجديدة")]
    #[name_localized("ar", "القيمة")]
    #[autocomplete = "ac_value"]
    value: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let user = ctx.author().id.get() as i64;
    let pool = &ctx.data().db;
    if setting.trim() == "reminders" {
        let mut view = RemindersView {
            pool: pool.clone(),
            user,
            page: 0,
            total: 0,
            rows: Vec::new(),
            notice: None,
        };
        view.load().await?;
        return session::run(ctx, view).await;
    }
    pdb::create_profile(pool, user).await?;
    let value = value.as_deref().map(str::trim).unwrap_or("");
    if value.is_empty() {
        // No value: show the current one.
        let profile = pdb::get_profile(pool, user).await?;
        let text = match (setting.trim(), profile) {
            ("language", Some(p)) => i18n::current_language(
                LANGUAGES
                    .iter()
                    .find(|(k, _)| *k == p.language)
                    .map_or("?", |(_, l)| l),
            ),
            ("privacy", Some(p)) => i18n::current_privacy(i18n::privacy_ar(&p.privacy)),
            _ => i18n::SETTINGS_UNKNOWN.to_string(),
        };
        return say_ephemeral(ctx, &text).await;
    }
    let text = match setting.trim() {
        "privacy" => match Privacy::parse(value) {
            Some(level) => {
                pdb::set_privacy(pool, user, level).await?;
                i18n::PRIVACY_SET
            }
            None => i18n::PRIVACY_UNKNOWN,
        },
        "language" => match pdb::set_language(pool, user, value).await {
            Ok(()) if value == "ar" => i18n::LANGUAGE_SET,
            Ok(()) => i18n::LANGUAGE_SET_EN,
            Err(_) => i18n::LANGUAGE_UNKNOWN,
        },
        _ => i18n::SETTINGS_UNKNOWN,
    };
    say_ephemeral(ctx, text).await
}

const PAGE: usize = 25;
const REMOVE_PICK: &str = "reminder_remove";

/// Options for cancelling reminders: the work title, the work id as the value.
pub fn reminder_choices(list: &[Reminder]) -> Vec<Choice> {
    normalize_choices(
        list.iter()
            .map(|r| Choice {
                label: fit(if r.title.is_empty() { "?" } else { &r.title }, 100),
                description: None,
                value: r.work_id.to_string(),
            })
            .collect(),
    )
}

struct RemindersView {
    pool: SqlitePool,
    user: i64,
    page: usize,
    total: i64,
    rows: Vec<Reminder>,
    notice: Option<String>,
}

impl RemindersView {
    async fn load(&mut self) -> Result<(), sqlx::Error> {
        let all = reminders::list(&self.pool, self.user).await?;
        self.total = all.len() as i64;
        self.page = self.page.min(page_count(all.len(), PAGE) - 1);
        self.rows = all.into_iter().skip(self.page * PAGE).take(PAGE).collect();
        Ok(())
    }
}

impl View for RemindersView {
    fn ephemeral(&self) -> bool {
        true
    }

    fn render(&self, enabled: bool) -> Page {
        let pages = page_count(self.total as usize, PAGE);
        let body = if self.rows.is_empty() {
            i18n::REMINDERS_NONE.to_string()
        } else {
            self.rows
                .iter()
                .enumerate()
                .map(|(i, r)| format!("**{}.** {}", self.page * PAGE + i + 1, r.title))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut embed = Emb::new().title(i18n::REMINDERS_TITLE).description(body);
        embed.footer = None;
        let mut rows: Vec<_> = select_row(
            REMOVE_PICK,
            i18n::REMINDERS_PLACEHOLDER,
            reminder_choices(&self.rows),
            enabled,
        )
        .into_iter()
        .collect();
        if pages > 1 {
            rows.push(nav_row(self.page, pages, enabled));
        }
        rows.push(buttons(vec![close_button(enabled)]));
        Page::embed(embed.build(), rows).notice(self.notice.clone())
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        self.notice = None;
        match id {
            CLOSE => return Outcome::Close,
            REMOVE_PICK => {
                let Some(work) = values.first().and_then(|v| v.parse::<i64>().ok()) else {
                    return Outcome::Keep;
                };
                self.notice = Some(match reminders::remove(&self.pool, self.user, work).await {
                    Ok(_) => i18n::REMINDER_CANCELLED.to_string(),
                    Err(_) => i18n::ERR_GENERIC.to_string(),
                });
            }
            other => {
                let Some(nav) = parse_nav(other) else {
                    return Outcome::Keep;
                };
                let next = apply_nav(nav, self.page, page_count(self.total as usize, PAGE));
                if next == self.page {
                    return Outcome::Keep;
                }
                self.page = next;
            }
        }
        if self.load().await.is_err() {
            self.notice = Some(i18n::ERR_GENERIC.to_string());
        }
        Outcome::Update
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminder_options_are_unique_short_and_capped() {
        let list: Vec<Reminder> = (0..60)
            .map(|i| Reminder {
                user_id: 1,
                work_id: i % 40,
                title: "T ".repeat(120),
                last_episode: 0,
            })
            .collect();
        let c = reminder_choices(&list);
        assert_eq!(c.len(), 25);
        let mut seen = std::collections::HashSet::new();
        assert!(c
            .iter()
            .all(|c| seen.insert(c.value.clone()) && c.label.chars().count() <= 100));
    }

    #[test]
    fn only_language_privacy_and_reminders_exist() {
        assert_eq!(
            SETTINGS.iter().map(|s| s.0).collect::<Vec<_>>(),
            vec!["language", "privacy", "reminders"]
        );
        assert_eq!(values_for("reminders").len(), 1);
        assert_eq!(values_for("language").len(), 2);
        assert_eq!(values_for("privacy").len(), 3);
        assert!(
            values_for("nsfw").is_empty(),
            "admin settings are not personal settings"
        );
    }

    #[test]
    fn every_privacy_value_parses() {
        for (v, _) in PRIVACY_LEVELS {
            assert!(Privacy::parse(v).is_some(), "{v}");
        }
    }
}
