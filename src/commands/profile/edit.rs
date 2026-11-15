//! `/edit`: every modification lives here. `target` picks profile, character or work, `action` what to change.

use super::{
    actions, manage, social,
    util::{
        ac_own, ac_own_work, begin_private, own_or_reply, parse_color, pool, private_reply,
        require_own, uid,
    },
};
use crate::{
    anilist::safety::contains_explicit_term,
    commands::profile::option_str,
    db::{
        custom,
        profile::{self as pdb, Privacy, MAX_BIO, MAX_TITLE},
    },
    error::{Context, Error},
    i18n,
    render::{BannerSpec, BANNER_PRESETS},
    ui::{autocomplete::to_choices, session::UndoFuture},
};
use poise::serenity_prelude as serenity;
use std::time::Duration;

pub const TARGETS: &[(&str, &str)] = &[
    ("profile", "البروفايل"),
    ("character", "شخصية"),
    ("work", "عمل"),
];

pub const PROFILE_ACTIONS: &[(&str, &str)] = &[
    ("banner", "الغلاف (صورة أو لون أو تدرج)"),
    ("description", "النبذة (حتى 300 حرف)"),
    ("title", "الاسم المعروض (حتى 40 حرفًا)"),
    ("color", "لون البطاقة"),
    ("privacy", "الخصوصية"),
    ("reset", "إعادة ضبط البروفايل"),
    ("export", "تصدير بياناتي JSON"),
    ("import", "استيراد JSON (أرفق الملف)"),
    ("delete-data", "حذف كل بياناتي"),
];

pub const CHARACTER_ACTIONS: &[(&str, &str)] = &[
    ("image", "تغيير الصورة (PNG/JPG/WebP/GIF)"),
    ("title", "اللقب (حتى 40 حرفًا)"),
    ("description", "الوصف (حتى 300 حرف)"),
    ("reset", "إعادة للأصل"),
    ("remove", "إزالة من البروفايل"),
    ("move", "نقل الموضع (النص = الرقم)"),
];

pub const WORK_ACTIONS: &[(&str, &str)] = &[
    ("remove", "إزالة من البروفايل"),
    ("move", "نقل الموضع (النص = الرقم)"),
];

pub fn actions_for(target: &str) -> &'static [(&'static str, &'static str)] {
    match target {
        "profile" => PROFILE_ACTIONS,
        "character" => CHARACTER_ACTIONS,
        "work" => WORK_ACTIONS,
        _ => &[],
    }
}

/// What a banner `value` means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BannerChoice {
    Reset,
    Spec(BannerSpec),
}

/// `reset`, a preset key or a colour like `#ff8800`.
pub fn parse_banner_value(v: &str) -> Option<BannerChoice> {
    let v = v.trim();
    if v.eq_ignore_ascii_case("reset") || super::actions_clear_word(v) {
        return Some(BannerChoice::Reset);
    }
    if BANNER_PRESETS.iter().any(|p| p.0 == v) {
        return Some(BannerChoice::Spec(BannerSpec::Preset(v.to_string())));
    }
    parse_color(v).map(|c| BannerChoice::Spec(BannerSpec::Color(c)))
}

/// Validate a text field: not empty, within `max` characters, nothing explicit. Returns the cleaned text.
pub fn check_text(text: &str, max: usize) -> Result<String, &'static str> {
    let t = text.trim();
    if t.chars().count() > max {
        return Err(if max == MAX_TITLE {
            i18n::TITLE_TOO_LONG
        } else {
            i18n::BIO_TOO_LONG
        });
    }
    if contains_explicit_term(t) {
        return Err(i18n::CHARACTER_HIDDEN);
    }
    Ok(t.to_string())
}

type Undo = Box<dyn FnOnce() -> UndoFuture + Send + Sync>;

/// Ephemeral confirmation; when there is an undo, a button works for one minute.
async fn confirm(
    ctx: Context<'_>,
    text: impl Into<String>,
    undo: Option<Undo>,
) -> Result<(), Error> {
    let text = text.into();
    let mut reply = poise::CreateReply::default()
        .content(text.clone())
        .ephemeral(true);
    if undo.is_some() {
        reply = reply.components(vec![serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new("undo")
                .label(i18n::BTN_UNDO)
                .emoji('↩')
                .style(serenity::ButtonStyle::Secondary),
        ])]);
    }
    let handle = ctx.send(reply).await?;
    let Some(undo) = undo else { return Ok(()) };
    let msg = handle.message().await?;
    let press = serenity::ComponentInteractionCollector::new(ctx.serenity_context())
        .message_id(msg.id)
        .author_id(ctx.author().id)
        .timeout(Duration::from_secs(60))
        .await;
    match press {
        Some(p) => {
            let done = undo().await;
            let update = serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .content(done)
                    .components(vec![]),
            );
            if let Err(e) = p.create_response(ctx.serenity_context(), update).await {
                tracing::debug!("could not confirm undo: {e}");
            }
        }
        None => {
            let _ = handle
                .edit(
                    ctx,
                    poise::CreateReply::default()
                        .content(text)
                        .components(vec![]),
                )
                .await;
        }
    }
    Ok(())
}

