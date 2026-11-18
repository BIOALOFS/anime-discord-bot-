//! Every user-facing string lives here (`docs/TONE.md` explains how they are written), so the voice stays
//! consistent and another language can be added later. The rest of the code reaches this module as `i18n`.

pub const ERR_GENERIC: &str = "عذرًا، حدث خلل من جهتي. جرّب مرة أخرى بعد قليل.";
pub const ERR_COOLDOWN: &str = "على مهلك 😄 انتظر لحظات ثم أعد المحاولة.";
pub const ERR_MISSING_PERMS: &str = "هذا الأمر يحتاج صلاحية لا تملكها.";

// ---- generic ----
pub const ERR_BUSY: &str = "الخدمة مزدحمة الآن، أعد المحاولة بعد لحظات.";
pub const ERR_SERVICE: &str = "لم أستطع الوصول إلى AniList الآن. جرّب بعد قليل.";
pub const NOT_YOURS: &str = "هذه الرسالة لمن استخدم الأمر. شغّل الأمر بنفسك لتتحكم بها.";
pub const NO_RESULTS: &str = "لم أجد شيئًا بهذا الاسم. جرّب كتابة مختلفة أو الاسم بالإنجليزية.";
pub const NOT_FOUND: &str = "لم أعثر على هذا العنصر.";
pub const HIDDEN_NOTICE: &str = "أخفيتُ بعض النتائج لأنها غير مناسبة.";
pub const READ_MORE: &str = "اقرأ المزيد";
pub const POWERED_BY: &str = "Powered by AniList";
pub const PICK_PLACEHOLDER: &str = "اختر من النتائج…";
pub const PICK_PROMPT: &str = "وجدتُ أكثر من نتيجة، اختر الأقرب:";
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
pub const BTN_REROLL: &str = "عشوائي آخر";

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
pub const RANDOM_FAILED: &str = "لم أجد نتيجة مناسبة بعد عدة محاولات. جرّب مرة أخرى أو خفّف الفلاتر.";

// ---- profiles ----
pub const PROFILE_TITLE: &str = "بروفايل";
pub const PROFILE_NONE_OTHER: &str = "هذا العضو لم ينشئ بروفايلًا بعد.";
pub const PROFILE_PRIVATE: &str = "هذا البروفايل خاص، لا يمكنك رؤيته.";
pub const PROFILE_EMPTY: &str =
    "بروفايلك فارغ حتى الآن. ابحث عن شخصية تحبها وأضفها بزر «أضف لبروفايلي».";
pub const PROFILE_NOT_IN_LIST: &str = "هذه الشخصية ليست في بروفايلك. اختر من القائمة المقترحة.";
pub const PROFILE_AMBIGUOUS: &str = "وجدت أكثر من شخصية بهذا الاسم، اختر واحدة من القائمة المقترحة.";
pub const ALREADY_ADDED: &str = "هذه الشخصية في بروفايلك أصلًا 😉";
pub const CHARACTER_HIDDEN: &str = "لا أستطيع إضافة هذه الشخصية لأن محتواها غير مناسب.";
pub fn added_character(name: &str, pos: i64) -> String {
    format!("أضفتُ **{name}** إلى بروفايلك في المركز {pos} ✅")
}
pub fn removed_character(name: &str) -> String {
    format!("أزلتُ **{name}** من بروفايلك.")
}
pub fn moved_character(name: &str, pos: i64) -> String {
    format!("نقلتُ **{name}** إلى المركز {pos}.")
}
pub const BTN_CANCEL: &str = "إلغاء";
pub const CANCELLED: &str = "تمام، ألغيتُ العملية.";
pub const BIO_SET: &str = "حدّثتُ نبذتك.";
pub const COLOR_SET: &str = "حدّثتُ اللون.";
pub const COLOR_INVALID: &str = "لون غير صالح، استخدم صيغة مثل `#3DB4F2`.";
pub const BANNER_SET: &str = "غلافك الجديد جاهز 🎨";
pub const PRIVACY_SET: &str = "حدّثتُ خصوصية بروفايلك.";
pub const EXPORT_DONE: &str = "هذا ملف بروفايلك. يمكنك استيراده لاحقًا بالأمر `/profile import`.";
pub const IMPORT_BAD_FILE: &str =
    "ملف غير صالح: أرسل ملف JSON مُصدَّر من `/profile export` (حتى 2 ميغابايت).";
