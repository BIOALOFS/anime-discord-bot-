//! `/help category`: commands grouped by category, with examples and a select menu to switch.

use super::buttons;
use crate::{
    error::{Context, Error},
    ui::{
        autocomplete::to_choices,
        embeds::Emb,
        pagination::{close_button, CLOSE},
        select::{select_row, Choice},
        session::{self, Outcome, Page, View},
    },
};
use poise::serenity_prelude as serenity;

const CATEGORY_ID: &str = "help_category";

pub struct HelpCategory {
    pub key: &'static str,
    pub label: &'static str,
    pub body: &'static str,
}

pub const CATEGORIES: &[HelpCategory] = &[
    HelpCategory {
        key: "search",
        label: "بحث",
        body: "**`/search`** ابحث عن عمل: أنمي أو مانجا أو مانهوا أو مانهوا صينية أو رواية، ولعبة أو رواية بصرية عند تفعيل مصادرها.\n\
               مثال: `/search type:مانهوا name:Solo Leveling`\n\
               • الاقتراحات تتغير بحسب النوع الذي اخترته.\n\
               • على بطاقة العمل: ⭐ أضف إلى بروفايلك، 👥 الشخصيات، 🔗 الرابط، وقائمة منسدلة بالشخصيات الرئيسية تفتح بطاقة الشخصية في نفس الرسالة.\n\n\
               **`/character`** ابحث عن شخصية من أنمي أو مانجا أو لعبة (IGDB) أو رواية بصرية (VNDB).\n\
               مثال: `/character name:Mario` أو `/character name:Pyramid Head source:ألعاب`\n\
               • خيار `work` يحصر النتائج في عمل معين، وخيار `source` يختار: الكل / أنمي-مانجا / ألعاب.\n\
               • عند تعدد النتائج تظهر قائمة اختيار. على البطاقة: 📖 الوصف، ➕ أضف لبروفايلي، 👥 شخصيات العمل، 🎲 عشوائية.\n\
               • تحت كل صورة شخصية أزرار: ➕ إضافة لبروفايلي (تتحول إلى ➖ إزالة إذا كانت فيه)، 🎬 الأعمال، 🔗 رابط المصدر، ↩ رجوع. وفي كل رسالة تذكر شخصيات قائمة منسدلة تفتح الشخصية في نفس الرسالة.\n\
               • على بطاقة الأنمي زر 🔔 «ذكّرني» يرسل لك رسالة خاصة عند صدور حلقة جديدة (وزر 🔕 لإلغائه).\n\n\
               **`/airing`** موعد الحلقة القادمة لأنمي، أو جدول هذا الأسبوع.\n\
               **`/random`** اختيار عشوائي: عمل أو شخصية أو `شخصية اليوم` أو `أعياد ميلاد اليوم`.\n\
               مثال: `/random type:أنمي genre:Action min_score:75`\n\
               **`/top`** أكثر الشخصيات أو الأعمال تفضيلًا في السيرفر أو عالميًا (مع قائمة منسدلة لفتح بطاقة أي شخصية).",
    },
    HelpCategory {
        key: "profile",
        label: "بروفايل",
        body: "**`/profile`** بدون خيارات يعرض بروفايلك (مع `user` يعرض بروفايل عضو) في رسالة واحدة: **غلاف** ثم **بطاقة معلومات** (الاسم والنبذة وعدد الشخصيات والأعمال وتاريخ الانضمام والخصوصية) ثم **بطاقة العنصر** الحالي (`3/12`).\n\
               الأزرار: ⏮ الأول ◀ السابق، زر التبديل (يعرض اسم الوضع الآخر: «أعمال» أو «شخصيات») ▶ التالي ⏭ الأخير.\n\
               القائمة الأولى للقفز إلى أي عنصر في الصفحة الحالية (25)، والثانية لتغيير الصفحة.\n\
               • `action:add-character` أو `add-work` مع `value` لإضافة شخصية أو عمل.\n\
               مثال: `/profile action:add-work value:Tower of God`\n\
               كل التعديلات من **`/edit`**.",
    },
    HelpCategory {
        key: "edit",
        label: "تعديل",
        body: "**`/edit`** كل شيء يُعدَّل من هنا: `target` ثم `action` (ثم `character` أو `value` أو `text` أو `image`).\n\
               • **profile**: `banner` الغلاف: بدون قيمة تفتح قائمة **16 غلافًا جاهزًا** مع معاينة، أو أرفق صورة/GIF (حتى 3MB، متحرك)، أو `value` لون مثل #ff8800 أو reset، \
               `description` النبذة (300 حرف)، `title` الاسم المعروض (40)، `color` لون البطاقة، `privacy` الخصوصية، `reset` إعادة الضبط، \
               `export` / `import` / `delete-data` لبياناتك.\n\
               • **character**: اختر `character` من بروفايلك ثم `image` (PNG/JPG/WebP/GIF حتى 3MB، وGIF لشخصيات بروفايلك فقط)، `title` اللقب (40)، \
               `description` الوصف (300)، `reset` إعادة للأصل، `remove` إزالة، `move` نقل الموضع (النص = الرقم).\n\
               • **work**: `remove` أو `move` لعمل من `value`.\n\
               كل تأكيد خاص بك، وفيه زر «تراجع» عند الحاجة. الصور المخصصة تعمل فقط إذا فعّلها مشرف السيرفر.",
    },
    HelpCategory {
        key: "alerts",
        label: "إشعارات وتذكير",
        body: "إشعارات الحلقات الجديدة تغطي **كل الأنمي تلقائيًا** عند تحديد قناة الإشعارات بـ **`/set-notifications-channel`**.\n\
               المشرف يضبط التصفية من **`/admin`** ← الإشعارات: الكل (الافتراضي)، الشائع فقط، أعمال بروفايلات الأعضاء فقط، أو صيغ وأنواع محددة. وعند كثرة الإصدارات تُجمَّع في رسائل قليلة.\n\
               **التذكير الشخصي:** زر «ذكّرني» على بطاقة الأنمي يرسل لك رسالة خاصة (وإن كانت مغلقة فتنبيه في قناة الإشعارات إذا سمح المشرف). قائمة تذكيراتك وحذفها: `/settings setting:reminders` (حتى 50).\n\
               ملاحظة: بيانات الجدول متاحة للأنمي فقط في AniList، وليست للمانجا أو المانهوا.",
    },
    HelpCategory {
        key: "suggest",
        label: "اقتراحات الأعضاء",
        body: "**`/suggest add`** اربط بين عملين من أي نوع (أنمي، مانجا، رواية، لعبة، رواية بصرية) مع سبب (300 حرف): `first` و`second` و`reason`. لا يُقبل نفس الزوج مرتين في السيرفر (بأي ترتيب)، وحد يومي لكل عضو يضبطه المشرف.\n\
               **`/suggest random`** اقتراح عشوائي من السيرفر مع ذكر صاحبه، وأزرار: اقتراح آخر وفتح العملين. خيار `work` يحصر الاقتراحات في عمل.\n\
               **`/suggest mine`** اقتراحاتك مع قائمة لحذف أحدها. المشرف يحذف أي اقتراح من زر 🗑 في `/suggest random`.\n\
               **`/recommend`** توصيات تلقائية من AniList مبنية على مفضلاتك.",
    },
    HelpCategory {
        key: "settings",
        label: "إعدادات",
        body: "**`/settings`** تفضيلاتك الشخصية فقط: `setting:language` اللغة، `setting:privacy` الخصوصية، `setting:reminders` تذكيراتك.\n\
               **إدارة السيرفر (للمشرفين، مخفية عن غيرهم):**\n\
               • **`/admin`** لوحة أزرار: الأمان (NSFW، الألعاب، الألعاب الناضجة)، الإشعارات (التشغيل، التصفية، تنبيه التذكير بالقناة)، الصور المخصصة (التفعيل ومن يرفع)، الرتب، القنوات (عرض وإلغاء)، الحدود (مهلة الأوامر وحد الاقتراحات)، إعادة الضبط.\n\
               • **`/set-bot-channel`** القناة الوحيدة التي تعمل فيها أوامر البوت (المشرفون مستثنون).\n\
               • **`/set-notifications-channel`** قناة إشعارات الحلقات.\n\
               إلغاء تحديد أي قناة من `/admin` ← القنوات.",
    },
    HelpCategory {
        key: "games",
        label: "ألعاب وتوصيات",
        body: "**`/game`** `guess-character` خمّن الشخصية من صورة مقصوصة، `guess-anime` خمّن الأنمي من وصف مقنّع، \
               `versus` أيهما تفضل، `leaderboard` الترتيب.\n\
               **`/recommend`** توصيات مبنية على مفضلاتك. مع `user` تحصل على توصيات مشتركة بينكما.\n\
               **`/help`** يعرض هذه المساعدة، ويمكنك تغيير القسم من القائمة.",
    },
];

