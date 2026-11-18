//! One display model for rating and popularity, whatever the source: the same wording and order everywhere, "—" when
//! a source has no value.

use super::format::thousands;

pub const NONE: &str = "—";

fn count(n: Option<i64>) -> String {
    n.filter(|n| *n > 0)
        .map_or_else(|| NONE.to_string(), thousands)
}

/// A score out of 100 as `85/100 (8.5/10)`.
fn out_of_100(score: Option<f64>) -> String {
    match score.filter(|s| *s > 0.0) {
        Some(s) => format!("{}/100 ({:.1}/10)", s.round() as i64, s / 10.0),
        None => NONE.to_string(),
    }
}

/// AniList work: score, members (popularity) and favourites.
pub fn anilist_work(
    score: Option<i32>,
    popularity: Option<i64>,
    favourites: Option<i64>,
) -> String {
    format!(
        "التقييم: {} · الشعبية: {} · المفضلة: {}",
        out_of_100(score.map(f64::from)),
        count(popularity),
        count(favourites)
    )
}

/// AniList character: how many members love them.
pub fn anilist_character(favourites: Option<i64>) -> String {
    format!("المحبون: {}", count(favourites))
}

/// TMDB movie or show: `vote_average` out of 10 with the vote count, and TMDB's popularity score.
pub fn tmdb_work(rating: Option<f32>, votes: Option<i64>, popularity: Option<f64>) -> String {
    let r = match rating.filter(|r| *r > 0.0) {
        Some(r) => match votes.filter(|v| *v > 0) {
            Some(v) => format!("{r:.1}/10 ({} صوت)", thousands(v)),
            None => format!("{r:.1}/10"),
        },
        None => NONE.to_string(),
    };
    let p = popularity
        .filter(|p| *p > 0.0)
        .map_or_else(|| NONE.to_string(), |p| format!("{p:.1}"));
    format!("التقييم: {r} · الشعبية: {p}")
}

/// TMDB person or cast entry.
pub fn tmdb_person(popularity: Option<f64>) -> String {
    format!(
        "الشعبية: {}",
        popularity
            .filter(|p| *p > 0.0)
            .map_or_else(|| NONE.to_string(), |p| format!("{p:.1}"))
    )
}

/// IGDB game: `total_rating` (0-100) with its count, plus follows when present.
pub fn igdb_game(rating: Option<f64>, rating_count: Option<i64>, follows: Option<i64>) -> String {
    let r = match (rating.filter(|r| *r > 0.0), rating_count.filter(|c| *c > 0)) {
        (Some(r), Some(c)) => format!("{}/100 ({} تقييم)", r.round() as i64, thousands(c)),
        (Some(r), None) => format!("{}/100", r.round() as i64),
        _ => NONE.to_string(),
    };
    format!("التقييم: {r} · المتابعون: {}", count(follows))
}

/// VNDB: rating (0-100) with the vote count.
pub fn vndb(rating: Option<f64>, votes: Option<i64>) -> String {
    let r = match (rating.filter(|r| *r > 0.0), votes.filter(|v| *v > 0)) {
        (Some(r), Some(v)) => format!("{}/100 ({} صوت)", r.round() as i64, thousands(v)),
        (Some(r), None) => format!("{}/100", r.round() as i64),
        _ => NONE.to_string(),
    };
    format!("التقييم: {r}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anilist_work_shows_score_both_scales_popularity_and_favourites() {
        assert_eq!(
            anilist_work(Some(85), Some(245_310), Some(18_402)),
            "التقييم: 85/100 (8.5/10) · الشعبية: 245,310 · المفضلة: 18,402"
        );
        assert_eq!(
            anilist_work(None, None, None),
            "التقييم: — · الشعبية: — · المفضلة: —"
        );
        assert_eq!(
            anilist_work(Some(0), Some(0), None),
            "التقييم: — · الشعبية: — · المفضلة: —",
            "zero is no value"
        );
    }

    #[test]
    fn anilist_characters_show_who_loves_them() {
        assert_eq!(anilist_character(Some(4_755)), "المحبون: 4,755");
        assert_eq!(anilist_character(None), "المحبون: —");
    }

    #[test]
    fn tmdb_shows_vote_average_with_count_and_the_popularity_score() {
        assert_eq!(
            tmdb_work(Some(8.71), Some(1500), Some(120.54)),
            "التقييم: 8.7/10 (1,500 صوت) · الشعبية: 120.5"
        );
        assert_eq!(
            tmdb_work(Some(7.0), None, None),
            "التقييم: 7.0/10 · الشعبية: —"
        );
        assert_eq!(tmdb_work(None, None, None), "التقييم: — · الشعبية: —");
        assert_eq!(tmdb_person(Some(30.24)), "الشعبية: 30.2");
        assert_eq!(tmdb_person(None), "الشعبية: —");
    }

    #[test]
    fn games_and_visual_novels_normalise_their_scales() {
        assert_eq!(
            igdb_game(Some(84.6), Some(230), Some(1200)),
            "التقييم: 85/100 (230 تقييم) · المتابعون: 1,200"
        );
        assert_eq!(igdb_game(None, None, None), "التقييم: — · المتابعون: —");
        assert_eq!(vndb(Some(82.0), Some(5000)), "التقييم: 82/100 (5,000 صوت)");
        assert_eq!(vndb(None, None), "التقييم: —");
    }
}
