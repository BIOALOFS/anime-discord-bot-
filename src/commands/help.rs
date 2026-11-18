//! `/help`: one ephemeral message. A section menu, a menu of every real command (built from the registered list, so
//! it can never go stale), three quick guides as buttons, and an about page with the required credits.

use super::buttons;
use crate::{
    error::{Context, Error},
    tmdb,
    ui::{
        embeds::{fit, Emb},
        pagination::{close_button, CLOSE},
        select::{select_row, Choice},
        session::{self, Outcome, Page, View},
    },
};
use poise::serenity_prelude as serenity;

const SECTION_ID: &str = "help_section";
const COMMAND_ID: &str = "help_command";
const GUIDE_PREFIX: &str = "help_guide:";

pub struct Section {
    pub key: &'static str,
    pub label: &'static str,
    pub intro: &'static str,
    /// Command names this section explains; every registered command is in exactly one section.
    pub commands: &'static [&'static str],
}

pub const SECTIONS: &[Section] = &[
    Section {
        key: "start",
        label: "البداية",
        intro: "أهلًا! أنا بوت للأنمي والمانجا والروايات والدراما والمسلسلات والأفلام والألعاب.\n\n\
                اكتب `/` في الشات واختر الأمر. أوامر الأعمال كلها تعمل بنفس الطريقة: تختار **النوع** (عمل، شخصية، شخص، عمل عشوائي، شخصية عشوائية) ثم تكتب **الاسم** وتختار من الاقتراحات.\n\n\
                تحت أي رسالة عن شخصية ستجد «أظهر العمل» (و«أظهر المؤدي» إن وُجد مؤدٍّ)، وتحت أي رسالة عن عمل ستجد «أظهر الشخصيات» و«أظهر الطاقم»، وتحت بطاقة الشخص «أظهر الأعمال». كلها تغيّر نفس الرسالة وتضيف زر «رجوع» ليعيدك للعرض السابق. وإن ضغط عضو آخر على أزرار رسالتك فتصله نسخة خاصة به ولا تتغير رسالتك. أزرار الإشعارات تفتح لك نتيجة خاصة.\n\n\
                الأزرار والقوائم تتعطّل بصمت بعد دقيقة من آخر ضغطة (بروفايلك 5 دقائق، والإشعارات ساعة من إرسالها)، وتبقى الرسالة كما هي.\n\n\
                اختر قسمًا من القائمة الأولى، أو أمرًا من الثانية، أو جرّب الأدلة السريعة بالأسفل.",
        commands: &["help"],
    },
    Section {
        key: "works",
        label: "الأعمال والشخصيات",
        intro: "سبعة أوامر بنفس الشكل: `/anime` `/manga` `/novel` `/drama` `/series` `/movie` `/game`، وأمر ثامن `/search` يبحث في كل الأقسام معًا.\n\n\
                • **النوع:** عمل، شخصية، شخص (ممثلون ومؤدو أصوات وطاقم)، عمل عشوائي، أو شخصية عشوائية. الأشخاص من AniList في الأنمي والمانجا والروايات، ومن TMDB في الدراما والمسلسلات والأفلام، ومن IGDB وVNDB في الألعاب.\n\
                • **`/search`:** اكتب اسمًا واختر من اقتراحات مختلطة (🎬 أعمال، 👤 شخصيات، 🎙️ أشخاص)، ويمكنك حصر البحث بالقسم أو النوع.\n\
                • **الاسم:** تظهر اقتراحات أثناء الكتابة. مع العشوائي لا تحتاج اسمًا.\n\
                • **الفلاتر** تختلف بحسب الأمر (الصيغة، السنة، الحالة، البلد، التصنيف، المنصة…) وتنطبق على البحث وعلى العشوائي.\n\
                • زر «عشوائي آخر» يغيّر النتيجة في الرسالة نفسها دون تكرار آخر خمس نتائج لك.\n\n\
                • **الويكيات:** مالك البوت يضيف ويكيات (Fandom وwiki.gg وغيرها) تعمل في كل السيرفرات تلقائيًا: تدخل في البحث، وإن لم تجد المصادر الأساسية شيئًا تظهر نتائجها مباشرة، وتكمل ما ينقص البطاقات (صورة أو ملخص). في `/game` يمكنك حصر البحث من خيار «المصدر»: IGDB أو VNDB أو «كل الويكيات» أو ويكي بعينها.\n\
                الدراما الآسيوية في `/drama` وبقية العالم في `/series`، والمشرف يقدر ينقل بلدًا بين الاثنين من `/admin`.\n\n**أمان الألعاب:** أحكم على اللعبة بمحتواها (المواضيع والأنواع والكلمات المفتاحية والعلامات) لا بتصنيفها العمري، فأخفي ما فيه محتوى جنسي دائمًا، وألعاب الرعب والحرب تظهر ما لم يطفئ المشرف «المواضيع الناضجة» من `/admin` ← الأمان.",
        commands: &[
            "anime", "manga", "novel", "drama", "series", "movie", "game", "search",
        ],
    },
    Section {
        key: "events",
        label: "الفعاليات والترتيب",
        intro: "ألعاب جماعية، ترتيب السيرفر، وجدول الأسبوع.\n\n• **`/recommend`** له خيار «المصدر»: «من بروفايلي» (توصيات من أعمالك من AniList وTMDB) أو «عامة». الافتراضي من بروفايلك إذا كان فيه 3 عناصر على الأقل.",
        commands: &["event", "top", "schedule", "airing", "recommend"],
    },
    Section {
        key: "profile",
        label: "بروفايلك",
        intro: "بروفايلك يجمع شخصياتك وأعمالك من كل المصادر، ويمكنك إضافة ممثلين ومؤدي أصوات وطاقم من بطاقة الشخص فيظهرون مع الشخصيات (شخص حقيقي · ممثل). العرض من `/profile`، وكل التعديلات من `/edit`.\n\n• **المقارنة:** `/profile` مع خيار «مقارنة» واختيار عضو يعرض نسبة تطابق ذوقكما والعناصر المشتركة (يتعذر مع البروفايلات الخاصة).\n• **الشارات:** سطر صغير يظهر في بروفايلك عند إنجازاتك (عدد العناصر، الفعاليات، الاقتراحات…).",
        commands: &["profile", "edit", "suggest"],
    },
    Section {
        key: "settings",
        label: "تذكيراتك وإعداداتك",
        intro: "التذكير زر 🔔 «ذكّرني» على بطاقة العمل: يصلك خاص عند صدور حلقة أو فصل جديد، وإن كان خاصك مغلقًا يصل في قناة الإشعارات إذا سمح المشرف.",
        commands: &["settings"],
    },
    Section {
        key: "admin",
        label: "للمشرفين",
        intro: "هذه الأوامر تظهر لمن يملك صلاحية «إدارة السيرفر» فقط.\n\n**أقسام `/admin`:** الأمان، الإشعارات، الصور، الرتب، القنوات، الحدود، مصادر الفصول (وتكرار الملخص)، المسلسلات والجدول، **حالة المصادر** (آخر نجاح وخطأ لكل مصدر وزر إعادة الفحص)، **منشورات يومية** (أعياد ميلاد الشخصيات وشخصية اليوم وعمل اليوم، مطفأة افتراضيًا)، **تصويت أسبوعي** «ماذا نشاهد؟»، **صلاحيات الرتب** (من يرفع الصور ويضيف الاقتراحات ويبدأ الفعاليات ويعدّل البروفايل)، **ليلة مشاهدة**، وللمالك فقط: النسخ الاحتياطي والإحصاءات ووضع الصيانة.",
        commands: &["admin", "set-notifications-channel", "set-bot-channel"],
    },
    Section {
        key: "about",
        label: "عن البوت والمصادر",
        intro: "",
        commands: &[],
    },
];

