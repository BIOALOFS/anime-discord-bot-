//! `/profile`: one command with chained options (`category` -> `action` -> `value`, plus `image`/`text`).
//!
//! With no options it shows a profile (yours, or `user`'s). The autocomplete of `action` depends on the
//! chosen `category`, and the autocomplete of `value` on `category` + `action`.

mod actions;
mod groups;
mod groups_ac;
mod imaging;
pub(crate) mod items;
mod manage;
mod reorder;
mod social;
mod util;
mod view;

pub use manage::add_and_cache;
pub use util::now;

use crate::{
    commands::{character::ac_character, media::any_autocomplete, policy, say_ephemeral},
    db::profile::{self as pdb, Privacy},
    error::{Context, Error},
    i18n,
    render::THEMES,
    ui::{autocomplete::to_choices, styles::Style},
};
use items::Section;
use poise::serenity_prelude as serenity;
use util::{ac_own, ac_own_work, begin_public, pool, private_reply, uid};

/// Public defer for commands outside this module that still want the guild remembered.
pub async fn begin_public_for_top(ctx: Context<'_>) -> Result<(), Error> {
    begin_public(ctx).await
}

/// What the `value` option holds for an action (drives its autocomplete).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    None,
    Text,
    AnilistCharacter,
    AnilistWork,
    OwnCharacter,
    OwnWork,
    Group,
    Style,
    Template,
    Privacy,
}

pub struct ActionDef {
    pub key: &'static str,
    pub label: &'static str,
    pub value: ValueKind,
}

pub struct Category {
    pub key: &'static str,
    pub label: &'static str,
    pub actions: &'static [ActionDef],
}

const fn a(key: &'static str, label: &'static str, value: ValueKind) -> ActionDef {
    ActionDef { key, label, value }
}

use ValueKind as V;

pub const CATEGORIES: &[Category] = &[
    Category {
        key: "customize",
        label: "تخصيص",
        actions: &[
            a("banner", "الغلاف (شخصية أو رابط)", V::AnilistCharacter),
            a("color", "اللون", V::Text),
            a("bio", "النبذة", V::Text),
            a("quote", "الاقتباس", V::Text),
            a("style", "نمط العرض", V::Style),
            a("template", "القالب", V::Template),
            a("character-image", "صورة مخصصة لشخصية", V::OwnCharacter),
            a(
                "reset-character-image",
                "حذف الصورة المخصصة",
                V::OwnCharacter,
            ),
            a("character-title", "لقب شخصية", V::OwnCharacter),
            a("character-description", "وصف شخصي لشخصية", V::OwnCharacter),
            a(
                "reset-character",
                "إعادة تعيين تخصيص شخصية",
                V::OwnCharacter,
            ),
        ],
    },
    Category {
        key: "add",
        label: "إضافة",
        actions: &[
            a("character", "شخصية", V::AnilistCharacter),
            a("work", "عمل مفضّل", V::AnilistWork),
            a("bulk", "إضافة عدة شخصيات", V::None),
        ],
    },
    Category {
        key: "organize",
        label: "تنظيم",
        actions: &[
            a("reorder-characters", "ترتيب الشخصيات", V::None),
            a("reorder-works", "ترتيب الأعمال", V::None),
            a("move-character", "نقل شخصية", V::OwnCharacter),
            a("move-work", "نقل عمل", V::OwnWork),
            a("remove-character", "إزالة شخصية", V::OwnCharacter),
            a("remove-work", "إزالة عمل", V::OwnWork),
            a("clear-characters", "مسح كل الشخصيات", V::None),
            a("clear-works", "مسح كل الأعمال", V::None),
            a("group-create", "إنشاء مجموعة", V::Text),
            a("group-rename", "تسمية مجموعة", V::Group),
            a("group-delete", "حذف مجموعة", V::Group),
            a("group-assign", "وضع شخصية في مجموعة", V::OwnCharacter),
            a("group-assign-work", "وضع عمل في مجموعة", V::OwnWork),
            a("rank-character", "تصنيف شخصية", V::OwnCharacter),
            a("rank-work", "تصنيف عمل", V::OwnWork),
            a("note-character", "ملاحظة على شخصية", V::OwnCharacter),
            a("note-work", "ملاحظة على عمل", V::OwnWork),
        ],
    },
    Category {
        key: "share",
        label: "مشاركة",
        actions: &[
            a("card", "بطاقة 1200×630", V::None),
            a("collage", "صور الشبكة", V::None),
        ],
    },
    Category {
        key: "privacy",
        label: "الخصوصية",
        actions: &[
            a("visibility", "من يرى بروفايلي", V::Privacy),
            a("export", "تصدير JSON", V::None),
            a("import", "استيراد JSON (أرفق الملف)", V::None),
            a("delete-data", "حذف كل بياناتي", V::None),
        ],
    },
    Category {
        key: "compare",
        label: "مقارنة",
        actions: &[a("with", "مقارنة مع عضو (خيار user)", V::None)],
    },
    Category {
        key: "lists",
        label: "القوائم",
        actions: &[
            a("watched", "شاهدتها", V::OwnWork),
            a("planning", "أخطط لمشاهدتها", V::OwnWork),
            a("remove", "إزالة من القوائم", V::OwnWork),
            a("show", "عرض القوائم", V::None),
        ],
    },
];

