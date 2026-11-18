//! Event rounds about dramas, series and films (TMDB): poster, rating/popularity and release year. TMDB has no
//! character pictures or descriptions, so those two modes are not offered for these categories.

use super::arena::{order_options, Difficulty, Hint, Mode, Round, TmdbPool};
use crate::{
    components::ItemKind,
    i18n,
    tmdb::{find, model::Work},
};

fn year_statement(w: &Work, lie: bool) -> Option<(String, bool)> {
    let year = w.year?;
    let shown = if lie {
        year + [-3, -2, -1, 1, 2, 3][rand::random_range(0..6)]
    } else {
        year
    };
    Some((format!("عُرض «{}» لأول مرة عام {shown}", w.title), !lie))
}

fn work_cover(set: &[Work], d: Difficulty) -> Option<Round> {
    let answer = set.first()?;
    let names: Vec<String> = set.iter().skip(1).map(|w| w.title.clone()).collect();
    let (options, correct) = super::event::shuffle_options(answer.title.clone(), names);
    let hints = (0..d.start_level()).rev().map(Hint::Picture).collect();
    Some(Round {
        title: i18n::ARENA_WORK_COVER.into(),
        prompt: i18n::ARENA_WHICH_WORK.into(),
        picture: Some((answer.poster.clone()?, rand::random())),
        level: d.start_level(),
        options,
        correct,
        hints,
        item: Some((ItemKind::Work, answer.key())),
        reveal: format!(
            "✅ **{}**{}",
            answer.title,
            answer.year.map(|y| format!(" ({y})")).unwrap_or_default()
        ),
    })
}

fn true_false(set: &[Work]) -> Option<Round> {
    let (text, truth) = year_statement(set.first()?, rand::random::<bool>())?;
    Some(Round {
        title: i18n::ARENA_TRUE_FALSE.into(),
        prompt: text,
        picture: None,
        level: 0,
        options: vec![i18n::ARENA_TRUE.into(), i18n::ARENA_FALSE.into()],
        correct: if truth { 0 } else { 1 },
        hints: Vec::new(),
        item: set.first().map(|w| (ItemKind::Work, w.key())),
        reveal: format!(
            "{} **{}**",
            i18n::ARENA_ANSWER_WAS,
            if truth {
                i18n::ARENA_TRUE
            } else {
                i18n::ARENA_FALSE
            }
        ),
    })
}

/// Which of two works has the higher rating or the higher popularity.
fn higher_lower(set: &[Work]) -> Option<Round> {
    let (a, b) = (set.first()?, set.get(1)?);
    let by_rating = rand::random::<bool>();
    let metric = |w: &Work| {
        if by_rating {
            w.rating.map(|r| (r * 10.0).round() as i64)
        } else {
            w.popularity.map(|p| p.round() as i64)
        }
    };
    let (x, y) = (metric(a)?, metric(b)?);
    if x == y {
        return None;
    }
    let (options, at) = super::event::shuffle_options(a.title.clone(), vec![b.title.clone()]);
    let right = if x > y { at } else { 1 - at };
    let show = |n: i64| {
        if by_rating {
            format!("{:.1}", n as f32 / 10.0)
        } else {
            n.to_string()
        }
    };
    Some(Round {
        title: i18n::ARENA_HIGHER_LOWER.into(),
        prompt: (if by_rating {
            i18n::ARENA_HIGHER_RATED
        } else {
            i18n::ARENA_MORE_POPULAR
        })
        .into(),
        picture: None,
        level: 0,
        options: options.clone(),
        correct: right,
        hints: Vec::new(),
        item: Some((ItemKind::Work, (if x > y { a } else { b }).key())),
        reveal: format!(
            "✅ **{}** ({}) · {} ({})",
            options[right],
            show(x.max(y)),
            options[1 - right],
            show(x.min(y))
        ),
    })
}

