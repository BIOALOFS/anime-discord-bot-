//! Profile storage: profiles, favourite characters, groups, tiers, notes, social queries.
//! Only AniList ids plus lightweight cached metadata are stored.
//! (Data-layer functions for groups, tiers, notes, lists and compatibility stay with the schema, even though
//! no command uses them any more.)
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

pub const MAX_BIO: usize = 300;
pub const MAX_TITLE: usize = 40;
pub const MAX_QUOTE: usize = 200;
pub const MAX_NOTE: usize = 300;
pub const MAX_TIER: usize = 30;
pub const MAX_GROUP_NAME: usize = 30;
pub const MAX_IMPORT: usize = 10_000;

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("no profile")]
    NoProfile,
    #[error("already exists")]
    Duplicate,
    #[error("not found")]
    NotFound,
    #[error("invalid input: {0}")]
    Invalid(&'static str),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

pub type Result<T> = std::result::Result<T, ProfileError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privacy {
    Public,
    Servers,
    Private,
}

impl Privacy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Servers => "servers",
            Self::Private => "private",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "public" => Some(Self::Public),
            "servers" => Some(Self::Servers),
            "private" => Some(Self::Private),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Profile {
    pub user_id: i64,
    pub bio: Option<String>,
    pub accent_color: Option<i64>,
    pub quote: Option<String>,
    pub banner: Option<String>,
    pub default_style: String,
    pub privacy: String,
    pub template: String,
    /// The name shown on the card instead of the Discord name (max `MAX_TITLE`).
    pub display_title: Option<String>,
    pub language: String,
    /// Unix time the profile was created.
    pub created_at: i64,
}

impl Profile {
    pub fn privacy(&self) -> Privacy {
        Privacy::parse(&self.privacy).unwrap_or(Privacy::Public)
    }
}

/// A favourite plus whatever metadata is cached for it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Entry {
    pub character_id: i64,
    pub position: i64,
    #[allow(dead_code)] // group filtering by id is done in SQL; kept for the group views
    pub group_id: Option<i64>,
    pub tier: Option<String>,
    pub note: Option<String>,
    pub custom_title: Option<String>,
    pub custom_description: Option<String>,
    pub custom_image_id: Option<i64>,
    pub name_romaji: Option<String>,
    pub name_native: Option<String>,
    pub image_url: Option<String>,
    pub is_safe: Option<bool>,
    pub main_work: Option<String>,
    pub main_work_url: Option<String>,
    pub favourites: Option<i64>,
    pub fetched_at: Option<i64>,
}

impl Entry {
    /// Fail-closed: only characters verified safe are ever displayed.
    pub fn visible(&self) -> bool {
        self.is_safe == Some(true)
    }

    pub fn name(&self) -> &str {
        self.name_romaji.as_deref().unwrap_or("?")
    }
}

