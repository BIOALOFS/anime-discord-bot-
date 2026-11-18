pub mod admin;
pub mod admin_ext;
pub mod airing;
pub mod arena;
pub mod arena_tmdb;
pub mod category;
pub mod channels;
pub mod character;
pub mod event;
pub mod fallback;
pub mod help;
pub mod media;
pub mod notify_panel;
pub mod picks;
pub mod profile;
pub mod recommend;
pub mod roster;
pub mod schedule;
pub mod settings;
pub mod suggest;
pub mod tmdb_view;
pub mod top;

use crate::{
    anilist::{AniError, ContentPolicy},
    error::{Context, Error},
    i18n,
};
use poise::serenity_prelude as serenity;

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
    /// Every select menu built with a constant id must be handled in the same file, otherwise choosing an option does
    /// nothing (this was the `/recommend` bug). Scans the sources.
    #[test]
    fn every_select_menu_has_a_handler_for_its_id() {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).expect("dir").flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs")
                    && !p.ends_with("tests.rs")
                    && !p.ends_with("ui/select.rs")
                {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let mut checked = 0;
        for f in files {
            let text = std::fs::read_to_string(&f).expect("read");
            // Only the production part of the file.
            let prod = text.split("#[cfg(test)]").next().unwrap_or(&text);
            let mut rest = prod;
            while let Some(i) = rest.find("select_row(") {
                rest = &rest[i + "select_row(".len()..];
                let arg: String = rest
                    .chars()
                    .skip_while(|c| c.is_whitespace())
                    .take_while(|c| c.is_ascii_uppercase() || *c == '_' || c.is_ascii_digit())
                    .collect();
                if arg.len() < 3 {
                    continue;
                }
                checked += 1;
                let handled = prod.contains(&format!("{arg} =>"))
                    || prod.contains(&format!("{arg} |"))
                    || prod.contains(&format!("| {arg}"))
                    || prod.contains(&format!("== {arg}"))
                    || prod.contains(&format!("{arg}) ="));
                assert!(
                    handled,
                    "{}: select menu id {arg} has no handler",
                    f.display()
                );
            }
        }
        assert!(checked >= 15, "the scan found only {checked} select menus");
    }

    use super::*;

    #[test]
    fn every_error_shown_to_users_is_arabic() {
        let has_arabic = |s: &str| s.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c));
        for e in [
            AniError::Busy,
            AniError::NotFound,
            AniError::Http("connection reset by peer".into()),
            AniError::Api("GraphQL: Max query complexity".into()),
        ] {
            let msg = ani_message(&e);
            assert!(has_arabic(msg), "{e:?} -> {msg}");
            assert!(
                !msg.contains("connection") && !msg.contains("GraphQL"),
                "raw error text must not leak"
            );
        }
        assert!(has_arabic(i18n::ERR_GENERIC));
    }

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
