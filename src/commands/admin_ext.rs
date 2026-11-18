//! The round-8 sections of `/admin`: provider status, daily posts, the weekly poll, role permissions, watch nights and
//! the owner-only tools (backups, usage statistics, maintenance mode). Screens are pure functions of `ExtState`, so
//! they can be tested; `handle` applies one press.

use super::{
    admin::{menu, toggle, yes_no, Opt},
    buttons,
};
use crate::{
    db::{chapters as cdb, feed as fdb},
    features::{
        daily, perms, poll,
        settings::{self, Feature},
        watch::{self, Night},
    },
    ops::{
        backup, health,
        stats::{self, Report},
        Ops,
    },
    providers::Providers,
    tmdb::Tmdb,
    ui::{
        embeds::{fit, Emb},
        pagination::{back_button, close_button},
        session::CardFollowup,
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::sync::Arc;

/// What `handle` needs from the bot besides the database.
#[derive(Clone)]
pub struct Tools {
    pub http: reqwest::Client,
    pub tmdb: Arc<Tmdb>,
    pub providers: Arc<Providers>,
    pub ops: Arc<Ops>,
    /// The person using the panel is the application owner.
    pub is_owner: bool,
    pub guild_count: usize,
    pub discord: Arc<serenity::Http>,
    pub anilist: Arc<crate::anilist::SafeAniList>,
}

/// One line of the provider status panel.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusRow {
    pub name: String,
    pub enabled: bool,
    pub entry: health::Entry,
    pub budget: &'static str,
}

/// Which sources are switched on (for the status panel).
#[derive(Debug, Clone, Copy, Default)]
pub struct Flags {
    pub tmdb: bool,
    pub igdb: bool,
    pub vndb: bool,
    pub mangadex: bool,
    pub mangaupdates: bool,
}

/// The rows of the status panel: every provider and each RSS feed, with the last success and error.
pub fn status_rows(f: &Flags, feeds: &[String]) -> Vec<StatusRow> {
    let row = |key: &str, name: &str, enabled: bool, budget: &'static str| StatusRow {
        name: name.to_string(),
        enabled,
        entry: health::get(key),
        budget,
    };
    let mut rows = vec![
        row("anilist", "AniList", true, "25 طلبًا/دقيقة (يتكيّف مع الحد)"),
        row("tmdb", "TMDB", f.tmdb, "30 طلبًا/10 ثوانٍ"),
        row("igdb", "IGDB", f.igdb, "4 طلبات/ثانية"),
        row("vndb", "VNDB", f.vndb, "نحو 200 طلب/5 دقائق"),
        row("mangadex", "MangaDex", f.mangadex, "3 طلبات/ثانية"),
        row(
            "mangaupdates",
            "MangaUpdates",
            f.mangaupdates,
            "طلبان/ثانية",
        ),
    ];
    rows.extend(feeds.iter().map(|h| {
        row(
            &format!("rss:{h}"),
            &format!("RSS · {h}"),
            true,
            "طلب كل 15 دقيقة",
        )
    }));
    rows
}

fn status_line(r: &StatusRow) -> String {
    let icon = if !r.enabled {
        "⚪"
    } else if health::healthy(&r.entry) {
        "✅"
    } else {
        "⚠️"
    };
    let state = if r.enabled { "" } else { " (معطّل)" };
    let ok = r.entry.last_ok.map_or("—".to_string(), health::when);
    let err = r
        .entry
        .last_error
        .as_ref()
        .map_or("لا يوجد".to_string(), |(t, m)| {
            format!("{} — {}", health::when(*t), fit(m, 90))
        });
    format!(
        "{icon} **{}**{state}\nآخر نجاح: {ok}\nآخر خطأ: {err}\nالميزانية: {}",
        r.name, r.budget
    )
}