fn undone(ok: bool) -> String {
    if ok { i18n::UNDONE } else { i18n::ERR_GENERIC }.to_string()
}

// ---- profile -------------------------------------------------------------------------------

async fn profile_banner(
    ctx: Context<'_>,
    value: Option<&str>,
    image: Option<serenity::Attachment>,
) -> Result<(), Error> {
    if let Some(att) = image {
        return actions::banner_image(ctx, att).await;
    }
    // No value and no picture: the menu of ready-made banners.
    if value.map(str::trim).filter(|v| !v.is_empty()).is_none() {
        return super::banner_picker::open(ctx).await;
    }
    begin_private(ctx).await?;
    let Some(p) = require_own(ctx).await? else {
        return Ok(());
    };
    let Some(choice) = value.and_then(parse_banner_value) else {
        return private_reply(ctx, i18n::BANNER_UNKNOWN).await;
    };
    let (pool, user) = (pool(ctx), uid(ctx));
    let (new, text) = match choice {
        BannerChoice::Reset => (None, i18n::BANNER_RESET),
        BannerChoice::Spec(s) => (Some(s.encode()), i18n::BANNER_PRESET_SET),
    };
    pdb::set_banner(&pool, user, new.as_deref()).await?;
    let old = p.banner;
    let undo_pool = pool.clone();
    confirm(
        ctx,
        text,
        Some(Box::new(move || {
            Box::pin(async move {
                undone(
                    pdb::set_banner(&undo_pool, user, old.as_deref())
                        .await
                        .is_ok(),
                )
            })
        })),
    )
    .await
}

async fn profile_text(
    ctx: Context<'_>,
    field: &'static str,
    text: Option<&str>,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some(p) = require_own(ctx).await? else {
        return Ok(());
    };
    let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) else {
        return private_reply(ctx, i18n::PROFILE_NEEDS_TEXT).await;
    };
    let clear = super::actions_clear_word(text);
    let (pool, user) = (pool(ctx), uid(ctx));
    let (max, old, done) = match field {
        "bio" => (MAX_BIO, p.bio.clone(), i18n::BIO_SET),
        _ => (MAX_TITLE, p.display_title.clone(), i18n::TITLE_SET),
    };
    let clean = if clear {
        None
    } else {
        match check_text(text, max) {
            Ok(t) => Some(t),
            Err(msg) => return private_reply(ctx, msg).await,
        }
    };
    if field == "bio" {
        pdb::set_bio(&pool, user, clean.as_deref()).await?;
    } else {
        // The title also loses mentions and links.
        let safe = match &clean {
            Some(t) => match custom::sanitize_text(t, MAX_TITLE) {
                Some(s) => Some(s),
                None => return private_reply(ctx, i18n::TEXT_REJECTED).await,
            },
            None => None,
        };
        pdb::set_display_title(&pool, user, safe.as_deref()).await?;
    }
    let undo_pool = pool.clone();
    confirm(
        ctx,
        done,
        Some(Box::new(move || {
            Box::pin(async move {
                let r = if field == "bio" {
                    pdb::set_bio(&undo_pool, user, old.as_deref()).await
                } else {
                    pdb::set_display_title(&undo_pool, user, old.as_deref()).await
                };
                undone(r.is_ok())
            })
        })),
    )
    .await
}

async fn profile_color(ctx: Context<'_>, value: Option<&str>) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some(p) = require_own(ctx).await? else {
        return Ok(());
    };
    let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return private_reply(ctx, i18n::PROFILE_NEEDS_VALUE).await;
    };
    let new = if super::actions_clear_word(v) {
        None
    } else {
        match parse_color(v) {
            Some(c) => Some(c),
            None => return private_reply(ctx, i18n::COLOR_INVALID).await,
        }
    };
    let (pool, user) = (pool(ctx), uid(ctx));
    pdb::set_color(&pool, user, new).await?;
    let old = p.accent_color.map(|c| c as u32);
    let undo_pool = pool.clone();
    confirm(
        ctx,
        i18n::COLOR_SET,
        Some(Box::new(move || {
            Box::pin(async move { undone(pdb::set_color(&undo_pool, user, old).await.is_ok()) })
        })),
    )
    .await
}

