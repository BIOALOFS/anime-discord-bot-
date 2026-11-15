//! Favourite works: anime, manga, manhwa, manhua and novels in a profile (provider-prefixed text ids).
//! (Data-layer functions for groups, tiers, notes, lists and compatibility stay with the schema, even though
//! no command uses them any more.)
#![allow(dead_code)]

use super::profile::{
    cap, require_profile, touch, AddOutcome, ProfileError, Result, MAX_NOTE, MAX_TIER,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

pub const LISTS: [&str; 2] = ["watched", "planning"];

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)] // columns are mapped as a whole; some are only read in tests
pub struct WorkEntry {
    pub work_id: String,
    pub kind: String,
    pub position: i64,
    pub tier: Option<String>,
    pub note: Option<String>,
    pub list: Option<String>,
    pub title_romaji: Option<String>,
    pub title_english: Option<String>,
    pub cover_url: Option<String>,
    pub format: Option<String>,
    pub country: Option<String>,
    pub year: Option<i64>,
    pub score: Option<i64>,
    pub status: Option<String>,
    pub is_safe: Option<bool>,
    pub source: Option<String>,
    pub fetched_at: Option<i64>,
}

impl WorkEntry {
    /// Fail-closed, like characters: only verified-safe works are displayed.
    pub fn visible(&self) -> bool {
        self.is_safe == Some(true)
    }

    pub fn title(&self) -> &str {
        self.title_romaji.as_deref().unwrap_or("?")
    }

    /// `anilist:123` -> 123 for AniList lookups.
    pub fn anilist_id(&self) -> Option<i64> {
        anilist_id(&self.work_id)
    }
}

pub fn anilist_id(work_id: &str) -> Option<i64> {
    work_id.strip_prefix("anilist:")?.parse().ok()
}

pub fn work_key(anilist_id: i64) -> String {
    format!("anilist:{anilist_id}")
}

#[derive(Debug, Clone)]
pub struct MediaCacheRow {
    pub work_id: String,
    pub title_romaji: String,
    pub title_english: Option<String>,
    pub cover_url: Option<String>,
    pub kind: String,
    pub format: Option<String>,
    pub country: Option<String>,
    pub year: Option<i64>,
    pub score: Option<i64>,
    pub status: Option<String>,
    pub is_safe: bool,
    pub source: String,
    pub fetched_at: i64,
}

const SELECT: &str = "SELECT pw.work_id, pw.kind, pw.position, pw.tier, pw.note, pw.list, \
    mc.title_romaji, mc.title_english, mc.cover_url, mc.format, mc.country, mc.year, mc.score, mc.status, \
    mc.is_safe, mc.source, mc.fetched_at \
    FROM profile_works pw LEFT JOIN media_cache mc ON mc.work_id = pw.work_id";

pub async fn upsert_media_cache(pool: &SqlitePool, r: &MediaCacheRow) -> Result<()> {
    sqlx::query(
        "INSERT INTO media_cache (work_id, title_romaji, title_english, cover_url, kind, format, country, year, \
         score, status, is_safe, source, fetched_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13) \
         ON CONFLICT(work_id) DO UPDATE SET title_romaji=excluded.title_romaji, title_english=excluded.title_english, \
         cover_url=excluded.cover_url, kind=excluded.kind, format=excluded.format, country=excluded.country, \
         year=excluded.year, score=excluded.score, status=excluded.status, is_safe=excluded.is_safe, \
         source=excluded.source, fetched_at=excluded.fetched_at",
    )
    .bind(&r.work_id)
    .bind(&r.title_romaji)
    .bind(&r.title_english)
    .bind(&r.cover_url)
    .bind(&r.kind)
    .bind(&r.format)
    .bind(&r.country)
    .bind(r.year)
    .bind(r.score)
    .bind(&r.status)
    .bind(r.is_safe)
    .bind(&r.source)
    .bind(r.fetched_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn count(pool: &SqlitePool, user: i64) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT count(*) FROM profile_works WHERE user_id = ?1")
            .bind(user)
            .fetch_one(pool)
            .await?,
    )
}

pub async fn count_filtered(
    pool: &SqlitePool,
    user: i64,
    group: Option<i64>,
    list: Option<&str>,
) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM profile_works WHERE user_id = ?1 AND (?2 IS NULL OR group_id = ?2) AND (?3 IS NULL OR list = ?3)",
    )
    .bind(user)
    .bind(group)
    .bind(list)
    .fetch_one(pool)
    .await?)
}