/// Everything the extra screens show.
#[derive(Debug, Clone, Default)]
pub struct ExtState {
    pub is_owner: bool,
    pub maintenance: bool,
    pub birthdays: Feature,
    pub featured_character: Feature,
    pub featured_work: Feature,
    pub poll: Feature,
    pub watch_event: bool,
    pub perms: Vec<(&'static str, Vec<i64>)>,
    pub nights: Vec<Night>,
    pub status: Vec<StatusRow>,
    pub report: Option<Report>,
    pub backups: (usize, Option<String>),
    pub channel_set: bool,
    /// Servers the bot is in (owner screen).
    pub guilds: usize,
}

impl ExtState {
    pub async fn load(
        pool: &SqlitePool,
        guild: i64,
        t: &Tools,
        chapters: &cdb::Settings,
        feeds: &[cdb::Feed],
    ) -> Result<Self, sqlx::Error> {
        let mut perms = Vec::new();
        for (key, _) in perms::ACTIONS {
            perms.push((*key, perms::allowed_roles(pool, guild, key).await?));
        }
        let hosts: Vec<String> = feeds
            .iter()
            .filter_map(|f| {
                reqwest::Url::parse(&f.url)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_string))
            })
            .collect();
        let flags = Flags {
            tmdb: t.tmdb.enabled(),
            igdb: t.providers.igdb_enabled(),
            vndb: t.providers.vndb_enabled(),
            mangadex: chapters.mangadex || t.providers.enabled(),
            mangaupdates: chapters.mangaupdates || t.providers.enabled(),
        };
        let (report, backups) = if t.is_owner {
            (
                stats::report(pool, &stats::today()).await.ok(),
                backups_of(&t.ops),
            )
        } else {
            (None, (0, None))
        };
        Ok(Self {
            is_owner: t.is_owner,
            maintenance: t.ops.maintenance(),
            birthdays: settings::get(pool, guild, daily::BIRTHDAYS).await?,
            featured_character: settings::get(pool, guild, daily::FEATURED_CHARACTER).await?,
            featured_work: settings::get(pool, guild, daily::FEATURED_WORK).await?,
            poll: settings::get(pool, guild, poll::FEATURE).await?,
            watch_event: settings::get(pool, guild, "watch").await?.value("event") == Some(1),
            perms,
            nights: watch::upcoming(pool, guild, chrono::Utc::now().timestamp()).await?,
            status: status_rows(&flags, &hosts),
            report,
            backups,
            guilds: t.guild_count,
            channel_set: fdb::get_config(pool, guild)
                .await
                .map(|c| c.channel_id.is_some())
                .unwrap_or(false),
        })
    }
}

/// `(how many, newest file name with size)`.
fn backups_of(ops: &Ops) -> (usize, Option<String>) {
    let dir = ops
        .db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."))
        .join("backups");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".db"))
        .collect();
    files.sort_by_key(|e| e.file_name());
    let newest = files.last().map(|e| {
        format!(
            "{} ({})",
            e.file_name().to_string_lossy(),
            backup::size_text(e.metadata().map(|m| m.len()).unwrap_or(0))
        )
    });
    (files.len(), newest)
}

fn hour_options(current: i64) -> Vec<Opt> {
    (0..24)
        .map(|h| (format!("{h:02}:00"), h.to_string(), h == current))
        .collect()
}

const DAYS_AR: [&str; 7] = [
    "الاثنين",
    "الثلاثاء",
    "الأربعاء",
    "الخميس",
    "الجمعة",
    "السبت",
    "الأحد",
];

