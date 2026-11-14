use crate::{error::Context, i18n};

/// Health check.
#[poise::command(
    slash_command,
    name_localized("ar", "بينغ"),
    description_localized("ar", "فحص سرعة استجابة البوت")
)]
pub async fn ping(ctx: Context<'_>) -> Result<(), crate::error::Error> {
    ctx.say(i18n::PING_REPLY).await?;
    Ok(())
}
