//! `/airing [anime]`: next episode of one anime, or this week's schedule.

use super::{buttons, fail, media::media_autocomplete, parse_id, policy, say_ephemeral};
use crate::{
    anilist::{
        models::Media, AiringEntry, AiringInfo, AniError, ContentPolicy, MediaKind, SafeAniList,
    },
    error::{Context, Error},
    i18n,
    ui::{
        cards,
        format::weekday_ar,
        pagination::{
            apply_nav, back_button, close_button, nav_row, page_count, page_slice, parse_nav, BACK,
            CLOSE,
        },
        select::{media_choices, pick_row, select_row, Choice, PICK_ID},
        session::{self, Outcome, Page, View},
    },
};
use chrono::{DateTime, Datelike, NaiveDate};
use poise::serenity_prelude as serenity;
use std::sync::Arc;

const DAY_ID: &str = "day";
const PER_PAGE: usize = 12;
const WEEK_SECS: i64 = 7 * 24 * 3600;

pub struct Day {
    pub date: NaiveDate,
    pub entries: Vec<AiringEntry>,
}

impl Day {
    fn label(&self) -> String {
        format!(
            "{} {:02}/{:02}",
            weekday_ar(self.date.weekday()),
            self.date.day(),
            self.date.month()
        )
    }
}

/// Group (already ascending) entries by UTC calendar day.
pub fn group_by_day(entries: Vec<AiringEntry>) -> Vec<Day> {
    let mut days: Vec<Day> = Vec::new();
    for e in entries {
        let Some(date) = DateTime::from_timestamp(e.airing_at, 0).map(|d| d.date_naive()) else {
            continue;
        };
        match days.last_mut() {
            Some(d) if d.date == date => d.entries.push(e),
            _ => days.push(Day {
                date,
                entries: vec![e],
            }),
        }
    }
    days
}

enum Mode {
    Pick(Vec<Media>),
    Single(Box<AiringInfo>),
    Week {
        days: Vec<Day>,
        day: usize,
        page: usize,
    },
}

struct AiringView {
    api: Arc<SafeAniList>,
    policy: ContentPolicy,
    mode: Mode,
    /// Only the picker can be returned to.
    picker: Option<Vec<Media>>,
    notice: Option<String>,
}

impl AiringView {
    fn fail(&mut self, e: AniError) -> Outcome {
        tracing::warn!("anilist error in airing view: {e}");
        self.notice = Some(super::ani_message(&e).to_string());
        Outcome::Update
    }
}

impl View for AiringView {
    fn render(&self, enabled: bool) -> Page {
        let (embed, mut rows) = match &self.mode {
            Mode::Pick(list) => (
                cards::picker_embed(list),
                pick_row(media_choices(list), enabled)
                    .into_iter()
                    .collect::<Vec<_>>(),
            ),
            Mode::Single(info) => {
                let mut row = Vec::new();
                if let Some(url) = &info.media.site_url {
                    row.push(
                        serenity::CreateButton::new_link(url)
                            .label(format!("🔗 {}", i18n::BTN_ANILIST)),
                    );
                }
                if self.picker.is_some() {
                    row.push(back_button(enabled));
                }
                row.push(close_button(enabled));
                return Page::embed(cards::airing_info_embed(info).build(), vec![buttons(row)])
                    .notice(self.notice.clone());
            }
            Mode::Week { days, day, page } => {
                let choices: Vec<Choice> = days
                    .iter()
                    .enumerate()
                    .map(|(i, d)| Choice {
                        label: format!("{} ({})", d.label(), d.entries.len()),
                        description: None,
                        value: i.to_string(),
                    })
                    .collect();
                let current = &days[(*day).min(days.len().saturating_sub(1))];
                let refs: Vec<&AiringEntry> = current.entries.iter().collect();
                let pages = page_count(refs.len(), PER_PAGE);
                let slice = page_slice(&refs, *page, PER_PAGE);
                let mut rows: Vec<_> =
                    select_row(DAY_ID, i18n::AIRING_DAY_PLACEHOLDER, choices, enabled)
                        .into_iter()
                        .collect();
                rows.push(nav_row(*page, pages, enabled));
                (
                    cards::airing_day_embed(&current.label(), slice, *page, pages),
                    rows,
                )
            }
        };
        let mut row = Vec::new();
        if matches!(self.mode, Mode::Single(_)) && self.picker.is_some() {
            row.push(back_button(enabled));
        }
        row.push(close_button(enabled));
        rows.push(buttons(row));
        Page::embed(embed.build(), rows).notice(self.notice.clone())
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        self.notice = None;
        match id {
            CLOSE => Outcome::Close,
            BACK => match self.picker.take() {
                Some(list) => {
                    self.mode = Mode::Pick(list);
                    Outcome::Update
                }
                None => Outcome::Keep,
            },
            PICK_ID => {
                let Some(mid) = values.first().and_then(|v| v.parse::<i64>().ok()) else {
                    return Outcome::Keep;
                };
                let Mode::Pick(list) = &self.mode else {
                    return Outcome::Keep;
                };
                let list = list.clone();
                match self
                    .api
                    .airing_for_media(mid, chrono::Utc::now().timestamp(), self.policy)
                    .await
                {
                    Ok(Some(info)) => {
                        self.picker = Some(list);
                        self.mode = Mode::Single(Box::new(info));
                    }
                    Ok(None) => self.notice = Some(i18n::NOT_FOUND.to_string()),
                    Err(e) => return self.fail(e),
                }
                Outcome::Update
            }
            DAY_ID => {
                let Mode::Week { days, day, page } = &mut self.mode else {
                    return Outcome::Keep;
                };
                let Some(i) = values
                    .first()
                    .and_then(|v| v.parse::<usize>().ok())
                    .filter(|i| *i < days.len())
                else {
                    return Outcome::Keep;
                };
                *day = i;
                *page = 0;
                Outcome::Update
            }
            other => {
                let (Some(nav), Mode::Week { days, day, page }) =
                    (parse_nav(other), &mut self.mode)
                else {
                    return Outcome::Keep;
                };
                let len = days.get(*day).map_or(0, |d| d.entries.len());
                let next = apply_nav(nav, *page, page_count(len, PER_PAGE));
                if next == *page {
                    return Outcome::Keep;
                }
                *page = next;
                Outcome::Update
            }
        }
    }
}