pub async fn has_work(pool: &SqlitePool, user: i64, work_id: &str) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM profile_works WHERE user_id = ?1 AND work_id = ?2",
    )
    .bind(user)
    .bind(work_id)
    .fetch_one(pool)
    .await?
        > 0)
}

pub async fn add_work(
    pool: &SqlitePool,
    user: i64,
    work_id: &str,
    kind: &str,
) -> Result<AddOutcome> {
    require_profile(pool, user).await?;
    let mut tx = pool.begin().await?;
    let next: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(position), 0) + 1 FROM profile_works WHERE user_id = ?1",
    )
    .bind(user)
    .fetch_one(&mut *tx)
    .await?;
    let r = sqlx::query("INSERT OR IGNORE INTO profile_works (user_id, work_id, kind, position) VALUES (?1, ?2, ?3, ?4)")
        .bind(user)
        .bind(work_id)
        .bind(kind)
        .bind(next)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    if r.rows_affected() == 0 {
        return Ok(AddOutcome::Duplicate);
    }
    touch(pool, user).await?;
    Ok(AddOutcome::Added(next))
}

pub async fn remove_work(pool: &SqlitePool, user: i64, work_id: &str) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let pos: Option<i64> = sqlx::query_scalar(
        "SELECT position FROM profile_works WHERE user_id = ?1 AND work_id = ?2",
    )
    .bind(user)
    .bind(work_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(pos) = pos else { return Ok(false) };
    sqlx::query("DELETE FROM profile_works WHERE user_id = ?1 AND work_id = ?2")
        .bind(user)
        .bind(work_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE profile_works SET position = position - 1 WHERE user_id = ?1 AND position > ?2",
    )
    .bind(user)
    .bind(pos)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    touch(pool, user).await?;
    Ok(true)
}

pub async fn clear(pool: &SqlitePool, user: i64) -> Result<u64> {
    let r = sqlx::query("DELETE FROM profile_works WHERE user_id = ?1")
        .bind(user)
        .execute(pool)
        .await?;
    touch(pool, user).await?;
    Ok(r.rows_affected())
}

pub async fn move_work(pool: &SqlitePool, user: i64, work_id: &str, target: i64) -> Result<i64> {
    let mut tx = pool.begin().await?;
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM profile_works WHERE user_id = ?1")
        .bind(user)
        .fetch_one(&mut *tx)
        .await?;
    let from: i64 = sqlx::query_scalar(
        "SELECT position FROM profile_works WHERE user_id = ?1 AND work_id = ?2",
    )
    .bind(user)
    .bind(work_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ProfileError::NotFound)?;
    let to = target.clamp(1, total.max(1));
    if to < from {
        sqlx::query("UPDATE profile_works SET position = position + 1 WHERE user_id = ?1 AND position >= ?2 AND position < ?3")
            .bind(user)
            .bind(to)
            .bind(from)
            .execute(&mut *tx)
            .await?;
    } else if to > from {
        sqlx::query("UPDATE profile_works SET position = position - 1 WHERE user_id = ?1 AND position > ?2 AND position <= ?3")
            .bind(user)
            .bind(from)
            .bind(to)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE profile_works SET position = ?3 WHERE user_id = ?1 AND work_id = ?2")
        .bind(user)
        .bind(work_id)
        .bind(to)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    touch(pool, user).await?;
    Ok(to)
}

pub async fn position_of(pool: &SqlitePool, user: i64, work_id: &str) -> Result<Option<i64>> {
    Ok(
        sqlx::query_scalar(
            "SELECT position FROM profile_works WHERE user_id = ?1 AND work_id = ?2",
        )
        .bind(user)
        .bind(work_id)
        .fetch_optional(pool)
        .await?,
    )
}

pub async fn list_page(
    pool: &SqlitePool,
    user: i64,
    group: Option<i64>,
    list: Option<&str>,
    offset: i64,
    limit: i64,
) -> Result<Vec<WorkEntry>> {
    let sql = format!(
        "{SELECT} WHERE pw.user_id = ?1 AND (?2 IS NULL OR pw.group_id = ?2) AND (?3 IS NULL OR pw.list = ?3) \
         ORDER BY pw.position LIMIT ?4 OFFSET ?5"
    );
    Ok(sqlx::query_as::<_, WorkEntry>(&sql)
        .bind(user)
        .bind(group)
        .bind(list)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?)
}

pub async fn get_entry(pool: &SqlitePool, user: i64, work_id: &str) -> Result<Option<WorkEntry>> {
    let sql = format!("{SELECT} WHERE pw.user_id = ?1 AND pw.work_id = ?2");
    Ok(sqlx::query_as::<_, WorkEntry>(&sql)
        .bind(user)
        .bind(work_id)
        .fetch_optional(pool)
        .await?)
}

pub async fn tiered(pool: &SqlitePool, user: i64) -> Result<Vec<WorkEntry>> {
    let sql =
        format!("{SELECT} WHERE pw.user_id = ?1 AND pw.tier IS NOT NULL ORDER BY pw.position");
    Ok(sqlx::query_as::<_, WorkEntry>(&sql)
        .bind(user)
        .fetch_all(pool)
        .await?)
}

pub async fn set_tier(
    pool: &SqlitePool,
    user: i64,
    work_id: &str,
    tier: Option<&str>,
) -> Result<()> {
    let tier = tier
        .map(|t| cap(t.trim(), MAX_TIER))
        .filter(|t| !t.is_empty());
    update_one(pool, "tier", user, work_id, tier).await
}

pub async fn set_note(
    pool: &SqlitePool,
    user: i64,
    work_id: &str,
    note: Option<&str>,
) -> Result<()> {
    let note = note
        .map(|t| cap(t.trim(), MAX_NOTE))
        .filter(|t| !t.is_empty());
    update_one(pool, "note", user, work_id, note).await
}

/// `list` must be one of `LISTS` or None.
pub async fn set_list(
    pool: &SqlitePool,
    user: i64,
    work_id: &str,
    list: Option<&str>,
) -> Result<()> {
    if list.is_some_and(|l| !LISTS.contains(&l)) {
        return Err(ProfileError::Invalid("unknown list"));
    }
    update_one(pool, "list", user, work_id, list.map(str::to_string)).await
}

async fn update_one(
    pool: &SqlitePool,
    column: &'static str,
    user: i64,
    work_id: &str,
    value: Option<String>,
) -> Result<()> {
    // `column` is one of our own constants, never user input.
    let sql = format!("UPDATE profile_works SET {column} = ?3 WHERE user_id = ?1 AND work_id = ?2");
    let r = sqlx::query(&sql)
        .bind(user)
        .bind(work_id)
        .bind(value)
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ProfileError::NotFound);
    }
    Ok(())
}

