//! People: actors, voice actors, staff, creators and companies. A person has a provider-prefixed key
//! (`anilist-staff:{id}`, `tmdb-person:{id}`, `vndb-staff:s{id}`, `igdb-company:{id}`) and is shown as a card with
//! its own works. The parsers here are pure (documents in, safe values out); `fetch` does the requests.

pub mod anilist;
pub mod fetch;
pub mod igdb;
pub mod msgs;
pub mod tmdb;
pub mod vndb;

/// Where a person comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PSource {
    AniList,
    Tmdb,
    Vndb,
    Igdb,
}

impl PSource {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::AniList => "anilist-staff",
            Self::Tmdb => "tmdb-person",
            Self::Vndb => "vndb-staff",
            Self::Igdb => "igdb-company",
        }
    }

    pub fn credit(self) -> &'static str {
        match self {
            Self::AniList => "AniList",
            Self::Tmdb => "TMDB",
            Self::Vndb => "VNDB",
            Self::Igdb => "IGDB",
        }
    }

    pub fn key(self, id: impl std::fmt::Display) -> String {
        format!("{}:{id}", self.prefix())
    }
}

/// Split and validate a person key. The id part is digits (VNDB: `s` and digits).
pub fn parse_key(key: &str) -> Option<(PSource, String)> {
    let (prefix, id) = key.split_once(':')?;
    let digits = |s: &str| !s.is_empty() && s.len() <= 12 && s.chars().all(|c| c.is_ascii_digit());
    let source = match prefix {
        "anilist-staff" if digits(id) => PSource::AniList,
        "tmdb-person" if digits(id) => PSource::Tmdb,
        "igdb-company" if digits(id) => PSource::Igdb,
        "vndb-staff" if id.strip_prefix('s').is_some_and(digits) => PSource::Vndb,
        _ => return None,
    };
    Some((source, id.to_string()))
}

/// A search result or crew member: enough to list and to open the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub key: String,
    pub name: String,
    pub native: Option<String>,
    /// What they are, in Arabic ("مؤدي أصوات").
    pub hint: String,
    pub popularity: i64,
}

impl Hit {
    /// The autocomplete label: "👤 Hiroshi Kamiya · مؤدي أصوات".
    pub fn label(&self) -> String {
        label_with_icon("👤", &self.name, &self.hint)
    }
}

pub fn label_with_icon(icon: &str, name: &str, hint: &str) -> String {
    if hint.is_empty() {
        format!("{icon} {name}")
    } else {
        format!("{icon} {name} · {hint}")
    }
}

/// One entry of a person's list of works.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credit {
    /// Provider key of the work (`anilist:1`, `tmdb-movie:3`…).
    pub work_key: Option<String>,
    pub title: String,
    pub year: Option<i32>,
    /// The job ("مخرج") when it is crew work.
    pub role: Option<String>,
    /// The character played or voiced: `(character key, name)`.
    pub character: Option<(String, String)>,
}

