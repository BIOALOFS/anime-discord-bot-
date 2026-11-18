//! Watch nights: an admin plans a night for a work (date and time in the server's zone), the bot posts an announcement
//! with attendance buttons and a live count, reminds the attendees when it starts and, only if the server allows it
//! and the bot may, also creates a Discord scheduled event.

use crate::{
    components::{self, ItemKind},
    ui::embeds::{fit, Emb},
};
use chrono::TimeZone;
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;

pub const RSVP_PREFIX: &str = "watch:";
/// A reminder is only sent within this time after the start (a restart must not remind about last week).
pub const REMIND_WINDOW_SECS: i64 = 6 * 3600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Night {
    pub id: i64,
    pub guild: i64,
    pub channel: i64,
    pub message: i64,
    pub work_key: String,
    pub title: String,
    pub description: String,
    pub starts_at: i64,
    pub created_by: i64,
    pub reminded: bool,
}

type Row = (i64, i64, i64, i64, String, String, String, i64, i64, i64);

fn night_of(r: Row) -> Night {
    Night {
        id: r.0,
        guild: r.1,
        channel: r.2,
        message: r.3,
        work_key: r.4,
        title: r.5,
        description: r.6,
        starts_at: r.7,
        created_by: r.8,
        reminded: r.9 != 0,
    }
}

const SELECT: &str = "SELECT id, guild_id, channel_id, message_id, work_key, title, description, starts_at, created_by, reminded FROM watch_nights";

/// `work name | 2026-11-21 21:00 | optional description` → (name, timestamp, description). The time is in `tz` and must
/// be in the future.
pub fn parse_input(
    input: &str,
    tz: chrono_tz::Tz,
    now: i64,
) -> Result<(String, i64, String), &'static str> {
    let parts: Vec<&str> = input.splitn(3, '|').map(str::trim).collect();
    let (Some(name), Some(when)) = (parts.first().filter(|n| !n.is_empty()), parts.get(1)) else {
        return Err(
            "اكتب: اسم العمل | التاريخ والوقت | وصف اختياري، مثل: Frieren | 2026-11-21 21:00",
        );
    };
    let naive = chrono::NaiveDateTime::parse_from_str(when, "%Y-%m-%d %H:%M")
        .map_err(|_| "صيغة الوقت غير صحيحة، اكتبها هكذا: 2026-11-21 21:00")?;
    let ts = tz
        .from_local_datetime(&naive)
        .earliest()
        .ok_or("هذا الوقت غير موجود في منطقتك الزمنية")?
        .timestamp();
    if ts <= now {
        return Err("هذا الوقت مضى، اختر وقتًا قادمًا.");
    }
    if ts > now + 90 * 86_400 {
        return Err("اختر موعدًا خلال الأيام التسعين القادمة.");
    }
    Ok((
        (*name).to_string(),
        ts,
        fit(parts.get(2).copied().unwrap_or(""), 300),
    ))
}

pub async fn create(
    pool: &SqlitePool,
    guild: i64,
    channel: i64,
    work_key: &str,
    title: &str,
    description: &str,
    starts_at: i64,
    by: i64,
) -> Result<i64, sqlx::Error> {
    Ok(sqlx::query("INSERT INTO watch_nights (guild_id, channel_id, work_key, title, description, starts_at, created_by) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")
        .bind(guild)
        .bind(channel)
        .bind(work_key)
        .bind(fit(title, 100))
        .bind(description)
        .bind(starts_at)
        .bind(by)
        .execute(pool)
        .await?
        .last_insert_rowid())
}