/// The text of a screen.
pub fn embed(screen: super::admin::Screen, e: &ExtState) -> Emb {
    use super::admin::Screen as S;
    let none_channel = if e.channel_set {
        ""
    } else {
        "\n\n⚠️ لا توجد قناة إشعارات: حدّدها بـ `/set-notifications-channel` وإلا لن يُنشر شيء."
    };
    match screen {
        S::Status => {
            let mut text = e.status.iter().map(status_line).collect::<Vec<_>>().join("\n\n");
            if e.maintenance {
                text.push_str("\n\n🛠 **وضع الصيانة مفعّل**: الأوامر متوقفة لغير المالك.");
            }
            Emb::new().title("📡 حالة المصادر").description(fit(&format!("{text}\n\nزر «إعادة الفحص» يجرّب كل مصدر بطلب خفيف الآن."), 4000))
        }
        S::Daily => Emb::new().title("🗓 المنشورات اليومية").description(format!(
            "كلها مطفأة افتراضيًا، وتُنشر في قناة الإشعارات بتوقيت السيرفر.\n• أعياد ميلاد الشخصيات (فقط لمن في بروفايل عضو هنا): {}\n• شخصية اليوم: {}\n• عمل اليوم (لا يتكرر قبل 30 يومًا): {}\n• ساعة النشر: **{:02}:00**{none_channel}",
            yes_no(e.birthdays.enabled),
            yes_no(e.featured_character.enabled),
            yes_no(e.featured_work.enabled),
            e.birthdays.hour
        )),
        S::Poll => {
            let c = poll::Config::from(&e.poll);
            Emb::new().title("🍿 تصويت «ماذا نشاهد؟»").description(format!(
                "تصويت أسبوعي من اقتراحات `/suggest` (3 إلى 5 اقتراحات لم تُطرح من قبل)، صوت واحد لكل عضو ويمكن تغييره.\n• الحالة: {}\n• اليوم: **{}**\n• الساعة: **{:02}:00**\n• المدة: **{} ساعة**{none_channel}",
                yes_no(e.poll.enabled),
                DAYS_AR[c.day as usize % 7],
                c.hour,
                c.duration_hours
            ))
        }
        S::Perms => {
            let lines: Vec<String> = e
                .perms
                .iter()
                .map(|(k, roles)| format!("• {}: {}", perms::label(k), if roles.is_empty() { "الجميع".to_string() } else { roles.iter().map(|r| format!("<@&{r}>")).collect::<Vec<_>>().join("، ") }))
                .collect();
            Emb::new().title("🔐 صلاحيات الرتب").description(format!("اختر الرتب المسموح لها بكل إجراء. بدون اختيار يستطيع الجميع، ومن يملك «إدارة السيرفر» يستطيع دائمًا.\n\n{}", lines.join("\n")))
        }
        S::Watch => {
            let list = if e.nights.is_empty() {
                "لا توجد ليالٍ قادمة.".to_string()
            } else {
                e.nights.iter().map(|n| format!("• **{}** — <t:{}:F>", fit(&n.title, 60), n.starts_at)).collect::<Vec<_>>().join("\n")
            };
            Emb::new().title("🍿 ليلة مشاهدة").description(format!(
                "أنشئ ليلة مشاهدة: اضغط «ليلة جديدة» واكتب `اسم العمل | 2026-11-21 21:00 | وصف اختياري` (بتوقيت السيرفر). يُنشر إعلان بزرّي «سأحضر» و«لن أحضر»، وتصل رسالة تذكير للحاضرين عند البدء.\n• إنشاء حدث ديسكورد أيضًا (يحتاج للبوت صلاحية «إدارة الفعاليات»): {}\n\n**القادمة:**\n{list}{none_channel}",
                yes_no(e.watch_event)
            ))
        }
        S::Owner => owner_embed(e),
        _ => Emb::new(),
    }
}

fn owner_embed(e: &ExtState) -> Emb {
    let mut lines = vec![
        format!("• وضع الصيانة: {}", yes_no(e.maintenance)),
        format!("• عدد السيرفرات: {}", e.guilds),
    ];
    match &e.report {
        Some(r) => {
            lines.push(format!(
                "• الأوامر آخر 7 أيام: {}",
                r.per_day
                    .iter()
                    .map(|(d, n)| format!("{}: {n}", d.get(5..).unwrap_or(d)))
                    .collect::<Vec<_>>()
                    .join(" · ")
            ));
            lines.push(format!(
                "• الأكثر استخدامًا: {}",
                if r.per_command.is_empty() {
                    "—".into()
                } else {
                    r.per_command
                        .iter()
                        .take(5)
                        .map(|(c, n)| format!("/{c} {n}"))
                        .collect::<Vec<_>>()
                        .join("، ")
                }
            ));
            lines.push(format!(
                "• أخطاء المصادر: {}",
                if r.errors.is_empty() {
                    "لا شيء".into()
                } else {
                    r.errors
                        .iter()
                        .take(5)
                        .map(|(p, n)| format!("{p} {n}"))
                        .collect::<Vec<_>>()
                        .join("، ")
                }
            ));
            lines.push(format!(
                "• المستخدمون النشطون اليوم: {}",
                r.active.last().map_or(0, |(_, n)| *n)
            ));
            lines.push(format!(
                "• الكاش: {} · طلبات API: {}",
                r.hit_rate()
                    .map_or("لا بيانات".to_string(), |p| format!("{p}% إصابة")),
                r.api_calls
            ));
        }
        None => lines.push("• لا توجد إحصاءات بعد.".into()),
    }
    lines.push(format!(
        "• النسخ الاحتياطية: {} (الأحدث: {})",
        e.backups.0,
        e.backups.1.as_deref().unwrap_or("لا يوجد")
    ));
    Emb::new()
        .title("👑 أدوات المالك")
        .description(fit(&lines.join("\n"), 3900))
}

