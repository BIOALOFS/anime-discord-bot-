//! Personal customization (title, description, image) for characters in a user's own profile,
//! plus custom-image storage, reports, upload bans and the audit log.

use super::profile::{ProfileError, Result};
use crate::anilist::safety::contains_explicit_term;
use sqlx::{Row, SqlitePool};

pub const MAX_TITLE: usize = 40;
pub const MAX_DESCRIPTION: usize = 300;
pub const MAX_IMAGES_PER_USER: usize = 30;

/// Clean user-written text: no mentions, invite links or URLs, one line, no explicit words, capped.
/// `None` when nothing usable is left (or the text is explicit).
pub fn sanitize_text(input: &str, max: usize) -> Option<String> {
    let mut words = Vec::new();
    for raw in input.split_whitespace() {
        let w = raw.trim();
        let lower = w.to_lowercase();
        let is_mention = lower.contains("@everyone")
            || lower.contains("@here")
            || (w.starts_with("<@") && w.ends_with('>'))
            || (w.starts_with("<#") && w.ends_with('>'))
            || w.starts_with('@');
        let is_link = lower.contains("://")
            || lower.starts_with("www.")
            || lower.contains("discord.gg")
            || lower.contains("discord.com/invite")
            || lower.contains("discordapp.com/invite");
        if is_mention || is_link {
            continue;
        }
        words.push(w.replace(['<', '>'], ""));
    }
    let text: String = words
        .join(" ")
        .chars()
        .take(max)
        .collect::<String>()
        .trim()
        .to_string();
    (!text.is_empty() && !contains_explicit_term(&text)).then_some(text)
}

async fn update_char(
    pool: &SqlitePool,
    column: &'static str,
    user: i64,
    character: i64,
    value: Option<String>,
) -> Result<()> {
    // `column` is always one of our own constants, never user input.
    let sql = format!(
        "UPDATE profile_characters SET {column} = ?3 WHERE user_id = ?1 AND character_id = ?2"
    );
    let r = sqlx::query(&sql)
        .bind(user)
        .bind(character)
        .bind(value)
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ProfileError::NotFound);
    }
    Ok(())
}

/// Only for a character already in the caller's profile (`NotFound` otherwise).
pub async fn set_title(
    pool: &SqlitePool,
    user: i64,
    character: i64,
    text: Option<&str>,
) -> Result<()> {
    let clean = match text {
        Some(t) => {
            Some(sanitize_text(t, MAX_TITLE).ok_or(ProfileError::Invalid("unusable title"))?)
        }
        None => None,
    };
    update_char(pool, "custom_title", user, character, clean).await
}

pub async fn set_description(
    pool: &SqlitePool,
    user: i64,
    character: i64,
    text: Option<&str>,
) -> Result<()> {
    let clean = match text {
        Some(t) => Some(
            sanitize_text(t, MAX_DESCRIPTION)
                .ok_or(ProfileError::Invalid("unusable description"))?,
        ),
        None => None,
    };
    update_char(pool, "custom_description", user, character, clean).await
}

/// Clears title, description and image of one character.
pub async fn reset_character(pool: &SqlitePool, user: i64, character: i64) -> Result<()> {
    let image: Option<i64> = sqlx::query_scalar(
        "SELECT custom_image_id FROM profile_characters WHERE user_id = ?1 AND character_id = ?2",
    )
    .bind(user)
    .bind(character)
    .fetch_optional(pool)
    .await?
    .ok_or(ProfileError::NotFound)?;
    sqlx::query("UPDATE profile_characters SET custom_title = NULL, custom_description = NULL, custom_image_id = NULL WHERE user_id = ?1 AND character_id = ?2")
        .bind(user)
        .bind(character)
        .execute(pool)
        .await?;
    if let Some(id) = image {
        drop_if_unused(pool, id).await?;
    }
    Ok(())
}

// ---- images ---------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NewImage {
    pub hash: String,
    pub mime: String,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub animated: bool,
}

