//! `/profile group create|rename|delete|assign`.

use super::util::{begin_private, own_or_reply, pool, private_reply, require_own, uid};
use crate::{
    db::profile::{self as db, ProfileError},
    error::{Context, Error},
    i18n,
};

pub(super) async fn group_create(ctx: Context<'_>, name: String) -> Result<(), Error> {
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

pub(super) async fn group_rename(
    ctx: Context<'_>,
    group: String,
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

pub(super) async fn group_delete(ctx: Context<'_>, group: String) -> Result<(), Error> {
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

pub(super) async fn group_assign(
    ctx: Context<'_>,
    character: String,
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

/// Put a favourite work in a group ('-' removes it from its group).
pub(super) async fn group_assign_work(
    ctx: Context<'_>,
    work: &str,
    group: &str,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let Some((id, _)) = super::util::resolve_own_work(&pool(ctx), uid(ctx), work).await? else {
        return private_reply(ctx, i18n::WORK_NOT_IN_LIST).await;
    };
    let group_id = if group.trim() == "-" {
        None
    } else {
        match db::find_group(&pool(ctx), uid(ctx), group).await? {
            Some(g) => Some(g.id),
            None => return private_reply(ctx, i18n::GROUP_NOT_FOUND).await,
        }
    };
    crate::db::works::assign_group(&pool(ctx), uid(ctx), &id, group_id).await?;
    private_reply(ctx, i18n::GROUP_ASSIGNED).await
}
