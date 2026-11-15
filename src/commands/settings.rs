//! `/settings section option value` (Manage Guild): command channel, airing feed, notification list,
//! custom images and moderation, games, birthdays and safety.

use super::{media::media_autocomplete, profile::option_str};
use crate::{
    anilist::{MediaKind, SafeAniList},
    db::{
        custom,
        feed::{self, ListMode, ALL_FORMATS},
    },
    error::{Context, Error},
    guild::{ExtraResult, MAX_EXTRA_CHANNELS},
    i18n,
    ui::{autocomplete::to_choices, embeds::Emb},
};
use poise::serenity_prelude as serenity;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    None,
    Channel,
    Role,
    Number,
    OnOff,
    Anime,
    ListedWork,
    Request,
    ReportedImage,
    User,
    Mode,
    Text,
}

pub struct Opt {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
}

pub struct Section {
    pub key: &'static str,
    pub label: &'static str,
    pub options: &'static [Opt],
}

const fn o(key: &'static str, label: &'static str, kind: Kind) -> Opt {
    Opt { key, label, kind }
}

pub const SECTIONS: &[Section] = &[
    Section {
        key: "channel",
        label: "قناة الأوامر",
        options: &[
            o("set", "تحديد القناة الوحيدة", Kind::Channel),
            o("add-extra", "إضافة قناة مسموحة", Kind::Channel),
            o("remove-extra", "إزالة قناة مسموحة", Kind::Channel),
            o("admin-bypass", "تجاوز المشرفين", Kind::OnOff),
            o("reset", "إلغاء القيد", Kind::None),
            o("show", "عرض الإعدادات", Kind::None),
        ],
    },
    Section {
        key: "feed",
        label: "تغذية الحلقات",
        options: &[
            o("channel", "قناة التغذية", Kind::Channel),
            o("toggle", "تشغيل/إيقاف", Kind::OnOff),
            o("min-popularity", "أدنى شعبية (افتراضي 1500)", Kind::Number),
            o("formats", "الصيغ (TV,TV_SHORT,ONA,MOVIE)", Kind::Text),
            o(
                "exclude-genres",
                "تصنيفات مستبعدة (مفصولة بفواصل)",
                Kind::Text,
            ),
            o("digest-hours", "ملخص كل N ساعة (0 = إيقاف)", Kind::Number),
            o("daily-cap", "الحد اليومي (0 = بلا حد)", Kind::Number),
            o("role", "رتبة الإشارة (- للحذف)", Kind::Role),
            o("mode", "الوضع: all أو listed", Kind::Mode),
            o("show", "عرض إعدادات التغذية", Kind::None),
        ],
    },
    Section {
        key: "alerts",
        label: "قائمة الإشعارات",
        options: &[
            o("add", "إضافة أنمي", Kind::Anime),
            o("pin", "تثبيت أنمي (ينشر دائمًا)", Kind::Anime),
            o("mute", "كتم أنمي", Kind::Anime),
            o("remove", "إزالة من القائمة", Kind::ListedWork),
            o("list", "عرض القائمة", Kind::None),
            o("requests", "طلبات الأعضاء", Kind::None),
            o("approve", "قبول طلب", Kind::Request),
            o("deny", "رفض طلب", Kind::Request),
        ],
    },
    Section {
        key: "images",
        label: "الصور المخصصة",
        options: &[
            o("toggle", "تفعيل/تعطيل الصور المخصصة", Kind::OnOff),
            o("reports", "عرض البلاغات", Kind::None),
            o("remove-image", "حذف صورة مبلّغ عنها", Kind::ReportedImage),
            o("ban", "منع عضو من الرفع", Kind::User),
            o("unban", "رفع المنع عن عضو", Kind::User),
        ],
    },
    Section {
        key: "games",
        label: "الألعاب",
        options: &[o("toggle", "تشغيل/إيقاف الألعاب", Kind::OnOff)],
    },
    Section {
        key: "birthdays",
        label: "أعياد الميلاد",
        options: &[
            o("channel", "قناة المنشور اليومي", Kind::Channel),
            o("toggle", "تشغيل/إيقاف المنشور", Kind::OnOff),
        ],
    },
    Section {
        key: "safety",
        label: "الأمان",
        options: &[
            o(
                "nsfw-mode",
                "وضع NSFW (يبقى المحتوى آمنًا افتراضيًا)",
                Kind::OnOff,
            ),
            o("show", "عرض حالة الأمان", Kind::None),
        ],
    },
];

