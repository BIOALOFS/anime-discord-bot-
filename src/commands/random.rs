//! `/random type genre min_score`: a random safe work or character, today's spotlight or birthdays.

use super::{
    character::{CharacterView, Mode as CharMode},
    fail,
    media::{Mode, Source},
    policy,
    profile::option_str,
    say_ephemeral,
};
use crate::{
    anilist::MediaKind,
    error::{Context, Error},
    i18n,
    ui::{autocomplete::to_choices, session},
};
use poise::serenity_prelude as serenity;
use std::{
    hash::{Hash, Hasher},
    time::Duration,
};

pub fn random_types() -> Vec<(&'static str, &'static str)> {
    vec![
        ("أنمي", "anime"),
        ("مانجا", "manga"),
        ("مانهوا", "manhwa"),
        ("مانهوا صينية", "manhua"),
        ("رواية", "novel"),
        ("شخصية", "character"),
        ("شخصية اليوم", "daily"),
        ("أعياد ميلاد اليوم", "birthdays"),
    ]
}

async fn ac_type(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    let needle = partial.trim().to_lowercase();
    to_choices(
        random_types()
            .into_iter()
            .filter(|(l, k)| needle.is_empty() || l.contains(&needle) || k.contains(&needle))
            .map(|(l, k)| (l.to_string(), k.to_string()))
            .collect(),
    )
}

async fn ac_genre(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    // Genres only matter for works.
    if matches!(
        option_str(ctx, "type").as_deref(),
        Some("character" | "daily" | "birthdays")
    ) {
        return Vec::new();
    }
    let api = ctx.data().anilist.clone();
    let needle = partial.trim().to_lowercase();
    let genres = tokio::time::timeout(Duration::from_millis(2500), api.genres())
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    to_choices(
        genres
            .into_iter()
            .filter(|g| g.to_lowercase().contains(&needle))
            .map(|g| (g.clone(), g))
            .collect(),
    )
}

/// Same value for everyone in the same server on the same UTC day.
pub fn daily_seed(guild: Option<u64>, day: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (guild.unwrap_or(0), day).hash(&mut h);
    h.finish()
}

/// A random safe pick, today's spotlight, or today's character birthdays.
#[poise::command(
    slash_command,
    name_localized("ar", "عشوائي"),
    description_localized("ar", "اختيار عشوائي: عمل أو شخصية أو شخصية اليوم أو أعياد الميلاد")
)]
pub async fn random(
    ctx: Context<'_>,
    #[description = "What to pick"]
    #[description_localized("ar", "نوع الاختيار")]
    #[name_localized("ar", "النوع")]
    #[autocomplete = "ac_type"]
    r#type: String,
    #[description = "Genre (works only)"]
    #[description_localized("ar", "التصنيف (للأعمال فقط)")]
    #[name_localized("ar", "التصنيف")]
    #[autocomplete = "ac_genre"]
    genre: Option<String>,
    #[description = "Minimum score 0-100 (works only)"]
    #[description_localized("ar", "أدنى تقييم 0-100 (للأعمال فقط)")]
    #[name_localized("ar", "أدنى_تقييم")]
    #[min = 0]
    #[max = 100]
    min_score: Option<u8>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let api = ctx.data().anilist.clone();
    let pol = policy(ctx);
    let (db, user) = (ctx.data().db.clone(), ctx.author().id.get() as i64);
    let character_view =
        |c| CharacterView::new(api.clone(), pol, CharMode::Card(c), db.clone(), user);

    match r#type.trim() {
        "character" => match api.random_character(pol).await {
            Ok(Some(c)) => session::run(ctx, character_view(c)).await,
            Ok(None) => say_ephemeral(ctx, i18n::RANDOM_FAILED).await,
            Err(e) => fail(ctx, e).await,
        },
        "daily" => {
            let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
            match api
                .daily_character(daily_seed(ctx.guild_id().map(|g| g.get()), &day), pol)
                .await
            {
                Ok(Some(c)) => session::run(ctx, character_view(c)).await,
                Ok(None) => say_ephemeral(ctx, i18n::RANDOM_FAILED).await,
                Err(e) => fail(ctx, e).await,
            }
        }
        "birthdays" => match api.birthdays_today(pol).await {
            Ok(list) if list.is_empty() => say_ephemeral(ctx, i18n::NO_BIRTHDAYS).await,
            Ok(mut list) if list.len() == 1 => {
                session::run(ctx, character_view(list.remove(0))).await
            }
            Ok(list) => {
                session::run(
                    ctx,
                    CharacterView::new(api.clone(), pol, CharMode::Pick(list), db.clone(), user),
                )
                .await
            }
            Err(e) => fail(ctx, e).await,
        },
        key => {
            let kind = MediaKind::from_key(key).unwrap_or(MediaKind::Anime);
            match api
                .random_media(kind, genre.as_deref(), min_score, pol)
                .await
            {
                Ok(Some(m)) => {
                    let view = super::media::MediaView::new(
                        ctx,
                        Source::Random {
                            kind,
                            genre,
                            min_score,
                        },
                        Mode::Card(Box::new(m)),
                    )
                    .await;
                    session::run(ctx, view).await
                }
                Ok(None) => say_ephemeral(ctx, i18n::RANDOM_FAILED).await,
                Err(e) => fail(ctx, e).await,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_daily_pick_is_stable_per_server_and_day() {
        let a = daily_seed(Some(1), "2026-11-15");
        assert_eq!(a, daily_seed(Some(1), "2026-11-15"));
        assert_ne!(a, daily_seed(Some(2), "2026-11-15"), "different server");
        assert_ne!(a, daily_seed(Some(1), "2026-11-16"), "different day");
        assert_eq!(daily_seed(None, "d"), daily_seed(None, "d"));
    }

    #[test]
    fn random_types_cover_works_characters_and_extras() {
        let keys: Vec<_> = random_types().into_iter().map(|(_, k)| k).collect();
        for k in [
            "anime",
            "manga",
            "manhwa",
            "manhua",
            "novel",
            "character",
            "daily",
            "birthdays",
        ] {
            assert!(keys.contains(&k), "{k}");
        }
    }
}
