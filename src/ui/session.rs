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
    /// The same page as Components V2 JSON (see `ui::v2`). When present and V2 works, it is sent instead of the
    /// embeds and `content`; otherwise the embeds are the fallback.
    pub v2: Option<Vec<serde_json::Value>>,
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
            v2: None,
        }
    }

    pub fn with_v2(mut self, components: Vec<serde_json::Value>) -> Self {
        self.v2 = Some(components);
        self
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

    /// Shared sessions (games) accept presses from every member; only `on_user_interaction` tells them apart.
    fn shared(&self) -> bool {
        false
    }

    /// A press by `who` (`admin`: Manage Guild). The default ignores who it was.
    fn on_user_interaction(
        &mut self,
        _who: serenity::UserId,
        _admin: bool,
        id: &str,
        values: &[String],
    ) -> impl Future<Output = Outcome> + Send {
        self.on_interaction(id, values)
    }

    /// Time until the view wants `on_tick` (round timers, progressive hints); `None` = no timer.
    fn tick_after(&self) -> Option<Duration> {
        None
    }

    /// Called when `tick_after` passed without a press.
    fn on_tick(&mut self) -> impl Future<Output = Outcome> + Send {
        async { Outcome::Keep }
    }

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

/// What `next_interaction` returned.
enum Wait {
    Press(serenity::ComponentInteraction),
    /// `wait` passed without a press.
    Quiet,
}

/// Wait up to `wait` for the next press on `message`. With `owner` set, others get an ephemeral refusal and the wait
/// goes on; shared sessions pass `None` and accept everyone.
async fn next_interaction(
    ctx: &serenity::Context,
    message: serenity::MessageId,
    owner: Option<serenity::UserId>,
    wait: Duration,
) -> Wait {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        let Some(remaining) = deadline.checked_duration_since(tokio::time::Instant::now()) else {
            return Wait::Quiet;
        };
        let Some(i) = ComponentInteractionCollector::new(ctx).message_id(message).timeout(remaining).await else {
            return Wait::Quiet;
        };
        if owner.is_none_or(|o| i.user.id == o) {
            return Wait::Press(i);
        }
        let refusal = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().content(i18n::NOT_YOURS).ephemeral(true),
        );
        if let Err(e) = i.create_response(ctx, refusal).await {
            tracing::debug!("could not answer non-owner interaction: {e}");
        }
    }
}

/// Send a page as Components V2 through the original (deferred) response; returns the message id.
async fn present_v2(ctx: Context<'_>, page: &Page, ephemeral: bool) -> Result<serenity::MessageId, String> {
    let Context::Application(app) = ctx else {
        return Err("not an application command".into());
    };
    let comps = page.v2.clone().ok_or("no V2 body")?;
    super::v2::validate(&comps)?;
    if ephemeral {
        ctx.defer_ephemeral().await.map_err(|e| e.to_string())?;
    } else {
        ctx.defer().await.map_err(|e| e.to_string())?;
    }
    let payload = super::v2::payload(comps, &page.files);
    let msg = ctx
        .serenity_context()
        .http
        .edit_original_interaction_response(&app.interaction.token, &payload, page.files.clone())
        .await
        .map_err(|e| e.to_string())?;
    Ok(msg.id)
}

