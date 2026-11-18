//! The weekly "ماذا نشاهد؟" poll: built from the server's `/suggest` entries (3 to 5 not yet polled), one vote per
//! member (changeable) with live counts, closed automatically, and a result message that opens the winning pair.

use super::settings;
use crate::{
    components,
    ui::embeds::{fit, Emb},
};
use chrono::{Datelike, Timelike};
use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

pub const FEATURE: &str = "poll";
pub const MIN_OPTIONS: usize = 3;
pub const MAX_OPTIONS: usize = 5;
pub const VOTE_PREFIX: &str = "poll:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opt {
    /// The suggestion id.
    pub suggestion: i64,
    pub first_key: String,
    pub second_key: String,
    pub label: String,
}

/// Settings read from `guild_features.extra` (`day` 0 = Monday ... 6 = Sunday, `hours` = how long it stays open).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub day: u32,
    pub hour: u32,
    pub duration_hours: i64,
}

pub const DURATIONS: &[i64] = &[6, 12, 24, 48, 72];

impl Config {
    pub fn from(f: &settings::Feature) -> Self {
        Self {
            day: f.value("day").filter(|d| (0..7).contains(d)).unwrap_or(4) as u32,
            hour: f.hour.clamp(0, 23) as u32,
            duration_hours: f
                .value("hours")
                .filter(|h| DURATIONS.contains(h))
                .unwrap_or(24),
        }
    }
}

/// Is it time to start this week's poll? On the chosen weekday, from the chosen hour, in the server's time zone.
pub fn should_start(now: i64, tz: chrono_tz::Tz, c: &Config) -> bool {
    chrono::DateTime::from_timestamp(now, 0)
        .map(|d| d.with_timezone(&tz))
        .is_some_and(|d| d.weekday().num_days_from_monday() == c.day && d.hour() >= c.hour)
}

/// Suggestions that were never in a poll of this server, newest first, at most `MAX_OPTIONS`. Fewer than `MIN_OPTIONS`
/// means no poll this week.
pub async fn pick_options(pool: &SqlitePool, guild: i64) -> Result<Vec<Opt>, sqlx::Error> {
    let used: Vec<String> = sqlx::query_scalar("SELECT options FROM polls WHERE guild_id = ?1")
        .bind(guild)
        .fetch_all(pool)
        .await?;
    let polled: std::collections::HashSet<i64> = used
        .iter()
        .filter_map(|j| serde_json::from_str::<Vec<Opt>>(j).ok())
        .flatten()
        .map(|o| o.suggestion)
        .collect();
    let rows: Vec<(i64, String, String, String, String)> =
        sqlx::query_as("SELECT id, first_id, second_id, first_title, second_title FROM suggestions WHERE guild_id = ?1 ORDER BY created_at DESC, id DESC LIMIT 60").bind(guild).fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .filter(|(id, ..)| !polled.contains(id))
        .take(MAX_OPTIONS)
        .map(|(id, a, b, ta, tb)| Opt {
            suggestion: id,
            first_key: a,
            second_key: b,
            label: fit(&format!("{ta} + {tb}"), 80),
        })
        .collect())
}

