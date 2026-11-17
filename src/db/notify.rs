//! Notification preferences per category, exclusions, dedupe of announced events and the server's
//! drama / series overrides.

use sqlx::SqlitePool;
use std::collections::HashMap;

pub type Result<T> = std::result::Result<T, sqlx::Error>;

pub const CATEGORIES: &[&str] = &["anime", "drama", "series", "movie", "manga", "novel"];
pub const DIGEST_HOURS: &[i64] = &[1, 2, 4, 8, 24];
pub const MODES: &[&str] = &["all", "mine", "popular"];
pub const EXCLUSION_KINDS: &[&str] = &["item", "category", "format", "genre"];

pub fn category_label(c: &str) -> &'static str {
    match c {
        "anime" => "الأنمي",
        "drama" => "الدراما",
        "series" => "المسلسلات",
        "movie" => "الأفلام",
        "manga" => "المانجا",
        "novel" => "الروايات",
        _ => "—",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub enabled: bool,
    pub mode: String,
    pub digest_hours: i64,
}

impl Default for Setting {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: "mine".into(),
            digest_hours: 2,
        }
    }
}

pub async fn setting(pool: &SqlitePool, user: i64, category: &str) -> Result<Setting> {
    let row: Option<(i64, String, i64)> =
        sqlx::query_as("SELECT enabled, mode, digest_hours FROM notification_settings WHERE user_id = ?1 AND category = ?2")
            .bind(user)
            .bind(category)
            .fetch_optional(pool)
            .await?;
    Ok(row
        .map(|(e, m, d)| Setting {
            enabled: e != 0,
            mode: m,
            digest_hours: d,
        })
        .unwrap_or_default())
}

pub async fn set_setting(pool: &SqlitePool, user: i64, category: &str, s: &Setting) -> Result<()> {
    if !CATEGORIES.contains(&category)
        || !MODES.contains(&s.mode.as_str())
        || !DIGEST_HOURS.contains(&s.digest_hours)
    {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO notification_settings (user_id, category, enabled, mode, digest_hours) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(user_id, category) DO UPDATE SET enabled = excluded.enabled, mode = excluded.mode, digest_hours = excluded.digest_hours",
    )
    .bind(user)
    .bind(category)
    .bind(i64::from(s.enabled))
    .bind(&s.mode)
    .bind(s.digest_hours)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefs {
    pub timezone: String,
    pub summary_hour: i64,
    pub spoiler_guard: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            timezone: "Asia/Muscat".into(),
            summary_hour: 9,
            spoiler_guard: false,
        }
    }
}

pub async fn prefs(pool: &SqlitePool, user: i64) -> Result<Prefs> {
    let row: Option<(String, i64, i64)> = sqlx::query_as(
        "SELECT timezone, summary_hour, spoiler_guard FROM notification_prefs WHERE user_id = ?1",
    )
    .bind(user)
    .fetch_optional(pool)
    .await?;
    Ok(row
        .map(|(t, h, s)| Prefs {
            timezone: t,
            summary_hour: h,
            spoiler_guard: s != 0,
        })
        .unwrap_or_default())
}

pub async fn set_prefs(pool: &SqlitePool, user: i64, p: &Prefs) -> Result<()> {
    if p.timezone.parse::<chrono_tz::Tz>().is_err() || !(0..24).contains(&p.summary_hour) {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO notification_prefs (user_id, timezone, summary_hour, spoiler_guard) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(user_id) DO UPDATE SET timezone = excluded.timezone, summary_hour = excluded.summary_hour, spoiler_guard = excluded.spoiler_guard",
    )
    .bind(user)
    .bind(&p.timezone)
    .bind(p.summary_hour)
    .bind(i64::from(p.spoiler_guard))
    .execute(pool)
    .await?;
    Ok(())
}

