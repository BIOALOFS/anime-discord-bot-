//! Autocomplete over the caller's own groups.

use super::util::uid;
use crate::{db::profile as db, error::Context};
use poise::serenity_prelude as serenity;

pub async fn ac_group(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let needle = partial.trim().to_lowercase();
    match db::list_groups(&ctx.data().db, uid(ctx)).await {
        Ok(groups) => groups
            .into_iter()
            .filter(|g| g.name.to_lowercase().contains(&needle))
            .take(25)
            .map(|g| serenity::AutocompleteChoice::new(g.name.clone(), g.name))
            .collect(),
        Err(_) => Vec::new(),
    }
}
