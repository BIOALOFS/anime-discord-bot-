//! Gateway events that are not tied to a command session: the "show the work" button and menu of notification
//! messages. A press always sends a NEW message; the notification itself is never edited.

use crate::{
    anilist::ContentPolicy, error::Error, feed::parse_show_work, guild, i18n, ui::cards, Data,
};
use poise::serenity_prelude as serenity;

/// Should the card be sent as an ephemeral message? Only where the server restricts the bot to one channel and
/// this is another one (members other than admins would be answered there against the rule).
pub fn card_is_ephemeral(bot_channel: Option<i64>, channel: i64, is_admin: bool) -> bool {
    matches!(bot_channel, Some(main) if main != channel && !is_admin)
}

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
    let press = crate::components::parse(&i.data.custom_id, &values);
    let work = match &press {
        Some(_) => None,
        None => match parse_show_work(&i.data.custom_id, &values) {
            Some(w) => Some(w),
            None => return Ok(()),
        },
    };
    let admin = guild::is_admin(i.member.as_ref().and_then(|m| m.permissions));
    let bot_channel = match i.guild_id {
        Some(g) => data
            .guilds
            .settings(g.get() as i64)
            .await
            .ok()
            .and_then(|s| s.command_channel.filter(|_| s.channel_exists)),
        None => None,
    };
    let ephemeral = card_is_ephemeral(bot_channel, i.channel_id.get() as i64, admin);
    if let Some(press) = press {
        return crate::global::handle(ctx, i, data, press, ephemeral).await;
    }
    let Some(work) = work else { return Ok(()) };
    // A new (deferred) message: this is what keeps the notification untouched.
    let defer = serenity::CreateInteractionResponse::Defer(
        serenity::CreateInteractionResponseMessage::new().ephemeral(ephemeral),
    );
    i.create_response(ctx, defer).await?;
    let followup = match data.anilist.media_by_id(work, ContentPolicy::SAFE).await {
        Ok(Some(m)) => {
            let mut row = Vec::new();
            if let Some(u) = &m.site_url {
                row.push(
                    serenity::CreateButton::new_link(u).label(format!("🔗 {}", i18n::BTN_ANILIST)),
                );
            }
            let mut f = serenity::CreateInteractionResponseFollowup::new()
                .embed(cards::work_card(&m).build())
                .ephemeral(ephemeral);
            if !row.is_empty() {
                f = f.components(vec![serenity::CreateActionRow::Buttons(row)]);
            }
            f
        }
        _ => serenity::CreateInteractionResponseFollowup::new()
            .content(i18n::SHOW_WORK_UNAVAILABLE)
            .ephemeral(true),
    };
    i.create_followup(ctx, followup).await?;
    Ok(())
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
    fn the_card_is_public_unless_the_server_restricts_the_channel() {
        assert!(!card_is_ephemeral(None, 5, false), "no restriction");
        assert!(
            !card_is_ephemeral(Some(5), 5, false),
            "the bot channel itself"
        );
        assert!(
            card_is_ephemeral(Some(5), 6, false),
            "another channel of a restricted server"
        );
        assert!(!card_is_ephemeral(Some(5), 6, true), "admins may bypass");
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