/// Is the spoiler guard on for this member here (the member's own setting or the server's)?
pub async fn spoiler_guard(pool: &SqlitePool, guild: Option<i64>, user: i64) -> bool {
    if prefs(pool, user)
        .await
        .map(|p| p.spoiler_guard)
        .unwrap_or(false)
    {
        return true;
    }
    match guild {
        Some(g) => sqlx::query_scalar::<_, i64>(
            "SELECT spoiler_guard FROM guild_settings WHERE guild_id = ?1",
        )
        .bind(g)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .is_some_and(|v| v != 0),
        None => false,
    }
}

pub async fn exclude(pool: &SqlitePool, user: i64, kind: &str, value: &str) -> Result<()> {
    if !EXCLUSION_KINDS.contains(&kind) || value.trim().is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT OR IGNORE INTO notification_exclusions (user_id, kind, value) VALUES (?1, ?2, ?3)",
    )
    .bind(user)
    .bind(kind)
    .bind(value.trim().to_lowercase())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn unexclude(pool: &SqlitePool, user: i64, kind: &str, value: &str) -> Result<()> {
    sqlx::query(
        "DELETE FROM notification_exclusions WHERE user_id = ?1 AND kind = ?2 AND value = ?3",
    )
    .bind(user)
    .bind(kind)
    .bind(value.trim().to_lowercase())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn exclusions(pool: &SqlitePool, user: i64) -> Result<Vec<(String, String)>> {
    sqlx::query_as(
        "SELECT kind, value FROM notification_exclusions WHERE user_id = ?1 ORDER BY kind, value",
    )
    .bind(user)
    .fetch_all(pool)
    .await
}

/// Would a notice about this item be suppressed by the member's exclusions?
pub fn is_excluded(
    excl: &[(String, String)],
    key: &str,
    category: &str,
    format: Option<&str>,
    genres: &[String],
) -> bool {
    excl.iter().any(|(k, v)| match k.as_str() {
        "item" => v.eq_ignore_ascii_case(key),
        "category" => v == category,
        "format" => format.is_some_and(|f| f.eq_ignore_ascii_case(v)),
        "genre" => genres.iter().any(|g| g.eq_ignore_ascii_case(v)),
        _ => false,
    })
}

/// May a notice about this item reach the member? Their switch for the category and their exclusions decide.
pub async fn allows(
    pool: &SqlitePool,
    user: i64,
    key: &str,
    category: &str,
    format: Option<&str>,
    genres: &[String],
) -> Result<bool> {
    if !setting(pool, user, category).await?.enabled {
        return Ok(false);
    }
    let excl = exclusions(pool, user).await?;
    Ok(!is_excluded(&excl, key, category, format, genres))
}

/// Records the event; `true` when it was new (so it should be sent).
pub async fn claim_event(pool: &SqlitePool, user: i64, key: &str, event: &str) -> Result<bool> {
    Ok(sqlx::query(
        "INSERT OR IGNORE INTO notified_events (user_id, work_key, event) VALUES (?1, ?2, ?3)",
    )
    .bind(user)
    .bind(key)
    .bind(event)
    .execute(pool)
    .await?
    .rows_affected()
        > 0)
}

/// Forget announcements older than `days` (the table never grows without bound).
pub async fn prune_events(pool: &SqlitePool, days: i64) -> Result<u64> {
    Ok(
        sqlx::query("DELETE FROM notified_events WHERE sent_at < unixepoch() - ?1")
            .bind(days * 86_400)
            .execute(pool)
            .await?
            .rows_affected(),
    )
}

pub async fn drama_overrides(
    pool: &SqlitePool,
    guild: i64,
) -> Result<HashMap<String, crate::tmdb::model::Target>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT country, target FROM drama_country_overrides WHERE guild_id = ?1")
            .bind(guild)
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(c, t)| match t.as_str() {
            "drama" => Some((c, crate::tmdb::model::Target::Drama)),
            "series" => Some((c, crate::tmdb::model::Target::Series)),
            _ => None,
        })
        .collect())
}

