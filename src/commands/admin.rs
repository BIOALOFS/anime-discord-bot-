//! `/admin`: the one admin command. It opens an ephemeral panel; every button opens its own menu with a Back
//! button. Hidden from non-admins through `default_member_permissions`, and checked again at run time.

use super::buttons;
use crate::{
    db::{
        admin as adb, chapters as cdb, custom,
        feed::{self, FeedConfig, FilterMode, ALL_FORMATS, ALL_KINDS},
    },
    error::{Context, Error},
    guild::{self, GuildStore},
    i18n,
    ui::{
        embeds::Emb,
        pagination::{back_button, close_button, BACK, CLOSE},
        session::{self, ModalSpec, Outcome, Page, View},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Safety,
    Notifications,
    Images,
    Roles,
    Channels,
    Limits,
    Chapters,
    Library,
    Status,
    Daily,
    Poll,
    Perms,
    Watch,
    Owner,
    Reset,
}

/// Home buttons: `(id, label, emoji, screen)`.
pub const HOME: &[(&str, &str, char, Screen)] = &[
    ("go:safety", "الأمان", '🛡', Screen::Safety),
    ("go:notify", "الإشعارات", '🔔', Screen::Notifications),
    ("go:images", "الصور المخصصة", '🖼', Screen::Images),
    ("go:roles", "الرتب والصلاحيات", '👥', Screen::Roles),
    ("go:channels", "القنوات", '📌', Screen::Channels),
    ("go:limits", "الحدود", '⏱', Screen::Limits),
    ("go:chapters", "مصادر الفصول", '📖', Screen::Chapters),
    ("go:library", "المسلسلات والجدول", '📺', Screen::Library),
    ("go:status", "حالة المصادر", '📡', Screen::Status),
    ("go:daily", "منشورات يومية", '🗓', Screen::Daily),
    ("go:poll", "تصويت أسبوعي", '🍿', Screen::Poll),
    ("go:perms", "صلاحيات الرتب", '🔐', Screen::Perms),
    ("go:watch", "ليلة مشاهدة", '🎬', Screen::Watch),
    ("go:reset", "إعادة الضبط", '🗑', Screen::Reset),
];

/// Only the application owner sees this button.
pub const OWNER_HOME: (&str, &str, char, Screen) = ("go:owner", "للمالك", '👑', Screen::Owner);

const T_NSFW: &str = "s:nsfw";
const T_MATURE: &str = "s:mature";
const T_GAMES: &str = "s:games";
const N_TOGGLE: &str = "n:toggle";
const N_PING: &str = "n:ping";
const L_SUGGEST: &str = "l:suggest";
const N_MODE: &str = "n:mode";
const N_POP: &str = "n:pop";
const N_FORMATS: &str = "n:formats";
const N_KINDS: &str = "n:kinds";
const I_TOGGLE: &str = "i:toggle";
const I_POLICY: &str = "i:policy";
const R_MANAGER: &str = "r:manager";
const R_PING: &str = "r:ping";
const R_CLEAR_MANAGER: &str = "r:clear_manager";
const R_CLEAR_PING: &str = "r:clear_ping";
const C_CLEAR_BOT: &str = "c:clear_bot";
const C_CLEAR_NOTIFY: &str = "c:clear_notify";
const L_COOLDOWN: &str = "l:cooldown";
const X_CONFIRM: &str = "x:confirm";
const LIB_SPOILER: &str = "lib:spoiler";
const LIB_TZ: &str = "lib:tz";
const LIB_COUNTRY: &str = "lib:country";
const CH_MANGADEX: &str = "ch:mangadex";
const CH_MANGAUPDATES: &str = "ch:mangaupdates";
const CH_RSS: &str = "ch:rss";
const CH_LANG: &str = "ch:lang";
const CH_DIGEST: &str = "ch:digest";
const CH_ADD_FEED: &str = "ch:addfeed";
const CH_REMOVE_FEED: &str = "ch:rmfeed";

/// Everything the panel shows, loaded from the database after each change.
#[derive(Debug, Clone)]
pub struct State {
    pub nsfw: bool,
    pub mature: bool,
    pub games: bool,
    pub images: bool,
    pub upload_admins: bool,
    pub manager_role: Option<i64>,
    pub bot_channel: Option<i64>,
    pub cooldown: i64,
    pub suggest_limit: i64,
    pub reminder_ping: bool,
    pub feed: FeedConfig,
    pub chapters: cdb::Settings,
    pub feeds: Vec<cdb::Feed>,
    pub timezone: String,
    pub spoiler: bool,
    pub digest_hours: i64,
    pub overrides: std::collections::HashMap<String, crate::tmdb::model::Target>,
    /// The round-8 sections (status, daily posts, poll, permissions, watch nights, owner tools).
    pub ext: super::admin_ext::ExtState,
}

pub const POPULARITY_CHOICES: [i64; 5] = [500, 1500, 3000, 5000, 10_000];
pub const COOLDOWN_CHOICES: [i64; 6] = [0, 3, 5, 10, 30, 60];
pub const SUGGEST_LIMIT_CHOICES: [i64; 6] = [0, 3, 5, 10, 20, 50];

pub fn mode_label(m: FilterMode) -> &'static str {
    match m {
        FilterMode::All => "الكل: كل إصدار لكل عمل",
        FilterMode::Popular => "الأعمال الشائعة فقط",
        FilterMode::Profiles => "الأعمال الموجودة في بروفايلات الأعضاء فقط",
        FilterMode::Formats => "صيغ وأنواع محددة فقط",
    }
}

fn kind_label(k: &str) -> &'static str {
    match k {
        "anime" => "أنمي",
        "manga" => "مانجا",
        "manhwa" => "مانهوا",
        "manhua" => "مانهوا صينية",
        "novel" => "رواية",
        _ => "?",
    }
}

