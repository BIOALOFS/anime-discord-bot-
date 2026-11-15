//! Helpers shared by the profile commands.

use crate::{
    anilist::{models::Character, CharacterLookup, ContentPolicy, SafeAniList},
    commands::parse_id,
    db::profile::{self as db, CacheRow, Entry, Profile},
    error::{Context, Error},
    i18n,
    ui::{autocomplete::to_choices, embeds::fit},
};
use futures::StreamExt;
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicBool, Ordering};

const STALE_SECS: i64 = 6 * 3600;
const REFRESH_CONCURRENCY: usize = 4;

pub fn uid(ctx: Context<'_>) -> i64 {
    ctx.author().id.get() as i64
}

pub fn pool(ctx: Context<'_>) -> SqlitePool {
    ctx.data().db.clone()
}

/// Defer privately (every reply is ephemeral) and remember which guild the user is active in.
pub async fn begin_private(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    remember_guild(ctx).await;
    Ok(())
}

pub async fn begin_public(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    remember_guild(ctx).await;
    Ok(())
}

async fn remember_guild(ctx: Context<'_>) {
    if let Some(g) = ctx.guild_id() {
        if let Err(e) = db::touch_user_guild(&ctx.data().db, g.get() as i64, uid(ctx)).await {
            tracing::debug!("could not record guild membership: {e}");
        }
    }
}

pub async fn private_reply(ctx: Context<'_>, text: impl Into<String>) -> Result<(), Error> {
    ctx.send(poise::CreateReply::default().content(text).ephemeral(true))
        .await?;
    Ok(())
}

/// The caller's profile, or a polite "create one first" reply and `None`.
pub async fn require_own(ctx: Context<'_>) -> Result<Option<Profile>, Error> {
    match db::get_profile(&ctx.data().db, uid(ctx)).await? {
        Some(p) => Ok(Some(p)),
        None => {
            private_reply(ctx, i18n::PROFILE_NONE_SELF).await?;
            Ok(None)
        }
    }
}

