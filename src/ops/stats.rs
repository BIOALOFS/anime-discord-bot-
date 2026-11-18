//! Usage statistics: counters kept in memory and written to `usage_stats` once in a while, aggregated per day. Only
//! names (a command, a provider) and counts are stored: no message content, no user ids. Retention is 30 days.

use sqlx::SqlitePool;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Mutex, OnceLock},
};

pub const RETENTION_DAYS: i64 = 30;

#[derive(Default)]
struct Pending {
    counts: HashMap<(String, String), i64>,
    /// Distinct members seen today (only their number is stored; the ids stay in memory).
    users_day: String,
    users: HashSet<u64>,
}

fn pending() -> &'static Mutex<Pending> {
    static P: OnceLock<Mutex<Pending>> = OnceLock::new();
    P.get_or_init(Default::default)
}

pub fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

/// Count one event: `kind` is `command`, `error`, `cache` (`hit` / `miss`) or `api`.
pub fn bump(kind: &str, name: &str) {
    if let Ok(mut p) = pending().lock() {
        *p.counts
            .entry((kind.to_string(), name.to_string()))
            .or_default() += 1;
    }
}

/// A member used the bot today (counted once per day; only the number is kept).
pub fn note_user(user: u64) {
    if let Ok(mut p) = pending().lock() {
        let day = today();
        if p.users_day != day {
            p.users_day = day;
            p.users.clear();
        }
        p.users.insert(user);
    }
}

