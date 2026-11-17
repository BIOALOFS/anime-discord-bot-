//! The chapter poller: every 15 minutes, for each server with a source switched on, the tracked works are checked
//! (batched, cached, per-source back-off) and new chapters are announced like episodes, with the "show the work"
//! button and a "Read" link. Reminders on the work are delivered once per chapter.

use super::{
    mangadex::MangaDex, mangaupdates::MangaUpdates, only_language, ordinal, plan, rss, ssrf,
    title_matches, Chapter, ChapterSource, SourceKind, Tracked, MAX_WORKS_PER_POLL,
};
use crate::{
    db::{chapters as cdb, feed as fdb, reminders},
    feed::{notify_user, Feed},
    i18n,
    ui::embeds::{fit, Emb},
};
use moka::future::Cache;
use poise::serenity_prelude as serenity;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub const INTERVAL_SECS: i64 = 15 * 60;
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
/// Chapter announcements per work and poll (a long outage must not become a flood).
const MAX_PER_WORK: usize = 5;

pub struct Sources {
    mangadex: MangaDex,
    mangaupdates: MangaUpdates,
    cache: Cache<String, Arc<Vec<Chapter>>>,
    backoff: Mutex<HashMap<SourceKind, (u32, Instant)>>,
}

/// Seconds to stay away from a source after its `fails`-th failure in a row: 1, 2, 4, ... minutes, capped at 30.
pub fn backoff_secs(fails: u32) -> u64 {
    (60u64 << fails.saturating_sub(1).min(5)).min(30 * 60)
}

impl Sources {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            mangadex: MangaDex::new(http.clone()),
            mangaupdates: MangaUpdates::new(http),
            cache: Cache::builder()
                .max_capacity(2_000)
                .time_to_live(CACHE_TTL)
                .build(),
            backoff: Mutex::new(HashMap::new()),
        }
    }

    fn usable(&self, kind: SourceKind) -> bool {
        let map = self.backoff.lock().unwrap_or_else(|e| e.into_inner());
        map.get(&kind)
            .is_none_or(|(_, until)| Instant::now() >= *until)
    }

    fn record(&self, kind: SourceKind, ok: bool) {
        let mut map = self.backoff.lock().unwrap_or_else(|e| e.into_inner());
        if ok {
            map.remove(&kind);
        } else {
            let fails = map.get(&kind).map_or(0, |(n, _)| *n) + 1;
            map.insert(
                kind,
                (
                    fails,
                    Instant::now() + Duration::from_secs(backoff_secs(fails)),
                ),
            );
            tracing::warn!(
                source = kind.key(),
                fails,
                "chapter source failed; backing off"
            );
        }
    }

    async fn cached<F, Fut>(&self, key: String, f: F) -> Option<Arc<Vec<Chapter>>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Option<Vec<Chapter>>>,
    {
        if let Some(hit) = self.cache.get(&key).await {
            return Some(hit);
        }
        let out = Arc::new(f().await?);
        self.cache.insert(key, out.clone()).await;
        Some(out)
    }

    async fn fetch_mangadex(
        &self,
        pool: &sqlx::SqlitePool,
        work: &Tracked,
        lang: &str,
    ) -> Vec<Chapter> {
        if !self.usable(SourceKind::MangaDex) {
            return Vec::new();
        }
        let id = match cdb::mapped_id(pool, work.work_id, "mangadex").await {
            Ok(Some(id)) => id,
            _ => match self.mangadex.find_id(work).await {
                Ok(Some(id)) => {
                    let _ = cdb::set_mapped_id(pool, work.work_id, "mangadex", &id).await;
                    id
                }
                Ok(None) => return Vec::new(),
                Err(_) => {
                    self.record(SourceKind::MangaDex, false);
                    return Vec::new();
                }
            },
        };
        let got = self
            .cached(format!("mdex:{id}:{lang}"), || async {
                match self.mangadex.feed(&id, work.work_id, lang).await {
                    Ok(v) => {
                        self.record(SourceKind::MangaDex, true);
                        Some(v)
                    }
                    Err(_) => {
                        self.record(SourceKind::MangaDex, false);
                        None
                    }
                }
            })
            .await;
        got.map(|v| (*v).clone()).unwrap_or_default()
    }

    async fn fetch_mangaupdates(&self, work: &Tracked, lang: &str) -> Vec<Chapter> {
        if !self.usable(SourceKind::MangaUpdates) {
            return Vec::new();
        }
        let got = self
            .cached(format!("mu:{}", work.work_id), || async {
                match self.mangaupdates.fetch(work, lang).await {
                    Ok(v) => {
                        self.record(SourceKind::MangaUpdates, true);
                        Some(v)
                    }
                    Err(_) => {
                        self.record(SourceKind::MangaUpdates, false);
                        None
                    }
                }
            })
            .await;
        got.map(|v| (*v).clone()).unwrap_or_default()
    }

    async fn fetch_feed(&self, feed: &cdb::Feed, work: &Tracked) -> Vec<Chapter> {
        let got = self
            .cached(format!("rss:{}", feed.url), || async {
                let body = ssrf::fetch_feed(&feed.url).await?;
                Some(rss::to_chapters(&rss::parse_entries(&body), work.work_id))
            })
            .await;
        // Cached entries belong to whichever work asked first; re-label them for this work.
        got.map(|v| {
            v.iter()
                .cloned()
                .map(|c| Chapter {
                    work_id: work.work_id,
                    ..c
                })
                .collect()
        })
        .unwrap_or_default()
    }
}

