//! Profile actions that are not plain character management: works, lists, customization, templates,
//! sharing and data deletion. Each is called by the `/profile` dispatcher.

use super::{
    imaging,
    items::Section,
    util::{
        begin_private, begin_public, now, own_or_reply, pool, private_reply, require_own,
        resolve_own_work, resolve_work, uid, ResolvedWork,
    },
    view::ProfileView,
};
use crate::{
    anilist::kind_of,
    commands::{buttons, media::media_cache_row, policy},
    db::{
        custom::{self, StoreOutcome},
        profile::{self as pdb, AddOutcome, ProfileError},
        works,
    },
    error::{Context, Error},
    i18n,
    render::{self, CellSpec, HeaderSpec, Images},
    ui::{
        session::{self, Outcome, Page, View},
        styles::Style,
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::{sync::OnceLock, time::Duration};

const CLEAR_WORDS: &[&str] = &["-", "حذف", "مسح"];
const UPLOAD_COOLDOWN: Duration = Duration::from_secs(20);

fn is_clear(s: &str) -> bool {
    CLEAR_WORDS.contains(&s.trim())
}

// ---- works ------------------------------------------------------------------------------

pub(super) async fn add_work(ctx: Context<'_>, value: &str) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
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

pub(super) async fn rank_work(ctx: Context<'_>, value: &str, tier: &str) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some((id, title)) = own_work_or_reply(ctx, value).await? else {
        return Ok(());
    };
    if is_clear(tier) {
        works::set_tier(&pool(ctx), uid(ctx), &id, None).await?;
        return private_reply(ctx, i18n::tier_cleared(&title)).await;
    }
    works::set_tier(&pool(ctx), uid(ctx), &id, Some(tier)).await?;
    private_reply(ctx, i18n::tier_set(&title, tier.trim())).await
}

pub(super) async fn note_work(ctx: Context<'_>, value: &str, text: &str) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some((id, _)) = own_work_or_reply(ctx, value).await? else {
        return Ok(());
    };
    if is_clear(text) {
        works::set_note(&pool(ctx), uid(ctx), &id, None).await?;
        return private_reply(ctx, i18n::NOTE_CLEARED).await;
    }
    works::set_note(&pool(ctx), uid(ctx), &id, Some(text)).await?;
    private_reply(ctx, i18n::NOTE_SET).await
}

// ---- lists --------------------------------------------------------------------------------

/// `list`: "watched" / "planning" / None to take the work off every list.
pub(super) async fn set_list(
    ctx: Context<'_>,
    value: &str,
    list: Option<&str>,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some((id, title)) = own_work_or_reply(ctx, value).await? else {
        return Ok(());
    };
    works::set_list(&pool(ctx), uid(ctx), &id, list).await?;
    private_reply(ctx, i18n::list_updated(&title, list)).await
}

pub(super) async fn show_lists(ctx: Context<'_>) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let (pool, user) = (pool(ctx), uid(ctx));
    let mut out = Vec::new();
    for (key, label) in [
        ("watched", i18n::LIST_WATCHED),
        ("planning", i18n::LIST_PLANNING),
    ] {
        let entries = works::list_page(&pool, user, None, Some(key), 0, 15).await?;
        let total = works::count_filtered(&pool, user, None, Some(key)).await?;
        let names: Vec<String> = entries
            .iter()
            .filter(|e| e.visible())
            .map(|e| format!("• {}", e.title()))
            .collect();
        out.push(format!(
            "**{label} ({total})**\n{}",
            if names.is_empty() {
                i18n::NONE.to_string()
            } else {
                names.join("\n")
            }
        ));
    }
    private_reply(ctx, out.join("\n\n")).await
}

// ---- customization -------------------------------------------------------------------------

pub(super) async fn set_quote(ctx: Context<'_>, text: &str) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let clean = if is_clear(text) {
        None
    } else {
        custom::sanitize_text(text, pdb::MAX_QUOTE)
    };
    if clean.is_none() && !is_clear(text) {
        return private_reply(ctx, i18n::TEXT_REJECTED).await;
    }
    pdb::set_quote(&pool(ctx), uid(ctx), clean.as_deref()).await?;
    private_reply(ctx, i18n::QUOTE_SET).await
}

