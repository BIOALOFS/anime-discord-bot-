//! Interactive message loop: owner-only components, a sliding lifetime per message class (see `nav::Lifetimes`) after
//! which the components are disabled silently, edit-in-place navigation with a Back button, a private view for any
//! other member who presses a navigation button, and silent handling of interaction failures.

use crate::{error::Context, error::Error, i18n};
use poise::serenity_prelude as serenity;
use serenity::{
    ComponentInteractionCollector, ComponentInteractionDataKind, CreateActionRow, CreateAttachment,
    CreateEmbed, CreateInteractionResponse, CreateInteractionResponseFollowup,
    CreateInteractionResponseMessage, CreateQuickModal, EditInteractionResponse,
};
use std::{future::Future, time::Duration};

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

    pub(crate) fn into_edit(self) -> EditInteractionResponse {
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

/// A work card a view wants sent as a new ephemeral message (e.g. choosing a work in a suggestion).
pub struct CardFollowup {
    pub embed: CreateEmbed,
    pub files: Vec<CreateAttachment>,
    /// A link button under the card (the work's page).
    pub link: Option<(String, String)>,
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

    /// Taken after every press: a card to send as a new ephemeral message.
    fn take_card(&mut self) -> Option<CardFollowup> {
        None
    }

    /// Which lifetime this message gets (the profile has its own).
    fn class(&self) -> crate::nav::Class {
        crate::nav::Class::Default
    }

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
    Press(Box<serenity::ComponentInteraction>),
    /// A navigation button (the universal ones): the owner's edits the message, anyone else gets a private view.
    Global(Box<serenity::ComponentInteraction>),
    /// `wait` passed without a press.
    Quiet,
}

/// Wait up to `wait` for the next press on `message`. With `owner` set, others get an ephemeral refusal for the
/// message's own controls and the wait goes on; shared sessions pass `None` and accept everyone. Navigation buttons are
/// returned for everybody (what they do depends on who pressed).
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
        let Some(i) = ComponentInteractionCollector::new(ctx)
            .message_id(message)
            .timeout(remaining)
            .await
        else {
            return Wait::Quiet;
        };
        if crate::components::is_global_id(&i.data.custom_id) {
            return Wait::Global(Box::new(i));
        }
        if owner.is_none_or(|o| i.user.id == o) {
            return Wait::Press(Box::new(i));
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

/// What is on screen as far as the sweeper is concerned: the components as last rendered.
pub struct Shown {
    pub components: Vec<serde_json::Value>,
    pub v2: bool,
}

impl Shown {
    pub fn of(page: &Page, use_v2: bool) -> Self {
        match (&page.v2, use_v2) {
            (Some(v2), true) => Self {
                components: v2.clone(),
                v2: true,
            },
            _ => Self {
                components: serde_json::to_value(&page.components)
                    .ok()
                    .and_then(|v| v.as_array().cloned())
                    .unwrap_or_default(),
                v2: false,
            },
        }
    }

    pub fn json(&self) -> String {
        serde_json::to_string(&self.components).unwrap_or_else(|_| "[]".into())
    }
}

/// Edit the message a component sits on with `page` (the interaction was acknowledged first).
async fn send_page(
    sctx: &serenity::Context,
    i: &serenity::ComponentInteraction,
    page: Page,
    use_v2: bool,
    known: &mut super::v2::Known,
) -> bool {
    if use_v2 && page.v2.is_some() {
        edit_v2(sctx, &i.token, &page, known).await.is_ok()
    } else {
        i.edit_response(sctx, page.into_edit()).await.is_ok()
    }
}

/// An ephemeral text only the presser sees.
async fn say_privately(sctx: &serenity::Context, i: &serenity::ComponentInteraction, text: &str) {
    let f = CreateInteractionResponseFollowup::new()
        .content(text)
        .ephemeral(true);
    if let Err(e) = i.create_followup(sctx, f).await {
        tracing::debug!("could not send the private answer: {e}");
    }
}

fn component_values(i: &serenity::ComponentInteraction) -> Vec<String> {
    match &i.data.kind {
        ComponentInteractionDataKind::StringSelect { values } => values.clone(),
        ComponentInteractionDataKind::RoleSelect { values } => {
            values.iter().map(|v| v.get().to_string()).collect()
        }
        ComponentInteractionDataKind::ChannelSelect { values } => {
            values.iter().map(|v| v.get().to_string()).collect()
        }
        _ => Vec::new(),
    }
}

/// Grey out every control of the message exactly as it was last shown. Content, images and text are not part of the
/// edit, so they stay. Public messages are edited through the channel, ephemeral ones through their token.
async fn disable_shown(
    sctx: &serenity::Context,
    token: &str,
    ephemeral: bool,
    channel: serenity::ChannelId,
    message: serenity::MessageId,
    shown: &Shown,
) {
    let mut comps = shown.components.clone();
    comps.iter_mut().for_each(crate::nav::disable_components);
    let body = crate::nav::sweeper::edit_body(comps, shown.v2);
    let r = if ephemeral {
        sctx.http
            .edit_original_interaction_response(token, &body, Vec::new())
            .await
            .map(|_| ())
    } else {
        sctx.http
            .edit_message(channel, message, &body, Vec::new())
            .await
            .map(|_| ())
    };
    if let Err(e) = r {
        tracing::debug!("could not disable components after timeout: {e}");
    }
}

/// Send a page as Components V2 through the original (deferred) response; returns the message id and what is attached.
async fn present_v2(
    ctx: Context<'_>,
    page: &Page,
    ephemeral: bool,
) -> Result<(serenity::MessageId, super::v2::Known), String> {
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
    let sent: Vec<(String, u64)> = msg
        .attachments
        .iter()
        .map(|a| (a.filename.clone(), a.id.get()))
        .collect();
    Ok((msg.id, super::v2::known_from(&sent, &page.files)))
}

/// Edit the message a component sits on with a V2 page (the interaction was acknowledged first). Files that are
/// already attached and unchanged are not uploaded again; if Discord refuses that, everything is sent once more.
async fn edit_v2(
    sctx: &serenity::Context,
    token: &str,
    page: &Page,
    known: &mut super::v2::Known,
) -> Result<(), String> {
    let comps = page.v2.clone().ok_or("no V2 body")?;
    super::v2::validate(&comps)?;
    let (payload, upload) = super::v2::payload_reusing(comps.clone(), &page.files, known);
    let reused = upload.len() < page.files.len();
    let first = sctx
        .http
        .edit_original_interaction_response(token, &payload, upload)
        .await;
    let msg = match first {
        Ok(m) => m,
        Err(e) if reused => {
            tracing::debug!("reusing attachments failed ({e}); uploading them again");
            let payload = super::v2::payload(comps, &page.files);
            sctx.http
                .edit_original_interaction_response(token, &payload, page.files.clone())
                .await
                .map_err(|e| e.to_string())?
        }
        Err(e) => return Err(e.to_string()),
    };
    let sent: Vec<(String, u64)> = msg
        .attachments
        .iter()
        .map(|a| (a.filename.clone(), a.id.get()))
        .collect();
    *known = super::v2::known_from(&sent, &page.files);
    Ok(())
}

async fn send_card(
    sctx: &serenity::Context,
    i: &serenity::ComponentInteraction,
    card: CardFollowup,
) {
    let mut f = CreateInteractionResponseFollowup::new()
        .embed(card.embed)
        .ephemeral(true);
    for file in card.files {
        f = f.add_file(file);
    }
    if let Some((url, label)) = card.link {
        f = f.components(vec![CreateActionRow::Buttons(vec![
            serenity::CreateButton::new_link(url).label(label),
        ])]);
    }
    if let Err(e) = i.create_followup(sctx, f).await {
        tracing::debug!("could not send the card: {e}");
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

/// Send the view and drive it until it closes or its lifetime is over. Pages with a Components V2 body are sent as one
/// container; when Discord refuses that, the embed version of the same page is used for the whole session.
pub async fn run<V: View>(ctx: Context<'_>, mut view: V) -> Result<(), Error> {
    use crate::nav::{
        self,
        overlay::{page_of, NavState, Step},
    };
    view.on_start().await;
    let sctx = ctx.serenity_context();
    let first = view.render(true);
    let mut use_v2 = first.v2.is_some() && matches!(ctx, Context::Application(_));
    let mut message_id = None;
    let mut legacy_handle = None;
    let mut known = super::v2::Known::new();
    if use_v2 {
        match present_v2(ctx, &first, view.ephemeral()).await {
            Ok((id, k)) => {
                message_id = Some(id);
                known = k;
            }
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
    let Some(message_id) = message_id else {
        return Ok(());
    };
    let owner = ctx.author().id;
    let shared = view.shared();
    let ephemeral = view.ephemeral();
    let class = view.class();
    let data = ctx.data();
    let (user_i, guild) = (owner.get() as i64, ctx.guild_id().map(|g| g.get() as i64));
    let (msg_i, channel) = (message_id.get() as i64, ctx.channel_id());
    let token = match ctx {
        Context::Application(app) => app.interaction.token.clone(),
        _ => String::new(),
    };
    let lifetimes = nav::lifetimes();
    let _live = nav::live::enter(msg_i, (!shared).then(|| owner.get()));
    let mut shown = Shown::of(&first, use_v2);
    nav_register(
        data,
        msg_i,
        channel.get() as i64,
        guild,
        (!shared).then_some(user_i),
        class,
        ephemeral,
        &token,
        &shown,
    )
    .await;
    let mut navstate = NavState::new();
    let started = tokio::time::Instant::now();
    let mut last_activity = tokio::time::Instant::now();

    loop {
        let (idle, cap) = lifetimes.of(class);
        let idle_left = Duration::from_secs(idle)
            .saturating_sub(last_activity.elapsed())
            .min(Duration::from_secs(cap).saturating_sub(started.elapsed()));
        if idle_left.is_zero() {
            break;
        }
        let wait = view.tick_after().map_or(idle_left, |t| t.min(idle_left));
        let i = match next_interaction(sctx, message_id, (!shared).then_some(owner), wait).await {
            Wait::Press(i) => *i,
            Wait::Global(i) => {
                let i = *i;
                // The channel lock also applies to navigation buttons.
                let admin = crate::guild::is_admin(i.member.as_ref().and_then(|m| m.permissions));
                if let Some(text) =
                    crate::guild::denial(&data.guilds, &sctx.http, i.guild_id, i.channel_id, admin)
                        .await
                {
                    let refusal = CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(text)
                            .ephemeral(true),
                    );
                    let _ = i.create_response(sctx, refusal).await;
                    continue;
                }
                let edit =
                    crate::nav::route((!shared).then(|| owner.get()), i.user.id.get(), use_v2)
                        == crate::nav::Route::EditInPlace;
                if !edit {
                    // Another member (or a shared message such as a game round, or the profile): the public message
                    // does not change, they get a private view of their own.
                    let (sctx2, data2) = (sctx.clone(), data.clone());
                    let (id, values) = (i.data.custom_id.clone(), component_values(&i));
                    tokio::spawn(crate::nav::private::open(sctx2, data2, i, id, values));
                    continue;
                }
                // The owner: acknowledge at once (the data may be slow), then edit this same message.
                if let Err(e) = i
                    .create_response(sctx, CreateInteractionResponse::Acknowledge)
                    .await
                {
                    tracing::debug!("could not acknowledge interaction: {e}");
                    continue;
                }
                last_activity = tokio::time::Instant::now();
                let values = component_values(&i);
                match navstate
                    .press(data, &i.data.custom_id, &values, user_i, guild)
                    .await
                {
                    Step::Show(msg) => {
                        let page = page_of(*msg, navstate.has_back());
                        shown = Shown::of(&page, false);
                        if !send_page(sctx, &i, page, false, &mut known).await {
                            tracing::debug!("could not edit message");
                            break;
                        }
                    }
                    Step::Root => {
                        let page = view.render(true);
                        shown = Shown::of(&page, false);
                        if !send_page(sctx, &i, page, false, &mut known).await {
                            break;
                        }
                    }
                    Step::Act(text) => say_privately(sctx, &i, &text).await,
                    Step::Ignore => {}
                }
                nav_touch(data, msg_i, ephemeral, &navstate, &shown).await;
                continue;
            }
            Wait::Quiet => {
                // A round timer fired (not the lifetime): let the view advance.
                if view.tick_after().is_some_and(|t| t <= wait) && !navstate.in_overlay() {
                    let outcome = view.on_tick().await;
                    if outcome != Outcome::Keep {
                        let page = view.render(outcome != Outcome::Close);
                        shown = Shown::of(&page, use_v2);
                        let sent = if use_v2 && page.v2.is_some() {
                            // Ticks have no interaction token of their own: use the command's.
                            match ctx {
                                Context::Application(app) => {
                                    edit_v2(sctx, &app.interaction.token, &page, &mut known)
                                        .await
                                        .is_ok()
                                }
                                _ => false,
                            }
                        } else {
                            match &legacy_handle {
                                Some(h) => h
                                    .edit(ctx, page.into_reply().ephemeral(view.ephemeral()))
                                    .await
                                    .is_ok(),
                                None => false,
                            }
                        };
                        if !sent {
                            tracing::debug!("could not edit the message after a timer");
                        }
                    }
                    if outcome == Outcome::Close {
                        nav_finish(data, msg_i, ephemeral).await;
                        return Ok(());
                    }
                    continue;
                }
                if view.tick_after().is_some_and(|t| t <= wait) {
                    continue;
                }
                break;
            }
        };
        last_activity = tokio::time::Instant::now();
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
                let page = view.render(outcome != Outcome::Close);
                shown = Shown::of(&page, use_v2);
                let sent = if use_v2 && page.v2.is_some() {
                    edit_v2(sctx, &answer.interaction.token, &page, &mut known)
                        .await
                        .is_ok()
                } else {
                    answer
                        .interaction
                        .edit_response(sctx, page.into_edit())
                        .await
                        .is_ok()
                };
                if !sent {
                    tracing::debug!("could not edit message after modal");
                    nav_finish(data, msg_i, ephemeral).await;
                    return Ok(());
                }
            }
            nav_touch(data, msg_i, ephemeral, &navstate, &shown).await;
            if outcome == Outcome::Close {
                nav_finish(data, msg_i, ephemeral).await;
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
        let values = component_values(&i);
        let outcome = view
            .on_user_interaction(i.user.id, admin, &i.data.custom_id, &values)
            .await;
        if let Outcome::Whisper(text) = &outcome {
            send_whisper(sctx, &i, Whisper::text(text.clone()), i.user.id).await;
            continue;
        }
        if outcome != Outcome::Keep {
            let page = view.render(outcome != Outcome::Close);
            shown = Shown::of(&page, use_v2);
            if !send_page(sctx, &i, page, use_v2, &mut known).await {
                // e.g. the message was deleted: stop quietly.
                tracing::debug!("could not edit message");
                nav_finish(data, msg_i, ephemeral).await;
                return Ok(());
            }
        }
        nav_touch(data, msg_i, ephemeral, &navstate, &shown).await;
        if let Some(w) = view.take_whisper() {
            send_whisper(sctx, &i, w, i.user.id).await;
        }
        if let Some(card) = view.take_card() {
            send_card(sctx, &i, card).await;
        }
        if outcome == Outcome::Close {
            nav_finish(data, msg_i, ephemeral).await;
            return Ok(());
        }
    }
    // The lifetime is over: grey everything out, silently, exactly as it was last shown.
    disable_shown(sctx, &token, ephemeral, channel, message_id, &shown).await;
    nav_finish(data, msg_i, ephemeral).await;
    Ok(())
}

/// Register the message with the session store (public) or the in-memory ephemeral list.
#[allow(clippy::too_many_arguments)]
async fn nav_register(
    data: &crate::Data,
    message: i64,
    channel: i64,
    guild: Option<i64>,
    owner: Option<i64>,
    class: crate::nav::Class,
    ephemeral: bool,
    token: &str,
    shown: &Shown,
) {
    use crate::nav;
    let l = nav::lifetimes();
    let now = nav::now();
    if ephemeral {
        let (expires, hard) = l.first_expiry(class, now);
        data.ephemerals.insert(
            message,
            nav::sweeper::Eph {
                token: token.to_string(),
                created: now,
                expires_at: expires,
                hard_expires_at: hard,
                class,
                components: shown.components.clone(),
                v2: shown.v2,
            },
        );
        return;
    }
    let s = nav::store::NewSession {
        message_id: message,
        channel_id: channel,
        guild_id: guild,
        owner_id: owner,
        kind: "view",
        class,
        components_json: &shown.json(),
        v2: shown.v2,
    };
    if let Err(e) = nav::store::register(&data.db, &s, &l, now).await {
        tracing::warn!("could not register the message session: {e}");
    }
}

/// A press: slide the lifetime and remember the Back stack and what is on screen.
async fn nav_touch(
    data: &crate::Data,
    message: i64,
    ephemeral: bool,
    nav: &crate::nav::overlay::NavState,
    shown: &Shown,
) {
    use crate::nav;
    let l = nav::lifetimes();
    let now = nav::now();
    if ephemeral {
        data.ephemerals
            .touch(message, now, &l, shown.components.clone(), shown.v2);
        return;
    }
    if let Err(e) = nav::store::touch(
        &data.db,
        message,
        &l,
        now,
        &nav.trail.to_json(),
        &shown.json(),
        shown.v2,
    )
    .await
    {
        tracing::debug!("could not extend the message session: {e}");
    }
}

/// The session is over: its row goes (the sweeper has nothing left to do for it).
async fn nav_finish(data: &crate::Data, message: i64, ephemeral: bool) {
    if ephemeral {
        data.ephemerals.forget(message);
    } else if let Err(e) = crate::nav::store::remove(&data.db, message).await {
        tracing::debug!("could not remove the message session: {e}");
    }
}
