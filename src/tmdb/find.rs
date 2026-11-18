//! Finding shows and films for the category commands: search by name and random picks, both honouring the
//! `/drama` / `/series` split, the filters and the member's recent picks.

use super::{
    lookup,
    model::{classify, Kind, Target, Work, ASIAN_DRAMA_COUNTRIES},
    Tmdb,
};
use std::collections::HashMap;

/// The three TMDB-backed commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cat {
    Drama,
    Series,
    Movie,
}

impl Cat {
    pub fn kind(self) -> Kind {
        if self == Self::Movie {
            Kind::Movie
        } else {
            Kind::Tv
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Drama => "drama",
            Self::Series => "series",
            Self::Movie => "movie",
        }
    }

    /// Does a show belong to this command? Films always belong to `/movie`.
    pub fn accepts(self, w: &Work, overrides: &HashMap<String, Target>) -> bool {
        match self {
            Self::Movie => w.kind == Some(Kind::Movie),
            Self::Drama => {
                w.kind == Some(Kind::Tv) && classify(&w.countries, overrides) == Target::Drama
            }
            Self::Series => {
                w.kind == Some(Kind::Tv) && classify(&w.countries, overrides) == Target::Series
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub year: Option<i32>,
    /// ISO code.
    pub country: Option<String>,
    /// Arabic status name (shows only).
    pub status: Option<String>,
    /// Arabic genre name.
    pub genre: Option<String>,
}

impl Filter {
    /// What a list entry can be checked against without loading its details.
    fn light_match(&self, w: &Work) -> bool {
        self.year.is_none_or(|y| w.year == Some(y))
            && self
                .country
                .as_deref()
                .is_none_or(|c| w.countries.iter().any(|x| x.eq_ignore_ascii_case(c)))
    }

    fn needs_details(&self) -> bool {
        self.status.is_some() || self.genre.is_some()
    }

    fn full_match(&self, w: &Work) -> bool {
        self.light_match(w)
            && self
                .status
                .as_deref()
                .is_none_or(|s| lookup::status_matches(w.status.as_deref(), s))
            && self.genre.as_deref().is_none_or(|g| {
                w.genres
                    .iter()
                    .any(|x| genre_synonyms(g).contains(&x.as_str()))
            })
    }
}

/// English genre names TMDB uses for an Arabic genre label.
fn genre_synonyms(arabic: &str) -> Vec<&'static str> {
    match arabic.trim() {
        "أكشن ومغامرة" => vec!["Action & Adventure", "Action"],
        "مغامرة" => vec!["Adventure", "Action & Adventure"],
        "رسوم متحركة" => vec!["Animation"],
        "كوميديا" => vec!["Comedy"],
        "جريمة" => vec!["Crime"],
        "وثائقي" => vec!["Documentary"],
        "دراما" => vec!["Drama"],
        "عائلي" => vec!["Family"],
        "فانتازيا" => vec!["Sci-Fi & Fantasy", "Fantasy"],
        "خيال علمي" => vec!["Sci-Fi & Fantasy", "Science Fiction"],
        "غموض" => vec!["Mystery"],
        "رومانسي" => vec!["Romance"],
        "رعب" => vec!["Horror"],
        "إثارة" => vec!["Thriller"],
        "حرب" => vec!["War & Politics", "War"],
        "تاريخي" => vec!["History"],
        "موسيقى" => vec!["Music"],
        "ويسترن" => vec!["Western"],
        "واقعي (ريالتي)" => vec!["Reality"],
        _ => Vec::new(),
    }
}

/// Search by name. Hits are list entries (no cast or providers yet); filters needing details check the first few.
pub async fn search(
    tmdb: &Tmdb,
    cat: Cat,
    query: &str,
    f: &Filter,
    overrides: &HashMap<String, Target>,
) -> Vec<Work> {
    let kind = cat.kind();
    let mut hits: Vec<Work> = tmdb
        .search(kind, query, 1)
        .await
        .into_iter()
        .filter(|w| cat.accepts(w, overrides) && f.light_match(w))
        .collect();
    hits.truncate(10);
    if !f.needs_details() {
        return hits;
    }
    let mut out = Vec::new();
    for w in hits.into_iter().take(6) {
        if let Some(full) = tmdb.details(kind, w.id).await {
            if f.full_match(&full) {
                out.push(full);
            }
        }
    }
    out
}

/// The `discover` parameters for a command and filters.
pub fn discover_params(cat: Cat, f: &Filter) -> Vec<(&'static str, String)> {
    let kind = cat.kind();
    let mut p: Vec<(&'static str, String)> = Vec::new();
    if let Some(y) = f.year {
        p.push((
            if kind == Kind::Tv {
                "first_air_date_year"
            } else {
                "primary_release_year"
            },
            y.to_string(),
        ));
    }
    match (&f.country, cat) {
        (Some(c), _) => p.push(("with_origin_country", c.clone())),
        (None, Cat::Drama) => p.push(("with_origin_country", ASIAN_DRAMA_COUNTRIES.join("|"))),
        _ => {}
    }
    if let Some(id) = f.genre.as_deref().and_then(|g| lookup::genre_id(g, kind)) {
        p.push(("with_genres", id.to_string()));
    }
    if kind == Kind::Tv {
        if let Some(s) = f.status.as_deref().and_then(lookup::tv_status_value) {
            p.push(("with_status", s.to_string()));
        }
    }
    p
}

/// A random work with full details: from a random page of the popularity-sorted discover list, skipping the
/// member's recent picks. Retries a few pages so a narrow filter or an empty page does not end in a refusal.
pub async fn random(
    tmdb: &Tmdb,
    cat: Cat,
    f: &Filter,
    overrides: &HashMap<String, Target>,
    user: i64,
) -> Option<Work> {
    let params = discover_params(cat, f);
    let scope = format!("tmdb:{}", cat.key());
    let mut max_page = 15u32;
    for _ in 0..3 {
        let page = rand::random_range(1..=max_page);
        let mut list: Vec<Work> = tmdb
            .discover(cat.kind(), &params, page)
            .await
            .into_iter()
            .filter(|w| cat.accepts(w, overrides) && f.light_match(w))
            .collect();
        if list.is_empty() {
            max_page = (max_page / 3).max(1);
            continue;
        }
        // Shuffle so the pick is random but still prefers an unseen entry.
        for i in (1..list.len()).rev() {
            list.swap(i, rand::random_range(0..=i));
        }
        let ids: Vec<(i64, String)> = list.iter().map(|w| (w.id, w.key())).collect();
        let pick = ids
            .iter()
            .find(|(_, k)| !crate::commands::picks::seen(user, &scope, k))
            .or(ids.first())?;
        if let Some(full) = tmdb.details(cat.kind(), pick.0).await {
            crate::commands::picks::remember(user, &scope, &pick.1);
            return Some(full);
        }
    }
    None
}

/// `n` distinct random works of the category from one discover page, each with a title and a poster: the answer
/// and its decoys for the event rounds.
pub async fn random_set(
    tmdb: &Tmdb,
    cat: Cat,
    overrides: &HashMap<String, Target>,
    n: usize,
) -> Option<Vec<Work>> {
    let params = discover_params(cat, &Filter::default());
    let mut max_page = 15u32;
    for _ in 0..3 {
        let page = rand::random_range(1..=max_page);
        let mut list: Vec<Work> = tmdb
            .discover(cat.kind(), &params, page)
            .await
            .into_iter()
            .filter(|w| cat.accepts(w, overrides) && w.poster.is_some() && !w.title.is_empty())
            .collect();
        if list.len() < n {
            max_page = (max_page / 3).max(1);
            continue;
        }
        for i in (1..list.len()).rev() {
            list.swap(i, rand::random_range(0..=i));
        }
        list.truncate(n);
        return Some(list);
    }
    None
}

/// A random cast entry of a random work: `(work, index into work.cast)`.
pub async fn random_cast(
    tmdb: &Tmdb,
    cat: Cat,
    f: &Filter,
    overrides: &HashMap<String, Target>,
    user: i64,
) -> Option<(Work, usize)> {
    for _ in 0..3 {
        let w = random(tmdb, cat, f, overrides, user).await?;
        if !w.cast.is_empty() {
            let i = rand::random_range(0..w.cast.len().min(15));
            return Some((w, i));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmdb::model::Work;

    fn show(id: i64, countries: &[&str], year: i32) -> Work {
        Work {
            id,
            kind: Some(Kind::Tv),
            countries: countries.iter().map(|c| (*c).to_string()).collect(),
            year: Some(year),
            ..Work::default()
        }
    }

    #[test]
    fn dramas_and_series_split_by_origin_and_the_server_override_moves_a_country() {
        let none = HashMap::new();
        let kr = show(1, &["KR"], 2020);
        let us = show(2, &["US"], 2020);
        assert!(Cat::Drama.accepts(&kr, &none) && !Cat::Series.accepts(&kr, &none));
        assert!(Cat::Series.accepts(&us, &none) && !Cat::Drama.accepts(&us, &none));
        let over = HashMap::from([
            ("KR".to_string(), Target::Series),
            ("US".to_string(), Target::Drama),
        ]);
        assert!(Cat::Series.accepts(&kr, &over) && Cat::Drama.accepts(&us, &over));
        let film = Work {
            kind: Some(Kind::Movie),
            ..Work::default()
        };
        assert!(
            Cat::Movie.accepts(&film, &none)
                && !Cat::Drama.accepts(&film, &none)
                && !Cat::Series.accepts(&film, &none)
        );
        assert!(!Cat::Movie.accepts(&kr, &none));
    }

    #[test]
    fn filters_check_year_country_status_and_genre() {
        let mut w = show(1, &["KR"], 2020);
        w.status = Some("Ended".into());
        w.genres = vec!["Drama".into(), "Sci-Fi & Fantasy".into()];
        let f = Filter {
            year: Some(2020),
            country: Some("KR".into()),
            status: Some("منتهٍ".into()),
            genre: Some("فانتازيا".into()),
        };
        assert!(f.full_match(&w));
        assert!(!Filter {
            year: Some(2019),
            ..f.clone()
        }
        .full_match(&w));
        assert!(!Filter {
            country: Some("JP".into()),
            ..f.clone()
        }
        .full_match(&w));
        assert!(!Filter {
            status: Some("مستمر".into()),
            ..f.clone()
        }
        .full_match(&w));
        assert!(!Filter {
            genre: Some("رعب".into()),
            ..f.clone()
        }
        .full_match(&w));
        assert!(
            Filter::default().full_match(&w),
            "no filter keeps everything"
        );
        assert!(
            f.needs_details()
                && !Filter {
                    year: Some(1),
                    ..Filter::default()
                }
                .needs_details()
        );
    }

    #[test]
    fn discover_params_follow_the_command_and_filters() {
        let p = discover_params(Cat::Drama, &Filter::default());
        assert!(
            p.iter()
                .any(|(k, v)| *k == "with_origin_country" && v.contains("KR|JP")),
            "dramas default to Asian origins"
        );
        let p = discover_params(
            Cat::Series,
            &Filter {
                year: Some(2019),
                genre: Some("دراما".into()),
                status: Some("منتهٍ".into()),
                country: Some("US".into()),
            },
        );
        for want in [
            ("first_air_date_year", "2019"),
            ("with_genres", "18"),
            ("with_status", "3"),
            ("with_origin_country", "US"),
        ] {
            assert!(
                p.iter().any(|(k, v)| *k == want.0 && v == want.1),
                "{want:?} in {p:?}"
            );
        }
        let p = discover_params(
            Cat::Movie,
            &Filter {
                year: Some(2001),
                genre: Some("رعب".into()),
                ..Filter::default()
            },
        );
        assert!(
            p.iter()
                .any(|(k, v)| *k == "primary_release_year" && v == "2001")
                && p.iter().any(|(k, v)| *k == "with_genres" && v == "27")
        );
        assert!(
            !p.iter().any(|(k, _)| *k == "with_status"),
            "films have no status filter"
        );
    }
}
