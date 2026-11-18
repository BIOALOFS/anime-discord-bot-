//! Reminders for dramas, series and films (TMDB): a "coming soon" notice some days before an episode or a release,
//! and a notice on the day itself. Every notice is claimed in `notified_events` first, so it reaches a member once.

use super::{notify_user_with, Feed};
use crate::{
    components::{self, ItemKind},
    db::{notify, reminders},
    i18n,
    tmdb::{
        card::overview,
        model::{self as tm, Kind, Work},
        FOOTER_CREDIT,
    },
    ui::embeds::Emb,
};
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use poise::serenity_prelude as serenity;

/// What a notice announces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// An episode or a film arrives in `days` days (`date`).
    Soon {
        id: String,
        date: String,
        days: i64,
        label: String,
    },
    /// An episode or a film is out today.
    Out { id: String, label: String },
}

impl Event {
    pub fn id(&self) -> &str {
        match self {
            Self::Soon { id, .. } | Self::Out { id, .. } => id,
        }
    }
}

fn date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

fn episode_label(season: i64, episode: i64) -> String {
    format!("الموسم {season} · الحلقة {episode}")
}

/// The notices a work owes on `today` (in the member's time zone) with a lead time of `notify_days`.
pub fn due_events(w: &Work, today: NaiveDate, notify_days: i64) -> Vec<Event> {
    let mut out = Vec::new();
    let mut soon = |d: &str, id: String, label: String| {
        if let Some(day) = date(d) {
            let days = (day - today).num_days();
            if days > 0 && days <= notify_days.clamp(1, 14) {
                out.push(Event::Soon {
                    id,
                    date: d.to_string(),
                    days,
                    label,
                });
            }
        }
    };
    match w.kind {
        Some(Kind::Movie) => {
            if let Some(d) = &w.release_date {
                soon(d, format!("soon:{d}"), String::new());
                if date(d) == Some(today) {
                    out.push(Event::Out {
                        id: format!("release:{d}"),
                        label: String::new(),
                    });
                }
            }
        }
        _ => {
            if let Some((d, s, e)) = &w.next_ep {
                soon(d, format!("soon:s{s}e{e}"), episode_label(*s, *e));
            }
            if let Some((d, s, e)) = &w.last_ep {
                // Today's episode, or yesterday's if the data arrived late; never an older backlog.
                let age = date(d).map(|day| (today - day).num_days());
                if matches!(age, Some(0 | 1)) {
                    out.push(Event::Out {
                        id: format!("ep:s{s}e{e}"),
                        label: episode_label(*s, *e),
                    });
                }
            }
        }
    }
    out
}

/// The member's local date.
pub fn local_today(now: i64, tz: Tz) -> NaiveDate {
    DateTime::<Utc>::from_timestamp(now, 0)
        .map(|d| d.with_timezone(&tz).date_naive())
        .unwrap_or_default()
}

pub fn embed(w: &Work, ev: &Event) -> Emb {
    let kind_ar = if w.kind == Some(Kind::Movie) {
        "الفيلم"
    } else {
        "المسلسل"
    };
    let headline = match ev {
        Event::Soon {
            days, label, date, ..
        } => {
            let when = if *days == 1 {
                "غدًا".to_string()
            } else {
                format!("بعد {days} أيام")
            };
            let what = if label.is_empty() {
                format!("يصدر {kind_ar}")
            } else {
                format!("تصدر {label}")
            };
            format!("⏳ قريبًا: {what} {when} ({date})")
        }
        Event::Out { label, .. } => {
            if label.is_empty() {
                format!("🎬 صدر {kind_ar} اليوم")
            } else {
                format!("📺 صدرت اليوم: {label}")
            }
        }
    };
    let mut e = Emb::new()
        .title(w.title.clone())
        .url(Some(w.url()))
        .description(format!("{headline}\n\n{}", overview(&w.overview, false)));
    e.image = w.poster.clone();
    e.footer = Some(FOOTER_CREDIT.to_string());
    e
}

/// One pass: every TMDB reminder, grouped by work so each work is fetched once (the details are cached anyway).
pub async fn poll(f: &Feed, now: i64) -> anyhow::Result<()> {
    if !f.tmdb.enabled() {
        return Ok(());
    }
    let rows: Vec<(i64, String, String, i64)> =
        sqlx::query_as("SELECT user_id, work_key, category, notify_days FROM reminders WHERE work_key LIKE 'tmdb-%' ORDER BY work_key LIMIT 2000").fetch_all(&f.pool).await?;
    let mut by_work: std::collections::BTreeMap<String, Vec<(i64, String, i64)>> =
        Default::default();
    for (user, key, category, days) in rows {
        by_work.entry(key).or_default().push((user, category, days));
    }
    for (key, members) in by_work.into_iter().take(150) {
        let Some((kind, id)) = tm::parse_work_key(&key) else {
            continue;
        };
        let Some(work) = f.tmdb.details(kind, id).await else {
            continue;
        };
        for (user, category, days) in members {
            if let Err(e) = notify_member(f, &work, &key, user, &category, days, now).await {
                tracing::warn!("tmdb reminder for {key} failed: {e:#}");
            }
        }
    }
    notify::prune_events(&f.pool, 60).await?;
    Ok(())
}