/// Create a poll record; the caller posts the message and stores its id.
pub async fn create(
    pool: &SqlitePool,
    guild: i64,
    channel: i64,
    options: &[Opt],
    closes_at: i64,
) -> Result<i64, sqlx::Error> {
    let json = serde_json::to_string(options).unwrap_or_else(|_| "[]".into());
    Ok(sqlx::query(
        "INSERT INTO polls (guild_id, channel_id, options, closes_at) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(guild)
    .bind(channel)
    .bind(json)
    .bind(closes_at)
    .execute(pool)
    .await?
    .last_insert_rowid())
}

pub async fn set_message(pool: &SqlitePool, poll: i64, message: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE polls SET message_id = ?2 WHERE id = ?1")
        .bind(poll)
        .bind(message)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Poll {
    pub id: i64,
    pub guild: i64,
    pub channel: i64,
    pub message: i64,
    pub options: Vec<Opt>,
    pub closes_at: i64,
    pub closed: bool,
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<Poll>, sqlx::Error> {
    let row: Option<(i64, i64, i64, String, i64, i64)> = sqlx::query_as("SELECT guild_id, channel_id, message_id, options, closes_at, closed FROM polls WHERE id = ?1").bind(id).fetch_optional(pool).await?;
    Ok(
        row.map(|(guild, channel, message, o, closes_at, closed)| Poll {
            id,
            guild,
            channel,
            message,
            options: serde_json::from_str(&o).unwrap_or_default(),
            closes_at,
            closed: closed != 0,
        }),
    )
}

/// The outcome of a vote press.
#[derive(Debug, PartialEq, Eq)]
pub enum Voted {
    /// First vote, or the same choice again.
    Counted,
    /// The member moved their vote.
    Changed,
    /// The poll is closed or the option does not exist.
    Refused,
}

/// One vote per member: a second press moves it. Votes after the closing time are refused.
pub async fn vote(
    pool: &SqlitePool,
    poll: i64,
    user: i64,
    option: usize,
    now: i64,
) -> Result<Voted, sqlx::Error> {
    let Some(p) = get(pool, poll).await? else {
        return Ok(Voted::Refused);
    };
    if p.closed || now >= p.closes_at || option >= p.options.len() {
        return Ok(Voted::Refused);
    }
    let before: Option<i64> =
        sqlx::query_scalar("SELECT option FROM poll_votes WHERE poll_id = ?1 AND user_id = ?2")
            .bind(poll)
            .bind(user)
            .fetch_optional(pool)
            .await?;
    sqlx::query("INSERT INTO poll_votes (poll_id, user_id, option) VALUES (?1, ?2, ?3) ON CONFLICT(poll_id, user_id) DO UPDATE SET option = excluded.option").bind(poll).bind(user).bind(option as i64).execute(pool).await?;
    Ok(match before {
        Some(b) if b != option as i64 => Voted::Changed,
        _ => Voted::Counted,
    })
}

/// Votes per option (same order as the options).
pub async fn counts(pool: &SqlitePool, poll: &Poll) -> Result<Vec<i64>, sqlx::Error> {
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT option, count(*) FROM poll_votes WHERE poll_id = ?1 GROUP BY option",
    )
    .bind(poll.id)
    .fetch_all(pool)
    .await?;
    let mut out = vec![0; poll.options.len()];
    for (o, n) in rows {
        if let Some(slot) = out.get_mut(o as usize) {
            *slot = n;
        }
    }
    Ok(out)
}

/// The winner: most votes; a tie goes to the earliest option. `None` when nobody voted.
pub fn winner(counts: &[i64]) -> Option<usize> {
    let max = *counts.iter().max()?;
    (max > 0)
        .then(|| counts.iter().position(|c| *c == max))
        .flatten()
}

/// Polls that are open and past their closing time.
pub async fn due_to_close(pool: &SqlitePool, now: i64) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM polls WHERE closed = 0 AND closes_at <= ?1")
        .bind(now)
        .fetch_all(pool)
        .await
}

/// Close a poll once (`true` the first time).
pub async fn close(pool: &SqlitePool, id: i64) -> Result<bool, sqlx::Error> {
    Ok(
        sqlx::query("UPDATE polls SET closed = 1 WHERE id = ?1 AND closed = 0")
            .bind(id)
            .execute(pool)
            .await?
            .rows_affected()
            > 0,
    )
}

/// The poll message: the options with their live counts.
pub fn embed(options: &[Opt], counts: &[i64], closes_at: i64, closed: bool) -> Emb {
    let total: i64 = counts.iter().sum();
    let lines: Vec<String> = options
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let n = counts.get(i).copied().unwrap_or(0);
            let pct = if total > 0 { n * 100 / total } else { 0 };
            format!("**{}.** {} — {n} صوت ({pct}%)", i + 1, o.label)
        })
        .collect();
    let head = if closed {
        "انتهى التصويت.".to_string()
    } else {
        format!("صوّت بزر الخيار (ويمكنك تغيير صوتك). ينتهي التصويت <t:{closes_at}:R>.")
    };
    let mut e = Emb::new()
        .title("🍿 ماذا نشاهد هذا الأسبوع؟")
        .description(format!("{head}\n\n{}", lines.join("\n")));
    e.footer = Some(format!("{total} صوت"));
    e
}

/// One button per option; disabled once closed.
pub fn buttons(poll: i64, options: &[Opt], closed: bool) -> serenity::CreateActionRow {
    serenity::CreateActionRow::Buttons(
        options
            .iter()
            .enumerate()
            .map(|(i, _)| {
                serenity::CreateButton::new(format!("{VOTE_PREFIX}{poll}:{i}"))
                    .label(format!("{}", i + 1))
                    .style(serenity::ButtonStyle::Primary)
                    .disabled(closed)
            })
            .collect(),
    )
}

/// `poll:7:2` → (7, 2).
pub fn parse_vote(id: &str) -> Option<(i64, usize)> {
    let rest = id.strip_prefix(VOTE_PREFIX)?;
    let (p, o) = rest.split_once(':')?;
    Some((p.parse().ok()?, o.parse().ok()?))
}