pub fn find(section: &str, option: &str) -> Option<&'static Opt> {
    SECTIONS
        .iter()
        .find(|s| s.key == section)?
        .options
        .iter()
        .find(|x| x.key == option)
}

fn kind_of_option(section: Option<&str>, option: Option<&str>) -> Kind {
    match (section, option) {
        (Some(s), Some(x)) => find(s, x).map_or(Kind::None, |d| d.kind),
        _ => Kind::None,
    }
}

// ---- pure parsing helpers ---------------------------------------------------------------------

/// `<#123>`, `#123` or `123`.
pub fn parse_id(input: &str) -> Option<i64> {
    let t = input
        .trim()
        .trim_start_matches("<#")
        .trim_start_matches("<@&")
        .trim_start_matches("<@!")
        .trim_start_matches("<@");
    let t = t.trim_end_matches('>').trim_start_matches('#');
    t.parse::<i64>().ok().filter(|n| *n > 0)
}

pub fn parse_on_off(input: &str) -> Option<bool> {
    match input.trim().to_lowercase().as_str() {
        "on" | "true" | "yes" | "1" | "تشغيل" | "نعم" | "تفعيل" => Some(true),
        "off" | "false" | "no" | "0" | "إيقاف" | "لا" | "تعطيل" => Some(false),
        _ => None,
    }
}

pub fn parse_csv(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_string)
        .collect()
}

/// `id:123` from the anime autocomplete, or a bare number.
fn parse_work(input: &str) -> Option<i64> {
    super::parse_id(input).or_else(|| input.trim().parse::<i64>().ok())
}

// ---- autocomplete ---------------------------------------------------------------------------------

async fn ac_section(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    let n = partial.trim().to_lowercase();
    to_choices(
        SECTIONS
            .iter()
            .filter(|s| n.is_empty() || s.label.contains(&n) || s.key.contains(&n))
            .map(|s| (s.label.to_string(), s.key.to_string()))
            .collect(),
    )
}

async fn ac_option(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    let Some(sec) = option_str(ctx, "section") else {
        return Vec::new();
    };
    let Some(s) = SECTIONS.iter().find(|s| s.key == sec) else {
        return Vec::new();
    };
    let n = partial.trim().to_lowercase();
    to_choices(
        s.options
            .iter()
            .filter(|x| n.is_empty() || x.label.contains(&n) || x.key.contains(&n))
            .map(|x| (x.label.to_string(), x.key.to_string()))
            .collect(),
    )
}

