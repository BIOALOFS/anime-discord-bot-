//! `/profile`: view a profile (yours or another member's) and add characters or works. Everything that edits
//! lives in `/edit`.

mod actions;
mod edit;
pub use edit::edit;
mod imaging;
pub(crate) mod items;
mod manage;
mod social;
mod util;
pub(crate) mod view;

pub use manage::add_and_cache;
pub use util::now;

use crate::{
    commands::{character::ac_character, media::any_autocomplete, say_ephemeral},
    db::profile as pdb,
    error::{Context, Error},
    i18n,
    ui::autocomplete::to_choices,
};
use items::Section;
use poise::serenity_prelude as serenity;
use util::{begin_public, pool, private_reply, uid};

/// Public defer for commands outside this module that still want the guild remembered.
pub async fn begin_public_for_top(ctx: Context<'_>) -> Result<(), Error> {
    begin_public(ctx).await
}

/// Words that clear a text field.
pub(super) fn actions_clear_word(s: &str) -> bool {
    ["-", "حذف", "مسح"].contains(&s.trim())
}

/// `(key, Arabic label)` of what `/profile` can do besides showing a profile.
pub const ACTIONS: &[(&str, &str)] = &[
    ("view", "عرض البروفايل"),
    ("add-character", "إضافة شخصية"),
    ("add-work", "إضافة عمل"),
];

/// The current text of another option of the command being autocompleted.
pub fn option_str(ctx: Context<'_>, name: &str) -> Option<String> {
    let Context::Application(app) = ctx else {
        return None;
    };
    app.interaction
        .data
        .options()
        .into_iter()
        .find(|o| o.name == name)
        .and_then(|o| match o.value {
            serenity::ResolvedValue::String(s) => Some(s.to_string()),
            _ => None,
        })
}

fn matching(
    items: impl IntoIterator<Item = (String, String)>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
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

async fn ac_action(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    matching(
        ACTIONS
            .iter()
            .map(|(k, l)| ((*l).to_string(), (*k).to_string())),
        partial,
    )
}

/// The `value` suggestions depend on the chosen action.
async fn ac_value(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    match option_str(ctx, "action").as_deref() {
        Some("add-character") => ac_character(ctx, partial).await,
        Some("add-work") => any_autocomplete(ctx, partial).await,
        _ => Vec::new(),
    }
}

/// Show a profile: yours, or `user`'s (respecting their privacy setting).
async fn show(ctx: Context<'_>, user: Option<serenity::User>) -> Result<(), Error> {
    begin_public(ctx).await?;
    let pool = pool(ctx);
    let target = user.unwrap_or_else(|| ctx.author().clone());
    let owner = target.id.get() as i64;
    if owner == uid(ctx) {
        pdb::create_profile(&pool, owner).await?;
    }
    let Some(profile) = pdb::get_profile(&pool, owner).await? else {
        return say_ephemeral(ctx, i18n::PROFILE_NONE_OTHER).await;
    };
    if !pdb::can_view(&pool, uid(ctx), &profile).await? {
        return say_ephemeral(ctx, i18n::PROFILE_PRIVATE).await;
    }
    let mut view = view::ProfileView::new(
        ctx,
        profile,
        owner,
        target.display_name().to_string(),
        Some(target.face()),
        Section::Characters,
    );
    view.load().await?;
    crate::ui::session::run(ctx, view).await
}

/// View a profile or add a character or work to yours. Edits are in /edit.
#[poise::command(
    slash_command,
    name_localized("ar", "بروفايل"),
    description_localized(
        "ar",
        "اعرض بروفايلك أو بروفايل عضو، أو أضف شخصية أو عملًا (التعديل من /edit)"
    )
)]
pub async fn profile(
    ctx: Context<'_>,
    #[description = "Whose profile to show (default: yours)"]
    #[description_localized("ar", "صاحب البروفايل (الافتراضي: أنت)")]
    #[name_localized("ar", "المستخدم")]
    user: Option<serenity::User>,
    #[description = "What to do: view, add a character or add a work"]
    #[description_localized("ar", "الإجراء: عرض، إضافة شخصية أو إضافة عمل")]
    #[name_localized("ar", "الإجراء")]
    #[autocomplete = "ac_action"]
    action: Option<String>,
    #[description = "The character or work to add"]
    #[description_localized("ar", "الشخصية أو العمل المراد إضافته")]
    #[name_localized("ar", "القيمة")]
    #[autocomplete = "ac_value"]
    value: Option<String>,
) -> Result<(), Error> {
    let action = action.as_deref().map(str::trim).unwrap_or("view");
    match action {
        "" | "view" => show(ctx, user).await,
        "add-character" | "add-work" => {
            let Some(v) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) else {
                ctx.defer_ephemeral().await?;
                return private_reply(ctx, i18n::PROFILE_NEEDS_VALUE).await;
            };
            if action == "add-character" {
                manage::add(ctx, v.to_string()).await
            } else {
                actions::add_work(ctx, v).await
            }
        }
        _ => {
            ctx.defer_ephemeral().await?;
            private_reply(ctx, i18n::PROFILE_PICK_ACTION).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_has_only_view_and_add() {
        assert_eq!(
            ACTIONS.iter().map(|a| a.0).collect::<Vec<_>>(),
            vec!["view", "add-character", "add-work"]
        );
    }

    #[test]
    fn matching_filters_by_label_or_key() {
        let items = vec![
            ("إضافة".to_string(), "add".to_string()),
            ("عرض".to_string(), "view".to_string()),
        ];
        assert_eq!(matching(items.clone(), "").len(), 2);
        assert_eq!(matching(items.clone(), "vie").len(), 1);
        assert_eq!(matching(items.clone(), "إضاف").len(), 1);
        assert_eq!(matching(items, "zzz").len(), 0);
    }
}