#[derive(Debug, Clone)]
pub struct StoredImage {
    pub id: i64,
    pub hash: String,
    pub bytes: Vec<u8>,
    pub animated: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StoreOutcome {
    Stored(i64),
    /// The same picture was already uploaded by this user (deduplicated by hash).
    Existing(i64),
    LimitReached,
}

fn image_from_row(r: &sqlx::sqlite::SqliteRow) -> StoredImage {
    StoredImage {
        id: r.get("id"),
        hash: r.get("hash"),
        bytes: r.get("bytes"),
        animated: r.get("animated"),
    }
}

pub async fn store_image(
    pool: &SqlitePool,
    user: i64,
    img: &NewImage,
    limit: usize,
) -> Result<StoreOutcome> {
    if let Some(id) = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM custom_images WHERE user_id = ?1 AND hash = ?2",
    )
    .bind(user)
    .bind(&img.hash)
    .fetch_optional(pool)
    .await?
    {
        return Ok(StoreOutcome::Existing(id));
    }
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM custom_images WHERE user_id = ?1")
        .bind(user)
        .fetch_one(pool)
        .await?;
    if n as usize >= limit {
        return Ok(StoreOutcome::LimitReached);
    }
    let r = sqlx::query("INSERT INTO custom_images (user_id, hash, mime, bytes, width, height, animated) VALUES (?1,?2,?3,?4,?5,?6,?7)")
        .bind(user)
        .bind(&img.hash)
        .bind(&img.mime)
        .bind(&img.bytes)
        .bind(i64::from(img.width))
        .bind(i64::from(img.height))
        .bind(img.animated)
        .execute(pool)
        .await?;
    Ok(StoreOutcome::Stored(r.last_insert_rowid()))
}

