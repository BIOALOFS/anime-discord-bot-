//! Adding and removing favourite characters.

use super::util::{
    begin_private, cache_row, now, own_or_reply, pool, private_reply, require_own,
    resolve_character, uid, Resolved,
};
use crate::{
    anilist::{models::Character, AniError},
    commands::policy,
    db::profile::{self as db, AddOutcome},
    error::{Context, Error},
    i18n,
    ui::{
        cards::picker_character_embed,
        pagination::{close_button, CLOSE},
        select::{character_choices, pick_row, PICK_ID},
        session::{self, Outcome, Page, View},
    },
};
use sqlx::SqlitePool;

pub(super) async fn fail_private(ctx: Context<'_>, e: AniError) -> Result<(), Error> {
    tracing::warn!(
        command = %ctx.command().name,
        user = %ctx.author().id,
        guild = ?ctx.guild_id(),
        "anilist error: {e}"
    );
    private_reply(ctx, crate::commands::ani_message(&e)).await
}

pub async fn add_and_cache(pool: &SqlitePool, user: i64, c: &Character) -> Result<String, Error> {
    db::upsert_cache(pool, &cache_row(c, now())).await?;
    Ok(match db::add_character(pool, user, c.id).await? {
        AddOutcome::Added(pos) => i18n::added_character(c.display_name(), pos),
        AddOutcome::Duplicate => i18n::ALREADY_ADDED.to_string(),
    })
}

// ---- create -----------------------------------------------------------------------------

// ---- add --------------------------------------------------------------------------------

struct PickAddView {
    pool: SqlitePool,
    user: i64,
    results: Vec<Character>,
    done: Option<String>,
}

impl View for PickAddView {
    fn render(&self, enabled: bool) -> Page {
        let rows = pick_row(character_choices(&self.results), enabled)
            .into_iter()
            .chain([crate::commands::buttons(vec![close_button(enabled)])])
            .collect();
        Page::embed(picker_character_embed(&self.results).build(), rows).notice(self.done.clone())
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        match id {
            CLOSE => Outcome::Close,
            PICK_ID => {
                let pick = values.first().and_then(|v| v.parse::<i64>().ok());
                let Some(c) = self.results.iter().find(|c| Some(c.id) == pick) else {
                    return Outcome::Keep;
                };
                self.done = Some(match add_and_cache(&self.pool, self.user, c).await {
                    Ok(text) => text,
                    Err(e) => {
                        tracing::warn!("could not add character: {e:#}");
                        i18n::ERR_GENERIC.to_string()
                    }
                });
                Outcome::Close
            }
            _ => Outcome::Keep,
        }
    }
}

pub(super) async fn add(ctx: Context<'_>, character: String) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    // A TMDB cast entry or a game / visual-novel character picked from a suggestion or a roster.
    let key = character.trim();
    if key.starts_with("tmdb-cast:") || key.starts_with("igdb:c") || key.starts_with("vndb:c") {
        let text = crate::global::add(ctx.data(), key, uid(ctx)).await;
        return private_reply(ctx, text).await;
    }
    let api = ctx.data().anilist.clone();
    match resolve_character(&api, policy(ctx), &character).await {
        Err(e) => fail_private(ctx, e).await,
        Ok(Resolved::One(c)) => {
            let text = add_and_cache(&pool(ctx), uid(ctx), &c).await?;
            private_reply(ctx, text).await
        }
        Ok(Resolved::Many(results)) => {
            session::run(
                ctx,
                PickAddView {
                    pool: pool(ctx),
                    user: uid(ctx),
                    results,
                    done: None,
                },
            )
            .await
        }
        Ok(Resolved::Hidden) => private_reply(ctx, i18n::CHARACTER_HIDDEN).await,
        Ok(Resolved::NotFound) => private_reply(ctx, i18n::NO_RESULTS).await,
    }
}

// ---- remove / clear / move --------------------------------------------------------------

pub(super) async fn remove(ctx: Context<'_>, character: String) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let Some((id, name)) = own_or_reply(ctx, &character).await? else {
        return Ok(());
    };
    db::remove_character(&pool(ctx), uid(ctx), id).await?;
    private_reply(ctx, i18n::removed_character(&name)).await
}

// ---- rank / note ------------------------------------------------------------------------

// ---- bio / colour / banner / style / privacy --------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use crate::db::connect_memory;
    use serde_json::Value;

    #[tokio::test]
    async fn add_and_cache_is_idempotent() {
        let pool = connect_memory().await;
        db::create_profile(&pool, 1).await.expect("profile");
        let c: Character = serde_json::from_value(
            serde_json::from_str::<Value>(include_str!(
                "../../../tests/fixtures/anilist/character_safe.json"
            ))
            .expect("fixture")["data"]["Character"]
                .clone(),
        )
        .expect("character");
        let first = add_and_cache(&pool, 1, &c).await.expect("first");
        assert!(first.contains("المركز 1"));
        assert_eq!(
            add_and_cache(&pool, 1, &c).await.expect("second"),
            i18n::ALREADY_ADDED
        );
        let e = db::get_entry(&pool, 1, 10).await.expect("e").expect("some");
        assert!(e.visible(), "an added character is cached as verified safe");
    }
}
