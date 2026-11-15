//! Airing feed configuration, notification list, member requests, dedupe and digest queue.

use sqlx::{Row, SqlitePool};

pub type Result<T> = std::result::Result<T, sqlx::Error>;

pub const ALL_FORMATS: [&str; 4] = ["TV", "TV_SHORT", "ONA", "MOVIE"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedConfig {
    pub guild_id: i64,
    pub channel_id: Option<i64>,
    pub enabled: bool,
    pub min_popularity: i64,
    pub formats: Vec<String>,
    pub excluded_genres: Vec<String>,
    pub digest_hours: i64,
    pub daily_cap: i64,
    pub role_id: Option<i64>,
    /// `true`: only the notification list; `false`: every anime plus pins.
    pub only_listed: bool,
    pub last_digest_at: i64,
}

fn split_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_string)
        .collect()
}

fn from_row(r: &sqlx::sqlite::SqliteRow) -> FeedConfig {
    FeedConfig {
        guild_id: r.get("guild_id"),
        channel_id: r.get("channel_id"),
        enabled: r.get("enabled"),
        min_popularity: r.get("min_popularity"),
        formats: split_csv(&r.get::<String, _>("formats")),
        excluded_genres: split_csv(&r.get::<String, _>("excluded_genres")),
        digest_hours: r.get("digest_hours"),
        daily_cap: r.get("daily_cap"),
        role_id: r.get("role_id"),
        only_listed: r.get::<String, _>("mode") == "listed",
        last_digest_at: r.get("last_digest_at"),
    }
}

pub async fn get_config(pool: &SqlitePool, guild: i64) -> Result<FeedConfig> {
    sqlx::query("INSERT OR IGNORE INTO guild_feed (guild_id) VALUES (?1)")
        .bind(guild)
        .execute(pool)
        .await?;
    let row = sqlx::query("SELECT * FROM guild_feed WHERE guild_id = ?1")
        .bind(guild)
        .fetch_one(pool)
        .await?;
    Ok(from_row(&row))
}

/// Every guild with the feed switched on and a channel chosen.
pub async fn active_guilds(pool: &SqlitePool) -> Result<Vec<FeedConfig>> {
    let rows = sqlx::query("SELECT * FROM guild_feed WHERE enabled = 1 AND channel_id IS NOT NULL")
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(from_row).collect())
}

async fn set(
    pool: &SqlitePool,
    guild: i64,
    column: &'static str,
    bind: impl for<'a> sqlx::Encode<'a, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send + 'static,
) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO guild_feed (guild_id) VALUES (?1)")
        .bind(guild)
        .execute(pool)
        .await?;
    // `column` is always one of our own constants, never user input.
    let sql = format!("UPDATE guild_feed SET {column} = ?2 WHERE guild_id = ?1");
    sqlx::query(&sql)
        .bind(guild)
        .bind(bind)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_channel(pool: &SqlitePool, guild: i64, channel: i64) -> Result<()> {
    set(pool, guild, "channel_id", channel).await
}
pub async fn set_enabled(pool: &SqlitePool, guild: i64, on: bool) -> Result<()> {
    set(pool, guild, "enabled", on).await
}
pub async fn set_min_popularity(pool: &SqlitePool, guild: i64, n: i64) -> Result<()> {
    set(pool, guild, "min_popularity", n.max(0)).await
}
pub async fn set_formats(pool: &SqlitePool, guild: i64, formats: &[String]) -> Result<()> {
    let clean: Vec<&str> = formats
        .iter()
        .map(String::as_str)
        .filter(|f| ALL_FORMATS.contains(f))
        .collect();
    set(pool, guild, "formats", clean.join(",")).await
}
pub async fn set_excluded_genres(pool: &SqlitePool, guild: i64, genres: &[String]) -> Result<()> {
    set(pool, guild, "excluded_genres", genres.join(",")).await
}
pub async fn set_digest_hours(pool: &SqlitePool, guild: i64, hours: i64) -> Result<()> {
    set(pool, guild, "digest_hours", hours.clamp(0, 72)).await
}
pub async fn set_daily_cap(pool: &SqlitePool, guild: i64, cap: i64) -> Result<()> {
    set(pool, guild, "daily_cap", cap.max(0)).await
}
pub async fn set_role(pool: &SqlitePool, guild: i64, role: Option<i64>) -> Result<()> {
    set(pool, guild, "role_id", role).await
}
pub async fn set_only_listed(pool: &SqlitePool, guild: i64, only_listed: bool) -> Result<()> {
    set(
        pool,
        guild,
        "mode",
        (if only_listed { "listed" } else { "all" }).to_string(),
    )
    .await
}
pub async fn set_last_digest(pool: &SqlitePool, guild: i64, at: i64) -> Result<()> {
    set(pool, guild, "last_digest_at", at).await
}