pub fn import_report(added: usize, dups: usize, invalid: usize) -> String {
    format!(
        "انتهى الاستيراد: أضفتُ **{added}**، وتخطيتُ **{dups}** مكررة و**{invalid}** غير صالحة.\nتظهر الشخصيات بعد أن أتحقق من سلامتها عند عرض البروفايل."
    )
}
pub const SCOPE_SERVER: &str = "السيرفر";
pub const SCOPE_GLOBAL: &str = "عالميًا";
pub fn privacy_ar(p: &str) -> &'static str {
    match p {
        "public" => "عام",
        "servers" => "السيرفرات المشتركة",
        _ => "خاص",
    }
}
pub const NOTE_LABEL: &str = "ملاحظتك";
pub const NICKNAMES: &str = "الأسماء المستعارة";
pub const BTN_DESCRIPTION: &str = "الوصف";
pub const NO_DESCRIPTION: &str = "لا يوجد وصف متاح لهذه الشخصية.";

// ---- work roster ----
pub const BTN_ROSTER: &str = "شخصيات العمل";
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
pub const WORK_ADDED: &str = "✅ أضفته إلى بروفايلك";
pub const WORK_REMOVED: &str = "أزلتُه من بروفايلك";
pub const UNDONE: &str = "تراجعتُ عن ذلك.";
pub const BTN_ADDED: &str = "مضاف";
pub const SELECT_ITEM_PLACEHOLDER: &str = "افتح عنصرًا…";
pub const BTN_SECTION_WORKS: &str = "أعمال";
pub const BTN_SECTION_CHARACTERS: &str = "شخصيات";
// ---- works / customization / lists ----
pub const WORK_AMBIGUOUS: &str = "وجدت عدة أعمال، اختر واحدًا من الاقتراحات المنسدلة:";
pub const WORK_ALREADY: &str = "هذا العمل في بروفايلك أصلًا 😉";
pub const WORK_NOT_IN_LIST: &str = "هذا العمل ليس في بروفايلك. اختر من الاقتراحات.";
pub const TEXT_REJECTED: &str = "النص غير مقبول: لا روابط ولا إشارات ولا دعوات ولا كلمات غير لائقة.";
pub fn custom_saved(name: &str) -> String {
    format!("حفظتُ تخصيصك لـ **{name}** (يظهر في بروفايلك فقط).")
}
pub fn custom_reset(name: &str) -> String {
    format!("أعدتُ **{name}** كما كانت في الأصل.")
}
pub fn custom_image_saved(name: &str, animated: bool) -> String {
    if animated {
        format!("حفظتُ الصورة المتحركة لـ **{name}**. تظهر متحركة في بطاقة الشخصية، وكإطار أول في الشبكة.")
    } else {
        format!("حفظتُ صورتك المخصصة لـ **{name}** (تظهر في بروفايلك فقط).")
    }
}
pub const CUSTOM_IMAGES_OFF: &str =
    "الصور المخصصة غير مفعّلة في هذا السيرفر. يمكن للمشرف تفعيلها من `/settings`.";
pub const UPLOAD_BANNED: &str =
    "مُنعتَ من رفع الصور في هذا السيرفر، تواصل مع المشرفين إن كان هذا خطأً.";
pub const IMAGE_REQUIRED: &str = "أرفق الصورة في خيار `image` لو سمحت.";
pub const IMAGE_TOO_LARGE: &str = "الصورة كبيرة جدًا (الحد 3 ميغابايت).";
pub const IMAGE_INVALID: &str = "ملف الصورة غير صالح. المسموح: PNG وJPG وWebP وGIF.";
pub const IMAGE_LIMIT: &str = "وصلت إلى الحد الأقصى للصور المخصصة (30). احذف بعضها أولًا.";
pub const IMAGE_DOWNLOAD_FAILED: &str = "لم أستطع تحميل الصورة. جرّب مرة أخرى.";
pub const UPLOAD_SLOW_DOWN: &str = "على مهلك، انتظر قليلًا قبل رفع صورة أخرى.";
pub const BTN_DELETE_ALL: &str = "احذف كل بياناتي";
pub const DELETE_CONFIRM: &str = "⚠️ سيتم حذف **كل** بياناتك نهائيًا: البروفايل والأعمال والصور المخصصة ونتائج الألعاب. هل أنت متأكد؟";
pub const DATA_DELETED: &str = "حذفتُ كل بياناتك.";
pub const PROFILE_PICK_ACTION: &str = "اختر الفئة ثم الإجراء من الاقتراحات المنسدلة.";
pub const PROFILE_NEEDS_VALUE: &str =
    "اختر القيمة من الاقتراحات في خيار `value` (شخصية أو عمل أو غيرها).";
