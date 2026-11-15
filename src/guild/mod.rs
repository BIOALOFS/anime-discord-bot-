//! Per-guild channel lock: where the bot is allowed to answer commands.
//!
//! `is_allowed` is a pure function so the rules are easy to test; `GuildStore` reads/writes the
//! settings (with a short cache) and `check` applies them to commands, autocomplete and components.

use moka::future::Cache;
use poise::serenity_prelude as serenity;
use sqlx::{Row, SqlitePool};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub const MAX_EXTRA_CHANNELS: usize = 5;
/// Commands that always work so a server can be configured.
const EXEMPT_ROOTS: &[&str] = &[
    "settings",
    "help",
    "admin",
    "set-bot-channel",
    "set-notifications-channel",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelSettings {
    pub command_channel: Option<i64>,
    pub extra: Vec<i64>,
    pub admin_bypass: bool,
    /// False when the configured command channel no longer exists (deleted).
    pub channel_exists: bool,
}

impl Default for ChannelSettings {
    fn default() -> Self {
        Self {
            command_channel: None,
            extra: Vec::new(),
            admin_bypass: true,
            channel_exists: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    /// Used in the wrong channel; the bot answers only in `channel`.
    Denied {
        channel: i64,
    },
}

/// The rules: no channel set -> everywhere; the command channel and extras are allowed; admins may bypass
/// (default on); a deleted command channel falls back to "no restriction".
pub fn is_allowed(channel: i64, s: &ChannelSettings, is_admin: bool) -> Decision {
    let Some(main) = s.command_channel else {
        return Decision::Allowed;
    };
    if !s.channel_exists
        || channel == main
        || s.extra.contains(&channel)
        || (is_admin && s.admin_bypass)
    {
        return Decision::Allowed;
    }
    Decision::Denied { channel: main }
}

/// Commands that must work in every channel (`/setup ...`, `/help`).
pub fn is_exempt(qualified_name: &str) -> bool {
    let root = qualified_name.split_whitespace().next().unwrap_or("");
    EXEMPT_ROOTS.contains(&root)
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExtraResult {
    Added,
    AlreadyThere,
    Full,
}

pub struct GuildStore {
    pool: SqlitePool,
    cache: Cache<i64, Arc<ChannelSettings>>,
    /// Last command time per (server, user) for the per-server cooldown.
    last_use: Mutex<HashMap<(i64, i64), Instant>>,
}

impl GuildStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            last_use: Mutex::new(HashMap::new()),
            cache: Cache::builder()
                .max_capacity(2_000)
                .time_to_live(Duration::from_secs(30))
                .build(),
        }
    }

    /// `Some(wait)` when the user must still wait `wait` before another command (server cooldown, `secs` > 0).
    pub fn cooldown_left(&self, guild: i64, user: i64, secs: u64) -> Option<Duration> {
        if secs == 0 {
            return None;
        }
        let mut map = self.last_use.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        // Bounded: forget entries older than the longest cooldown.
        if map.len() > 10_000 {
            map.retain(|_, t| now.duration_since(*t) < Duration::from_secs(600));
        }
        let limit = Duration::from_secs(secs);
        if let Some(last) = map.get(&(guild, user)) {
            let since = now.duration_since(*last);
            if since < limit {
                return Some(limit - since);
            }
        }
        map.insert((guild, user), now);
        None
    }

    pub async fn settings(&self, guild: i64) -> Result<Arc<ChannelSettings>, sqlx::Error> {
        if let Some(hit) = self.cache.get(&guild).await {
            return Ok(hit);
        }
        let row = sqlx::query(
            "SELECT command_channel_id, admin_bypass FROM guild_settings WHERE guild_id = ?1",
        )
        .bind(guild)
        .fetch_optional(&self.pool)
        .await?;
        let extra: Vec<i64> = sqlx::query_scalar(
            "SELECT channel_id FROM guild_extra_channels WHERE guild_id = ?1 ORDER BY channel_id",
        )
        .bind(guild)
        .fetch_all(&self.pool)
        .await?;
        let s = Arc::new(ChannelSettings {
            command_channel: row
                .as_ref()
                .and_then(|r| r.get::<Option<i64>, _>("command_channel_id")),
            admin_bypass: row
                .as_ref()
                .is_none_or(|r| r.get::<bool, _>("admin_bypass")),
            extra,
            channel_exists: true,
        });
        self.cache.insert(guild, s.clone()).await;
        Ok(s)
    }

    /// Remember that the command channel is gone (until settings change or the cache expires).
    pub async fn mark_channel_deleted(&self, guild: i64, current: &ChannelSettings) {
        let mut s = current.clone();
        s.channel_exists = false;
        self.cache.insert(guild, Arc::new(s)).await;
    }

    async fn ensure_row(&self, guild: i64) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT OR IGNORE INTO guild_settings (guild_id) VALUES (?1)")
            .bind(guild)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_command_channel(&self, guild: i64, channel: i64) -> Result<(), sqlx::Error> {
        self.ensure_row(guild).await?;
        sqlx::query("UPDATE guild_settings SET command_channel_id = ?2 WHERE guild_id = ?1")
            .bind(guild)
            .bind(channel)
            .execute(&self.pool)
            .await?;
        // The command channel never needs to be listed as an extra too.
        sqlx::query("DELETE FROM guild_extra_channels WHERE guild_id = ?1 AND channel_id = ?2")
            .bind(guild)
            .bind(channel)
            .execute(&self.pool)
            .await?;
        self.cache.invalidate(&guild).await;
        Ok(())
    }

    pub async fn add_extra(&self, guild: i64, channel: i64) -> Result<ExtraResult, sqlx::Error> {
        let current = self.settings(guild).await?;
        if current.extra.contains(&channel) || current.command_channel == Some(channel) {
            return Ok(ExtraResult::AlreadyThere);
        }
        if current.extra.len() >= MAX_EXTRA_CHANNELS {
            return Ok(ExtraResult::Full);
        }
        sqlx::query(
            "INSERT OR IGNORE INTO guild_extra_channels (guild_id, channel_id) VALUES (?1, ?2)",
        )
        .bind(guild)
        .bind(channel)
        .execute(&self.pool)
        .await?;
        self.cache.invalidate(&guild).await;
        Ok(ExtraResult::Added)
    }

    pub async fn remove_extra(&self, guild: i64, channel: i64) -> Result<bool, sqlx::Error> {
        let r =
            sqlx::query("DELETE FROM guild_extra_channels WHERE guild_id = ?1 AND channel_id = ?2")
                .bind(guild)
                .bind(channel)
                .execute(&self.pool)
                .await?;
        self.cache.invalidate(&guild).await;
        Ok(r.rows_affected() > 0)
    }

    pub async fn set_admin_bypass(&self, guild: i64, on: bool) -> Result<(), sqlx::Error> {
        self.ensure_row(guild).await?;
        sqlx::query("UPDATE guild_settings SET admin_bypass = ?2 WHERE guild_id = ?1")
            .bind(guild)
            .bind(on)
            .execute(&self.pool)
            .await?;
        self.cache.invalidate(&guild).await;
        Ok(())
    }

    /// Clears the command channel and the extras (the admin-bypass choice is kept).
    pub async fn reset(&self, guild: i64) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE guild_settings SET command_channel_id = NULL WHERE guild_id = ?1")
            .bind(guild)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM guild_extra_channels WHERE guild_id = ?1")
            .bind(guild)
            .execute(&self.pool)
            .await?;
        self.cache.invalidate(&guild).await;
        Ok(())
    }
}