pub fn find_action(category: &str, action: &str) -> Option<&'static ActionDef> {
    CATEGORIES
        .iter()
        .find(|c| c.key == category)?
        .actions
        .iter()
        .find(|x| x.key == action)
}

pub fn value_kind(category: Option<&str>, action: Option<&str>) -> ValueKind {
    match (category, action) {
        (Some(c), Some(x)) => find_action(c, x).map_or(V::None, |d| d.value),
        _ => V::None,
    }
}

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

async fn ac_category(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    matching(
        CATEGORIES
            .iter()
            .map(|c| (c.label.to_string(), c.key.to_string())),
        partial,
    )
}

async fn ac_action(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    let Some(cat) = option_str(ctx, "category") else {
        return Vec::new();
    };
    let Some(c) = CATEGORIES.iter().find(|c| c.key == cat) else {
        return Vec::new();
    };
    matching(
        c.actions
            .iter()
            .map(|x| (x.label.to_string(), x.key.to_string())),
        partial,
    )
}

async fn ac_value(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let kind = value_kind(
        option_str(ctx, "category").as_deref(),
        option_str(ctx, "action").as_deref(),
    );
    match kind {
        V::AnilistCharacter => ac_character(ctx, partial).await,
        V::AnilistWork => any_autocomplete(ctx, partial).await,
        V::OwnCharacter => ac_own(ctx, partial).await,
        V::OwnWork => ac_own_work(ctx, partial).await,
        V::Group => groups_ac::ac_group(ctx, partial).await,
        V::Style => matching(
            Style::ALL
                .iter()
                .map(|s| (s.label().to_string(), s.as_str().to_string())),
            partial,
        ),
        V::Template => matching(
            THEMES.iter().map(|t| (t.to_string(), t.to_string())),
            partial,
        ),
        V::Privacy => matching(
            [
                ("public", "عام"),
                ("servers", "السيرفرات المشتركة"),
                ("private", "خاص"),
            ]
            .map(|(k, l)| (l.to_string(), k.to_string())),
            partial,
        ),
        V::None | V::Text => Vec::new(),
    }
}

async fn need<'a>(
    ctx: Context<'_>,
    v: &'a Option<String>,
    hint: &str,
) -> Result<Option<&'a str>, Error> {
    match v.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Ok(Some(s)),
        None => {
            private_reply(ctx, hint).await?;
            Ok(None)
        }
    }
}