pub const PROFILE_NEEDS_TEXT: &str = "اكتب النص في خيار `text`.";
pub const PROFILE_NEEDS_POSITION: &str = "اكتب رقم المركز في خيار `text` (مثل 1).";
pub const PRIVACY_UNKNOWN: &str = "قيمة غير معروفة. اختر من الاقتراحات.";
pub const NO_BIRTHDAYS: &str = "لا توجد شخصيات مشهورة يوافق عيد ميلادها اليوم.";
pub const TOP_CHARACTERS: &str = "أكثر الشخصيات تفضيلًا";
pub const TOP_WORKS: &str = "أكثر الأعمال تفضيلًا";

// ---- /settings ----
pub const SETTINGS_SAVED: &str = "تم، حفظتُ التغيير ✅";
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
pub const GAMES_DISABLED: &str = "الفعاليات معطّلة في هذا السيرفر.";
pub const GAME_PICK_MODE: &str = "اختر نوع اللعبة من الاقتراحات.";
pub const VERSUS_PROMPT: &str = "أيهما تفضّل؟";
pub const VERSUS_RESULTS: &str = "نتائج التصويت حتى الآن:";
pub const LEADERBOARD_TITLE: &str = "ترتيب الألعاب";
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
pub const NOT_ADMIN: &str = "هذا الأمر للمشرفين فقط (صلاحية «إدارة السيرفر»).";
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
pub const LANGUAGE_SET: &str = "✅ حفظتُ اللغة: العربية.";
pub const LANGUAGE_SET_EN: &str = "✅ Language saved. The bot's texts are currently Arabic only.";
pub const LANGUAGE_UNKNOWN: &str = "اللغة غير معروفة. اختر من القائمة.";
pub const SETTINGS_UNKNOWN: &str = "إعداد غير معروف. اختر اللغة أو الخصوصية.";
pub const UPLOAD_ADMINS_ONLY: &str = "رفع الصور المخصصة في هذا السيرفر للمشرفين فقط.";
pub const BANNER_PRESET_SET: &str = "✅ غيّرتُ غلافك 🎨";
pub const BANNER_RESET: &str = "✅ عاد الغلاف إلى الافتراضي.";
pub const BANNER_UNKNOWN: &str =
    "اختر تدرجًا من القائمة، أو لونًا مثل #ff8800، أو reset، أو أرفق صورة.";
pub const PROFILE_RESET_DONE: &str = "✅ أُعيد ضبط البروفايل (الشخصيات والأعمال محفوظة).";
pub const TITLE_SET: &str = "✅ غيّرتُ اسمك المعروض.";
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
pub const F_PROFILE_SUGGESTIONS: &str = "الاقتراحات";
pub const BANNER_PICKER_TITLE: &str = "🎨 اختر غلافًا جاهزًا";
pub const BANNER_PICKER_HELP: &str = "اختر من القائمة لترى المعاينة ثم اضغط «تطبيق».\nلرفع صورتك أو GIF: `/edit` ← البروفايل ← الغلاف وأرفق الملف (إن سمح السيرفر بالصور المخصصة)؛ أي مقاس يصلح وتبقى الـGIF متحركة. للون: اكتب `#ff8800` في القيمة.";
pub const BANNER_PICKER_PLACEHOLDER: &str = "الأغلفة الجاهزة…";
pub const BANNER_APPLY: &str = "تطبيق";
pub const BANNER_RESET_BTN: &str = "إعادة للافتراضي";
pub const BTN_REMOVE_FROM_PROFILE: &str = "إزالة من بروفايلي";
pub const CHARACTER_REMOVED: &str = "أزلتُ الشخصية من بروفايلك.";
pub const SUGGESTED_BY: &str = "اقترحه";
pub const SUGGEST_ADDED: &str = "✅ أضفتُ اقتراحك";
pub const SUGGEST_REASON_LONG: &str = "السبب أطول من 300 حرف.";
pub const SUGGEST_REASON_BAD: &str =
    "اكتب سببًا نصيًا (الروابط والإشارات والكلمات غير المناسبة تُحذف).";
