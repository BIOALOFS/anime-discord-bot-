//! A private view: what another member gets when they press a navigation button on someone else's message (or on a
//! notification). The public message does not change; they receive an ephemeral message with its own Back stack and its
//! own lifetime.

use super::{
    live,
    overlay::{page_of, NavState, Step},
    stack::Desc,
    sweeper::Eph,
    Class,
};
use crate::{components, global, ui::session::Page, Data};
use poise::serenity_prelude as serenity;
use std::time::Duration;

fn followup_of(page: Page) -> serenity::CreateInteractionResponseFollowup {
    let mut f = serenity::CreateInteractionResponseFollowup::new().ephemeral(true);
    if let Some(c) = page.content {
        f = f.content(c);
    }
    for e in page.embeds {
        f = f.embed(e);
    }
    for file in page.files {
        f = f.add_file(file);
    }
    if !page.components.is_empty() {
        f = f.components(page.components);
    }
    f
}

fn values_of(i: &serenity::ComponentInteraction) -> Vec<String> {
    match &i.data.kind {
        serenity::ComponentInteractionDataKind::StringSelect { values } => values.clone(),
        _ => Vec::new(),
    }
}

/// Open the private view a press asks for and drive it until its lifetime is over. A press that does not ask for a view
/// (Back on someone else's message, an unknown id) is only acknowledged.
pub async fn open(
    sctx: serenity::Context,
    data: Data,
    i: serenity::ComponentInteraction,
    id: String,
    values: Vec<String>,
) {
    let Some(press) = components::parse(&id, &values) else {
        let _ = i
            .create_response(&sctx, serenity::CreateInteractionResponse::Acknowledge)
            .await;
        return;
    };
    if press == components::Press::Back {
        let _ = i
            .create_response(&sctx, serenity::CreateInteractionResponse::Acknowledge)
            .await;
        return;
    }
    let (user, guild) = (i.user.id.get() as i64, i.guild_id.map(|g| g.get() as i64));
    // Acknowledge at once (an ephemeral placeholder), then build the view.
    let defer = serenity::CreateInteractionResponse::Defer(
        serenity::CreateInteractionResponseMessage::new().ephemeral(true),
    );
    if i.create_response(&sctx, defer).await.is_err() {
        return;
    }
    // Reminders, adding to the profile and the status menu act for the presser and answer with a line of text.
    if let Some(text) = global::act_text(&data, &press, user, guild).await {
        let f = serenity::CreateInteractionResponseFollowup::new()
            .content(text)
            .ephemeral(true);
        let _ = i.create_followup(&sctx, f).await;
        return;
    }
    let Some(msg) = global::view_msg(&data, &press, user, guild).await else {
        return;
    };
    let mut state = NavState::private(Desc::new(&id, &values));
    let page = page_of(msg, false);
    let mut shown = crate::ui::session::Shown::of(&page, false);
    let sent = match i.create_followup(&sctx, followup_of(page)).await {
        Ok(m) => m,
        Err(e) => {
            tracing::debug!("could not send the private view: {e}");
            return;
        }
    };
    let message = sent.id;
    let msg_i = message.get() as i64;
    let _live = live::enter(msg_i, Some(i.user.id.get()));
    let l = super::lifetimes();
    let now = super::now();
    let (expires, hard) = l.first_expiry(Class::Default, now);
    data.ephemerals.insert(
        msg_i,
        Eph {
            token: i.token.clone(),
            created: now,
            expires_at: expires,
            hard_expires_at: hard,
            class: Class::Default,
            components: shown.components.clone(),
            v2: false,
        },
    );
    let started = tokio::time::Instant::now();
    let mut last = tokio::time::Instant::now();
    loop {
        let (idle, cap) = l.of(Class::Default);
        let left = Duration::from_secs(idle)
            .saturating_sub(last.elapsed())
            .min(Duration::from_secs(cap).saturating_sub(started.elapsed()));
        if left.is_zero() {
            break;
        }
        let Some(j) = serenity::ComponentInteractionCollector::new(&sctx)
            .message_id(message)
            .author_id(i.user.id)
            .timeout(left)
            .await
        else {
            break;
        };
        if j.create_response(&sctx, serenity::CreateInteractionResponse::Acknowledge)
            .await
            .is_err()
        {
            continue;
        }
        last = tokio::time::Instant::now();
        let vals = values_of(&j);
        match state
            .press(&data, &j.data.custom_id, &vals, user, guild)
            .await
        {
            Step::Show(m) => {
                let page = page_of(*m, state.has_back());
                shown = crate::ui::session::Shown::of(&page, false);
                if j.edit_response(&sctx, page.into_edit()).await.is_err() {
                    break;
                }
            }
            Step::Act(text) => {
                let f = serenity::CreateInteractionResponseFollowup::new()
                    .content(text)
                    .ephemeral(true);
                let _ = j.create_followup(&sctx, f).await;
            }
            Step::Root | Step::Ignore => {}
        }
        data.ephemerals
            .touch(msg_i, super::now(), &l, shown.components.clone(), false);
    }
    // Lifetime over: grey the controls out (silently), then forget the message.
    let mut comps = shown.components.clone();
    comps.iter_mut().for_each(super::disable_components);
    let body = super::sweeper::edit_body(comps, false);
    if let Err(e) = sctx
        .http
        .edit_followup_message(&i.token, message, &body, Vec::new())
        .await
    {
        tracing::debug!("could not disable the private view: {e}");
    }
    data.ephemerals.forget(msg_i);
}