impl Credit {
    /// "Title (2019) — Character / job".
    pub fn line(&self) -> String {
        let mut s = format!("**{}**", self.title);
        if let Some(y) = self.year {
            s.push_str(&format!(" ({y})"));
        }
        let what = self
            .character
            .as_ref()
            .map(|(_, n)| n.clone())
            .or_else(|| self.role.clone());
        if let Some(w) = what {
            s.push_str(&format!(" — {w}"));
        }
        s
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Person {
    pub key: String,
    pub name: String,
    pub native: Option<String>,
    /// Arabic role labels ("مؤدي أصوات", "مخرج").
    pub roles: Vec<String>,
    pub birth: Option<String>,
    pub age: Option<i32>,
    pub deceased: bool,
    pub popularity: Option<i64>,
    pub image: Option<String>,
    pub about: Option<String>,
    pub url: Option<String>,
    /// Birthplace, home town or a company's country.
    pub place: Option<String>,
    pub credits: Vec<Credit>,
}

/// A member of a work's crew: who, and what they did ("ممثل · Walter White").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrewEntry {
    pub person: Hit,
    pub role: String,
}

// ---- shared helpers -------------------------------------------------------------------------------

/// Arabic label of a provider role / department / occupation; unknown ones stay as they are.
pub fn role_ar(role: &str) -> String {
    let r = role.trim().to_lowercase();
    let known: &[(&str, &str)] = &[
        ("voice actor", "مؤدي أصوات"),
        ("voice acting", "مؤدي أصوات"),
        ("seiyuu", "مؤدي أصوات"),
        ("acting", "ممثل"),
        ("actor", "ممثل"),
        ("actress", "ممثلة"),
        ("director", "مخرج"),
        ("directing", "مخرج"),
        ("chief director", "مخرج"),
        ("episode director", "مخرج حلقة"),
        ("writing", "كاتب"),
        ("writer", "كاتب"),
        ("screenplay", "كاتب سيناريو"),
        ("script", "كاتب سيناريو"),
        ("series composition", "تأليف السلسلة"),
        ("original creator", "مؤلف العمل الأصلي"),
        ("original story", "مؤلف العمل الأصلي"),
        ("story", "قصة"),
        ("story & art", "قصة ورسم"),
        ("art", "رسام"),
        ("illustrator", "رسام"),
        ("character design", "تصميم الشخصيات"),
        ("character designer", "تصميم الشخصيات"),
        ("music", "موسيقى"),
        ("composer", "مؤلف موسيقي"),
        ("sound", "صوت"),
        ("production", "إنتاج"),
        ("producer", "منتج"),
        ("executive producer", "منتج تنفيذي"),
        ("creator", "صانع العمل"),
        ("novel", "مؤلف الرواية"),
        ("animation director", "مدير الرسوم المتحركة"),
        ("key animation", "رسوم رئيسية"),
        ("art director", "مدير فني"),
        ("sound director", "مخرج صوتي"),
        ("developer", "مطوّر"),
        ("publisher", "ناشر"),
        ("scenario", "كاتب السيناريو"),
        ("artist", "فنان"),
        ("staff", "طاقم"),
    ];
    for (en, ar) in known {
        if r == *en {
            return (*ar).to_string();
        }
    }
    role.trim().to_string()
}

/// "1980-05-12" from year/month/day parts (partial dates keep what is known).
pub fn date_string(y: Option<i64>, m: Option<i64>, d: Option<i64>) -> Option<String> {
    let y = y.filter(|y| *y > 0)?;
    Some(
        match (
            m.filter(|m| (1..=12).contains(m)),
            d.filter(|d| (1..=31).contains(d)),
        ) {
            (Some(m), Some(d)) => format!("{y:04}-{m:02}-{d:02}"),
            (Some(m), None) => format!("{y:04}-{m:02}"),
            _ => format!("{y:04}"),
        },
    )
}

/// Age in whole years today from a `YYYY[-MM[-DD]]` birth date (none for a bad or future date).
pub fn age_from(birth: &str, today: chrono::NaiveDate) -> Option<i32> {
    use chrono::Datelike;
    let mut p = birth.split('-');
    let y: i32 = p.next()?.parse().ok()?;
    let m: u32 = p.next().and_then(|m| m.parse().ok()).unwrap_or(1);
    let d: u32 = p.next().and_then(|d| d.parse().ok()).unwrap_or(1);
    let born = chrono::NaiveDate::from_ymd_opt(y, m.clamp(1, 12), d.clamp(1, 28))?;
    if born > today {
        return None;
    }
    let mut age = today.year() - born.year();
    if (today.month(), today.day()) < (born.month(), born.day()) {
        age -= 1;
    }
    (0..=130).contains(&age).then_some(age)
}

pub fn today() -> chrono::NaiveDate {
    chrono::Utc::now().date_naive()
}

/// Plain text of a provider biography: markup removed, whitespace collapsed, cut at a sentence or word.
pub fn clean_about(raw: &str, max: usize) -> Option<String> {
    let mut s = String::with_capacity(raw.len());
    let mut in_tag = false;
    for c in raw.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            '_' | '*' | '~' | '`' => {}
            c => s.push(c),
        }
    }
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() {
        return None;
    }
    Some(crate::ui::text::truncate_words(&s, max))
}