/// The quick guides: `(key, button label, text)`.
pub const GUIDES: &[(&str, &str, &str)] = &[
    (
        "character",
        "كيف أضيف شخصية لبروفايلي",
        "1. ابحث عن الشخصية بأي أمر من أوامر الأعمال (مثل `/anime` ثم النوع «شخصية»).\n\
         2. اضغط **➕ أضف لبروفايلي** تحت بطاقتها.\n\
         3. افتح `/profile` لتراها، وعدّل ترتيبها أو لقبها أو صورتها من `/edit`.\n\n\
         تقدر أيضًا تضيفها من قائمة شخصيات أي عمل بزر «أظهر الشخصيات».",
    ),
    (
        "reminder",
        "كيف أفعّل التذكير",
        "1. افتح بطاقة العمل (أنمي أو دراما أو مسلسل أو فيلم أو مانجا).\n\
         2. اضغط **🔔 ذكّرني**، وللإلغاء اضغطه مرة ثانية.\n\
         3. ستصلك رسالة خاصة عند صدور حلقة أو فصل جديد، ورسالة «قريبًا» قبل صدور الأفلام والحلقات.\n\n\
         قائمة تذكيراتك وإعدادات الإشعارات في `/settings`.",
    ),
    (
        "customize",
        "كيف أخصص بروفايلي",
        "كل شيء من `/edit`:\n\
         • **الغلاف:** اختر غلافًا جاهزًا أو أرفق صورتك أو GIF بأي مقاس.\n\
         • **النبذة والاسم واللون والخصوصية.**\n\
         • **الشخصيات:** غيّر صورة أو لقب أو وصف أي شخصية في بروفايلك، أو انقلها لمكان آخر.\n\n\
         الصور المخصصة تعمل إذا فعّلها مشرف السيرفر.",
    ),
];