#[derive(Debug, Clone)]
pub struct CacheRow {
    pub character_id: i64,
    pub name_romaji: String,
    pub name_native: Option<String>,
    pub image_url: Option<String>,
    pub is_safe: bool,
    pub main_work: Option<String>,
    pub main_work_url: Option<String>,
    pub favourites: Option<i64>,
    pub fetched_at: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Group {
    pub id: i64,
    pub name: String,
    #[allow(dead_code)] // ordering is done in SQL
    pub position: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddOutcome {
    Added(i64),
    Duplicate,
}

const ENTRY_SELECT: &str = "SELECT pc.character_id, pc.position, pc.group_id, pc.tier, pc.note, \
    pc.custom_title, pc.custom_description, pc.custom_image_id, \
    cc.name_romaji, cc.name_native, cc.image_url, cc.is_safe, cc.main_work, cc.main_work_url, \
    cc.favourites, cc.fetched_at \
    FROM profile_characters pc LEFT JOIN character_cache cc ON cc.character_id = pc.character_id";

pub(crate) fn cap(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

// ---- profile ----------------------------------------------------------------------------

/// Idempotent. Returns true when the profile was just created.
pub async fn create_profile(pool: &SqlitePool, user: i64) -> Result<bool> {
    sqlx::query("INSERT OR IGNORE INTO users (discord_id) VALUES (?1)")
        .bind(user)
        .execute(pool)
        .await?;
    let r = sqlx::query("INSERT OR IGNORE INTO profiles (user_id) VALUES (?1)")
        .bind(user)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn get_profile(pool: &SqlitePool, user: i64) -> Result<Option<Profile>> {
    Ok(sqlx::query_as::<_, Profile>(
        "SELECT user_id, bio, accent_color, quote, banner, default_style, privacy, template, display_title, language, created_at \
         FROM profiles WHERE user_id = ?1",
    )
    .bind(user)
    .fetch_optional(pool)
    .await?)
}

pub async fn require_profile(pool: &SqlitePool, user: i64) -> Result<Profile> {
    get_profile(pool, user)
        .await?
        .ok_or(ProfileError::NoProfile)
}

async fn set_field(
    pool: &SqlitePool,
    user: i64,
    column: &'static str,
    value: Option<String>,
) -> Result<()> {
    // `column` is always one of our own constants, never user input.
    let sql =
        format!("UPDATE profiles SET {column} = ?1, updated_at = unixepoch() WHERE user_id = ?2");
    let r = sqlx::query(&sql)
        .bind(value)
        .bind(user)
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ProfileError::NoProfile);
    }
    Ok(())
}

pub async fn set_bio(pool: &SqlitePool, user: i64, bio: Option<&str>) -> Result<()> {
    set_field(
        pool,
        user,
        "bio",
        bio.map(|b| cap(b.trim(), MAX_BIO))
            .filter(|b| !b.is_empty()),
    )
    .await
}

pub async fn set_quote(pool: &SqlitePool, user: i64, quote: Option<&str>) -> Result<()> {
    set_field(
        pool,
        user,
        "quote",
        quote
            .map(|b| cap(b.trim(), MAX_QUOTE))
            .filter(|b| !b.is_empty()),
    )
    .await
}

pub async fn set_banner(pool: &SqlitePool, user: i64, banner: Option<&str>) -> Result<()> {
    set_field(pool, user, "banner", banner.map(str::to_string)).await
}

pub async fn set_display_title(pool: &SqlitePool, user: i64, title: Option<&str>) -> Result<()> {
    set_field(
        pool,
        user,
        "display_title",
        title
            .map(|t| cap(t.trim(), MAX_TITLE))
            .filter(|t| !t.is_empty()),
    )
    .await
}

/// `ar` or `en`; anything else is ignored.
pub async fn set_language(pool: &SqlitePool, user: i64, lang: &str) -> Result<()> {
    if !["ar", "en"].contains(&lang) {
        return Err(ProfileError::NotFound);
    }
    set_field(pool, user, "language", Some(lang.to_string())).await
}

/// Back to the defaults: no bio, quote, banner, colour or title (favourites are kept).
pub async fn reset_profile(pool: &SqlitePool, user: i64) -> Result<()> {
    let r = sqlx::query(
        "UPDATE profiles SET bio = NULL, quote = NULL, banner = NULL, accent_color = NULL, display_title = NULL, \
         updated_at = unixepoch() WHERE user_id = ?1",
    )
    .bind(user)
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(ProfileError::NoProfile);
    }
    Ok(())
}

pub async fn set_style(pool: &SqlitePool, user: i64, style: &str) -> Result<()> {
    set_field(pool, user, "default_style", Some(style.to_string())).await
}

pub async fn set_privacy(pool: &SqlitePool, user: i64, privacy: Privacy) -> Result<()> {
    set_field(pool, user, "privacy", Some(privacy.as_str().to_string())).await
}

pub async fn set_template(pool: &SqlitePool, user: i64, template: &str) -> Result<()> {
    set_field(pool, user, "template", Some(template.to_string())).await
}

/// `colour` is 0xRRGGBB or None to reset.
pub async fn set_color(pool: &SqlitePool, user: i64, colour: Option<u32>) -> Result<()> {
    let r = sqlx::query(
        "UPDATE profiles SET accent_color = ?1, updated_at = unixepoch() WHERE user_id = ?2",
    )
    .bind(colour.map(i64::from))
    .bind(user)
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(ProfileError::NoProfile);
    }
    Ok(())
}

