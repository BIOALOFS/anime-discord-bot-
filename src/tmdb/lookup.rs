//! Arabic names for TMDB countries, genres and statuses, and the choices of the category commands built from them.

use super::model::Kind;

/// `(ISO code, Arabic name)`.
pub const COUNTRIES: &[(&str, &str)] = &[
    ("KR", "كوريا الجنوبية"),
    ("JP", "اليابان"),
    ("CN", "الصين"),
    ("TW", "تايوان"),
    ("HK", "هونغ كونغ"),
    ("TH", "تايلاند"),
    ("VN", "فيتنام"),
    ("ID", "إندونيسيا"),
    ("PH", "الفلبين"),
    ("MY", "ماليزيا"),
    ("SG", "سنغافورة"),
    ("US", "الولايات المتحدة"),
    ("GB", "بريطانيا"),
    ("CA", "كندا"),
    ("AU", "أستراليا"),
    ("FR", "فرنسا"),
    ("DE", "ألمانيا"),
    ("ES", "إسبانيا"),
    ("IT", "إيطاليا"),
    ("SE", "السويد"),
    ("DK", "الدنمارك"),
    ("NO", "النرويج"),
    ("TR", "تركيا"),
    ("IN", "الهند"),
    ("EG", "مصر"),
    ("SA", "السعودية"),
    ("AE", "الإمارات"),
    ("KW", "الكويت"),
    ("SY", "سوريا"),
    ("LB", "لبنان"),
    ("MX", "المكسيك"),
    ("BR", "البرازيل"),
    ("RU", "روسيا"),
];

pub fn country_name(code: &str) -> String {
    COUNTRIES
        .iter()
        .find(|(c, _)| c.eq_ignore_ascii_case(code))
        .map_or_else(|| code.to_uppercase(), |(_, n)| (*n).to_string())
}

/// A country typed or picked: the code, whether given as a code or as the Arabic name.
pub fn country_code(input: &str) -> Option<&'static str> {
    let t = input.trim();
    COUNTRIES
        .iter()
        .find(|(c, n)| c.eq_ignore_ascii_case(t) || *n == t)
        .map(|(c, _)| *c)
}

/// `(Arabic name, TV genre id, movie genre id)`; `0` means the genre does not exist for that kind.
pub const GENRES: &[(&str, u32, u32)] = &[
    ("أكشن ومغامرة", 10759, 28),
    ("مغامرة", 10759, 12),
    ("رسوم متحركة", 16, 16),
    ("كوميديا", 35, 35),
    ("جريمة", 80, 80),
    ("وثائقي", 99, 99),
    ("دراما", 18, 18),
    ("عائلي", 10751, 10751),
    ("فانتازيا", 10765, 14),
    ("خيال علمي", 10765, 878),
    ("غموض", 9648, 9648),
    ("رومانسي", 0, 10749),
    ("رعب", 0, 27),
    ("إثارة", 0, 53),
    ("حرب", 10768, 10752),
    ("تاريخي", 0, 36),
    ("موسيقى", 0, 10402),
    ("ويسترن", 37, 37),
    ("واقعي (ريالتي)", 10764, 0),
];

pub fn genre_id(name: &str, kind: Kind) -> Option<u32> {
    GENRES
        .iter()
        .find(|(n, _, _)| *n == name.trim())
        .map(|(_, tv, movie)| if kind == Kind::Tv { *tv } else { *movie })
        .filter(|id| *id > 0)
}

/// Genres that exist for the kind, as Arabic names.
pub fn genre_names(kind: Kind) -> Vec<&'static str> {
    GENRES
        .iter()
        .filter(|(_, tv, movie)| {
            if kind == Kind::Tv {
                *tv > 0
            } else {
                *movie > 0
            }
        })
        .map(|(n, _, _)| *n)
        .collect()
}

/// `(Arabic name, TMDB `with_status` value)` for shows.
pub const TV_STATUSES: &[(&str, &str)] = &[
    ("مستمر", "0"),
    ("قيد الإنتاج", "2"),
    ("منتهٍ", "3"),
    ("ملغي", "4"),
];

pub fn tv_status_value(name: &str) -> Option<&'static str> {
    TV_STATUSES
        .iter()
        .find(|(n, _)| *n == name.trim())
        .map(|(_, v)| *v)
}

/// Does a detail `status` string match the chosen Arabic status?
pub fn status_matches(status: Option<&str>, chosen: &str) -> bool {
    let s = status.unwrap_or("");
    match chosen.trim() {
        "مستمر" => s == "Returning Series",
        "قيد الإنتاج" => matches!(s, "In Production" | "Planned" | "Pilot"),
        "منتهٍ" => s == "Ended",
        "ملغي" => s == "Canceled",
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countries_resolve_by_code_or_arabic_name_and_print_in_arabic() {
        assert_eq!(country_name("kr"), "كوريا الجنوبية");
        assert_eq!(country_name("ZZ"), "ZZ", "unknown codes stay readable");
        assert_eq!(country_code("JP"), Some("JP"));
        assert_eq!(country_code("اليابان"), Some("JP"));
        assert_eq!(country_code("nowhere"), None);
        let mut seen = std::collections::HashSet::new();
        assert!(
            COUNTRIES.iter().all(|(c, _)| seen.insert(*c)),
            "unique codes"
        );
    }

    #[test]
    fn genres_map_to_the_id_of_each_kind() {
        assert_eq!(genre_id("دراما", Kind::Tv), Some(18));
        assert_eq!(genre_id("رعب", Kind::Movie), Some(27));
        assert_eq!(genre_id("رعب", Kind::Tv), None, "no horror genre for shows");
        assert_eq!(genre_id("واقعي (ريالتي)", Kind::Movie), None);
        assert!(
            genre_names(Kind::Tv).contains(&"دراما") && !genre_names(Kind::Tv).contains(&"رعب")
        );
        assert!(genre_names(Kind::Movie).contains(&"رعب"));
    }

    #[test]
    fn show_statuses_map_both_ways() {
        assert_eq!(tv_status_value("منتهٍ"), Some("3"));
        assert_eq!(tv_status_value("غير معروف"), None);
        assert!(status_matches(Some("Returning Series"), "مستمر"));
        assert!(status_matches(Some("Canceled"), "ملغي"));
        assert!(!status_matches(Some("Ended"), "مستمر"));
        assert!(
            status_matches(None, "أي شيء"),
            "unknown choices do not filter"
        );
    }
}
