pub mod airing;
pub mod character;
pub mod media;
pub mod profile;
pub mod random;
pub mod seasonal;

use crate::{
    anilist::{AniError, ContentPolicy},
    error::{Context, Error},
    i18n,
};
use poise::serenity_prelude as serenity;

/// Health check.
#[poise::command(
    slash_command,
    name_localized("ar", "بينغ"),
    description_localized("ar", "فحص سرعة استجابة البوت")
)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say(i18n::PING_REPLY).await?;
    Ok(())
}

/// Content policy for the current guild. Always safe until `/settings` exists (milestone 8).
pub fn policy(_ctx: Context<'_>) -> ContentPolicy {
    ContentPolicy::SAFE
}

/// Autocomplete values look like `id:123` so that picking a suggestion skips a second search.
pub fn parse_id(input: &str) -> Option<i64> {
    input.trim().strip_prefix("id:")?.parse().ok()
}

pub fn ani_message(e: &AniError) -> &'static str {
    match e {
        AniError::Busy => i18n::ERR_BUSY,
        _ => i18n::ERR_SERVICE,
    }
}

/// Ephemeral message. After a public `defer` the placeholder is removed first,
/// because a deferred response keeps its public flag.
pub async fn say_ephemeral(ctx: Context<'_>, text: &str) -> Result<(), Error> {
    if let Context::Application(app) = ctx {
        // Fails harmlessly when nothing was deferred.
        let _ = app
            .interaction
            .delete_response(&ctx.serenity_context().http)
            .await;
    }
    ctx.send(poise::CreateReply::default().content(text).ephemeral(true))
        .await?;
    Ok(())
}

pub async fn fail(ctx: Context<'_>, e: AniError) -> Result<(), Error> {
    tracing::warn!(
        command = %ctx.command().name,
        user = %ctx.author().id,
        guild = ?ctx.guild_id(),
        "anilist error: {e}"
    );
    say_ephemeral(ctx, ani_message(&e)).await
}

pub fn buttons(list: Vec<serenity::CreateButton>) -> serenity::CreateActionRow {
    serenity::CreateActionRow::Buttons(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_come_from_autocomplete_values_only() {
        assert_eq!(parse_id("id:42"), Some(42));
        assert_eq!(parse_id(" id:7 "), Some(7));
        assert_eq!(
            parse_id("86"),
            None,
            "a title like '86' is a search, not an id"
        );
        assert_eq!(parse_id("id:abc"), None);
    }

    /// Commands and UI code must go through `SafeAniList`, never the raw client.
    #[test]
    fn nothing_outside_anilist_touches_the_raw_client() {
        fn scan(dir: &std::path::Path, banned: &[String], hits: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).expect("read dir").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    scan(&path, banned, hits);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).expect("read file");
                    for b in banned {
                        if text.contains(b.as_str())
                            && !text.contains("fn nothing_outside_anilist_touches_the_raw_client")
                        {
                            hits.push(format!("{} uses {b}", path.display()));
                        }
                    }
                }
            }
        }
        let banned = vec![
            ["AniList", "Client"].concat(),
            ["anilist", "::client"].concat(),
            ["anilist", "::queries"].concat(),
        ];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut hits = Vec::new();
        for dir in ["commands", "ui"] {
            scan(&root.join(dir), &banned, &mut hits);
        }
        assert!(hits.is_empty(), "raw client used: {hits:?}");
    }

    #[test]
    fn raw_client_is_not_exported() {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/anilist/mod.rs"),
        )
        .expect("read mod.rs");
        assert!(src.contains("mod client;") && !src.contains("pub mod client"));
        assert!(!src.contains(&["AniList", "Client"].concat()));
    }
}