/// What `/help` knows about one registered command.
#[derive(Debug, Clone, PartialEq)]
pub struct CmdInfo {
    pub name: String,
    pub ar_name: String,
    pub description: String,
    /// `(Arabic option name, Arabic description)`.
    pub options: Vec<(String, String)>,
    pub subcommands: Vec<String>,
}

type Command = poise::Command<crate::Data, Error>;

/// Built from the registered commands themselves.
pub fn cmd_infos(cmds: &[Command]) -> Vec<CmdInfo> {
    cmds.iter()
        .map(|c| CmdInfo {
            name: c.name.to_string(),
            ar_name: c
                .name_localizations
                .get("ar")
                .cloned()
                .unwrap_or_else(|| c.name.to_string()),
            description: c
                .description_localizations
                .get("ar")
                .cloned()
                .or_else(|| c.description.clone().map(|d| d.to_string()))
                .unwrap_or_default(),
            options: c
                .parameters
                .iter()
                .map(|p| {
                    (
                        p.name_localizations
                            .get("ar")
                            .cloned()
                            .unwrap_or_else(|| p.name.to_string()),
                        p.description_localizations
                            .get("ar")
                            .cloned()
                            .or_else(|| p.description.clone().map(|d| d.to_string()))
                            .unwrap_or_default(),
                    )
                })
                .collect(),
            subcommands: c
                .subcommands
                .iter()
                .map(|s| {
                    format!(
                        "{} — {}",
                        s.name_localizations
                            .get("ar")
                            .cloned()
                            .unwrap_or_else(|| s.name.to_string()),
                        s.description_localizations
                            .get("ar")
                            .cloned()
                            .unwrap_or_default()
                    )
                })
                .collect(),
        })
        .collect()
}

