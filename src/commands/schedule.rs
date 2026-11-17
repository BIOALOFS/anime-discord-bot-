//! `/schedule`: the week's calendar. Anime come from AniList (exact times), dramas and series from TMDB (the day).
//! Day select, category select, "my works only", pages, and a menu that opens any listed work as a new message.

use super::buttons;
use crate::{
    anilist::ContentPolicy,
    components,
    db::notify,
    error::{Context, Error},
    i18n,
    tmdb::find::Cat,
    ui::{
        embeds::{fit, Emb},
        format::weekday_ar,
        pagination::{apply_nav, close_button, nav_row, page_count, page_slice, parse_nav, CLOSE},
        select::{select_row, Choice},
        session::{self, Outcome, Page, View},
    },
};
use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use std::collections::HashSet;

const DAY_ID: &str = "sched_day";
const CAT_ID: &str = "sched_cat";
const MINE_ID: &str = "sched_mine";
const PER_PAGE: usize = 10;
const DAYS: i64 = 7;

/// `(value, Arabic label)` of the category filter; `all` shows everything.
pub const CATEGORIES: &[(&str, &str)] = &[
    ("all", "الكل"),
    ("anime", "أنمي"),
    ("drama", "دراما"),
    ("series", "مسلسلات"),
];

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Provider-prefixed work key (`anilist:1`, `tmdb-tv:2`).
    pub key: String,
    pub title: String,
    /// Exact airing time (UTC seconds) when the source has one.
    pub at: Option<i64>,
    pub date: NaiveDate,
    pub category: &'static str,
    pub episode: Option<i32>,
}

/// Local calendar date of a timestamp.
pub fn local_date(ts: i64, tz: Tz) -> Option<NaiveDate> {
    DateTime::from_timestamp(ts, 0).map(|d| d.with_timezone(&tz).date_naive())
}

/// `HH:MM` in the time zone.
pub fn local_time(ts: i64, tz: Tz) -> String {
    DateTime::from_timestamp(ts, 0)
        .map(|d| d.with_timezone(&tz).format("%H:%M").to_string())
        .unwrap_or_default()
}

/// The seven days starting today in `tz`.
pub fn week_days(now: i64, tz: Tz) -> Vec<NaiveDate> {
    let today = local_date(now, tz).unwrap_or_default();
    (0..DAYS)
        .filter_map(|i| today.checked_add_signed(chrono::Duration::days(i)))
        .collect()
}

/// UTC bounds `[start, end]` of the local days.
pub fn week_bounds(days: &[NaiveDate], tz: Tz) -> Option<(i64, i64)> {
    let first = days.first()?;
    let last = days.last()?;
    let start = tz
        .from_local_datetime(&first.and_hms_opt(0, 0, 0)?)
        .earliest()?
        .timestamp();
    let end = tz
        .from_local_datetime(&last.and_hms_opt(23, 59, 59)?)
        .latest()?
        .timestamp();
    Some((start, end))
}

/// Entries of one day, optionally narrowed to a category and to the member's own works; timed entries first.
pub fn select_entries<'a>(
    all: &'a [Entry],
    day: NaiveDate,
    category: &str,
    mine: Option<&HashSet<String>>,
) -> Vec<&'a Entry> {
    let mut v: Vec<&Entry> = all
        .iter()
        .filter(|e| {
            e.date == day
                && (category == "all" || e.category == category)
                && mine.is_none_or(|m| m.contains(&e.key))
        })
        .collect();
    v.sort_by_key(|e| (e.at.is_none(), e.at.unwrap_or(0), e.title.clone()));
    v
}

fn day_label(d: NaiveDate) -> String {
    format!(
        "{} {:02}/{:02}",
        weekday_ar(d.weekday()),
        d.day(),
        d.month()
    )
}

fn line(e: &Entry, tz: Tz) -> String {
    let time =
        e.at.map(|t| format!("`{}` · ", local_time(t, tz)))
            .unwrap_or_default();
    let ep = e
        .episode
        .map(|n| format!(" — {} {n}", i18n::F_EPISODES))
        .unwrap_or_default();
    let cat = CATEGORIES
        .iter()
        .find(|(v, _)| *v == e.category)
        .map_or("", |(_, l)| *l);
    format!("{time}**{}**{ep} · {cat}", fit(&e.title, 80))
}

