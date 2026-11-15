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
pub const BTN_CANCEL: &str = "إلغاء";
pub const CANCELLED: &str = "تم الإلغاء.";
pub const BIO_SET: &str = "تم تحديث النبذة.";
pub const COLOR_SET: &str = "تم تحديث اللون.";
pub const COLOR_INVALID: &str = "لون غير صالح، استخدم صيغة مثل `#3DB4F2`.";
pub const BANNER_SET: &str = "تم تحديث الغلاف.";
pub const PRIVACY_SET: &str = "تم تحديث الخصوصية.";
pub const EXPORT_DONE: &str = "هذا ملف بروفايلك. يمكنك استيراده لاحقًا بالأمر `/profile import`.";
pub const IMPORT_BAD_FILE: &str =
    "ملف غير صالح: أرسل ملف JSON مُصدَّر من `/profile export` (حتى 2 ميغابايت).";
pub fn import_report(added: usize, dups: usize, invalid: usize) -> String {
    format!(
        "تم الاستيراد: أُضيفت **{added}**، مكررة **{dups}**، غير صالحة **{invalid}**.\nتظهر الشخصيات بعد التحقق من سلامتها عند عرض البروفايل."
    )
}
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
pub const BADGE_COLLECTOR: &str = "🏅 جامع";
pub const BADGE_GAMER: &str = "🎮 لاعب";
pub const BADGE_WATCHER: &str = "📺 متابع وفيّ";
pub const SELECT_ITEM_PLACEHOLDER: &str = "افتح عنصرًا…";
pub const BTN_SECTION_WORKS: &str = "🎬 الأعمال";
pub const BTN_SECTION_CHARACTERS: &str = "👤 الشخصيات";
pub const BTN_REPORT: &str = "إبلاغ";
pub const REPORT_SENT: &str = "🚩 تم إرسال البلاغ للمشرفين.";
pub const REPORT_ALREADY: &str = "أبلغتَ عن هذه الصورة من قبل.";
// ---- works / customization / lists ----
pub const WORK_AMBIGUOUS: &str = "وجدت عدة أعمال، اختر واحدًا من الاقتراحات المنسدلة:";
pub const WORK_ALREADY: &str = "هذا العمل موجود بالفعل في بروفايلك.";
pub const WORK_NOT_IN_LIST: &str = "هذا العمل ليس في بروفايلك. اختر من القائمة المقترحة.";
pub const TEXT_REJECTED: &str = "النص غير مقبول: لا روابط ولا إشارات ولا دعوات ولا كلمات غير لائقة.";
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
pub const PRIVACY_UNKNOWN: &str = "قيمة غير معروفة. اختر من الاقتراحات.";
pub const NO_BIRTHDAYS: &str = "لا توجد شخصيات مشهورة لها عيد ميلاد اليوم.";
pub const TOP_CHARACTERS: &str = "أكثر الشخصيات تفضيلًا";
pub const TOP_WORKS: &str = "أكثر الأعمال تفضيلًا";

// ---- /settings ----
pub const SETTINGS_SAVED: &str = "تم الحفظ ✅";
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
pub const GAMES_SERVER_OFF: &str = "الألعاب معطلة في هذا السيرفر. يمكن للمشرف تفعيلها من /settings.";
pub const GAMES_UNAVAILABLE: &str = "مصادر الألعاب غير مفعّلة على هذا البوت.";
pub const CHARACTER_FROM: &str = "من";
pub const F_TYPE: &str = "النوع";
pub const FEED_BATCH_TITLE: &str = "🆕 إصدارات جديدة";
pub fn cooldown_wait(secs: u64) -> String {
    format!("⏳ انتظر {secs} ثانية قبل استخدام أمر آخر.")
}
pub const ADMIN_CONFIRM_RESET: &str = "⚠️ هل أنت متأكد؟ اضغط «نعم، احذف كل شيء» للتأكيد.";
pub const SERVER_ONLY: &str = "هذا الأمر يعمل داخل السيرفرات فقط.";
pub const NOT_ADMIN: &str = "هذا الأمر للمشرفين فقط (صلاحية إدارة السيرفر).";
pub const CHANNEL_NOT_TEXT: &str = "اختر قناة نصية من هذا السيرفر.";
pub fn channel_missing_permissions(channel: u64, missing: &[&str]) -> String {
    format!(
        "❌ البوت يفتقد الصلاحيات التالية في <#{channel}>: {}.",
        missing.join("، ")
    )
}
pub fn notifications_channel_set(channel: u64) -> String {
    format!("✅ ستُنشر إشعارات الحلقات الجديدة في <#{channel}>.")
}
pub fn bot_channel_set(channel: u64) -> String {
    format!("✅ أوامر البوت تعمل الآن في <#{channel}> فقط (المشرفون مستثنون). لإلغاء القيد استخدم `/admin` ← القنوات.")
}
pub const LANGUAGE_SET: &str = "✅ تم حفظ اللغة: العربية.";
pub const LANGUAGE_SET_EN: &str = "✅ Language saved. The bot's texts are currently Arabic only.";
pub const LANGUAGE_UNKNOWN: &str = "اللغة غير معروفة. اختر من القائمة.";
pub const SETTINGS_UNKNOWN: &str = "إعداد غير معروف. اختر اللغة أو الخصوصية.";
pub const EDIT_HINT: &str = "للتعديل: /edit";
pub const UPLOAD_ADMINS_ONLY: &str = "رفع الصور المخصصة في هذا السيرفر للمشرفين فقط.";
pub const BANNER_PRESET_SET: &str = "✅ تم تغيير الغلاف.";
pub const BANNER_RESET: &str = "✅ عاد الغلاف إلى الافتراضي.";
pub const BANNER_UNKNOWN: &str =
    "اختر تدرجًا من القائمة، أو لونًا مثل #ff8800، أو reset، أو أرفق صورة.";