pub(crate) async fn touch(pool: &SqlitePool, user: i64) -> Result<()> {
    sqlx::query("UPDATE profiles SET updated_at = unixepoch() WHERE user_id = ?1")
        .bind(user)
        .execute(pool)
        .await?;
    Ok(())
}

// ---- characters -------------------------------------------------------------------------

pub async fn count(pool: &SqlitePool, user: i64) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT count(*) FROM profile_characters WHERE user_id = ?1")
            .bind(user)
            .fetch_one(pool)
            .await?,
    )
}

#[allow(dead_code)] // used by the tier/collage styles and the character counters
pub async fn has_character(pool: &SqlitePool, user: i64, character: i64) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM profile_characters WHERE user_id = ?1 AND character_id = ?2",
    )
    .bind(user)
    .bind(character)
    .fetch_one(pool)
    .await?
        > 0)
}

pub async fn add_character(pool: &SqlitePool, user: i64, character: i64) -> Result<AddOutcome> {
    require_profile(pool, user).await?;
    let mut tx = pool.begin().await?;
    let next: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(position), 0) + 1 FROM profile_characters WHERE user_id = ?1",
    )
    .bind(user)
    .fetch_one(&mut *tx)
    .await?;
    let r = sqlx::query("INSERT OR IGNORE INTO profile_characters (user_id, character_id, position) VALUES (?1, ?2, ?3)")
        .bind(user)
        .bind(character)
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

/// Removes and closes the gap so positions stay 1..n.
pub async fn remove_character(pool: &SqlitePool, user: i64, character: i64) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let pos: Option<i64> = sqlx::query_scalar(
        "SELECT position FROM profile_characters WHERE user_id = ?1 AND character_id = ?2",
    )
    .bind(user)
    .bind(character)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(pos) = pos else { return Ok(false) };
    sqlx::query("DELETE FROM profile_characters WHERE user_id = ?1 AND character_id = ?2")
        .bind(user)
        .bind(character)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE profile_characters SET position = position - 1 WHERE user_id = ?1 AND position > ?2")
        .bind(user)
        .bind(pos)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    touch(pool, user).await?;
    Ok(true)
}

pub async fn clear(pool: &SqlitePool, user: i64) -> Result<u64> {
    let r = sqlx::query("DELETE FROM profile_characters WHERE user_id = ?1")
        .bind(user)
        .execute(pool)
        .await?;
    touch(pool, user).await?;
    Ok(r.rows_affected())
}

/// Move to 1-based `target` (clamped). Shifts only the affected range, in one transaction.
pub async fn move_character(
    pool: &SqlitePool,
    user: i64,
    character: i64,
    target: i64,
) -> Result<i64> {
    let mut tx = pool.begin().await?;
    let total: i64 =
        sqlx::query_scalar("SELECT count(*) FROM profile_characters WHERE user_id = ?1")
            .bind(user)
            .fetch_one(&mut *tx)
            .await?;
    let from: i64 = sqlx::query_scalar(
        "SELECT position FROM profile_characters WHERE user_id = ?1 AND character_id = ?2",
    )
    .bind(user)
    .bind(character)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ProfileError::NotFound)?;
    let to = target.clamp(1, total.max(1));
    if to < from {
        sqlx::query("UPDATE profile_characters SET position = position + 1 WHERE user_id = ?1 AND position >= ?2 AND position < ?3")
            .bind(user)
            .bind(to)
            .bind(from)
            .execute(&mut *tx)
            .await?;
    } else if to > from {
        sqlx::query("UPDATE profile_characters SET position = position - 1 WHERE user_id = ?1 AND position > ?2 AND position <= ?3")
            .bind(user)
            .bind(from)
            .bind(to)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(
        "UPDATE profile_characters SET position = ?3 WHERE user_id = ?1 AND character_id = ?2",
    )
    .bind(user)
    .bind(character)
    .bind(to)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    touch(pool, user).await?;
    Ok(to)
}