fn roles_menu(
    id: &str,
    placeholder: &str,
    current: &[i64],
    enabled: bool,
) -> serenity::CreateActionRow {
    let defaults: Vec<serenity::RoleId> = current
        .iter()
        .map(|r| serenity::RoleId::new(*r as u64))
        .collect();
    serenity::CreateActionRow::SelectMenu(
        serenity::CreateSelectMenu::new(
            id,
            serenity::CreateSelectMenuKind::Role {
                default_roles: Some(defaults),
            },
        )
        .placeholder(placeholder)
        .min_values(0)
        .max_values(10)
        .disabled(!enabled),
    )
}

/// The components of a screen.
pub fn components(
    screen: super::admin::Screen,
    e: &ExtState,
    enabled: bool,
) -> Vec<serenity::CreateActionRow> {
    use super::admin::Screen as S;
    let nav = |mut extra: Vec<serenity::CreateButton>| {
        extra.push(back_button(enabled));
        extra.push(close_button(enabled));
        buttons(extra)
    };
    let action = |id: &str, label: &str| {
        serenity::CreateButton::new(id)
            .label(label)
            .style(serenity::ButtonStyle::Secondary)
            .disabled(!enabled)
    };
    match screen {
        S::Status => vec![nav(vec![action("st:recheck", "إعادة الفحص")])],
        S::Daily => vec![
            buttons(vec![
                toggle("d:bd", "أعياد الميلاد", e.birthdays.enabled, enabled),
                toggle("d:fc", "شخصية اليوم", e.featured_character.enabled, enabled),
                toggle("d:fw", "عمل اليوم", e.featured_work.enabled, enabled),
            ]),
            menu(
                "d:hour",
                "ساعة النشر",
                hour_options(e.birthdays.hour),
                1,
                1,
                enabled,
            ),
            nav(vec![]),
        ],
        S::Poll => {
            let c = poll::Config::from(&e.poll);
            vec![
                buttons(vec![
                    toggle("p:toggle", "التصويت الأسبوعي", e.poll.enabled, enabled),
                    back_button(enabled),
                    close_button(enabled),
                ]),
                menu(
                    "p:day",
                    "اليوم",
                    DAYS_AR
                        .iter()
                        .enumerate()
                        .map(|(i, d)| ((*d).to_string(), i.to_string(), i as u32 == c.day))
                        .collect(),
                    1,
                    1,
                    enabled,
                ),
                menu(
                    "p:hour",
                    "الساعة",
                    hour_options(i64::from(c.hour)),
                    1,
                    1,
                    enabled,
                ),
                menu(
                    "p:dur",
                    "مدة التصويت",
                    poll::DURATIONS
                        .iter()
                        .map(|h| (format!("{h} ساعة"), h.to_string(), *h == c.duration_hours))
                        .collect(),
                    1,
                    1,
                    enabled,
                ),
            ]
        }
        S::Perms => {
            let mut rows: Vec<_> = e
                .perms
                .iter()
                .map(|(k, roles)| roles_menu(&format!("pm:{k}"), perms::label(k), roles, enabled))
                .collect();
            rows.push(nav(vec![]));
            rows
        }
        S::Watch => vec![nav(vec![
            action("w:new", "ليلة جديدة").style(serenity::ButtonStyle::Primary),
            toggle("w:event", "حدث ديسكورد", e.watch_event, enabled),
        ])],
        S::Owner => vec![nav(vec![
            action("o:backup", "نسخة احتياطية الآن"),
            toggle("o:maint", "وضع الصيانة", e.maintenance, enabled),
        ])],
        _ => Vec::new(),
    }
}