pub const PROFILE_RESET_DONE: &str = "✅ أُعيد ضبط البروفايل (الشخصيات والأعمال محفوظة).";
pub const TITLE_SET: &str = "✅ تم تغيير الاسم المعروض.";
pub const BIO_TOO_LONG: &str = "النبذة أطول من 300 حرف.";
pub const TITLE_TOO_LONG: &str = "الاسم أطول من 40 حرفًا.";
pub const EDIT_NEEDS_CHARACTER: &str = "اختر شخصية من بروفايلك في خيار «الشخصية».";
pub const EDIT_NEEDS_WORK: &str = "اختر عملًا من أعمالك في خيار «القيمة».";
pub const CAST_PLACEHOLDER: &str = "اختر شخصية لعرض بطاقتها…";
pub const PAGE_WORD: &str = "الصفحة";
pub const PAGES_PREV_GROUP: &str = "◀ مجموعة الصفحات السابقة";
pub const PAGES_NEXT_GROUP: &str = "مجموعة الصفحات التالية ▶";
pub const SELECT_PAGE_PLACEHOLDER: &str = "انتقل إلى صفحة…";
pub const F_PROFILE_CHARACTERS: &str = "الشخصيات";
pub const F_PROFILE_WORKS: &str = "الأعمال";
pub const F_PROFILE_PRIVACY: &str = "الخصوصية";
pub const F_PROFILE_JOINED: &str = "انضم";
pub const F_PROFILE_SUGGESTIONS: &str = "الاقتراحات";
pub const F_PROFILE_BADGES: &str = "الأوسمة";
pub const BANNER_PICKER_TITLE: &str = "🎨 اختر غلافًا جاهزًا";
pub const BANNER_PICKER_HELP: &str = "اختر من القائمة لترى المعاينة ثم اضغط «تطبيق».\nلرفع صورتك أو GIF: `/edit` ← البروفايل ← الغلاف وأرفق الملف (إن سمح السيرفر بالصور المخصصة). للون: اكتب `#ff8800` في القيمة.";
pub const BANNER_PICKER_PLACEHOLDER: &str = "الأغلفة الجاهزة…";
pub const BANNER_APPLY: &str = "تطبيق";
pub const BANNER_RESET_BTN: &str = "إعادة للافتراضي";
pub const BTN_REMOVE_FROM_PROFILE: &str = "إزالة من بروفايلي";
pub const CHARACTER_REMOVED: &str = "تمت إزالة الشخصية من بروفايلك.";
pub const SUGGESTED_BY: &str = "اقترحه";
pub const SUGGEST_ADDED: &str = "✅ تمت إضافة الاقتراح";
pub const SUGGEST_REASON_LONG: &str = "السبب أطول من 300 حرف.";
pub const SUGGEST_REASON_BAD: &str =
    "اكتب سببًا نصيًا (الروابط والإشارات والكلمات غير المناسبة تُحذف).";
pub const SUGGEST_SAME: &str = "اختر عملين مختلفين.";
pub const SUGGEST_NONE: &str =
    "لا توجد اقتراحات في هذا السيرفر بعد. أضف أول اقتراح بـ `/suggest add`.";
pub const SUGGEST_NONE_MINE: &str = "ليس لديك اقتراحات في هذا السيرفر.";
pub const SUGGEST_ANOTHER: &str = "اقتراح آخر";
pub const SUGGEST_DELETED: &str = "🗑 تم حذف الاقتراح.";
pub const SUGGEST_CANNOT_DELETE: &str = "لا يمكنك حذف هذا الاقتراح.";
pub const SUGGEST_MINE_TITLE: &str = "اقتراحاتي";
pub const SUGGEST_DELETE_PLACEHOLDER: &str = "اختر اقتراحًا لحذفه…";
pub const BTN_DELETE: &str = "حذف";
pub fn suggest_exists(author: i64) -> String {
    format!("هذا الاقتراح موجود بالفعل، أضافه <@{author}>.")
}
pub fn suggest_limit(limit: i64) -> String {
    format!("وصلت إلى الحد اليومي ({limit} اقتراحات في اليوم). حاول غدًا.")
}
pub const REMINDER_DM: &str = "🔔 تذكير: صدرت حلقة جديدة من عمل طلبتَ التذكير به.";
pub const BTN_REMIND: &str = "ذكّرني";
pub const BTN_REMINDER_CANCEL: &str = "إلغاء التذكير";
pub const REMINDER_SET: &str = "🔔 سأذكّرك عند صدور حلقة جديدة (رسالة خاصة).";
pub const REMINDER_CANCELLED: &str = "🔕 أُلغي التذكير.";
pub const REMINDER_FULL: &str = "وصلت إلى الحد الأقصى للتذكيرات (50). احذف بعضها من /settings.";
pub const REMINDERS_TITLE: &str = "تذكيراتي";
pub const REMINDERS_NONE: &str = "ليس لديك تذكيرات. استخدم زر «ذكّرني» على بطاقة أنمي.";
pub const REMINDERS_PLACEHOLDER: &str = "اختر تذكيرًا لإلغائه…";
