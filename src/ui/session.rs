//! Interactive message loop: owner-only components, 3-minute inactivity timeout that
//! disables the components, silent handling of interaction failures.

use crate::{error::Context, error::Error, i18n};
use poise::serenity_prelude as serenity;
use serenity::{
    ComponentInteractionCollector, ComponentInteractionDataKind, CreateActionRow, CreateAttachment,
    CreateEmbed, CreateInteractionResponse, CreateInteractionResponseFollowup,
    CreateInteractionResponseMessage, CreateQuickModal, EditInteractionResponse,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Re-render the message.
    Update,
    /// Nothing changed; just acknowledge.
    Keep,
    /// Disable the components and end the session.
    Close,
    /// Leave the message as is and show this text to the presser only (ephemeral follow-up).
    Whisper(String),
}

/// A one-field modal a button can open (e.g. "go to page").
#[derive(Debug, Clone)]
pub struct ModalSpec {
    pub title: String,
    pub label: String,
}

pub type UndoFuture = futures::future::BoxFuture<'static, String>;

/// An ephemeral follow-up to the presser, optionally with an "undo" button (valid for a minute).
pub struct Whisper {
    pub text: String,
    pub undo: Option<Box<dyn FnOnce() -> UndoFuture + Send + Sync>>,
}

impl Whisper {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            undo: None,
        }
    }
}

pub trait View: Send {
    /// Taken by the engine after every interaction.
    fn take_whisper(&mut self) -> Option<Whisper> {
        None
    }

    fn render(&self, enabled: bool) -> Page;

    /// Runs once before the first render (load covers, check profile state...).
    fn on_start(&mut self) -> impl Future<Output = ()> + Send {
        async {}
    }

    /// Panels that only their opener should ever see (admin menus) are sent ephemerally.
    fn ephemeral(&self) -> bool {
        false
    }

    /// Buttons that open a modal instead of a normal update. The modal must be the interaction's first
    /// response, so the engine asks before acknowledging.
    fn modal_for(&self, _id: &str) -> Option<ModalSpec> {
        None
    }

    fn on_modal(&mut self, _id: &str, _input: String) -> impl Future<Output = Outcome> + Send {
        async { Outcome::Keep }
    }

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

async fn send_whisper(
    sctx: &serenity::Context,
    i: &serenity::ComponentInteraction,
    whisper: Whisper,
    owner: serenity::UserId,
) {
    let mut followup = CreateInteractionResponseFollowup::new()
        .content(&whisper.text)
        .ephemeral(true);
    if whisper.undo.is_some() {
        followup = followup.components(vec![CreateActionRow::Buttons(vec![
            serenity::CreateButton::new("undo")
                .label(i18n::BTN_UNDO)
                .emoji('↩')
                .style(serenity::ButtonStyle::Secondary),
        ])]);
    }
    let msg = match i.create_followup(sctx, followup).await {
        Ok(m) => m,
        Err(e) => {
            tracing::debug!("could not send ephemeral follow-up: {e}");
            return;
        }
    };
    if let Some(undo) = whisper.undo {
        let sctx = sctx.clone();
        tokio::spawn(async move {
            let press = ComponentInteractionCollector::new(&sctx)
                .message_id(msg.id)
                .author_id(owner)
                .timeout(Duration::from_secs(60))
                .await;
            if let Some(press) = press {
                let text = undo().await;
                let update = CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .content(text)
                        .components(vec![]),
                );
                if let Err(e) = press.create_response(&sctx, update).await {
                    tracing::debug!("could not confirm undo: {e}");
                }
            }
        });
    }
}

/// Send the view and drive it until it closes or times out.
pub async fn run<V: View>(ctx: Context<'_>, mut view: V) -> Result<(), Error> {
    view.on_start().await;
    let reply = view.render(true).into_reply().ephemeral(view.ephemeral());
    let handle = ctx.send(reply).await?;
    let message_id = handle.message().await?.id;
    let sctx = ctx.serenity_context();
    let owner = ctx.author().id;

    while let Some(i) = next_owner_interaction(sctx, message_id, owner).await {
        // The channel lock also applies to buttons, selects and modals of older messages.
        let admin = crate::guild::is_admin(i.member.as_ref().and_then(|m| m.permissions));
        if let Some(text) = crate::guild::denial(
            &ctx.data().guilds,
            &sctx.http,
            i.guild_id,
            i.channel_id,
            admin,
        )
        .await
        {
            let refusal = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(text)
                    .ephemeral(true),
            );
            if let Err(e) = i.create_response(sctx, refusal).await {
                tracing::debug!("could not send channel refusal: {e}");
            }
            continue;
        }
        if let Some(spec) = view.modal_for(&i.data.custom_id) {
            let builder = CreateQuickModal::new(spec.title)
                .timeout(Duration::from_secs(120))
                .short_field(spec.label);
            let Ok(Some(answer)) = i.quick_modal(sctx, builder).await else {
                continue;
            };
            if answer
                .interaction
                .create_response(sctx, CreateInteractionResponse::Acknowledge)
                .await
                .is_err()
            {
                continue;
            }
            let input = answer.inputs.first().cloned().unwrap_or_default();
            let outcome = view.on_modal(&i.data.custom_id, input).await;
            if outcome != Outcome::Keep {
                let page = view.render(outcome != Outcome::Close).into_edit();
                if let Err(e) = answer.interaction.edit_response(sctx, page).await {
                    tracing::debug!("could not edit message after modal: {e}");
                    return Ok(());
                }
            }
            if outcome == Outcome::Close {
                return Ok(());
            }
            continue;
        }
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
            ComponentInteractionDataKind::RoleSelect { values } => {
                values.iter().map(|v| v.get().to_string()).collect()
            }
            ComponentInteractionDataKind::ChannelSelect { values } => {
                values.iter().map(|v| v.get().to_string()).collect()
            }
            _ => Vec::new(),
        };
        let outcome = view.on_interaction(&i.data.custom_id, &values).await;
        if let Outcome::Whisper(text) = &outcome {
            send_whisper(sctx, &i, Whisper::text(text.clone()), owner).await;
            continue;
        }
        if outcome != Outcome::Keep {
            let page = view.render(outcome != Outcome::Close).into_edit();
            if let Err(e) = i.edit_response(sctx, page).await {
                // e.g. the message was deleted: stop quietly.
                tracing::debug!("could not edit message: {e}");
                return Ok(());
            }
        }
        if let Some(w) = view.take_whisper() {
            send_whisper(sctx, &i, w, owner).await;
        }
        if outcome == Outcome::Close {
            return Ok(());
        }
    }
    if let Err(e) = handle
        .edit(
            ctx,
            view.render(false).into_reply().ephemeral(view.ephemeral()),
        )
        .await
    {
        tracing::debug!("could not disable components after timeout: {e}");
    }
    Ok(())
}