pub fn is_admin(perms: Option<serenity::Permissions>) -> bool {
    perms.is_some_and(|p| p.manage_guild() || p.administrator())
}

/// Admin commands: Manage Guild / Administrator, or the server's configured management role.
pub fn can_manage(
    perms: Option<serenity::Permissions>,
    member_roles: &[i64],
    manager_role: Option<i64>,
) -> bool {
    is_admin(perms) || manager_role.is_some_and(|r| member_roles.contains(&r))
}

/// Shared by commands, autocomplete and components: `Some(text)` is the ephemeral refusal.
/// Errors fail open (a broken settings read must not lock everyone out).
pub async fn denial(
    store: &GuildStore,
    http: &serenity::Http,
    guild: Option<serenity::GuildId>,
    channel: serenity::ChannelId,
    admin: bool,
) -> Option<String> {
    let guild = guild?.get() as i64;
    let settings = match store.settings(guild).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("could not read guild settings: {e}");
            return None;
        }
    };
    let Decision::Denied { channel: main } = is_allowed(channel.get() as i64, &settings, admin)
    else {
        return None;
    };
    // The lock only holds while the command channel exists.
    if let Err(serenity::Error::Http(e)) =
        serenity::ChannelId::new(main as u64).to_channel(http).await
    {
        if matches!(&e, serenity::HttpError::UnsuccessfulRequest(r) if r.status_code.as_u16() == 404)
        {
            tracing::warn!(
                guild,
                channel = main,
                "configured command channel was deleted; lock disabled"
            );
            store.mark_channel_deleted(guild, &settings).await;
            return None;
        }
    }
    Some(crate::i18n::wrong_channel(main))
}

pub async fn command_check(ctx: crate::error::Context<'_>) -> Result<bool, crate::error::Error> {
    if is_exempt(&ctx.command().qualified_name) {
        return Ok(true);
    }
    let admin = is_admin(ctx.author_member().await.and_then(|m| m.permissions));
    let data = ctx.data();
    if let Some(g) = ctx.guild_id() {
        let secs = crate::db::admin::cooldown_secs(&data.db, g.get() as i64)
            .await
            .unwrap_or(0);
        if let Some(wait) =
            data.guilds
                .cooldown_left(g.get() as i64, ctx.author().id.get() as i64, secs as u64)
        {
            if !admin {
                ctx.send(
                    poise::CreateReply::default()
                        .content(crate::i18n::cooldown_wait(wait.as_secs() + 1))
                        .ephemeral(true),
                )
                .await?;
                return Ok(false);
            }
        }
    }
    let Some(text) = denial(
        &data.guilds,
        &ctx.serenity_context().http,
        ctx.guild_id(),
        ctx.channel_id(),
        admin,
    )
    .await
    else {
        return Ok(true);
    };
    ctx.send(poise::CreateReply::default().content(text).ephemeral(true))
        .await?;
    Ok(false)
}

/// For autocomplete: a quiet yes/no (the user simply gets no suggestions).
pub async fn autocomplete_allowed(ctx: crate::error::Context<'_>) -> bool {
    if is_exempt(&ctx.command().qualified_name) {
        return true;
    }
    let admin = is_admin(ctx.author_member().await.and_then(|m| m.permissions));
    denial(
        &ctx.data().guilds,
        &ctx.serenity_context().http,
        ctx.guild_id(),
        ctx.channel_id(),
        admin,
    )
    .await
    .is_none()
}

#[cfg(test)]
mod tests;