/// Show a profile: yours, or `user`'s (respecting their privacy setting).
async fn show(ctx: Context<'_>, user: Option<serenity::User>) -> Result<(), Error> {
    begin_public(ctx).await?;
    let pool = pool(ctx);
    let target = user.unwrap_or_else(|| ctx.author().clone());
    let owner = target.id.get() as i64;
    let own = owner == uid(ctx);
    if own {
        pdb::create_profile(&pool, owner).await?;
    }
    let Some(profile) = pdb::get_profile(&pool, owner).await? else {
        return say_ephemeral(ctx, i18n::PROFILE_NONE_OTHER).await;
    };
    if !pdb::can_view(&pool, uid(ctx), &profile).await? {
        return say_ephemeral(ctx, i18n::PROFILE_PRIVATE).await;
    }
    let style = Style::parse(&profile.default_style).unwrap_or(Style::Collage);
    let mut view = actions::new_view(
        ctx,
        profile,
        owner,
        target.display_name().to_string(),
        Some(target.face()),
        None,
        style,
        Section::Characters,
    );
    view.load().await?;
    crate::ui::session::run(ctx, view).await
}

/// Your global profile: characters and works, customization, sharing, privacy.
#[poise::command(
    slash_command,
    name_localized("ar", "بروفايل"),
    description_localized("ar", "بروفايلك العام: عرض وتخصيص وإضافة وتنظيم ومشاركة")
)]
pub async fn profile(
    ctx: Context<'_>,
    #[description = "Whose profile to show (default: yours)"]
    #[description_localized("ar", "صاحب البروفايل (الافتراضي: أنت)")]
    #[name_localized("ar", "المستخدم")]
    user: Option<serenity::User>,
    #[description = "Category: customize, add, organize, share, privacy, compare, lists"]
    #[description_localized("ar", "الفئة: تخصيص، إضافة، تنظيم، مشاركة، خصوصية، مقارنة، قوائم")]
    #[name_localized("ar", "الفئة")]
    #[autocomplete = "ac_category"]
    category: Option<String>,
    #[description = "Action inside the category"]
    #[description_localized("ar", "الإجراء داخل الفئة")]
    #[name_localized("ar", "الإجراء")]
    #[autocomplete = "ac_action"]
    action: Option<String>,
    #[description = "Character, work, group or value"]
    #[description_localized("ar", "الشخصية أو العمل أو المجموعة أو القيمة")]
    #[name_localized("ar", "القيمة")]
    #[autocomplete = "ac_value"]
    value: Option<String>,
    #[description = "Image (custom image) or JSON file (import)"]
    #[description_localized("ar", "صورة (للصورة المخصصة) أو ملف JSON (للاستيراد)")]
    #[name_localized("ar", "الصورة")]
    image: Option<serenity::Attachment>,
    #[description = "Free text: title, description, note, bio, position, tier..."]
    #[description_localized("ar", "نص حر: لقب أو وصف أو ملاحظة أو نبذة أو مركز أو فئة")]
    #[name_localized("ar", "النص")]
    text: Option<String>,
) -> Result<(), Error> {
    let Some(category) = category.as_deref().map(str::trim).filter(|c| !c.is_empty()) else {
        return show(ctx, user).await;
    };
    let action_key = action.as_deref().map(str::trim).unwrap_or("");
    if find_action(category, action_key).is_none() {
        ctx.defer_ephemeral().await?;
        return private_reply(ctx, i18n::PROFILE_PICK_ACTION).await;
    }
    let pick = i18n::PROFILE_NEEDS_VALUE;
    let txt = i18n::PROFILE_NEEDS_TEXT;
    match (category, action_key) {
        // -- add
        ("add", "character") => {
            let Some(v) = need_early(ctx, &value, pick).await? else {
                return Ok(());
            };
            manage::add(ctx, v.to_string()).await
        }
        ("add", "work") => {
            let Some(v) = need_early(ctx, &value, pick).await? else {
                return Ok(());
            };
            actions::add_work(ctx, v).await
        }
        ("add", "bulk") => manage::bulk_add(ctx).await,
        // -- customize
        ("customize", "banner") => {
            let Some(v) = need_early(ctx, &value, pick).await? else {
                return Ok(());
            };
            manage::set_banner(ctx, v.to_string()).await
        }
        ("customize", "color") => {
            let Some(t) = need_early(ctx, &text, txt).await? else {
                return Ok(());
            };
            manage::set_color(ctx, t.to_string()).await
        }
        ("customize", "bio") => {
            let Some(t) = need_early(ctx, &text, txt).await? else {
                return Ok(());
            };
            manage::set_bio(ctx, t.to_string()).await
        }
        ("customize", "quote") => {
            let Some(t) = need_early(ctx, &text, txt).await? else {
                return Ok(());
            };
            actions::set_quote(ctx, t).await
        }
        ("customize", "style") => {
            let Some(v) = need_early(ctx, &value, pick).await? else {
                return Ok(());
            };
            set_style(ctx, v).await
        }
        ("customize", "template") => {
            let Some(v) = need_early(ctx, &value, pick).await? else {
                return Ok(());
            };
            actions::set_template(ctx, v).await
        }
        ("customize", "character-image") => {
            let Some(v) = need_early(ctx, &value, pick).await? else {
                return Ok(());
            };
            actions::character_image(ctx, v, image).await
        }
        ("customize", "reset-character-image") => {
            let Some(v) = need_early(ctx, &value, pick).await? else {
                return Ok(());
            };
            actions::reset_character_image(ctx, v).await
        }
        ("customize", "character-title") => {
            let (Some(v), Some(t)) = (
                need_early(ctx, &value, pick).await?,
                need_early(ctx, &text, txt).await?,
            ) else {
                return Ok(());
            };
            actions::character_title(ctx, v, t).await
        }
        ("customize", "character-description") => {
            let (Some(v), Some(t)) = (
                need_early(ctx, &value, pick).await?,
                need_early(ctx, &text, txt).await?,
            ) else {
                return Ok(());
            };
            actions::character_description(ctx, v, t).await
        }
        ("customize", "reset-character") => {
            let Some(v) = need_early(ctx, &value, pick).await? else {
                return Ok(());
            };
            actions::reset_character(ctx, v).await
        }
        // -- organize
        ("organize", "reorder-characters") => reorder::reorder(ctx, Section::Characters).await,
        ("organize", "reorder-works") => reorder::reorder(ctx, Section::Works).await,
        ("organize", "move-character") => {
            let (Some(v), Some(t)) = (
                need_early(ctx, &value, pick).await?,
                need_early(ctx, &text, i18n::PROFILE_NEEDS_POSITION).await?,
            ) else {
                return Ok(());
            };
            let Ok(pos) = t.parse::<i64>() else {
                return private_early(ctx, i18n::PROFILE_NEEDS_POSITION).await;
            };
            manage::move_to(ctx, v.to_string(), pos).await
        }
        ("organize", "move-work") => {
            let (Some(v), Some(t)) = (
                need_early(ctx, &value, pick).await?,
                need_early(ctx, &text, i18n::PROFILE_NEEDS_POSITION).await?,
            ) else {
                return Ok(());
            };
            let Ok(pos) = t.parse::<i64>() else {
                return private_early(ctx, i18n::PROFILE_NEEDS_POSITION).await;
            };
            actions::move_work(ctx, v, pos).await
        }
        ("organize", "remove-character") => {
            let Some(v) = need_early(ctx, &value, pick).await? else {
                return Ok(());
            };
            manage::remove(ctx, v.to_string()).await
        }
        ("organize", "remove-work") => {
            let Some(v) = need_early(ctx, &value, pick).await? else {
                return Ok(());
            };
            actions::remove_work(ctx, v).await
        }
        ("organize", "clear-characters") => manage::clear(ctx).await,
        ("organize", "clear-works") => manage::clear_works(ctx).await,
        ("organize", "group-create") => {
            let Some(t) = need_early(ctx, &text, txt).await?.or(value.as_deref()) else {
                return Ok(());
            };
            groups::group_create(ctx, t.to_string()).await
        }
        ("organize", "group-rename") => {
            let (Some(v), Some(t)) = (
                need_early(ctx, &value, pick).await?,
                need_early(ctx, &text, txt).await?,
            ) else {
                return Ok(());
            };
            groups::group_rename(ctx, v.to_string(), t.to_string()).await
        }
        ("organize", "group-delete") => {
            let Some(v) = need_early(ctx, &value, pick).await? else {
                return Ok(());
            };
            groups::group_delete(ctx, v.to_string()).await
        }
        ("organize", "group-assign") => {
            let (Some(v), Some(t)) = (
                need_early(ctx, &value, pick).await?,
                need_early(ctx, &text, txt).await?,
            ) else {
                return Ok(());
            };
            groups::group_assign(ctx, v.to_string(), t.to_string()).await
        }
        ("organize", "group-assign-work") => {
            let (Some(v), Some(t)) = (
                need_early(ctx, &value, pick).await?,
                need_early(ctx, &text, txt).await?,
            ) else {
                return Ok(());
            };
            groups::group_assign_work(ctx, v, t).await
        }
        ("organize", "rank-character") => {
            let (Some(v), Some(t)) = (
                need_early(ctx, &value, pick).await?,
                need_early(ctx, &text, txt).await?,
            ) else {
                return Ok(());
            };
            manage::rank(ctx, v.to_string(), t.to_string()).await
        }
        ("organize", "rank-work") => {
            let (Some(v), Some(t)) = (
                need_early(ctx, &value, pick).await?,
                need_early(ctx, &text, txt).await?,
            ) else {
                return Ok(());
            };
            actions::rank_work(ctx, v, t).await
        }
        ("organize", "note-character") => {
            let (Some(v), Some(t)) = (
                need_early(ctx, &value, pick).await?,
                need_early(ctx, &text, txt).await?,
            ) else {
                return Ok(());
            };
            manage::note(ctx, v.to_string(), t.to_string()).await
        }
        ("organize", "note-work") => {
            let (Some(v), Some(t)) = (
                need_early(ctx, &value, pick).await?,
                need_early(ctx, &text, txt).await?,
            ) else {
                return Ok(());
            };
            actions::note_work(ctx, v, t).await
        }
        // -- share
        ("share", "card") => actions::share(ctx, true).await,
        ("share", "collage") => actions::share(ctx, false).await,
        // -- privacy
        ("privacy", "visibility") => {
            let Some(v) = need_early(ctx, &value, pick).await? else {
                return Ok(());
            };
            set_visibility(ctx, v).await
        }
        ("privacy", "export") => social::export(ctx).await,
        ("privacy", "import") => match image {
            Some(file) => social::import(ctx, file).await,
            None => private_early(ctx, i18n::IMPORT_BAD_FILE).await,
        },
        ("privacy", "delete-data") => actions::delete_data(ctx).await,
        // -- compare
        ("compare", "with") => match user {
            Some(u) => social::compare(ctx, u).await,
            None => private_early(ctx, i18n::PROFILE_NEEDS_USER).await,
        },
        // -- lists
        ("lists", "watched") | ("lists", "planning") => {
            let Some(v) = need_early(ctx, &value, pick).await? else {
                return Ok(());
            };
            actions::set_list(ctx, v, Some(action_key)).await
        }
        ("lists", "remove") => {
            let Some(v) = need_early(ctx, &value, pick).await? else {
                return Ok(());
            };
            actions::set_list(ctx, v, None).await
        }
        ("lists", "show") => actions::show_lists(ctx).await,
        _ => {
            ctx.defer_ephemeral().await?;
            private_reply(ctx, i18n::PROFILE_PICK_ACTION).await
        }
    }
}