pub const SUGGEST_SAME: &str = "اختر عملين مختلفين.";
pub const SUGGEST_NONE: &str =
    "لا توجد اقتراحات في هذا السيرفر بعد. أضف أول اقتراح بـ `/suggest add`.";
pub const SUGGEST_NONE_MINE: &str = "ليس لديك اقتراحات في هذا السيرفر.";
pub const SUGGEST_ANOTHER: &str = "اقتراح آخر";
pub const SUGGEST_DELETED: &str = "🗑 حذفتُ الاقتراح.";
pub const SUGGEST_CANNOT_DELETE: &str = "هذا الاقتراح ليس لك، فلا تستطيع حذفه.";
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
pub const REMINDER_SET: &str = "تمام 🔔 سأرسل لك رسالة خاصة عند صدور الجديد.";
pub const REMINDER_CANCELLED: &str = "🔕 ألغيتُ التذكير.";
pub const REMINDER_FULL: &str =
    "وصلتَ إلى الحد الأقصى للتذكيرات (50). احذف بعضها من /settings ثم جرّب مجددًا.";
pub const REMINDERS_TITLE: &str = "تذكيراتي";
pub const REMINDERS_NONE: &str = "لا توجد تذكيرات عندك. اضغط «ذكّرني» على أي عمل لتضيف واحدًا.";
pub const REMINDERS_PLACEHOLDER: &str = "اختر تذكيرًا لإلغائه…";
pub fn current_language(label: &str) -> String {
    format!("اللغة الحالية: **{label}**. اختر قيمة لتغييرها.")
}
pub fn current_privacy(label: &str) -> String {
    format!("الخصوصية الحالية: **{label}**. اختر قيمة لتغييرها.")
}
pub const BTN_SHOW_WORK: &str = "أظهر العمل";
pub const SHOW_WORK_UNAVAILABLE: &str = "لم أستطع عرض هذا العمل الآن. جرّب بعد قليل.";
pub const SUGGEST_OPEN_PLACEHOLDER: &str = "افتح بطاقة أحد العملين…";
pub const ARENA_CHARACTER_IMAGE: &str = "🖼 خمّن الشخصية من الصورة";
pub const ARENA_WORK_COVER: &str = "📚 خمّن العمل من الغلاف";
pub const ARENA_CHARACTER_HINTS: &str = "🕵️ خمّن الشخصية من الوصف";
pub const ARENA_TRUE_FALSE: &str = "✔️ صح أم خطأ";
pub const ARENA_HIGHER_LOWER: &str = "⚖️ الأعلى";
pub const ARENA_ORDER_YEARS: &str = "📅 رتّب حسب السنة";
pub const ARENA_WHO_IS_THIS: &str = "من هذه الشخصية؟";
pub const ARENA_WHICH_WORK: &str = "ما هذا العمل؟";
pub const ARENA_FROM: &str = "تظهر في:";
pub const ARENA_TRUE: &str = "صحيح";
pub const ARENA_FALSE: &str = "خطأ";
pub const ARENA_ANSWER_WAS: &str = "الإجابة:";
pub const ARENA_HIGHER_RATED: &str = "أيهما تقييمه أعلى؟";
pub const ARENA_MORE_POPULAR: &str = "أيهما أكثر شعبية؟";
pub const ARENA_ORDER_PROMPT: &str = "رتّب الأعمال من الأقدم إلى الأحدث:";
pub const ARENA_SKIP: &str = "تخطّي";
pub const ARENA_END: &str = "إنهاء اللعبة";
pub const ARENA_SCOREBOARD: &str = "النقاط";
pub const ARENA_NO_SCORES: &str = "لا نقاط بعد — أول إجابة صحيحة تكسب!";
pub const ARENA_FINAL: &str = "🏁 انتهت اللعبة! الترتيب النهائي:";
pub const ARENA_NOBODY: &str = "لم يُجب أحد بشكل صحيح.";
pub const ARENA_MANAGE_DENIED: &str = "التخطي والإنهاء لمن بدأ اللعبة أو للمشرفين فقط.";
pub const ARENA_ALREADY_ANSWERED: &str = "أجبتَ في هذه الجولة بالفعل.";
pub const ARENA_WRONG: &str = "❌ إجابة خاطئة — انتظر الجولة القادمة.";
pub const ARENA_CHANNEL_BUSY: &str = "هناك فعالية جارية في هذه القناة الآن. انتظر حتى تنتهي.";
pub fn arena_winner(user: u64) -> String {
    format!("🏆 أسرع إجابة صحيحة: <@{user}>")
}
pub const ARENA_WEEKLY: &str = "🗓 نقاط هذا الأسبوع (الألعاب الجماعية)";
pub const CHAPTER_WORD: &str = "الفصل";
pub const CHAPTER_NEW: &str = "فصل جديد:";
pub const CHAPTER_NEW_UNNUMBERED: &str = "صدر فصل جديد";
pub const CHAPTER_LANGUAGE: &str = "اللغة:";
pub const CHAPTER_SOURCE: &str = "المصدر:";
pub const BTN_READ: &str = "اقرأ";
pub const REMINDER_CHAPTER: &str = "🔔 تذكير: صدر فصل جديد من عمل طلبتَ التذكير به.";
pub const FEED_INPUT_HELP: &str =
    "اكتب: `رابط RSS | عنوان العمل` مثل `https://site.example/feed.xml | Solo Leveling`.";