pub(super) fn yes_no(b: bool) -> &'static str {
    if b {
        "✅ مفعّل"
    } else {
        "❌ معطّل"
    }
}

fn channel_text(c: Option<i64>, unset: &str) -> String {
    c.map_or_else(|| unset.to_string(), |c| format!("<#{c}>"))
}

/// Time zones offered for schedules: `(Arabic city, IANA name)`.
pub const TIMEZONES: &[(&str, &str)] = &[
    ("مسقط", "Asia/Muscat"),
    ("الرياض", "Asia/Riyadh"),
    ("دبي", "Asia/Dubai"),
    ("الكويت", "Asia/Kuwait"),
    ("الدوحة", "Asia/Qatar"),
    ("المنامة", "Asia/Bahrain"),
    ("بغداد", "Asia/Baghdad"),
    ("عمّان", "Asia/Amman"),
    ("بيروت", "Asia/Beirut"),
    ("دمشق", "Asia/Damascus"),
    ("القاهرة", "Africa/Cairo"),
    ("الخرطوم", "Africa/Khartoum"),
    ("طرابلس", "Africa/Tripoli"),
    ("تونس", "Africa/Tunis"),
    ("الجزائر", "Africa/Algiers"),
    ("الدار البيضاء", "Africa/Casablanca"),
    ("إسطنبول", "Europe/Istanbul"),
    ("التوقيت العالمي (UTC)", "UTC"),
];

pub fn timezone_options(current: &str) -> Vec<Opt> {
    TIMEZONES
        .iter()
        .map(|(label, tz)| ((*label).to_string(), (*tz).to_string(), *tz == current))
        .collect()
}

/// Where a country is listed for this server: `/drama` or `/series`, with the override applied.
pub fn country_target(
    code: &str,
    overrides: &std::collections::HashMap<String, crate::tmdb::model::Target>,
) -> crate::tmdb::model::Target {
    crate::tmdb::model::classify(&[code.to_string()], overrides)
}

pub fn target_label(t: crate::tmdb::model::Target) -> &'static str {
    match t {
        crate::tmdb::model::Target::Drama => "الدراما",
        crate::tmdb::model::Target::Series => "المسلسلات",
    }
}

/// Countries as menu options; choosing one moves it to the other command.
pub fn country_options(
    overrides: &std::collections::HashMap<String, crate::tmdb::model::Target>,
) -> Vec<Opt> {
    crate::tmdb::lookup::COUNTRIES
        .iter()
        .take(25)
        .map(|(c, n)| {
            (
                format!(
                    "{n} — الآن في {}",
                    target_label(country_target(c, overrides))
                ),
                (*c).to_string(),
                false,
            )
        })
        .collect()
}

/// One option of a select menu: `(label, value, selected)`.
pub type Opt = (String, String, bool);

pub fn mode_options(current: FilterMode) -> Vec<Opt> {
    [
        FilterMode::All,
        FilterMode::Popular,
        FilterMode::Profiles,
        FilterMode::Formats,
    ]
    .into_iter()
    .map(|m| {
        (
            mode_label(m).to_string(),
            m.as_str().to_string(),
            m == current,
        )
    })
    .collect()
}

pub fn popularity_options(current: i64) -> Vec<Opt> {
    POPULARITY_CHOICES
        .iter()
        .map(|p| (format!("شعبية ≥ {p}"), p.to_string(), *p == current))
        .collect()
}

pub fn format_options(selected: &[String]) -> Vec<Opt> {
    ALL_FORMATS
        .iter()
        .map(|f| {
            (
                (*f).to_string(),
                (*f).to_string(),
                selected.iter().any(|s| s == f),
            )
        })
        .collect()
}

pub fn kind_options(selected: &[String]) -> Vec<Opt> {
    ALL_KINDS
        .iter()
        .map(|k| {
            (
                kind_label(k).to_string(),
                (*k).to_string(),
                selected.iter().any(|s| s == k),
            )
        })
        .collect()
}