/// Like `need`, but defers privately first so the refusal is ephemeral.
async fn need_early<'a>(
    ctx: Context<'_>,
    v: &'a Option<String>,
    hint: &str,
) -> Result<Option<&'a str>, Error> {
    match v.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Ok(Some(s)),
        None => {
            ctx.defer_ephemeral().await?;
            need(ctx, v, hint).await
        }
    }
}

async fn private_early(ctx: Context<'_>, text: &str) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    private_reply(ctx, text).await
}

async fn set_style(ctx: Context<'_>, value: &str) -> Result<(), Error> {
    util::begin_private(ctx).await?;
    let Some(style) = Style::parse(value) else {
        return private_reply(ctx, i18n::STYLE_UNKNOWN).await;
    };
    if util::require_own(ctx).await?.is_none() {
        return Ok(());
    }
    pdb::set_style(&pool(ctx), uid(ctx), style.as_str()).await?;
    private_reply(ctx, i18n::STYLE_SET).await
}

async fn set_visibility(ctx: Context<'_>, value: &str) -> Result<(), Error> {
    util::begin_private(ctx).await?;
    let Some(level) = Privacy::parse(value) else {
        return private_reply(ctx, i18n::PRIVACY_UNKNOWN).await;
    };
    if util::require_own(ctx).await?.is_none() {
        return Ok(());
    }
    pdb::set_privacy(&pool(ctx), uid(ctx), level).await?;
    private_reply(ctx, i18n::PRIVACY_SET).await
}