/// A short usage example per command.
pub fn example(name: &str) -> &'static str {
    match name {
        "anime" => "`/anime` النوع: عمل، الاسم: Frieren",
        "manga" => "`/manga` النوع: عمل عشوائي، الصيغة: مانهوا كورية",
        "novel" => "`/novel` النوع: شخصية، الاسم: Subaru",
        "drama" => "`/drama` النوع: عمل، الاسم: Crash Landing on You",
        "series" => "`/series` النوع: عمل عشوائي، التصنيف: غموض",
        "movie" => "`/movie` النوع: عمل، الاسم: Spirited Away",
        "game" => "`/game` النوع: عمل، الاسم: Hades، المنصة: PC",
        "search" => "`/search` الاسم: Silent Hill",
        "event" => "`/event` الفئة: أنمي، النوع: تخمين الشخصية",
        "top" => "`/top` القسم: دراما، المدة: هذا الأسبوع",
        "schedule" => "`/schedule` ثم اختر اليوم والفئة",
        "profile" => "`/profile` أو `/profile` المستخدم: @عضو",
        "edit" => "`/edit` الهدف: بروفايل، الإجراء: غلاف",
        "suggest" => "`/suggest add` ربط عملين بسبب",
        "recommend" => "`/recommend` توصيات من مفضلاتك",
        "airing" => "`/airing` الحلقة القادمة لأنمي",
        "settings" => "`/settings` تذكيراتك وخصوصيتك",
        "help" => "`/help`",
        "admin" => "`/admin` لوحة إعدادات السيرفر",
        "set-notifications-channel" => "`/set-notifications-channel` قناة الإشعارات",
        "set-bot-channel" => "`/set-bot-channel` القناة الوحيدة للبوت",
        _ => "",
    }
}