/// Options to remove a feed: the work it maps to and its host.
pub fn feed_options(feeds: &[cdb::Feed]) -> Vec<Opt> {
    feeds
        .iter()
        .take(25)
        .map(|f| {
            let host = reqwest::Url::parse(&f.url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_default();
            (
                format!("{} — {host}", f.title_match),
                f.id.to_string(),
                false,
            )
        })
        .collect()
}

/// `"https://site/feed.xml | Work title"` -> (url, title).
pub fn parse_feed_input(input: &str) -> Option<(String, String)> {
    let (url, title) = input.split_once('|')?;
    let (url, title) = (url.trim(), title.trim());
    (!url.is_empty() && title.chars().count() >= 3 && title.chars().count() <= 80)
        .then(|| (url.to_string(), title.to_string()))
}

pub fn cooldown_options(current: i64) -> Vec<Opt> {
    COOLDOWN_CHOICES
        .iter()
        .map(|c| {
            let label = if *c == 0 {
                "الافتراضي".to_string()
            } else {
                format!("{c} ثانية")
            };
            (label, c.to_string(), *c == current)
        })
        .collect()
}

pub(super) fn menu(
    id: &str,
    placeholder: &str,
    opts: Vec<Opt>,
    min: u8,
    max: u8,
    enabled: bool,
) -> serenity::CreateActionRow {
    let options = opts
        .into_iter()
        .take(25)
        .map(|(label, value, selected)| {
            serenity::CreateSelectMenuOption::new(crate::ui::embeds::fit(&label, 100), value)
                .default_selection(selected)
        })
        .collect::<Vec<_>>();
    let max = max.min(options.len() as u8).max(1);
    serenity::CreateActionRow::SelectMenu(
        serenity::CreateSelectMenu::new(id, serenity::CreateSelectMenuKind::String { options })
            .placeholder(placeholder)
            .min_values(min.min(max))
            .max_values(max)
            .disabled(!enabled),
    )
}

fn role_menu(id: &str, placeholder: &str, enabled: bool) -> serenity::CreateActionRow {
    serenity::CreateActionRow::SelectMenu(
        serenity::CreateSelectMenu::new(
            id,
            serenity::CreateSelectMenuKind::Role {
                default_roles: None,
            },
        )
        .placeholder(placeholder)
        .disabled(!enabled),
    )
}

pub(super) fn toggle(id: &str, label: &str, on: bool, enabled: bool) -> serenity::CreateButton {
    serenity::CreateButton::new(id)
        .label(format!("{label}: {}", if on { "✅" } else { "❌" }))
        .style(if on {
            serenity::ButtonStyle::Success
        } else {
            serenity::ButtonStyle::Secondary
        })
        .disabled(!enabled)
}

pub struct AdminView {
    pool: SqlitePool,
    guilds: Arc<GuildStore>,
    guild: i64,
    screen: Screen,
    state: State,
    notice: Option<String>,
    user: i64,
    tools: super::admin_ext::Tools,
    /// A card (the backup file) to send privately after a press.
    card: Option<crate::ui::session::CardFollowup>,
}

impl AdminView {
    async fn load(
        pool: &SqlitePool,
        guilds: &GuildStore,
        guild: i64,
        tools: &super::admin_ext::Tools,
    ) -> Result<State, Error> {
        let mut st = State {
            nsfw: custom::nsfw_mode(pool, guild).await?,
            mature: custom::mature_themes(pool, guild).await?,
            games: custom::games_enabled(pool, guild).await?,
            images: custom::custom_images_enabled(pool, guild).await?,
            upload_admins: adb::upload_admins_only(pool, guild).await?,
            manager_role: adb::manager_role(pool, guild).await?,
            bot_channel: guilds.settings(guild).await?.command_channel,
            cooldown: adb::cooldown_secs(pool, guild).await?,
            suggest_limit: adb::suggest_limit(pool, guild).await?,
            reminder_ping: adb::reminder_ping(pool, guild).await?,
            feed: feed::get_config(pool, guild).await?,
            chapters: cdb::settings(pool, guild).await?,
            feeds: cdb::feeds(pool, guild).await?,
            timezone: adb::timezone(pool, guild).await?,
            digest_hours: cdb::digest_hours(pool, guild).await?,
            spoiler: adb::spoiler_guard(pool, guild).await?,
            overrides: crate::db::notify::drama_overrides(pool, guild).await?,
            ext: Default::default(),
        };
        st.ext =
            super::admin_ext::ExtState::load(pool, guild, tools, &st.chapters, &st.feeds).await?;
        Ok(st)
    }

    async fn reload(&mut self) {
        match Self::load(&self.pool, &self.guilds, self.guild, &self.tools).await {
            Ok(s) => self.state = s,
            Err(e) => {
                tracing::warn!("admin panel reload failed: {e:#}");
                self.notice = Some(i18n::ERR_GENERIC.to_string());
            }
        }
    }

    /// Validate and store an RSS feed; returns the message for the admin.
    async fn add_feed(&self, input: &str) -> String {
        use crate::chapters::ssrf;
        let Some((url, title)) = parse_feed_input(input) else {
            return i18n::FEED_INPUT_HELP.to_string();
        };
        let parsed = match ssrf::check_syntax(&url) {
            Ok(u) => u,
            Err(e) => return i18n::feed_url_error(&format!("{e:?}")),
        };
        if let Err(e) = ssrf::resolve_public(&parsed).await {
            return i18n::feed_url_error(&format!("{e:?}"));
        }
        match cdb::add_feed(&self.pool, self.guild, parsed.as_str(), &title).await {
            Ok(cdb::AddFeed::Added) => i18n::FEED_ADDED.to_string(),
            Ok(cdb::AddFeed::Duplicate) => i18n::FEED_DUPLICATE.to_string(),
            Ok(cdb::AddFeed::Full) => i18n::FEED_FULL.to_string(),
            Err(e) => {
                tracing::warn!("feed add failed: {e}");
                i18n::ERR_GENERIC.to_string()
            }
        }
    }

    fn saved(&mut self) {
        self.notice = Some(i18n::SETTINGS_SAVED.to_string());
    }
}

/// The embed of a screen (pure, so it is testable).
pub fn embed_for(screen: Screen, s: &State, guild: i64) -> Emb {
    let e = Emb::new();
    match screen {
        Screen::Home => e.title("🛠 لوحة الإدارة").description("اختر قسمًا من الأزرار. كل التغييرات تُحفظ فورًا."),
        Screen::Safety => e.title("🛡 الأمان").description(format!(
            "• سياسة NSFW: {}\n• المواضيع الناضجة (الرعب والحرب والعنف) في الألعاب والروايات البصرية: {}\n• مصادر الألعاب (IGDB / VNDB): {}\n\nالحكم على الألعاب يعتمد على المحتوى (المواضيع والأنواع والكلمات المفتاحية والعلامات) وليس على التصنيف العمري. المحتوى الجنسي مخفي دائمًا مهما كانت الإعدادات، وإطفاء «المواضيع الناضجة» يخفي أيضًا ألعاب الرعب والحرب والعنف الشديد.",
            yes_no(s.nsfw),
            yes_no(s.mature),
            yes_no(s.games)
        )),
        Screen::Notifications => e.title("🔔 الإشعارات").description(format!(
            "• الحالة: {}\n• تنبيه التذكير في القناة عند إغلاق الرسائل الخاصة: {}\n• القناة: {}\n• التصفية: **{}**\n• حد الشعبية: {}\n• الصيغ: {}\n• الأنواع: {}\n\nجدول البث متاح للأنمي فقط في AniList؛ المانجا والمانهوا لا يوفّر لها جدول فصول.",
            yes_no(s.feed.enabled),
            yes_no(s.reminder_ping),
            channel_text(s.feed.channel_id, "غير محددة (استخدم /set-notifications-channel)"),
            mode_label(s.feed.filter),
            s.feed.min_popularity,
            s.feed.formats.join("، "),
            s.feed.kinds.iter().map(|k| kind_label(k)).collect::<Vec<_>>().join("، ")
        )),
        Screen::Images => e.title("🖼 الصور المخصصة").description(format!(
            "• الصور المخصصة: {}\n• من يستطيع الرفع: **{}**",
            yes_no(s.images),
            if s.upload_admins { "المشرفون فقط" } else { "الجميع" }
        )),
        Screen::Roles => e.title("👥 الرتب والصلاحيات").description(format!(
            "• رتبة إدارة البوت: {}\n• رتبة منشن الإشعارات: {}\n\nمن يملك رتبة الإدارة يستطيع استخدام أوامر الإدارة. لتظهر له الأوامر اسمح لها في إعدادات السيرفر ← التكاملات.",
            s.manager_role.map_or("غير محددة".to_string(), |r| format!("<@&{r}>")),
            s.feed.role_id.map_or("غير محددة".to_string(), |r| format!("<@&{r}>"))
        )),
        Screen::Channels => e.title("📌 القنوات").description(format!(
            "• قناة البوت (الأوامر تعمل فيها فقط): {}\n• قناة الإشعارات: {}\n\nلتغييرها استخدم `/set-bot-channel` و`/set-notifications-channel`. من هنا يمكنك إلغاء التحديد فقط.\nرابط الخادم: {guild}",
            channel_text(s.bot_channel, "غير محددة (الأوامر تعمل في كل القنوات)"),
            channel_text(s.feed.channel_id, "غير محددة (لا إشعارات)")
        )),
        Screen::Limits => e.title("⏱ الحدود").description(format!(
            "• مهلة الأوامر لكل عضو: **{}** (المشرفون معفون)\n• حد الاقتراحات اليومي لكل عضو: **{}**",
            if s.cooldown == 0 { "الافتراضي".to_string() } else { format!("{} ثانية", s.cooldown) },
            if s.suggest_limit == 0 { "بلا حد".to_string() } else { s.suggest_limit.to_string() }
        )),
        Screen::Chapters => e.title("📖 مصادر الفصول").description(format!(
            "إشعارات الفصول الجديدة للمانجا والمانهوا والمانهوا الصينية **المتابَعة فقط** (الموجودة في بروفايل عضو أو تذكيراته) في قناة الإشعارات.\n\n• MangaDex (رسمي، مرتبة الأولوية الأولى): {}\n• MangaUpdates: {}\n• روابط RSS/Atom: {} ({} رابط)\n• لغة MangaDex المفضلة: **{}**\n• ملخص الفصول: **{}** (رسالة واحدة بحد أقصى 15 فصلًا، والباقي خلف زر «المزيد»)\n\nلا يوجد أي سحب لمواقع.",
            yes_no(s.chapters.mangadex),
            yes_no(s.chapters.mangaupdates),
            yes_no(s.chapters.rss),
            s.feeds.len(),
            if s.chapters.lang == "ar" { "العربية" } else { "English" },
            crate::commands::notify_panel::digest_label(s.digest_hours)
        )),
        Screen::Status | Screen::Daily | Screen::Poll | Screen::Perms | Screen::Watch | Screen::Owner => super::admin_ext::embed(screen, &s.ext),
        Screen::Library => e.title("📺 المسلسلات والجدول").description(format!(
            "• منطقة الجدول: **{}**\n• حماية الحرق (إخفاء ملخص العمل خلف ||سبويلر||): {}\n• تصنيف البلدان: الدراما الآسيوية في `/drama` وبقية العالم في `/series`، ويمكنك نقل أي بلد للآخر من القائمة الثانية.\n\nالبلدان المنقولة حاليًا: {}",
            TIMEZONES.iter().find(|(_, tz)| *tz == s.timezone).map_or(s.timezone.as_str(), |(l, _)| *l),
            yes_no(s.spoiler),
            if s.overrides.is_empty() { "لا شيء".to_string() } else { s.overrides.iter().map(|(c, t)| format!("{} ← {}", crate::tmdb::lookup::country_name(c), target_label(*t))).collect::<Vec<_>>().join("، ") }
        )),
        Screen::Reset => e.title("🗑 إعادة الضبط").description(
            "سيتم حذف إعدادات هذا السيرفر: القنوات والإشعارات والصور المخصصة والرتب والنقاط. بروفايلات الأعضاء العامة لا تُحذف.",
        ),
    }
}

/// The components of a screen (pure).
pub fn components_for(
    screen: Screen,
    s: &State,
    guild: i64,
    confirming: bool,
    enabled: bool,
) -> Vec<serenity::CreateActionRow> {
    let nav = |extra: Vec<serenity::CreateButton>| {
        let mut row = extra;
        row.push(back_button(enabled));
        row.push(close_button(enabled));
        buttons(row)
    };
    match screen {
        Screen::Home => {
            let mut all: Vec<&(&str, &str, char, Screen)> = HOME.iter().collect();
            if s.ext.is_owner {
                all.push(&OWNER_HOME);
            }
            let mut rows: Vec<serenity::CreateActionRow> = all
                .chunks(5)
                .map(|chunk| {
                    buttons(
                        chunk
                            .iter()
                            .map(|(id, label, emoji, _)| {
                                serenity::CreateButton::new(*id)
                                    .label(*label)
                                    .emoji(*emoji)
                                    .style(serenity::ButtonStyle::Secondary)
                                    .disabled(!enabled)
                            })
                            .collect(),
                    )
                })
                .collect();
            rows.push(buttons(vec![close_button(enabled)]));
            rows
        }
        Screen::Safety => vec![
            buttons(vec![
                toggle(T_NSFW, "NSFW", s.nsfw, enabled),
                toggle(T_MATURE, "مواضيع ناضجة", s.mature, enabled),
                toggle(T_GAMES, "الألعاب", s.games, enabled),
            ]),
            nav(vec![]),
        ],
        Screen::Notifications => vec![
            buttons(vec![
                toggle(N_TOGGLE, "الإشعارات", s.feed.enabled, enabled),
                back_button(enabled),
                close_button(enabled),
            ]),
            menu(
                N_MODE,
                "نوع التصفية",
                mode_options(s.feed.filter),
                1,
                1,
                enabled,
            ),
            menu(
                N_POP,
                "حد الشعبية (للتصفية الشائعة)",
                popularity_options(s.feed.min_popularity),
                1,
                1,
                enabled,
            ),
            menu(
                N_FORMATS,
                "الصيغ (للتصفية بالصيغ)",
                format_options(&s.feed.formats),
                0,
                4,
                enabled,
            ),
            menu(
                N_KINDS,
                "الأنواع (للتصفية بالصيغ)",
                kind_options(&s.feed.kinds),
                0,
                5,
                enabled,
            ),
        ],
        Screen::Status
        | Screen::Daily
        | Screen::Poll
        | Screen::Perms
        | Screen::Watch
        | Screen::Owner => super::admin_ext::components(screen, &s.ext, enabled),
        Screen::Library => vec![
            buttons(vec![
                toggle(LIB_SPOILER, "حماية الحرق", s.spoiler, enabled),
                back_button(enabled),
                close_button(enabled),
            ]),
            menu(
                LIB_TZ,
                "منطقة الجدول الزمنية",
                timezone_options(&s.timezone),
                1,
                1,
                enabled,
            ),
            menu(
                LIB_COUNTRY,
                "انقل بلدًا بين الدراما والمسلسلات",
                country_options(&s.overrides),
                1,
                1,
                enabled,
            ),
        ],
        Screen::Images => vec![
            buttons(vec![
                toggle(I_TOGGLE, "الصور المخصصة", s.images, enabled),
                serenity::CreateButton::new(I_POLICY)
                    .label(format!(
                        "الرفع: {}",
                        if s.upload_admins {
                            "المشرفون"
                        } else {
                            "الجميع"
                        }
                    ))
                    .style(serenity::ButtonStyle::Secondary)
                    .disabled(!enabled),
            ]),
            nav(vec![]),
        ],
        Screen::Roles => vec![
            role_menu(R_MANAGER, "اختر رتبة إدارة البوت", enabled),
            role_menu(R_PING, "اختر رتبة منشن الإشعارات", enabled),
            nav(vec![
                serenity::CreateButton::new(R_CLEAR_MANAGER)
                    .label("إلغاء رتبة الإدارة")
                    .style(serenity::ButtonStyle::Secondary)
                    .disabled(!enabled || s.manager_role.is_none()),
                serenity::CreateButton::new(R_CLEAR_PING)
                    .label("إلغاء رتبة المنشن")
                    .style(serenity::ButtonStyle::Secondary)
                    .disabled(!enabled || s.feed.role_id.is_none()),
            ]),
        ],
        Screen::Channels => {
            let mut links = Vec::new();
            for c in [s.bot_channel, s.feed.channel_id].into_iter().flatten() {
                links.push(
                    serenity::CreateButton::new_link(format!(
                        "https://discord.com/channels/{guild}/{c}"
                    ))
                    .label(format!("↗ #{c}")),
                );
            }
            let mut rows = vec![buttons(vec![
                serenity::CreateButton::new(C_CLEAR_BOT)
                    .label("إلغاء قناة البوت")
                    .style(serenity::ButtonStyle::Danger)
                    .disabled(!enabled || s.bot_channel.is_none()),
                serenity::CreateButton::new(C_CLEAR_NOTIFY)
                    .label("إلغاء قناة الإشعارات")
                    .style(serenity::ButtonStyle::Danger)
                    .disabled(!enabled || s.feed.channel_id.is_none()),
            ])];
            if !links.is_empty() {
                rows.push(buttons(links));
            }
            rows.push(nav(vec![]));
            rows
        }
        Screen::Limits => vec![
            menu(
                L_COOLDOWN,
                "مهلة الأوامر لكل عضو",
                cooldown_options(s.cooldown),
                1,
                1,
                enabled,
            ),
            nav(vec![]),
        ],
        Screen::Chapters => {
            let mut rows = vec![
                buttons(vec![
                    toggle(CH_MANGADEX, "MangaDex", s.chapters.mangadex, enabled),
                    toggle(
                        CH_MANGAUPDATES,
                        "MangaUpdates",
                        s.chapters.mangaupdates,
                        enabled,
                    ),
                    toggle(CH_RSS, "RSS", s.chapters.rss, enabled),
                    back_button(enabled),
                    close_button(enabled),
                ]),
                menu(
                    CH_LANG,
                    "لغة MangaDex",
                    vec![
                        (
                            "العربية".to_string(),
                            "ar".to_string(),
                            s.chapters.lang == "ar",
                        ),
                        (
                            "English".to_string(),
                            "en".to_string(),
                            s.chapters.lang == "en",
                        ),
                    ],
                    1,
                    1,
                    enabled,
                ),
                menu(
                    CH_DIGEST,
                    "كم مرة يصل ملخص الفصول؟",
                    crate::db::notify::DIGEST_HOURS
                        .iter()
                        .map(|h| {
                            (
                                crate::commands::notify_panel::digest_label(*h),
                                h.to_string(),
                                *h == s.digest_hours,
                            )
                        })
                        .collect(),
                    1,
                    1,
                    enabled,
                ),
                buttons(vec![serenity::CreateButton::new(CH_ADD_FEED)
                    .label("إضافة رابط RSS")
                    .emoji('➕')
                    .style(serenity::ButtonStyle::Secondary)
                    .disabled(
                        !enabled || s.feeds.len() as i64 >= cdb::MAX_FEEDS_PER_GUILD,
                    )]),
            ];
            if !s.feeds.is_empty() {
                rows.push(menu(
                    CH_REMOVE_FEED,
                    "احذف رابطًا",
                    feed_options(&s.feeds),
                    1,
                    1,
                    enabled,
                ));
            }
            rows
        }
        Screen::Reset => {
            let mut extra = Vec::new();
            if confirming {
                extra.push(
                    serenity::CreateButton::new(X_CONFIRM)
                        .label("نعم، احذف كل شيء")
                        .style(serenity::ButtonStyle::Danger)
                        .disabled(!enabled),
                );
            } else {
                extra.push(
                    serenity::CreateButton::new("x:ask")
                        .label("حذف بيانات السيرفر…")
                        .style(serenity::ButtonStyle::Danger)
                        .disabled(!enabled),
                );
            }
            vec![nav(extra)]
        }
    }
}

impl View for AdminView {
    fn ephemeral(&self) -> bool {
        true
    }

    fn take_card(&mut self) -> Option<crate::ui::session::CardFollowup> {
        self.card.take()
    }

    fn modal_for(&self, id: &str) -> Option<ModalSpec> {
        match id {
            CH_ADD_FEED => Some(ModalSpec {
                title: "إضافة رابط RSS".into(),
                label: "الرابط | عنوان العمل".into(),
            }),
            "w:new" => Some(ModalSpec {
                title: "ليلة مشاهدة جديدة".into(),
                label: "اسم العمل | 2026-11-21 21:00 | وصف اختياري".into(),
            }),
            _ => None,
        }
    }

    async fn on_modal(&mut self, id: &str, input: String) -> Outcome {
        if id == "w:new" {
            self.notice = Some(
                super::admin_ext::create_night(
                    &self.tools,
                    &self.pool,
                    self.guild,
                    self.user,
                    &input,
                )
                .await,
            );
            self.reload().await;
            return Outcome::Update;
        }
        if id != CH_ADD_FEED {
            return Outcome::Keep;
        }
        self.notice = Some(self.add_feed(&input).await);
        self.reload().await;
        Outcome::Update
    }

    fn render(&self, enabled: bool) -> Page {
        let confirming = self.screen == Screen::Reset
            && self.notice.as_deref() == Some(i18n::ADMIN_CONFIRM_RESET);
        Page::embed(
            embed_for(self.screen, &self.state, self.guild).build(),
            components_for(self.screen, &self.state, self.guild, confirming, enabled),
        )
        .notice(self.notice.clone())
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        self.notice = None;
        let (pool, g) = (self.pool.clone(), self.guild);
        let first = values.first().map(String::as_str);
        let result: Result<(), Error> = async {
            match id {
                CLOSE => return Ok(()),
                BACK => self.screen = Screen::Home,
                T_NSFW => custom::set_nsfw_mode(&pool, g, !self.state.nsfw).await?,
                T_MATURE => custom::set_mature_themes(&pool, g, !self.state.mature).await?,
                T_GAMES => custom::set_games(&pool, g, !self.state.games).await?,
                N_TOGGLE => feed::set_enabled(&pool, g, !self.state.feed.enabled).await?,
                N_PING => adb::set_reminder_ping(&pool, g, !self.state.reminder_ping).await?,
                CH_MANGADEX => {
                    cdb::set_source(&pool, g, "mangadex", !self.state.chapters.mangadex).await?
                }
                CH_MANGAUPDATES => {
                    cdb::set_source(&pool, g, "mangaupdates", !self.state.chapters.mangaupdates)
                        .await?
                }
                CH_RSS => cdb::set_source(&pool, g, "rss", !self.state.chapters.rss).await?,
                CH_LANG => {
                    if let Some(l) = first {
                        cdb::set_lang(&pool, g, l).await?;
                    }
                }
                CH_DIGEST => {
                    if let Some(h) = first.and_then(|v| v.parse::<i64>().ok()) {
                        cdb::set_digest_hours(&pool, g, h).await?;
                    }
                }
                CH_REMOVE_FEED => {
                    if let Some(id) = first.and_then(|v| v.parse::<i64>().ok()) {
                        cdb::remove_feed(&pool, g, id).await?;
                    }
                }
                L_SUGGEST => {
                    if let Some(n) = first
                        .and_then(|v| v.parse::<i64>().ok())
                        .filter(|n| SUGGEST_LIMIT_CHOICES.contains(n))
                    {
                        adb::set_suggest_limit(&pool, g, n).await?;
                    }
                }
                N_MODE => {
                    if let Some(m) = first.and_then(FilterMode::parse) {
                        feed::set_filter_mode(&pool, g, m).await?;
                    }
                }
                N_POP => {
                    if let Some(n) = first
                        .and_then(|v| v.parse::<i64>().ok())
                        .filter(|n| POPULARITY_CHOICES.contains(n))
                    {
                        feed::set_min_popularity(&pool, g, n).await?;
                    }
                }
                N_FORMATS => feed::set_formats(&pool, g, values).await?,
                N_KINDS => feed::set_kinds(&pool, g, values).await?,
                LIB_SPOILER => adb::set_spoiler_guard(&pool, g, !self.state.spoiler).await?,
                LIB_TZ => {
                    if let Some(tz) = first.filter(|t| TIMEZONES.iter().any(|(_, z)| z == t)) {
                        adb::set_timezone(&pool, g, tz).await?;
                    }
                }
                LIB_COUNTRY => {
                    if let Some(code) =
                        first.filter(|c| crate::tmdb::lookup::country_code(c).is_some())
                    {
                        let default =
                            crate::tmdb::model::classify(&[code.to_string()], &Default::default());
                        let now = country_target(code, &self.state.overrides);
                        let wanted = if now == crate::tmdb::model::Target::Drama {
                            crate::tmdb::model::Target::Series
                        } else {
                            crate::tmdb::model::Target::Drama
                        };
                        let value = if wanted == default {
                            "default"
                        } else if wanted == crate::tmdb::model::Target::Drama {
                            "drama"
                        } else {
                            "series"
                        };
                        crate::db::notify::set_drama_override(&pool, g, code, value).await?;
                    }
                }
                I_TOGGLE => custom::set_custom_images(&pool, g, !self.state.images).await?,
                I_POLICY => {
                    adb::set_upload_admins_only(&pool, g, !self.state.upload_admins).await?
                }
                R_MANAGER => {
                    adb::set_manager_role(&pool, g, first.and_then(|v| v.parse().ok())).await?
                }
                R_PING => feed::set_role(&pool, g, first.and_then(|v| v.parse().ok())).await?,
                R_CLEAR_MANAGER => adb::set_manager_role(&pool, g, None).await?,
                R_CLEAR_PING => feed::set_role(&pool, g, None).await?,
                C_CLEAR_BOT => self.guilds.reset(g).await?,
                C_CLEAR_NOTIFY => feed::clear_channel(&pool, g).await?,
                L_COOLDOWN => {
                    if let Some(n) = first
                        .and_then(|v| v.parse::<i64>().ok())
                        .filter(|n| COOLDOWN_CHOICES.contains(n))
                    {
                        adb::set_cooldown_secs(&pool, g, n).await?;
                    }
                }
                "x:ask" => {
                    self.notice = Some(i18n::ADMIN_CONFIRM_RESET.to_string());
                    return Ok(());
                }
                X_CONFIRM => {
                    adb::reset_guild(&pool, g).await?;
                    self.guilds.reset(g).await?;
                    self.screen = Screen::Home;
                }
                id if super::admin_ext::handles(id) => {
                    let reply = super::admin_ext::handle(
                        &self.tools,
                        &pool,
                        g,
                        &self.state.ext,
                        id,
                        values,
                    )
                    .await?;
                    self.notice = reply.notice;
                    self.card = reply.card;
                }
                other => {
                    let owner_home = self.state.ext.is_owner.then_some(&OWNER_HOME);
                    if let Some((_, _, _, screen)) =
                        HOME.iter().chain(owner_home).find(|h| h.0 == other)
                    {
                        self.screen = *screen;
                    }
                    return Ok(());
                }
            }
            Ok(())
        }
        .await;
        if id == CLOSE {
            return Outcome::Close;
        }
        match result {
            Ok(()) => {
                let is_nav = id == BACK || id.starts_with("go:") || id == "x:ask";
                self.reload().await;
                if !is_nav && self.notice.is_none() {
                    self.saved();
                }
            }
            Err(e) => {
                tracing::warn!("admin action {id} failed: {e:#}");
                self.notice = Some(i18n::ERR_GENERIC.to_string());
            }
        }
        Outcome::Update
    }
}

/// Is the invoker allowed to manage this server (Manage Guild, Administrator or the management role)?
pub async fn invoker_can_manage(ctx: Context<'_>) -> bool {
    let Some(g) = ctx.guild_id() else {
        return false;
    };
    let Some(member) = ctx.author_member().await else {
        return false;
    };
    let roles: Vec<i64> = member.roles.iter().map(|r| r.get() as i64).collect();
    let manager = adb::manager_role(&ctx.data().db, g.get() as i64)
        .await
        .unwrap_or(None);
    guild::can_manage(member.permissions, &roles, manager)
}

/// The server admin panel (Manage Guild).
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "MANAGE_GUILD",
    name_localized("ar", "إدارة"),
    description_localized("ar", "لوحة إدارة السيرفر: الأمان والإشعارات والصور والقنوات (للمشرفين)")
)]
pub async fn admin(ctx: Context<'_>) -> Result<(), Error> {
    let Some(g) = ctx.guild_id().map(|g| g.get() as i64) else {
        return super::say_ephemeral(ctx, i18n::SERVER_ONLY).await;
    };
    if !invoker_can_manage(ctx).await {
        ctx.send(
            poise::CreateReply::default()
                .content(i18n::NOT_ADMIN)
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }
    let pool = ctx.data().db.clone();
    let guilds = ctx.data().guilds.clone();
    let data = ctx.data();
    let tools = super::admin_ext::Tools {
        http: data.http.clone(),
        tmdb: data.tmdb.clone(),
        providers: data.providers.clone(),
        ops: data.ops.clone(),
        is_owner: data.ops.is_owner(ctx.author().id.get()),
        guild_count: ctx.cache().guild_count(),
        discord: ctx.serenity_context().http.clone(),
        anilist: data.anilist.clone(),
    };
    let state = AdminView::load(&pool, &guilds, g, &tools).await?;
    session::run(
        ctx,
        AdminView {
            pool,
            guilds,
            guild: g,
            screen: Screen::Home,
            state,
            notice: None,
            user: ctx.author().id.get() as i64,
            tools,
            card: None,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        State {
            nsfw: false,
            mature: true,
            games: true,
            images: false,
            upload_admins: false,
            manager_role: Some(5),
            bot_channel: Some(10),
            cooldown: 0,
            suggest_limit: 10,
            reminder_ping: false,
            chapters: cdb::Settings::default(),
            timezone: "Asia/Muscat".into(),
            ext: Default::default(),
            digest_hours: 2,
            spoiler: false,
            overrides: Default::default(),
            feeds: vec![cdb::Feed {
                id: 1,
                url: "https://a.example/f".into(),
                title_match: "Solo".into(),
            }],
            feed: FeedConfig {
                guild_id: 1,
                channel_id: Some(20),
                enabled: true,
                min_popularity: 1500,
                formats: vec!["TV".into()],
                role_id: Some(6),
                filter: FilterMode::All,
                kinds: vec!["anime".into()],
            },
        }
    }

    const SCREENS: [Screen; 16] = [
        Screen::Home,
        Screen::Safety,
        Screen::Notifications,
        Screen::Images,
        Screen::Roles,
        Screen::Channels,
        Screen::Limits,
        Screen::Chapters,
        Screen::Library,
        Screen::Status,
        Screen::Daily,
        Screen::Poll,
        Screen::Perms,
        Screen::Watch,
        Screen::Owner,
        Screen::Reset,
    ];

    #[test]
    fn every_screen_fits_discords_five_rows_and_has_a_way_back() {
        let s = state();
        for screen in SCREENS {
            for confirming in [false, true] {
                let rows = components_for(screen, &s, 1, confirming, true);
                assert!(
                    !rows.is_empty() && rows.len() <= 5,
                    "{screen:?}: {} rows",
                    rows.len()
                );
                let json = serde_json::to_string(&rows).expect("json");
                let nav = if screen == Screen::Home {
                    "\"close\""
                } else {
                    "\"back\""
                };
                assert!(json.contains(nav), "{screen:?} lacks {nav}");
            }
            assert!(embed_for(screen, &s, 1).description.is_some());
        }
    }

    #[test]
    fn home_has_every_section_and_the_owner_button_only_for_the_owner() {
        assert_eq!(HOME.len(), 14);
        let rows = components_for(Screen::Home, &state(), 1, false, true);
        assert!(rows.len() <= 5, "{} rows", rows.len());
        let json = serde_json::to_string(&rows).expect("json");
        for (id, ..) in HOME {
            assert!(json.contains(id), "{id}");
        }
        assert!(
            !json.contains("go:owner"),
            "members and admins do not see the owner section"
        );
        let mut owner = state();
        owner.ext.is_owner = true;
        let rows = components_for(Screen::Home, &owner, 1, false, true);
        assert!(
            rows.len() <= 5
                && serde_json::to_string(&rows)
                    .expect("json")
                    .contains("go:owner")
        );
    }

    #[test]
    fn option_builders_mark_the_current_value_and_stay_unique() {
        let modes = mode_options(FilterMode::Profiles);
        assert_eq!(modes.iter().filter(|o| o.2).count(), 1);
        assert!(modes.iter().any(|o| o.1 == "profiles" && o.2));
        assert_eq!(popularity_options(1500).iter().filter(|o| o.2).count(), 1);
        assert_eq!(
            format_options(&["TV".into(), "ONA".into()])
                .iter()
                .filter(|o| o.2)
                .count(),
            2
        );
        assert_eq!(kind_options(&[]).iter().filter(|o| o.2).count(), 0);
        assert_eq!(kind_options(&[]).len(), 5);
        for opts in [
            mode_options(FilterMode::All),
            cooldown_options(0),
            kind_options(&[]),
        ] {
            let mut seen = std::collections::HashSet::new();
            assert!(opts.iter().all(|o| seen.insert(o.1.clone())));
        }
    }

    #[test]
    fn channel_summary_shows_both_channels_and_unset_hints() {
        let mut s = state();
        let text = embed_for(Screen::Channels, &s, 1)
            .description
            .unwrap_or_default();
        assert!(text.contains("<#10>") && text.contains("<#20>"));
        s.bot_channel = None;
        s.feed.channel_id = None;
        let text = embed_for(Screen::Channels, &s, 1)
            .description
            .unwrap_or_default();
        assert!(text.contains("غير محددة") && !text.contains("<#"));
    }

    #[test]
    fn feed_input_needs_a_url_and_a_work_title() {
        assert_eq!(
            parse_feed_input("https://a.example/f.xml | Solo Leveling"),
            Some(("https://a.example/f.xml".into(), "Solo Leveling".into()))
        );
        assert_eq!(parse_feed_input("https://a.example/f.xml"), None);
        assert_eq!(parse_feed_input(" | Solo"), None);
        assert_eq!(
            parse_feed_input("https://a.example/f.xml | ab"),
            None,
            "titles under 3 characters match everything"
        );
    }

    #[test]
    fn the_chapters_screen_lists_sources_language_and_feeds_within_five_rows() {
        let s = state();
        let rows = components_for(Screen::Chapters, &s, 1, false, true);
        assert!((3..=5).contains(&rows.len()));
        let json = serde_json::to_string(&rows).expect("json");
        for id in [
            "ch:mangadex",
            "ch:mangaupdates",
            "ch:rss",
            "ch:lang",
            "ch:addfeed",
            "ch:rmfeed",
            "\"back\"",
        ] {
            assert!(json.contains(id), "{id}");
        }
        let text = embed_for(Screen::Chapters, &s, 1)
            .description
            .unwrap_or_default();
        assert!(text.contains("MangaDex") && text.contains("RSS") && text.contains("العربية"));
        assert_eq!(feed_options(&s.feeds)[0].1, "1");
    }

    #[test]
    fn admin_permission_rules() {
        use serenity::Permissions as P;
        assert!(guild::can_manage(Some(P::MANAGE_GUILD), &[], None));
        assert!(guild::can_manage(Some(P::ADMINISTRATOR), &[], None));
        assert!(!guild::can_manage(Some(P::SEND_MESSAGES), &[1, 2], None));
        assert!(!guild::can_manage(None, &[], Some(9)));
        assert!(
            guild::can_manage(Some(P::SEND_MESSAGES), &[1, 9], Some(9)),
            "management role"
        );
        assert!(!guild::can_manage(Some(P::SEND_MESSAGES), &[1], Some(9)));
    }
}