/// Find the work a watch night is about: an exact title wins, then AniList, then TMDB, then games.
async fn resolve_work(t: &Tools, name: &str, mature: bool) -> Option<(String, String)> {
    let q = name.trim();
    let mut found: Vec<(String, String)> = Vec::new();
    if let Ok(list) = t
        .anilist
        .search_any(q, 5, crate::anilist::ContentPolicy::SAFE)
        .await
    {
        found.extend(
            list.iter()
                .map(|m| (format!("anilist:{}", m.id), m.title.main().to_string())),
        );
    }
    if t.tmdb.enabled() {
        for kind in [crate::tmdb::Kind::Tv, crate::tmdb::Kind::Movie] {
            found.extend(
                t.tmdb
                    .search(kind, q, 1)
                    .await
                    .into_iter()
                    .take(3)
                    .map(|w| (w.key(), w.title)),
            );
        }
    }
    if t.providers.games_enabled() {
        found.extend(
            t.providers
                .search_games_found(q, None, mature, None)
                .await
                .works
                .into_iter()
                .take(3)
                .map(|w| (w.id, w.title)),
        );
    }
    found
        .iter()
        .find(|(_, title)| title.eq_ignore_ascii_case(q))
        .cloned()
        .or_else(|| found.into_iter().next())
}

/// Create a watch night from the modal line and announce it in the notifications channel.
pub async fn create_night(
    t: &Tools,
    pool: &SqlitePool,
    guild: i64,
    user: i64,
    input: &str,
) -> String {
    let now = chrono::Utc::now().timestamp();
    let tz: chrono_tz::Tz = crate::db::admin::timezone(pool, guild)
        .await
        .ok()
        .and_then(|z| z.parse().ok())
        .unwrap_or(chrono_tz::Asia::Muscat);
    let (name, starts, description) = match watch::parse_input(input, tz, now) {
        Ok(v) => v,
        Err(why) => return why.to_string(),
    };
    let Some(channel) = fdb::get_config(pool, guild)
        .await
        .ok()
        .filter(|c| c.enabled)
        .and_then(|c| c.channel_id)
    else {
        return "حدّد قناة الإشعارات أولًا بالأمر `/set-notifications-channel` ليُنشر فيها الإعلان."
            .to_string();
    };
    let mature = crate::db::custom::mature_themes(pool, guild)
        .await
        .unwrap_or(true);
    let Some((key, title)) = resolve_work(t, &name, mature).await else {
        return "لم أجد عملًا بهذا الاسم، جرّب اسمًا آخر.".to_string();
    };
    let new = watch::NewNight {
        guild,
        channel,
        work_key: &key,
        title: &title,
        description: &description,
        starts_at: starts,
        created_by: user,
    };
    let Ok(id) = watch::create(pool, &new).await else {
        return crate::i18n::ERR_GENERIC.to_string();
    };
    let Ok(Some(night)) = watch::get(pool, id).await else {
        return crate::i18n::ERR_GENERIC.to_string();
    };
    let msg = serenity::CreateMessage::new()
        .embed(watch::embed(&night, (0, 0)).build())
        .components(watch::rows(&night, false));
    let sent = serenity::ChannelId::new(channel as u64)
        .send_message(&t.discord, msg)
        .await;
    let Ok(sent) = sent else {
        return "لم أستطع النشر في قناة الإشعارات، تأكد من صلاحيات البوت فيها.".to_string();
    };
    // A Discord scheduled event too, only when the server asked for it and the bot may; failing is fine.
    let mut event_id = None;
    if settings::get(pool, guild, "watch")
        .await
        .map(|f| f.value("event") == Some(1))
        .unwrap_or(false)
    {
        if let Ok(start) = serenity::Timestamp::from_unix_timestamp(starts) {
            let end = serenity::Timestamp::from_unix_timestamp(starts + 2 * 3600).unwrap_or(start);
            let builder = serenity::CreateScheduledEvent::new(
                serenity::ScheduledEventType::External,
                fit(&title, 100),
                start,
            )
            .end_time(end)
            .location("ليلة مشاهدة في السيرفر")
            .description(fit(&description, 900));
            match serenity::GuildId::new(guild as u64)
                .create_scheduled_event(&t.discord, builder)
                .await
            {
                Ok(ev) => event_id = Some(ev.id.get().to_string()),
                Err(e) => tracing::debug!(
                    "could not create the scheduled event (needs Manage Events): {e}"
                ),
            }
        }
    }
    let _ = watch::set_message(pool, id, sent.id.get() as i64, event_id.as_deref()).await;
    format!(
        "أنشأتُ ليلة المشاهدة لـ «{title}» ونشرتُ الإعلان.{}",
        if event_id.is_some() {
            " وأنشأتُ حدثًا في ديسكورد أيضًا."
        } else {
            ""
        }
    )
}

