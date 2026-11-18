//! Gateway events that are not tied to a command session: the "show the work" button and menu of notification
//! messages. A press always sends a NEW message; the notification itself is never edited.

use crate::{error::Error, feed::parse_show_work, i18n, Data};
use poise::serenity_prelude as serenity;

pub async fn handle(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    data: &Data,
) -> Result<(), Error> {
    let serenity::FullEvent::InteractionCreate {
        interaction: serenity::Interaction::Component(i),
    } = event
    else {
        return Ok(());
    };
    let values = match &i.data.kind {
        serenity::ComponentInteractionDataKind::StringSelect { values } => values.clone(),
        _ => Vec::new(),
    };
    if i.data
        .custom_id
        .starts_with(crate::chapters::digest::MORE_PREFIX)
    {
        return digest_more(ctx, i).await;
    }
    if let Some((poll, option)) = crate::features::poll::parse_vote(&i.data.custom_id) {
        return poll_vote(ctx, i, data, poll, option).await;
    }
    if let Some((night, going)) = crate::features::watch::parse_rsvp(&i.data.custom_id) {
        return watch_rsvp(ctx, i, data, night, going).await;
    }
    // Old notification buttons (`shownotif:`) ask for an AniList work.
    let (id, vals) = match crate::components::parse(&i.data.custom_id, &values) {
        Some(_) => (i.data.custom_id.clone(), values.clone()),
        None => match parse_show_work(&i.data.custom_id, &values) {
            Some(w) => (
                crate::components::show_work_id(&format!("anilist:{w}")),
                Vec::new(),
            ),
            None => {
                // Not one of the navigation buttons. A running session (or a short-lived collector such as the
                // undo button) answers its own presses; a press nobody answered is a stale one (a message from
                // before a restart): acknowledge it quietly after a moment, so nothing shows as a failure.
                if !crate::nav::live::is_live(i.message.id.get() as i64) {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    let _ = i
                        .create_response(ctx, serenity::CreateInteractionResponse::Acknowledge)
                        .await;
                }
                return Ok(());
            }
        },
    };
    // Only a notification is shared by the whole server: a press on it opens a private view for the presser and never
    // edits it. A running session answers its own presses; anything else (a message from before a restart, a session
    // that ended) is a stale press: acknowledged, nothing else.
    let mid = i.message.id.get() as i64;
    let notification = matches!(
        crate::nav::store::get(&data.db, mid).await,
        Ok(Some(row)) if row.kind == "notification"
    );
    match crate::nav::event_route(crate::nav::live::is_live(mid), notification) {
        crate::nav::EventRoute::Session => Ok(()),
        crate::nav::EventRoute::Private => {
            crate::nav::private::open(ctx.clone(), data.clone(), i.clone(), id, vals).await;
            Ok(())
        }
        crate::nav::EventRoute::Stale => {
            tracing::debug!(message = %i.message.id, "stale press ignored");
            let _ = i
                .create_response(ctx, serenity::CreateInteractionResponse::Acknowledge)
                .await;
            Ok(())
        }
    }
}

/// Answer a button with a short private message.
async fn private(
    ctx: &serenity::Context,
    i: &serenity::ComponentInteraction,
    text: &str,
) -> Result<(), Error> {
    let r = serenity::CreateInteractionResponseMessage::new()
        .content(text)
        .ephemeral(true);
    Ok(
        i.create_response(ctx, serenity::CreateInteractionResponse::Message(r))
            .await?,
    )
}