pub async fn assign_group(
    pool: &SqlitePool,
    user: i64,
    work_id: &str,
    group: Option<i64>,
) -> Result<()> {
    let r =
        sqlx::query("UPDATE profile_works SET group_id = ?3 WHERE user_id = ?1 AND work_id = ?2")
            .bind(user)
            .bind(work_id)
            .bind(group)
            .execute(pool)
            .await?;
    if r.rows_affected() == 0 {
        return Err(ProfileError::NotFound);
    }
    Ok(())
}

/// Own favourites whose (verified safe) title matches `partial`.
pub async fn search_own(
    pool: &SqlitePool,
    user: i64,
    partial: &str,
    limit: i64,
) -> Result<Vec<(String, String)>> {
    let escaped = partial
        .trim()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let like = format!("%{escaped}%");
    let rows = sqlx::query(
        "SELECT pw.work_id AS id, mc.title_romaji AS title FROM profile_works pw \
         JOIN media_cache mc ON mc.work_id = pw.work_id \
         WHERE pw.user_id = ?1 AND mc.is_safe = 1 \
         AND (mc.title_romaji LIKE ?2 ESCAPE '\\' OR mc.title_english LIKE ?2 ESCAPE '\\') \
         ORDER BY pw.position LIMIT ?3",
    )
    .bind(user)
    .bind(like)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.get("id"), r.get("title")))
        .collect())
}

pub async fn shared_works(pool: &SqlitePool, a: i64, b: i64) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT x.work_id FROM profile_works x JOIN profile_works y ON x.work_id = y.work_id \
         WHERE x.user_id = ?1 AND y.user_id = ?2 ORDER BY x.position",
    )
    .bind(a)
    .bind(b)
    .fetch_all(pool)
    .await?)
}

