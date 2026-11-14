//! Every user-facing string lives here so another language can be added later.

pub const PING_REPLY: &str = "🏓 بونغ!";
pub const ERR_GENERIC: &str = "حدث خطأ غير متوقع، حاول مرة أخرى لاحقًا.";
pub const ERR_COOLDOWN: &str = "تمهّل قليلًا، حاول بعد لحظات.";
pub const ERR_MISSING_PERMS: &str = "ليست لديك الصلاحية لاستخدام هذا الأمر.";

// ---- generic ----
pub const ERR_BUSY: &str = "الخدمة مشغولة، حاول بعد قليل.";
pub const ERR_SERVICE: &str = "تعذّر الاتصال بـ AniList، حاول مرة أخرى لاحقًا.";
pub const NOT_YOURS: &str = "هذه الأزرار ليست لك.";
pub const NO_RESULTS: &str = "لم أجد نتائج مطابقة.";
pub const NOT_FOUND: &str = "لم أجد هذا العنصر.";
pub const HIDDEN_NOTICE: &str = "تم إخفاء عناصر غير مناسبة.";
pub const READ_MORE: &str = "اقرأ المزيد";
pub const POWERED_BY: &str = "Powered by AniList";
pub const PICK_PLACEHOLDER: &str = "اختر من النتائج…";
pub const PICK_PROMPT: &str = "وجدت عدة نتائج، اختر واحدًا:";
pub const NONE: &str = "—";

// ---- buttons ----
pub const BTN_CHARACTERS: &str = "الشخصيات";
pub const BTN_SIMILAR: &str = "المشابه";
pub const BTN_RELATED: &str = "الأعمال المرتبطة";
pub const BTN_FAVOURITE: &str = "إضافة للمفضلة";
pub const BTN_ANILIST: &str = "AniList";
pub const BTN_TRAILER: &str = "العرض الترويجي";
pub const BTN_CLOSE: &str = "إغلاق";
pub const BTN_BACK: &str = "رجوع";
pub const BTN_WORKS: &str = "الأعمال";
pub const BTN_ADD_TO_PROFILE: &str = "أضف لبروفايلي";
pub const BTN_RANDOM_CHARACTER: &str = "شخصية عشوائية";
pub const BTN_REROLL: &str = "مرة أخرى";

// ---- media card ----
pub const F_SCORE: &str = "التقييم";
pub const F_POPULARITY: &str = "الشعبية";
pub const F_EPISODES: &str = "الحلقات";
pub const F_CHAPTERS: &str = "الفصول";
pub const F_STATUS: &str = "الحالة";
pub const F_SEASON: &str = "الموسم";
pub const F_STUDIO: &str = "الاستوديو";
pub const F_AUTHOR: &str = "المؤلف";
pub const F_DURATION: &str = "المدة";
pub const F_SOURCE: &str = "المصدر";
pub const F_GENRES: &str = "التصنيفات";
pub fn minutes(n: i32) -> String {
    format!("{n} دقيقة")
}

// ---- lists ----
pub const TITLE_CHARACTERS: &str = "شخصيات";
pub const TITLE_SIMILAR: &str = "أعمال مشابهة لـ";
pub const TITLE_RELATED: &str = "أعمال مرتبطة بـ";
pub const EMPTY_LIST: &str = "لا توجد عناصر لعرضها.";
pub const ROLE_MAIN: &str = "رئيسية";
pub const ROLE_SUPPORTING: &str = "داعمة";
pub const ROLE_BACKGROUND: &str = "ثانوية";
pub fn episodes_n(n: i32) -> String {
    format!("{n} حلقة")
}

// ---- character card ----
pub const F_AGE: &str = "العمر";
pub const F_GENDER: &str = "الجنس";
pub const F_BIRTHDAY: &str = "تاريخ الميلاد";
pub const F_BLOOD: &str = "فصيلة الدم";
pub const F_APPEARANCES: &str = "أبرز الأعمال";
pub const F_ANILIST_FAVS: &str = "المفضلة على AniList";
pub const F_SERVER_FAVS: &str = "في مفضلة السيرفر";
pub fn server_favs(n: i64) -> String {
    format!("مفضلة عند {n} من أعضاء السيرفر")
}
pub const TITLE_WORKS: &str = "أعمال";

// ---- airing ----
pub const F_NEXT_EPISODE: &str = "الحلقة القادمة";
pub const F_PREV_EPISODE: &str = "الحلقة السابقة";
pub const F_TOTAL_EPISODES: &str = "إجمالي الحلقات";
pub const AIRING_FINISHED: &str = "انتهى عرض هذا الأنمي.";
pub const AIRING_UNSCHEDULED: &str = "لم يُحدَّد موعد الحلقة القادمة بعد.";
pub const AIRING_WEEK_TITLE: &str = "جدول عرض هذا الأسبوع";
pub const AIRING_DAY_PLACEHOLDER: &str = "اختر اليوم…";
pub const AIRING_EMPTY: &str = "لا توجد حلقات مجدولة لهذا الأسبوع.";
pub const AIRING_TZ_NOTE: &str = "الأوقات تظهر بتوقيتك المحلي، والأيام بحسب UTC.";
pub fn episode_n(n: i32) -> String {
    format!("حلقة {n}")
}

// ---- seasonal ----
pub const SEASONAL_PLACEHOLDER: &str = "افتح بطاقة عمل…";

// ---- random ----
pub const RANDOM_FAILED: &str = "لم أجد نتيجة مناسبة بعد عدة محاولات، جرّب مرة أخرى.";
pub fn seasonal_title(season_ar: &str, year: i32) -> String {
    format!("أنمي {season_ar} {year}")
}
