//! The background pass of the optional features: daily posts, the weekly poll (start and close) and watch-night
//! reminders. Called from the feed tick; every step is claimed first, so nothing is posted twice.

use super::{daily, poll, settings, watch};
use crate::{db::feed as fdb, feed::Feed};
use poise::serenity_prelude as serenity;

async fn tz_of(f: &Feed, guild: i64) -> chrono_tz::Tz {
    crate::db::admin::timezone(&f.pool, guild)
        .await
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or(chrono_tz::Asia::Muscat)
}

async fn notify_channel(f: &Feed, guild: i64) -> Option<i64> {
    let cfg = fdb::get_config(&f.pool, guild).await.ok()?;
    cfg.enabled.then_some(cfg.channel_id).flatten()
}

fn channel(id: i64) -> serenity::ChannelId {
    serenity::ChannelId::new(id as u64)
}

/// Start this week's poll in every server that switched it on and has enough suggestions.
async fn start_polls(f: &Feed, now: i64) -> anyhow::Result<()> {
    for (guild, feat) in settings::enabled_guilds(&f.pool, poll::FEATURE).await? {
        let (tz, cfg) = (tz_of(f, guild).await, poll::Config::from(&feat));
        if !poll::should_start(now, tz, &cfg) {
            continue;
        }
        let Some(ch) = notify_channel(f, guild).await else {
            continue;
        };
        let week = crate::db::game::week_key(now);
        // Once a week and server, even when there were not enough suggestions this time.
        if !settings::claim(&f.pool, guild, "poll", "week", &week).await? {
            continue;
        }
        let options = poll::pick_options(&f.pool, guild).await?;
        if options.len() < poll::MIN_OPTIONS {
            continue;
        }
        let closes = now + cfg.duration_hours * 3600;
        let id = poll::create(&f.pool, guild, ch, &options, closes).await?;
        let msg = serenity::CreateMessage::new()
            .embed(poll::embed(&options, &vec![0; options.len()], closes, false).build())
            .components(vec![poll::buttons(id, &options, false)]);
        match channel(ch).send_message(&f.http, msg).await {
            Ok(m) => poll::set_message(&f.pool, id, m.id.get() as i64).await?,
            Err(e) => {
                tracing::debug!("could not post the poll: {e}");
                poll::close(&f.pool, id).await?;
            }
        }
    }
    Ok(())
}

/// Close the polls whose time is up: the message shows the final counts and a result message opens the winning pair.
async fn close_polls(f: &Feed, now: i64) -> anyhow::Result<()> {
    for id in poll::due_to_close(&f.pool, now).await? {
        if !poll::close(&f.pool, id).await? {
            continue;
        }
        let Some(p) = poll::get(&f.pool, id).await? else {
            continue;
        };
        let counts = poll::counts(&f.pool, &p).await?;
        if p.message != 0 {
            let edit = serenity::EditMessage::new()
                .embed(poll::embed(&p.options, &counts, p.closes_at, true).build())
                .components(vec![poll::buttons(id, &p.options, true)]);
            let _ = channel(p.channel)
                .edit_message(&f.http, serenity::MessageId::new(p.message as u64), edit)
                .await;
        }
        let (e, rows) = poll::result_message(&p.options, &counts);
        let mut msg = serenity::CreateMessage::new().embed(e.build());
        if !rows.is_empty() {
            msg = msg.components(rows);
        }
        if let Err(e) = channel(p.channel).send_message(&f.http, msg).await {
            tracing::debug!("could not post the poll result: {e}");
        }
    }
    Ok(())
}

/// Remind the attendees of nights that just started (a direct message, or one mention in the channel when it is closed).
async fn remind_nights(f: &Feed, now: i64) -> anyhow::Result<()> {
    for n in watch::due_reminders(&f.pool, now).await? {
        if !watch::mark_reminded(&f.pool, n.id).await? {
            continue;
        }
        let mut missed = Vec::new();
        for user in watch::attendees(&f.pool, n.id).await? {
            let dm = async {
                let ch = serenity::UserId::new(user as u64)
                    .create_dm_channel(&f.http)
                    .await?;
                ch.send_message(
                    &f.http,
                    serenity::CreateMessage::new().content(watch::reminder_text(&n)),
                )
                .await
            };
            if dm.await.is_err() {
                missed.push(user);
            }
        }
        if !missed.is_empty() {
            let mentions = missed
                .iter()
                .take(40)
                .map(|u| format!("<@{u}>"))
                .collect::<Vec<_>>()
                .join(" ");
            let users: Vec<serenity::UserId> = missed
                .iter()
                .take(40)
                .map(|u| serenity::UserId::new(*u as u64))
                .collect();
            let msg = serenity::CreateMessage::new()
                .content(format!("{mentions}\n{}", watch::reminder_text(&n)))
                .allowed_mentions(serenity::CreateAllowedMentions::new().users(users));
            let _ = channel(n.channel).send_message(&f.http, msg).await;
        }
    }
    watch::prune(&f.pool, now).await?;
    Ok(())
}

/// The whole background pass. One feature failing never stops the others.
pub async fn tick(f: &Feed, now: i64) {
    for (name, r) in [
        ("daily posts", daily::tick(f, now).await),
        ("poll start", start_polls(f, now).await),
        ("poll close", close_polls(f, now).await),
        ("watch reminders", remind_nights(f, now).await),
    ] {
        if let Err(e) = r {
            tracing::warn!("{name} failed (will retry): {e:#}");
        }
    }
}