async fn profile_privacy(ctx: Context<'_>, value: Option<&str>) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some(p) = require_own(ctx).await? else {
        return Ok(());
    };
    let Some(level) = value.and_then(Privacy::parse) else {
        return private_reply(ctx, i18n::PRIVACY_UNKNOWN).await;
    };
    let (pool, user) = (pool(ctx), uid(ctx));
    pdb::set_privacy(&pool, user, level).await?;
    let old = p.privacy();
    let undo_pool = pool.clone();
    confirm(
        ctx,
        i18n::PRIVACY_SET,
        Some(Box::new(move || {
            Box::pin(async move { undone(pdb::set_privacy(&undo_pool, user, old).await.is_ok()) })
        })),
    )
    .await
}

async fn profile_reset(ctx: Context<'_>) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some(p) = require_own(ctx).await? else {
        return Ok(());
    };
    let (pool, user) = (pool(ctx), uid(ctx));
    pdb::reset_profile(&pool, user).await?;
    let undo_pool = pool.clone();
    confirm(
        ctx,
        i18n::PROFILE_RESET_DONE,
        Some(Box::new(move || {
            Box::pin(async move {
                let r: Result<(), pdb::ProfileError> = async {
                    pdb::set_bio(&undo_pool, user, p.bio.as_deref()).await?;
                    pdb::set_quote(&undo_pool, user, p.quote.as_deref()).await?;
                    pdb::set_banner(&undo_pool, user, p.banner.as_deref()).await?;
                    pdb::set_color(&undo_pool, user, p.accent_color.map(|c| c as u32)).await?;
                    pdb::set_display_title(&undo_pool, user, p.display_title.as_deref()).await
                }
                .await;
                undone(r.is_ok())
            })
        })),
    )
    .await
}

// ---- character -----------------------------------------------------------------------------

async fn character_text(
    ctx: Context<'_>,
    field: &'static str,
    who: &str,
    text: Option<&str>,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some((id, name)) = own_or_reply(ctx, who).await? else {
        return Ok(());
    };
    let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) else {
        return private_reply(ctx, i18n::PROFILE_NEEDS_TEXT).await;
    };
    let (pool, user) = (pool(ctx), uid(ctx));
    let max = if field == "title" {
        MAX_TITLE
    } else {
        custom::MAX_DESCRIPTION
    };
    if text.chars().count() > max && !super::actions_clear_word(text) {
        return private_reply(
            ctx,
            if field == "title" {
                i18n::TITLE_TOO_LONG
            } else {
                i18n::BIO_TOO_LONG
            },
        )
        .await;
    }
    let (old_title, old_desc) = custom::character_texts(&pool, user, id).await?;
    let new = (!super::actions_clear_word(text)).then_some(text);
    let r = if field == "title" {
        custom::set_title(&pool, user, id, new).await
    } else {
        custom::set_description(&pool, user, id, new).await
    };
    match r {
        Ok(()) => {}
        Err(pdb::ProfileError::Invalid(_)) => return private_reply(ctx, i18n::TEXT_REJECTED).await,
        Err(e) => return Err(e.into()),
    }
    let old = if field == "title" {
        old_title
    } else {
        old_desc
    };
    let undo_pool = pool.clone();
    confirm(
        ctx,
        i18n::custom_saved(&name),
        Some(Box::new(move || {
            Box::pin(async move {
                let r = if field == "title" {
                    custom::set_title(&undo_pool, user, id, old.as_deref()).await
                } else {
                    custom::set_description(&undo_pool, user, id, old.as_deref()).await
                };
                undone(r.is_ok())
            })
        })),
    )
    .await
}

async fn character_move(ctx: Context<'_>, who: &str, text: Option<&str>) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some((id, name)) = own_or_reply(ctx, who).await? else {
        return Ok(());
    };
    let Some(pos) = text
        .and_then(|t| t.trim().parse::<i64>().ok())
        .filter(|p| *p > 0)
    else {
        return private_reply(ctx, i18n::PROFILE_NEEDS_POSITION).await;
    };
    let (pool, user) = (pool(ctx), uid(ctx));
    let old = pdb::get_entry(&pool, user, id).await?.map(|e| e.position);
    let to = pdb::move_character(&pool, user, id, pos).await?;
    let undo_pool = pool.clone();
    confirm(
        ctx,
        i18n::moved_character(&name, to),
        old.map(|old| -> Undo {
            Box::new(move || {
                Box::pin(async move {
                    undone(pdb::move_character(&undo_pool, user, id, old).await.is_ok())
                })
            })
        }),
    )
    .await
}

