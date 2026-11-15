//! Every user-facing string lives here so another language can be added later.

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
pub const BTN_FAVOURITE: &str = "إضافة للمفضلة";
pub const BTN_ANILIST: &str = "AniList";
pub const BTN_CLOSE: &str = "إغلاق";
pub const BTN_BACK: &str = "رجوع";
pub const BTN_WORKS: &str = "الأعمال";
pub const BTN_ADD_TO_PROFILE: &str = "أضف لبروفايلي";
pub const BTN_RANDOM_CHARACTER: &str = "شخصية عشوائية";
pub const BTN_REROLL: &str = "مرة أخرى";

// ---- media card ----
pub const F_SCORE: &str = "التقييم";
pub const F_EPISODES: &str = "الحلقات";
pub const F_CHAPTERS: &str = "الفصول";
pub const F_STATUS: &str = "الحالة";

// ---- lists ----
pub const TITLE_CHARACTERS: &str = "شخصيات";
pub const EMPTY_LIST: &str = "لا توجد عناصر لعرضها.";
pub const ROLE_MAIN: &str = "رئيسية";
pub const ROLE_SUPPORTING: &str = "داعمة";
pub const ROLE_BACKGROUND: &str = "ثانوية";
pub fn episodes_n(n: i32) -> String {
    format!("{n} حلقة")
}

// ---- character card ----
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

// ---- random ----
pub const RANDOM_FAILED: &str = "لم أجد نتيجة مناسبة بعد عدة محاولات، جرّب مرة أخرى.";

// ---- profiles ----
pub const PROFILE_TITLE: &str = "بروفايل";
pub const PROFILE_NONE_SELF: &str = "ليس لديك بروفايل بعد، أنشئه بالأمر `/profile create`.";
pub const PROFILE_NONE_OTHER: &str = "هذا المستخدم ليس لديه بروفايل.";
pub const PROFILE_PRIVATE: &str = "هذا البروفايل غير متاح لك.";
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
pub const SCOPE_SERVER: &str = "السيرفر";
pub const SCOPE_GLOBAL: &str = "عالميًا";
pub fn profile_stats(characters: i64, works: i64, privacy: &str) -> String {
    format!("{characters} شخصية · {works} عمل · الخصوصية: {privacy}")
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
pub const BTN_DESCRIPTION: &str = "الوصف";
pub const NO_DESCRIPTION: &str = "لا يوجد وصف متاح لهذه الشخصية.";

// ---- work roster ----
pub const BTN_ROSTER: &str = "شخصيات العمل";
pub const BTN_BACK_TO_LIST: &str = "رجوع للقائمة";
pub const ROSTER_TITLE: &str = "شخصيات";
pub const ROSTER_PICK_WORK: &str = "هذه الشخصية تظهر في عدة أعمال، اختر العمل:";
pub const ROSTER_WORK_PLACEHOLDER: &str = "اختر العمل…";
pub const ROSTER_RANGE_PLACEHOLDER: &str = "انتقل إلى نطاق…";
pub const ROSTER_GROUP_PLACEHOLDER: &str = "اختر مجموعة النطاقات…";
pub const ROSTER_CHAR_PLACEHOLDER: &str = "افتح شخصية…";
pub const ROSTER_NO_WORKS: &str = "لا توجد أعمال معروفة لهذه الشخصية.";
pub const ROSTER_JUMP_TITLE: &str = "الانتقال إلى صفحة";
pub const ROSTER_BAD_PAGE: &str = "رقم صفحة غير صالح.";
pub fn roster_jump_label(pages: usize) -> String {
    format!("رقم الصفحة (1–{pages})")
}
pub fn roster_footer(page: usize, pages: usize, total: usize) -> String {
    format!("صفحة {page} / {pages} · {total} شخصية · {POWERED_BY}")
}

// ---- /setup (command channel lock) ----
pub fn wrong_channel(channel: i64) -> String {
    format!("استخدم البوت في القناة <#{channel}> فقط.")
}
pub fn setup_channel_set(channel: i64) -> String {
    format!("تم! البوت سيرد على الأوامر في <#{channel}> فقط (المشرفون يستطيعون تجاوز القيد حسب إعداد `admin-bypass`).")
}
pub fn setup_extra_added(channel: i64) -> String {
    format!("تمت إضافة <#{channel}> إلى القنوات المسموحة.")
}
pub const SETUP_EXTRA_EXISTS: &str = "هذه القناة مسموحة بالفعل.";
pub fn setup_extra_full(max: usize) -> String {
    format!("وصلت إلى الحد الأقصى ({max} قنوات إضافية). احذف قناة أولًا.")
}
pub fn setup_extra_removed(channel: i64) -> String {
    format!("تمت إزالة <#{channel}> من القنوات المسموحة.")
}
pub const SETUP_EXTRA_MISSING: &str = "هذه القناة ليست في قائمة القنوات الإضافية.";
pub const SETUP_RESET: &str = "تم إلغاء القيد: البوت يعمل الآن في كل القنوات.";
pub fn setup_bypass(on: bool) -> &'static str {
    if on {
        "تجاوز المشرفين للقيد: مفعّل."
    } else {
        "تجاوز المشرفين للقيد: معطّل."
    }
}
pub const SETUP_TITLE: &str = "إعدادات قناة الأوامر";
pub const SETUP_NONE: &str = "لا توجد قناة محددة، البوت يعمل في كل القنوات.";
pub const SETUP_MAIN: &str = "قناة الأوامر";
pub const SETUP_EXTRA: &str = "قنوات إضافية";
pub const SETUP_BYPASS: &str = "تجاوز المشرفين";
pub const SETUP_ON: &str = "مفعّل";
pub const SETUP_OFF: &str = "معطّل";
pub const SETUP_DELETED_WARNING: &str =
    "⚠️ القناة المحددة محذوفة، لذلك القيد معطّل حاليًا. اختر قناة جديدة بالأمر `/setup channel`.";