/// The notification for one new chapter.
pub fn chapter_embed(work_title: &str, c: &Chapter) -> Emb {
    let mut text = match &c.number {
        Some(n) => format!("{} **{n}**", i18n::CHAPTER_WORD),
        None => i18n::CHAPTER_NEW_UNNUMBERED.to_string(),
    };
    if let Some(t) = c.title.as_deref().filter(|t| !t.is_empty()) {
        text.push_str(&format!(" — {}", fit(t, 120)));
    }
    if let Some(l) = &c.lang {
        text.push_str(&format!(
            "\n{} {}",
            i18n::CHAPTER_LANGUAGE,
            if l == "ar" {
                "العربية"
            } else {
                l.as_str()
            }
        ));
    }
    let mut e = Emb::new()
        .title(format!("📖 {} {}", i18n::CHAPTER_NEW, fit(work_title, 200)))
        .description(text);
    e.footer = Some(format!("{} {}", i18n::CHAPTER_SOURCE, c.source.credit()));
    e
}

pub async fn poll(f: &Feed, now: i64) -> anyhow::Result<()> {
    let guilds = cdb::enabled_guilds(&f.pool).await?;
    let mut announced: HashMap<(i64, String), (Chapter, String)> = HashMap::new();
    for (guild, settings) in guilds {
        if let Err(e) = poll_guild(f, guild, &settings, now, &mut announced).await {
            tracing::warn!(guild, "chapter poll for guild failed: {e:#}");
        }
    }
    remind(f, announced).await;
    Ok(())
}