pub async fn set_birthday_channel(pool: &SqlitePool, guild: i64, channel: i64) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO guild_settings (guild_id) VALUES (?1)")
        .bind(guild)
        .execute(pool)
        .await?;
    sqlx::query("UPDATE guild_settings SET birthday_channel = ?2 WHERE guild_id = ?1")
        .bind(guild)
        .bind(channel)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_birthdays_enabled(pool: &SqlitePool, guild: i64, on: bool) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO guild_settings (guild_id) VALUES (?1)")
        .bind(guild)
        .execute(pool)
        .await?;
    sqlx::query("UPDATE guild_settings SET birthday_channel_enabled = ?2 WHERE guild_id = ?1")
        .bind(guild)
        .bind(on)
        .execute(pool)
        .await?;
    Ok(())
}

/// Servers that want the daily birthday post: `(guild_id, channel_id)`.
pub async fn birthday_targets(pool: &SqlitePool) -> Result<Vec<(i64, i64)>> {
    let rows = sqlx::query("SELECT guild_id, birthday_channel FROM guild_settings WHERE birthday_channel_enabled = 1 AND birthday_channel IS NOT NULL")
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| (r.get("guild_id"), r.get("birthday_channel")))
        .collect())
}

// ---- notification list --------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListMode {
    Listed,
    Pinned,
    Muted,
}

impl ListMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Listed => "listed",
            Self::Pinned => "pinned",
            Self::Muted => "muted",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "listed" => Some(Self::Listed),
            "pinned" => Some(Self::Pinned),
            "muted" => Some(Self::Muted),
            _ => None,
        }
    }
}

pub async fn set_list_entry(
    pool: &SqlitePool,
    guild: i64,
    work: i64,
    title: &str,
    mode: ListMode,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO alert_list (guild_id, work_id, title, mode) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(guild_id, work_id) DO UPDATE SET mode = excluded.mode, title = excluded.title",
    )
    .bind(guild)
    .bind(work)
    .bind(title)
    .bind(mode.as_str())
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM alert_requests WHERE guild_id = ?1 AND work_id = ?2")
        .bind(guild)
        .bind(work)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn remove_list_entry(pool: &SqlitePool, guild: i64, work: i64) -> Result<bool> {
    let r = sqlx::query("DELETE FROM alert_list WHERE guild_id = ?1 AND work_id = ?2")
        .bind(guild)
        .bind(work)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn list_entries(pool: &SqlitePool, guild: i64) -> Result<Vec<(i64, String, ListMode)>> {
    let rows = sqlx::query(
        "SELECT work_id, title, mode FROM alert_list WHERE guild_id = ?1 ORDER BY title",
    )
    .bind(guild)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            Some((
                r.get("work_id"),
                r.get("title"),
                ListMode::parse(&r.get::<String, _>("mode"))?,
            ))
        })
        .collect())
}

// ---- member requests ----------------------------------------------------------------------