pub async fn get_image(pool: &SqlitePool, id: i64) -> Result<Option<StoredImage>> {
    let r = sqlx::query("SELECT id, user_id, hash, mime, bytes, width, height, animated FROM custom_images WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(r.as_ref().map(image_from_row))
}

/// Attach an image the caller uploaded to a character of the caller's profile.
pub async fn set_character_image(
    pool: &SqlitePool,
    user: i64,
    character: i64,
    image_id: i64,
) -> Result<()> {
    let owner: Option<i64> = sqlx::query_scalar("SELECT user_id FROM custom_images WHERE id = ?1")
        .bind(image_id)
        .fetch_optional(pool)
        .await?;
    if owner != Some(user) {
        return Err(ProfileError::NotFound);
    }
    let previous: Option<i64> = sqlx::query_scalar(
        "SELECT custom_image_id FROM profile_characters WHERE user_id = ?1 AND character_id = ?2",
    )
    .bind(user)
    .bind(character)
    .fetch_optional(pool)
    .await?
    .ok_or(ProfileError::NotFound)?;
    update_char(
        pool,
        "custom_image_id",
        user,
        character,
        Some(image_id.to_string()),
    )
    .await?;
    // The column is INTEGER: make sure SQLite stored a number, not text.
    sqlx::query("UPDATE profile_characters SET custom_image_id = CAST(custom_image_id AS INTEGER) WHERE user_id = ?1 AND character_id = ?2")
        .bind(user)
        .bind(character)
        .execute(pool)
        .await?;
    if let Some(prev) = previous.filter(|p| *p != image_id) {
        drop_if_unused(pool, prev).await?;
    }
    Ok(())
}

pub async fn clear_character_image(pool: &SqlitePool, user: i64, character: i64) -> Result<()> {
    let previous: Option<i64> = sqlx::query_scalar(
        "SELECT custom_image_id FROM profile_characters WHERE user_id = ?1 AND character_id = ?2",
    )
    .bind(user)
    .bind(character)
    .fetch_optional(pool)
    .await?
    .ok_or(ProfileError::NotFound)?;
    sqlx::query("UPDATE profile_characters SET custom_image_id = NULL WHERE user_id = ?1 AND character_id = ?2")
        .bind(user)
        .bind(character)
        .execute(pool)
        .await?;
    if let Some(id) = previous {
        drop_if_unused(pool, id).await?;
    }
    Ok(())
}

async fn drop_if_unused(pool: &SqlitePool, image_id: i64) -> Result<()> {
    sqlx::query("DELETE FROM custom_images WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM profile_characters WHERE custom_image_id = ?1)")
        .bind(image_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// The image the owner set for this character (never another user's).
pub async fn image_of_character(
    pool: &SqlitePool,
    user: i64,
    character: i64,
) -> Result<Option<StoredImage>> {
    let r = sqlx::query(
        "SELECT i.id, i.user_id, i.hash, i.mime, i.bytes, i.width, i.height, i.animated FROM profile_characters pc \
         JOIN custom_images i ON i.id = pc.custom_image_id AND i.user_id = pc.user_id \
         WHERE pc.user_id = ?1 AND pc.character_id = ?2",
    )
    .bind(user)
    .bind(character)
    .fetch_optional(pool)
    .await?;
    Ok(r.as_ref().map(image_from_row))
}

#[cfg(test)]
pub async fn image_count(pool: &SqlitePool, user: i64) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT count(*) FROM custom_images WHERE user_id = ?1")
            .bind(user)
            .fetch_one(pool)
            .await?,
    )
}

// ---- moderation -------------------------------------------------------------------------------

/// `false` if this member already reported the image.
pub async fn report_image(
    pool: &SqlitePool,
    image_id: i64,
    reporter: i64,
    guild: Option<i64>,
) -> Result<bool> {
    let owner: i64 = sqlx::query_scalar("SELECT user_id FROM custom_images WHERE id = ?1")
        .bind(image_id)
        .fetch_optional(pool)
        .await?
        .ok_or(ProfileError::NotFound)?;
    let r = sqlx::query("INSERT OR IGNORE INTO image_reports (image_id, owner_id, reporter_id, guild_id) VALUES (?1, ?2, ?3, ?4)")
        .bind(image_id)
        .bind(owner)
        .bind(reporter)
        .bind(guild)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// Unresolved reports of this server: `(image_id, owner_id, reports)`.
pub async fn open_reports(pool: &SqlitePool, guild: i64) -> Result<Vec<(i64, i64, i64)>> {
    let rows = sqlx::query(
        "SELECT image_id, owner_id, count(*) AS n FROM image_reports WHERE guild_id = ?1 AND resolved = 0 \
         GROUP BY image_id ORDER BY n DESC LIMIT 25",
    )
    .bind(guild)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| (r.get("image_id"), r.get("owner_id"), r.get("n")))
        .collect())
}

/// Moderator removal: detach from every character, delete, resolve reports, write the audit log.
pub async fn remove_image_as_mod(
    pool: &SqlitePool,
    image_id: i64,
    actor: i64,
    guild: i64,
) -> Result<bool> {
    sqlx::query("UPDATE profile_characters SET custom_image_id = NULL WHERE custom_image_id = ?1")
        .bind(image_id)
        .execute(pool)
        .await?;
    let r = sqlx::query("DELETE FROM custom_images WHERE id = ?1")
        .bind(image_id)
        .execute(pool)
        .await?;
    sqlx::query("UPDATE image_reports SET resolved = 1 WHERE image_id = ?1")
        .bind(image_id)
        .execute(pool)
        .await?;
    if r.rows_affected() > 0 {
        audit(
            pool,
            Some(guild),
            actor,
            "remove_image",
            Some(&image_id.to_string()),
        )
        .await?;
    }
    Ok(r.rows_affected() > 0)
}