/// `target`: `drama`, `series`, or `default` (removes the override).
pub async fn set_drama_override(
    pool: &SqlitePool,
    guild: i64,
    country: &str,
    target: &str,
) -> Result<()> {
    let country = country.trim().to_uppercase();
    if country.len() != 2 || !country.chars().all(|c| c.is_ascii_uppercase()) {
        return Ok(());
    }
    match target {
        "drama" | "series" => {
            sqlx::query(
                "INSERT INTO drama_country_overrides (guild_id, country, target) VALUES (?1, ?2, ?3)
                 ON CONFLICT(guild_id, country) DO UPDATE SET target = excluded.target",
            )
            .bind(guild)
            .bind(&country)
            .bind(target)
            .execute(pool)
            .await?;
        }
        _ => {
            sqlx::query("DELETE FROM drama_country_overrides WHERE guild_id = ?1 AND country = ?2")
                .bind(guild)
                .bind(&country)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> SqlitePool {
        let p = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("db");
        sqlx::migrate!("./migrations")
            .run(&p)
            .await
            .expect("migrate");
        p
    }

    #[tokio::test]
    async fn settings_default_on_and_round_trip_with_validation() {
        let p = pool().await;
        assert_eq!(
            setting(&p, 1, "drama").await.expect("get"),
            Setting::default()
        );
        let s = Setting {
            enabled: false,
            mode: "all".into(),
            digest_hours: 8,
        };
        set_setting(&p, 1, "drama", &s).await.expect("set");
        assert_eq!(setting(&p, 1, "drama").await.expect("get"), s);
        assert_eq!(
            setting(&p, 1, "anime").await.expect("get"),
            Setting::default(),
            "per category"
        );
        set_setting(
            &p,
            1,
            "anime",
            &Setting {
                enabled: true,
                mode: "weird".into(),
                digest_hours: 2,
            },
        )
        .await
        .expect("ignored");
        assert_eq!(
            setting(&p, 1, "anime").await.expect("get"),
            Setting::default(),
            "invalid values are refused"
        );
    }

    #[tokio::test]
    async fn prefs_validate_the_timezone_and_hour() {
        let p = pool().await;
        assert_eq!(prefs(&p, 1).await.expect("get").timezone, "Asia/Muscat");
        set_prefs(
            &p,
            1,
            &Prefs {
                timezone: "Asia/Riyadh".into(),
                summary_hour: 20,
                spoiler_guard: true,
            },
        )
        .await
        .expect("set");
        let got = prefs(&p, 1).await.expect("get");
        assert_eq!(
            (got.timezone.as_str(), got.summary_hour, got.spoiler_guard),
            ("Asia/Riyadh", 20, true)
        );
        set_prefs(
            &p,
            1,
            &Prefs {
                timezone: "Mars/Base".into(),
                summary_hour: 3,
                spoiler_guard: false,
            },
        )
        .await
        .expect("ignored");
        assert_eq!(prefs(&p, 1).await.expect("get").timezone, "Asia/Riyadh");
    }

    #[tokio::test]
    async fn exclusions_match_items_categories_formats_and_genres() {
        let p = pool().await;
        exclude(&p, 1, "item", "TMDB-TV:5").await.expect("x");
        exclude(&p, 1, "genre", "Ecchi").await.expect("x");
        exclude(&p, 1, "category", "movie").await.expect("x");
        exclude(&p, 1, "bogus", "x").await.expect("ignored");
        let e = exclusions(&p, 1).await.expect("list");
        assert_eq!(e.len(), 3);
        assert!(is_excluded(&e, "tmdb-tv:5", "series", None, &[]));
        assert!(is_excluded(
            &e,
            "anilist:1",
            "anime",
            None,
            &["Ecchi".into()]
        ));
        assert!(is_excluded(&e, "tmdb-movie:9", "movie", None, &[]));
        assert!(!is_excluded(
            &e,
            "anilist:2",
            "anime",
            Some("TV"),
            &["Drama".into()]
        ));
        unexclude(&p, 1, "genre", "ecchi").await.expect("remove");
        assert_eq!(exclusions(&p, 1).await.expect("list").len(), 2);
    }

    #[tokio::test]
    async fn a_notice_is_allowed_unless_the_category_is_off_or_the_item_is_excluded() {
        let p = pool().await;
        assert!(allows(&p, 1, "tmdb-tv:5", "series", None, &[])
            .await
            .expect("a"));
        set_setting(
            &p,
            1,
            "series",
            &Setting {
                enabled: false,
                ..Setting::default()
            },
        )
        .await
        .expect("set");
        assert!(
            !allows(&p, 1, "tmdb-tv:5", "series", None, &[])
                .await
                .expect("a"),
            "category switched off"
        );
        assert!(
            allows(&p, 1, "tmdb-tv:5", "drama", None, &[])
                .await
                .expect("a"),
            "other categories are unaffected"
        );
        exclude(&p, 1, "genre", "Reality").await.expect("x");
        assert!(
            !allows(&p, 1, "tmdb-tv:6", "drama", None, &["reality".into()])
                .await
                .expect("a")
        );
        assert!(
            allows(&p, 2, "tmdb-tv:5", "series", None, &[])
                .await
                .expect("a"),
            "another member"
        );
    }

    #[tokio::test]
    async fn an_event_is_claimed_once_and_old_ones_are_pruned() {
        let p = pool().await;
        assert!(claim_event(&p, 1, "tmdb-tv:5", "s1e1")
            .await
            .expect("claim"));
        assert!(!claim_event(&p, 1, "tmdb-tv:5", "s1e1")
            .await
            .expect("again"));
        assert!(claim_event(&p, 2, "tmdb-tv:5", "s1e1")
            .await
            .expect("other member"));
        sqlx::query("UPDATE notified_events SET sent_at = 1 WHERE user_id = 2")
            .execute(&p)
            .await
            .expect("age");
        assert_eq!(prune_events(&p, 30).await.expect("prune"), 1);
    }

    #[tokio::test]
    async fn overrides_move_a_country_and_default_removes_it() {
        use crate::tmdb::model::{classify, Target};
        let p = pool().await;
        assert!(drama_overrides(&p, 1).await.expect("get").is_empty());
        set_drama_override(&p, 1, "kr", "series")
            .await
            .expect("set");
        set_drama_override(&p, 1, "TR", "drama").await.expect("set");
        set_drama_override(&p, 1, "zzz", "drama")
            .await
            .expect("ignored");
        let o = drama_overrides(&p, 1).await.expect("get");
        assert_eq!(o.len(), 2);
        assert_eq!(classify(&["KR".to_string()], &o), Target::Series);
        assert_eq!(classify(&["TR".to_string()], &o), Target::Drama);
        assert!(drama_overrides(&p, 2)
            .await
            .expect("other guild")
            .is_empty());
        set_drama_override(&p, 1, "KR", "default")
            .await
            .expect("clear");
        assert_eq!(drama_overrides(&p, 1).await.expect("get").len(), 1);
    }

    #[tokio::test]
    async fn the_migration_keeps_old_reminders_and_fills_their_key() {
        let p = pool().await;
        sqlx::query("INSERT INTO reminders (user_id, work_id, title) VALUES (1, 77, 'x')")
            .execute(&p)
            .await
            .expect("insert");
        let key: String = sqlx::query_scalar("SELECT work_key FROM reminders WHERE work_id = 77")
            .fetch_one(&p)
            .await
            .expect("row");
        assert_eq!(
            key, "",
            "rows inserted after the migration get their key from code"
        );
        let cat: String = sqlx::query_scalar("SELECT category FROM reminders WHERE work_id = 77")
            .fetch_one(&p)
            .await
            .expect("row");
        assert_eq!(cat, "anime");
    }
}