pub(super) async fn set_template(ctx: Context<'_>, value: &str) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    if !render::is_theme(value.trim()) {
        return private_reply(ctx, i18n::TEMPLATE_UNKNOWN).await;
    }
    pdb::set_template(&pool(ctx), uid(ctx), value.trim()).await?;
    private_reply(ctx, i18n::TEMPLATE_SET).await
}

/// Personal title (<= 40 chars), shown under the character's name in this user's profile only.
pub(super) async fn character_title(
    ctx: Context<'_>,
    value: &str,
    text: &str,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some((id, name)) = own_or_reply(ctx, value).await? else {
        return Ok(());
    };
    let new = if is_clear(text) { None } else { Some(text) };
    match custom::set_title(&pool(ctx), uid(ctx), id, new).await {
        Ok(()) => private_reply(ctx, i18n::custom_saved(&name)).await,
        Err(ProfileError::Invalid(_)) => private_reply(ctx, i18n::TEXT_REJECTED).await,
        Err(e) => Err(e.into()),
    }
}

/// Personal description (<= 300 chars), shown in the character's detail view only.
pub(super) async fn character_description(
    ctx: Context<'_>,
    value: &str,
    text: &str,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some((id, name)) = own_or_reply(ctx, value).await? else {
        return Ok(());
    };
    let new = if is_clear(text) { None } else { Some(text) };
    match custom::set_description(&pool(ctx), uid(ctx), id, new).await {
        Ok(()) => private_reply(ctx, i18n::custom_saved(&name)).await,
        Err(ProfileError::Invalid(_)) => private_reply(ctx, i18n::TEXT_REJECTED).await,
        Err(e) => Err(e.into()),
    }
}

pub(super) async fn reset_character(ctx: Context<'_>, value: &str) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some((id, name)) = own_or_reply(ctx, value).await? else {
        return Ok(());
    };
    custom::reset_character(&pool(ctx), uid(ctx), id).await?;
    private_reply(ctx, i18n::custom_reset(&name)).await
}

pub(super) async fn reset_character_image(ctx: Context<'_>, value: &str) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some((id, name)) = own_or_reply(ctx, value).await? else {
        return Ok(());
    };
    custom::clear_character_image(&pool(ctx), uid(ctx), id).await?;
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

/// Custom character image: attachment only, character must already be in the caller's profile, the server
/// admin must have enabled custom images, and the picture is validated and re-encoded by the bot.
pub(super) async fn character_image(
    ctx: Context<'_>,
    value: &str,
    image: Option<serenity::Attachment>,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some(guild) = ctx.guild_id().map(|g| g.get() as i64) else {
        return private_reply(ctx, i18n::GUILD_ONLY).await;
    };
    let (pool, user) = (pool(ctx), uid(ctx));
    if !custom::custom_images_enabled(&pool, guild).await? {
        return private_reply(ctx, i18n::CUSTOM_IMAGES_OFF).await;
    }
    if custom::is_banned(&pool, guild, user).await? {
        return private_reply(ctx, i18n::UPLOAD_BANNED).await;
    }
    let Some(att) = image else {
        return private_reply(ctx, i18n::IMAGE_REQUIRED).await;
    };
    let Some((character, name)) = own_or_reply(ctx, value).await? else {
        return Ok(());
    };
    if att.size as usize > imaging::MAX_BYTES || !discord_cdn(&att.url) {
        return private_reply(ctx, i18n::IMAGE_TOO_LARGE).await;
    }
    if upload_limiter().contains_key(&user) {
        return private_reply(ctx, i18n::UPLOAD_SLOW_DOWN).await;
    }
    upload_limiter().insert(user, ());
    let bytes = match ctx.data().http.get(&att.url).send().await {
        Ok(r) if r.status().is_success() => r.bytes().await.ok(),
        _ => None,
    };
    let Some(bytes) = bytes else {
        return private_reply(ctx, i18n::IMAGE_DOWNLOAD_FAILED).await;
    };
    let processed = match tokio::task::spawn_blocking(move || imaging::process(&bytes)).await {
        Ok(Ok(p)) => p,
        Ok(Err(imaging::ImageError::TooLarge)) => {
            return private_reply(ctx, i18n::IMAGE_TOO_LARGE).await
        }
        Ok(Err(_)) | Err(_) => return private_reply(ctx, i18n::IMAGE_INVALID).await,
    };
    let new = custom::NewImage {
        hash: processed.hash,
        mime: processed.mime.to_string(),
        bytes: processed.bytes,
        width: processed.width,
        height: processed.height,
        animated: processed.animated,
    };
    let id = match custom::store_image(&pool, user, &new, custom::MAX_IMAGES_PER_USER).await? {
        StoreOutcome::Stored(id) | StoreOutcome::Existing(id) => id,
        StoreOutcome::LimitReached => return private_reply(ctx, i18n::IMAGE_LIMIT).await,
    };
    custom::set_character_image(&pool, user, character, id).await?;
    private_reply(ctx, i18n::custom_image_saved(&name, new.animated)).await
}

