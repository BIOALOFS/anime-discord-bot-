//! Notification feed configuration and the per-server dedupe of posted episodes.

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
    pub role_id: Option<i64>,
    pub filter: FilterMode,
    /// Work kinds that may be announced (anime, manga, manhwa, manhua, novel).
    pub kinds: Vec<String>,
}

/// What the notification feed announces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    /// Every release of every work (default).
    All,
    /// Only works at or above the popularity threshold.
    Popular,
    /// Only works that members of the server have in their profiles.
    Profiles,
    /// Only the selected formats / kinds.
    Formats,
}

impl FilterMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Popular => "popular",
            Self::Profiles => "profiles",
            Self::Formats => "formats",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "all" => Self::All,
            "popular" => Self::Popular,
            "profiles" => Self::Profiles,
            "formats" => Self::Formats,
            _ => return None,
        })
    }
}

pub const ALL_KINDS: [&str; 5] = ["anime", "manga", "manhwa", "manhua", "novel"];

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
        role_id: r.get("role_id"),
        filter: FilterMode::parse(&r.get::<String, _>("filter_mode")).unwrap_or(FilterMode::All),
        kinds: split_csv(&r.get::<String, _>("kinds")),
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

/// Setting the notifications channel switches notifications on the first time (default on once a channel is set).
pub async fn set_channel(pool: &SqlitePool, guild: i64, channel: i64) -> Result<()> {
    get_config(pool, guild).await?;
    sqlx::query("UPDATE guild_feed SET enabled = CASE WHEN channel_id IS NULL THEN 1 ELSE enabled END, channel_id = ?2 WHERE guild_id = ?1")
        .bind(guild)
        .bind(channel)
        .execute(pool)
        .await?;
    Ok(())
}
pub async fn clear_channel(pool: &SqlitePool, guild: i64) -> Result<()> {
    set(pool, guild, "channel_id", Option::<i64>::None).await
}
pub async fn set_filter_mode(pool: &SqlitePool, guild: i64, mode: FilterMode) -> Result<()> {
    set(pool, guild, "filter_mode", mode.as_str().to_string()).await
}
pub async fn set_kinds(pool: &SqlitePool, guild: i64, kinds: &[String]) -> Result<()> {
    let clean: Vec<&str> = kinds
        .iter()
        .map(String::as_str)
        .filter(|k| ALL_KINDS.contains(k))
        .collect();
    set(pool, guild, "kinds", clean.join(",")).await
}

/// AniList ids of the works that members of this server have in their profiles.
pub async fn profile_work_ids(
    pool: &SqlitePool,
    guild: i64,
) -> Result<std::collections::HashSet<i64>> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT pw.work_id FROM profile_works pw JOIN user_guilds ug ON ug.user_id = pw.user_id \
         WHERE ug.guild_id = ?1 AND pw.work_id LIKE 'anilist:%'",
    )
    .bind(guild)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .filter_map(|w| w.strip_prefix("anilist:")?.parse().ok())
        .collect())
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
pub async fn set_role(pool: &SqlitePool, guild: i64, role: Option<i64>) -> Result<()> {
    set(pool, guild, "role_id", role).await
}
/// Channels where a reminder may be pinged when the member's DMs are closed: servers the member shares with the
/// bot that switched `reminder_ping` on and have a notifications channel.
pub async fn reminder_fallback_channels(pool: &SqlitePool, user: i64) -> Result<Vec<(i64, i64)>> {
    sqlx::query_as(
        "SELECT f.guild_id, f.channel_id FROM user_guilds ug \
         JOIN guild_settings s ON s.guild_id = ug.guild_id AND s.reminder_ping = 1 \
         JOIN guild_feed f ON f.guild_id = ug.guild_id AND f.channel_id IS NOT NULL \
         WHERE ug.user_id = ?1 ORDER BY f.guild_id",
    )
    .bind(user)
    .fetch_all(pool)
    .await
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