/// `false` when this member already asked for it.
pub async fn add_request(
    pool: &SqlitePool,
    guild: i64,
    work: i64,
    title: &str,
    user: i64,
) -> Result<bool> {
    let r = sqlx::query("INSERT OR IGNORE INTO alert_requests (guild_id, work_id, title, user_id) VALUES (?1, ?2, ?3, ?4)")
        .bind(guild)
        .bind(work)
        .bind(title)
        .bind(user)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// Pending requests grouped by work: `(work_id, title, requesters)`.
pub async fn pending_requests(pool: &SqlitePool, guild: i64) -> Result<Vec<(i64, String, i64)>> {
    let rows = sqlx::query("SELECT work_id, title, count(*) AS n FROM alert_requests WHERE guild_id = ?1 GROUP BY work_id ORDER BY n DESC, title LIMIT 25")
        .bind(guild)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| (r.get("work_id"), r.get("title"), r.get("n")))
        .collect())
}

pub async fn deny_request(pool: &SqlitePool, guild: i64, work: i64) -> Result<bool> {
    let r = sqlx::query("DELETE FROM alert_requests WHERE guild_id = ?1 AND work_id = ?2")
        .bind(guild)
        .bind(work)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

// ---- dedupe, cap and digest ---------------------------------------------------------------

pub async fn already_posted(
    pool: &SqlitePool,
    guild: i64,
    work: i64,
    episode: i64,
) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM posted_episodes WHERE guild_id = ?1 AND work_id = ?2 AND episode = ?3")
        .bind(guild)
        .bind(work)
        .bind(episode)
        .fetch_one(pool)
        .await?
        > 0)
}

pub async fn mark_posted(
    pool: &SqlitePool,
    guild: i64,
    work: i64,
    episode: i64,
    at: i64,
) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO posted_episodes (guild_id, work_id, episode, posted_at) VALUES (?1, ?2, ?3, ?4)")
        .bind(guild)
        .bind(work)
        .bind(episode)
        .bind(at)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn posted_since(pool: &SqlitePool, guild: i64, since: i64) -> Result<i64> {
    sqlx::query_scalar(
        "SELECT count(*) FROM posted_episodes WHERE guild_id = ?1 AND posted_at >= ?2",
    )
    .bind(guild)
    .bind(since)
    .fetch_one(pool)
    .await
}

/// Forget posted rows older than `before` so the table stays small.
pub async fn prune_posted(pool: &SqlitePool, before: i64) -> Result<u64> {
    Ok(
        sqlx::query("DELETE FROM posted_episodes WHERE posted_at < ?1")
            .bind(before)
            .execute(pool)
            .await?
            .rows_affected(),
    )
}

pub async fn enqueue_digest(
    pool: &SqlitePool,
    guild: i64,
    work: i64,
    episode: i64,
    title: &str,
    airing_at: i64,
) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO digest_queue (guild_id, work_id, episode, title, airing_at) VALUES (?1, ?2, ?3, ?4, ?5)")
        .bind(guild)
        .bind(work)
        .bind(episode)
        .bind(title)
        .bind(airing_at)
        .execute(pool)
        .await?;
    Ok(())
}

/// Take (and clear) the queued digest entries: `(work_id, episode, title, airing_at)`.
pub async fn drain_digest(pool: &SqlitePool, guild: i64) -> Result<Vec<(i64, i64, String, i64)>> {
    let rows = sqlx::query("SELECT work_id, episode, title, airing_at FROM digest_queue WHERE guild_id = ?1 ORDER BY airing_at")
        .bind(guild)
        .fetch_all(pool)
        .await?;
    sqlx::query("DELETE FROM digest_queue WHERE guild_id = ?1")
        .bind(guild)
        .execute(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                r.get("work_id"),
                r.get("episode"),
                r.get("title"),
                r.get("airing_at"),
            )
        })
        .collect())
}

pub async fn last_checked(pool: &SqlitePool) -> Result<i64> {
    sqlx::query_scalar("SELECT last_checked FROM feed_state WHERE id = 1")
        .fetch_one(pool)
        .await
}

pub async fn set_last_checked(pool: &SqlitePool, at: i64) -> Result<()> {
    sqlx::query("UPDATE feed_state SET last_checked = ?1 WHERE id = 1")
        .bind(at)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