pub fn find(key: &str) -> Option<&'static HelpCategory> {
    CATEGORIES.iter().find(|c| c.key == key)
}

struct HelpView {
    current: &'static HelpCategory,
}

impl View for HelpView {
    fn render(&self, enabled: bool) -> Page {
        let embed = Emb::new()
            .title(format!("المساعدة — {}", self.current.label))
            .description(self.current.body)
            .build();
        let choices = CATEGORIES
            .iter()
            .map(|c| Choice {
                label: c.label.to_string(),
                description: None,
                value: c.key.to_string(),
            })
            .collect();
        let mut rows: Vec<_> = select_row(CATEGORY_ID, "اختر القسم…", choices, enabled)
            .into_iter()
            .collect();
        rows.push(buttons(vec![close_button(enabled)]));
        Page::embed(embed, rows)
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        match id {
            CLOSE => Outcome::Close,
            CATEGORY_ID => match values.first().and_then(|v| find(v)) {
                Some(c) => {
                    self.current = c;
                    Outcome::Update
                }
                None => Outcome::Keep,
            },
            _ => Outcome::Keep,
        }
    }
}

async fn ac_category(_ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let needle = partial.trim().to_lowercase();
    to_choices(
        CATEGORIES
            .iter()
            .filter(|c| needle.is_empty() || c.label.contains(&needle) || c.key.contains(&needle))
            .map(|c| (c.label.to_string(), c.key.to_string()))
            .collect(),
    )
}