pub async fn position_of(pool: &SqlitePool, user: i64, character: i64) -> Result<Option<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT position FROM profile_characters WHERE user_id = ?1 AND character_id = ?2",
    )
    .bind(user)
    .bind(character)
    .fetch_optional(pool)
    .await?)
}

pub async fn list_page(
    pool: &SqlitePool,
    user: i64,
    group: Option<i64>,
    offset: i64,
    limit: i64,
) -> Result<Vec<Entry>> {
    let sql = format!(
        "{ENTRY_SELECT} WHERE pc.user_id = ?1 AND (?2 IS NULL OR pc.group_id = ?2) \
         ORDER BY pc.position LIMIT ?3 OFFSET ?4"
    );
    Ok(sqlx::query_as::<_, Entry>(&sql)
        .bind(user)
        .bind(group)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?)
}

pub async fn count_in_group(pool: &SqlitePool, user: i64, group: Option<i64>) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM profile_characters WHERE user_id = ?1 AND (?2 IS NULL OR group_id = ?2)",
    )
    .bind(user)
    .bind(group)
    .fetch_one(pool)
    .await?)
}

pub async fn get_entry(pool: &SqlitePool, user: i64, character: i64) -> Result<Option<Entry>> {
    let sql = format!("{ENTRY_SELECT} WHERE pc.user_id = ?1 AND pc.character_id = ?2");
    Ok(sqlx::query_as::<_, Entry>(&sql)
        .bind(user)
        .bind(character)
        .fetch_optional(pool)
        .await?)
}

/// Own favourites whose (verified safe) name matches `partial`, in profile order.
pub async fn search_own(
    pool: &SqlitePool,
    user: i64,
    partial: &str,
    limit: i64,
) -> Result<Vec<(i64, String)>> {
    let escaped = partial
        .trim()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let like = format!("%{escaped}%");
    let rows = sqlx::query(
        "SELECT pc.character_id AS id, cc.name_romaji AS name FROM profile_characters pc \
         JOIN character_cache cc ON cc.character_id = pc.character_id \
         WHERE pc.user_id = ?1 AND cc.is_safe = 1 \
         AND (cc.name_romaji LIKE ?2 ESCAPE '\\' OR cc.name_native LIKE ?2 ESCAPE '\\') \
         ORDER BY pc.position LIMIT ?3",
    )
    .bind(user)
    .bind(like)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.get("id"), r.get("name")))
        .collect())
}

#[allow(dead_code)] // used by the tier/collage styles and the character counters
pub async fn all_character_ids(pool: &SqlitePool, user: i64) -> Result<Vec<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT character_id FROM profile_characters WHERE user_id = ?1 ORDER BY position",
    )
    .bind(user)
    .fetch_all(pool)
    .await?)
}

/// Entries with a tier, in position order (for the tier-list style).
#[allow(dead_code)] // used by the tier/collage styles and the character counters
pub async fn tiered(pool: &SqlitePool, user: i64) -> Result<Vec<Entry>> {
    let sql = format!(
        "{ENTRY_SELECT} WHERE pc.user_id = ?1 AND pc.tier IS NOT NULL ORDER BY pc.position"
    );
    Ok(sqlx::query_as::<_, Entry>(&sql)
        .bind(user)
        .fetch_all(pool)
        .await?)
}