fn order_years(set: &[Work]) -> Option<Round> {
    let mut works: [Work; 3] = [
        set.first()?.clone(),
        set.get(1)?.clone(),
        set.get(2)?.clone(),
    ];
    let years: Vec<i32> = works.iter().filter_map(|w| w.year).collect();
    let distinct: std::collections::HashSet<i32> = years.iter().copied().collect();
    if years.len() != 3 || distinct.len() != 3 {
        return None;
    }
    for i in (1..3).rev() {
        works.swap(i, rand::random_range(0..=i));
    }
    let mut order = [0usize, 1, 2];
    order.sort_by_key(|i| works[*i].year.unwrap_or(0));
    let (options, correct) = order_options(order);
    let letters = ["أ", "ب", "ج"];
    let list = works
        .iter()
        .enumerate()
        .map(|(i, w)| format!("**{})** {}", letters[i], w.title))
        .collect::<Vec<_>>()
        .join("\n");
    let reveal = order
        .iter()
        .map(|i| format!("{} ({})", works[*i].title, works[*i].year.unwrap_or(0)))
        .collect::<Vec<_>>()
        .join(" ← ");
    Some(Round {
        title: i18n::ARENA_ORDER_YEARS.into(),
        prompt: format!("{}\n{list}", i18n::ARENA_ORDER_PROMPT),
        picture: None,
        level: 0,
        options,
        correct,
        hints: Vec::new(),
        item: None,
        reveal: format!("✅ {reveal}"),
    })
}

/// One round of `mode` for a TMDB category; retries because a discover page can come back thin.
pub async fn make_round(pool: &TmdbPool, mode: Mode, d: Difficulty) -> Option<Round> {
    for _ in 0..4 {
        let chosen = if mode == Mode::Race {
            [
                Mode::WorkCover,
                Mode::TrueFalse,
                Mode::HigherLower,
                Mode::OrderYears,
            ][rand::random_range(0..4)]
        } else {
            mode
        };
        let n = if chosen == Mode::WorkCover { 4 } else { 3 };
        let Some(set) = find::random_set(&pool.tmdb, pool.cat, &pool.overrides, n).await else {
            continue;
        };
        let round = match chosen {
            Mode::WorkCover => work_cover(&set, d),
            Mode::TrueFalse => true_false(&set),
            Mode::HigherLower => higher_lower(&set),
            Mode::OrderYears => order_years(&set),
            _ => None,
        };
        if round.is_some() {
            return round;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(title: &str, year: i32, rating: f32, pop: f64) -> Work {
        Work {
            id: 1,
            title: title.into(),
            year: Some(year),
            rating: Some(rating),
            popularity: Some(pop),
            poster: Some("https://image.tmdb.org/t/p/w500/p.jpg".into()),
            ..Work::default()
        }
    }

    #[test]
    fn the_cover_round_hides_the_answer_among_the_decoys_and_reveals_the_year() {
        let set = [
            w("A", 2020, 8.0, 5.0),
            w("B", 2019, 7.0, 4.0),
            w("C", 2018, 6.0, 3.0),
            w("D", 2017, 5.0, 2.0),
        ];
        let r = work_cover(&set, Difficulty::Normal).expect("round");
        assert_eq!(r.options.len(), 4);
        assert_eq!(r.options[r.correct], "A");
        assert!(r.picture.is_some() && r.reveal.contains("2020"));
    }

    #[test]
    fn higher_lower_names_the_work_with_the_bigger_number() {
        let set = [w("High", 2020, 9.0, 100.0), w("Low", 2020, 4.0, 1.0)];
        for _ in 0..20 {
            let r = higher_lower(&set).expect("round");
            assert_eq!(
                r.options[r.correct], "High",
                "both metrics favour the first work"
            );
        }
        assert!(
            higher_lower(&[w("A", 2020, 5.0, 1.0), w("B", 2020, 5.0, 1.0)]).is_none(),
            "a tie is not a question"
        );
    }

    #[test]
    fn the_order_round_needs_three_distinct_years_and_ranks_them() {
        let set = [
            w("New", 2022, 1.0, 1.0),
            w("Old", 2001, 1.0, 1.0),
            w("Mid", 2010, 1.0, 1.0),
        ];
        let r = order_years(&set).expect("round");
        assert!(
            r.reveal.find("Old").expect("old") < r.reveal.find("Mid").expect("mid")
                && r.reveal.find("Mid").expect("mid") < r.reveal.find("New").expect("new")
        );
        assert!(order_years(&[
            w("A", 2000, 1.0, 1.0),
            w("B", 2000, 1.0, 1.0),
            w("C", 2001, 1.0, 1.0)
        ])
        .is_none());
    }

    #[test]
    fn true_false_states_the_real_or_a_shifted_year() {
        for _ in 0..30 {
            let r = true_false(&[w("A", 2015, 1.0, 1.0)]).expect("round");
            let says_2015 = r.prompt.contains("2015");
            assert_eq!(
                r.correct == 0,
                says_2015,
                "true exactly when the statement has the real year"
            );
        }
    }
}
