//! Gateway events that are not tied to a command session: the "show the work" button and menu of notification
//! messages. A press always sends a NEW message; the notification itself is never edited.

use crate::{
    anilist::ContentPolicy,
    error::Error,
    feed::parse_show_work,
    guild, i18n,
    ui::cards,
    Data,
};
use poise::serenity_prelude as serenity;

/// Should the card be sent as an ephemeral message? Only where the server restricts the bot to one channel and
/// this is another one (members other than admins would be answered there against the rule).
pub fn card_is_ephemeral(bot_channel: Option<i64>, channel: i64, is_admin: bool) -> bool {
    matches!(bot_channel, Some(main) if main != channel && !is_admin)
}

pub async fn handle(ctx: &serenity::Context, event: &serenity::FullEvent, data: &Data) -> Result<(), Error> {
    let serenity::FullEvent::InteractionCreate { interaction: serenity::Interaction::Component(i) } = event else {
        return Ok(());
    };
    let values = match &i.data.kind {
        serenity::ComponentInteractionDataKind::StringSelect { values } => values.clone(),
        _ => Vec::new(),
    };
    let Some(work) = parse_show_work(&i.data.custom_id, &values) else {
        return Ok(());
    };
    let admin = guild::is_admin(i.member.as_ref().and_then(|m| m.permissions));
    let bot_channel = match i.guild_id {
        Some(g) => data.guilds.settings(g.get() as i64).await.ok().and_then(|s| s.command_channel.filter(|_| s.channel_exists)),
        None => None,
    };
    let ephemeral = card_is_ephemeral(bot_channel, i.channel_id.get() as i64, admin);
    // A new (deferred) message: this is what keeps the notification untouched.
    let defer = serenity::CreateInteractionResponse::Defer(serenity::CreateInteractionResponseMessage::new().ephemeral(ephemeral));
    i.create_response(ctx, defer).await?;
    let followup = match data.anilist.media_by_id(work, ContentPolicy::SAFE).await {
        Ok(Some(m)) => {
            let mut row = Vec::new();
            if let Some(u) = &m.site_url {
                row.push(serenity::CreateButton::new_link(u).label(format!("🔗 {}", i18n::BTN_ANILIST)));
            }
            let mut f = serenity::CreateInteractionResponseFollowup::new().embed(cards::work_card(&m).build()).ephemeral(ephemeral);
            if !row.is_empty() {
                f = f.components(vec![serenity::CreateActionRow::Buttons(row)]);
            }
            f
        }
        _ => serenity::CreateInteractionResponseFollowup::new().content(i18n::SHOW_WORK_UNAVAILABLE).ephemeral(true),
    };
    i.create_followup(ctx, followup).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::{show_work_id, SHOW_WORK_PICK};

    #[test]
    fn the_button_and_the_menu_name_the_work() {
        assert_eq!(parse_show_work(&show_work_id(42), &[]), Some(42));
        assert_eq!(parse_show_work(SHOW_WORK_PICK, &["7".into()]), Some(7));
        assert_eq!(parse_show_work(SHOW_WORK_PICK, &[]), None);
        assert_eq!(parse_show_work("shownotif:abc", &[]), None);
        assert_eq!(parse_show_work("close", &[]), None);
    }

    #[test]
    fn the_card_is_public_unless_the_server_restricts_the_channel() {
        assert!(!card_is_ephemeral(None, 5, false), "no restriction");
        assert!(!card_is_ephemeral(Some(5), 5, false), "the bot channel itself");
        assert!(card_is_ephemeral(Some(5), 6, false), "another channel of a restricted server");
        assert!(!card_is_ephemeral(Some(5), 6, true), "admins may bypass");
    }

    #[test]
    fn the_notification_button_carries_the_id_and_the_arabic_label() {
        let j = serde_json::to_value(crate::feed::show_work_button(9)).expect("json");
        assert_eq!(j["custom_id"], "shownotif:9");
        assert_eq!(j["label"], "أظهر العمل");
    }
}
