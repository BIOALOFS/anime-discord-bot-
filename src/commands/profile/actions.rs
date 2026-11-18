//! Profile actions used by `/profile` and `/edit`: adding and removing works, uploads, and data deletion.

use super::{
    imaging,
    util::{
        begin_private, now, own_or_reply, pool, private_reply, require_own, resolve_own_work,
        resolve_work, uid, ResolvedWork,
    },
};
use crate::{
    anilist::kind_of,
    commands::{buttons, media::media_cache_row, policy},
    db::{
        custom::{self, StoreOutcome},
        profile::{self as pdb, AddOutcome},
        works,
    },
    error::{Context, Error},
    i18n, render,
    ui::session::{self, Outcome, Page, View},
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::{sync::OnceLock, time::Duration};

const UPLOAD_COOLDOWN: Duration = Duration::from_secs(20);

// ---- works ------------------------------------------------------------------------------

pub(super) async fn add_work(ctx: Context<'_>, value: &str) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    // A key from a suggestion that is not an AniList id: a TMDB show or film, a game or a visual novel.
    if crate::components::is_work_key(value.trim()) && !value.trim().starts_with("anilist:") {
        let text = crate::global::add(
            ctx.data(),
            value.trim(),
            uid(ctx),
            super::super::fallback::game_flags(ctx).await.1,
        )
        .await;
        return private_reply(ctx, text).await;
    }
    let api = ctx.data().anilist.clone();
    match resolve_work(&api, policy(ctx), value).await {
        Err(e) => super::manage::fail_private(ctx, e).await,
        Ok(ResolvedWork::NotFound) => private_reply(ctx, i18n::NO_RESULTS).await,
        Ok(ResolvedWork::Many(list)) => {
            let names: Vec<String> = list
                .iter()
                .take(5)
                .map(|m| format!("• {}", m.title.main()))
                .collect();
            private_reply(
                ctx,
                format!("{}\n{}", i18n::WORK_AMBIGUOUS, names.join("\n")),
            )
            .await
        }
        Ok(ResolvedWork::One(m)) => {
            let pool = pool(ctx);
            works::upsert_media_cache(&pool, &media_cache_row(&m, now())).await?;
            let key = works::work_key(m.id);
            let text = match works::add_work(&pool, uid(ctx), &key, kind_of(&m).key()).await? {
                AddOutcome::Added(pos) => i18n::added_character(m.title.main(), pos),
                AddOutcome::Duplicate => i18n::WORK_ALREADY.to_string(),
            };
            private_reply(ctx, text).await
        }
    }
}

async fn own_work_or_reply(
    ctx: Context<'_>,
    value: &str,
) -> Result<Option<(String, String)>, Error> {
    match resolve_own_work(&pool(ctx), uid(ctx), value).await? {
        Some(w) => Ok(Some(w)),
        None => {
            private_reply(ctx, i18n::WORK_NOT_IN_LIST).await?;
            Ok(None)
        }
    }
}

pub(super) async fn remove_work(ctx: Context<'_>, value: &str) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some((id, title)) = own_work_or_reply(ctx, value).await? else {
        return Ok(());
    };
    works::remove_work(&pool(ctx), uid(ctx), &id).await?;
    private_reply(ctx, i18n::removed_character(&title)).await
}

pub(super) async fn move_work(ctx: Context<'_>, value: &str, position: i64) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some((id, title)) = own_work_or_reply(ctx, value).await? else {
        return Ok(());
    };
    let to = works::move_work(&pool(ctx), uid(ctx), &id, position).await?;
    private_reply(ctx, i18n::moved_character(&title, to)).await
}

// ---- lists --------------------------------------------------------------------------------

// ---- customization -------------------------------------------------------------------------

pub(super) async fn reset_character(ctx: Context<'_>, value: &str) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some((id, name)) = own_or_reply(ctx, value).await? else {
        return Ok(());
    };
    custom::reset_character(&pool(ctx), uid(ctx), id).await?;
    private_reply(ctx, i18n::custom_reset(&name)).await
}

fn upload_limiter() -> &'static moka::sync::Cache<i64, ()> {
    static LIMIT: OnceLock<moka::sync::Cache<i64, ()>> = OnceLock::new();
    LIMIT.get_or_init(|| {
        moka::sync::Cache::builder()
            .max_capacity(10_000)
            .time_to_live(UPLOAD_COOLDOWN)
            .build()
    })
}