pub async fn set_tier(
    pool: &SqlitePool,
    user: i64,
    character: i64,
    tier: Option<&str>,
) -> Result<()> {
    let tier = tier
        .map(|t| cap(t.trim(), MAX_TIER))
        .filter(|t| !t.is_empty());
    let r = sqlx::query(
        "UPDATE profile_characters SET tier = ?3 WHERE user_id = ?1 AND character_id = ?2",
    )
    .bind(user)
    .bind(character)
    .bind(tier)
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(ProfileError::NotFound);
    }
    Ok(())
}

pub async fn set_note(
    pool: &SqlitePool,
    user: i64,
    character: i64,
    note: Option<&str>,
) -> Result<()> {
    let note = note
        .map(|t| cap(t.trim(), MAX_NOTE))
        .filter(|t| !t.is_empty());
    let r = sqlx::query(
        "UPDATE profile_characters SET note = ?3 WHERE user_id = ?1 AND character_id = ?2",
    )
    .bind(user)
    .bind(character)
    .bind(note)
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(ProfileError::NotFound);
    }
    Ok(())
}

// ---- groups -----------------------------------------------------------------------------

pub async fn list_groups(pool: &SqlitePool, user: i64) -> Result<Vec<Group>> {
    Ok(sqlx::query_as::<_, Group>(
        "SELECT id, name, position FROM profile_groups WHERE user_id = ?1 ORDER BY position, id",
    )
    .bind(user)
    .fetch_all(pool)
    .await?)
}

pub async fn create_group(pool: &SqlitePool, user: i64, name: &str) -> Result<i64> {
    require_profile(pool, user).await?;
    let name = cap(name.trim(), MAX_GROUP_NAME);
    if name.is_empty() {
        return Err(ProfileError::Invalid("empty group name"));
    }
    let pos: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(position), 0) + 1 FROM profile_groups WHERE user_id = ?1",
    )
    .bind(user)
    .fetch_one(pool)
    .await?;
    let r = sqlx::query(
        "INSERT OR IGNORE INTO profile_groups (user_id, name, position) VALUES (?1, ?2, ?3)",
    )
    .bind(user)
    .bind(&name)
    .bind(pos)
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(ProfileError::Duplicate);
    }
    Ok(r.last_insert_rowid())
}

pub async fn find_group(pool: &SqlitePool, user: i64, name: &str) -> Result<Option<Group>> {
    Ok(sqlx::query_as::<_, Group>(
        "SELECT id, name, position FROM profile_groups WHERE user_id = ?1 AND name = ?2 COLLATE NOCASE",
    )
    .bind(user)
    .bind(name.trim())
    .fetch_optional(pool)
    .await?)
}

pub async fn rename_group(pool: &SqlitePool, user: i64, id: i64, new_name: &str) -> Result<()> {
    let name = cap(new_name.trim(), MAX_GROUP_NAME);
    if name.is_empty() {
        return Err(ProfileError::Invalid("empty group name"));
    }
    let r =
        sqlx::query("UPDATE OR IGNORE profile_groups SET name = ?3 WHERE user_id = ?1 AND id = ?2")
            .bind(user)
            .bind(id)
            .bind(name)
            .execute(pool)
            .await?;
    if r.rows_affected() == 0 {
        // Either missing or the new name is taken.
        return if find_group_by_id(pool, user, id).await?.is_some() {
            Err(ProfileError::Duplicate)
        } else {
            Err(ProfileError::NotFound)
        };
    }
    Ok(())
}