pub const FEED_ADDED: &str = "✅ أُضيف الرابط. سيُربط بالعمل المتابَع الذي يطابق العنوان.";
pub const FEED_DUPLICATE: &str = "هذا الرابط مضاف بالفعل.";
pub const FEED_FULL: &str = "وصلتَ إلى الحد الأقصى من الروابط (10).";
pub fn feed_url_error(code: &str) -> String {
    let why = match code {
        "TooLong" => "الرابط طويل جدًا",
        "NotHttps" => "يجب أن يبدأ بـ https://",
        "Credentials" => "لا يُسمح بكلمة مرور داخل الرابط",
        "Port" => "المنفذ غير مسموح",
        "InternalHost" | "PrivateAddress" => "عنوان داخلي أو خاص غير مسموح",
        "Unresolvable" => "لم أستطع الوصول إلى هذا النطاق",
        _ => "رابط غير صالح",
    };
    format!("❌ {why}.")
}
pub const F_STATS: &str = "التقييم والشعبية";
pub const F_SEASONS: &str = "المواسم";
pub const F_NETWORK: &str = "القناة";
pub const F_RELEASE: &str = "تاريخ الإصدار";
pub const F_RUNTIME: &str = "المدة";
pub const F_COUNTRY: &str = "البلد";
pub const F_GENRES: &str = "التصنيفات";
pub const MINUTES: &str = "دقيقة";
pub const F_WHERE_TO_WATCH: &str = "أين أشاهد";
pub const PLAYED_BY: &str = "يؤدي الدور:";
pub const ACTOR_PHOTO_NOTE: &str = "📷 الصورة للممثل، فـ TMDB لا يوفّر صور الشخصيات.";
pub const NO_CHARACTERS: &str = "لا توجد شخصيات لعرضها لهذا العمل.";
pub const NO_CHARACTERS_SOURCE: &str = "هذا المصدر لا يوفّر قائمة شخصيات للأعمال.";
pub const PICK_CHARACTER_PLACEHOLDER: &str = "اختر شخصية لعرضها";
pub const REMINDER_UNSUPPORTED: &str = "التذكير غير متاح لهذا النوع من الأعمال.";
pub const STATUS_CLEARED: &str = "أزلتُ حالتك مع هذا العمل.";
pub fn status_saved(label: &str) -> String {
    format!("حفظتُ حالتك: {label}")
}
pub const CATEGORY_NEEDS_NAME: &str = "اكتب اسمًا أولًا، أو اختر «عمل عشوائي» أو «شخصية عشوائية».";
pub const TMDB_UNAVAILABLE: &str = "مصدر الدراما والمسلسلات والأفلام غير مفعّل على هذا البوت.";
pub const SCHEDULE_TITLE: &str = "جدول الأسبوع";
pub const SCHEDULE_EMPTY: &str = "لا شيء مجدول في هذا اليوم بهذه الفلاتر.";
pub const SCHEDULE_SOURCES: &str = "AniList · TMDB";
pub const SCHEDULE_CATEGORY_PLACEHOLDER: &str = "الفئة";
pub const SCHEDULE_OPEN_PLACEHOLDER: &str = "افتح عملًا من القائمة";
pub const SCHEDULE_MINE: &str = "أعمالي فقط";
pub const SCHEDULE_NO_MINE: &str = "لا توجد أعمال في بروفايلك أو تذكيراتك بعد، فالقائمة فارغة.";
pub const ARENA_CATEGORY_UNSUPPORTED: &str = "هذا النوع من الفعاليات غير متاح لهذه الفئة، جرّب «خمّن العمل من الغلاف» أو «صح أم خطأ» أو «الأعلى تقييمًا» أو «الترتيب بالسنة» أو «السباق».";
pub const BANNER_TOO_LARGE: &str = "الصورة كبيرة جدًا للغلاف (الحد 8 ميغابايت).";
pub const BANNER_TOO_MANY_PIXELS: &str = "الصورة ضخمة الأبعاد، صغّر أبعادها وجرّب مرة أخرى.";
pub const KIND_SERIES: &str = "مسلسل";
pub const KIND_MOVIE: &str = "فيلم";
pub const F_PROFILE_STATUS: &str = "الحالة";
pub const TOP_EMPTY: &str =
    "لا توجد بيانات بعد لهذا القسم. أضف أعمالًا أو شخصيات لبروفايلك وسيظهر الترتيب هنا.";
