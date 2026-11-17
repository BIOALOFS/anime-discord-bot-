//! TMDB safety: `include_adult=false` is always sent, and anything flagged adult, carrying an explicit certification
//! or an explicit keyword is dropped as well. Items without any reliable signal are kept (TMDB already leaves most
//! adult titles out with `include_adult=false`).

/// Easy to extend: explicit erotic or sexual keywords (lower case, matched as whole keywords).
pub const BLACKLISTED_KEYWORDS: &[&str] = &[
    "softcore",
    "hardcore",
    "erotic",
    "erotica",
    "pornography",
    "porn",
    "sex scene",
    "explicit sex",
    "hentai",
    "adult film",
    "nudity",
    "sexploitation",
    "xxx",
];

/// Certifications that mean explicit adult material (compared case-insensitively, spaces ignored).
pub const BLOCKED_CERTIFICATIONS: &[&str] = &[
    "nc-17", "nc17", "x", "xxx", "r18+", "r18", "r-18", "18+", "adult",
];

/// Should this item be hidden?
pub fn is_blocked(adult: bool, keywords: &[String], certifications: &[String]) -> bool {
    if adult {
        return true;
    }
    if keywords
        .iter()
        .any(|k| BLACKLISTED_KEYWORDS.contains(&k.trim().to_lowercase().as_str()))
    {
        return true;
    }
    certifications.iter().any(|c| {
        BLOCKED_CERTIFICATIONS.contains(&c.trim().to_lowercase().replace(' ', "").as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn adult_flag_blacklisted_keyword_and_explicit_certification_are_blocked() {
        assert!(is_blocked(true, &[], &[]), "adult flag");
        assert!(
            is_blocked(false, &v(&["Erotic"]), &[]),
            "blacklisted keyword, any case"
        );
        assert!(is_blocked(false, &v(&["drama", "softcore"]), &[]));
        assert!(is_blocked(false, &[], &v(&["NC-17"])), "NC-17");
        assert!(is_blocked(false, &[], &v(&["R 18+"])), "R18+ style");
        assert!(is_blocked(false, &[], &v(&["X"])));
    }

    #[test]
    fn clean_items_and_items_without_signals_are_kept() {
        assert!(!is_blocked(
            false,
            &v(&["based on novel", "friendship"]),
            &v(&["PG-13", "TV-14", "R"])
        ));
        assert!(!is_blocked(false, &[], &[]), "no signal at all: kept");
        assert!(
            !is_blocked(false, &v(&["sexual harassment awareness"]), &[]),
            "whole keywords only"
        );
    }
}