async fn ac_value(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    let kind = kind_of_option(
        option_str(ctx, "section").as_deref(),
        option_str(ctx, "option").as_deref(),
    );
    let needle = partial.trim().to_lowercase();
    let guild = ctx.guild_id().map(|g| g.get() as i64);
    match kind {
        Kind::Channel => {
            // Collected first: the cache guard must not live across an await.
            let list: Vec<(String, String)> = ctx
                .guild()
                .map(|g| {
                    g.channels
                        .values()
                        .filter(|c| {
                            matches!(
                                c.kind,
                                serenity::ChannelType::Text | serenity::ChannelType::News
                            )
                        })
                        .map(|c| (format!("#{}", c.name), format!("<#{}>", c.id)))
                        .collect()
                })
                .unwrap_or_default();
            to_choices(
                list.into_iter()
                    .filter(|(l, _)| needle.is_empty() || l.to_lowercase().contains(&needle))
                    .take(25)
                    .collect(),
            )
        }
        Kind::Role => {
            let list: Vec<(String, String)> = ctx
                .guild()
                .map(|g| {
                    g.roles
                        .values()
                        .filter(|r| !r.managed)
                        .map(|r| (format!("@{}", r.name), format!("<@&{}>", r.id)))
                        .collect()
                })
                .unwrap_or_default();
            let mut list: Vec<_> = list
                .into_iter()
                .filter(|(l, _)| needle.is_empty() || l.to_lowercase().contains(&needle))
                .take(24)
                .collect();
            list.push(("حذف الرتبة".to_string(), "-".to_string()));
            to_choices(list)
        }
        Kind::OnOff => to_choices(vec![
            ("تشغيل".to_string(), "on".to_string()),
            ("إيقاف".to_string(), "off".to_string()),
        ]),
        Kind::Mode => to_choices(vec![
            ("كل الأنمي مع المثبّت".to_string(), "all".to_string()),
            ("المدرج في القائمة فقط".to_string(), "listed".to_string()),
        ]),
        Kind::Anime => media_autocomplete(ctx, MediaKind::Anime, "settings-anime", partial).await,
        Kind::ListedWork => match guild {
            Some(g) => feed::list_entries(&ctx.data().db, g).await.map_or_else(
                |_| Vec::new(),
                |l| {
                    to_choices(
                        l.into_iter()
                            .filter(|(_, t, _)| {
                                needle.is_empty() || t.to_lowercase().contains(&needle)
                            })
                            .take(25)
                            .map(|(id, t, _)| (t, format!("id:{id}")))
                            .collect(),
                    )
                },
            ),
            None => Vec::new(),
        },
        Kind::Request => match guild {
            Some(g) => feed::pending_requests(&ctx.data().db, g).await.map_or_else(
                |_| Vec::new(),
                |l| {
                    to_choices(
                        l.into_iter()
                            .map(|(id, t, n)| (format!("{t} ({n})"), format!("id:{id}")))
                            .collect(),
                    )
                },
            ),
            None => Vec::new(),
        },
        Kind::ReportedImage => match guild {
            Some(g) => custom::open_reports(&ctx.data().db, g).await.map_or_else(
                |_| Vec::new(),
                |l| {
                    to_choices(
                        l.into_iter()
                            .map(|(id, owner, n)| {
                                (format!("صورة {id} · {n} بلاغ · <{owner}>"), id.to_string())
                            })
                            .collect(),
                    )
                },
            ),
            None => Vec::new(),
        },
        Kind::Number | Kind::User | Kind::Text | Kind::None => Vec::new(),
    }
}

// ---- the command ---------------------------------------------------------------------------------------

async fn reply(ctx: Context<'_>, text: impl Into<String>) -> Result<(), Error> {
    ctx.send(poise::CreateReply::default().content(text).ephemeral(true))
        .await?;
    Ok(())
}

