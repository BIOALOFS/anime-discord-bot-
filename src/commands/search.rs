//! `/search type name`: anime, manga, manhwa, manhua, novel or character.

use super::{
    character::{ac_character, search_character},
    media::{media_autocomplete, run_media},
    profile::option_str,
};
use crate::{
    anilist::MediaKind,
    error::{Context, Error},
    ui::autocomplete::to_choices,
};
use poise::serenity_prelude as serenity;

/// `(label, key)` pairs for the `type` option; keys are `MediaKind::key()` or "character".
pub fn search_types() -> Vec<(&'static str, &'static str)> {
    vec![
        ("أنمي", "anime"),
        ("مانجا", "manga"),
        ("مانهوا", "manhwa"),
        ("مانهوا صينية", "manhua"),
        ("رواية", "novel"),
        ("شخصية", "character"),
    ]
}

async fn ac_type(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    let needle = partial.trim().to_lowercase();
    to_choices(
        search_types()
            .into_iter()
            .filter(|(l, k)| needle.is_empty() || l.contains(&needle) || k.contains(&needle))
            .map(|(l, k)| (l.to_string(), k.to_string()))
            .collect(),
    )
}

/// The name suggestions depend on the chosen type.
async fn ac_name(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    match option_str(ctx, "type").as_deref() {
        Some("character") => ac_character(ctx, partial).await,
        Some(key) => match MediaKind::from_key(key) {
            Some(kind) => media_autocomplete(ctx, kind, kind.key(), partial).await,
            None => Vec::new(),
        },
        None => media_autocomplete(ctx, MediaKind::Anime, "anime", partial).await,
    }
}

/// Search for an anime, manga, manhwa, manhua, novel or character.
#[poise::command(
    slash_command,
    name_localized("ar", "بحث"),
    description_localized("ar", "ابحث عن أنمي أو مانجا أو مانهوا أو رواية أو شخصية")
)]
pub async fn search(
    ctx: Context<'_>,
    #[description = "What to search for"]
    #[description_localized("ar", "نوع البحث")]
    #[name_localized("ar", "النوع")]
    #[autocomplete = "ac_type"]
    r#type: String,
    #[description = "Name"]
    #[description_localized("ar", "الاسم")]
    #[name_localized("ar", "الاسم")]
    #[autocomplete = "ac_name"]
    name: String,
) -> Result<(), Error> {
    if r#type.trim() == "character" {
        return search_character(ctx, name).await;
    }
    let kind = MediaKind::from_key(r#type.trim()).unwrap_or(MediaKind::Anime);
    run_media(ctx, kind, name).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_search_type_maps_to_a_kind_or_character() {
        for (label, key) in search_types() {
            assert!(!label.is_empty());
            assert!(
                key == "character" || MediaKind::from_key(key).is_some(),
                "{key}"
            );
        }
        assert_eq!(search_types().len(), 6);
    }
}
