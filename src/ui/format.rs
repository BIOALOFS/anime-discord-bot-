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
        "RELEASING" => "مستمر",
        "FINISHED" => "مكتمل",
        "NOT_YET_RELEASED" => "لم يصدر",
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

pub fn role_ar(r: &str) -> &'static str {
    match r {
        "MAIN" => crate::i18n::ROLE_MAIN,
        "SUPPORTING" => crate::i18n::ROLE_SUPPORTING,
        _ => crate::i18n::ROLE_BACKGROUND,
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
        assert_eq!(status_ar("RELEASING"), "مستمر");
        assert_eq!(status_ar("FINISHED"), "مكتمل");
        assert_eq!(status_ar("NOT_YET_RELEASED"), "لم يصدر");
        assert_eq!(status_ar("HIATUS"), "متوقف");
        assert_eq!(format_ar("TV"), "مسلسل");
        assert_eq!(format_ar("MOVIE"), "فيلم");
        assert_eq!(format_ar("OVA"), "OVA");
        assert_eq!(format_ar("ONA"), "ONA");
        assert_eq!(format_ar("SPECIAL"), "خاص");
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
}