#[allow(dead_code)]
fn _policy(ctx: Context<'_>) -> crate::anilist::ContentPolicy {
    policy(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_is_unique_and_reachable() {
        let mut seen = std::collections::HashSet::new();
        for c in CATEGORIES {
            assert!(!c.actions.is_empty() && !c.label.is_empty());
            for a in c.actions {
                assert!(seen.insert((c.key, a.key)), "duplicate {}/{}", c.key, a.key);
                assert!(find_action(c.key, a.key).is_some());
                assert!(a.label.chars().count() <= 100 && a.key.chars().count() <= 100);
            }
        }
        assert!(find_action("customize", "nope").is_none());
        assert!(find_action("nope", "banner").is_none());
    }

    #[test]
    fn value_autocomplete_depends_on_category_and_action() {
        assert_eq!(
            value_kind(Some("add"), Some("character")),
            ValueKind::AnilistCharacter
        );
        assert_eq!(
            value_kind(Some("add"), Some("work")),
            ValueKind::AnilistWork
        );
        assert_eq!(
            value_kind(Some("organize"), Some("move-work")),
            ValueKind::OwnWork
        );
        assert_eq!(
            value_kind(Some("customize"), Some("character-title")),
            ValueKind::OwnCharacter
        );
        assert_eq!(
            value_kind(Some("customize"), Some("template")),
            ValueKind::Template
        );
        assert_eq!(
            value_kind(Some("privacy"), Some("visibility")),
            ValueKind::Privacy
        );
        assert_eq!(
            value_kind(Some("lists"), Some("watched")),
            ValueKind::OwnWork
        );
        assert_eq!(value_kind(Some("share"), Some("card")), ValueKind::None);
        assert_eq!(value_kind(None, None), ValueKind::None);
        assert_eq!(value_kind(Some("add"), None), ValueKind::None);
    }

    #[test]
    fn matching_filters_by_label_or_key() {
        let items = vec![
            ("إضافة".to_string(), "add".to_string()),
            ("تنظيم".to_string(), "organize".to_string()),
        ];
        assert_eq!(matching(items.clone(), "").len(), 2);
        assert_eq!(matching(items.clone(), "org").len(), 1);
        assert_eq!(matching(items.clone(), "إضاف").len(), 1);
        assert_eq!(matching(items, "zzz").len(), 0);
    }
}