// ---- sharing --------------------------------------------------------------------------------

pub(super) async fn share(ctx: Context<'_>, social_card: bool) -> Result<(), Error> {
    begin_public(ctx).await?;
    let Some(mut view) = open_own_view(ctx, Style::Collage).await? else {
        return Ok(());
    };
    view.load().await?;
    let mut page = if social_card {
        let theme = render::Theme::by_name(&view.profile.template);
        let mut extra = Images::new();
        let top = super::items::load_page(
            &view.pool,
            view.owner,
            Section::Characters,
            None,
            None,
            0,
            5,
        )
        .await?;
        let mut cells: Vec<CellSpec> = top
            .iter()
            .filter(|i| i.visible)
            .map(|i| CellSpec {
                label: i.name.clone(),
                subtitle: i.display_title().map(str::to_string),
                rank: (i.position <= 3).then_some(i.position as u8),
                image_url: i.image_url.clone(),
            })
            .collect();
        if cells.is_empty() {
            let w =
                super::items::load_page(&view.pool, view.owner, Section::Works, None, None, 0, 5)
                    .await?;
            cells = w
                .iter()
                .filter(|i| i.visible)
                .map(|i| CellSpec {
                    label: i.name.clone(),
                    subtitle: None,
                    rank: None,
                    image_url: i.image_url.clone(),
                })
                .collect();
        }
        let header = HeaderSpec {
            name: view.owner_name.clone(),
            badges: view.badges.iter().map(|b| b.letter).collect(),
            bio: view.profile.bio.clone(),
            count: view.counts.0 + view.counts.1,
            avatar_url: view.owner_avatar.clone(),
        };
        match ctx
            .data()
            .renderer
            .share(
                cells,
                Some(header),
                view.profile.accent_color.map(|c| c as u32),
                theme,
                std::mem::take(&mut extra),
            )
            .await
        {
            Some(img) => {
                let name = format!("profile-share.{}", img.ext);
                let embed = crate::ui::embeds::Emb::new()
                    .image(Some(format!("attachment://{name}")))
                    .colour(view.profile.accent_color.map(|c| c as u32))
                    .build();
                Page::embed(embed, vec![]).with_files(vec![serenity::CreateAttachment::bytes(
                    img.bytes.clone(),
                    name,
                )])
            }
            None => view.static_page(),
        }
    } else {
        view.static_page()
    };
    page.content = Some(i18n::SHARE_TITLE.to_string());
    ctx.send(page.into_reply()).await?;
    Ok(())
}

/// A view of the caller's own profile (created on first use), or `None` after replying.
pub(super) async fn open_own_view(
    ctx: Context<'_>,
    style: Style,
) -> Result<Option<ProfileView>, Error> {
    let Some(profile) = require_own(ctx).await? else {
        return Ok(None);
    };
    let author = ctx.author();
    Ok(Some(new_view(
        ctx,
        profile,
        author.id.get() as i64,
        author.display_name().to_string(),
        Some(author.face()),
        None,
        style,
        Section::Characters,
    )))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn new_view(
    ctx: Context<'_>,
    profile: pdb::Profile,
    owner: i64,
    owner_name: String,
    owner_avatar: Option<String>,
    group: Option<pdb::Group>,
    style: Style,
    section: Section,
) -> ProfileView {
    ProfileView {
        api: ctx.data().anilist.clone(),
        pool: ctx.data().db.clone(),
        policy: policy(ctx),
        renderer: ctx.data().renderer.clone(),
        viewer: uid(ctx),
        owner,
        owner_name,
        owner_avatar,
        guild: ctx.guild_id().map(|g| g.get() as i64),
        profile,
        group,
        section,
        style,
        page: 0,
        total: 0,
        counts: (0, 0),
        badges: Vec::new(),
        items: Vec::new(),
        image: None,
        image_label: String::new(),
        detail: None,
        roster: None,
        notice: None,
        whisper: None,
    }
}

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