pub async fn set_message(
    pool: &SqlitePool,
    id: i64,
    message: i64,
    event: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE watch_nights SET message_id = ?2, event_id = ?3 WHERE id = ?1")
        .bind(id)
        .bind(message)
        .bind(event)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<Night>, sqlx::Error> {
    Ok(sqlx::query_as::<_, Row>(&format!("{SELECT} WHERE id = ?1"))
        .bind(id)
        .fetch_optional(pool)
        .await?
        .map(night_of))
}

/// Upcoming nights of a server, soonest first.
pub async fn upcoming(pool: &SqlitePool, guild: i64, now: i64) -> Result<Vec<Night>, sqlx::Error> {
    Ok(sqlx::query_as::<_, Row>(&format!(
        "{SELECT} WHERE guild_id = ?1 AND starts_at > ?2 ORDER BY starts_at LIMIT 10"
    ))
    .bind(guild)
    .bind(now)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(night_of)
    .collect())
}

#[derive(Debug, PartialEq, Eq)]
pub enum Rsvp {
    Going,
    NotGoing,
    /// The night already started or does not exist.
    Closed,
}

/// Set a member's answer (changing it is allowed until the night starts).
pub async fn rsvp(
    pool: &SqlitePool,
    night: i64,
    user: i64,
    going: bool,
    now: i64,
) -> Result<Rsvp, sqlx::Error> {
    let Some(n) = get(pool, night).await? else {
        return Ok(Rsvp::Closed);
    };
    if now >= n.starts_at {
        return Ok(Rsvp::Closed);
    }
    sqlx::query("INSERT INTO watch_night_rsvps (night_id, user_id, going) VALUES (?1, ?2, ?3) ON CONFLICT(night_id, user_id) DO UPDATE SET going = excluded.going").bind(night).bind(user).bind(i64::from(going)).execute(pool).await?;
    Ok(if going { Rsvp::Going } else { Rsvp::NotGoing })
}

/// `(going, not going)`.
pub async fn counts(pool: &SqlitePool, night: i64) -> Result<(i64, i64), sqlx::Error> {
    let going: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM watch_night_rsvps WHERE night_id = ?1 AND going = 1",
    )
    .bind(night)
    .fetch_one(pool)
    .await?;
    let not: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM watch_night_rsvps WHERE night_id = ?1 AND going = 0",
    )
    .bind(night)
    .fetch_one(pool)
    .await?;
    Ok((going, not))
}

pub async fn attendees(pool: &SqlitePool, night: i64) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT user_id FROM watch_night_rsvps WHERE night_id = ?1 AND going = 1 ORDER BY user_id",
    )
    .bind(night)
    .fetch_all(pool)
    .await
}

/// Nights that have started (within the window) and whose reminder was not sent yet.
pub async fn due_reminders(pool: &SqlitePool, now: i64) -> Result<Vec<Night>, sqlx::Error> {
    Ok(sqlx::query_as::<_, Row>(&format!(
        "{SELECT} WHERE reminded = 0 AND starts_at <= ?1 AND starts_at > ?2"
    ))
    .bind(now)
    .bind(now - REMIND_WINDOW_SECS)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(night_of)
    .collect())
}

/// Mark the reminder as sent; `true` the first time only.
pub async fn mark_reminded(pool: &SqlitePool, id: i64) -> Result<bool, sqlx::Error> {
    Ok(
        sqlx::query("UPDATE watch_nights SET reminded = 1 WHERE id = ?1 AND reminded = 0")
            .bind(id)
            .execute(pool)
            .await?
            .rows_affected()
            > 0,
    )
}

/// Old nights are dropped after a week (with their answers).
pub async fn prune(pool: &SqlitePool, now: i64) -> Result<u64, sqlx::Error> {
    let old: Vec<i64> = sqlx::query_scalar("SELECT id FROM watch_nights WHERE starts_at < ?1")
        .bind(now - 7 * 86_400)
        .fetch_all(pool)
        .await?;
    for id in &old {
        sqlx::query("DELETE FROM watch_night_rsvps WHERE night_id = ?1")
            .bind(id)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM watch_nights WHERE id = ?1")
            .bind(id)
            .execute(pool)
            .await?;
    }
    Ok(old.len() as u64)
}

/// The announcement: the work, the time (in everyone's own zone through Discord timestamps) and the live counts.
pub fn embed(n: &Night, counts: (i64, i64)) -> Emb {
    let mut lines = vec![
        format!("🎬 **{}**", n.title),
        format!("🕘 <t:{0}:F> · <t:{0}:R>", n.starts_at),
    ];
    if !n.description.is_empty() {
        lines.push(n.description.clone());
    }
    lines.push(format!(
        "✅ سيحضر: **{}** · ❌ لن يحضر: **{}**",
        counts.0, counts.1
    ));
    Emb::new()
        .title("🍿 ليلة مشاهدة")
        .description(lines.join("\n"))
}