fn need(v: &Option<String>) -> Option<&str> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// Server settings for admins.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD",
    name_localized("ar", "إعدادات"),
    description_localized("ar", "إعدادات السيرفر (للمشرفين)")
)]
pub async fn settings(
    ctx: Context<'_>,
    #[description = "Section: channel, feed, alerts, images, games, birthdays, safety"]
    #[description_localized("ar", "القسم: قناة، تغذية، إشعارات، صور، ألعاب، أعياد ميلاد، أمان")]
    #[name_localized("ar", "القسم")]
    #[autocomplete = "ac_section"]
    section: String,
    #[description = "Option inside the section"]
    #[description_localized("ar", "الخيار داخل القسم")]
    #[name_localized("ar", "الخيار")]
    #[autocomplete = "ac_option"]
    option: String,
    #[description = "Value (channel, role, number, on/off, anime...)"]
    #[description_localized("ar", "القيمة (قناة، رتبة، رقم، تشغيل/إيقاف، أنمي...)")]
    #[name_localized("ar", "القيمة")]
    #[autocomplete = "ac_value"]
    value: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let Some(guild) = ctx.guild_id().map(|g| g.get() as i64) else {
        return reply(ctx, i18n::GUILD_ONLY).await;
    };
    let Some(def) = find(section.trim(), option.trim()) else {
        return reply(ctx, i18n::SETTINGS_PICK).await;
    };
    let (pool, actor) = (ctx.data().db.clone(), ctx.author().id.get() as i64);
    let store = ctx.data().guilds.clone();
    let v = need(&value);
    // Options that need a value say so before touching anything.
    if def.kind != Kind::None && v.is_none() {
        return reply(ctx, i18n::SETTINGS_NEEDS_VALUE).await;
    }
    let v = v.unwrap_or("");
    let on = parse_on_off(v);
    let chan = parse_id(v);

    match (section.trim(), option.trim()) {
        ("channel", "set") => match chan {
            Some(c) => {
                store.set_command_channel(guild, c).await?;
                reply(ctx, i18n::setup_channel_set(c)).await
            }
            None => reply(ctx, i18n::SETTINGS_BAD_CHANNEL).await,
        },
        ("channel", "add-extra") => match chan {
            Some(c) => match store.add_extra(guild, c).await? {
                ExtraResult::Added => reply(ctx, i18n::setup_extra_added(c)).await,
                ExtraResult::AlreadyThere => reply(ctx, i18n::SETUP_EXTRA_EXISTS).await,
                ExtraResult::Full => reply(ctx, i18n::setup_extra_full(MAX_EXTRA_CHANNELS)).await,
            },
            None => reply(ctx, i18n::SETTINGS_BAD_CHANNEL).await,
        },
        ("channel", "remove-extra") => match chan {
            Some(c) if store.remove_extra(guild, c).await? => {
                reply(ctx, i18n::setup_extra_removed(c)).await
            }
            Some(_) => reply(ctx, i18n::SETUP_EXTRA_MISSING).await,
            None => reply(ctx, i18n::SETTINGS_BAD_CHANNEL).await,
        },
        ("channel", "admin-bypass") => match on {
            Some(b) => {
                store.set_admin_bypass(guild, b).await?;
                reply(ctx, i18n::setup_bypass(b)).await
            }
            None => reply(ctx, i18n::SETTINGS_BAD_TOGGLE).await,
        },
        ("channel", "reset") => {
            store.reset(guild).await?;
            reply(ctx, i18n::SETUP_RESET).await
        }
        ("channel", "show") => show_channels(ctx, guild).await,

        ("feed", "channel") => match chan {
            Some(c) => {
                feed::set_channel(&pool, guild, c).await?;
                reply(ctx, i18n::feed_channel_set(c)).await
            }
            None => reply(ctx, i18n::SETTINGS_BAD_CHANNEL).await,
        },
        ("feed", "toggle") => match on {
            Some(b) => {
                let cfg = feed::get_config(&pool, guild).await?;
                if b && cfg.channel_id.is_none() {
                    return reply(ctx, i18n::FEED_NEEDS_CHANNEL).await;
                }
                feed::set_enabled(&pool, guild, b).await?;
                reply(ctx, if b { i18n::FEED_ON } else { i18n::FEED_OFF }).await
            }
            None => reply(ctx, i18n::SETTINGS_BAD_TOGGLE).await,
        },
        ("feed", "min-popularity") => match v.parse::<i64>() {
            Ok(n) => {
                feed::set_min_popularity(&pool, guild, n).await?;
                reply(ctx, i18n::SETTINGS_SAVED).await
            }
            Err(_) => reply(ctx, i18n::SETTINGS_BAD_NUMBER).await,
        },
        ("feed", "formats") => {
            let formats: Vec<String> = parse_csv(v).into_iter().map(|f| f.to_uppercase()).collect();
            if formats.is_empty() || formats.iter().any(|f| !ALL_FORMATS.contains(&f.as_str())) {
                return reply(ctx, i18n::SETTINGS_BAD_FORMATS).await;
            }
            feed::set_formats(&pool, guild, &formats).await?;
            reply(ctx, i18n::SETTINGS_SAVED).await
        }
        ("feed", "exclude-genres") => {
            let genres = if v == "-" { Vec::new() } else { parse_csv(v) };
            feed::set_excluded_genres(&pool, guild, &genres).await?;
            reply(ctx, i18n::SETTINGS_SAVED).await
        }
        ("feed", "digest-hours") => match v.parse::<i64>() {
            Ok(n) => {
                feed::set_digest_hours(&pool, guild, n).await?;
                reply(ctx, i18n::SETTINGS_SAVED).await
            }
            Err(_) => reply(ctx, i18n::SETTINGS_BAD_NUMBER).await,
        },
        ("feed", "daily-cap") => match v.parse::<i64>() {
            Ok(n) => {
                feed::set_daily_cap(&pool, guild, n).await?;
                reply(ctx, i18n::SETTINGS_SAVED).await
            }
            Err(_) => reply(ctx, i18n::SETTINGS_BAD_NUMBER).await,
        },
        ("feed", "role") => {
            let role = if v == "-" { None } else { chan };
            if role.is_none() && v != "-" {
                return reply(ctx, i18n::SETTINGS_BAD_ROLE).await;
            }
            feed::set_role(&pool, guild, role).await?;
            reply(ctx, i18n::SETTINGS_SAVED).await
        }
        ("feed", "mode") => match v {
            "all" | "listed" => {
                feed::set_only_listed(&pool, guild, v == "listed").await?;
                reply(ctx, i18n::SETTINGS_SAVED).await
            }
            _ => reply(ctx, i18n::SETTINGS_BAD_MODE).await,
        },
        ("feed", "show") => show_feed(ctx, guild).await,

        ("alerts", opt @ ("add" | "pin" | "mute")) => {
            let Some(id) = parse_work(v) else {
                return reply(ctx, i18n::SETTINGS_PICK_ANIME).await;
            };
            let api = ctx.data().anilist.clone();
            let Some(title) = anime_title(&api, id).await else {
                return reply(ctx, i18n::NOT_FOUND).await;
            };
            let mode = match opt {
                "add" => ListMode::Listed,
                "pin" => ListMode::Pinned,
                _ => ListMode::Muted,
            };
            feed::set_list_entry(&pool, guild, id, &title, mode).await?;
            custom::audit(
                &pool,
                Some(guild),
                actor,
                &format!("alerts_{opt}"),
                Some(&id.to_string()),
            )
            .await?;
            reply(ctx, i18n::alert_list_updated(&title, mode)).await
        }
        ("alerts", "remove") => match parse_work(v) {
            Some(id) if feed::remove_list_entry(&pool, guild, id).await? => {
                reply(ctx, i18n::SETTINGS_SAVED).await
            }
            _ => reply(ctx, i18n::NOT_FOUND).await,
        },
        ("alerts", "list") => show_alert_list(ctx, guild).await,
        ("alerts", "requests") => show_requests(ctx, guild).await,
        ("alerts", "approve") => match parse_work(v) {
            Some(id) => {
                let title = feed::pending_requests(&pool, guild)
                    .await?
                    .into_iter()
                    .find(|(w, _, _)| *w == id)
                    .map(|(_, t, _)| t);
                match title {
                    Some(t) => {
                        feed::set_list_entry(&pool, guild, id, &t, ListMode::Listed).await?;
                        reply(ctx, i18n::alert_list_updated(&t, ListMode::Listed)).await
                    }
                    None => reply(ctx, i18n::NOT_FOUND).await,
                }
            }
            None => reply(ctx, i18n::NOT_FOUND).await,
        },
        ("alerts", "deny") => match parse_work(v) {
            Some(id) if feed::deny_request(&pool, guild, id).await? => {
                reply(ctx, i18n::SETTINGS_SAVED).await
            }
            _ => reply(ctx, i18n::NOT_FOUND).await,
        },

        ("images", "toggle") => match on {
            Some(b) => {
                custom::set_custom_images(&pool, guild, b).await?;
                custom::audit(
                    &pool,
                    Some(guild),
                    actor,
                    "custom_images",
                    Some(if b { "on" } else { "off" }),
                )
                .await?;
                reply(
                    ctx,
                    if b {
                        i18n::CUSTOM_IMAGES_ENABLED
                    } else {
                        i18n::CUSTOM_IMAGES_DISABLED
                    },
                )
                .await
            }
            None => reply(ctx, i18n::SETTINGS_BAD_TOGGLE).await,
        },
        ("images", "reports") => {
            let list = custom::open_reports(&pool, guild).await?;
            let body = if list.is_empty() {
                i18n::NO_REPORTS.to_string()
            } else {
                list.iter()
                    .map(|(id, owner, n)| format!("• صورة `{id}` · <@{owner}> · {n} بلاغ"))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            reply(ctx, body).await
        }
        ("images", "remove-image") => match v.parse::<i64>() {
            Ok(id) if custom::remove_image_as_mod(&pool, id, actor, guild).await? => {
                reply(ctx, i18n::IMAGE_REMOVED).await
            }
            _ => reply(ctx, i18n::NOT_FOUND).await,
        },
        ("images", "ban") => match chan {
            Some(u) => {
                custom::ban_uploads(&pool, guild, u, actor).await?;
                reply(ctx, i18n::UPLOAD_BAN_SET).await
            }
            None => reply(ctx, i18n::SETTINGS_BAD_USER).await,
        },
        ("images", "unban") => match chan {
            Some(u) if custom::unban_uploads(&pool, guild, u, actor).await? => {
                reply(ctx, i18n::UPLOAD_BAN_LIFTED).await
            }
            Some(_) => reply(ctx, i18n::NOT_FOUND).await,
            None => reply(ctx, i18n::SETTINGS_BAD_USER).await,
        },

        ("games", "toggle") => match on {
            Some(b) => {
                custom::set_games(&pool, guild, b).await?;
                reply(ctx, if b { i18n::GAMES_ON } else { i18n::GAMES_OFF }).await
            }
            None => reply(ctx, i18n::SETTINGS_BAD_TOGGLE).await,
        },
        ("birthdays", "channel") => match chan {
            Some(c) => {
                feed::set_birthday_channel(&pool, guild, c).await?;
                reply(ctx, i18n::SETTINGS_SAVED).await
            }
            None => reply(ctx, i18n::SETTINGS_BAD_CHANNEL).await,
        },
        ("birthdays", "toggle") => match on {
            Some(b) => {
                feed::set_birthdays_enabled(&pool, guild, b).await?;
                reply(ctx, i18n::SETTINGS_SAVED).await
            }
            None => reply(ctx, i18n::SETTINGS_BAD_TOGGLE).await,
        },
        ("safety", "nsfw-mode") => match on {
            Some(b) => {
                custom::set_nsfw_mode(&pool, guild, b).await?;
                reply(ctx, i18n::NSFW_NOTE).await
            }
            None => reply(ctx, i18n::SETTINGS_BAD_TOGGLE).await,
        },
        ("safety", "show") => {
            let nsfw = custom::nsfw_mode(&pool, guild).await?;
            reply(ctx, i18n::safety_status(nsfw)).await
        }
        _ => reply(ctx, i18n::SETTINGS_PICK).await,
    }
}

async fn anime_title(api: &Arc<SafeAniList>, id: i64) -> Option<String> {
    api.media_by_id(id, crate::anilist::ContentPolicy::SAFE)
        .await
        .ok()
        .flatten()
        .map(|m| m.title.main().to_string())
}

async fn show_channels(ctx: Context<'_>, guild: i64) -> Result<(), Error> {
    let s = ctx.data().guilds.settings(guild).await?;
    let mut deleted = false;
    let main = match s.command_channel {
        Some(id) => {
            let http = &ctx.serenity_context().http;
            if let Err(serenity::Error::Http(e)) =
                serenity::ChannelId::new(id as u64).to_channel(http).await
            {
                deleted = matches!(&e, serenity::HttpError::UnsuccessfulRequest(r) if r.status_code.as_u16() == 404);
            }
            format!("<#{id}>")
        }
        None => i18n::SETUP_NONE.to_string(),
    };
    let extras = if s.extra.is_empty() {
        i18n::NONE.to_string()
    } else {
        s.extra
            .iter()
            .map(|c| format!("<#{c}>"))
            .collect::<Vec<_>>()
            .join("، ")
    };
    let mut embed = Emb::new()
        .title(i18n::SETUP_TITLE)
        .field(i18n::SETUP_MAIN, main, false)
        .field(i18n::SETUP_EXTRA, extras, false)
        .field(
            i18n::SETUP_BYPASS,
            if s.admin_bypass {
                i18n::SETUP_ON
            } else {
                i18n::SETUP_OFF
            },
            true,
        );
    if deleted {
        embed = embed.description(i18n::SETUP_DELETED_WARNING);
    }
    ctx.send(
        poise::CreateReply::default()
            .embed(embed.build())
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

async fn show_feed(ctx: Context<'_>, guild: i64) -> Result<(), Error> {
    let c = feed::get_config(&ctx.data().db, guild).await?;
    let chan = c
        .channel_id
        .map_or(i18n::NONE.to_string(), |id| format!("<#{id}>"));
    let role = c
        .role_id
        .map_or(i18n::NONE.to_string(), |id| format!("<@&{id}>"));
    let embed = Emb::new()
        .title(i18n::FEED_TITLE)
        .field(
            i18n::FEED_STATE,
            if c.enabled {
                i18n::SETUP_ON
            } else {
                i18n::SETUP_OFF
            },
            true,
        )
        .field(i18n::SETUP_MAIN, chan, true)
        .field(i18n::FEED_MIN_POP, c.min_popularity.to_string(), true)
        .field(i18n::FEED_FORMATS, c.formats.join(", "), true)
        .field(
            i18n::FEED_EXCLUDED,
            if c.excluded_genres.is_empty() {
                i18n::NONE.to_string()
            } else {
                c.excluded_genres.join(", ")
            },
            true,
        )
        .field(
            i18n::FEED_DIGEST,
            if c.digest_hours == 0 {
                i18n::SETUP_OFF.to_string()
            } else {
                format!("{}h", c.digest_hours)
            },
            true,
        )
        .field(
            i18n::FEED_CAP,
            if c.daily_cap == 0 {
                i18n::SETUP_OFF.to_string()
            } else {
                c.daily_cap.to_string()
            },
            true,
        )
        .field(i18n::FEED_ROLE, role, true)
        .field(
            i18n::FEED_MODE,
            if c.only_listed {
                i18n::FEED_MODE_LISTED
            } else {
                i18n::FEED_MODE_ALL
            },
            true,
        )
        .build();
    ctx.send(poise::CreateReply::default().embed(embed).ephemeral(true))
        .await?;
    Ok(())
}

async fn show_alert_list(ctx: Context<'_>, guild: i64) -> Result<(), Error> {
    let list = feed::list_entries(&ctx.data().db, guild).await?;
    let body = if list.is_empty() {
        i18n::ALERT_LIST_EMPTY.to_string()
    } else {
        list.iter()
            .map(|(id, t, m)| format!("• {} `{id}` — {}", t, i18n::list_mode_label(*m)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    reply(ctx, body).await
}

async fn show_requests(ctx: Context<'_>, guild: i64) -> Result<(), Error> {
    let list = feed::pending_requests(&ctx.data().db, guild).await?;
    let body = if list.is_empty() {
        i18n::NO_REQUESTS.to_string()
    } else {
        list.iter()
            .map(|(id, t, n)| format!("• {t} `{id}` — {n} طلب"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    reply(ctx, body).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_parsed_from_mentions_and_numbers() {
        assert_eq!(parse_id("<#123>"), Some(123));
        assert_eq!(parse_id("#456"), Some(456));
        assert_eq!(parse_id(" 789 "), Some(789));
        assert_eq!(parse_id("<@&55>"), Some(55));
        assert_eq!(parse_id("<@!7>"), Some(7));
        assert_eq!(parse_id("general"), None);
        assert_eq!(parse_id("-5"), None);
        assert_eq!(parse_id("0"), None);
    }

    #[test]
    fn toggles_accept_arabic_and_english() {
        for yes in ["on", "ON", "true", "تشغيل", "نعم", "1"] {
            assert_eq!(parse_on_off(yes), Some(true), "{yes}");
        }
        for no in ["off", "false", "إيقاف", "لا", "0"] {
            assert_eq!(parse_on_off(no), Some(false), "{no}");
        }
        assert_eq!(parse_on_off("maybe"), None);
    }

    #[test]
    fn csv_and_work_ids() {
        assert_eq!(parse_csv(" a, b ,,c "), vec!["a", "b", "c"]);
        assert_eq!(parse_work("id:42"), Some(42));
        assert_eq!(parse_work("42"), Some(42));
        assert_eq!(parse_work("naruto"), None);
    }

    #[test]
    fn the_option_table_is_consistent() {
        let mut seen = std::collections::HashSet::new();
        for s in SECTIONS {
            for x in s.options {
                assert!(seen.insert((s.key, x.key)), "duplicate {}/{}", s.key, x.key);
                assert!(find(s.key, x.key).is_some());
                assert!(x.label.chars().count() <= 100);
            }
        }
        assert_eq!(kind_of_option(Some("feed"), Some("role")), Kind::Role);
        assert_eq!(
            kind_of_option(Some("alerts"), Some("approve")),
            Kind::Request
        );
        assert_eq!(kind_of_option(Some("games"), Some("toggle")), Kind::OnOff);
        assert_eq!(kind_of_option(None, None), Kind::None);
        assert!(find("images", "toggle").is_some());
        assert!(find("images", "nope").is_none());
    }
}
