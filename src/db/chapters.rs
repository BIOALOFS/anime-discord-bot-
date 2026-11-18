//! Chapter sources per server, RSS feeds, source id mapping and the per-server dedupe of announced chapters.

use crate::chapters::Tracked;
use sqlx::{FromRow, SqlitePool};
use std::collections::HashSet;

pub type Result<T> = std::result::Result<T, sqlx::Error>;

pub const MAX_FEEDS_PER_GUILD: i64 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub mangadex: bool,
    pub mangaupdates: bool,
    pub rss: bool,
    /// Preferred MangaDex translation language: `ar` or `en`.
    pub lang: String,
}

impl Settings {
    pub fn any(&self) -> bool {
        self.mangadex || self.mangaupdates || self.rss
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mangadex: false,
            mangaupdates: false,
            rss: false,
            lang: "ar".into(),
        }
    }
}

pub async fn settings(pool: &SqlitePool, guild: i64) -> Result<Settings> {
    let row: Option<(bool, bool, bool, String)> = sqlx::query_as(
        "SELECT mangadex, mangaupdates, rss, lang FROM chapter_settings WHERE guild_id = ?1",
    )
    .bind(guild)
    .fetch_optional(pool)
    .await?;
    Ok(
        row.map_or_else(Settings::default, |(mangadex, mangaupdates, rss, lang)| {
            Settings {
                mangadex,
                mangaupdates,
                rss,
                lang,
            }
        }),
    )
}

/// Hours between chapter digests (1, 2 default, 4, 8 or 24).
pub async fn digest_hours(pool: &SqlitePool, guild: i64) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT digest_hours FROM chapter_settings WHERE guild_id = ?1",
    )
    .bind(guild)
    .fetch_optional(pool)
    .await?
    .unwrap_or(2))
}

pub async fn set_digest_hours(pool: &SqlitePool, guild: i64, hours: i64) -> Result<()> {
    if !crate::db::notify::DIGEST_HOURS.contains(&hours) {
        return Ok(());
    }
    ensure(pool, guild).await?;
    sqlx::query("UPDATE chapter_settings SET digest_hours = ?2 WHERE guild_id = ?1")
        .bind(guild)
        .bind(hours)
        .execute(pool)
        .await?;
    Ok(())
}

/// Is it time for this server's next digest? (The first one is always due.)
pub async fn digest_due(pool: &SqlitePool, guild: i64, now: i64) -> Result<bool> {
    let row: Option<(i64, i64)> =
        sqlx::query_as("SELECT digest_hours, digest_at FROM chapter_settings WHERE guild_id = ?1")
            .bind(guild)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_none_or(|(hours, at)| now - at >= hours * 3600 - 60))
}

pub async fn mark_digest(pool: &SqlitePool, guild: i64, now: i64) -> Result<()> {
    ensure(pool, guild).await?;
    sqlx::query("UPDATE chapter_settings SET digest_at = ?2 WHERE guild_id = ?1")
        .bind(guild)
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

async fn ensure(pool: &SqlitePool, guild: i64) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO chapter_settings (guild_id) VALUES (?1)")
        .bind(guild)
        .execute(pool)
        .await?;
    Ok(())
}

/// `source`: `mangadex`, `mangaupdates` or `rss`.
pub async fn set_source(pool: &SqlitePool, guild: i64, source: &str, on: bool) -> Result<()> {
    let column = match source {
        "mangadex" => "mangadex",
        "mangaupdates" => "mangaupdates",
        "rss" => "rss",
        _ => return Ok(()),
    };
    ensure(pool, guild).await?;
    // `column` is one of the constants above, never user input.
    sqlx::query(&format!(
        "UPDATE chapter_settings SET {column} = ?2 WHERE guild_id = ?1"
    ))
    .bind(guild)
    .bind(on)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_lang(pool: &SqlitePool, guild: i64, lang: &str) -> Result<()> {
    if !["ar", "en"].contains(&lang) {
        return Ok(());
    }
    ensure(pool, guild).await?;
    sqlx::query("UPDATE chapter_settings SET lang = ?2 WHERE guild_id = ?1")
        .bind(guild)
        .bind(lang)
        .execute(pool)
        .await?;
    Ok(())
}

/// Servers with at least one source switched on.
pub async fn enabled_guilds(pool: &SqlitePool) -> Result<Vec<(i64, Settings)>> {
    let rows: Vec<(i64, bool, bool, bool, String)> =
        sqlx::query_as("SELECT guild_id, mangadex, mangaupdates, rss, lang FROM chapter_settings WHERE mangadex = 1 OR mangaupdates = 1 OR rss = 1").fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(|(g, mangadex, mangaupdates, rss, lang)| {
            (
                g,
                Settings {
                    mangadex,
                    mangaupdates,
                    rss,
                    lang,
                },
            )
        })
        .collect())
}

