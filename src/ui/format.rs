//! Arabic labels and small formatting helpers.

use chrono::Weekday;

pub fn score_text(avg: Option<i32>) -> String {
    match avg {
        Some(a) => {
            let filled = ((a as f32 / 10.0).round() as usize).min(10);
            format!(
                "{:.1}/10 {}{}",
                a as f32 / 10.0,
                "▰".repeat(filled),
                "▱".repeat(10 - filled)
            )
        }
        None => crate::i18n::NONE.to_string(),
    }
}

pub fn status_ar(s: &str) -> &'static str {
    match s {
        "RELEASING" => "يعرض حاليًا",
        "FINISHED" => "انتهى",
        "NOT_YET_RELEASED" => "لم يعرض بعد",
        "HIATUS" => "متوقف",
        "CANCELLED" => "ملغى",
        _ => "غير معروف",
    }
}

pub fn format_ar(f: &str) -> &'static str {
    match f {
        "TV" => "مسلسل",
        "TV_SHORT" => "مسلسل قصير",
        "MOVIE" => "فيلم",
        "SPECIAL" => "خاص",
        "OVA" => "OVA",
        "ONA" => "ONA",
        "MUSIC" => "موسيقى",
        "MANGA" => "مانغا",
        "NOVEL" => "رواية",
        "ONE_SHOT" => "فصل واحد",
        _ => "غير معروف",
    }
}

pub fn season_ar(s: &str) -> &'static str {
    match s {
        "WINTER" => "شتاء",
        "SPRING" => "ربيع",
        "SUMMER" => "صيف",
        "FALL" => "خريف",
        _ => "غير معروف",
    }
}

pub fn source_ar(s: &str) -> String {
    match s {
        "ORIGINAL" => "أصلي".into(),
        "MANGA" => "مانغا".into(),
        "LIGHT_NOVEL" => "رواية خفيفة".into(),
        "VISUAL_NOVEL" => "رواية بصرية".into(),
        "VIDEO_GAME" => "لعبة فيديو".into(),
        "WEB_NOVEL" => "رواية ويب".into(),
        "NOVEL" => "رواية".into(),
        "ANIME" => "أنمي".into(),
        other => other.replace('_', " ").to_lowercase(),
    }
}

pub fn role_ar(r: &str) -> &'static str {
    match r {
        "MAIN" => crate::i18n::ROLE_MAIN,
        "SUPPORTING" => crate::i18n::ROLE_SUPPORTING,
        _ => crate::i18n::ROLE_BACKGROUND,
    }
}

pub fn relation_ar(r: &str) -> &'static str {
    match r {
        "SEQUEL" => "تتمة",
        "PREQUEL" => "جزء سابق",
        "SIDE_STORY" => "قصة جانبية",
        "SPIN_OFF" => "عمل فرعي",
        "PARENT" => "العمل الأصلي",
        "ALTERNATIVE" => "نسخة بديلة",
        "ADAPTATION" => "اقتباس",
        "SOURCE" => "المصدر",
        "SUMMARY" => "ملخص",
        "COMPILATION" => "تجميع",
        "CONTAINS" => "يحتوي",
        "CHARACTER" => "شخصيات مشتركة",
        _ => "أخرى",
    }
}

pub fn weekday_ar(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "الاثنين",
        Weekday::Tue => "الثلاثاء",
        Weekday::Wed => "الأربعاء",
        Weekday::Thu => "الخميس",
        Weekday::Fri => "الجمعة",
        Weekday::Sat => "السبت",
        Weekday::Sun => "الأحد",
    }
}

/// 1234567 -> "1,234,567"
pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        out.insert(0, '-');
    }
    out
}

pub fn ts_relative(unix: i64) -> String {
    format!("<t:{unix}:R>")
}

pub fn ts_full(unix: i64) -> String {
    format!("<t:{unix}:F>")
}

pub fn ts_time(unix: i64) -> String {
    format!("<t:{unix}:t>")
}

/// Season for a calendar date: Dec/Jan/Feb winter, Mar-May spring, Jun-Aug summer, Sep-Nov fall.
/// December belongs to the winter season of the following year.
pub fn season_from_date(year: i32, month: u32) -> (&'static str, i32) {
    match month {
        12 => ("WINTER", year + 1),
        1 | 2 => ("WINTER", year),
        3..=5 => ("SPRING", year),
        6..=8 => ("SUMMER", year),
        _ => ("FALL", year),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_bar_rendering() {
        assert_eq!(score_text(Some(82)), "8.2/10 ▰▰▰▰▰▰▰▰▱▱");
        assert_eq!(score_text(Some(100)), "10.0/10 ▰▰▰▰▰▰▰▰▰▰");
        assert_eq!(score_text(Some(0)), "0.0/10 ▱▱▱▱▱▱▱▱▱▱");
        assert_eq!(score_text(Some(45)), "4.5/10 ▰▰▰▰▰▱▱▱▱▱");
        assert_eq!(score_text(None), "—");
        for s in 0..=100 {
            assert_eq!(
                score_text(Some(s))
                    .chars()
                    .filter(|c| "▰▱".contains(*c))
                    .count(),
                10
            );
        }
    }

    #[test]
    fn arabic_mappings() {
        assert_eq!(status_ar("RELEASING"), "يعرض حاليًا");
        assert_eq!(status_ar("FINISHED"), "انتهى");
        assert_eq!(status_ar("NOT_YET_RELEASED"), "لم يعرض بعد");
        assert_eq!(status_ar("HIATUS"), "متوقف");
        assert_eq!(format_ar("TV"), "مسلسل");
        assert_eq!(format_ar("MOVIE"), "فيلم");
        assert_eq!(format_ar("OVA"), "OVA");
        assert_eq!(format_ar("ONA"), "ONA");
        assert_eq!(format_ar("SPECIAL"), "خاص");
        assert_eq!(season_ar("WINTER"), "شتاء");
        assert_eq!(season_ar("SPRING"), "ربيع");
        assert_eq!(season_ar("SUMMER"), "صيف");
        assert_eq!(season_ar("FALL"), "خريف");
        assert_eq!(status_ar("???"), "غير معروف");
    }

    #[test]
    fn thousands_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(1234567), "1,234,567");
        assert_eq!(thousands(-12345), "-12,345");
    }

    #[test]
    fn timestamps() {
        assert_eq!(ts_relative(10), "<t:10:R>");
        assert_eq!(ts_full(10), "<t:10:F>");
    }

    #[test]
    fn season_boundaries() {
        assert_eq!(season_from_date(2025, 11), ("FALL", 2025));
        assert_eq!(season_from_date(2025, 12), ("WINTER", 2026));
        assert_eq!(season_from_date(2026, 1), ("WINTER", 2026));
        assert_eq!(season_from_date(2026, 2), ("WINTER", 2026));
        assert_eq!(season_from_date(2026, 3), ("SPRING", 2026));
        assert_eq!(season_from_date(2026, 5), ("SPRING", 2026));
        assert_eq!(season_from_date(2026, 6), ("SUMMER", 2026));
        assert_eq!(season_from_date(2026, 8), ("SUMMER", 2026));
        assert_eq!(season_from_date(2026, 9), ("FALL", 2026));
    }
}