/// The member's time zone: their own setting, else the server's, else Muscat.
pub async fn timezone_for(pool: &sqlx::SqlitePool, guild: Option<i64>, user: i64) -> Tz {
    let own: Option<String> =
        sqlx::query_scalar("SELECT timezone FROM notification_prefs WHERE user_id = ?1")
            .bind(user)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    let server: Option<String> = match guild {
        Some(g) => sqlx::query_scalar("SELECT timezone FROM guild_settings WHERE guild_id = ?1")
            .bind(g)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten(),
        None => None,
    };
    own.or(server)
        .and_then(|t| t.parse().ok())
        .unwrap_or(chrono_tz::Asia::Muscat)
}

/// Keys of the works the member follows: profile, reminders, and what they are watching or plan to.
async fn my_keys(pool: &sqlx::SqlitePool, user: i64) -> HashSet<String> {
    let mut set = HashSet::new();
    for sql in [
        "SELECT work_id FROM profile_works WHERE user_id = ?1",
        "SELECT work_key FROM reminders WHERE user_id = ?1 AND work_key != ''",
        "SELECT work_key FROM item_status WHERE user_id = ?1 AND status IN ('current', 'planned')",
    ] {
        if let Ok(rows) = sqlx::query_scalar::<_, String>(sql)
            .bind(user)
            .fetch_all(pool)
            .await
        {
            set.extend(rows);
        }
    }
    set
}

pub struct ScheduleView {
    tz: Tz,
    days: Vec<NaiveDate>,
    entries: Vec<Entry>,
    day: usize,
    page: usize,
    category: String,
    mine: Option<HashSet<String>>,
    pool: sqlx::SqlitePool,
    user: i64,
    notice: Option<String>,
}

impl ScheduleView {
    fn visible(&self) -> Vec<&Entry> {
        select_entries(
            &self.entries,
            self.days[self.day.min(self.days.len().saturating_sub(1))],
            &self.category,
            self.mine.as_ref(),
        )
    }
}

impl View for ScheduleView {
    fn render(&self, enabled: bool) -> Page {
        let day = self.days[self.day.min(self.days.len().saturating_sub(1))];
        let list = self.visible();
        let pages = page_count(list.len(), PER_PAGE);
        let slice = page_slice(&list, self.page, PER_PAGE);
        let body = if slice.is_empty() {
            i18n::SCHEDULE_EMPTY.to_string()
        } else {
            slice
                .iter()
                .map(|e| line(e, self.tz))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut emb = Emb::new()
            .title(format!("{} — {}", i18n::SCHEDULE_TITLE, day_label(day)))
            .description(body);
        emb.footer = Some(format!(
            "{} · {}/{} · {}",
            self.tz.name(),
            self.page.min(pages - 1) + 1,
            pages,
            i18n::SCHEDULE_SOURCES
        ));
        let day_choices: Vec<Choice> = self
            .days
            .iter()
            .enumerate()
            .map(|(i, d)| Choice {
                label: format!(
                    "{} ({})",
                    day_label(*d),
                    select_entries(&self.entries, *d, &self.category, self.mine.as_ref()).len()
                ),
                description: None,
                value: i.to_string(),
            })
            .collect();
        let cat_choices: Vec<Choice> = CATEGORIES
            .iter()
            .map(|(v, l)| Choice {
                label: (*l).to_string(),
                description: None,
                value: (*v).to_string(),
            })
            .collect();
        let items: Vec<(String, String)> = slice
            .iter()
            .map(|e| (fit(&e.title, 100), e.key.clone()))
            .collect();
        let mut rows: Vec<_> =
            select_row(DAY_ID, i18n::AIRING_DAY_PLACEHOLDER, day_choices, enabled)
                .into_iter()
                .collect();
        rows.extend(select_row(
            CAT_ID,
            i18n::SCHEDULE_CATEGORY_PLACEHOLDER,
            cat_choices,
            enabled,
        ));
        rows.extend(components::work_menu(
            i18n::SCHEDULE_OPEN_PLACEHOLDER,
            &items,
        ));
        rows.push(nav_row(self.page.min(pages - 1), pages, enabled));
        let mine_btn = serenity::CreateButton::new(MINE_ID)
            .label(i18n::SCHEDULE_MINE)
            .emoji('⭐')
            .style(if self.mine.is_some() {
                serenity::ButtonStyle::Success
            } else {
                serenity::ButtonStyle::Secondary
            })
            .disabled(!enabled);
        rows.push(buttons(vec![mine_btn, close_button(enabled)]));
        Page::embed(emb.build(), rows).notice(self.notice.clone())
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        self.notice = None;
        match id {
            CLOSE => Outcome::Close,
            DAY_ID => match values
                .first()
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|i| *i < self.days.len())
            {
                Some(i) => {
                    self.day = i;
                    self.page = 0;
                    Outcome::Update
                }
                None => Outcome::Keep,
            },
            CAT_ID => match values
                .first()
                .filter(|v| CATEGORIES.iter().any(|(c, _)| c == v))
            {
                Some(c) => {
                    self.category = c.clone();
                    self.page = 0;
                    Outcome::Update
                }
                None => Outcome::Keep,
            },
            MINE_ID => {
                self.mine = if self.mine.is_some() {
                    None
                } else {
                    Some(my_keys(&self.pool, self.user).await)
                };
                self.page = 0;
                if self.mine.as_ref().is_some_and(HashSet::is_empty) {
                    self.notice = Some(i18n::SCHEDULE_NO_MINE.to_string());
                }
                Outcome::Update
            }
            other => match parse_nav(other) {
                Some(nav) => {
                    let pages = page_count(self.visible().len(), PER_PAGE);
                    let next = apply_nav(nav, self.page, pages);
                    if next == self.page {
                        return Outcome::Keep;
                    }
                    self.page = next;
                    Outcome::Update
                }
                None => Outcome::Keep,
            },
        }
    }
}

