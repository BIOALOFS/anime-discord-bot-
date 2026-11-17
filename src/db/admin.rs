//! Per-server admin settings that live outside the feed and custom-image modules: who may upload pictures, the
//! management role, the cooldown override and the full server reset.

use sqlx::SqlitePool;

pub type Result<T> = std::result::Result<T, sqlx::Error>;

async fn ensure(pool: &SqlitePool, guild: i64) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO guild_settings (guild_id) VALUES (?1)")
        .bind(guild)
        .execute(pool)
        .await?;
    Ok(())
}

/// `true` when only admins may upload custom pictures (default: everyone, once custom images are on).
pub async fn upload_admins_only(pool: &SqlitePool, guild: i64) -> Result<bool> {
    let v: Option<String> =
        sqlx::query_scalar("SELECT upload_policy FROM guild_settings WHERE guild_id = ?1")
            .bind(guild)
            .fetch_optional(pool)
            .await?;
    Ok(v.as_deref() == Some("admins"))
}

pub async fn set_upload_admins_only(pool: &SqlitePool, guild: i64, admins: bool) -> Result<()> {
    ensure(pool, guild).await?;
    sqlx::query("UPDATE guild_settings SET upload_policy = ?2 WHERE guild_id = ?1")
        .bind(guild)
        .bind(if admins { "admins" } else { "everyone" })
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn manager_role(pool: &SqlitePool, guild: i64) -> Result<Option<i64>> {
    Ok(sqlx::query_scalar::<_, Option<i64>>(
        "SELECT manager_role_id FROM guild_settings WHERE guild_id = ?1",
    )
    .bind(guild)
    .fetch_optional(pool)
    .await?
    .flatten())
}

pub async fn set_manager_role(pool: &SqlitePool, guild: i64, role: Option<i64>) -> Result<()> {
    ensure(pool, guild).await?;
    sqlx::query("UPDATE guild_settings SET manager_role_id = ?2 WHERE guild_id = ?1")
        .bind(guild)
        .bind(role)
        .execute(pool)
        .await?;
    Ok(())
}

/// Per-user cooldown override in seconds; 0 means the bot's default.
pub async fn cooldown_secs(pool: &SqlitePool, guild: i64) -> Result<i64> {
    Ok(
        sqlx::query_scalar::<_, i64>(
            "SELECT cooldown_secs FROM guild_settings WHERE guild_id = ?1",
        )
        .bind(guild)
        .fetch_optional(pool)
        .await?
        .unwrap_or(0),
    )
}

pub async fn set_cooldown_secs(pool: &SqlitePool, guild: i64, secs: i64) -> Result<()> {
    ensure(pool, guild).await?;
    sqlx::query("UPDATE guild_settings SET cooldown_secs = ?2 WHERE guild_id = ?1")
        .bind(guild)
        .bind(secs.clamp(0, 600))
        .execute(pool)
        .await?;
    Ok(())
}

/// How many suggestions one member may add per day in this server (0 = unlimited).
pub async fn suggest_limit(pool: &SqlitePool, guild: i64) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT suggest_daily_limit FROM guild_settings WHERE guild_id = ?1",
    )
    .bind(guild)
    .fetch_optional(pool)
    .await?
    .unwrap_or(crate::db::suggest::DEFAULT_DAILY_LIMIT))
}

pub async fn set_suggest_limit(pool: &SqlitePool, guild: i64, limit: i64) -> Result<()> {
    ensure(pool, guild).await?;
    sqlx::query("UPDATE guild_settings SET suggest_daily_limit = ?2 WHERE guild_id = ?1")
        .bind(guild)
        .bind(limit.clamp(0, 1000))
        .execute(pool)
        .await?;
    Ok(())
}

/// May a reminder fall back to a ping in the notifications channel when the member's DMs are closed? (default: no)
pub async fn reminder_ping(pool: &SqlitePool, guild: i64) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT reminder_ping FROM guild_settings WHERE guild_id = ?1",
    )
    .bind(guild)
    .fetch_optional(pool)
    .await?
    .unwrap_or(false))
}

pub async fn set_reminder_ping(pool: &SqlitePool, guild: i64, on: bool) -> Result<()> {
    ensure(pool, guild).await?;
    sqlx::query("UPDATE guild_settings SET reminder_ping = ?2 WHERE guild_id = ?1")
        .bind(guild)
        .bind(on)
        .execute(pool)
        .await?;
    Ok(())
}

