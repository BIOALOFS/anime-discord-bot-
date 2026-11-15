//! The airing feed: one shared, paged AniList poll for every server, per-server dedupe, filters,
//! digest mode and the daily birthday post. Decisions are pure functions so they can be tested.

use crate::{
    anilist::{models::Media, AiringEntry, ContentPolicy, SafeAniList},
    db::feed::{self, FeedConfig, ListMode},
    i18n,
    ui::{
        embeds::Emb,
        format::{score_text, ts_full, ts_relative},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::watch;

const TICK: Duration = Duration::from_secs(300);
/// An episode that aired longer ago than this is never posted (restart catch-up window).
pub const CATCH_UP_SECS: i64 = 6 * 3600;
/// Look slightly back from the last successful check so a late AniList update is not missed.
const OVERLAP_SECS: i64 = 15 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    NotDue,
    TooOld,
    AlreadyPosted,
    Muted,
    NotListed,
    WrongFormat,
    ExcludedGenre,
    BelowPopularity,
    DailyCap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Post,
    /// Queue for the next digest instead of posting now.
    Digest,
    Skip(Skip),
}

/// Should this airing be announced in this server right now?
pub fn decide(
    entry: &AiringEntry,
    cfg: &FeedConfig,
    now: i64,
    list_mode: Option<ListMode>,
    already_posted: bool,
    posted_today: i64,
) -> Decision {
    // Post only once `airingAt` has passed; an episode AniList moved later is simply not due.
    if entry.airing_at > now {
        return Decision::Skip(Skip::NotDue);
    }
    if now - entry.airing_at > CATCH_UP_SECS {
        return Decision::Skip(Skip::TooOld);
    }
    if already_posted {
        return Decision::Skip(Skip::AlreadyPosted);
    }
    if list_mode == Some(ListMode::Muted) {
        return Decision::Skip(Skip::Muted);
    }
    let listed = matches!(list_mode, Some(ListMode::Listed | ListMode::Pinned));
    if cfg.only_listed && !listed {
        return Decision::Skip(Skip::NotListed);
    }
    let pinned = list_mode == Some(ListMode::Pinned);
    let m = &entry.media;
    if !pinned {
        let format_ok = m
            .format
            .as_deref()
            .is_some_and(|f| cfg.formats.iter().any(|x| x == f));
        if !format_ok {
            return Decision::Skip(Skip::WrongFormat);
        }
        if m.genres.iter().any(|g| {
            cfg.excluded_genres
                .iter()
                .any(|x| x.eq_ignore_ascii_case(g))
        }) {
            return Decision::Skip(Skip::ExcludedGenre);
        }
        if !listed && m.popularity.unwrap_or(0) < cfg.min_popularity {
            return Decision::Skip(Skip::BelowPopularity);
        }
        if cfg.daily_cap > 0 && posted_today >= cfg.daily_cap {
            return Decision::Skip(Skip::DailyCap);
        }
    }
    if cfg.digest_hours > 0 {
        Decision::Digest
    } else {
        Decision::Post
    }
}

/// Window to ask AniList for: from a little before the last successful check (never older than the
/// catch-up limit) up to now. A first run only looks back one overlap.
pub fn poll_window(last_checked: i64, now: i64) -> (i64, i64) {
    let from = if last_checked <= 0 {
        now - OVERLAP_SECS
    } else {
        (last_checked - OVERLAP_SECS).max(now - CATCH_UP_SECS)
    };
    (from, now)
}

/// Start of the current UTC day (for the daily cap).
pub fn day_start(now: i64) -> i64 {
    now - now.rem_euclid(86_400)
}

/// A digest is due once `hours` have passed since the last one and something is queued.
pub fn digest_due(hours: i64, last: i64, now: i64, queued: usize) -> bool {
    hours > 0 && queued > 0 && now - last >= hours * 3600
}

// ---- embeds ----------------------------------------------------------------------------------------

pub fn episode_embed(m: &Media, episode: i32, airing_at: i64) -> Emb {
    let mut lines = vec![
        format!("**{}**", i18n::episode_n(episode)),
        format!("{} · {}", ts_full(airing_at), ts_relative(airing_at)),
    ];
    let mut meta = vec![score_text(m.average_score)];
    if !m.genres.is_empty() {
        meta.push(
            m.genres
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(" • "),
        );
    }
    lines.push(meta.join(" · "));
    let mut e = Emb::new()
        .title(m.title.main())
        .url(m.site_url.clone())
        .image(
            m.cover_image
                .as_ref()
                .and_then(|c| c.extra_large.clone().or(c.large.clone())),
        )
        .description(lines.join("\n"));
    if let Some(en) = m
        .title
        .english
        .as_deref()
        .filter(|t| Some(*t) != m.title.romaji.as_deref())
    {
        e = e.author(en);
    }
    e.colour = m
        .cover_image
        .as_ref()
        .and_then(|c| c.color.as_deref())
        .and_then(crate::ui::embeds::parse_hex_colour);
    e
}

pub fn digest_embed(rows: &[(i64, i64, String, i64)]) -> Emb {
    let body = rows
        .iter()
        .map(|(work, ep, title, at)| {
            format!(
                "• [{title}](https://anilist.co/anime/{work}) — {} · {}",
                i18n::episode_n(*ep as i32),
                crate::ui::format::ts_time(*at)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Emb::new().title(i18n::FEED_DIGEST_TITLE).description(body)
}

// ---- the background task ------------------------------------------------------------------------------

pub struct Feed {
    pub pool: SqlitePool,
    pub api: Arc<SafeAniList>,
    pub http: Arc<serenity::Http>,
}

/// Run until `shutdown` flips to true. Never panics the process: every error is logged and retried.
pub async fn run(feed: Feed, mut shutdown: watch::Receiver<bool>) {
    tracing::info!("airing feed started");
    loop {
        if *shutdown.borrow() {
            break;
        }
        if let Err(e) = tick(&feed).await {
            tracing::warn!("feed tick failed (will retry): {e:#}");
        }
        tokio::select! {
            _ = tokio::time::sleep(TICK) => {}
            _ = shutdown.changed() => {}
        }
    }
    tracing::info!("airing feed stopped");
}

async fn tick(f: &Feed) -> anyhow::Result<()> {
    let now = chrono::Utc::now().timestamp();
    let guilds = feed::active_guilds(&f.pool).await?;
    if !guilds.is_empty() {
        let (from, to) = poll_window(feed::last_checked(&f.pool).await?, now);
        // One shared fetch for every server; AniList failures keep `last_checked` so we retry the same window.
        let entries = f
            .api
            .airing_between(from, to + 1, ContentPolicy::SAFE)
            .await?;
        for cfg in &guilds {
            if let Err(e) = serve_guild(f, cfg, &entries, now).await {
                tracing::warn!(guild = cfg.guild_id, "feed for guild failed: {e:#}");
            }
        }
        feed::set_last_checked(&f.pool, now).await?;
    }
    feed::prune_posted(&f.pool, now - 2 * 86_400).await?;
    post_birthdays(f, now).await
}

async fn serve_guild(
    f: &Feed,
    cfg: &FeedConfig,
    entries: &[AiringEntry],
    now: i64,
) -> anyhow::Result<()> {
    let Some(channel) = cfg.channel_id else {
        return Ok(());
    };
    let channel = serenity::ChannelId::new(channel as u64);
    let modes: HashMap<i64, ListMode> = feed::list_entries(&f.pool, cfg.guild_id)
        .await?
        .into_iter()
        .map(|(id, _, m)| (id, m))
        .collect();
    let mut posted_today = feed::posted_since(&f.pool, cfg.guild_id, day_start(now)).await?;
    for e in entries {
        let already =
            feed::already_posted(&f.pool, cfg.guild_id, e.media.id, i64::from(e.episode)).await?;
        match decide(
            e,
            cfg,
            now,
            modes.get(&e.media.id).copied(),
            already,
            posted_today,
        ) {
            Decision::Skip(_) => {}
            Decision::Digest => {
                feed::enqueue_digest(
                    &f.pool,
                    cfg.guild_id,
                    e.media.id,
                    i64::from(e.episode),
                    e.media.title.main(),
                    e.airing_at,
                )
                .await?;
                feed::mark_posted(&f.pool, cfg.guild_id, e.media.id, i64::from(e.episode), now)
                    .await?;
            }
            Decision::Post => {
                if send_episode(f, channel, cfg, e).await {
                    feed::mark_posted(&f.pool, cfg.guild_id, e.media.id, i64::from(e.episode), now)
                        .await?;
                    posted_today += 1;
                }
            }
        }
    }
    if cfg.digest_hours > 0 {
        let queued = feed::drain_digest(&f.pool, cfg.guild_id).await?;
        if digest_due(cfg.digest_hours, cfg.last_digest_at, now, queued.len()) {
            let msg = serenity::CreateMessage::new().embed(digest_embed(&queued).build());
            if channel.send_message(&f.http, msg).await.is_ok() {
                feed::set_last_digest(&f.pool, cfg.guild_id, now).await?;
            } else {
                // Put them back so nothing is lost.
                for (w, ep, t, at) in queued {
                    feed::enqueue_digest(&f.pool, cfg.guild_id, w, ep, &t, at).await?;
                }
            }
        } else {
            for (w, ep, t, at) in queued {
                feed::enqueue_digest(&f.pool, cfg.guild_id, w, ep, &t, at).await?;
            }
        }
    }
    Ok(())
}

async fn send_episode(
    f: &Feed,
    channel: serenity::ChannelId,
    cfg: &FeedConfig,
    e: &AiringEntry,
) -> bool {
    let m = &e.media;
    let mut links = Vec::new();
    if let Some(u) = &m.site_url {
        links.push(serenity::CreateButton::new_link(u).label(format!("🔗 {}", i18n::BTN_ANILIST)));
    }
    if let Some(u) = m.streaming_link() {
        links.push(serenity::CreateButton::new_link(u).label(format!("▶ {}", i18n::BTN_WATCH)));
    }
    let mut msg =
        serenity::CreateMessage::new().embed(episode_embed(m, e.episode, e.airing_at).build());
    if !links.is_empty() {
        msg = msg.components(vec![serenity::CreateActionRow::Buttons(links)]);
    }
    if let Some(role) = cfg.role_id {
        msg = msg.content(format!("<@&{role}>")).allowed_mentions(
            serenity::CreateAllowedMentions::new().roles(vec![serenity::RoleId::new(role as u64)]),
        );
    }
    match channel.send_message(&f.http, msg).await {
        Ok(_) => true,
        Err(e) => {
            tracing::debug!("could not post an episode: {e}");
            false
        }
    }
}

async fn post_birthdays(f: &Feed, now: i64) -> anyhow::Result<()> {
    let today = chrono::DateTime::from_timestamp(now, 0)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    let last: String = sqlx::query_scalar("SELECT last_birthday_day FROM feed_state WHERE id = 1")
        .fetch_one(&f.pool)
        .await?;
    if today.is_empty() || last == today {
        return Ok(());
    }
    let targets = feed::birthday_targets(&f.pool).await?;
    if !targets.is_empty() {
        let list = f.api.birthdays_today(ContentPolicy::SAFE).await?;
        if !list.is_empty() {
            let body = list
                .iter()
                .map(|c| match c.main_work() {
                    Some(w) => format!("🎂 **{}** — {w}", c.display_name()),
                    None => format!("🎂 **{}**", c.display_name()),
                })
                .collect::<Vec<_>>()
                .join("\n");
            let embed = Emb::new()
                .title(i18n::BIRTHDAYS_TITLE)
                .description(body)
                .image(list.first().and_then(|c| c.image_url()).map(str::to_string))
                .build();
            for (_, channel) in targets {
                let msg = serenity::CreateMessage::new().embed(embed.clone());
                if let Err(e) = serenity::ChannelId::new(channel as u64)
                    .send_message(&f.http, msg)
                    .await
                {
                    tracing::debug!("could not post birthdays: {e}");
                }
            }
        }
    }
    sqlx::query("UPDATE feed_state SET last_birthday_day = ?1 WHERE id = 1")
        .bind(today)
        .execute(&f.pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
