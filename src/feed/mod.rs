//! The airing feed: one shared, paged AniList poll for every server, per-server dedupe, filters,
//! batching and member reminders. Decisions are pure functions so they can be tested.

use crate::{
    anilist::{models::Media, AiringEntry, ContentPolicy, SafeAniList},
    db::{
        feed::{self, FeedConfig, FilterMode},
        reminders,
    },
    i18n,
    ui::{
        embeds::Emb,
        format::{score_text, ts_full, ts_relative},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

pub mod summary;
pub mod tmdb_notify;

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
    NotInProfiles,
    WrongKind,
    WrongFormat,
    BelowPopularity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Post,
    Skip(Skip),
}

/// Should this airing be announced in this server right now? `in_profiles` says whether a member of the
/// server has the work in their profile (only used by the `profiles` filter).
pub fn decide(
    entry: &AiringEntry,
    cfg: &FeedConfig,
    now: i64,
    in_profiles: bool,
    already_posted: bool,
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
    let m = &entry.media;
    // Airing schedules exist for anime only, so this is the one kind the feed can announce.
    if !cfg.kinds.iter().any(|k| k == "anime") {
        return Decision::Skip(Skip::WrongKind);
    }
    match cfg.filter {
        FilterMode::All => {}
        FilterMode::Popular if m.popularity.unwrap_or(0) < cfg.min_popularity => {
            return Decision::Skip(Skip::BelowPopularity);
        }
        FilterMode::Profiles if !in_profiles => return Decision::Skip(Skip::NotInProfiles),
        FilterMode::Formats
            if !m
                .format
                .as_deref()
                .is_some_and(|f| cfg.formats.iter().any(|x| x == f)) =>
        {
            return Decision::Skip(Skip::WrongFormat);
        }
        _ => {}
    }
    Decision::Post
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

// ---- the background task ------------------------------------------------------------------------------

pub struct Feed {
    pub pool: SqlitePool,
    pub api: Arc<SafeAniList>,
    pub http: Arc<serenity::Http>,
    /// Chapter sources (MangaDex, MangaUpdates, RSS) with their caches and back-off state.
    pub chapters: Arc<crate::chapters::poll::Sources>,
    /// Unix time of the last chapter poll (chapters are checked every 15 minutes).
    pub last_chapter_poll: std::sync::atomic::AtomicI64,
    /// Dramas, series and films (reminders look up their next episode / release here).
    pub tmdb: Arc<crate::tmdb::Tmdb>,
    /// Unix time of the last TMDB reminder pass (every 30 minutes).
    pub last_tmdb_poll: std::sync::atomic::AtomicI64,
    /// Owner state, statistics and backups.
    pub ops: Arc<crate::ops::Ops>,
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
    // The optional features (daily posts, the weekly poll, watch nights): each is off until a server switches it on.
    crate::features::scheduler::tick(f, now).await;
    let guilds = feed::active_guilds(&f.pool).await?;
    // Reminders reuse the same batched fetch: no AniList call per reminder, and none at all when nothing needs it.
    if !guilds.is_empty() || reminders::any(&f.pool).await? {
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
            if let Some(channel) = cfg.channel_id {
                if let Err(e) = summary::send(f, cfg.guild_id, channel, now).await {
                    tracing::warn!(guild = cfg.guild_id, "weekly summary failed: {e:#}");
                }
            }
        }
        if let Err(e) = deliver_reminders(f, &entries, now).await {
            tracing::warn!("reminders failed (will retry): {e:#}");
        }
        feed::set_last_checked(&f.pool, now).await?;
    }
    // Chapters of tracked manga: a separate, slower poll that fails on its own.
    let last = f
        .last_chapter_poll
        .load(std::sync::atomic::Ordering::Relaxed);
    if now - last >= crate::chapters::poll::INTERVAL_SECS {
        f.last_chapter_poll
            .store(now, std::sync::atomic::Ordering::Relaxed);
        if let Err(e) = crate::chapters::poll::poll(f, now).await {
            tracing::warn!("chapter poll failed (will retry): {e:#}");
        }
    }
    // Reminders for dramas, series and films: a separate pass every 30 minutes.
    let last = f.last_tmdb_poll.load(std::sync::atomic::Ordering::Relaxed);
    if now - last >= 1800 {
        f.last_tmdb_poll
            .store(now, std::sync::atomic::Ordering::Relaxed);
        if let Err(e) = tmdb_notify::poll(f, now).await {
            tracing::warn!("tmdb reminders failed (will retry): {e:#}");
        }
    }
    // Housekeeping: usage counters are written out, old rows and old announcements are dropped, and the daily backup runs.
    if let Err(e) = crate::ops::stats::flush(&f.pool).await {
        tracing::warn!("could not write the usage counters: {e}");
    }
    let day = crate::ops::stats::today();
    let _ = crate::ops::stats::prune(&f.pool, &day).await;
    crate::ops::backup::daily(&f.ops, &f.pool, now).await;
    feed::prune_posted(&f.pool, now - 2 * 86_400).await?;
    Ok(())
}

/// The `(user, work, episode)` notifications owed for a batch of releases, in episode order. Each episode notifies a
/// user once: `last` is what was already announced, and later releases in the same batch build on earlier ones.
pub fn reminders_owed(
    entries: &[&AiringEntry],
    by_work: &std::collections::HashMap<i64, Vec<reminders::Reminder>>,
) -> Vec<(i64, i64, i64)> {
    let mut sorted: Vec<&&AiringEntry> = entries.iter().collect();
    sorted.sort_by_key(|e| (e.media.id, e.episode));
    let mut last: std::collections::HashMap<(i64, i64), i64> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for e in sorted {
        for r in by_work.get(&e.media.id).map(Vec::as_slice).unwrap_or(&[]) {
            let seen = last.entry((r.user_id, r.work_id)).or_insert(r.last_episode);
            let ep = i64::from(e.episode);
            if reminders::is_due(ep, *seen) {
                *seen = ep;
                out.push((r.user_id, r.work_id, ep));
            }
        }
    }
    out
}

async fn deliver_reminders(f: &Feed, entries: &[AiringEntry], now: i64) -> anyhow::Result<()> {
    let due: Vec<&AiringEntry> = entries
        .iter()
        .filter(|e| e.airing_at <= now && now - e.airing_at <= CATCH_UP_SECS)
        .collect();
    if due.is_empty() {
        return Ok(());
    }
    let mut ids: Vec<i64> = due.iter().map(|e| e.media.id).collect();
    ids.sort_unstable();
    ids.dedup();
    let by_work = reminders::for_works(&f.pool, &ids).await?;
    for (user, work, episode) in reminders_owed(&due, &by_work) {
        let Some(e) = due
            .iter()
            .find(|e| e.media.id == work && i64::from(e.episode) == episode)
        else {
            continue;
        };
        // Marked first: whatever happens next, this release never notifies this user twice.
        reminders::mark(&f.pool, user, work, episode).await?;
        let key = format!("anilist:{work}");
        if !crate::db::notify::allows(
            &f.pool,
            user,
            &key,
            "anime",
            e.media.format.as_deref(),
            &e.media.genres,
        )
        .await
        .unwrap_or(true)
        {
            continue;
        }
        send_reminder(f, user, e).await;
    }
    Ok(())
}

/// DM the member; if their DMs are closed, ping them in a notifications channel only where the server allows it.
async fn send_reminder(f: &Feed, user: i64, e: &AiringEntry) {
    let embed = episode_embed(&e.media, e.episode, e.airing_at).build();
    notify_user(f, user, i18n::REMINDER_DM, embed).await;
}

/// DM the member; when DMs are closed, ping them in a notifications channel only where the server allows it.
pub async fn notify_user(f: &Feed, user: i64, text: &str, embed: serenity::CreateEmbed) {
    notify_user_with(f, user, text, embed, Vec::new()).await;
}

/// Same, with component rows (the universal buttons) under the embed.
pub async fn notify_user_with(
    f: &Feed,
    user: i64,
    text: &str,
    embed: serenity::CreateEmbed,
    rows: Vec<serenity::CreateActionRow>,
) {
    let dm = async {
        let channel = serenity::UserId::new(user as u64)
            .create_dm_channel(&f.http)
            .await?;
        let mut msg = serenity::CreateMessage::new()
            .content(text)
            .embed(embed.clone());
        if !rows.is_empty() {
            msg = msg.components(rows.clone());
        }
        channel.send_message(&f.http, msg).await
    };
    if dm.await.is_ok() {
        return;
    }
    let Ok(channels) = feed::reminder_fallback_channels(&f.pool, user).await else {
        return;
    };
    for (_, channel) in channels {
        let mut msg = serenity::CreateMessage::new()
            .content(format!("<@{user}> {text}"))
            .embed(embed.clone());
        if !rows.is_empty() {
            msg = msg.components(rows.clone());
        }
        let msg = msg.allowed_mentions(
            serenity::CreateAllowedMentions::new().users(vec![serenity::UserId::new(user as u64)]),
        );
        if serenity::ChannelId::new(channel as u64)
            .send_message(&f.http, msg)
            .await
            .is_ok()
        {
            return;
        }
    }
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
    let wanted = if cfg.filter == FilterMode::Profiles {
        feed::profile_work_ids(&f.pool, cfg.guild_id).await?
    } else {
        Default::default()
    };
    let mut batch: Vec<&AiringEntry> = Vec::new();
    for e in entries {
        let already =
            feed::already_posted(&f.pool, cfg.guild_id, e.media.id, i64::from(e.episode)).await?;
        if decide(e, cfg, now, wanted.contains(&e.media.id), already) == Decision::Post {
            batch.push(e);
        }
    }
    for e in send_batch(f, channel, cfg, &batch).await {
        feed::mark_posted(&f.pool, cfg.guild_id, e.media.id, i64::from(e.episode), now).await?;
    }
    Ok(())
}

/// Menu id of the grouped notifications sent before the universal buttons (old messages keep working).
pub const SHOW_WORK_PICK: &str = "shownotif:pick";

/// The work a "show the work" press asks for: from the button id, or the chosen value of the menu.
pub fn parse_show_work(custom_id: &str, values: &[String]) -> Option<i64> {
    if custom_id == SHOW_WORK_PICK {
        return values.first()?.parse().ok();
    }
    custom_id.strip_prefix("shownotif:")?.parse().ok()
}

/// The buttons every work notification carries (the universal rule: a work message gets "أظهر الشخصيات", plus a
/// reminder). Old messages that still carry `shownotif:` buttons keep working through `parse_show_work`.
pub fn notification_buttons(anilist_id: i64) -> Vec<serenity::CreateButton> {
    let key = format!("anilist:{anilist_id}");
    let mut v = crate::components::standard_actions(crate::components::ItemKind::Work, Some(&key));
    v.push(crate::components::remind_button(&key));
    v
}

/// Up to this many releases get their own card; more are grouped so a busy night is a few messages, not a flood.
pub const MAX_INDIVIDUAL: usize = 3;
/// Lines per grouped message.
pub const LINES_PER_MESSAGE: usize = 15;

/// One compact line per release for the grouped messages (no character art).
pub fn compact_line(m: &Media, episode: i32, airing_at: i64) -> String {
    format!(
        "• **{}** — {} {episode} · {}",
        m.title.main(),
        i18n::F_EPISODES,
        ts_relative(airing_at)
    )
}

/// Post the releases; returns those that were delivered (so only they are marked as posted).
async fn send_batch<'a>(
    f: &Feed,
    channel: serenity::ChannelId,
    cfg: &FeedConfig,
    batch: &[&'a AiringEntry],
) -> Vec<&'a AiringEntry> {
    let mut done = Vec::new();
    if batch.len() <= MAX_INDIVIDUAL {
        for e in batch {
            if send_episode(f, channel, cfg, e).await {
                done.push(*e);
            }
        }
        return done;
    }
    for chunk in batch.chunks(LINES_PER_MESSAGE) {
        let body = chunk
            .iter()
            .map(|e| compact_line(&e.media, e.episode, e.airing_at))
            .collect::<Vec<_>>()
            .join("\n");
        let mut msg = serenity::CreateMessage::new().embed(
            Emb::new()
                .title(i18n::FEED_BATCH_TITLE)
                .description(body)
                .build(),
        );
        // One menu with the works of this message: each choice sends the work card as a new message.
        let items: Vec<(String, String)> = chunk
            .iter()
            .map(|e| {
                (
                    crate::ui::embeds::fit(e.media.title.main(), 100),
                    format!("anilist:{}", e.media.id),
                )
            })
            .collect();
        if let Some(menu) = crate::components::work_menu(i18n::BTN_SHOW_WORK, &items) {
            msg = msg.components(vec![menu]);
        }
        if let Some(role) = cfg.role_id {
            msg = msg.content(format!("<@&{role}>")).allowed_mentions(
                serenity::CreateAllowedMentions::new()
                    .roles(vec![serenity::RoleId::new(role as u64)]),
            );
        }
        match channel.send_message(&f.http, msg).await {
            Ok(_) => done.extend(chunk.iter().copied()),
            Err(e) => tracing::debug!("could not post a grouped release message: {e}"),
        }
    }
    done
}

async fn send_episode(
    f: &Feed,
    channel: serenity::ChannelId,
    cfg: &FeedConfig,
    e: &AiringEntry,
) -> bool {
    let m = &e.media;
    let mut links = notification_buttons(m.id);
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

#[cfg(test)]
mod tests;