// ---- dispatch ------------------------------------------------------------------------------

async fn need_character<'a>(
    ctx: Context<'_>,
    v: &'a Option<String>,
) -> Result<Option<&'a str>, Error> {
    match v.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Ok(Some(s)),
        None => {
            begin_private(ctx).await?;
            private_reply(ctx, i18n::EDIT_NEEDS_CHARACTER).await?;
            Ok(None)
        }
    }
}

async fn need_work<'a>(ctx: Context<'_>, v: &'a Option<String>) -> Result<Option<&'a str>, Error> {
    match v.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Ok(Some(s)),
        None => {
            begin_private(ctx).await?;
            private_reply(ctx, i18n::EDIT_NEEDS_WORK).await?;
            Ok(None)
        }
    }
}

fn matching(items: Vec<(String, String)>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let needle = partial.trim().to_lowercase();
    to_choices(
        items
            .into_iter()
            .filter(|(l, v)| {
                needle.is_empty() || l.to_lowercase().contains(&needle) || v.contains(&needle)
            })
            .collect(),
    )
}

async fn ac_target(_ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    matching(
        TARGETS
            .iter()
            .map(|(k, l)| ((*l).to_string(), (*k).to_string()))
            .collect(),
        partial,
    )
}

async fn ac_action(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let target = option_str(ctx, "target").unwrap_or_default();
    matching(
        actions_for(&target)
            .iter()
            .map(|(k, l)| ((*l).to_string(), (*k).to_string()))
            .collect(),
        partial,
    )
}

async fn ac_character(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    ac_own(ctx, partial).await
}

/// `value`: banner presets, privacy levels, or one of my works, depending on target and action.
async fn ac_value(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let target = option_str(ctx, "target").unwrap_or_default();
    let action = option_str(ctx, "action").unwrap_or_default();
    match (target.as_str(), action.as_str()) {
        ("profile", "banner") => matching(
            std::iter::once(("إعادة للافتراضي".to_string(), "reset".to_string()))
                .chain(
                    BANNER_PRESETS
                        .iter()
                        .map(|p| (p.1.to_string(), p.0.to_string())),
                )
                .collect(),
            partial,
        ),
        ("profile", "privacy") => matching(
            crate::commands::settings::PRIVACY_LEVELS
                .iter()
                .map(|(v, l)| ((*l).to_string(), (*v).to_string()))
                .collect(),
            partial,
        ),
        ("work", _) => ac_own_work(ctx, partial).await,
        _ => Vec::new(),
    }
}