/// Most favourited works (public/servers profiles only): `(work_id, count)`.
pub async fn top_works(
    pool: &SqlitePool,
    guild: Option<i64>,
    kind: Option<&str>,
    offset: i64,
    limit: i64,
) -> Result<Vec<(String, i64)>> {
    let rows = sqlx::query(
        "SELECT pw.work_id AS id, count(*) AS n FROM profile_works pw \
         JOIN profiles p ON p.user_id = pw.user_id AND p.privacy != 'private' \
         WHERE (?1 IS NULL OR pw.user_id IN (SELECT user_id FROM user_guilds WHERE guild_id = ?1)) \
         AND (?2 IS NULL OR pw.kind = ?2) \
         GROUP BY pw.work_id ORDER BY n DESC, pw.work_id LIMIT ?3 OFFSET ?4",
    )
    .bind(guild)
    .bind(kind)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.get("id"), r.get("n")))
        .collect())
}

pub async fn top_work_count(
    pool: &SqlitePool,
    guild: Option<i64>,
    kind: Option<&str>,
) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(DISTINCT pw.work_id) FROM profile_works pw \
         JOIN profiles p ON p.user_id = pw.user_id AND p.privacy != 'private' \
         WHERE (?1 IS NULL OR pw.user_id IN (SELECT user_id FROM user_guilds WHERE guild_id = ?1)) \
         AND (?2 IS NULL OR pw.kind = ?2)",
    )
    .bind(guild)
    .bind(kind)
    .fetch_one(pool)
    .await?)
}

pub async fn media_cache_get(pool: &SqlitePool, work_id: &str) -> Result<Option<MediaCacheRow>> {
    let r = sqlx::query(
        "SELECT work_id, title_romaji, title_english, cover_url, kind, format, country, year, score, status, is_safe, source, fetched_at \
         FROM media_cache WHERE work_id = ?1",
    )
    .bind(work_id)
    .fetch_optional(pool)
    .await?;
    Ok(r.map(|r| MediaCacheRow {
        work_id: r.get("work_id"),
        title_romaji: r.get("title_romaji"),
        title_english: r.get("title_english"),
        cover_url: r.get("cover_url"),
        kind: r.get("kind"),
        format: r.get("format"),
        country: r.get("country"),
        year: r.get("year"),
        score: r.get("score"),
        status: r.get("status"),
        is_safe: r.get("is_safe"),
        source: r.get("source"),
        fetched_at: r.get("fetched_at"),
    }))
}

// ---- export / import --------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExportWork {
    pub id: String,
    pub kind: String,
    pub group: Option<String>,
    pub tier: Option<String>,
    pub note: Option<String>,
    pub list: Option<String>,
}

pub async fn export_works(
    pool: &SqlitePool,
    user: i64,
    groups: &[super::profile::Group],
) -> Result<Vec<ExportWork>> {
    let rows = sqlx::query("SELECT work_id, kind, group_id, tier, note, list FROM profile_works WHERE user_id = ?1 ORDER BY position")
        .bind(user)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let gid: Option<i64> = r.get("group_id");
            ExportWork {
                id: r.get("work_id"),
                kind: r.get("kind"),
                group: gid
                    .and_then(|g| groups.iter().find(|x| x.id == g))
                    .map(|g| g.name.clone()),
                tier: r.get("tier"),
                note: r.get("note"),
                list: r.get("list"),
            }
        })
        .collect())
}

/// Returns (added, duplicates, invalid). Works start hidden until verified (no cache row).
pub async fn import_works(
    pool: &SqlitePool,
    user: i64,
    works: &[ExportWork],
) -> Result<(usize, usize, usize)> {
    let (mut added, mut dups, mut invalid) = (0, 0, 0);
    for w in works {
        if anilist_id(&w.id).is_none()
            || !["anime", "manga", "manhwa", "manhua", "novel"].contains(&w.kind.as_str())
        {
            invalid += 1;
            continue;
        }
        match add_work(pool, user, &w.id, &w.kind).await? {
            AddOutcome::Duplicate => dups += 1,
            AddOutcome::Added(_) => {
                added += 1;
                if let Some(t) = &w.tier {
                    set_tier(pool, user, &w.id, Some(t)).await?;
                }
                if let Some(n) = &w.note {
                    set_note(pool, user, &w.id, Some(n)).await?;
                }
                if let Some(l) = w.list.as_deref().filter(|l| LISTS.contains(l)) {
                    set_list(pool, user, &w.id, Some(l)).await?;
                }
                if let Some(name) = w.group.as_deref() {
                    if let Some(g) = super::profile::find_group(pool, user, name).await? {
                        assign_group(pool, user, &w.id, Some(g.id)).await?;
                    }
                }
            }
        }
    }
    Ok((added, dups, invalid))
}

#[cfg(test)]
mod tests;