/// Edit the message a component sits on with a V2 page (the interaction was acknowledged first).
async fn edit_v2(sctx: &serenity::Context, token: &str, page: &Page) -> Result<(), String> {
    let comps = page.v2.clone().ok_or("no V2 body")?;
    super::v2::validate(&comps)?;
    let payload = super::v2::payload(comps, &page.files);
    sctx.http.edit_original_interaction_response(token, &payload, page.files.clone()).await.map(|_| ()).map_err(|e| e.to_string())
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

/// Send the view and drive it until it closes or times out. Pages with a Components V2 body are sent as one
/// container; when Discord refuses that, the embed version of the same page is used for the whole session.
pub async fn run<V: View>(ctx: Context<'_>, mut view: V) -> Result<(), Error> {
    view.on_start().await;
    let sctx = ctx.serenity_context();
    let first = view.render(true);
    let mut use_v2 = first.v2.is_some() && matches!(ctx, Context::Application(_));
    let mut message_id = None;
    let mut legacy_handle = None;
    if use_v2 {
        match present_v2(ctx, &first, view.ephemeral()).await {
            Ok(id) => message_id = Some(id),
            Err(e) => {
                tracing::warn!("Components V2 refused, using embeds: {e}");
                use_v2 = false;
            }
        }
    }
    if !use_v2 {
        let reply = view.render(true).into_reply().ephemeral(view.ephemeral());
        let handle = ctx.send(reply).await?;
        message_id = Some(handle.message().await?.id);
        legacy_handle = Some(handle);
    }
    let Some(message_id) = message_id else { return Ok(()) };
    let owner = ctx.author().id;
    let shared = view.shared();
    let mut last_activity = tokio::time::Instant::now();

    loop {
        let idle_left = TIMEOUT.saturating_sub(last_activity.elapsed());
        if idle_left.is_zero() {
            break;
        }
        let wait = view.tick_after().map_or(idle_left, |t| t.min(idle_left));
        let i = match next_interaction(sctx, message_id, (!shared).then_some(owner), wait).await {
            Wait::Press(i) => i,
            Wait::Quiet => {
                // A round timer fired (not the inactivity timeout): let the view advance.
                if view.tick_after().is_some_and(|t| t <= wait) {
                    let outcome = view.on_tick().await;
                    if outcome != Outcome::Keep {
                        let page = view.render(outcome != Outcome::Close);
                        let sent = if use_v2 && page.v2.is_some() {
                            // Ticks have no interaction token of their own: use the command's.
                            match ctx {
                                Context::Application(app) => edit_v2(sctx, &app.interaction.token, &page).await.is_ok(),
                                _ => false,
                            }
                        } else {
                            match &legacy_handle {
                                Some(h) => h.edit(ctx, page.into_reply().ephemeral(view.ephemeral())).await.is_ok(),
                                None => false,
                            }
                        };
                        if !sent {
                            tracing::debug!("could not edit the message after a timer");
                        }
                    }
                    if outcome == Outcome::Close {
                        return Ok(());
                    }
                    continue;
                }
                break;
            }
        };
        last_activity = tokio::time::Instant::now();
        // The channel lock also applies to buttons, selects and modals of older messages.
        let admin = crate::guild::is_admin(i.member.as_ref().and_then(|m| m.permissions));
        if let Some(text) = crate::guild::denial(&ctx.data().guilds, &sctx.http, i.guild_id, i.channel_id, admin).await {
            let refusal = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content(text).ephemeral(true),
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
            if answer.interaction.create_response(sctx, CreateInteractionResponse::Acknowledge).await.is_err() {
                continue;
            }
            let input = answer.inputs.first().cloned().unwrap_or_default();
            let outcome = view.on_modal(&i.data.custom_id, input).await;
            if outcome != Outcome::Keep {
                let page = view.render(outcome != Outcome::Close);
                let sent = if use_v2 && page.v2.is_some() {
                    edit_v2(sctx, &answer.interaction.token, &page).await.is_ok()
                } else {
                    answer.interaction.edit_response(sctx, page.into_edit()).await.is_ok()
                };
                if !sent {
                    tracing::debug!("could not edit message after modal");
                    return Ok(());
                }
            }
            if outcome == Outcome::Close {
                return Ok(());
            }
            continue;
        }
        // Acknowledge first so slow AniList calls never hit Discord's 3 s limit.
        if let Err(e) = i.create_response(sctx, CreateInteractionResponse::Acknowledge).await {
            tracing::debug!("could not acknowledge interaction: {e}");
            continue;
        }
        let values = match &i.data.kind {
            ComponentInteractionDataKind::StringSelect { values } => values.clone(),
            ComponentInteractionDataKind::RoleSelect { values } => values.iter().map(|v| v.get().to_string()).collect(),
            ComponentInteractionDataKind::ChannelSelect { values } => values.iter().map(|v| v.get().to_string()).collect(),
            _ => Vec::new(),
        };
        let outcome = view.on_user_interaction(i.user.id, admin, &i.data.custom_id, &values).await;
        if let Outcome::Whisper(text) = &outcome {
            send_whisper(sctx, &i, Whisper::text(text.clone()), i.user.id).await;
            continue;
        }
        if outcome != Outcome::Keep {
            let page = view.render(outcome != Outcome::Close);
            let sent = if use_v2 && page.v2.is_some() {
                edit_v2(sctx, &i.token, &page).await.is_ok()
            } else {
                i.edit_response(sctx, page.into_edit()).await.is_ok()
            };
            if !sent {
                // e.g. the message was deleted: stop quietly.
                tracing::debug!("could not edit message");
                return Ok(());
            }
        }
        if let Some(w) = view.take_whisper() {
            send_whisper(sctx, &i, w, i.user.id).await;
        }
        if outcome == Outcome::Close {
            return Ok(());
        }
    }
    let page = view.render(false);
    if use_v2 && page.v2.is_some() {
        if let Context::Application(app) = ctx {
            if let Err(e) = edit_v2(sctx, &app.interaction.token, &page).await {
                tracing::debug!("could not disable components after timeout: {e}");
            }
        }
    } else if let Some(h) = legacy_handle {
        if let Err(e) = h.edit(ctx, page.into_reply().ephemeral(view.ephemeral())).await {
            tracing::debug!("could not disable components after timeout: {e}");
        }
    }
    Ok(())
}
