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
               • في قوائم «شخصيات العمل» و«أعمال الشخصية» اختر عنصرًا من القائمة المنسدلة لفتحه في نفس الرسالة.\n\n\
               **`/airing`** موعد الحلقة القادمة لأنمي، أو جدول هذا الأسبوع.\n\
               **`/random`** اختيار عشوائي: عمل أو شخصية أو `شخصية اليوم` أو `أعياد ميلاد اليوم`.\n\
               مثال: `/random type:أنمي genre:Action min_score:75`\n\
               **`/top`** أكثر الشخصيات أو الأعمال تفضيلًا في السيرفر أو عالميًا (مع قائمة منسدلة لفتح بطاقة أي شخصية).",
    },
    HelpCategory {
        key: "profile",
        label: "بروفايل",
        body: "**`/profile`** بدون خيارات يعرض بروفايلك (مع `user` يعرض بروفايل عضو): غلاف في الأعلى ثم بطاقة لكل عنصر، أزرار ⏮ ◀ ▶ ⏭ وقائمة منسدلة للقفز لأي عنصر (25 في كل مرة).\n\
               • `action:add-character` أو `add-work` مع `value` لإضافة شخصية أو عمل.\n\
               مثال: `/profile action:add-work value:Tower of God`\n\
               كل التعديلات من **`/edit`** (انظر قسم «تعديل»).",
    },
    HelpCategory {
        key: "edit",
        label: "تعديل",
        body: "**`/edit`** كل شيء يُعدَّل من هنا: `target` ثم `action` (ثم `character` أو `value` أو `text` أو `image`).\n\
               • **profile**: `banner` الغلاف (أرفق صورة PNG/JPG/WebP حتى 3MB، أو `value` تدرج مثل sunset أو لون #ff8800 أو reset)، \
               `description` النبذة (300 حرف)، `title` الاسم المعروض (40)، `color` لون البطاقة، `privacy` الخصوصية، `reset` إعادة الضبط، \
               `export` / `import` / `delete-data` لبياناتك.\n\
               • **character**: اختر `character` من بروفايلك ثم `image` (PNG/JPG/WebP/GIF حتى 3MB، وGIF لشخصيات بروفايلك فقط)، `title` اللقب (40)، \
               `description` الوصف (300)، `reset` إعادة للأصل، `remove` إزالة، `move` نقل الموضع (النص = الرقم).\n\
               • **work**: `remove` أو `move` لعمل من `value`.\n\
               كل تأكيد خاص بك، وفيه زر «تراجع» عند الحاجة. الصور المخصصة تعمل فقط إذا فعّلها مشرف السيرفر.",
    },
    HelpCategory {
        key: "alerts",
        label: "إشعارات",
        body: "إشعارات الحلقات الجديدة تغطي **كل الأنمي تلقائيًا** عند تحديد قناة الإشعارات بـ **`/set-notifications-channel`**.\n\
               المشرف يضبط التصفية من **`/admin`** ← الإشعارات: الكل (الافتراضي)، الشائع فقط، أعمال بروفايلات الأعضاء فقط، أو صيغ وأنواع محددة.\n\
               عند كثرة الإصدارات تُجمَّع في رسائل قليلة بدل الإغراق.\n\
               ملاحظة: بيانات الجدول متاحة للأنمي فقط في AniList، وليست للمانجا أو المانهوا.",
    },
    HelpCategory {
        key: "settings",
        label: "إعدادات",
        body: "**`/settings`** تفضيلاتك الشخصية فقط: `setting:language` اللغة و`setting:privacy` الخصوصية.\n\
               **إدارة السيرفر (للمشرفين، مخفية عن غيرهم):**\n\
               • **`/admin`** لوحة أزرار: الأمان، الإشعارات، الصور المخصصة، الرتب، القنوات، الحدود، إعادة الضبط.\n\
               • **`/set-bot-channel`** القناة الوحيدة التي تعمل فيها أوامر البوت (المشرفون مستثنون).\n\
               • **`/set-notifications-channel`** قناة إشعارات الحلقات.\n\
               إلغاء تحديد أي قناة من `/admin` ← القنوات.",
    },
    HelpCategory {
        key: "games",
        label: "ألعاب وتوصيات",
        body: "**`/game`** `guess-character` خمّن الشخصية من صورة مقصوصة، `guess-anime` خمّن الأنمي من وصف مقنّع، \
               `versus` أيهما تفضل، `leaderboard` الترتيب.\n\
               **`/recommend`** توصيات مبنية على مفضلاتك. مع `user` تحصل على توصيات مشتركة بينكما.",
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