/// Merge hits from several providers: exact name first, then the most popular; at most `n`.
pub fn rank(mut hits: Vec<Hit>, query: &str, n: usize) -> Vec<Hit> {
    let q = query.trim().to_lowercase();
    hits.sort_by_key(|h| {
        let exact = h.name.to_lowercase() == q
            || h.native.as_deref().is_some_and(|n| n.to_lowercase() == q);
        (std::cmp::Reverse(exact), std::cmp::Reverse(h.popularity))
    });
    let mut seen = std::collections::HashSet::new();
    hits.retain(|h| seen.insert(h.key.clone()));
    hits.truncate(n);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn person_keys_parse_and_reject_everything_else() {
        assert_eq!(
            parse_key("anilist-staff:95"),
            Some((PSource::AniList, "95".into()))
        );
        assert_eq!(
            parse_key("tmdb-person:287"),
            Some((PSource::Tmdb, "287".into()))
        );
        assert_eq!(
            parse_key("vndb-staff:s42"),
            Some((PSource::Vndb, "s42".into()))
        );
        assert_eq!(
            parse_key("igdb-company:70"),
            Some((PSource::Igdb, "70".into()))
        );
        for bad in [
            "anilist-staff:",
            "anilist-staff:x",
            "vndb-staff:42",
            "igdb-company:s1",
            "tmdb-cast:tv:1:2",
            "anilist:1",
            "x",
        ] {
            assert_eq!(parse_key(bad), None, "{bad}");
        }
        for s in [PSource::AniList, PSource::Tmdb, PSource::Igdb] {
            assert!(parse_key(&s.key(5)).is_some());
        }
        assert!(parse_key(&PSource::Vndb.key("s5")).is_some());
    }

    #[test]
    fn labels_carry_the_role_hint() {
        let h = Hit {
            key: "anilist-staff:1".into(),
            name: "Hiroshi Kamiya".into(),
            native: None,
            hint: "مؤدي أصوات".into(),
            popularity: 9,
        };
        assert_eq!(h.label(), "👤 Hiroshi Kamiya · مؤدي أصوات");
        assert_eq!(label_with_icon("🎙️", "X", ""), "🎙️ X");
    }

    #[test]
    fn roles_are_arabic_and_unknown_ones_stay() {
        assert_eq!(role_ar("Voice Actor"), "مؤدي أصوات");
        assert_eq!(role_ar("Director"), "مخرج");
        assert_eq!(role_ar("Gaffer"), "Gaffer");
    }

    #[test]
    fn dates_and_ages() {
        assert_eq!(
            date_string(Some(1975), Some(1), Some(28)),
            Some("1975-01-28".into())
        );
        assert_eq!(
            date_string(Some(1975), Some(1), None),
            Some("1975-01".into())
        );
        assert_eq!(date_string(Some(1975), None, Some(3)), Some("1975".into()));
        assert_eq!(date_string(None, Some(1), Some(1)), None);
        let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).expect("date");
        assert_eq!(age_from("1975-01-28", today), Some(51));
        assert_eq!(
            age_from("1975-05-02", today),
            Some(50),
            "birthday not yet reached"
        );
        assert_eq!(age_from("1975", today), Some(51));
        assert_eq!(age_from("2999-01-01", today), None);
        assert_eq!(age_from("1800-01-01", today), None);
    }

    #[test]
    fn about_is_plain_and_short() {
        let t = clean_about(
            "<b>Known</b> for __many__ roles.<br>\n\nSecond   line.",
            300,
        )
        .expect("text");
        assert_eq!(t, "Known for many roles. Second line.");
        assert!(clean_about("   ", 10).is_none());
        let long = "word ".repeat(200);
        assert!(clean_about(&long, 100).expect("text").chars().count() <= 100);
    }

    #[test]
    fn ranking_puts_the_exact_name_first_then_popularity_and_drops_duplicates() {
        let h = |k: &str, n: &str, p: i64| Hit {
            key: k.into(),
            name: n.into(),
            native: None,
            hint: String::new(),
            popularity: p,
        };
        let out = rank(
            vec![
                h("a:1", "Other Kamiya", 500),
                h("a:2", "Kamiya", 1),
                h("a:1", "Other Kamiya", 500),
                h("a:3", "Big Name", 900),
            ],
            "kamiya",
            10,
        );
        assert_eq!(
            out.iter().map(|h| h.key.as_str()).collect::<Vec<_>>(),
            vec!["a:2", "a:3", "a:1"]
        );
        assert_eq!(rank(out, "kamiya", 2).len(), 2);
    }

    #[test]
    fn a_credit_line_names_the_title_year_and_role() {
        let c = Credit {
            work_key: Some("anilist:1".into()),
            title: "Steins;Gate".into(),
            year: Some(2011),
            role: None,
            character: Some(("1".into(), "Okabe".into())),
        };
        assert_eq!(c.line(), "**Steins;Gate** (2011) — Okabe");
        let c = Credit {
            work_key: None,
            title: "Film".into(),
            year: None,
            role: Some("مخرج".into()),
            character: None,
        };
        assert_eq!(c.line(), "**Film** — مخرج");
    }
}