use poise::serenity_prelude as serenity;

async fn load(ctx: Context<'_>, tz: Tz, days: &[NaiveDate]) -> Vec<Entry> {
    let data = ctx.data();
    let mut out = Vec::new();
    // Anime: one AniList query for the whole week.
    if let Some((from, to)) = week_bounds(days, tz) {
        if let Ok(list) = data
            .anilist
            .airing_between(from, to, ContentPolicy::SAFE)
            .await
        {
            for e in list {
                if let Some(date) = local_date(e.airing_at, tz) {
                    out.push(Entry {
                        key: format!("anilist:{}", e.media.id),
                        title: e.media.title.main().to_string(),
                        at: Some(e.airing_at),
                        date,
                        category: "anime",
                        episode: Some(e.episode),
                    });
                }
            }
        }
    }
    // Dramas and series: one TMDB listing per day (cached for an hour).
    if data.tmdb.enabled() {
        let overrides = match ctx.guild_id() {
            Some(g) => notify::drama_overrides(&data.db, g.get() as i64)
                .await
                .unwrap_or_default(),
            None => Default::default(),
        };
        let results = futures::future::join_all(days.iter().map(|d| {
            let tmdb = data.tmdb.clone();
            let (date, tzname) = (d.format("%Y-%m-%d").to_string(), tz.name().to_string());
            async move {
                let shows = tmdb.airing_on(&date, &tzname, 1).await;
                (date, shows)
            }
        }))
        .await;
        for (date, shows) in results {
            let Ok(day) = NaiveDate::parse_from_str(&date, "%Y-%m-%d") else {
                continue;
            };
            for w in shows.into_iter().take(40) {
                let category = if Cat::Drama.accepts(&w, &overrides) {
                    "drama"
                } else if Cat::Series.accepts(&w, &overrides) {
                    "series"
                } else {
                    continue;
                };
                out.push(Entry {
                    key: w.key(),
                    title: w.title.clone(),
                    at: None,
                    date: day,
                    category,
                    episode: None,
                });
            }
        }
    }
    out
}