/// Change your profile, a character in it or a work in it.
#[poise::command(
    slash_command,
    name_localized("ar", "تعديل"),
    description_localized("ar", "كل التعديلات هنا: البروفايل أو شخصية أو عمل")
)]
pub async fn edit(
    ctx: Context<'_>,
    #[description = "What to edit: profile, character or work"]
    #[description_localized("ar", "ما الذي تريد تعديله: البروفايل أو شخصية أو عمل")]
    #[name_localized("ar", "الهدف")]
    #[autocomplete = "ac_target"]
    target: String,
    #[description = "The change to make"]
    #[description_localized("ar", "التعديل المطلوب")]
    #[name_localized("ar", "الإجراء")]
    #[autocomplete = "ac_action"]
    action: String,
    #[description = "A character of your profile (for target: character)"]
    #[description_localized("ar", "شخصية من بروفايلك (عند اختيار شخصية)")]
    #[name_localized("ar", "الشخصية")]
    #[autocomplete = "ac_character"]
    character: Option<String>,
    #[description = "Value: banner preset or colour, privacy level, or a work of yours"]
    #[description_localized("ar", "القيمة: تدرج أو لون الغلاف، مستوى الخصوصية، أو عمل من أعمالك")]
    #[name_localized("ar", "القيمة")]
    #[autocomplete = "ac_value"]
    value: Option<String>,
    #[description = "Free text: description, title, position or colour"]
    #[description_localized("ar", "نص حر: وصف أو لقب أو رقم الموضع")]
    #[name_localized("ar", "النص")]
    text: Option<String>,
    #[description = "Image (banner or character) or a JSON file (import)"]
    #[description_localized("ar", "صورة (الغلاف أو الشخصية) أو ملف JSON للاستيراد")]
    #[name_localized("ar", "الصورة")]
    image: Option<serenity::Attachment>,
) -> Result<(), Error> {
    let (target, action) = (target.trim(), action.trim());
    if !actions_for(target).iter().any(|(k, _)| *k == action) {
        begin_private(ctx).await?;
        return private_reply(ctx, i18n::PROFILE_PICK_ACTION).await;
    }
    match (target, action) {
        ("profile", "banner") => {
            profile_banner(ctx, value.as_deref().or(text.as_deref()), image).await
        }
        ("profile", "description") => {
            profile_text(ctx, "bio", text.as_deref().or(value.as_deref())).await
        }
        ("profile", "title") => {
            profile_text(ctx, "title", text.as_deref().or(value.as_deref())).await
        }
        ("profile", "color") => profile_color(ctx, value.as_deref().or(text.as_deref())).await,
        ("profile", "privacy") => profile_privacy(ctx, value.as_deref()).await,
        ("profile", "reset") => profile_reset(ctx).await,
        ("profile", "export") => social::export(ctx).await,
        ("profile", "import") => match image {
            Some(file) => social::import(ctx, file).await,
            None => {
                begin_private(ctx).await?;
                private_reply(ctx, i18n::IMPORT_BAD_FILE).await
            }
        },
        ("profile", "delete-data") => actions::delete_data(ctx).await,
        ("character", act) => {
            let Some(who) = need_character(ctx, &character).await? else {
                return Ok(());
            };
            match act {
                "image" => actions::character_image(ctx, who, image).await,
                "title" => character_text(ctx, "title", who, text.as_deref()).await,
                "description" => character_text(ctx, "description", who, text.as_deref()).await,
                "reset" => actions::reset_character(ctx, who).await,
                "remove" => manage::remove(ctx, who.to_string()).await,
                _ => character_move(ctx, who, text.as_deref()).await,
            }
        }
        ("work", act) => {
            let Some(work) = need_work(ctx, &value).await? else {
                return Ok(());
            };
            if act == "remove" {
                actions::remove_work(ctx, work).await
            } else {
                match text
                    .as_deref()
                    .and_then(|t| t.trim().parse::<i64>().ok())
                    .filter(|p| *p > 0)
                {
                    Some(pos) => actions::move_work(ctx, work, pos).await,
                    None => {
                        begin_private(ctx).await?;
                        private_reply(ctx, i18n::PROFILE_NEEDS_POSITION).await
                    }
                }
            }
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_target_has_unique_actions() {
        for (t, _) in TARGETS {
            let acts = actions_for(t);
            assert!(!acts.is_empty(), "{t}");
            let mut seen = std::collections::HashSet::new();
            assert!(acts
                .iter()
                .all(|a| seen.insert(a.0) && a.1.chars().count() <= 100));
        }
        assert!(actions_for("nope").is_empty());
        let chars: Vec<_> = CHARACTER_ACTIONS.iter().map(|a| a.0).collect();
        assert_eq!(
            chars,
            vec!["image", "title", "description", "reset", "remove", "move"]
        );
    }

    #[test]
    fn banner_values_parse_to_reset_presets_or_colours() {
        assert_eq!(parse_banner_value("reset"), Some(BannerChoice::Reset));
        assert_eq!(parse_banner_value("-"), Some(BannerChoice::Reset));
        assert_eq!(
            parse_banner_value("sunset"),
            Some(BannerChoice::Spec(BannerSpec::Preset("sunset".into())))
        );
        assert_eq!(
            parse_banner_value("#ff8800"),
            Some(BannerChoice::Spec(BannerSpec::Color(0xff8800)))
        );
        assert_eq!(parse_banner_value("not-a-colour"), None);
        assert_eq!(parse_banner_value(""), None);
    }

    #[test]
    fn text_limits_are_enforced_without_truncating() {
        assert!(check_text(&"x".repeat(MAX_TITLE), MAX_TITLE).is_ok());
        assert_eq!(
            check_text(&"x".repeat(MAX_TITLE + 1), MAX_TITLE),
            Err(i18n::TITLE_TOO_LONG)
        );
        assert!(check_text(&"x".repeat(300), MAX_BIO).is_ok());
        assert_eq!(
            check_text(&"x".repeat(301), MAX_BIO),
            Err(i18n::BIO_TOO_LONG)
        );
        assert_eq!(
            check_text("a sexual scene", MAX_BIO),
            Err(i18n::CHARACTER_HIDDEN)
        );
    }
}