/// A vote in the weekly poll: one per member, changeable; the message is updated with the live counts.
async fn poll_vote(
    ctx: &serenity::Context,
    i: &serenity::ComponentInteraction,
    data: &Data,
    poll: i64,
    option: usize,
) -> Result<(), Error> {
    use crate::features::poll;
    let now = chrono::Utc::now().timestamp();
    if poll::vote(&data.db, poll, i.user.id.get() as i64, option, now).await?
        == poll::Voted::Refused
    {
        return private(ctx, i, i18n::POLL_CLOSED).await;
    }
    let Some(p) = poll::get(&data.db, poll).await? else {
        return private(ctx, i, i18n::POLL_CLOSED).await;
    };
    let counts = poll::counts(&data.db, &p).await?;
    let r = serenity::CreateInteractionResponseMessage::new()
        .embed(poll::embed(&p.options, &counts, p.closes_at, p.closed).build())
        .components(vec![poll::buttons(poll, &p.options, p.closed)]);
    Ok(
        i.create_response(ctx, serenity::CreateInteractionResponse::UpdateMessage(r))
            .await?,
    )
}

/// "سأحضر" / "لن أحضر" on a watch-night announcement.
async fn watch_rsvp(
    ctx: &serenity::Context,
    i: &serenity::ComponentInteraction,
    data: &Data,
    night: i64,
    going: bool,
) -> Result<(), Error> {
    use crate::features::watch;
    let now = chrono::Utc::now().timestamp();
    if watch::rsvp(&data.db, night, i.user.id.get() as i64, going, now).await?
        == watch::Rsvp::Closed
    {
        return private(ctx, i, i18n::WATCH_STARTED).await;
    }
    let Some(n) = watch::get(&data.db, night).await? else {
        return private(ctx, i, i18n::WATCH_STARTED).await;
    };
    let counts = watch::counts(&data.db, night).await?;
    let r = serenity::CreateInteractionResponseMessage::new()
        .embed(watch::embed(&n, counts).build())
        .components(watch::rows(&n, false));
    Ok(
        i.create_response(ctx, serenity::CreateInteractionResponse::UpdateMessage(r))
            .await?,
    )
}

/// "المزيد" under a chapter digest: the next lines as a new message (the digest itself stays as it is).
async fn digest_more(
    ctx: &serenity::Context,
    i: &serenity::ComponentInteraction,
) -> Result<(), Error> {
    use crate::chapters::digest;
    i.create_response(
        ctx,
        serenity::CreateInteractionResponse::Defer(
            serenity::CreateInteractionResponseMessage::new().ephemeral(true),
        ),
    )
    .await?;
    let Some(rest) = digest::take_overflow(i.message.id.get()).await else {
        i.create_followup(
            ctx,
            serenity::CreateInteractionResponseFollowup::new()
                .content(i18n::CHAPTER_DIGEST_EXPIRED)
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };
    let (page, remaining) = digest::next_page(&rest);
    let mut f = serenity::CreateInteractionResponseFollowup::new()
        .ephemeral(true)
        .embed(digest::embed(page, page.len()).build());
    if !remaining.is_empty() {
        f = f.components(vec![serenity::CreateActionRow::Buttons(vec![
            digest::more_button(remaining.len()),
        ])]);
    }
    let sent = i.create_followup(ctx, f).await?;
    if !remaining.is_empty() {
        digest::keep_overflow(sent.id.get(), remaining.to_vec()).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::SHOW_WORK_PICK;

    #[test]
    fn the_button_and_the_menu_name_the_work() {
        assert_eq!(parse_show_work("shownotif:42", &[]), Some(42));
        assert_eq!(parse_show_work(SHOW_WORK_PICK, &["7".into()]), Some(7));
        assert_eq!(parse_show_work(SHOW_WORK_PICK, &[]), None);
        assert_eq!(parse_show_work("shownotif:abc", &[]), None);
        assert_eq!(parse_show_work("close", &[]), None);
    }

    #[test]
    fn notifications_carry_the_universal_buttons() {
        let j = serde_json::to_string(&crate::feed::notification_buttons(9)).expect("json");
        assert!(j.contains("showchars:anilist:9") && j.contains("أظهر الشخصيات"));
        assert!(j.contains("remind:anilist:9") && j.contains("ذكّرني"));
        assert!(
            !j.contains("shownotif"),
            "the old button is gone from new messages"
        );
    }
}