/// This week's calendar of anime, dramas and series.
#[poise::command(
    slash_command,
    name_localized("ar", "جدول"),
    description_localized("ar", "جدول الأسبوع: الأنمي والدراما والمسلسلات")
)]
pub async fn schedule(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let (pool, user) = (ctx.data().db.clone(), ctx.author().id.get() as i64);
    let tz = timezone_for(&pool, ctx.guild_id().map(|g| g.get() as i64), user).await;
    let days = week_days(Utc::now().timestamp(), tz);
    let entries = load(ctx, tz, &days).await;
    session::run(
        ctx,
        ScheduleView {
            tz,
            days,
            entries,
            day: 0,
            page: 0,
            category: "all".into(),
            mine: None,
            pool,
            user,
            notice: None,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(key: &str, title: &str, at: Option<i64>, date: NaiveDate, cat: &'static str) -> Entry {
        Entry {
            key: key.into(),
            title: title.into(),
            at,
            date,
            category: cat,
            episode: None,
        }
    }

    #[test]
    fn a_timestamp_lands_on_the_local_day_of_the_time_zone() {
        // 2026-11-17 21:30 UTC is already the 18th in Muscat (UTC+4).
        let ts = Utc
            .with_ymd_and_hms(2026, 11, 17, 21, 30, 0)
            .single()
            .expect("ts")
            .timestamp();
        assert_eq!(
            local_date(ts, chrono_tz::Asia::Muscat),
            NaiveDate::from_ymd_opt(2026, 11, 18)
        );
        assert_eq!(
            local_date(ts, chrono_tz::UTC),
            NaiveDate::from_ymd_opt(2026, 11, 17)
        );
        assert_eq!(local_time(ts, chrono_tz::Asia::Muscat), "01:30");
    }

    #[test]
    fn the_week_has_seven_days_from_today_and_bounds_cover_all_of_them() {
        let now = Utc
            .with_ymd_and_hms(2026, 11, 17, 10, 0, 0)
            .single()
            .expect("ts")
            .timestamp();
        let days = week_days(now, chrono_tz::Asia::Muscat);
        assert_eq!(days.len(), 7);
        assert_eq!(days[0], NaiveDate::from_ymd_opt(2026, 11, 17).expect("d"));
        assert_eq!(days[6], NaiveDate::from_ymd_opt(2026, 11, 23).expect("d"));
        let (a, b) = week_bounds(&days, chrono_tz::Asia::Muscat).expect("bounds");
        assert_eq!(b - a, 7 * 86_400 - 1);
    }

    #[test]
    fn entries_are_filtered_by_day_category_and_my_works_and_timed_ones_come_first() {
        let d1 = NaiveDate::from_ymd_opt(2026, 11, 17).expect("d");
        let d2 = NaiveDate::from_ymd_opt(2026, 11, 18).expect("d");
        let all = vec![
            e("tmdb-tv:1", "B drama", None, d1, "drama"),
            e("anilist:2", "A anime late", Some(2_000), d1, "anime"),
            e("anilist:3", "A anime early", Some(1_000), d1, "anime"),
            e("tmdb-tv:4", "Other day", None, d2, "series"),
        ];
        let titles = |v: Vec<&Entry>| v.iter().map(|e| e.title.clone()).collect::<Vec<_>>();
        assert_eq!(
            titles(select_entries(&all, d1, "all", None)),
            ["A anime early", "A anime late", "B drama"]
        );
        assert_eq!(titles(select_entries(&all, d1, "drama", None)), ["B drama"]);
        let mine: HashSet<String> = ["anilist:2".to_string()].into();
        assert_eq!(
            titles(select_entries(&all, d1, "all", Some(&mine))),
            ["A anime late"]
        );
        assert_eq!(
            titles(select_entries(&all, d2, "series", None)),
            ["Other day"]
        );
        assert!(select_entries(&all, d2, "anime", None).is_empty());
    }

    #[test]
    fn a_line_shows_the_time_the_episode_and_the_category() {
        let ts = Utc
            .with_ymd_and_hms(2026, 11, 17, 12, 0, 0)
            .single()
            .expect("ts")
            .timestamp();
        let d = NaiveDate::from_ymd_opt(2026, 11, 17).expect("d");
        let mut en = e("anilist:1", "Show", Some(ts), d, "anime");
        en.episode = Some(5);
        let l = line(&en, chrono_tz::Asia::Muscat);
        assert!(
            l.contains("`16:00`")
                && l.contains("**Show**")
                && l.contains("5")
                && l.contains("أنمي"),
            "{l}"
        );
        let l = line(&e("tmdb-tv:1", "Drama", None, d, "drama"), chrono_tz::UTC);
        assert!(!l.contains('`') && l.contains("دراما"));
    }
}