/// Forget everything the bot stores about one server. Members' global profiles are untouched.
pub async fn reset_guild(pool: &SqlitePool, guild: i64) -> Result<()> {
    let mut tx = pool.begin().await?;
    for sql in [
        "DELETE FROM guild_settings WHERE guild_id = ?1",
        "DELETE FROM guild_feed WHERE guild_id = ?1",
        "DELETE FROM posted_episodes WHERE guild_id = ?1",
        "DELETE FROM suggestions WHERE guild_id = ?1",
        "DELETE FROM user_guilds WHERE guild_id = ?1",
        "DELETE FROM upload_bans WHERE guild_id = ?1",
        "DELETE FROM game_scores WHERE guild_id = ?1",
    ] {
        sqlx::query(sql).bind(guild).execute(&mut *tx).await?;
    }
    tx.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::feed;

    #[tokio::test]
    async fn settings_round_trip_with_safe_defaults() {
        let pool = crate::db::connect_memory().await;
        assert!(!upload_admins_only(&pool, 1).await.expect("read"));
        assert_eq!(manager_role(&pool, 1).await.expect("read"), None);
        assert_eq!(cooldown_secs(&pool, 1).await.expect("read"), 0);
        assert_eq!(
            suggest_limit(&pool, 1).await.expect("read"),
            10,
            "10 suggestions a day by default"
        );
        assert!(
            !reminder_ping(&pool, 1).await.expect("read"),
            "channel pings are off until the server allows them"
        );
        set_suggest_limit(&pool, 1, 5).await.expect("set");
        set_reminder_ping(&pool, 1, true).await.expect("set");
        assert_eq!(suggest_limit(&pool, 1).await.expect("read"), 5);
        assert!(reminder_ping(&pool, 1).await.expect("read"));
        set_upload_admins_only(&pool, 1, true).await.expect("set");
        set_manager_role(&pool, 1, Some(77)).await.expect("set");
        set_cooldown_secs(&pool, 1, 9_999).await.expect("set");
        assert!(upload_admins_only(&pool, 1).await.expect("read"));
        assert_eq!(manager_role(&pool, 1).await.expect("read"), Some(77));
        assert_eq!(cooldown_secs(&pool, 1).await.expect("read"), 600, "clamped");
        set_manager_role(&pool, 1, None).await.expect("clear");
        assert_eq!(manager_role(&pool, 1).await.expect("read"), None);
    }

    #[tokio::test]
    async fn reset_clears_only_that_server() {
        let pool = crate::db::connect_memory().await;
        for g in [1, 2] {
            feed::set_channel(&pool, g, 50 + g).await.expect("channel");
            set_cooldown_secs(&pool, g, 5).await.expect("cooldown");
        }
        reset_guild(&pool, 1).await.expect("reset");
        assert_eq!(cooldown_secs(&pool, 1).await.expect("read"), 0);
        assert_eq!(
            feed::get_config(&pool, 1).await.expect("cfg").channel_id,
            None
        );
        assert_eq!(cooldown_secs(&pool, 2).await.expect("read"), 5);
        assert_eq!(
            feed::get_config(&pool, 2).await.expect("cfg").channel_id,
            Some(52)
        );
    }

    #[tokio::test]
    async fn setting_the_channel_turns_notifications_on_once() {
        let pool = crate::db::connect_memory().await;
        assert!(!feed::get_config(&pool, 1).await.expect("cfg").enabled);
        feed::set_channel(&pool, 1, 10).await.expect("channel");
        assert!(
            feed::get_config(&pool, 1).await.expect("cfg").enabled,
            "default on once a channel is set"
        );
        feed::set_enabled(&pool, 1, false).await.expect("off");
        feed::set_channel(&pool, 1, 11).await.expect("channel");
        let c = feed::get_config(&pool, 1).await.expect("cfg");
        assert!(!c.enabled, "changing the channel keeps an explicit off");
        assert_eq!(c.channel_id, Some(11));
        feed::clear_channel(&pool, 1).await.expect("clear");
        assert_eq!(
            feed::get_config(&pool, 1).await.expect("cfg").channel_id,
            None
        );
    }
}

/// The server's time zone for schedules and notification hours (default Muscat).
pub async fn timezone(pool: &SqlitePool, guild: i64) -> Result<String> {
    Ok(
        sqlx::query_scalar::<_, String>("SELECT timezone FROM guild_settings WHERE guild_id = ?1")
            .bind(guild)
            .fetch_optional(pool)
            .await?
            .unwrap_or_else(|| "Asia/Muscat".to_string()),
    )
}

pub async fn set_timezone(pool: &SqlitePool, guild: i64, tz: &str) -> Result<()> {
    if tz.parse::<chrono_tz::Tz>().is_err() {
        return Ok(());
    }
    ensure(pool, guild).await?;
    sqlx::query("UPDATE guild_settings SET timezone = ?2 WHERE guild_id = ?1")
        .bind(guild)
        .bind(tz)
        .execute(pool)
        .await?;
    Ok(())
}

/// The spoiler guard of the whole server (off by default).
pub async fn spoiler_guard(pool: &SqlitePool, guild: i64) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT spoiler_guard FROM guild_settings WHERE guild_id = ?1",
    )
    .bind(guild)
    .fetch_optional(pool)
    .await?
    .unwrap_or(false))
}

pub async fn set_spoiler_guard(pool: &SqlitePool, guild: i64, on: bool) -> Result<()> {
    ensure(pool, guild).await?;
    sqlx::query("UPDATE guild_settings SET spoiler_guard = ?2 WHERE guild_id = ?1")
        .bind(guild)
        .bind(on)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod round7_tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn timezone_and_spoiler_defaults_round_trip_and_reject_nonsense() {
        let p = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("db");
        sqlx::migrate!("./migrations")
            .run(&p)
            .await
            .expect("migrate");
        assert_eq!(timezone(&p, 1).await.expect("tz"), "Asia/Muscat");
        assert!(!spoiler_guard(&p, 1).await.expect("sp"));
        set_timezone(&p, 1, "Asia/Riyadh").await.expect("set");
        set_timezone(&p, 1, "Not/AZone").await.expect("ignored");
        set_spoiler_guard(&p, 1, true).await.expect("set");
        assert_eq!(timezone(&p, 1).await.expect("tz"), "Asia/Riyadh");
        assert!(spoiler_guard(&p, 1).await.expect("sp"));
        assert!(
            crate::db::notify::spoiler_guard(&p, Some(1), 5).await,
            "the server setting applies to its members"
        );
        assert!(!crate::db::notify::spoiler_guard(&p, Some(2), 5).await);
    }
}