pub const TOP_CACHE_NOTE: &str = "يتحدّث كل 5 دقائق";
pub const TOP_SECTION_PLACEHOLDER: &str = "القسم";
pub const TOP_RANGE_PLACEHOLDER: &str = "المدة";
pub const TOP_OPEN_PLACEHOLDER: &str = "افتح عنصرًا من القائمة";
pub const REMINDER_SOON: &str = "🔔 تنبيه من تذكيراتك: عمل تتابعه يصدر قريبًا.";
pub const REMINDER_OUT: &str = "🔔 تنبيه من تذكيراتك: صدر شيء جديد من عمل تتابعه.";
pub const CHAPTER_DIGEST_TITLE: &str = "📖 فصول جديدة";
pub const CHAPTER_DIGEST_MORE: &str = "المزيد";
pub const BTN_CHAPTER: &str = "فصل";
pub const CHAPTER_DIGEST_EXPIRED: &str =
    "انتهت صلاحية هذا الملخص، ستصلك الفصول القادمة في الملخص التالي.";

pub const GAMES_DOWN: &str =
    "مصدر الألعاب لا يستجيب الآن، فلا أستطيع الحكم على الألعاب بأمان. جرّب بعد قليل.";
pub fn game_without_characters(title: &str) -> String {
    format!("لا أجد شخصيات مسجّلة للعبة «{title}» في المصادر. جرّب البحث عن الشخصية باسمها مباشرة.")
}
pub fn roster_of_game(title: &str) -> String {
    format!("📚 شخصيات لعبة {title}")
}
pub const CHARACTER_NO_IMAGE: &str =
    "لا توجد صورة لهذه الشخصية. يمكنك تعيين صورة لها من /edit بعد إضافتها لبروفايلك.";
pub const MAINTENANCE: &str = "البوت في صيانة قصيرة، نعود قريبًا 🛠";
/// Why an action was refused, naming the roles that may do it.
pub fn permission_denied(action: &str, roles: &[i64]) -> String {
    let who = roles
        .iter()
        .map(|r| format!("<@&{r}>"))
        .collect::<Vec<_>>()
        .join("، ");
    format!(
        "هذا الإجراء ({action}) متاح في هذا السيرفر لأصحاب الرتب: {who}. اطلب من المشرفين إضافتك."
    )
}
pub const POLL_CLOSED: &str = "انتهى هذا التصويت، ننتظرك في التصويت القادم.";
pub const WATCH_STARTED: &str = "بدأت ليلة المشاهدة هذه أو لم تعد موجودة.";
pub const COMPARE_YOURSELF: &str = "قارن مع عضو آخر، فمقارنتك مع نفسك ستكون 100% دائمًا 😄";
pub const COMPARE_PRIVATE: &str = "لا يمكن المقارنة: أحد البروفايلين خاص أو لا يمكنك رؤيته.";
pub const COMPARE_WORKS_PLACEHOLDER: &str = "افتح عملًا مشتركًا";
pub const COMPARE_CHARS_PLACEHOLDER: &str = "افتح شخصية مشتركة";
pub const REC_GENERAL_WHY: &str = "ترشيح عام من الأعمال الرائجة";
pub const REC_TMDB_FIELD: &str = "🎬 مسلسلات وأفلام";
pub const REC_TMDB_PLACEHOLDER: &str = "افتح مسلسلًا أو فيلمًا";