pub fn cache_row(c: &Character, now: i64) -> CacheRow {
    let work = c.media.as_ref().and_then(|m| m.nodes.first());
    CacheRow {
        character_id: c.id,
        name_romaji: c.display_name().to_string(),
        name_native: c.name.native.clone(),
        image_url: c.image_url().map(str::to_string),
        is_safe: true,
        main_work: work
            .and_then(|w| w.title.as_ref())
            .map(|t| t.main().to_string()),
        main_work_url: work.and_then(|w| w.site_url.clone()),
        favourites: c.favourites,
        fetched_at: now,
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Verify and cache entries whose metadata is missing or stale. Bounded concurrency; stops at the
/// first AniList failure so a dead API never makes a profile view slow (cached data is used).
pub async fn refresh_entries(
    api: &SafeAniList,
    pool: &SqlitePool,
    policy: ContentPolicy,
    entries: &[Entry],
) {
    let t = now();
    let ids: Vec<i64> = entries
        .iter()
        .filter(|e| e.fetched_at.is_none_or(|f| t - f > STALE_SECS))
        .map(|e| e.character_id)
        .collect();
    if ids.is_empty() {
        return;
    }
    let failed = AtomicBool::new(false);
    let work = futures::stream::iter(ids).for_each_concurrent(REFRESH_CONCURRENCY, |id| {
        let failed = &failed;
        async move {
            if failed.load(Ordering::Relaxed) {
                return;
            }
            let row = match api.character_by_id(id, policy).await {
                Ok(CharacterLookup::Found(c)) => cache_row(&c, t),
                Ok(CharacterLookup::Hidden) => CacheRow {
                    character_id: id,
                    name_romaji: "?".into(),
                    name_native: None,
                    image_url: None,
                    is_safe: false,
                    main_work: None,
                    main_work_url: None,
                    favourites: None,
                    fetched_at: t,
                },
                Ok(CharacterLookup::NotFound) => return,
                Err(e) => {
                    tracing::debug!("character refresh failed, using cached data: {e}");
                    failed.store(true, Ordering::Relaxed);
                    return;
                }
            };
            if let Err(e) = db::upsert_cache(pool, &row).await {
                tracing::warn!("could not cache character {id}: {e}");
            }
        }
    });
    if tokio::time::timeout(std::time::Duration::from_secs(10), work)
        .await
        .is_err()
    {
        tracing::debug!("character refresh timed out");
    }
}

pub enum Resolved {
    One(Box<Character>),
    Many(Vec<Character>),
    Hidden,
    NotFound,
}

/// Autocomplete id (`id:123`) or free text -> safe AniList character(s).
pub async fn resolve_character(
    api: &SafeAniList,
    policy: ContentPolicy,
    input: &str,
) -> Result<Resolved, crate::anilist::AniError> {
    if let Some(id) = parse_id(input) {
        return Ok(match api.character_by_id(id, policy).await? {
            CharacterLookup::Found(c) => Resolved::One(c),
            CharacterLookup::Hidden => Resolved::Hidden,
            CharacterLookup::NotFound => Resolved::NotFound,
        });
    }
    let mut list = api.search_characters(input.trim(), 10, policy).await?;
    Ok(match list.len() {
        0 => Resolved::NotFound,
        1 => Resolved::One(Box::new(list.remove(0))),
        _ => Resolved::Many(list),
    })
}

pub enum Own {
    One(i64, String),
    NotFound,
    Many,
}

/// A character from the caller's own list: autocomplete id or a unique name match.
pub async fn resolve_own(pool: &SqlitePool, user: i64, input: &str) -> Result<Own, Error> {
    if let Some(id) = parse_id(input) {
        return Ok(match db::get_entry(pool, user, id).await? {
            Some(e) => Own::One(id, e.name().to_string()),
            None => Own::NotFound,
        });
    }
    let mut hits = db::search_own(pool, user, input, 2).await?;
    Ok(match hits.len() {
        0 => Own::NotFound,
        1 => {
            let (id, name) = hits.remove(0);
            Own::One(id, name)
        }
        _ => Own::Many,
    })
}

/// Resolve or send the matching Arabic refusal.
pub async fn own_or_reply(ctx: Context<'_>, input: &str) -> Result<Option<(i64, String)>, Error> {
    match resolve_own(&ctx.data().db, uid(ctx), input).await? {
        Own::One(id, name) => Ok(Some((id, name))),
        Own::NotFound => {
            private_reply(ctx, i18n::PROFILE_NOT_IN_LIST).await?;
            Ok(None)
        }
        Own::Many => {
            private_reply(ctx, i18n::PROFILE_AMBIGUOUS).await?;
            Ok(None)
        }
    }
}

/// Autocomplete over the caller's own favourites (values are `id:123`).
pub async fn ac_own(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    match db::search_own(&ctx.data().db, uid(ctx), partial, 25).await {
        Ok(hits) => to_choices(
            hits.into_iter()
                .map(|(id, name)| (fit(&name, 100), format!("id:{id}")))
                .collect(),
        ),
        Err(_) => Vec::new(),
    }
}

pub fn parse_color(s: &str) -> Option<u32> {
    let t = s.trim().trim_start_matches('#');
    (t.len() == 6 && t.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| u32::from_str_radix(t, 16).ok())
        .flatten()
}

const BANNER_HOSTS: &[&str] = &[
    "s4.anilist.co",
    "cdn.discordapp.com",
    "media.discordapp.net",
    "i.imgur.com",
];
const BANNER_EXTS: &[&str] = &[".png", ".jpg", ".jpeg", ".webp", ".gif"];

/// Only https images from a small allow-list of hosts.
pub fn banner_url_ok(url: &str) -> bool {
    let Some(rest) = url.trim().strip_prefix("https://") else {
        return false;
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = path.split(['?', '#']).next().unwrap_or("").to_lowercase();
    BANNER_HOSTS.contains(&host.to_lowercase().as_str())
        && !host.contains('@')
        && BANNER_EXTS.iter().any(|e| path.ends_with(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours() {
        assert_eq!(parse_color("#3DB4F2"), Some(0x3DB4F2));
        assert_eq!(parse_color("3db4f2"), Some(0x3DB4F2));
        assert_eq!(parse_color("#fff"), None);
        assert_eq!(parse_color("#GGGGGG"), None);
        assert_eq!(parse_color(""), None);
    }

    #[test]
    fn banner_urls_are_allow_listed() {
        assert!(banner_url_ok(
            "https://s4.anilist.co/file/anilistcdn/character/large/b1.png"
        ));
        assert!(banner_url_ok(
            "https://cdn.discordapp.com/attachments/1/2/a.JPG?ex=1"
        ));
        assert!(!banner_url_ok("http://s4.anilist.co/a.png"), "https only");
        assert!(!banner_url_ok("https://evil.example/a.png"));
        assert!(!banner_url_ok("https://s4.anilist.co.evil.example/a.png"));
        assert!(!banner_url_ok("https://s4.anilist.co/a.exe"));
        assert!(!banner_url_ok("https://user@s4.anilist.co/a.png"));
        assert!(!banner_url_ok("javascript:alert(1)"));
    }

    use crate::db::connect_memory;
    use serde_json::{json, Value};

    fn fixture(name: &str) -> Value {
        let text = match name {
            "safe" => include_str!("../../../tests/fixtures/anilist/character_safe.json"),
            _ => include_str!("../../../tests/fixtures/anilist/character_adult.json"),
        };
        serde_json::from_str::<Value>(text).expect("fixture")["data"].clone()
    }

    /// AniList mock: character 10 is safe, 11 is adult-linked, anything else is unknown.
    fn mock_api() -> SafeAniList {
        SafeAniList::with_fixture(|_, vars| match vars["id"].as_i64() {
            Some(10) => Ok(fixture("safe")),
            Some(11) => Ok(fixture("adult")),
            _ => Err(crate::anilist::AniError::NotFound),
        })
    }

    #[tokio::test]
    async fn stored_characters_are_rechecked_and_adult_ones_hidden() {
        let pool = connect_memory().await;
        db::create_profile(&pool, 1).await.expect("profile");
        for id in [10, 11] {
            db::add_character(&pool, 1, id).await.expect("add");
        }
        let entries = db::list_page(&pool, 1, None, 0, 10).await.expect("list");
        assert!(
            entries.iter().all(|e| !e.visible()),
            "unverified entries start hidden"
        );

        refresh_entries(&mock_api(), &pool, ContentPolicy::SAFE, &entries).await;
        let entries = db::list_page(&pool, 1, None, 0, 10).await.expect("list");
        let visible: Vec<i64> = entries
            .iter()
            .filter(|e| e.visible())
            .map(|e| e.character_id)
            .collect();
        assert_eq!(visible, vec![10], "the adult-linked character stays hidden");
        assert_eq!(entries[0].name(), "Safe Char");
        assert_eq!(entries[1].is_safe, Some(false));
    }

    #[tokio::test]
    async fn profile_renders_from_cache_when_anilist_is_down() {
        let pool = connect_memory().await;
        db::create_profile(&pool, 1).await.expect("profile");
        db::add_character(&pool, 1, 10).await.expect("add");
        let mut row = cache_row(
            &serde_json::from_value::<Character>(fixture("safe")["Character"].clone()).expect("c"),
            1,
        );
        row.fetched_at = 1; // very stale
        db::upsert_cache(&pool, &row).await.expect("cache");

        let down =
            SafeAniList::with_fixture(|_, _| Err(crate::anilist::AniError::Http("down".into())));
        let entries = db::list_page(&pool, 1, None, 0, 10).await.expect("list");
        refresh_entries(&down, &pool, ContentPolicy::SAFE, &entries).await;
        let entries = db::list_page(&pool, 1, None, 0, 10).await.expect("list");
        assert!(entries[0].visible(), "cached, verified data is still shown");
        assert_eq!(entries[0].name(), "Safe Char");
        let _ = json!(null);
    }
}