#[derive(Debug, Clone, FromRow, PartialEq, Eq)]
pub struct Feed {
    pub id: i64,
    pub url: String,
    pub title_match: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddFeed {
    Added,
    Duplicate,
    Full,
}

pub async fn add_feed(
    pool: &SqlitePool,
    guild: i64,
    url: &str,
    title_match: &str,
) -> Result<AddFeed> {
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM rss_feeds WHERE guild_id = ?1")
        .bind(guild)
        .fetch_one(pool)
        .await?;
    if n >= MAX_FEEDS_PER_GUILD {
        return Ok(AddFeed::Full);
    }
    let r = sqlx::query(
        "INSERT OR IGNORE INTO rss_feeds (guild_id, url, title_match) VALUES (?1, ?2, ?3)",
    )
    .bind(guild)
    .bind(url)
    .bind(title_match)
    .execute(pool)
    .await?;
    Ok(if r.rows_affected() > 0 {
        AddFeed::Added
    } else {
        AddFeed::Duplicate
    })
}

pub async fn feeds(pool: &SqlitePool, guild: i64) -> Result<Vec<Feed>> {
    sqlx::query_as("SELECT id, url, title_match FROM rss_feeds WHERE guild_id = ?1 ORDER BY id")
        .bind(guild)
        .fetch_all(pool)
        .await
}

pub async fn remove_feed(pool: &SqlitePool, guild: i64, id: i64) -> Result<bool> {
    Ok(
        sqlx::query("DELETE FROM rss_feeds WHERE guild_id = ?1 AND id = ?2")
            .bind(guild)
            .bind(id)
            .execute(pool)
            .await?
            .rows_affected()
            > 0,
    )
}

pub async fn mapped_id(pool: &SqlitePool, work: i64, source: &str) -> Result<Option<String>> {
    sqlx::query_scalar("SELECT source_id FROM chapter_map WHERE work_id = ?1 AND source = ?2")
        .bind(work)
        .bind(source)
        .fetch_optional(pool)
        .await
}

pub async fn set_mapped_id(pool: &SqlitePool, work: i64, source: &str, id: &str) -> Result<()> {
    sqlx::query("INSERT INTO chapter_map (work_id, source, source_id) VALUES (?1, ?2, ?3) ON CONFLICT(work_id, source) DO UPDATE SET source_id = excluded.source_id")
        .bind(work)
        .bind(source)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Keys already announced (or recorded) for a work in a server.
pub async fn posted_keys(pool: &SqlitePool, guild: i64, work: i64) -> Result<HashSet<String>> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT chapter_key FROM chapter_posted WHERE guild_id = ?1 AND work_id = ?2",
    )
    .bind(guild)
    .bind(work)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

pub async fn mark_posted(
    pool: &SqlitePool,
    guild: i64,
    work: i64,
    key: &str,
    now: i64,
) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO chapter_posted (guild_id, work_id, chapter_key, posted_at) VALUES (?1, ?2, ?3, ?4)").bind(guild).bind(work).bind(key).bind(now).execute(pool).await?;
    Ok(())
}