/// Only Discord's own CDN is ever fetched (attachments only, never URLs typed as text).
fn discord_cdn(url: &str) -> bool {
    url.starts_with("https://cdn.discordapp.com/")
        || url.starts_with("https://media.discordapp.net/")
}

/// Checks shared by every upload: a server, custom images switched on, upload policy, not banned.
/// Replies privately and returns `None` when the upload must not go on.
async fn upload_gate(ctx: Context<'_>) -> Result<Option<i64>, Error> {
    let Some(guild) = ctx.guild_id().map(|g| g.get() as i64) else {
        private_reply(ctx, i18n::GUILD_ONLY).await?;
        return Ok(None);
    };
    let (pool, user) = (pool(ctx), uid(ctx));
    if !custom::custom_images_enabled(&pool, guild).await? {
        private_reply(ctx, i18n::CUSTOM_IMAGES_OFF).await?;
        return Ok(None);
    }
    if custom::is_banned(&pool, guild, user).await? {
        private_reply(ctx, i18n::UPLOAD_BANNED).await?;
        return Ok(None);
    }
    if crate::db::admin::upload_admins_only(&pool, guild).await?
        && !crate::guild::is_admin(ctx.author_member().await.and_then(|m| m.permissions))
    {
        private_reply(ctx, i18n::UPLOAD_ADMINS_ONLY).await?;
        return Ok(None);
    }
    Ok(Some(guild))
}

/// Download an attachment (Discord's CDN only) and check its size and type. Replies and returns `None` on a problem.
async fn fetch_upload(
    ctx: Context<'_>,
    att: &serenity::Attachment,
) -> Result<Option<Vec<u8>>, Error> {
    let user = uid(ctx);
    if att.size as usize > imaging::MAX_BYTES || !discord_cdn(&att.url) {
        private_reply(ctx, i18n::IMAGE_TOO_LARGE).await?;
        return Ok(None);
    }
    if upload_limiter().contains_key(&user) {
        private_reply(ctx, i18n::UPLOAD_SLOW_DOWN).await?;
        return Ok(None);
    }
    upload_limiter().insert(user, ());
    let bytes = match ctx.data().http.get(&att.url).send().await {
        Ok(r) if r.status().is_success() => r.bytes().await.ok(),
        _ => None,
    };
    let Some(bytes) = bytes else {
        private_reply(ctx, i18n::IMAGE_DOWNLOAD_FAILED).await?;
        return Ok(None);
    };
    if !imaging::is_supported(&bytes) {
        private_reply(ctx, i18n::IMAGE_INVALID).await?;
        return Ok(None);
    }
    Ok(Some(bytes.to_vec()))
}

/// Download an attachment (Discord's CDN only), validate and re-encode it. PNG, JPG, WebP and GIF are accepted.
pub(super) async fn take_upload(
    ctx: Context<'_>,
    att: &serenity::Attachment,
) -> Result<Option<imaging::Processed>, Error> {
    let Some(bytes) = fetch_upload(ctx, att).await? else {
        return Ok(None);
    };
    match tokio::task::spawn_blocking(move || imaging::process(&bytes)).await {
        Ok(Ok(p)) => Ok(Some(p)),
        Ok(Err(imaging::ImageError::TooLarge)) => {
            private_reply(ctx, i18n::IMAGE_TOO_LARGE).await?;
            Ok(None)
        }
        Ok(Err(_)) | Err(_) => {
            private_reply(ctx, i18n::IMAGE_INVALID).await?;
            Ok(None)
        }
    }
}

/// Same for a banner: at least 600 px wide; pictures are center-cropped, GIFs need a ratio of 2:1 to 4:1.
async fn take_banner(
    ctx: Context<'_>,
    att: &serenity::Attachment,
) -> Result<Option<imaging::Processed>, Error> {
    let Some(bytes) = fetch_upload(ctx, att).await? else {
        return Ok(None);
    };
    let reply = match tokio::task::spawn_blocking(move || imaging::process_banner(&bytes)).await {
        Ok(Ok(p)) => return Ok(Some(p)),
        Ok(Err(imaging::BannerError::TooNarrow)) => i18n::BANNER_TOO_NARROW,
        Ok(Err(imaging::BannerError::BadRatio)) => i18n::BANNER_BAD_RATIO,
        Ok(Err(imaging::BannerError::Image(imaging::ImageError::TooLarge))) => {
            i18n::IMAGE_TOO_LARGE
        }
        Ok(Err(_)) | Err(_) => i18n::IMAGE_INVALID,
    };
    private_reply(ctx, reply).await?;
    Ok(None)
}

