//! Per server switches of the optional features (`guild_features`). Everything that posts messages is off until an admin
//! turns it on; the hour is in the server's time zone.

use sqlx::SqlitePool;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feature {
    pub enabled: bool,
    pub hour: i64,
    /// A few feature specific values, e.g. `day=5;hours=24`.
    pub extra: String,
}

impl Default for Feature {
    fn default() -> Self {
        Self {
            enabled: false,
            hour: 9,
            extra: String::new(),
        }
    }
}

impl Feature {
    /// A value out of `extra` (`key=value;key=value`).
    pub fn value(&self, key: &str) -> Option<i64> {
        self.extra
            .split(';')
            .filter_map(|p| p.split_once('='))
            .find(|(k, _)| *k == key)
            .and_then(|(_, v)| v.trim().parse().ok())
    }
}

pub async fn get(pool: &SqlitePool, guild: i64, feature: &str) -> Result<Feature, sqlx::Error> {
    let row: Option<(i64, i64, String)> = sqlx::query_as(
        "SELECT enabled, hour, extra FROM guild_features WHERE guild_id = ?1 AND feature = ?2",
    )
    .bind(guild)
    .bind(feature)
    .fetch_optional(pool)
    .await?;
    Ok(row
        .map(|(e, h, x)| Feature {
            enabled: e != 0,
            hour: h,
            extra: x,
        })
        .unwrap_or_default())
}

pub async fn set(
    pool: &SqlitePool,
    guild: i64,
    feature: &str,
    f: &Feature,
) -> Result<(), sqlx::Error> {
    if !(0..24).contains(&f.hour) || f.extra.len() > 100 {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO guild_features (guild_id, feature, enabled, hour, extra) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(guild_id, feature) DO UPDATE SET enabled = excluded.enabled, hour = excluded.hour, extra = excluded.extra",
    )
    .bind(guild)
    .bind(feature)
    .bind(i64::from(f.enabled))
    .bind(f.hour)
    .bind(&f.extra)
    .execute(pool)
    .await?;
    Ok(())
}

/// Servers where a feature is on, with its settings.
pub async fn enabled_guilds(
    pool: &SqlitePool,
    feature: &str,
) -> Result<Vec<(i64, Feature)>, sqlx::Error> {
    let rows: Vec<(i64, i64, String)> = sqlx::query_as(
        "SELECT guild_id, hour, extra FROM guild_features WHERE feature = ?1 AND enabled = 1",
    )
    .bind(feature)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(g, h, x)| {
            (
                g,
                Feature {
                    enabled: true,
                    hour: h,
                    extra: x,
                },
            )
        })
        .collect())
}

/// Claim something once per server and day-key (birthday of a character, a weekly poll...). `true` when it was new.
pub async fn claim(
    pool: &SqlitePool,
    guild: i64,
    kind: &str,
    item: &str,
    day: &str,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query(
        "INSERT OR IGNORE INTO daily_posts (guild_id, kind, item_key, day) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(guild)
    .bind(kind)
    .bind(item)
    .bind(day)
    .execute(pool)
    .await?
    .rows_affected()
        > 0)
}

/// Items of a kind posted for the server on or after `since_day` (`YYYY-MM-DD`).
pub async fn posted_since(
    pool: &SqlitePool,
    guild: i64,
    kind: &str,
    since_day: &str,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT item_key FROM daily_posts WHERE guild_id = ?1 AND kind = ?2 AND day >= ?3",
    )
    .bind(guild)
    .bind(kind)
    .bind(since_day)
    .fetch_all(pool)
    .await
}

/// Forget old claims.
pub async fn prune_posts(pool: &SqlitePool, before_day: &str) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query("DELETE FROM daily_posts WHERE day < ?1")
        .bind(before_day)
        .execute(pool)
        .await?
        .rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn features_are_off_by_default_and_round_trip_with_validation() {
        let pool = crate::db::connect_memory().await;
        let d = get(&pool, 1, "poll").await.expect("get");
        assert_eq!(
            (d.enabled, d.hour, d.extra.as_str()),
            (false, 9, ""),
            "posting features start off"
        );
        set(
            &pool,
            1,
            "poll",
            &Feature {
                enabled: true,
                hour: 20,
                extra: "day=5;hours=48".into(),
            },
        )
        .await
        .expect("set");
        let f = get(&pool, 1, "poll").await.expect("get");
        assert!(f.enabled && f.hour == 20);
        assert_eq!(
            (f.value("day"), f.value("hours"), f.value("nope")),
            (Some(5), Some(48), None)
        );
        set(
            &pool,
            1,
            "poll",
            &Feature {
                enabled: true,
                hour: 30,
                extra: String::new(),
            },
        )
        .await
        .expect("ignored");
        assert_eq!(
            get(&pool, 1, "poll").await.expect("get").hour,
            20,
            "an invalid hour is refused"
        );
        assert!(
            !get(&pool, 2, "poll").await.expect("get").enabled,
            "another server"
        );
        assert_eq!(enabled_guilds(&pool, "poll").await.expect("g").len(), 1);
        assert!(enabled_guilds(&pool, "birthdays")
            .await
            .expect("g")
            .is_empty());
    }

    #[tokio::test]
    async fn a_claim_is_new_once_per_server_kind_item_and_day() {
        let pool = crate::db::connect_memory().await;
        assert!(claim(&pool, 1, "birthday", "anilist-char:5", "2026-11-17")
            .await
            .expect("c"));
        assert!(!claim(&pool, 1, "birthday", "anilist-char:5", "2026-11-17")
            .await
            .expect("c"));
        assert!(
            claim(&pool, 2, "birthday", "anilist-char:5", "2026-11-17")
                .await
                .expect("c"),
            "another server"
        );
        assert!(
            claim(&pool, 1, "birthday", "anilist-char:5", "2027-11-17")
                .await
                .expect("c"),
            "next year"
        );
        assert_eq!(
            posted_since(&pool, 1, "birthday", "2026-11-17")
                .await
                .expect("p")
                .len(),
            2
        );
        assert_eq!(prune_posts(&pool, "2027-01-01").await.expect("prune"), 2);
    }
}