/// Works tracked in a server: manga, manhwa and manhua that a member of the server has in their profile, or has a
/// reminder on (when the reminder's work is known to be a manga kind). Novels have no chapter feeds here.
pub async fn tracked_works(pool: &SqlitePool, guild: i64, limit: i64) -> Result<Vec<Tracked>> {
    let rows: Vec<(i64, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT w, MAX(romaji), MAX(english) FROM ( \
            SELECT CAST(substr(pw.work_id, 9) AS INTEGER) AS w, mc.title_romaji AS romaji, mc.title_english AS english \
              FROM profile_works pw JOIN user_guilds ug ON ug.user_id = pw.user_id AND ug.guild_id = ?1 \
              LEFT JOIN media_cache mc ON mc.work_id = pw.work_id \
             WHERE pw.work_id LIKE 'anilist:%' AND pw.kind IN ('manga', 'manhwa', 'manhua') \
            UNION ALL \
            SELECT r.work_id, mc.title_romaji, mc.title_english \
              FROM reminders r JOIN user_guilds ug ON ug.user_id = r.user_id AND ug.guild_id = ?1 \
              JOIN media_cache mc ON mc.work_id = 'anilist:' || r.work_id AND mc.kind IN ('manga', 'manhwa', 'manhua') \
        ) GROUP BY w ORDER BY w LIMIT ?2",
    )
    .bind(guild)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(work_id, romaji, english)| {
            let mut titles: Vec<String> = [romaji, english]
                .into_iter()
                .flatten()
                .filter(|t| !t.trim().is_empty() && t != "?")
                .collect();
            titles.dedup();
            Tracked { work_id, titles }
        })
        .filter(|t| !t.titles.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{connect_memory, profile, reminders, works};

    async fn seed_work(pool: &SqlitePool, user: i64, id: i64, kind: &str, title: &str) {
        profile::create_profile(pool, user).await.expect("profile");
        works::upsert_media_cache(
            pool,
            &works::MediaCacheRow {
                work_id: format!("anilist:{id}"),
                title_romaji: title.into(),
                title_english: None,
                cover_url: None,
                kind: kind.into(),
                format: None,
                country: None,
                year: None,
                score: None,
                status: None,
                is_safe: true,
                source: "anilist".into(),
                fetched_at: 0,
            },
        )
        .await
        .expect("cache");
        works::add_work(pool, user, &format!("anilist:{id}"), kind)
            .await
            .expect("add");
    }

    async fn member(pool: &SqlitePool, guild: i64, user: i64) {
        sqlx::query("INSERT OR IGNORE INTO user_guilds (guild_id, user_id) VALUES (?1, ?2)")
            .bind(guild)
            .bind(user)
            .execute(pool)
            .await
            .expect("member");
    }

    #[tokio::test]
    async fn only_tracked_manga_of_this_servers_members_are_listed() {
        let pool = connect_memory().await;
        seed_work(&pool, 1, 100, "manga", "Solo Leveling").await;
        seed_work(&pool, 1, 101, "anime", "Some Anime").await;
        seed_work(&pool, 1, 102, "novel", "A Novel").await;
        seed_work(&pool, 2, 103, "manhwa", "Outsider Work").await;
        member(&pool, 10, 1).await;
        member(&pool, 20, 2).await;
        let t = tracked_works(&pool, 10, 50).await.expect("tracked");
        assert_eq!(
            t.iter().map(|w| w.work_id).collect::<Vec<_>>(),
            vec![100],
            "profile manga only; no anime, novel or other server"
        );
        assert_eq!(t[0].titles, vec!["Solo Leveling"]);
        // A reminder on a manga also tracks it (the member never added it to the profile).
        seed_work(&pool, 3, 104, "manhua", "Reminded").await;
        works::remove_work(&pool, 3, "anilist:104")
            .await
            .expect("remove");
        reminders::add(&pool, 1, 104, "Reminded")
            .await
            .expect("reminder");
        let t = tracked_works(&pool, 10, 50).await.expect("tracked");
        assert_eq!(
            t.iter().map(|w| w.work_id).collect::<Vec<_>>(),
            vec![100, 104]
        );
        assert!(
            tracked_works(&pool, 30, 50)
                .await
                .expect("tracked")
                .is_empty(),
            "a server without members tracks nothing"
        );
        assert_eq!(tracked_works(&pool, 10, 1).await.expect("limited").len(), 1);
    }

    #[tokio::test]
    async fn digests_are_due_after_their_interval_and_only_valid_hours_are_stored() {
        let pool = connect_memory().await;
        assert_eq!(
            digest_hours(&pool, 1).await.expect("h"),
            2,
            "two hours by default"
        );
        assert!(
            digest_due(&pool, 1, 1_000).await.expect("due"),
            "the first digest is always due"
        );
        mark_digest(&pool, 1, 10_000).await.expect("mark");
        assert!(!digest_due(&pool, 1, 10_000 + 3_000).await.expect("due"));
        assert!(digest_due(&pool, 1, 10_000 + 2 * 3600).await.expect("due"));
        set_digest_hours(&pool, 1, 8).await.expect("set");
        set_digest_hours(&pool, 1, 5).await.expect("ignored");
        assert_eq!(digest_hours(&pool, 1).await.expect("h"), 8);
        assert!(
            !digest_due(&pool, 1, 10_000 + 2 * 3600).await.expect("due"),
            "the new interval applies"
        );
        assert!(digest_due(&pool, 1, 10_000 + 8 * 3600).await.expect("due"));
    }

    #[tokio::test]
    async fn settings_default_off_and_round_trip() {
        let pool = connect_memory().await;
        assert_eq!(settings(&pool, 1).await.expect("s"), Settings::default());
        assert!(!settings(&pool, 1).await.expect("s").any());
        set_source(&pool, 1, "mangadex", true).await.expect("on");
        set_source(&pool, 1, "bogus", true).await.expect("ignored");
        set_lang(&pool, 1, "en").await.expect("lang");
        set_lang(&pool, 1, "fr").await.expect("ignored");
        let s = settings(&pool, 1).await.expect("s");
        assert_eq!(
            (s.mangadex, s.mangaupdates, s.rss, s.lang.as_str()),
            (true, false, false, "en")
        );
        assert_eq!(enabled_guilds(&pool).await.expect("g").len(), 1);
        set_source(&pool, 1, "mangadex", false).await.expect("off");
        assert!(enabled_guilds(&pool).await.expect("g").is_empty());
    }

    #[tokio::test]
    async fn feeds_are_capped_deduplicated_and_removable() {
        let pool = connect_memory().await;
        assert_eq!(
            add_feed(&pool, 1, "https://a.example/f", "Solo")
                .await
                .expect("a"),
            AddFeed::Added
        );
        assert_eq!(
            add_feed(&pool, 1, "https://a.example/f", "Solo")
                .await
                .expect("a"),
            AddFeed::Duplicate
        );
        for i in 0..(MAX_FEEDS_PER_GUILD - 1) {
            assert_eq!(
                add_feed(&pool, 1, &format!("https://b.example/{i}"), "X")
                    .await
                    .expect("b"),
                AddFeed::Added
            );
        }
        assert_eq!(
            add_feed(&pool, 1, "https://c.example/f", "Y")
                .await
                .expect("c"),
            AddFeed::Full
        );
        assert_eq!(
            add_feed(&pool, 2, "https://c.example/f", "Y")
                .await
                .expect("other server"),
            AddFeed::Added
        );
        let list = feeds(&pool, 1).await.expect("list");
        assert!(
            remove_feed(&pool, 1, list[0].id).await.expect("rm")
                && !remove_feed(&pool, 1, list[0].id).await.expect("again")
        );
        assert!(
            !remove_feed(&pool, 2, list[1].id)
                .await
                .expect("other server's"),
            "feeds are per server"
        );
    }

    #[tokio::test]
    async fn announced_chapters_and_source_ids_are_remembered() {
        let pool = connect_memory().await;
        assert!(posted_keys(&pool, 1, 5).await.expect("k").is_empty());
        mark_posted(&pool, 1, 5, "12", 100).await.expect("m");
        mark_posted(&pool, 1, 5, "12", 200).await.expect("again");
        assert_eq!(posted_keys(&pool, 1, 5).await.expect("k").len(), 1);
        assert!(
            posted_keys(&pool, 2, 5).await.expect("k").is_empty(),
            "per server"
        );
        assert_eq!(mapped_id(&pool, 5, "mangadex").await.expect("m"), None);
        set_mapped_id(&pool, 5, "mangadex", "abc")
            .await
            .expect("set");
        set_mapped_id(&pool, 5, "mangadex", "def")
            .await
            .expect("replace");
        assert_eq!(
            mapped_id(&pool, 5, "mangadex").await.expect("m").as_deref(),
            Some("def")
        );
    }
}
