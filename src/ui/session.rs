//! Interactive message loop: owner-only components, 3-minute inactivity timeout that
//! disables the components, silent handling of interaction failures.

use crate::{error::Context, error::Error, i18n};
use poise::serenity_prelude as serenity;
use serenity::{
    ComponentInteractionCollector, ComponentInteractionDataKind, CreateActionRow, CreateAttachment,
    CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage,
    EditInteractionResponse,
};
use std::{future::Future, time::Duration};

pub const TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone, Default)]
pub struct Page {
    pub content: Option<String>,
    pub embeds: Vec<CreateEmbed>,
    pub components: Vec<CreateActionRow>,
    /// Attached images; an edit always replaces the previous attachments.
    pub files: Vec<CreateAttachment>,
}

impl Page {
    #[allow(dead_code)] // used by the image styles (collage, podium, tier)
    pub fn with_files(mut self, files: Vec<CreateAttachment>) -> Self {
        self.files = files;
        self
    }

    pub fn embed(embed: CreateEmbed, components: Vec<CreateActionRow>) -> Self {
        Self {
            content: None,
            embeds: vec![embed],
            components,
            files: Vec::new(),
        }
    }

    pub fn notice(mut self, text: Option<String>) -> Self {
        self.content = text;
        self
    }

    pub fn into_reply(self) -> poise::CreateReply {
        let mut r = poise::CreateReply::default().content(self.content.unwrap_or_default());
        for e in self.embeds {
            r = r.embed(e);
        }
        for f in self.files {
            r = r.attachment(f);
        }
        r.components(self.components)
    }

    fn into_edit(self) -> EditInteractionResponse {
        let mut e = EditInteractionResponse::new()
            .content(self.content.unwrap_or_default())
            .embeds(self.embeds)
            .components(self.components)
            .clear_attachments();
        for f in self.files {
            e = e.new_attachment(f);
        }
        e
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Re-render the message.
    Update,
    /// Nothing changed; just acknowledge.
    Keep,
    /// Disable the components and end the session.
    Close,
}

pub trait View: Send {
    fn render(&self, enabled: bool) -> Page;
    fn on_interaction(
        &mut self,
        id: &str,
        values: &[String],
    ) -> impl Future<Output = Outcome> + Send;
}

/// Wait for the next press by `owner` on `message`; others get an ephemeral refusal.
/// `None` once `TIMEOUT` passes without an owner interaction.
async fn next_owner_interaction(
    ctx: &serenity::Context,
    message: serenity::MessageId,
    owner: serenity::UserId,
) -> Option<serenity::ComponentInteraction> {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let remaining = deadline.checked_duration_since(tokio::time::Instant::now())?;
        let i = ComponentInteractionCollector::new(ctx)
            .message_id(message)
            .timeout(remaining)
            .await?;
        if i.user.id == owner {
            return Some(i);
        }
        let refusal = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content(i18n::NOT_YOURS)
                .ephemeral(true),
        );
        if let Err(e) = i.create_response(ctx, refusal).await {
            tracing::debug!("could not answer non-owner interaction: {e}");
        }
    }
}

/// Send the view and drive it until it closes or times out.
pub async fn run<V: View>(ctx: Context<'_>, mut view: V) -> Result<(), Error> {
    let handle = ctx.send(view.render(true).into_reply()).await?;
    let message_id = handle.message().await?.id;
    let sctx = ctx.serenity_context();
    let owner = ctx.author().id;

    while let Some(i) = next_owner_interaction(sctx, message_id, owner).await {
        // Acknowledge first so slow AniList calls never hit Discord's 3 s limit.
        if let Err(e) = i
            .create_response(sctx, CreateInteractionResponse::Acknowledge)
            .await
        {
            tracing::debug!("could not acknowledge interaction: {e}");
            continue;
        }
        let values = match &i.data.kind {
            ComponentInteractionDataKind::StringSelect { values } => values.clone(),
            _ => Vec::new(),
        };
        let outcome = view.on_interaction(&i.data.custom_id, &values).await;
        if outcome != Outcome::Keep {
            let page = view.render(outcome != Outcome::Close).into_edit();
            if let Err(e) = i.edit_response(sctx, page).await {
                // e.g. the message was deleted: stop quietly.
                tracing::debug!("could not edit message: {e}");
                return Ok(());
            }
        }
        if outcome == Outcome::Close {
            return Ok(());
        }
    }
    if let Err(e) = handle.edit(ctx, view.render(false).into_reply()).await {
        tracing::debug!("could not disable components after timeout: {e}");
    }
    Ok(())
}
