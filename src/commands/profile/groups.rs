//! `/profile group create|rename|delete|assign`.

use super::{
    groups_ac::ac_group,
    util::{ac_own, begin_private, own_or_reply, pool, private_reply, require_own, uid},
};
use crate::{
    db::profile::{self as db, ProfileError},
    error::{Context, Error},
    i18n,
};

/// Manage groups.
#[poise::command(
    slash_command,
    subcommands("group_create", "group_rename", "group_delete", "group_assign"),
    name_localized("ar", "مجموعة"),
    description_localized("ar", "إدارة مجموعات الشخصيات")
)]
pub async fn group(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Create a group.
#[poise::command(
    slash_command,
    rename = "create",
    name_localized("ar", "إنشاء"),
    description_localized("ar", "أنشئ مجموعة")
)]
pub async fn group_create(
    ctx: Context<'_>,
    #[description = "Group name"]
    #[description_localized("ar", "اسم المجموعة")]
    #[name_localized("ar", "الاسم")]
    name: String,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    match db::create_group(&pool(ctx), uid(ctx), &name).await {
        Ok(_) => private_reply(ctx, i18n::GROUP_CREATED).await,
        Err(ProfileError::Duplicate) => private_reply(ctx, i18n::GROUP_EXISTS).await,
        Err(ProfileError::Invalid(_)) => private_reply(ctx, i18n::GROUP_NAME_INVALID).await,
        Err(e) => Err(e.into()),
    }
}

/// Rename a group.
#[poise::command(
    slash_command,
    rename = "rename",
    name_localized("ar", "تسمية"),
    description_localized("ar", "غيّر اسم مجموعة")
)]
pub async fn group_rename(
    ctx: Context<'_>,
    #[description = "Group"]
    #[description_localized("ar", "المجموعة")]
    #[name_localized("ar", "المجموعة")]
    #[autocomplete = "ac_group"]
    group: String,
    #[description = "New name"]
    #[description_localized("ar", "الاسم الجديد")]
    #[name_localized("ar", "الاسم")]
    new_name: String,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let Some(g) = db::find_group(&pool(ctx), uid(ctx), &group).await? else {
        return private_reply(ctx, i18n::GROUP_NOT_FOUND).await;
    };
    match db::rename_group(&pool(ctx), uid(ctx), g.id, &new_name).await {
        Ok(()) => private_reply(ctx, i18n::GROUP_RENAMED).await,
        Err(ProfileError::Duplicate) => private_reply(ctx, i18n::GROUP_EXISTS).await,
        Err(ProfileError::Invalid(_)) => private_reply(ctx, i18n::GROUP_NAME_INVALID).await,
        Err(ProfileError::NotFound) => private_reply(ctx, i18n::GROUP_NOT_FOUND).await,
        Err(e) => Err(e.into()),
    }
}

/// Delete a group (its characters stay in your profile).
#[poise::command(
    slash_command,
    rename = "delete",
    name_localized("ar", "حذف"),
    description_localized("ar", "احذف مجموعة (تبقى شخصياتها)")
)]
pub async fn group_delete(
    ctx: Context<'_>,
    #[description = "Group"]
    #[description_localized("ar", "المجموعة")]
    #[name_localized("ar", "المجموعة")]
    #[autocomplete = "ac_group"]
    group: String,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let Some(g) = db::find_group(&pool(ctx), uid(ctx), &group).await? else {
        return private_reply(ctx, i18n::GROUP_NOT_FOUND).await;
    };
    db::delete_group(&pool(ctx), uid(ctx), g.id).await?;
    private_reply(ctx, i18n::GROUP_DELETED).await
}

/// Put a character in a group ('-' removes it from its group).
#[poise::command(
    slash_command,
    rename = "assign",
    name_localized("ar", "تعيين"),
    description_localized("ar", "ضع شخصية في مجموعة (- لإزالتها)")
)]
pub async fn group_assign(
    ctx: Context<'_>,
    #[description = "Character"]
    #[description_localized("ar", "الشخصية")]
    #[name_localized("ar", "الشخصية")]
    #[autocomplete = "ac_own"]
    character: String,
    #[description = "Group, or '-' for none"]
    #[description_localized("ar", "المجموعة، أو - بدون مجموعة")]
    #[name_localized("ar", "المجموعة")]
    #[autocomplete = "ac_group"]
    group: String,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let Some((id, _)) = own_or_reply(ctx, &character).await? else {
        return Ok(());
    };
    let group_id = if group.trim() == "-" {
        None
    } else {
        match db::find_group(&pool(ctx), uid(ctx), &group).await? {
            Some(g) => Some(g.id),
            None => return private_reply(ctx, i18n::GROUP_NOT_FOUND).await,
        }
    };
    db::assign_group(&pool(ctx), uid(ctx), id, group_id).await?;
    private_reply(ctx, i18n::GROUP_ASSIGNED).await
}