async fn poll_guild(
    f: &Feed,
    guild: i64,
    settings: &cdb::Settings,
    now: i64,
    announced: &mut HashMap<(i64, String), (Chapter, String)>,
) -> anyhow::Result<()> {
    if !settings.any() {
        return Ok(());
    }
    let cfg = fdb::get_config(&f.pool, guild).await?;
    // Chapters go to the notifications channel; without one (or switched off) there is nowhere to post.
    let (true, Some(channel)) = (cfg.enabled, cfg.channel_id) else {
        return Ok(());
    };
    let channel = serenity::ChannelId::new(channel as u64);
    // One digest every few hours (2 by default): between digests nothing is marked as posted, so chapters wait.
    if !cdb::digest_due(&f.pool, guild, now).await? {
        return Ok(());
    }
    let mut works = cdb::tracked_works(&f.pool, guild, 1_000).await?;
    if works.len() > MAX_WORKS_PER_POLL {
        // Rotate through the list so every work is checked over a few polls.
        let rounds = works.len().div_ceil(MAX_WORKS_PER_POLL) as i64;
        let start = ((now / INTERVAL_SECS) % rounds) as usize * MAX_WORKS_PER_POLL;
        works = works
            .into_iter()
            .skip(start)
            .take(MAX_WORKS_PER_POLL)
            .collect();
    }
    let feeds = if settings.rss {
        cdb::feeds(&f.pool, guild).await?
    } else {
        Vec::new()
    };
    let mut pending: Vec<(i64, Chapter, String)> = Vec::new();
    for work in works {
        let mut all: Vec<Chapter> = Vec::new();
        // Priority order: MangaDex, MangaUpdates, then the admin's feeds.
        for kind in SourceKind::ORDER {
            match kind {
                SourceKind::MangaDex if settings.mangadex => all.extend(only_language(
                    f.chapters
                        .fetch_mangadex(&f.pool, &work, &settings.lang)
                        .await,
                    &settings.lang,
                )),
                SourceKind::MangaUpdates if settings.mangaupdates => {
                    all.extend(f.chapters.fetch_mangaupdates(&work, &settings.lang).await)
                }
                SourceKind::Rss if settings.rss => {
                    for feed in feeds
                        .iter()
                        .filter(|fd| title_matches(&fd.title_match, &work))
                    {
                        all.extend(f.chapters.fetch_feed(feed, &work).await);
                    }
                }
                _ => {}
            }
        }
        if all.is_empty() {
            continue;
        }
        let posted = cdb::posted_keys(&f.pool, guild, work.work_id).await?;
        let p = plan(all, &posted, now);
        for c in &p.record_only {
            cdb::mark_posted(&f.pool, guild, work.work_id, &c.key, now).await?;
        }
        let title = work.titles.first().cloned().unwrap_or_default();
        for c in p.announce.into_iter().rev().take(MAX_PER_WORK).rev() {
            pending.push((work.work_id, c, title.clone()));
        }
    }
    send_digest(f, guild, channel, cfg.role_id, &pending, now, announced).await?;
    Ok(())
}

/// One message with the new chapters (up to 15 lines, the rest behind a button). Every chapter is marked as posted
/// once the message is out, and member reminders are fed with all of them.
async fn send_digest(
    f: &Feed,
    guild: i64,
    channel: serenity::ChannelId,
    role: Option<i64>,
    pending: &[(i64, Chapter, String)],
    now: i64,
    announced: &mut HashMap<(i64, String), (Chapter, String)>,
) -> anyhow::Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let lines: Vec<crate::chapters::digest::Line> = pending
        .iter()
        .map(|(_, c, title)| crate::chapters::digest::Line {
            work: title.clone(),
            chapter: c.number.as_deref().map_or_else(
                || i18n::CHAPTER_NEW.to_string(),
                |n| format!("{} {n}", i18n::BTN_CHAPTER),
            ),
            url: c.url.clone(),
        })
        .collect();
    let mut msg = serenity::CreateMessage::new()
        .embed(crate::chapters::digest::embed(&lines, lines.len()).build());
    let overflow = lines
        .len()
        .saturating_sub(crate::chapters::digest::PER_DIGEST);
    if overflow > 0 {
        msg = msg.components(vec![serenity::CreateActionRow::Buttons(vec![
            crate::chapters::digest::more_button(overflow),
        ])]);
    }
    if let Some(role) = role {
        msg = msg.content(format!("<@&{role}>")).allowed_mentions(
            serenity::CreateAllowedMentions::new().roles(vec![serenity::RoleId::new(role as u64)]),
        );
    }
    let Ok(sent) = channel.send_message(&f.http, msg).await else {
        // Nothing is marked: the same chapters are tried again at the next poll.
        return Ok(());
    };
    if overflow > 0 {
        crate::chapters::digest::keep_overflow(
            sent.id.get(),
            lines[crate::chapters::digest::PER_DIGEST..].to_vec(),
        )
        .await;
    }
    for (work, c, title) in pending {
        cdb::mark_posted(&f.pool, guild, *work, &c.key, now).await?;
        announced
            .entry((*work, c.key.clone()))
            .or_insert((c.clone(), title.clone()));
    }
    cdb::mark_digest(&f.pool, guild, now).await?;
    Ok(())
}

