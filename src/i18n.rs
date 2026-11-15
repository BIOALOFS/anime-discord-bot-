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

// ---- profiles ----
pub const PROFILE_TITLE: &str = "بروفايل";
pub const PROFILE_NONE_SELF: &str = "ليس لديك بروفايل بعد، أنشئه بالأمر `/profile create`.";
pub const PROFILE_NONE_OTHER: &str = "هذا المستخدم ليس لديه بروفايل.";
pub const PROFILE_PRIVATE: &str = "هذا البروفايل غير متاح لك.";
pub const PROFILE_CREATED: &str = "تم إنشاء بروفايلك 🎉 أضف شخصياتك بالأمر `/profile add`.";
pub const PROFILE_EXISTS: &str = "لديك بروفايل بالفعل.";
pub const PROFILE_EMPTY: &str = "لا توجد شخصيات في هذا البروفايل بعد.";
pub const PROFILE_NOT_IN_LIST: &str = "هذه الشخصية ليست في بروفايلك. اختر من القائمة المقترحة.";
pub const PROFILE_AMBIGUOUS: &str = "وجدت أكثر من شخصية بهذا الاسم، اختر واحدة من القائمة المقترحة.";
pub const ALREADY_ADDED: &str = "هذه الشخصية موجودة بالفعل في بروفايلك.";
pub const CHARACTER_HIDDEN: &str = "لا يمكن إضافة هذه الشخصية (محتوى غير مناسب).";
pub fn added_character(name: &str, pos: i64) -> String {
    format!("تمت إضافة **{name}** في المركز {pos} ✅")
}
pub fn removed_character(name: &str) -> String {
    format!("تمت إزالة **{name}** من بروفايلك.")
}
pub fn moved_character(name: &str, pos: i64) -> String {
    format!("تم نقل **{name}** إلى المركز {pos}.")
}
pub const CLEAR_CONFIRM: &str = "⚠️ سيتم حذف **كل** الشخصيات من بروفايلك. هل أنت متأكد؟";
pub const BTN_CONFIRM: &str = "تأكيد الحذف";
pub const BTN_CANCEL: &str = "إلغاء";
pub fn cleared(n: u64) -> String {
    format!("تم حذف {n} شخصية من بروفايلك.")
}
pub const CANCELLED: &str = "تم الإلغاء.";
pub const BIO_SET: &str = "تم تحديث النبذة.";
pub const COLOR_SET: &str = "تم تحديث اللون.";
pub const COLOR_INVALID: &str = "لون غير صالح، استخدم صيغة مثل `#3DB4F2`.";
pub const BANNER_SET: &str = "تم تحديث الغلاف.";
pub const BANNER_INVALID: &str =
    "رابط الصورة غير مقبول: استخدم رابط https من AniList أو Discord أو Imgur (png/jpg/webp/gif).";
pub const STYLE_SET: &str = "تم حفظ نمط العرض الافتراضي.";
pub const PRIVACY_SET: &str = "تم تحديث الخصوصية.";
pub const NOTE_SET: &str = "تم حفظ الملاحظة (خاصة بك فقط).";
pub const NOTE_CLEARED: &str = "تم حذف الملاحظة.";
pub fn tier_set(name: &str, tier: &str) -> String {
    format!("تم وضع **{name}** في الفئة **{tier}**.")
}
pub fn tier_cleared(name: &str) -> String {
    format!("تمت إزالة فئة **{name}**.")
}
pub const GROUP_CREATED: &str = "تم إنشاء المجموعة.";
pub const GROUP_EXISTS: &str = "توجد مجموعة بهذا الاسم بالفعل.";
pub const GROUP_NOT_FOUND: &str = "لم أجد هذه المجموعة.";
pub const GROUP_RENAMED: &str = "تم تغيير اسم المجموعة.";
pub const GROUP_DELETED: &str = "تم حذف المجموعة (الشخصيات بقيت دون مجموعة).";
pub const GROUP_ASSIGNED: &str = "تم تحديث مجموعة الشخصية.";
pub const GROUP_NAME_INVALID: &str = "اسم المجموعة غير صالح.";
pub const SELECT_STYLE_PLACEHOLDER: &str = "🔀 تغيير النمط";
pub const BTN_ANOTHER: &str = "اختر أخرى";
pub const EXPORT_DONE: &str = "هذا ملف بروفايلك. يمكنك استيراده لاحقًا بالأمر `/profile import`.";
pub const IMPORT_BAD_FILE: &str =
    "ملف غير صالح: أرسل ملف JSON مُصدَّر من `/profile export` (حتى 2 ميغابايت).";
