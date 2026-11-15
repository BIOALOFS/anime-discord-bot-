//! `/setup`: choose where the bot answers commands (Manage Guild).

use crate::{
    error::{Context, Error},
    guild::{ExtraResult, MAX_EXTRA_CHANNELS},
    i18n,
    ui::embeds::Emb,
};
use poise::serenity_prelude as serenity;

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum Toggle {
    #[name = "تشغيل"]
    #[name_localized("en-US", "On")]
    On,
    #[name = "إيقاف"]
    #[name_localized("en-US", "Off")]
    Off,
}

async fn reply(ctx: Context<'_>, text: impl Into<String>) -> Result<(), Error> {
    ctx.send(poise::CreateReply::default().content(text).ephemeral(true))
        .await?;
    Ok(())
}

fn guild_of(ctx: Context<'_>) -> Option<i64> {
    ctx.guild_id().map(|g| g.get() as i64)
}

/// Where the bot answers commands.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD",
    subcommands(
        "setup_channel",
        "add_channel",
        "remove_channel",
        "show",
        "reset",
        "admin_bypass"
    ),
    name_localized("ar", "إعداد"),
    description_localized("ar", "تحديد القناة التي يرد فيها البوت")
)]
pub async fn setup(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Set the only channel where the bot answers commands.
#[poise::command(
    slash_command,
    guild_only,
    rename = "channel",
    required_permissions = "MANAGE_GUILD",
    name_localized("ar", "قناة"),
    description_localized("ar", "حدد القناة الوحيدة التي يرد فيها البوت")
)]
pub async fn setup_channel(
    ctx: Context<'_>,
    #[description = "The command channel"]
    #[description_localized("ar", "قناة الأوامر")]
    #[name_localized("ar", "القناة")]
    #[channel_types("Text", "News")]
    channel: serenity::GuildChannel,
) -> Result<(), Error> {
    let Some(guild) = guild_of(ctx) else {
        return reply(ctx, i18n::GUILD_ONLY).await;
    };
    ctx.data()
        .guilds
        .set_command_channel(guild, channel.id.get() as i64)
        .await?;
    reply(ctx, i18n::setup_channel_set(channel.id.get() as i64)).await
}

/// Allow one more channel (up to 5).
#[poise::command(
    slash_command,
    guild_only,
    rename = "add-channel",
    required_permissions = "MANAGE_GUILD",
    name_localized("ar", "إضافة-قناة"),
    description_localized("ar", "اسمح بقناة إضافية (حتى 5)")
)]
pub async fn add_channel(
    ctx: Context<'_>,
    #[description = "Channel to allow"]
    #[description_localized("ar", "القناة المسموحة")]
    #[name_localized("ar", "القناة")]
    #[channel_types("Text", "News")]
    channel: serenity::GuildChannel,
) -> Result<(), Error> {
    let Some(guild) = guild_of(ctx) else {
        return reply(ctx, i18n::GUILD_ONLY).await;
    };
    let id = channel.id.get() as i64;
    match ctx.data().guilds.add_extra(guild, id).await? {
        ExtraResult::Added => reply(ctx, i18n::setup_extra_added(id)).await,
        ExtraResult::AlreadyThere => reply(ctx, i18n::SETUP_EXTRA_EXISTS).await,
        ExtraResult::Full => reply(ctx, i18n::setup_extra_full(MAX_EXTRA_CHANNELS)).await,
    }
}

/// Remove an extra allowed channel.
#[poise::command(
    slash_command,
    guild_only,
    rename = "remove-channel",
    required_permissions = "MANAGE_GUILD",
    name_localized("ar", "إزالة-قناة"),
    description_localized("ar", "أزل قناة إضافية من القائمة")
)]
pub async fn remove_channel(
    ctx: Context<'_>,
    #[description = "Channel to remove"]
    #[description_localized("ar", "القناة")]
    #[name_localized("ar", "القناة")]
    #[channel_types("Text", "News")]
    channel: serenity::GuildChannel,
) -> Result<(), Error> {
    let Some(guild) = guild_of(ctx) else {
        return reply(ctx, i18n::GUILD_ONLY).await;
    };
    let id = channel.id.get() as i64;
    if ctx.data().guilds.remove_extra(guild, id).await? {
        reply(ctx, i18n::setup_extra_removed(id)).await
    } else {
        reply(ctx, i18n::SETUP_EXTRA_MISSING).await
    }
}

/// Show the current settings.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    name_localized("ar", "عرض"),
    description_localized("ar", "اعرض الإعدادات الحالية")
)]
pub async fn show(ctx: Context<'_>) -> Result<(), Error> {
    let Some(guild) = guild_of(ctx) else {
        return reply(ctx, i18n::GUILD_ONLY).await;
    };
    let s = ctx.data().guilds.settings(guild).await?;
    let mut deleted = false;
    let main = match s.command_channel {
        Some(id) => {
            // Detect a deleted channel so admins are warned instead of silently unrestricted.
            let http = &ctx.serenity_context().http;
            if let Err(serenity::Error::Http(e)) =
                serenity::ChannelId::new(id as u64).to_channel(http).await
            {
                deleted = matches!(&e, serenity::HttpError::UnsuccessfulRequest(r) if r.status_code.as_u16() == 404);
            }
            format!("<#{id}>")
        }
        None => i18n::SETUP_NONE.to_string(),
    };
    let extras = if s.extra.is_empty() {
        i18n::NONE.to_string()
    } else {
        s.extra
            .iter()
            .map(|c| format!("<#{c}>"))
            .collect::<Vec<_>>()
            .join("، ")
    };
    let mut embed = Emb::new()
        .title(i18n::SETUP_TITLE)
        .field(i18n::SETUP_MAIN, main, false)
        .field(i18n::SETUP_EXTRA, extras, false)
        .field(
            i18n::SETUP_BYPASS,
            if s.admin_bypass {
                i18n::SETUP_ON
            } else {
                i18n::SETUP_OFF
            },
            true,
        );
    if deleted {
        embed = embed.description(i18n::SETUP_DELETED_WARNING);
    }
    ctx.send(
        poise::CreateReply::default()
            .embed(embed.build())
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

/// Remove the restriction: the bot answers everywhere again.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    name_localized("ar", "إلغاء"),
    description_localized("ar", "ألغِ القيد وارجع للعمل في كل القنوات")
)]
pub async fn reset(ctx: Context<'_>) -> Result<(), Error> {
    let Some(guild) = guild_of(ctx) else {
        return reply(ctx, i18n::GUILD_ONLY).await;
    };
    ctx.data().guilds.reset(guild).await?;
    reply(ctx, i18n::SETUP_RESET).await
}

/// Let admins use commands in any channel (default on).
#[poise::command(
    slash_command,
    guild_only,
    rename = "admin-bypass",
    required_permissions = "MANAGE_GUILD",
    name_localized("ar", "تجاوز-المشرفين"),
    description_localized("ar", "اسمح للمشرفين باستخدام البوت في أي قناة")
)]
pub async fn admin_bypass(
    ctx: Context<'_>,
    #[description = "On or off"]
    #[description_localized("ar", "تشغيل أو إيقاف")]
    #[name_localized("ar", "الحالة")]
    state: Toggle,
) -> Result<(), Error> {
    let Some(guild) = guild_of(ctx) else {
        return reply(ctx, i18n::GUILD_ONLY).await;
    };
    let on = matches!(state, Toggle::On);
    ctx.data().guilds.set_admin_bypass(guild, on).await?;
    reply(ctx, i18n::setup_bypass(on)).await
}
