//! The two standalone admin-only channel commands. Each has a single `channel` option and nothing else.
//! Unsetting a channel is done from the `/admin` panel.

use super::{admin::invoker_can_manage, say_ephemeral};
use crate::{
    db::feed,
    error::{Context, Error},
    i18n,
};
use poise::serenity_prelude as serenity;

/// Permissions the bot needs in a channel it posts in.
pub fn missing_permissions(p: serenity::Permissions) -> Vec<&'static str> {
    let mut out = Vec::new();
    if !p.view_channel() {
        out.push("عرض القناة");
    }
    if !p.send_messages() {
        out.push("إرسال الرسائل");
    }
    if !p.embed_links() {
        out.push("تضمين الروابط");
    }
    if !p.attach_files() {
        out.push("إرفاق الملفات");
    }
    out
}

/// Text channels only: voice, forum and category ids are refused.
pub fn is_postable(kind: serenity::ChannelType) -> bool {
    matches!(
        kind,
        serenity::ChannelType::Text | serenity::ChannelType::News
    )
}

/// Common checks; `Ok(Some(guild))` means "go ahead". Every refusal is ephemeral.
async fn validate(
    ctx: Context<'_>,
    channel: &serenity::GuildChannel,
) -> Result<Option<i64>, Error> {
    let Some(g) = ctx.guild_id() else {
        say_ephemeral(ctx, i18n::SERVER_ONLY).await?;
        return Ok(None);
    };
    if !invoker_can_manage(ctx).await {
        ctx.send(
            poise::CreateReply::default()
                .content(i18n::NOT_ADMIN)
                .ephemeral(true),
        )
        .await?;
        return Ok(None);
    }
    if channel.guild_id != g || !is_postable(channel.kind) {
        ctx.send(
            poise::CreateReply::default()
                .content(i18n::CHANNEL_NOT_TEXT)
                .ephemeral(true),
        )
        .await?;
        return Ok(None);
    }
    let bot = ctx.framework().bot_id;
    let member = g.member(ctx.http(), bot).await?;
    // Compute in a block: the cache guard must not live across an await.
    let perms = ctx
        .guild()
        .map(|guild| guild.user_permissions_in(channel, &member));
    let missing = perms.map(missing_permissions).unwrap_or_default();
    if !missing.is_empty() {
        ctx.send(
            poise::CreateReply::default()
                .content(i18n::channel_missing_permissions(
                    channel.id.get(),
                    &missing,
                ))
                .ephemeral(true),
        )
        .await?;
        return Ok(None);
    }
    Ok(Some(g.get() as i64))
}

/// The channel where new episode notifications are posted.
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "MANAGE_GUILD",
    rename = "set-notifications-channel",
    name_localized("ar", "تحديد-قناة-الإشعارات"),
    description_localized("ar", "القناة التي تُنشر فيها إشعارات الحلقات الجديدة (للمشرفين)")
)]
pub async fn set_notifications_channel(
    ctx: Context<'_>,
    #[description = "Notifications channel"]
    #[description_localized("ar", "قناة الإشعارات")]
    #[name_localized("ar", "القناة")]
    #[channel_types("Text", "News")]
    channel: serenity::GuildChannel,
) -> Result<(), Error> {
    let Some(g) = validate(ctx, &channel).await? else {
        return Ok(());
    };
    feed::set_channel(&ctx.data().db, g, channel.id.get() as i64).await?;
    ctx.send(
        poise::CreateReply::default()
            .content(i18n::notifications_channel_set(channel.id.get()))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

/// The one channel where the bot's commands work.
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "MANAGE_GUILD",
    rename = "set-bot-channel",
    name_localized("ar", "تحديد-قناة-البوت"),
    description_localized("ar", "القناة الوحيدة التي تعمل فيها أوامر البوت (للمشرفين)")
)]
pub async fn set_bot_channel(
    ctx: Context<'_>,
    #[description = "Bot channel"]
    #[description_localized("ar", "قناة البوت")]
    #[name_localized("ar", "القناة")]
    #[channel_types("Text", "News")]
    channel: serenity::GuildChannel,
) -> Result<(), Error> {
    let Some(g) = validate(ctx, &channel).await? else {
        return Ok(());
    };
    ctx.data()
        .guilds
        .set_command_channel(g, channel.id.get() as i64)
        .await?;
    ctx.send(
        poise::CreateReply::default()
            .content(i18n::bot_channel_set(channel.id.get()))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serenity::Permissions as P;

    #[test]
    fn all_four_permissions_are_required() {
        let all = P::VIEW_CHANNEL | P::SEND_MESSAGES | P::EMBED_LINKS | P::ATTACH_FILES;
        assert!(missing_permissions(all).is_empty());
        assert_eq!(missing_permissions(P::empty()).len(), 4);
        assert_eq!(
            missing_permissions(all - P::ATTACH_FILES),
            vec!["إرفاق الملفات"]
        );
        assert_eq!(
            missing_permissions(all - P::EMBED_LINKS - P::SEND_MESSAGES).len(),
            2
        );
    }

    #[test]
    fn only_text_like_channels_are_postable() {
        assert!(is_postable(serenity::ChannelType::Text));
        assert!(is_postable(serenity::ChannelType::News));
        assert!(!is_postable(serenity::ChannelType::Voice));
        assert!(!is_postable(serenity::ChannelType::Category));
        assert!(!is_postable(serenity::ChannelType::Forum));
    }
}