async fn find_group_by_id(pool: &SqlitePool, user: i64, id: i64) -> Result<Option<Group>> {
    Ok(sqlx::query_as::<_, Group>(
        "SELECT id, name, position FROM profile_groups WHERE user_id = ?1 AND id = ?2",
    )
    .bind(user)
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

/// Characters of the group become ungrouped (ON DELETE SET NULL).
pub async fn delete_group(pool: &SqlitePool, user: i64, id: i64) -> Result<()> {
    let r = sqlx::query("DELETE FROM profile_groups WHERE user_id = ?1 AND id = ?2")
        .bind(user)
        .bind(id)
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
    character: i64,
    group: Option<i64>,
) -> Result<()> {
    let r = sqlx::query(
        "UPDATE profile_characters SET group_id = ?3 WHERE user_id = ?1 AND character_id = ?2",
    )
    .bind(user)
    .bind(character)
    .bind(group)
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(ProfileError::NotFound);
    }
    Ok(())
}

// ---- cache ------------------------------------------------------------------------------

pub async fn upsert_cache(pool: &SqlitePool, row: &CacheRow) -> Result<()> {
    sqlx::query(
        "INSERT INTO character_cache (character_id, name_romaji, name_native, image_url, is_safe, main_work, \
         main_work_url, favourites, fetched_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(character_id) DO UPDATE SET name_romaji = excluded.name_romaji, name_native = excluded.name_native, \
         image_url = excluded.image_url, is_safe = excluded.is_safe, main_work = excluded.main_work, \
         main_work_url = excluded.main_work_url, favourites = excluded.favourites, fetched_at = excluded.fetched_at",
    )
    .bind(row.character_id)
    .bind(&row.name_romaji)
    .bind(&row.name_native)
    .bind(&row.image_url)
    .bind(row.is_safe)
    .bind(&row.main_work)
    .bind(&row.main_work_url)
    .bind(row.favourites)
    .bind(row.fetched_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn cache_get(pool: &SqlitePool, character: i64) -> Result<Option<CacheRow>> {
    let row = sqlx::query(
        "SELECT character_id, name_romaji, name_native, image_url, is_safe, main_work, main_work_url, favourites, fetched_at \
         FROM character_cache WHERE character_id = ?1",
    )
    .bind(character)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| CacheRow {
        character_id: r.get("character_id"),
        name_romaji: r.get("name_romaji"),
        name_native: r.get("name_native"),
        image_url: r.get("image_url"),
        is_safe: r.get("is_safe"),
        main_work: r.get("main_work"),
        main_work_url: r.get("main_work_url"),
        favourites: r.get("favourites"),
        fetched_at: r.get("fetched_at"),
    }))
}

// ---- social -----------------------------------------------------------------------------

pub async fn touch_user_guild(pool: &SqlitePool, guild: i64, user: i64) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO user_guilds (guild_id, user_id) VALUES (?1, ?2)")
        .bind(guild)
        .bind(user)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn shares_guild(pool: &SqlitePool, a: i64, b: i64) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM user_guilds x JOIN user_guilds y ON x.guild_id = y.guild_id \
         WHERE x.user_id = ?1 AND y.user_id = ?2",
    )
    .bind(a)
    .bind(b)
    .fetch_one(pool)
    .await?
        > 0)
}

/// Owner always; otherwise by the profile's privacy setting.
pub async fn can_view(pool: &SqlitePool, viewer: i64, profile: &Profile) -> Result<bool> {
    if viewer == profile.user_id {
        return Ok(true);
    }
    Ok(match profile.privacy() {
        Privacy::Public => true,
        Privacy::Private => false,
        Privacy::Servers => shares_guild(pool, viewer, profile.user_id).await?,
    })
}

/// How many members of `guild` (that used the bot) have this character.
#[allow(dead_code)] // used by the tier/collage styles and the character counters
pub async fn server_favourite_count(pool: &SqlitePool, guild: i64, character: i64) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM profile_characters pc JOIN user_guilds ug ON ug.user_id = pc.user_id \
         WHERE ug.guild_id = ?1 AND pc.character_id = ?2",
    )
    .bind(guild)
    .bind(character)
    .fetch_one(pool)
    .await?)
}

/// Most favourited characters, per guild or global: `(character_id, count)`.
pub async fn top_characters(
    pool: &SqlitePool,
    guild: Option<i64>,
    offset: i64,
    limit: i64,
) -> Result<Vec<(i64, i64)>> {
    let rows = sqlx::query(
        "SELECT pc.character_id AS cid, count(*) AS n FROM profile_characters pc \
         JOIN profiles p ON p.user_id = pc.user_id AND p.privacy != 'private' \
         WHERE (?1 IS NULL OR pc.user_id IN (SELECT user_id FROM user_guilds WHERE guild_id = ?1)) \
         GROUP BY pc.character_id ORDER BY n DESC, pc.character_id LIMIT ?2 OFFSET ?3",
    )
    .bind(guild)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.get("cid"), r.get("n")))
        .collect())
}