/// The ids this module handles.
pub fn handles(id: &str) -> bool {
    ["st:", "d:", "p:", "pm:", "w:", "o:"]
        .iter()
        .any(|p| id.starts_with(p))
}

/// What a press produced.
#[derive(Default)]
pub struct Reply {
    pub notice: Option<String>,
    pub card: Option<CardFollowup>,
}

fn note(t: impl Into<String>) -> Reply {
    Reply {
        notice: Some(t.into()),
        card: None,
    }
}

/// A lightweight call to every source: records success or failure in the health table.
async fn recheck(t: &Tools) -> String {
    let http = &t.http;
    let mut results: Vec<(&str, bool)> = Vec::new();
    let call = |url: &'static str| async move {
        http.get(url)
            .timeout(std::time::Duration::from_secs(8))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    };
    let ani = http
        .post("https://graphql.anilist.co")
        .json(&serde_json::json!({"query": "query { GenreCollection }"}))
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);
    results.push(("anilist", ani));
    results.push(("mangadex", call("https://api.mangadex.org/ping").await));
    results.push((
        "mangaupdates",
        call("https://api.mangaupdates.com/v1/genres").await,
    ));
    if t.providers.vndb_enabled() {
        results.push(("vndb", call("https://api.vndb.org/kana/stats").await));
    }
    if t.tmdb.enabled() {
        results.push(("tmdb", t.tmdb.ping().await));
    }
    if t.providers.igdb_enabled() {
        results.push(("igdb", t.providers.ping_igdb().await));
    }
    for (name, ok) in &results {
        if *ok {
            health::ok(name);
        } else {
            health::err(name, "فشل فحص الصحة");
        }
    }
    let good = results.iter().filter(|(_, ok)| *ok).count();
    format!(
        "فحصتُ {} مصدرًا: {good} يعمل، {} لا يستجيب.",
        results.len(),
        results.len() - good
    )
}

fn toggled(f: &Feature) -> Feature {
    Feature {
        enabled: !f.enabled,
        ..f.clone()
    }
}

