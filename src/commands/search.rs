//! `/search`: one command over every category. The name suggestions mix works, characters and people (see
//! `crate::search`); the submit opens the picked item directly, or lists what a typed name found.

use super::{profile::option_str, say_ephemeral};
use crate::{
    error::{Context, Error},
    i18n,
    people::fetch::Sources,
    search::{self, Cat, Params, Ty},
    ui::autocomplete::{to_choices, MIN_CHARS},
};
use poise::serenity_prelude as serenity;

async fn overrides(
    ctx: Context<'_>,
) -> std::collections::HashMap<String, crate::tmdb::model::Target> {
    match ctx.guild_id() {
        Some(g) => crate::db::notify::drama_overrides(&ctx.data().db, g.get() as i64)
            .await
            .unwrap_or_default(),
        None => Default::default(),
    }
}

/// Suggestions that follow the category and type already chosen (both default to "all").
pub async fn ac_name(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    let cat = option_str(ctx, "category")
        .and_then(|k| Cat::from_key(&k))
        .unwrap_or(Cat::All);
    let ty = option_str(ctx, "type")
        .and_then(|k| Ty::from_key(&k))
        .unwrap_or(Ty::All);
    let (games_on, mature) = super::fallback::game_flags(ctx).await;
    let p = Params {
        cat,
        ty,
        mature,
        games_on,
        // Autocomplete may not wait for a wiki: it reads the local index only.
        live_wikis: false,
    };
    let src = Sources::of(ctx.data());
    let overrides = overrides(ctx).await;
    let scope = format!("{}|{}|{games_on}|{mature}", cat.key(), ty.key());
    let items =
        ctx.data()
            .auto
            .items_scoped(
                ctx.author().id.get(),
                "search",
                &scope,
                partial,
                MIN_CHARS,
                |q| async move {
                    search::choices(&search::gather(&src, &overrides, p, &q).await.entries)
                },
            )
            .await;
    to_choices(items)
}

/// General search across every category.
#[poise::command(
    slash_command,
    name_localized("ar", "بحث"),
    description_localized("ar", "بحث عام في كل الأقسام")
)]
pub async fn search(
    ctx: Context<'_>,
    #[description = "Name of a work, character or person"]
    #[description_localized("ar", "اسم عمل أو شخصية أو شخص")]
    #[name_localized("ar", "الاسم")]
    #[autocomplete = "ac_name"]
    name: String,
    #[description = "Category (default: all)"]
    #[description_localized("ar", "القسم (الافتراضي: الكل)")]
    #[name_localized("ar", "القسم")]
    category: Option<Cat>,
    #[description = "Type (default: all)"]
    #[description_localized("ar", "النوع (الافتراضي: الكل)")]
    #[name_localized("ar", "النوع")]
    r#type: Option<Ty>,
) -> Result<(), Error> {
    let name = name.trim().to_string();
    if name.is_empty() {
        ctx.defer_ephemeral().await?;
        return say_ephemeral(ctx, i18n::SEARCH_NEEDS_NAME).await;
    }
    ctx.defer().await?;
    let msg = crate::global::search_or_open(
        ctx.data(),
        ctx.guild_id().map(|g| g.get() as i64),
        ctx.author().id.get() as i64,
        category.unwrap_or(Cat::All),
        r#type.unwrap_or(Ty::All),
        &name,
    )
    .await;
    crate::global::reply(ctx, msg).await
}
