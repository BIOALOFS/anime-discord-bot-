use crate::{i18n, Data};
use poise::FrameworkError;

pub type Error = anyhow::Error;
pub type Context<'a> = poise::Context<'a, Data, Error>;

/// Log details with tracing, show the user a short Arabic message.
pub async fn on_error(err: FrameworkError<'_, Data, Error>) {
    match err {
        FrameworkError::Command { error, ctx, .. } => {
            tracing::error!(
                command = %ctx.command().name,
                user = %ctx.author().id,
                guild = ?ctx.guild_id(),
                "command failed: {error:#}"
            );
            // Ephemeral even when the command had deferred publicly.
            let _ = crate::commands::say_ephemeral(ctx, i18n::ERR_GENERIC).await;
        }
        FrameworkError::CooldownHit { ctx, .. } => {
            let _ = ctx
                .send(
                    poise::CreateReply::default()
                        .content(i18n::ERR_COOLDOWN)
                        .ephemeral(true),
                )
                .await;
        }
        // The channel check already told the user why.
        FrameworkError::CommandCheckFailed { .. } => {}
        FrameworkError::MissingUserPermissions { ctx, .. } => {
            let _ = ctx
                .send(
                    poise::CreateReply::default()
                        .content(i18n::ERR_MISSING_PERMS)
                        .ephemeral(true),
                )
                .await;
        }
        other => {
            if let Err(e) = poise::builtins::on_error(other).await {
                tracing::error!("error handler failed: {e}");
            }
        }
    }
}
