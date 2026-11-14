//! `/random anime|manga|character`.

use super::{
    character::{CharacterView, Mode as CharMode},
    fail, policy, say_ephemeral,
};
use crate::{
    anilist::MediaKind,
    commands::media::{MediaView, Mode, Source},
    error::{Context, Error},
    i18n,
    ui::{autocomplete::to_choices, session},
};
use poise::serenity_prelude as serenity;
use std::time::Duration;

async fn ac_genre(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
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

/// Random picks.
#[poise::command(
    slash_command,
    subcommands("random_anime", "random_manga", "random_character"),
    rename = "random",
    name_localized("ar", "عشوائي"),
    description_localized("ar", "اختيار عشوائي")
)]
pub async fn random(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

async fn random_media(
    ctx: Context<'_>,
    kind: MediaKind,
    genre: Option<String>,
    min_score: Option<u8>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let api = ctx.data().anilist.clone();
    let policy = policy(ctx);
    match api
        .random_media(kind, genre.as_deref(), min_score, policy)
        .await
    {
        Ok(Some(m)) => {
            let view = MediaView::new(
                api,
                policy,
                Source::Random {
                    kind,
                    genre,
                    min_score,
                },
                Mode::Card(m),
            );
            session::run(ctx, view).await
        }
        Ok(None) => say_ephemeral(ctx, i18n::RANDOM_FAILED).await,
        Err(e) => fail(ctx, e).await,
    }
}

/// Random anime.
#[poise::command(
    slash_command,
    rename = "anime",
    name_localized("ar", "أنمي"),
    description_localized("ar", "أنمي عشوائي")
)]
pub async fn random_anime(
    ctx: Context<'_>,
    #[description = "Genre"]
    #[description_localized("ar", "التصنيف")]
    #[name_localized("ar", "التصنيف")]
    #[autocomplete = "ac_genre"]
    genre: Option<String>,
    #[description = "Minimum score (0-100)"]
    #[description_localized("ar", "أدنى تقييم (0-100)")]
    #[name_localized("ar", "أدنى_تقييم")]
    #[min = 0]
    #[max = 100]
    min_score: Option<u8>,
) -> Result<(), Error> {
    random_media(ctx, MediaKind::Anime, genre, min_score).await
}

/// Random manga.
#[poise::command(
    slash_command,
    rename = "manga",
    name_localized("ar", "مانغا"),
    description_localized("ar", "مانغا عشوائية")
)]
pub async fn random_manga(
    ctx: Context<'_>,
    #[description = "Genre"]
    #[description_localized("ar", "التصنيف")]
    #[name_localized("ar", "التصنيف")]
    #[autocomplete = "ac_genre"]
    genre: Option<String>,
    #[description = "Minimum score (0-100)"]
    #[description_localized("ar", "أدنى تقييم (0-100)")]
    #[name_localized("ar", "أدنى_تقييم")]
    #[min = 0]
    #[max = 100]
    min_score: Option<u8>,
) -> Result<(), Error> {
    random_media(ctx, MediaKind::Manga, genre, min_score).await
}

/// Random character (genre and score filters do not apply to characters).
#[poise::command(
    slash_command,
    rename = "character",
    name_localized("ar", "شخصية"),
    description_localized("ar", "شخصية عشوائية")
)]
pub async fn random_character(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let api = ctx.data().anilist.clone();
    let policy = policy(ctx);
    match api.random_character(policy).await {
        Ok(Some(c)) => session::run(ctx, CharacterView::new(api, policy, CharMode::Card(c))).await,
        Ok(None) => say_ephemeral(ctx, i18n::RANDOM_FAILED).await,
        Err(e) => fail(ctx, e).await,
    }
}