pub fn import_report(added: usize, dups: usize, invalid: usize) -> String {
    format!(
        "تم الاستيراد: أُضيفت **{added}**، مكررة **{dups}**، غير صالحة **{invalid}**.\nتظهر الشخصيات بعد التحقق من سلامتها عند عرض البروفايل."
    )
}
pub const BULK_EMPTY: &str = "لم تكتب أي شخصية.";
pub fn bulk_report(
    added: &[String],
    not_found: &[String],
    skipped: &[String],
    over_limit: usize,
) -> String {
    let list = |v: &[String]| v.iter().take(15).cloned().collect::<Vec<_>>().join("، ");
    let mut out = format!(
        "**أُضيفت ({})**: {}",
        added.len(),
        if added.is_empty() {
            "—".into()
        } else {
            list(added)
        }
    );
    out.push_str(&format!(
        "\n**لم أجدها ({})**: {}",
        not_found.len(),
        if not_found.is_empty() {
            "—".into()
        } else {
            list(not_found)
        }
    ));
    out.push_str(&format!(
        "\n**تم تخطيها ({})**: {}",
        skipped.len(),
        if skipped.is_empty() {
            "—".into()
        } else {
            list(skipped)
        }
    ));
    if over_limit > 0 {
        out.push_str(&format!(
            "\n⏳ تجاوزت الحد في المرة الواحدة؛ لم تُعالج {over_limit} سطرًا، أعد إرسالها."
        ));
    }
    out
}
pub const REORDER_TITLE: &str = "ترتيب الشخصيات";
pub const REORDER_HINT: &str = "اختر شخصية من القائمة ثم استخدم الأزرار لتحريكها.";
pub const REORDER_SELECT: &str = "اختر شخصية…";
pub const COMPARE_TITLE: &str = "مقارنة البروفايلات";
pub fn compat(pct: u32) -> String {
    format!("نسبة التوافق: **{pct}%**")
}
pub const COMPARE_NONE_SHARED: &str = "لا توجد شخصيات مشتركة.";
pub const TOP_TITLE: &str = "أكثر الشخصيات تفضيلًا";
pub const SCOPE_SERVER: &str = "السيرفر";
pub const SCOPE_GLOBAL: &str = "عالميًا";
pub fn profile_stats(count: i64, privacy: &str) -> String {
    format!("{count} شخصية · الخصوصية: {privacy}")
}
pub fn privacy_ar(p: &str) -> &'static str {
    match p {
        "public" => "عام",
        "servers" => "السيرفرات المشتركة",
        _ => "خاص",
    }
}
pub fn position_of(pos: i64, total: i64) -> String {
    format!("{pos} / {total}")
}
pub const NOTE_LABEL: &str = "ملاحظتك";
pub const NICKNAMES: &str = "الأسماء المستعارة";
pub const STYLE_COLLAGE: &str = "صور";
pub const STYLE_NAMES: &str = "أسماء";
pub const STYLE_CARDS: &str = "بطاقات";
pub const STYLE_GALLERY: &str = "معرض";
pub const STYLE_PODIUM: &str = "منصة التتويج";
pub const STYLE_COMPACT: &str = "مختصر";
pub const STYLE_TIER: &str = "جدول التصنيفات";
pub const STYLE_RANDOM: &str = "عشوائي";
pub fn more_n(n: i64) -> String {
    format!("+{n} أخرى")
}
pub const TIER_NONE: &str = "لم تضع أي شخصية في فئة بعد، استخدم `/profile rank` أولًا.";
pub const SHARE_TITLE: &str = "صورة بروفايلك جاهزة للمشاركة 📸";