pub async fn top_character_count(pool: &SqlitePool, guild: Option<i64>) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(DISTINCT pc.character_id) FROM profile_characters pc \
         JOIN profiles p ON p.user_id = pc.user_id AND p.privacy != 'private' \
         WHERE (?1 IS NULL OR pc.user_id IN (SELECT user_id FROM user_guilds WHERE guild_id = ?1))",
    )
    .bind(guild)
    .fetch_one(pool)
    .await?)
}

/// Character ids both users have, in `a`'s order.
pub async fn shared_characters(pool: &SqlitePool, a: i64, b: i64) -> Result<Vec<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT x.character_id FROM profile_characters x JOIN profile_characters y \
         ON x.character_id = y.character_id WHERE x.user_id = ?1 AND y.user_id = ?2 ORDER BY x.position",
    )
    .bind(a)
    .bind(b)
    .fetch_all(pool)
    .await?)
}

/// Overlap as a percentage of the smaller profile (Szymkiewicz-Simpson).
pub fn compatibility(shared: usize, a_total: usize, b_total: usize) -> u32 {
    let smaller = a_total.min(b_total);
    if smaller == 0 {
        return 0;
    }
    ((shared as f64 / smaller as f64) * 100.0)
        .round()
        .min(100.0) as u32
}

/// Everything stored about a user: profile (characters, works, groups), custom images, scores,
/// hidden recommendations, report/ban records and guild links. Irreversible.
pub async fn delete_user_data(pool: &SqlitePool, user: i64) -> Result<()> {
    let mut tx = pool.begin().await?;
    for sql in [
        "DELETE FROM profile_characters WHERE user_id = ?1",
        "DELETE FROM profile_works WHERE user_id = ?1",
        "DELETE FROM profile_groups WHERE user_id = ?1",
        "DELETE FROM profiles WHERE user_id = ?1",
        "DELETE FROM custom_images WHERE user_id = ?1",
        "DELETE FROM image_reports WHERE reporter_id = ?1 OR owner_id = ?1",
        "DELETE FROM upload_bans WHERE user_id = ?1",
        "DELETE FROM game_scores WHERE user_id = ?1",
        "DELETE FROM hidden_recommendations WHERE user_id = ?1",
        "DELETE FROM user_guilds WHERE user_id = ?1",
        "DELETE FROM users WHERE discord_id = ?1",
    ] {
        sqlx::query(sql).bind(user).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

// ---- export / import --------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExportChar {
    pub id: i64,
    pub group: Option<String>,
    pub tier: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExportDoc {
    pub version: u32,
    pub bio: Option<String>,
    pub accent_color: Option<u32>,
    pub quote: Option<String>,
    pub banner: Option<String>,
    pub default_style: String,
    pub privacy: String,
    pub groups: Vec<String>,
    pub characters: Vec<ExportChar>,
    /// Added in version 2; version 1 files simply have none.
    #[serde(default)]
    pub works: Vec<super::works::ExportWork>,
}

pub async fn export(pool: &SqlitePool, user: i64) -> Result<ExportDoc> {
    let p = require_profile(pool, user).await?;
    let groups = list_groups(pool, user).await?;
    let rows = sqlx::query(
        "SELECT character_id, group_id, tier, note FROM profile_characters WHERE user_id = ?1 ORDER BY position",
    )
    .bind(user)
    .fetch_all(pool)
    .await?;
    let group_name = |id: Option<i64>| {
        id.and_then(|i| groups.iter().find(|g| g.id == i))
            .map(|g| g.name.clone())
    };
    let works = super::works::export_works(pool, user, &groups).await?;
    Ok(ExportDoc {
        version: 2,
        bio: p.bio,
        accent_color: p.accent_color.map(|c| c as u32),
        quote: p.quote,
        banner: p.banner,
        default_style: p.default_style,
        privacy: p.privacy,
        groups: groups.iter().map(|g| g.name.clone()).collect(),
        characters: rows
            .iter()
            .map(|r| ExportChar {
                id: r.get("character_id"),
                group: group_name(r.get("group_id")),
                tier: r.get("tier"),
                note: r.get("note"),
            })
            .collect(),
        works,
    })
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub added: usize,
    pub duplicates: usize,
    pub invalid: usize,
}

/// Validates and merges a document into the user's profile. Characters are added without cached
/// metadata, so they stay hidden until verified safe (see `Entry::visible`).
pub async fn import(
    pool: &SqlitePool,
    user: i64,
    doc: &ExportDoc,
    allowed_styles: &[&str],
) -> Result<ImportReport> {
    if !(1..=2).contains(&doc.version) {
        return Err(ProfileError::Invalid("unsupported version"));
    }
    if doc.characters.len() + doc.works.len() > MAX_IMPORT {
        return Err(ProfileError::Invalid("too many characters"));
    }
    require_profile(pool, user).await?;
    let mut report = ImportReport::default();

    set_bio(pool, user, doc.bio.as_deref()).await?;
    set_quote(pool, user, doc.quote.as_deref()).await?;
    set_color(pool, user, doc.accent_color.filter(|c| *c <= 0xFF_FFFF)).await?;
    if let Some(b) = doc.banner.as_deref().filter(|b| b.starts_with("https://")) {
        set_banner(pool, user, Some(b)).await?;
    }
    if allowed_styles.contains(&doc.default_style.as_str()) {
        set_style(pool, user, &doc.default_style).await?;
    }
    if let Some(p) = Privacy::parse(&doc.privacy) {
        set_privacy(pool, user, p).await?;
    }
    for name in &doc.groups {
        match create_group(pool, user, name).await {
            Ok(_) | Err(ProfileError::Duplicate) | Err(ProfileError::Invalid(_)) => {}
            Err(e) => return Err(e),
        }
    }
    for c in &doc.characters {
        if c.id <= 0 {
            report.invalid += 1;
            continue;
        }
        match add_character(pool, user, c.id).await? {
            AddOutcome::Duplicate => report.duplicates += 1,
            AddOutcome::Added(_) => {
                report.added += 1;
                if let Some(t) = &c.tier {
                    set_tier(pool, user, c.id, Some(t)).await?;
                }
                if let Some(n) = &c.note {
                    set_note(pool, user, c.id, Some(n)).await?;
                }
                if let Some(g) = c.group.as_deref().filter(|n| !n.trim().is_empty()) {
                    if let Some(group) = find_group(pool, user, g).await? {
                        assign_group(pool, user, c.id, Some(group.id)).await?;
                    }
                }
            }
        }
    }
    let (added, duplicates, invalid) = super::works::import_works(pool, user, &doc.works).await?;
    report.added += added;
    report.duplicates += duplicates;
    report.invalid += invalid;
    Ok(report)
}

/// Overlap of characters and works as one percentage (characters weigh 60%, works 40%; an empty side
/// does not count).
pub fn combined_compatibility(
    shared_c: usize,
    a_c: usize,
    b_c: usize,
    shared_w: usize,
    a_w: usize,
    b_w: usize,
) -> u32 {
    let c = (a_c.min(b_c) > 0).then(|| compatibility(shared_c, a_c, b_c));
    let w = (a_w.min(b_w) > 0).then(|| compatibility(shared_w, a_w, b_w));
    match (c, w) {
        (Some(c), Some(w)) => ((f64::from(c) * 0.6) + (f64::from(w) * 0.4)).round() as u32,
        (Some(x), None) | (None, Some(x)) => x,
        (None, None) => 0,
    }
}

#[cfg(test)]
mod tests;