pub(super) fn new_image(p: imaging::Processed) -> custom::NewImage {
    custom::NewImage {
        hash: p.hash,
        mime: p.mime.to_string(),
        bytes: p.bytes,
        width: p.width,
        height: p.height,
        animated: p.animated,
    }
}

/// Custom character image: attachment only, character must already be in the caller's profile (so GIFs are only
/// possible for own characters), the server must allow custom images, and the picture is validated and re-encoded.
pub(super) async fn character_image(
    ctx: Context<'_>,
    value: &str,
    image: Option<serenity::Attachment>,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    if upload_gate(ctx).await?.is_none() {
        return Ok(());
    }
    let (pool, user) = (pool(ctx), uid(ctx));
    let Some(att) = image else {
        return private_reply(ctx, i18n::IMAGE_REQUIRED).await;
    };
    let Some((character, name)) = own_or_reply(ctx, value).await? else {
        return Ok(());
    };
    let Some(processed) = take_upload(ctx, &att).await? else {
        return Ok(());
    };
    let new = new_image(processed);
    let id = match custom::store_image(&pool, user, &new, custom::MAX_IMAGES_PER_USER).await? {
        StoreOutcome::Stored(id) | StoreOutcome::Existing(id) => id,
        StoreOutcome::LimitReached => return private_reply(ctx, i18n::IMAGE_LIMIT).await,
    };
    custom::set_character_image(&pool, user, character, id).await?;
    private_reply(ctx, i18n::custom_image_saved(&name, new.animated)).await
}

/// Profile banner from an uploaded picture or GIF (same rules as character images). Stored as a custom image of the caller.
pub(super) async fn banner_image(
    ctx: Context<'_>,
    image: serenity::Attachment,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    if upload_gate(ctx).await?.is_none() {
        return Ok(());
    }
    let (pool, user) = (pool(ctx), uid(ctx));
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let Some(processed) = take_banner(ctx, &image).await? else {
        return Ok(());
    };
    // A static picture was cropped for the banner: show the crop and let the member confirm. A GIF is kept whole.
    if !processed.animated {
        return super::banner_picker::confirm(ctx, processed).await;
    }
    let id = match custom::store_image(
        &pool,
        user,
        &new_image(processed),
        custom::MAX_IMAGES_PER_USER,
    )
    .await?
    {
        StoreOutcome::Stored(id) | StoreOutcome::Existing(id) => id,
        StoreOutcome::LimitReached => return private_reply(ctx, i18n::IMAGE_LIMIT).await,
    };
    pdb::set_banner(&pool, user, Some(&render::BannerSpec::Image(id).encode())).await?;
    private_reply(ctx, i18n::BANNER_SET).await
}

// ---- sharing --------------------------------------------------------------------------------

// ---- deleting everything --------------------------------------------------------------------

struct DeleteView {
    pool: SqlitePool,
    user: i64,
    text: String,
}

impl View for DeleteView {
    fn render(&self, enabled: bool) -> Page {
        let row = buttons(vec![
            serenity::CreateButton::new("confirm")
                .label(i18n::BTN_DELETE_ALL)
                .style(serenity::ButtonStyle::Danger)
                .disabled(!enabled),
            serenity::CreateButton::new("cancel")
                .label(i18n::BTN_CANCEL)
                .style(serenity::ButtonStyle::Secondary)
                .disabled(!enabled),
        ]);
        Page {
            content: Some(self.text.clone()),
            embeds: vec![],
            components: vec![row],
            files: vec![],
            v2: None,
        }
    }

    async fn on_interaction(&mut self, id: &str, _values: &[String]) -> Outcome {
        match id {
            "confirm" => {
                self.text = match pdb::delete_user_data(&self.pool, self.user).await {
                    Ok(()) => i18n::DATA_DELETED.to_string(),
                    Err(e) => {
                        tracing::warn!("delete my data failed: {e}");
                        i18n::ERR_GENERIC.to_string()
                    }
                };
                Outcome::Close
            }
            "cancel" => {
                self.text = i18n::CANCELLED.to_string();
                Outcome::Close
            }
            _ => Outcome::Keep,
        }
    }
}

/// "Delete all my data": asks for confirmation, then erases everything stored about the user.
pub(super) async fn delete_data(ctx: Context<'_>) -> Result<(), Error> {
    begin_private(ctx).await?;
    session::run(
        ctx,
        DeleteView {
            pool: pool(ctx),
            user: uid(ctx),
            text: i18n::DELETE_CONFIRM.to_string(),
        },
    )
    .await
}