/// The result message: the winning pair, with a menu that opens each of its works as a new message (universal rule).
pub fn result_message(options: &[Opt], counts: &[i64]) -> (Emb, Vec<serenity::CreateActionRow>) {
    match winner(counts).and_then(|i| options.get(i).map(|o| (i, o))) {
        Some((i, o)) => {
            let e = Emb::new().title("🏆 نتيجة التصويت").description(format!(
                "فاز الاقتراح: **{}** بـ {} صوت.\nافتح العملين من القائمة.",
                o.label, counts[i]
            ));
            let items = vec![
                (fit(&o.first_key, 100), o.first_key.clone()),
                (fit(&o.second_key, 100), o.second_key.clone()),
            ];
            let items = labelled(&o.label, items);
            (
                e,
                components::work_menu("افتح عملًا من الزوج الفائز", &items)
                    .into_iter()
                    .collect(),
            )
        }
        None => (
            Emb::new()
                .title("🏆 نتيجة التصويت")
                .description("لم يصوّت أحد هذا الأسبوع، سنحاول في الأسبوع القادم."),
            Vec::new(),
        ),
    }
}

/// Menu labels from the pair label "A + B" (falls back to the keys).
fn labelled(label: &str, items: Vec<(String, String)>) -> Vec<(String, String)> {
    let parts: Vec<&str> = label.splitn(2, " + ").collect();
    items
        .into_iter()
        .enumerate()
        .map(|(i, (fallback, key))| (parts.get(i).map_or(fallback, |p| fit(p, 100)), key))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool_with_suggestions(guild: i64, n: usize) -> SqlitePool {
        let pool = crate::db::connect_memory().await;
        for i in 0..n {
            sqlx::query("INSERT INTO suggestions (guild_id, author_id, first_id, second_id, first_title, second_title, pair_key, reason, created_at) VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, 'r', ?7)")
                .bind(guild)
                .bind(format!("anilist:{}", i * 2))
                .bind(format!("anilist:{}", i * 2 + 1))
                .bind(format!("A{i}"))
                .bind(format!("B{i}"))
                .bind(format!("p{i}"))
                .bind(1_000 + i as i64)
                .execute(&pool)
                .await
                .expect("suggestion");
        }
        pool
    }

    #[tokio::test]
    async fn options_are_the_newest_unpolled_suggestions_three_to_five() {
        let pool = pool_with_suggestions(1, 7).await;
        let first = pick_options(&pool, 1).await.expect("opts");
        assert_eq!(first.len(), MAX_OPTIONS);
        assert_eq!(first[0].label, "A6 + B6", "newest first");
        create(&pool, 1, 10, &first, 9_999).await.expect("poll");
        let second = pick_options(&pool, 1).await.expect("opts");
        assert_eq!(second.len(), 2, "the five already polled are skipped");
        assert!(
            second.len() < MIN_OPTIONS,
            "too few left: no poll this week"
        );
        assert!(
            pick_options(&pool, 2).await.expect("opts").is_empty(),
            "another server has none"
        );
    }

    #[tokio::test]
    async fn one_vote_per_member_can_be_changed_and_counts_are_live() {
        let pool = pool_with_suggestions(1, 3).await;
        let opts = pick_options(&pool, 1).await.expect("opts");
        let id = create(&pool, 1, 10, &opts, 5_000).await.expect("poll");
        assert_eq!(vote(&pool, id, 7, 0, 100).await.expect("v"), Voted::Counted);
        assert_eq!(
            vote(&pool, id, 7, 0, 101).await.expect("v"),
            Voted::Counted,
            "the same choice again changes nothing"
        );
        assert_eq!(vote(&pool, id, 8, 1, 102).await.expect("v"), Voted::Counted);
        assert_eq!(
            vote(&pool, id, 7, 2, 103).await.expect("v"),
            Voted::Changed,
            "a member may change their vote"
        );
        let p = get(&pool, id).await.expect("get").expect("poll");
        assert_eq!(
            counts(&pool, &p).await.expect("c"),
            vec![0, 1, 1],
            "member 7 counts once, for the last choice"
        );
        assert_eq!(
            vote(&pool, id, 9, 9, 104).await.expect("v"),
            Voted::Refused,
            "no such option"
        );
        assert_eq!(
            vote(&pool, id, 9, 0, 5_000).await.expect("v"),
            Voted::Refused,
            "after the closing time"
        );
    }

    #[tokio::test]
    async fn a_poll_closes_once_after_its_time_and_the_earliest_option_wins_a_tie() {
        let pool = pool_with_suggestions(1, 3).await;
        let opts = pick_options(&pool, 1).await.expect("opts");
        let id = create(&pool, 1, 10, &opts, 1_000).await.expect("poll");
        assert!(due_to_close(&pool, 999).await.expect("d").is_empty());
        assert_eq!(due_to_close(&pool, 1_000).await.expect("d"), vec![id]);
        assert!(close(&pool, id).await.expect("c"), "first close");
        assert!(!close(&pool, id).await.expect("c"), "never twice");
        assert!(due_to_close(&pool, 2_000).await.expect("d").is_empty());
        assert_eq!(
            vote(&pool, id, 1, 0, 10).await.expect("v"),
            Voted::Refused,
            "closed polls take no votes"
        );
        assert_eq!(winner(&[1, 3, 3]), Some(1));
        assert_eq!(winner(&[0, 0]), None);
        assert_eq!(winner(&[]), None);
        assert_eq!(winner(&[2, 1]), Some(0));
    }

    #[test]
    fn the_schedule_starts_a_poll_on_the_chosen_weekday_from_the_chosen_hour() {
        use chrono::TimeZone;
        let c = Config {
            day: 4,
            hour: 18,
            duration_hours: 24,
        };
        let tz = chrono_tz::Asia::Muscat;
        let friday_19 = tz
            .with_ymd_and_hms(2026, 11, 20, 19, 0, 0)
            .single()
            .expect("t")
            .timestamp();
        assert!(should_start(friday_19, tz, &c));
        assert!(
            !should_start(friday_19 - 2 * 3600, tz, &c),
            "too early on Friday"
        );
        assert!(!should_start(friday_19 + 86_400, tz, &c), "Saturday");
        let cfg = Config::from(&settings::Feature {
            enabled: true,
            hour: 18,
            extra: "day=2;hours=48".into(),
        });
        assert_eq!(
            cfg,
            Config {
                day: 2,
                hour: 18,
                duration_hours: 48
            }
        );
        assert_eq!(
            Config::from(&settings::Feature::default()),
            Config {
                day: 4,
                hour: 9,
                duration_hours: 24
            },
            "Friday morning for a day by default"
        );
        assert_eq!(
            Config::from(&settings::Feature {
                enabled: true,
                hour: 9,
                extra: "hours=7;day=9".into()
            })
            .duration_hours,
            24,
            "unknown values fall back"
        );
    }

    #[test]
    fn vote_ids_round_trip_and_the_message_shows_counts_and_closes() {
        assert_eq!(parse_vote("poll:12:3"), Some((12, 3)));
        assert_eq!(parse_vote("poll:x:3"), None);
        assert_eq!(parse_vote("showwork:anilist:1"), None);
        let opts = vec![
            Opt {
                suggestion: 1,
                first_key: "anilist:1".into(),
                second_key: "anilist:2".into(),
                label: "A + B".into(),
            },
            Opt {
                suggestion: 2,
                first_key: "anilist:3".into(),
                second_key: "anilist:4".into(),
                label: "C + D".into(),
            },
        ];
        let e = embed(&opts, &[3, 1], 5_000, false);
        let d = e.description.as_deref().unwrap_or("");
        assert!(
            d.contains("A + B — 3 صوت (75%)")
                && d.contains("C + D — 1 صوت (25%)")
                && d.contains("<t:5000:R>"),
            "{d}"
        );
        assert!(embed(&opts, &[3, 1], 5_000, true)
            .description
            .as_deref()
            .unwrap_or("")
            .contains("انتهى التصويت"));
        let json = serde_json::to_string(&buttons(7, &opts, true)).expect("json");
        assert!(
            json.contains("poll:7:0")
                && json.contains("poll:7:1")
                && json.contains("\"disabled\":true")
        );
    }

    #[test]
    fn the_result_opens_the_winning_pair_and_says_when_nobody_voted() {
        let opts = vec![Opt {
            suggestion: 1,
            first_key: "anilist:1".into(),
            second_key: "tmdb-tv:2".into(),
            label: "Alpha + Beta".into(),
        }];
        let (e, rows) = result_message(&opts, &[4]);
        assert!(e
            .description
            .as_deref()
            .unwrap_or("")
            .contains("Alpha + Beta"));
        let json = serde_json::to_string(&rows).expect("json");
        assert!(
            json.contains("pickwork")
                && json.contains("anilist:1")
                && json.contains("tmdb-tv:2")
                && json.contains("Alpha")
                && json.contains("Beta"),
            "{json}"
        );
        let (none, rows) = result_message(&opts, &[0]);
        assert!(
            none.description
                .as_deref()
                .unwrap_or("")
                .contains("لم يصوّت أحد")
                && rows.is_empty()
        );
    }
}