pub const GUILD_ONLY: &str = "هذا الأمر يعمل داخل السيرفرات فقط.";

// ---- work card / kinds ----
pub const KIND_ANIME: &str = "أنمي";
pub const KIND_MANGA: &str = "مانجا";
pub const KIND_MANHWA: &str = "مانهوا";
pub const KIND_MANHUA: &str = "مانهوا صينية";
pub const KIND_NOVEL: &str = "رواية";
pub const UNKNOWN: &str = "غير معروف";
pub fn chapters_n(n: i32) -> String {
    format!("{n} فصل")
}
pub fn volumes_n(n: i32) -> String {
    format!("{n} مجلد")
}
pub const BTN_UNDO: &str = "تراجع";
pub const WORK_ADDED: &str = "✅ تمت الإضافة إلى بروفايلك";
pub const WORK_REMOVED: &str = "تمت الإزالة من بروفايلك";
pub const UNDONE: &str = "تم التراجع.";
pub const BTN_ADDED: &str = "مضاف";
pub const BTN_ALERTS: &str = "تنبيهات";
pub const ALERT_ADDED: &str = "🔔 أُضيف إلى قائمة التنبيهات في السيرفر.";
pub const ALERT_REQUESTED: &str = "🔔 أُرسل طلبك إلى المشرفين.";
pub const ALERT_ALREADY_REQUESTED: &str = "طلبك مسجّل بالفعل وينتظر موافقة المشرفين.";
pub const BADGE_COLLECTOR: &str = "🏅 جامع";
pub const BADGE_GAMER: &str = "🎮 لاعب";
pub const BADGE_WATCHER: &str = "📺 متابع وفيّ";
pub const SELECT_ITEM_PLACEHOLDER: &str = "افتح عنصرًا…";
pub const BTN_SECTION_WORKS: &str = "🎬 الأعمال";
pub const BTN_SECTION_CHARACTERS: &str = "👤 الشخصيات";
pub const BTN_REPORT: &str = "إبلاغ";
pub const REPORT_SENT: &str = "🚩 تم إرسال البلاغ للمشرفين.";
pub const REPORT_ALREADY: &str = "أبلغتَ عن هذه الصورة من قبل.";
pub const COMPARE_CHARACTERS: &str = "شخصيات مشتركة";
pub const COMPARE_WORKS: &str = "أعمال مشتركة";