/// Write the pending counters into today's rows (adding) and today's distinct-user number (replacing).
pub async fn flush(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let (counts, users, day) = {
        let Ok(mut p) = pending().lock() else {
            return Ok(());
        };
        (
            std::mem::take(&mut p.counts),
            p.users.len() as i64,
            p.users_day.clone(),
        )
    };
    let day_now = today();
    for ((kind, name), n) in counts {
        sqlx::query("INSERT INTO usage_stats (day, kind, name, count) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(day, kind, name) DO UPDATE SET count = count + excluded.count")
            .bind(&day_now)
            .bind(kind)
            .bind(name)
            .bind(n)
            .execute(pool)
            .await?;
    }
    if users > 0 && day == day_now {
        sqlx::query("INSERT INTO usage_stats (day, kind, name, count) VALUES (?1, 'users', 'active', ?2) ON CONFLICT(day, kind, name) DO UPDATE SET count = excluded.count")
            .bind(&day_now)
            .bind(users)
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Delete rows older than the retention.
pub async fn prune(pool: &SqlitePool, today: &str) -> Result<u64, sqlx::Error> {
    let Ok(d) = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d") else {
        return Ok(0);
    };
    let cutoff = (d - chrono::Duration::days(RETENTION_DAYS))
        .format("%Y-%m-%d")
        .to_string();
    Ok(sqlx::query("DELETE FROM usage_stats WHERE day < ?1")
        .bind(cutoff)
        .execute(pool)
        .await?
        .rows_affected())
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    /// `(day, commands)` for the last seven days, oldest first.
    pub per_day: Vec<(String, i64)>,
    /// `(command, uses)` over the last seven days, most used first.
    pub per_command: Vec<(String, i64)>,
    /// `(provider, errors)` over the last seven days, most first.
    pub errors: Vec<(String, i64)>,
    /// Cache hits and misses over the last seven days.
    pub cache: (i64, i64),
    /// API calls made over the last seven days.
    pub api_calls: i64,
    /// Distinct active members per day (last seven days), oldest first.
    pub active: Vec<(String, i64)>,
}

impl Report {
    /// Cache hit rate in percent, if there was any lookup.
    pub fn hit_rate(&self) -> Option<i64> {
        let total = self.cache.0 + self.cache.1;
        (total > 0).then(|| self.cache.0 * 100 / total)
    }
}

/// The report of the seven days ending on `today`.
pub async fn report(pool: &SqlitePool, today: &str) -> Result<Report, sqlx::Error> {
    let Ok(d) = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d") else {
        return Ok(Report::default());
    };
    let days: Vec<String> = (0..7)
        .rev()
        .map(|i| {
            (d - chrono::Duration::days(i))
                .format("%Y-%m-%d")
                .to_string()
        })
        .collect();
    let first = days[0].clone();
    let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
        "SELECT day, kind, name, count FROM usage_stats WHERE day >= ?1 AND day <= ?2",
    )
    .bind(&first)
    .bind(today)
    .fetch_all(pool)
    .await?;
    let mut per_day: BTreeMap<String, i64> = days.iter().map(|d| (d.clone(), 0)).collect();
    let mut active: BTreeMap<String, i64> = days.iter().map(|d| (d.clone(), 0)).collect();
    let (mut cmd, mut err): (HashMap<String, i64>, HashMap<String, i64>) = Default::default();
    let mut r = Report::default();
    for (day, kind, name, n) in rows {
        match kind.as_str() {
            "command" => {
                *per_day.entry(day).or_default() += n;
                *cmd.entry(name).or_default() += n;
            }
            "error" => *err.entry(name).or_default() += n,
            "cache" => {
                if name == "hit" {
                    r.cache.0 += n
                } else {
                    r.cache.1 += n
                }
            }
            "api" => r.api_calls += n,
            "users" => {
                active.insert(day, n);
            }
            _ => {}
        }
    }
    let sorted = |m: HashMap<String, i64>| {
        let mut v: Vec<(String, i64)> = m.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    };
    r.per_day = per_day.into_iter().collect();
    r.active = active.into_iter().collect();
    r.per_command = sorted(cmd);
    r.errors = sorted(err);
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn put(pool: &SqlitePool, day: &str, kind: &str, name: &str, n: i64) {
        sqlx::query("INSERT INTO usage_stats (day, kind, name, count) VALUES (?1, ?2, ?3, ?4)")
            .bind(day)
            .bind(kind)
            .bind(name)
            .bind(n)
            .execute(pool)
            .await
            .expect("row");
    }

    #[tokio::test]
    async fn the_report_sums_the_last_seven_days_by_command_error_and_cache() {
        let pool = crate::db::connect_memory().await;
        put(&pool, "2026-11-17", "command", "anime", 5).await;
        put(&pool, "2026-11-17", "command", "top", 2).await;
        put(&pool, "2026-11-16", "command", "anime", 3).await;
        put(&pool, "2026-11-01", "command", "anime", 99).await; // too old
        put(&pool, "2026-11-17", "error", "vndb", 4).await;
        put(&pool, "2026-11-16", "error", "igdb", 1).await;
        put(&pool, "2026-11-17", "cache", "hit", 30).await;
        put(&pool, "2026-11-17", "cache", "miss", 10).await;
        put(&pool, "2026-11-17", "api", "tmdb", 12).await;
        put(&pool, "2026-11-17", "users", "active", 7).await;
        let r = report(&pool, "2026-11-17").await.expect("report");
        assert_eq!(r.per_day.len(), 7);
        assert_eq!(r.per_day.last(), Some(&("2026-11-17".to_string(), 7)));
        assert_eq!(r.per_day[5], ("2026-11-16".to_string(), 3));
        assert_eq!(
            r.per_command,
            vec![("anime".to_string(), 8), ("top".to_string(), 2)],
            "most used first, old days excluded"
        );
        assert_eq!(
            r.errors,
            vec![("vndb".to_string(), 4), ("igdb".to_string(), 1)]
        );
        assert_eq!(
            (r.cache, r.hit_rate(), r.api_calls),
            ((30, 10), Some(75), 12)
        );
        assert_eq!(r.active.last(), Some(&("2026-11-17".to_string(), 7)));
        assert_eq!(Report::default().hit_rate(), None);
    }

    #[tokio::test]
    async fn counters_flush_into_todays_row_adding_up_and_active_users_are_a_number() {
        let pool = crate::db::connect_memory().await;
        bump("command", "stats-test-cmd");
        bump("command", "stats-test-cmd");
        note_user(1);
        note_user(1);
        note_user(2);
        flush(&pool).await.expect("flush");
        bump("command", "stats-test-cmd");
        flush(&pool).await.expect("flush again");
        let n: i64 = sqlx::query_scalar(
            "SELECT count FROM usage_stats WHERE kind = 'command' AND name = 'stats-test-cmd'",
        )
        .fetch_one(&pool)
        .await
        .expect("row");
        assert_eq!(n, 3, "flushes add up");
        let users: i64 = sqlx::query_scalar("SELECT count FROM usage_stats WHERE kind = 'users'")
            .fetch_one(&pool)
            .await
            .expect("row");
        assert!(
            users >= 2,
            "two distinct members were seen (other tests may add more)"
        );
        let ids: i64 =
            sqlx::query_scalar("SELECT count(*) FROM usage_stats WHERE name IN ('1', '2')")
                .fetch_one(&pool)
                .await
                .expect("row");
        assert_eq!(ids, 0, "no member id is ever stored");
    }

    #[tokio::test]
    async fn rows_older_than_thirty_days_are_deleted() {
        let pool = crate::db::connect_memory().await;
        put(&pool, "2026-10-01", "command", "x", 1).await;
        put(&pool, "2026-10-17", "command", "x", 1).await;
        put(&pool, "2026-10-18", "command", "x", 1).await;
        put(&pool, "2026-11-17", "command", "x", 1).await;
        assert_eq!(prune(&pool, "2026-11-17").await.expect("prune"), 2);
        let left: i64 = sqlx::query_scalar("SELECT count(*) FROM usage_stats")
            .fetch_one(&pool)
            .await
            .expect("n");
        assert_eq!(left, 2);
        assert_eq!(prune(&pool, "not a date").await.expect("ok"), 0);
    }
}