async fn ac_airing(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    media_autocomplete(ctx, MediaKind::Anime, "airing", partial).await
}

/// Airing schedule.
#[poise::command(
    slash_command,
    name_localized("ar", "عرض"),
    description_localized("ar", "موعد الحلقة القادمة أو جدول الأسبوع")
)]
pub async fn airing(
    ctx: Context<'_>,
    #[description = "Anime title (leave empty for this week's schedule)"]
    #[description_localized("ar", "اسم الأنمي (اتركه فارغًا لجدول الأسبوع)")]
    #[name_localized("ar", "الأنمي")]
    #[autocomplete = "ac_airing"]
    anime: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let api = ctx.data().anilist.clone();
    let policy = policy(ctx);
    let now = chrono::Utc::now().timestamp();

    let mode = match anime.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => match api.airing_between(now, now + WEEK_SECS, policy).await {
            Ok(entries) => {
                let days = group_by_day(entries);
                if days.is_empty() {
                    return say_ephemeral(ctx, i18n::AIRING_EMPTY).await;
                }
                Mode::Week {
                    days,
                    day: 0,
                    page: 0,
                }
            }
            Err(e) => return fail(ctx, e).await,
        },
        Some(input) => {
            let id = match parse_id(input) {
                Some(id) => id,
                None => match api.search_media(input, MediaKind::Anime, 10, policy).await {
                    Ok(v) if v.is_empty() => return say_ephemeral(ctx, i18n::NO_RESULTS).await,
                    Ok(v) if v.len() > 1 => {
                        let view = AiringView {
                            api,
                            policy,
                            mode: Mode::Pick(v),
                            picker: None,
                            notice: None,
                        };
                        return session::run(ctx, view).await;
                    }
                    Ok(v) => v[0].id,
                    Err(e) => return fail(ctx, e).await,
                },
            };
            match api.airing_for_media(id, now, policy).await {
                Ok(Some(info)) => Mode::Single(Box::new(info)),
                Ok(None) => return say_ephemeral(ctx, i18n::NOT_FOUND).await,
                Err(e) => return fail(ctx, e).await,
            }
        }
    };
    session::run(
        ctx,
        AiringView {
            api,
            policy,
            mode,
            picker: None,
            notice: None,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(at: i64, id: i64) -> AiringEntry {
        AiringEntry {
            airing_at: at,
            episode: 1,
            media: serde_json::from_value(json!({"id": id, "title": {"romaji": "x"}}))
                .expect("media"),
        }
    }

    #[test]
    fn groups_by_utc_day_keeping_order() {
        // 2026-11-14 23:00 UTC, 2026-11-15 00:30 UTC, 2026-11-15 10:00 UTC
        let base = 1_794_697_200_i64; // 2026-11-14 23:00:00 UTC
        let days = group_by_day(vec![
            entry(base, 1),
            entry(base + 5_400, 2),
            entry(base + 39_600, 3),
        ]);
        assert_eq!(days.len(), 2);
        assert_eq!(days[0].entries.len(), 1);
        assert_eq!(days[1].entries.len(), 2);
        assert_eq!(days[0].date.to_string(), "2026-11-14");
        assert!(days[0].label().starts_with("السبت"));
    }

    #[test]
    fn empty_schedule_has_no_days() {
        assert!(group_by_day(Vec::new()).is_empty());
    }
}