// ---- works / customization / lists ----
pub const WORK_AMBIGUOUS: &str = "وجدت عدة أعمال، اختر واحدًا من الاقتراحات المنسدلة:";
pub const WORK_ALREADY: &str = "هذا العمل موجود بالفعل في بروفايلك.";
pub const WORK_NOT_IN_LIST: &str = "هذا العمل ليس في بروفايلك. اختر من القائمة المقترحة.";
pub const LIST_WATCHED: &str = "شاهدتها";
pub const LIST_PLANNING: &str = "أخطط لمشاهدتها";
pub fn list_updated(title: &str, list: Option<&str>) -> String {
    match list {
        Some("watched") => format!("تمت إضافة **{title}** إلى قائمة «{LIST_WATCHED}»."),
        Some(_) => format!("تمت إضافة **{title}** إلى قائمة «{LIST_PLANNING}»."),
        None => format!("تمت إزالة **{title}** من القوائم."),
    }
}
pub const QUOTE_SET: &str = "تم تحديث الاقتباس.";
pub const TEXT_REJECTED: &str = "النص غير مقبول: لا روابط ولا إشارات ولا دعوات ولا كلمات غير لائقة.";
pub const TEMPLATE_SET: &str = "تم تغيير قالب البروفايل.";
pub const TEMPLATE_UNKNOWN: &str =
    "قالب غير معروف. القوالب: classic, midnight, sakura, ocean, mono.";
pub fn custom_saved(name: &str) -> String {
    format!("تم حفظ التخصيص لـ **{name}** (يظهر في بروفايلك فقط).")
}
pub fn custom_reset(name: &str) -> String {
    format!("تمت إعادة **{name}** إلى الوضع الافتراضي.")
}
pub fn custom_image_saved(name: &str, animated: bool) -> String {
    if animated {
        format!("تم حفظ الصورة المتحركة لـ **{name}**. تظهر متحركة في بطاقة الشخصية وكإطار أول في الشبكة.")
    } else {
        format!("تم حفظ الصورة المخصصة لـ **{name}** (تظهر في بروفايلك فقط).")
    }
}
pub const CUSTOM_IMAGES_OFF: &str =
    "الصور المخصصة غير مفعّلة في هذا السيرفر. يمكن للمشرف تفعيلها من `/settings`.";
pub const UPLOAD_BANNED: &str = "تم منعك من رفع الصور في هذا السيرفر.";
pub const IMAGE_REQUIRED: &str = "أرفق صورة في خيار `image`.";
pub const IMAGE_TOO_LARGE: &str = "الصورة كبيرة جدًا (الحد 3 ميغابايت).";
pub const IMAGE_INVALID: &str = "ملف الصورة غير صالح. المسموح: PNG وJPG وWebP وGIF.";
pub const IMAGE_LIMIT: &str = "وصلت إلى الحد الأقصى للصور المخصصة (30). احذف بعضها أولًا.";
pub const IMAGE_DOWNLOAD_FAILED: &str = "تعذّر تحميل الصورة، حاول مرة أخرى.";
pub const UPLOAD_SLOW_DOWN: &str = "تمهّل قليلًا قبل رفع صورة جديدة.";
pub const BTN_DELETE_ALL: &str = "احذف كل بياناتي";
pub const DELETE_CONFIRM: &str = "⚠️ سيتم حذف **كل** بياناتك نهائيًا: البروفايل والأعمال والصور المخصصة ونتائج الألعاب. هل أنت متأكد؟";
pub const DATA_DELETED: &str = "تم حذف كل بياناتك.";
pub const PROFILE_PICK_ACTION: &str = "اختر الفئة ثم الإجراء من الاقتراحات المنسدلة.";
pub const PROFILE_NEEDS_VALUE: &str =
    "اختر القيمة من الاقتراحات في خيار `value` (شخصية أو عمل أو غيرها).";
pub const PROFILE_NEEDS_TEXT: &str = "اكتب النص في خيار `text`.";
pub const PROFILE_NEEDS_POSITION: &str = "اكتب رقم المركز في خيار `text` (مثل 1).";
pub const PROFILE_NEEDS_USER: &str = "اختر العضو في خيار `user`.";
pub const STYLE_UNKNOWN: &str = "نمط غير معروف. اختر من الاقتراحات.";
pub const PRIVACY_UNKNOWN: &str = "قيمة غير معروفة. اختر من الاقتراحات.";
pub const CLEAR_WORKS_CONFIRM: &str =
    "⚠️ سيتم حذف **كل** الأعمال المفضلة من بروفايلك. هل أنت متأكد؟";