/// Attendance buttons, then the universal button of a work message ("أظهر الشخصيات").
pub fn rows(n: &Night, closed: bool) -> Vec<serenity::CreateActionRow> {
    let btn = |going: u8, label: &str, style| {
        serenity::CreateButton::new(format!("{RSVP_PREFIX}{}:{going}", n.id))
            .label(label)
            .style(style)
            .disabled(closed)
    };
    let mut rows = vec![serenity::CreateActionRow::Buttons(vec![
        btn(1, "سأحضر", serenity::ButtonStyle::Success),
        btn(0, "لن أحضر", serenity::ButtonStyle::Secondary),
    ])];
    let extra = components::standard_actions(ItemKind::Work, Some(&n.work_key));
    if !extra.is_empty() {
        rows.push(serenity::CreateActionRow::Buttons(extra));
    }
    rows
}

/// `watch:7:1` → (7, going).
pub fn parse_rsvp(id: &str) -> Option<(i64, bool)> {
    let (n, g) = id.strip_prefix(RSVP_PREFIX)?.split_once(':')?;
    Some((n.parse().ok()?, g == "1"))
}

/// The reminder text sent to attendees.
pub fn reminder_text(n: &Night) -> String {
    format!("🍿 ليلة المشاهدة تبدأ الآن: **{}**. استمتع!", n.title)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TZ: chrono_tz::Tz = chrono_tz::Asia::Muscat;

    #[test]
    fn the_modal_line_is_parsed_in_the_servers_time_zone_and_must_be_in_the_future() {
        let now = TZ
            .with_ymd_and_hms(2026, 11, 17, 12, 0, 0)
            .single()
            .expect("t")
            .timestamp();
        let (name, ts, desc) =
            parse_input("Frieren | 2026-11-21 21:00 | مع الفشار", TZ, now).expect("ok");
        assert_eq!((name.as_str(), desc.as_str()), ("Frieren", "مع الفشار"));
        assert_eq!(
            ts,
            TZ.with_ymd_and_hms(2026, 11, 21, 21, 0, 0)
                .single()
                .expect("t")
                .timestamp()
        );
        assert!(
            parse_input("Frieren | 2026-11-21 21:00", TZ, now)
                .expect("ok")
                .2
                .is_empty(),
            "the description is optional"
        );
        assert!(
            parse_input("Frieren | 2026-11-10 21:00", TZ, now).is_err(),
            "a past time"
        );
        assert!(
            parse_input("Frieren | 2027-06-01 21:00", TZ, now).is_err(),
            "too far"
        );
        assert!(
            parse_input("Frieren | غدًا", TZ, now).is_err(),
            "bad time format"
        );
        assert!(
            parse_input("| 2026-11-21 21:00", TZ, now).is_err()
                && parse_input("Frieren", TZ, now).is_err()
        );
    }

    #[tokio::test]
    async fn answers_are_one_per_member_changeable_and_closed_after_the_start() {
        let pool = crate::db::connect_memory().await;
        let id = create(&pool, 1, 10, "anilist:5", "Frieren", "", 10_000, 99)
            .await
            .expect("create");
        assert_eq!(rsvp(&pool, id, 1, true, 100).await.expect("r"), Rsvp::Going);
        assert_eq!(rsvp(&pool, id, 2, true, 100).await.expect("r"), Rsvp::Going);
        assert_eq!(
            rsvp(&pool, id, 3, false, 100).await.expect("r"),
            Rsvp::NotGoing
        );
        assert_eq!(counts(&pool, id).await.expect("c"), (2, 1));
        assert_eq!(
            rsvp(&pool, id, 2, false, 200).await.expect("r"),
            Rsvp::NotGoing,
            "changing the answer"
        );
        assert_eq!(counts(&pool, id).await.expect("c"), (1, 2));
        assert_eq!(attendees(&pool, id).await.expect("a"), vec![1]);
        assert_eq!(
            rsvp(&pool, id, 4, true, 10_000).await.expect("r"),
            Rsvp::Closed,
            "after it started"
        );
        assert_eq!(
            rsvp(&pool, 404, 4, true, 1).await.expect("r"),
            Rsvp::Closed,
            "no such night"
        );
    }

    #[tokio::test]
    async fn the_reminder_is_due_at_the_start_sent_once_and_never_for_old_nights() {
        let pool = crate::db::connect_memory().await;
        let soon = create(&pool, 1, 10, "anilist:5", "A", "", 10_000, 1)
            .await
            .expect("n");
        let old = create(
            &pool,
            1,
            10,
            "anilist:6",
            "B",
            "",
            10_000 - REMIND_WINDOW_SECS - 10,
            1,
        )
        .await
        .expect("n");
        assert!(
            due_reminders(&pool, 9_999)
                .await
                .expect("d")
                .iter()
                .all(|n| n.id != soon),
            "not before the start"
        );
        let due: Vec<i64> = due_reminders(&pool, 10_000)
            .await
            .expect("d")
            .iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(
            due,
            vec![soon],
            "the night that started a day ago is not reminded"
        );
        assert!(!due.contains(&old));
        assert!(mark_reminded(&pool, soon).await.expect("m"));
        assert!(!mark_reminded(&pool, soon).await.expect("m"), "once");
        assert!(due_reminders(&pool, 10_100).await.expect("d").is_empty());
    }

    #[tokio::test]
    async fn upcoming_nights_are_listed_per_server_and_old_ones_are_pruned_with_their_answers() {
        let pool = crate::db::connect_memory().await;
        let a = create(&pool, 1, 10, "anilist:5", "A", "", 5_000, 1)
            .await
            .expect("n");
        let _b = create(&pool, 1, 10, "anilist:6", "B", "", 3_000, 1)
            .await
            .expect("n");
        let _c = create(&pool, 2, 10, "anilist:7", "C", "", 4_000, 1)
            .await
            .expect("n");
        let up = upcoming(&pool, 1, 1_000).await.expect("u");
        assert_eq!(
            up.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(),
            ["B", "A"],
            "soonest first, this server only"
        );
        rsvp(&pool, a, 1, true, 10).await.expect("r");
        assert_eq!(prune(&pool, 5_000 + 7 * 86_400 + 1).await.expect("p"), 3);
        assert!(get(&pool, a).await.expect("g").is_none());
        let answers: i64 = sqlx::query_scalar("SELECT count(*) FROM watch_night_rsvps")
            .fetch_one(&pool)
            .await
            .expect("n");
        assert_eq!(answers, 0);
    }

    #[test]
    fn the_announcement_shows_the_time_the_counts_and_the_universal_button() {
        let n = Night {
            id: 7,
            guild: 1,
            channel: 2,
            message: 0,
            work_key: "anilist:5".into(),
            title: "Frieren".into(),
            description: "مع الفشار".into(),
            starts_at: 12_345,
            created_by: 1,
            reminded: false,
        };
        let d = embed(&n, (3, 1)).description.unwrap_or_default();
        assert!(
            d.contains("<t:12345:F>")
                && d.contains("سيحضر: **3**")
                && d.contains("لن يحضر: **1**")
                && d.contains("مع الفشار"),
            "{d}"
        );
        let json = serde_json::to_string(&rows(&n, false)).expect("json");
        assert!(
            json.contains("watch:7:1")
                && json.contains("watch:7:0")
                && json.contains("showchars:anilist:5")
                && json.contains("أظهر الشخصيات"),
            "{json}"
        );
        assert!(serde_json::to_string(&rows(&n, true))
            .expect("json")
            .contains("\"disabled\":true"));
        assert_eq!(parse_rsvp("watch:7:1"), Some((7, true)));
        assert_eq!(parse_rsvp("watch:7:0"), Some((7, false)));
        assert_eq!(parse_rsvp("poll:7:0"), None);
        assert!(reminder_text(&n).contains("Frieren"));
    }
}