/// Reminders ("remind me") also cover new chapters of tracked manga: one DM per chapter and member.
async fn remind(f: &Feed, announced: HashMap<(i64, String), (Chapter, String)>) {
    for ((work, _), (c, title)) in announced {
        let Some(ord) = c.number.as_deref().and_then(ordinal) else {
            continue;
        };
        let Ok(by_work) = reminders::for_works(&f.pool, &[work]).await else {
            continue;
        };
        for r in by_work.get(&work).map(Vec::as_slice).unwrap_or(&[]) {
            if !reminders::is_due(ord, r.last_episode) {
                continue;
            }
            // Marked first: this chapter never notifies this member twice.
            if reminders::mark(&f.pool, r.user_id, work, ord)
                .await
                .is_err()
            {
                continue;
            }
            let key = format!("anilist:{work}");
            if !crate::db::notify::allows(&f.pool, r.user_id, &key, "manga", None, &[])
                .await
                .unwrap_or(true)
            {
                continue;
            }
            notify_user(
                f,
                r.user_id,
                i18n::REMINDER_CHAPTER,
                chapter_embed(&title, &c).build(),
            )
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_is_capped() {
        assert_eq!(backoff_secs(1), 60);
        assert_eq!(backoff_secs(2), 120);
        assert_eq!(backoff_secs(3), 240);
        assert_eq!(backoff_secs(6), 1_800);
        assert_eq!(backoff_secs(50), 1_800, "never more than 30 minutes");
    }

    #[test]
    fn a_failing_source_is_skipped_until_the_back_off_ends_and_recovers_on_success() {
        let s = Sources::new(reqwest::Client::new());
        assert!(s.usable(SourceKind::MangaDex));
        s.record(SourceKind::MangaDex, false);
        assert!(!s.usable(SourceKind::MangaDex), "backing off");
        assert!(
            s.usable(SourceKind::MangaUpdates),
            "other sources are unaffected"
        );
        s.record(SourceKind::MangaDex, true);
        assert!(s.usable(SourceKind::MangaDex), "a success clears it");
    }

    #[test]
    fn the_notification_credits_the_source_and_links_only_to_https() {
        let c = Chapter {
            source: SourceKind::MangaDex,
            work_id: 100,
            key: "12".into(),
            number: Some("12".into()),
            title: Some("Return".into()),
            url: "https://mangadex.org/chapter/c3".into(),
            lang: Some("ar".into()),
            published: None,
        };
        let e = chapter_embed("Solo Leveling", &c);
        assert!(e.footer.as_deref().unwrap_or("").contains("MangaDex"));
        let d = e.description.as_deref().unwrap_or("");
        assert!(d.contains("12") && d.contains("Return") && d.contains("العربية"));
        // The digest line carries the "اقرأ" link only for https URLs.
        let line = crate::chapters::digest::line_text(&crate::chapters::digest::Line {
            work: "W".into(),
            chapter: "12".into(),
            url: c.url.clone(),
        });
        assert!(line.contains("https://mangadex.org/chapter/c3"));
        let insecure = crate::chapters::digest::line_text(&crate::chapters::digest::Line {
            work: "W".into(),
            chapter: "12".into(),
            url: "http://x".into(),
        });
        assert!(
            !insecure.contains("http://x"),
            "only https links become links"
        );
    }
}