pub const NO_BIRTHDAYS: &str = "لا توجد شخصيات مشهورة لها عيد ميلاد اليوم.";
pub const TOP_CHARACTERS: &str = "أكثر الشخصيات تفضيلًا";
pub const TOP_WORKS: &str = "أكثر الأعمال تفضيلًا";

// ---- /settings ----
pub const SETTINGS_PICK: &str = "اختر القسم ثم الخيار من الاقتراحات المنسدلة.";
pub const SETTINGS_NEEDS_VALUE: &str = "هذا الخيار يحتاج قيمة: اختر من الاقتراحات في خيار `value`.";
pub const SETTINGS_SAVED: &str = "تم الحفظ ✅";
pub const SETTINGS_BAD_CHANNEL: &str = "قناة غير صالحة: اختر من الاقتراحات أو اكتب منشن القناة.";
pub const SETTINGS_BAD_TOGGLE: &str = "اختر تشغيل أو إيقاف.";
pub const SETTINGS_BAD_NUMBER: &str = "اكتب رقمًا صحيحًا.";
pub const SETTINGS_BAD_FORMATS: &str =
    "صيغ غير صالحة. المسموح: TV, TV_SHORT, ONA, MOVIE (مفصولة بفواصل).";
pub const SETTINGS_BAD_ROLE: &str = "رتبة غير صالحة: اختر من الاقتراحات أو `-` للحذف.";
pub const SETTINGS_BAD_MODE: &str = "الوضع: all أو listed.";
pub const SETTINGS_BAD_USER: &str = "عضو غير صالح: اكتب منشن العضو أو رقمه.";
pub const SETTINGS_PICK_ANIME: &str = "اختر الأنمي من الاقتراحات في خيار `value`.";
pub fn feed_channel_set(channel: i64) -> String {
    format!("قناة التغذية: <#{channel}>. فعّلها بـ `/settings section:feed option:toggle value:on`.")
}
pub const FEED_NEEDS_CHANNEL: &str = "حدّد قناة التغذية أولًا (`option:channel`).";
pub const FEED_ON: &str = "تم تشغيل تغذية الحلقات 📺";
pub const FEED_OFF: &str = "تم إيقاف تغذية الحلقات.";
pub const FEED_TITLE: &str = "إعدادات تغذية الحلقات";
pub const FEED_STATE: &str = "الحالة";
pub const FEED_MIN_POP: &str = "أدنى شعبية";
pub const FEED_FORMATS: &str = "الصيغ";
pub const FEED_EXCLUDED: &str = "تصنيفات مستبعدة";
pub const FEED_DIGEST: &str = "الملخص الدوري";
pub const FEED_CAP: &str = "الحد اليومي";
pub const FEED_ROLE: &str = "رتبة الإشارة";
pub const FEED_MODE: &str = "الوضع";
pub const FEED_MODE_LISTED: &str = "المدرج في القائمة فقط";
pub const FEED_MODE_ALL: &str = "كل الأنمي مع المثبّت";
pub fn alert_list_updated(title: &str, mode: crate::db::feed::ListMode) -> String {
    format!("**{title}**: {}", list_mode_label(mode))
}
pub fn list_mode_label(mode: crate::db::feed::ListMode) -> &'static str {
    match mode {
        crate::db::feed::ListMode::Listed => "مُدرج",
        crate::db::feed::ListMode::Pinned => "مثبّت (ينشر دائمًا)",
        crate::db::feed::ListMode::Muted => "مكتوم",
    }
}
pub const ALERT_LIST_EMPTY: &str = "قائمة الإشعارات فارغة.";
pub const NO_REQUESTS: &str = "لا توجد طلبات معلّقة.";
pub const NO_REPORTS: &str = "لا توجد بلاغات مفتوحة.";
pub const CUSTOM_IMAGES_ENABLED: &str = "تم تفعيل الصور المخصصة في هذا السيرفر.";
pub const CUSTOM_IMAGES_DISABLED: &str = "تم تعطيل الصور المخصصة في هذا السيرفر.";
pub const IMAGE_REMOVED: &str = "تم حذف الصورة وإغلاق بلاغاتها.";
pub const UPLOAD_BAN_SET: &str = "تم منع العضو من رفع الصور.";
pub const UPLOAD_BAN_LIFTED: &str = "تم رفع المنع عن العضو.";
pub const GAMES_ON: &str = "تم تشغيل الألعاب 🎮";
pub const GAMES_OFF: &str = "تم إيقاف الألعاب.";
pub const NSFW_NOTE: &str = "تم الحفظ. ملاحظة: المحتوى يبقى آمنًا؛ هذا الخيار لا يعرض محتوى للبالغين إلا في قنوات NSFW عند دعمه لاحقًا.";
pub fn safety_status(nsfw: bool) -> String {
    format!(
        "الوضع الآمن: مفعّل دائمًا · وضع NSFW: {}",
        if nsfw {
            "مفعّل (غير فعّال حاليًا)"
        } else {
            "معطّل"
        }
    )
}