fn about_text() -> String {
    format!(
        "**المصادر**\n\
         • الأنمي والمانجا والروايات والشخصيات: AniList.\n\
         • الدراما والمسلسلات والأفلام والممثلون: TMDB. «{}»\n\
         • _{}_\n\
         • أماكن المشاهدة: {} (JustWatch) عبر TMDB.\n\
         • الألعاب: IGDB، والروايات البصرية: VNDB.\n\
         • فصول المانجا: MangaDex وMangaUpdates.\n\n\
         **الأمان:** لا أعرض أي عمل للبالغين ولا محتوى جنسيًا من أي مصدر، وإذا لم أتأكد من أمان عمل أخفيه.\n\
         **الخصوصية:** لا أطلب صلاحيات حساسة ولا أقرأ رسائل الأعضاء.",
        tmdb::ATTRIBUTION_AR,
        tmdb::ATTRIBUTION_EN,
        tmdb::JUSTWATCH_CREDIT,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Shown {
    Section(usize),
    Command(usize),
    Guide(usize),
}

struct HelpView {
    cmds: Vec<CmdInfo>,
    shown: Shown,
}

impl HelpView {
    fn embed(&self) -> Emb {
        match &self.shown {
            Shown::Section(i) => {
                let s = &SECTIONS[*i];
                let body = if s.key == "about" {
                    about_text()
                } else {
                    s.intro.to_string()
                };
                let mut list = String::new();
                for name in s.commands {
                    if let Some(c) = self.cmds.iter().find(|c| c.name == *name) {
                        list.push_str(&format!("\n• `/{}` — {}", c.name, c.description));
                    }
                }
                Emb::new()
                    .title(format!("المساعدة — {}", s.label))
                    .description(fit(&format!("{body}\n{list}"), 3900))
            }
            Shown::Command(i) => {
                let c = &self.cmds[*i];
                let mut body = format!("{}\n", c.description);
                if !c.options.is_empty() {
                    body.push_str("\n**الخيارات:**\n");
                    for (n, d) in &c.options {
                        body.push_str(&format!("• **{n}** — {d}\n"));
                    }
                }
                if !c.subcommands.is_empty() {
                    body.push_str("\n**الأوامر الفرعية:**\n");
                    for s in &c.subcommands {
                        body.push_str(&format!("• {s}\n"));
                    }
                }
                let ex = example(&c.name);
                if !ex.is_empty() {
                    body.push_str(&format!("\n**مثال:** {ex}"));
                }
                Emb::new()
                    .title(format!("/{} — {}", c.name, c.ar_name))
                    .description(fit(&body, 3900))
            }
            Shown::Guide(i) => Emb::new().title(GUIDES[*i].1).description(GUIDES[*i].2),
        }
    }
}

impl View for HelpView {
    fn ephemeral(&self) -> bool {
        true
    }

    fn render(&self, enabled: bool) -> Page {
        let sections = SECTIONS
            .iter()
            .map(|s| Choice {
                label: s.label.to_string(),
                description: None,
                value: s.key.to_string(),
            })
            .collect();
        let commands = self
            .cmds
            .iter()
            .map(|c| Choice {
                label: format!("/{}", c.name),
                description: Some(fit(&c.description, 100)),
                value: c.name.clone(),
            })
            .collect();
        let mut rows: Vec<_> = select_row(SECTION_ID, "اختر قسمًا…", sections, enabled)
            .into_iter()
            .collect();
        rows.extend(select_row(COMMAND_ID, "أو اختر أمرًا…", commands, enabled));
        let mut guide_row: Vec<_> = GUIDES
            .iter()
            .map(|(k, label, _)| {
                serenity::CreateButton::new(format!("{GUIDE_PREFIX}{k}"))
                    .label(*label)
                    .style(serenity::ButtonStyle::Secondary)
                    .disabled(!enabled)
            })
            .collect();
        guide_row.push(close_button(enabled));
        rows.push(buttons(guide_row));
        Page::embed(self.embed().build(), rows)
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        match id {
            CLOSE => Outcome::Close,
            SECTION_ID => match values
                .first()
                .and_then(|v| SECTIONS.iter().position(|s| s.key == v))
            {
                Some(i) => {
                    self.shown = Shown::Section(i);
                    Outcome::Update
                }
                None => Outcome::Keep,
            },
            COMMAND_ID => match values
                .first()
                .and_then(|v| self.cmds.iter().position(|c| &c.name == v))
            {
                Some(i) => {
                    self.shown = Shown::Command(i);
                    Outcome::Update
                }
                None => Outcome::Keep,
            },
            other => match other
                .strip_prefix(GUIDE_PREFIX)
                .and_then(|k| GUIDES.iter().position(|g| g.0 == k))
            {
                Some(i) => {
                    self.shown = Shown::Guide(i);
                    Outcome::Update
                }
                None => Outcome::Keep,
            },
        }
    }
}

/// How to use the bot, from the real list of commands.
#[poise::command(
    slash_command,
    name_localized("ar", "مساعدة"),
    description_localized("ar", "شرح الأوامر والأدلة السريعة والمصادر")
)]
pub async fn help(ctx: Context<'_>) -> Result<(), Error> {
    let cmds = cmd_infos(&ctx.framework().options().commands);
    session::run(
        ctx,
        HelpView {
            cmds,
            shown: Shown::Section(0),
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_section_and_guide_fits_the_limits() {
        assert!(SECTIONS.len() <= 25 && GUIDES.len() <= 4);
        let mut keys = std::collections::HashSet::new();
        assert!(SECTIONS.iter().all(|s| keys.insert(s.key)));
        for s in SECTIONS {
            assert!(s.intro.chars().count() <= 3900, "{}", s.key);
        }
        for (_, label, text) in GUIDES {
            assert!(label.chars().count() <= 80 && text.chars().count() <= 3900);
        }
        let labels: Vec<_> = GUIDES.iter().map(|g| g.1).collect();
        assert_eq!(
            labels,
            [
                "كيف أضيف شخصية لبروفايلي",
                "كيف أفعّل التذكير",
                "كيف أخصص بروفايلي"
            ]
        );
    }

    #[test]
    fn the_about_page_carries_the_required_credits() {
        let t = about_text();
        assert!(t.contains(tmdb::ATTRIBUTION_AR) && t.contains(tmdb::ATTRIBUTION_EN));
        assert!(
            t.contains("JustWatch")
                && t.contains("AniList")
                && t.contains("IGDB")
                && t.contains("VNDB")
        );
        assert!(t.chars().count() < 3900);
    }

    #[test]
    fn nothing_is_registered_without_a_section_and_nothing_is_documented_that_does_not_exist() {
        // The cross-check against the real command list lives in main's tests (it needs the registry).
        let mut all: Vec<&str> = SECTIONS
            .iter()
            .flat_map(|s| s.commands.iter().copied())
            .collect();
        let n = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(n, all.len(), "a command is in two sections");
        assert!(
            all.iter().all(|c| !example(c).is_empty()),
            "every documented command has an example"
        );
    }
}