pub async fn ban_uploads(pool: &SqlitePool, guild: i64, user: i64, by: i64) -> Result<()> {
    sqlx::query(
        "INSERT OR REPLACE INTO upload_bans (guild_id, user_id, banned_by) VALUES (?1, ?2, ?3)",
    )
    .bind(guild)
    .bind(user)
    .bind(by)
    .execute(pool)
    .await?;
    audit(
        pool,
        Some(guild),
        by,
        "ban_uploads",
        Some(&user.to_string()),
    )
    .await
}

pub async fn unban_uploads(pool: &SqlitePool, guild: i64, user: i64, by: i64) -> Result<bool> {
    let r = sqlx::query("DELETE FROM upload_bans WHERE guild_id = ?1 AND user_id = ?2")
        .bind(guild)
        .bind(user)
        .execute(pool)
        .await?;
    if r.rows_affected() > 0 {
        audit(
            pool,
            Some(guild),
            by,
            "unban_uploads",
            Some(&user.to_string()),
        )
        .await?;
    }
    Ok(r.rows_affected() > 0)
}

pub async fn is_banned(pool: &SqlitePool, guild: i64, user: i64) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM upload_bans WHERE guild_id = ?1 AND user_id = ?2",
    )
    .bind(guild)
    .bind(user)
    .fetch_one(pool)
    .await?
        > 0)
}

pub async fn audit(
    pool: &SqlitePool,
    guild: Option<i64>,
    actor: i64,
    action: &str,
    target: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO audit_log (guild_id, actor_id, action, target) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(guild)
    .bind(actor)
    .bind(action)
    .bind(target)
    .execute(pool)
    .await?;
    Ok(())
}

// ---- per-server switches ----------------------------------------------------------------------

async fn flag(pool: &SqlitePool, guild: i64, column: &'static str) -> Result<bool> {
    // `column` is one of our own constants. A missing row means the column default.
    let default = matches!(column, "games" | "mature_games");
    let v: Option<bool> = sqlx::query_scalar(&format!(
        "SELECT {column} FROM guild_settings WHERE guild_id = ?1"
    ))
    .bind(guild)
    .fetch_optional(pool)
    .await?;
    Ok(v.unwrap_or(default))
}

async fn set_flag(pool: &SqlitePool, guild: i64, column: &'static str, on: bool) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO guild_settings (guild_id) VALUES (?1)")
        .bind(guild)
        .execute(pool)
        .await?;
    sqlx::query(&format!(
        "UPDATE guild_settings SET {column} = ?2 WHERE guild_id = ?1"
    ))
    .bind(guild)
    .bind(on)
    .execute(pool)
    .await?;
    Ok(())
}

/// Off until an admin enables it.
pub async fn custom_images_enabled(pool: &SqlitePool, guild: i64) -> Result<bool> {
    flag(pool, guild, "custom_images").await
}
pub async fn set_custom_images(pool: &SqlitePool, guild: i64, on: bool) -> Result<()> {
    set_flag(pool, guild, "custom_images", on).await
}
pub async fn games_enabled(pool: &SqlitePool, guild: i64) -> Result<bool> {
    flag(pool, guild, "games").await
}
pub async fn set_games(pool: &SqlitePool, guild: i64, on: bool) -> Result<()> {
    set_flag(pool, guild, "games", on).await
}
/// Whether mature-rated (violent/horror) games may be shown. Sexual content is hidden regardless.
pub async fn mature_games(pool: &SqlitePool, guild: i64) -> Result<bool> {
    flag(pool, guild, "mature_games").await
}
pub async fn set_mature_games(pool: &SqlitePool, guild: i64, on: bool) -> Result<()> {
    set_flag(pool, guild, "mature_games", on).await
}
pub async fn nsfw_mode(pool: &SqlitePool, guild: i64) -> Result<bool> {
    flag(pool, guild, "nsfw_mode").await
}
pub async fn set_nsfw_mode(pool: &SqlitePool, guild: i64, on: bool) -> Result<()> {
    set_flag(pool, guild, "nsfw_mode", on).await
}

#[cfg(test)]
mod tests;