// ---- recommend ----
pub const REC_TITLE: &str = "توصيات لك";
pub const REC_TITLE_TOGETHER: &str = "توصيات مشتركة لكما";
pub const REC_PICK: &str = "اختر عملًا…";
pub const REC_NO_SEEDS: &str = "أضف بعض الأعمال أو الشخصيات المفضلة إلى بروفايلك أولًا لأقترح عليك.";
pub const REC_NONE: &str = "لا توجد توصيات جديدة الآن.";
pub const REC_HIDDEN: &str = "🔕 لن يظهر هذا العمل في توصياتك مرة أخرى.";
pub const BTN_NOT_FOR_ME: &str = "ليس لي";
pub fn rec_why(seed: &str) -> String {
    format!("لأنك تحب {seed}")
}
pub fn rec_why_together(seed: &str) -> String {
    format!("لأنكما تحبان {seed}")
}

// ---- games ----
pub const GUESS_CHARACTER_TITLE: &str = "من هذه الشخصية؟";
pub const GUESS_ANIME_TITLE: &str = "ما هذا الأنمي؟";
pub fn guess_hint(seconds: u64, level: u8, streak: i64) -> String {
    format!(
        "لديك {seconds} ثانية · المستوى {} · سلسلتك: {streak}",
        level + 1
    )
}
pub fn game_correct(streak: i64) -> String {
    format!("✅ إجابة صحيحة! السلسلة: {streak}")
}
pub const GAME_WRONG: &str = "❌ إجابة خاطئة (أو انتهى الوقت). هذه هي الإجابة:";
pub const GAME_ALREADY_ACTIVE: &str = "لديك لعبة نشطة في هذه القناة. أنهِها أولًا.";
pub const GAMES_DISABLED: &str = "الألعاب معطّلة في هذا السيرفر.";
pub const GAME_PICK_MODE: &str = "اختر نوع اللعبة من الاقتراحات.";
pub const VERSUS_PROMPT: &str = "أيهما تفضّل؟";
pub const VERSUS_RESULTS: &str = "نتائج التصويت حتى الآن:";
pub const LEADERBOARD_TITLE: &str = "ترتيب الألعاب";
pub const FEED_DIGEST_TITLE: &str = "ملخص الحلقات الجديدة 📺";
pub const BIRTHDAYS_TITLE: &str = "🎂 أعياد ميلاد اليوم";
pub const BTN_WATCH: &str = "مشاهدة";
pub const F_YEAR: &str = "السنة";
pub const WORKS_PICK_PLACEHOLDER: &str = "افتح عملًا…";

pub const KIND_GAME: &str = "لعبة";
pub const KIND_VN: &str = "رواية بصرية";
pub const MATURE_GAMES_ON: &str =
    "✅ سيتم عرض الألعاب ذات التصنيف الناضج (العنف والرعب فقط، المحتوى الجنسي مخفي دائمًا).";
pub const MATURE_GAMES_OFF: &str = "✅ سيتم إخفاء الألعاب المصنفة M / PEGI 18 / CERO Z.";
pub const GAMES_SERVER_OFF: &str = "الألعاب معطلة في هذا السيرفر. يمكن للمشرف تفعيلها من /settings.";
pub const GAMES_UNAVAILABLE: &str = "مصادر الألعاب غير مفعّلة على هذا البوت.";
pub const CHARACTER_FROM: &str = "من";
pub const F_TYPE: &str = "النوع";