/// Apply one press. Owner-only ids are refused for everyone else.
pub async fn handle(
    t: &Tools,
    pool: &SqlitePool,
    guild: i64,
    e: &ExtState,
    id: &str,
    values: &[String],
) -> Result<Reply, sqlx::Error> {
    let first = values.first().map(String::as_str);
    if id.starts_with("o:") && !t.is_owner {
        return Ok(note("هذه الأدوات للمالك فقط."));
    }
    match id {
        "st:recheck" => Ok(note(recheck(t).await)),
        "d:bd" => settings::set(pool, guild, daily::BIRTHDAYS, &toggled(&e.birthdays))
            .await
            .map(|_| Reply::default()),
        "d:fc" => settings::set(
            pool,
            guild,
            daily::FEATURED_CHARACTER,
            &toggled(&e.featured_character),
        )
        .await
        .map(|_| Reply::default()),
        "d:fw" => settings::set(
            pool,
            guild,
            daily::FEATURED_WORK,
            &toggled(&e.featured_work),
        )
        .await
        .map(|_| Reply::default()),
        "d:hour" => {
            if let Some(h) = first
                .and_then(|h| h.parse::<i64>().ok())
                .filter(|h| (0..24).contains(h))
            {
                // One hour for the three daily posts.
                for (name, f) in [
                    (daily::BIRTHDAYS, &e.birthdays),
                    (daily::FEATURED_CHARACTER, &e.featured_character),
                    (daily::FEATURED_WORK, &e.featured_work),
                ] {
                    settings::set(
                        pool,
                        guild,
                        name,
                        &Feature {
                            hour: h,
                            ..f.clone()
                        },
                    )
                    .await?;
                }
            }
            Ok(Reply::default())
        }
        "p:toggle" => settings::set(pool, guild, poll::FEATURE, &toggled(&e.poll))
            .await
            .map(|_| Reply::default()),
        "p:day" | "p:hour" | "p:dur" => {
            let c = poll::Config::from(&e.poll);
            let v = first.and_then(|v| v.parse::<i64>().ok());
            let (mut day, mut hour, mut dur) =
                (i64::from(c.day), i64::from(c.hour), c.duration_hours);
            match (id, v) {
                ("p:day", Some(v)) if (0..7).contains(&v) => day = v,
                ("p:hour", Some(v)) if (0..24).contains(&v) => hour = v,
                ("p:dur", Some(v)) if poll::DURATIONS.contains(&v) => dur = v,
                _ => return Ok(Reply::default()),
            }
            settings::set(
                pool,
                guild,
                poll::FEATURE,
                &Feature {
                    enabled: e.poll.enabled,
                    hour,
                    extra: format!("day={day};hours={dur}"),
                },
            )
            .await
            .map(|_| Reply::default())
        }
        id if id.starts_with("pm:") => {
            let action = &id[3..];
            let roles: Vec<i64> = values.iter().filter_map(|v| v.parse().ok()).collect();
            perms::set_roles(pool, guild, action, &roles)
                .await
                .map(|_| Reply::default())
        }
        "w:event" => {
            let cur = settings::get(pool, guild, "watch").await?;
            let on = cur.value("event") == Some(1);
            settings::set(
                pool,
                guild,
                "watch",
                &Feature {
                    enabled: true,
                    extra: format!("event={}", u8::from(!on)),
                    ..cur
                },
            )
            .await
            .map(|_| Reply::default())
        }
        "o:maint" => {
            t.ops.set_maintenance(!e.maintenance).await?;
            Ok(note(if e.maintenance {
                "أُطفئ وضع الصيانة."
            } else {
                "فُعّل وضع الصيانة: الأوامر متوقفة لغيرك."
            }))
        }
        "o:backup" => Ok(match backup::make(pool, &t.ops.db_path).await {
            Ok(m) if m.size <= backup::MAX_ATTACH_BYTES => match std::fs::read(&m.path) {
                Ok(bytes) => Reply {
                    notice: Some(format!(
                        "نسخة جاهزة ({}). أرسلتُها لك خاصةً.",
                        backup::size_text(m.size)
                    )),
                    card: Some(CardFollowup {
                        embed: Emb::new()
                            .title("💾 نسخة احتياطية")
                            .description(format!(
                                "{} · {}",
                                m.path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default(),
                                backup::size_text(m.size)
                            ))
                            .build(),
                        files: vec![serenity::CreateAttachment::bytes(
                            bytes,
                            m.path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "backup.db".into()),
                        )],
                        link: None,
                    }),
                },
                Err(_) => note(format!(
                    "تمت النسخة لكن تعذّرت قراءتها: {} ({})",
                    m.path.display(),
                    backup::size_text(m.size)
                )),
            },
            Ok(m) => note(format!(
                "النسخة أكبر من حد ديسكورد للملفات، موجودة هنا: {} ({})",
                m.path.display(),
                backup::size_text(m.size)
            )),
            Err(backup::Skipped::LowSpace { .. }) => {
                note("لم أنشئ النسخة: المساحة الحرة قليلة. احذف نسخًا قديمة أو وسّع القرص.")
            }
            Err(e) => note(format!("فشلت النسخة: {e}")),
        }),
        _ => Ok(Reply::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::admin::Screen;

    fn ext() -> ExtState {
        ExtState {
            is_owner: true,
            channel_set: true,
            perms: perms::ACTIONS.iter().map(|(k, _)| (*k, vec![])).collect(),
            status: status_rows(
                &Flags {
                    tmdb: true,
                    igdb: false,
                    vndb: true,
                    mangadex: true,
                    mangaupdates: false,
                },
                &["feeds.example".into()],
            ),
            report: Some(Report::default()),
            ..ExtState::default()
        }
    }

    #[test]
    fn the_status_panel_lists_every_provider_and_each_feed_with_enabled_state_and_budget() {
        let rows = status_rows(
            &Flags {
                tmdb: true,
                igdb: false,
                vndb: true,
                mangadex: true,
                mangaupdates: false,
            },
            &["a.example".into(), "b.example".into()],
        );
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "AniList",
                "TMDB",
                "IGDB",
                "VNDB",
                "MangaDex",
                "MangaUpdates",
                "RSS · a.example",
                "RSS · b.example"
            ]
        );
        assert!(!rows[2].enabled && rows[1].enabled);
        assert!(rows.iter().all(|r| !r.budget.is_empty()));
        let text = embed(Screen::Status, &ext())
            .description
            .unwrap_or_default();
        assert!(
            text.contains("⚪ **IGDB** (معطّل)")
                && text.contains("**AniList**")
                && text.contains("آخر نجاح")
                && text.contains("الميزانية"),
            "{text}"
        );
    }

    #[test]
    fn a_failing_provider_shows_a_warning_with_a_short_error_and_never_a_secret() {
        health::err("vndb", "400 /vn: Unknown field");
        let rows = status_rows(
            &Flags {
                vndb: true,
                ..Flags::default()
            },
            &[],
        );
        let line = status_line(rows.iter().find(|r| r.name == "VNDB").expect("vndb"));
        assert!(
            line.starts_with("⚠️ **VNDB**") && line.contains("Unknown field"),
            "{line}"
        );
        health::ok("vndb");
        let rows = status_rows(
            &Flags {
                vndb: true,
                ..Flags::default()
            },
            &[],
        );
        assert!(
            status_line(rows.iter().find(|r| r.name == "VNDB").expect("vndb")).starts_with("✅"),
            "a success after the error"
        );
    }

    #[test]
    fn maintenance_shows_in_the_status_panel_and_every_screen_fits_five_rows() {
        let mut e = ext();
        e.maintenance = true;
        assert!(embed(Screen::Status, &e)
            .description
            .unwrap_or_default()
            .contains("وضع الصيانة مفعّل"));
        for screen in [
            Screen::Status,
            Screen::Daily,
            Screen::Poll,
            Screen::Perms,
            Screen::Watch,
            Screen::Owner,
        ] {
            let rows = components(screen, &e, true);
            let json = serde_json::to_string(&rows).expect("json");
            assert!(
                !rows.is_empty()
                    && rows.len() <= 5
                    && json.contains("\"back\"")
                    && json.contains("\"close\""),
                "{screen:?}: {} rows",
                rows.len()
            );
            assert!(embed(screen, &e).description.is_some(), "{screen:?}");
        }
    }

    #[test]
    fn daily_posts_default_off_and_warn_when_there_is_no_notifications_channel() {
        let mut e = ext();
        e.channel_set = false;
        let d = embed(Screen::Daily, &e).description.unwrap_or_default();
        assert!(
            d.contains("❌ معطّل") && d.contains("لا توجد قناة إشعارات"),
            "{d}"
        );
        assert!(!ExtState::default().poll.enabled && !ExtState::default().birthdays.enabled);
    }

    #[test]
    fn the_owner_screen_shows_the_statistics_and_backups() {
        let mut e = ext();
        e.report = Some(Report {
            per_day: vec![("2026-11-17".into(), 12)],
            per_command: vec![("anime".into(), 8)],
            errors: vec![("vndb".into(), 3)],
            cache: (3, 1),
            api_calls: 9,
            active: vec![("2026-11-17".into(), 4)],
        });
        e.backups = (3, Some("bot-20261117.db (1.0 MB)".into()));
        let d = embed(Screen::Owner, &e).description.unwrap_or_default();
        for want in [
            "11-17: 12",
            "/anime 8",
            "vndb 3",
            "النشطون اليوم: 4",
            "75% إصابة",
            "طلبات API: 9",
            "النسخ الاحتياطية: 3",
        ] {
            assert!(d.contains(want), "{want} in {d}");
        }
    }

    #[test]
    fn only_the_ids_of_this_module_are_handled_here() {
        for id in [
            "st:recheck",
            "d:bd",
            "p:day",
            "pm:upload",
            "w:new",
            "o:backup",
        ] {
            assert!(handles(id));
        }
        for id in ["s:nsfw", "n:toggle", "go:poll", "back", "close"] {
            assert!(!handles(id), "{id}");
        }
    }
}