// ---- people ----------------------------------------------------------------------------------------
pub const PERSON_UNAVAILABLE: &str = "ما وجدت هذا الشخص، أو أن بياناته غير متاحة الآن.";
pub const PERSON_SOURCE_DOWN: &str = "المصدر لا يردّ الآن، جرّب بعد قليل.";
pub const PERSON_NO_WORKS: &str = "ما عندي أعمال أعرضها لهذا الشخص.";
pub const NO_CREW: &str = "ما وصلتني بيانات الطاقم لهذا العمل.";
pub const NO_PERFORMER: &str = "ما عندي مؤدٍّ مسجّل لهذه الشخصية.";
pub const PICK_WORK_OF_PERSON: &str = "افتح عملًا…";
pub const PICK_CHAR_OF_PERSON: &str = "افتح شخصية…";
pub const PICK_PERSON_TITLE: &str = "من تقصد؟";
pub const PICK_PERSON_PLACEHOLDER: &str = "افتح بطاقة شخص…";
pub const PICK_PERFORMER_PLACEHOLDER: &str = "اختر المؤدي…";
pub const F_BORN: &str = "الميلاد";
pub const F_NOTABLE_WORKS: &str = "من أعماله";
pub const PERSON_FAVOURITES: &str = "معجب";
pub const PERSON_POPULARITY: &str = "نقاط الشعبية";
pub fn years_old(age: i32) -> String {
    format!("{age} سنة")
}
pub fn works_of(name: &str) -> String {
    format!("أعمال {name}")
}
pub fn crew_of(title: &str) -> String {
    format!("طاقم {title}")
}
pub fn performers_of(name: &str) -> String {
    format!("مؤدّو {name}")
}

#[cfg(test)]
mod tests {
    /// Constants that are brand names or symbols, not sentences.
    const NOT_SENTENCES: &[&str] = &[
        "BTN_ANILIST",
        "POWERED_BY",
        "NONE",
        "COLOR_INVALID",
        "LANGUAGE_SET_EN",
    ];

    fn constants() -> Vec<(String, String)> {
        let src = include_str!("texts.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        let mut out = Vec::new();
        let mut lines = prod.lines().peekable();
        while let Some(line) = lines.next() {
            let Some(rest) = line.trim().strip_prefix("pub const ") else {
                continue;
            };
            let Some((name, tail)) = rest.split_once(':') else {
                continue;
            };
            // The value may start on the next line.
            let mut body = tail.to_string();
            if !body.contains('"') {
                if let Some(next) = lines.next() {
                    body.push_str(next);
                }
            }
            if let (Some(a), Some(b)) = (body.find('"'), body.rfind('"')) {
                if b > a {
                    out.push((name.trim().to_string(), body[a + 1..b].to_string()));
                }
            }
        }
        out
    }

    #[test]
    fn every_message_is_non_empty_and_arabic() {
        let all = constants();
        assert!(all.len() > 200, "the scan found only {} strings", all.len());
        let arabic = |s: &str| s.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c));
        for (name, text) in &all {
            if NOT_SENTENCES.contains(&name.as_str()) || name.starts_with("F_") && !arabic(text) {
                continue;
            }
            assert!(!text.trim().is_empty(), "{name} is empty");
            // Pure ASCII values are ids or labels such as "TV"; sentences must have Arabic.
            let has_letters = text.chars().any(|c| c.is_alphabetic());
            assert!(
                !has_letters || arabic(text) || text.len() <= 24,
                "{name} has no Arabic: {text}"
            );
        }
    }

    #[test]
    fn the_stiff_passive_openers_that_were_rewritten_stay_rewritten() {
        let all = constants();
        for (name, text) in &all {
            for stiff in [
                "تمت إضافة",
                "تمت إزالة",
                "تم نقل",
                "حدث خطأ غير متوقع",
                "لم أجد نتائج مطابقة",
            ] {
                assert!(
                    !text.starts_with(stiff),
                    "{name} went back to the stiff wording: {text}"
                );
            }
        }
    }
}