async fn notify_member(
    f: &Feed,
    w: &Work,
    key: &str,
    user: i64,
    category: &str,
    days: i64,
    now: i64,
) -> anyhow::Result<()> {
    let prefs = notify::prefs(&f.pool, user).await?;
    let tz: Tz = prefs.timezone.parse().unwrap_or(chrono_tz::Asia::Muscat);
    let events = due_events(w, local_today(now, tz), days);
    if events.is_empty() || !notify::allows(&f.pool, user, key, category, None, &w.genres).await? {
        return Ok(());
    }
    for ev in events {
        // Claimed first: whatever happens next, this notice never reaches this member twice.
        if !notify::claim_event(&f.pool, user, key, ev.id()).await? {
            continue;
        }
        let mut buttons = components::standard_actions(ItemKind::Work, Some(key));
        buttons.push(serenity::CreateButton::new_link(w.url()).label("🔗 TMDB"));
        let text = if matches!(ev, Event::Soon { .. }) {
            i18n::REMINDER_SOON
        } else {
            i18n::REMINDER_OUT
        };
        notify_user_with(
            f,
            user,
            text,
            embed(w, &ev).build(),
            vec![serenity::CreateActionRow::Buttons(buttons)],
        )
        .await;
    }
    let _ = reminders::MAX_PER_USER;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("date")
    }

    fn show(next: Option<(&str, i64, i64)>, last: Option<(&str, i64, i64)>) -> Work {
        Work {
            id: 1,
            kind: Some(Kind::Tv),
            title: "Show".into(),
            next_ep: next.map(|(d, s, e)| (d.into(), s, e)),
            last_ep: last.map(|(d, s, e)| (d.into(), s, e)),
            ..Work::default()
        }
    }

    #[test]
    fn a_coming_soon_notice_fires_only_inside_the_lead_time() {
        let w = show(Some(("2026-11-20", 2, 5)), None);
        assert!(
            due_events(&w, day("2026-11-17"), 2).is_empty(),
            "three days out, lead time two"
        );
        let e = due_events(&w, day("2026-11-18"), 2);
        assert_eq!(
            e,
            vec![Event::Soon {
                id: "soon:s2e5".into(),
                date: "2026-11-20".into(),
                days: 2,
                label: "الموسم 2 · الحلقة 5".into()
            }]
        );
        assert_eq!(due_events(&w, day("2026-11-19"), 2).len(), 1);
        assert!(
            due_events(&w, day("2026-11-20"), 2).is_empty(),
            "on the day the 'out' notice takes over"
        );
    }

    #[test]
    fn an_episode_that_aired_today_or_yesterday_is_announced_but_old_ones_are_not() {
        let today = day("2026-11-17");
        for (aired, owed) in [
            ("2026-11-17", true),
            ("2026-11-16", true),
            ("2026-11-10", false),
            ("2026-11-18", false),
        ] {
            let e = due_events(&show(None, Some((aired, 1, 3))), today, 1);
            assert_eq!(!e.is_empty(), owed, "aired {aired}");
            if owed {
                assert_eq!(
                    e[0],
                    Event::Out {
                        id: "ep:s1e3".into(),
                        label: "الموسم 1 · الحلقة 3".into()
                    }
                );
            }
        }
    }

    #[test]
    fn a_film_has_a_soon_notice_and_a_release_day_notice() {
        let w = Work {
            kind: Some(Kind::Movie),
            release_date: Some("2026-12-01".into()),
            ..Work::default()
        };
        assert_eq!(due_events(&w, day("2026-11-30"), 1).len(), 1);
        assert!(
            matches!(&due_events(&w, day("2026-12-01"), 1)[..], [Event::Out { id, .. }] if id == "release:2026-12-01")
        );
        assert!(due_events(&w, day("2026-11-25"), 1).is_empty());
        assert!(
            due_events(&w, day("2026-12-09"), 1).is_empty(),
            "a film already out is not announced again"
        );
    }

    #[test]
    fn the_member_local_day_follows_their_time_zone() {
        let ts = Utc::now()
            .date_naive()
            .and_hms_opt(21, 30, 0)
            .expect("t")
            .and_utc()
            .timestamp();
        let muscat = local_today(ts, chrono_tz::Asia::Muscat);
        let utc = local_today(ts, chrono_tz::UTC);
        assert_eq!(
            (muscat - utc).num_days(),
            1,
            "21:30 UTC is already tomorrow in Muscat"
        );
    }

    #[test]
    fn the_notice_names_the_episode_the_lead_time_and_credits_tmdb() {
        let w = Work {
            id: 9,
            kind: Some(Kind::Tv),
            title: "Show".into(),
            overview: "About".into(),
            ..Work::default()
        };
        let soon = embed(
            &w,
            &Event::Soon {
                id: "x".into(),
                date: "2026-11-20".into(),
                days: 1,
                label: "الموسم 1 · الحلقة 2".into(),
            },
        );
        let d = soon.description.as_deref().unwrap_or("");
        assert!(
            d.contains("غدًا") && d.contains("الحلقة 2") && d.contains("2026-11-20"),
            "{d}"
        );
        assert_eq!(soon.footer.as_deref(), Some("المصدر: TMDB"));
        let out = embed(
            &w,
            &Event::Out {
                id: "y".into(),
                label: String::new(),
            },
        );
        assert!(out
            .description
            .as_deref()
            .unwrap_or("")
            .contains("صدر المسلسل اليوم"));
        let film = Work {
            kind: Some(Kind::Movie),
            ..w
        };
        assert!(embed(
            &film,
            &Event::Soon {
                id: "z".into(),
                date: "d".into(),
                days: 3,
                label: String::new()
            }
        )
        .description
        .as_deref()
        .unwrap_or("")
        .contains("بعد 3 أيام"));
    }
}