/// Commands and examples, grouped by category.
#[poise::command(
    slash_command,
    name_localized("ar", "مساعدة"),
    description_localized("ar", "الأوامر وأمثلتها حسب القسم")
)]
pub async fn help(
    ctx: Context<'_>,
    #[description = "Category"]
    #[description_localized("ar", "القسم")]
    #[name_localized("ar", "القسم")]
    #[autocomplete = "ac_category"]
    category: Option<String>,
) -> Result<(), Error> {
    let current = category
        .as_deref()
        .and_then(|c| find(c.trim()))
        .unwrap_or(&CATEGORIES[0]);
    session::run(ctx, HelpView { current }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_covers_every_command_and_fits_discord_limits() {
        let all: String = CATEGORIES.iter().map(|c| c.body).collect();
        for cmd in [
            "/search",
            "/character",
            "/airing",
            "/random",
            "/top",
            "/profile",
            "/edit",
            "/settings",
            "/admin",
            "/set-bot-channel",
            "/set-notifications-channel",
            "/game",
            "/recommend",
            "/suggest",
            "/help",
        ] {
            if cmd == "/help" {
                continue;
            }
            assert!(all.contains(cmd), "{cmd} is not documented in /help");
        }
        for c in CATEGORIES {
            assert!(
                c.body.chars().count() <= 4000,
                "{} is too long for an embed",
                c.key
            );
            assert!(find(c.key).is_some());
        }
        assert!(CATEGORIES.len() <= 25);
        assert!(find("nope").is_none());
    }
}
