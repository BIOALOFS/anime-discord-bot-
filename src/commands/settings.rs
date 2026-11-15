//! `/settings`: personal preferences only (language and privacy). Server administration lives in `/admin`.

use super::{profile::option_str, say_ephemeral};
use crate::{
    db::profile::{self as pdb, Privacy},
    error::{Context, Error},
    i18n,
    ui::autocomplete::to_choices,
};
use poise::serenity_prelude as serenity;

/// `(key, Arabic label)` of the personal settings.
pub const SETTINGS: &[(&str, &str)] = &[("language", "اللغة"), ("privacy", "الخصوصية")];

pub const LANGUAGES: &[(&str, &str)] = &[("ar", "العربية"), ("en", "English")];
pub const PRIVACY_LEVELS: &[(&str, &str)] = &[
    ("public", "عام"),
    ("servers", "السيرفرات المشتركة"),
    ("private", "خاص"),
];

/// Options of one setting as `(value, label)` pairs.
pub fn values_for(setting: &str) -> &'static [(&'static str, &'static str)] {
    match setting {
        "language" => LANGUAGES,
        "privacy" => PRIVACY_LEVELS,
        _ => &[],
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

async fn ac_setting(_ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    matching(
        SETTINGS
            .iter()
            .map(|(k, l)| ((*l).to_string(), (*k).to_string()))
            .collect(),
        partial,
    )
}

async fn ac_value(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let Some(setting) = option_str(ctx, "setting") else {
        return Vec::new();
    };
    matching(
        values_for(&setting)
            .iter()
            .map(|(v, l)| ((*l).to_string(), (*v).to_string()))
            .collect(),
        partial,
    )
}

/// Your personal preferences: language and privacy.
#[poise::command(
    slash_command,
    name_localized("ar", "إعداداتي"),
    description_localized("ar", "تفضيلاتك الشخصية: اللغة والخصوصية")
)]
pub async fn settings(
    ctx: Context<'_>,
    #[description = "Setting: language or privacy"]
    #[description_localized("ar", "الإعداد: اللغة أو الخصوصية")]
    #[name_localized("ar", "الإعداد")]
    #[autocomplete = "ac_setting"]
    setting: String,
    #[description = "New value"]
    #[description_localized("ar", "القيمة الجديدة")]
    #[name_localized("ar", "القيمة")]
    #[autocomplete = "ac_value"]
    value: String,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let user = ctx.author().id.get() as i64;
    let pool = &ctx.data().db;
    pdb::create_profile(pool, user).await?;
    let value = value.trim();
    let text = match setting.trim() {
        "privacy" => match Privacy::parse(value) {
            Some(level) => {
                pdb::set_privacy(pool, user, level).await?;
                i18n::PRIVACY_SET
            }
            None => i18n::PRIVACY_UNKNOWN,
        },
        "language" => match pdb::set_language(pool, user, value).await {
            Ok(()) if value == "ar" => i18n::LANGUAGE_SET,
            Ok(()) => i18n::LANGUAGE_SET_EN,
            Err(_) => i18n::LANGUAGE_UNKNOWN,
        },
        _ => i18n::SETTINGS_UNKNOWN,
    };
    say_ephemeral(ctx, text).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_language_and_privacy_exist() {
        assert_eq!(
            SETTINGS.iter().map(|s| s.0).collect::<Vec<_>>(),
            vec!["language", "privacy"]
        );
        assert_eq!(values_for("language").len(), 2);
        assert_eq!(values_for("privacy").len(), 3);
        assert!(
            values_for("nsfw").is_empty(),
            "admin settings are not personal settings"
        );
    }

    #[test]
    fn every_privacy_value_parses() {
        for (v, _) in PRIVACY_LEVELS {
            assert!(Privacy::parse(v).is_some(), "{v}");
        }
    }
}
