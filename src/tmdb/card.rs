//! TMDB cards: a work card (poster, status, counts, rating, where to watch) and a cast card (the actor's photo,
//! clearly labelled, since TMDB has no character art). Footers carry the TMDB credit; the JustWatch credit goes with
//! the "where to watch" data.

use super::{
    model::{Cast, Kind, Providers, Work},
    FOOTER_CREDIT, JUSTWATCH_CREDIT,
};
use crate::{
    i18n,
    ui::{
        embeds::{fit, Emb},
        rating,
    },
};

/// `YYYY-MM-DD` for people: kept as is (TMDB gives dates, not times).
fn status_ar(status: Option<&str>, kind: Kind) -> &'static str {
    match (status.unwrap_or(""), kind) {
        ("Returning Series", _) => "مستمر",
        ("Ended", _) => "منتهٍ",
        ("Canceled", _) => "ملغي",
        ("In Production" | "Planned" | "Post Production", Kind::Tv) => "قيد الإنتاج",
        ("Released", _) => "متاح",
        ("In Production" | "Planned" | "Post Production", Kind::Movie) => "قريبًا",
        ("Rumored", _) => "إشاعة",
        _ => "—",
    }
}

/// At most six providers are shown, subscriptions first.
pub const MAX_PROVIDERS: usize = 6;

fn providers_text(p: &Providers, region: &str) -> String {
    let mut budget = MAX_PROVIDERS;
    let mut cap = |list: &Vec<String>| {
        let take = list.len().min(budget);
        budget -= take;
        list[..take].to_vec()
    };
    let p = &Providers {
        flatrate: cap(&p.flatrate),
        rent: cap(&p.rent),
        buy: cap(&p.buy),
    };
    if p.is_empty() {
        return format!("لا توجد بيانات مشاهدة لمنطقة {region} حاليًا.");
    }
    let mut lines = Vec::new();
    for (label, list) in [
        ("اشتراك", &p.flatrate),
        ("إيجار", &p.rent),
        ("شراء", &p.buy),
    ] {
        if !list.is_empty() {
            lines.push(format!("**{label}:** {}", list.join("، ")));
        }
    }
    lines.join("\n")
}

/// Trim an overview to a safe length, optionally wrapped in spoiler markup.
pub fn overview(text: &str, spoiler: bool) -> String {
    let t = fit(text.trim(), 350);
    if t.is_empty() {
        return String::new();
    }
    if spoiler && text.chars().count() > 350 {
        format!("||{}||", t.replace("||", ""))
    } else {
        t
    }
}

pub fn work_embed(w: &Work, region: &str, spoiler: bool) -> Emb {
    let kind = w.kind.unwrap_or(Kind::Tv);
    let mut title = w.title.clone();
    if !w.original_title.is_empty() && w.original_title != w.title {
        title = format!("{title} ({})", w.original_title);
    }
    if let Some(y) = w.year {
        title = format!("{title} · {y}");
    }
    let mut e = Emb::new().title(fit(&title, 250)).url(Some(w.url()));
    let d = overview(&w.overview, spoiler);
    if !d.is_empty() {
        e = e.description(d);
    }
    e = e.field(i18n::F_STATUS, status_ar(w.status.as_deref(), kind), true);
    match kind {
        Kind::Tv => {
            if let (Some(s), Some(n)) = (w.seasons, w.episodes) {
                e = e.field(
                    i18n::F_SEASONS,
                    format!("{s} · {n} {}", i18n::F_EPISODES),
                    true,
                );
            }
            if let Some(d) = &w.next_air {
                e = e.field(i18n::F_NEXT_EPISODE, d, true);
            }
            if !w.networks.is_empty() {
                e = e.field(
                    i18n::F_NETWORK,
                    w.networks
                        .iter()
                        .take(2)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("، "),
                    true,
                );
            }
        }
        Kind::Movie => {
            if let Some(d) = &w.release_date {
                e = e.field(i18n::F_RELEASE, d, true);
            }
            if let Some(r) = w.runtime {
                e = e.field(i18n::F_RUNTIME, format!("{r} {}", i18n::MINUTES), true);
            }
        }
    }
    if !w.countries.is_empty() {
        e = e.field(
            i18n::F_COUNTRY,
            w.countries
                .iter()
                .map(|c| super::lookup::country_name(c))
                .collect::<Vec<_>>()
                .join("، "),
            true,
        );
    }
    if !w.genres.is_empty() {
        e = e.field(
            i18n::F_GENRES,
            w.genres
                .iter()
                .take(4)
                .cloned()
                .collect::<Vec<_>>()
                .join(" • "),
            false,
        );
    }
    e = e.field(
        i18n::F_STATS,
        rating::tmdb_work(w.rating, w.votes, w.popularity),
        false,
    );
    // "Where to watch" is only shown when the details were loaded (a search hit has no provider data).
    if !w.providers.is_empty() || !w.cast.is_empty() {
        e = e.field(
            format!("{} ({JUSTWATCH_CREDIT})", i18n::F_WHERE_TO_WATCH),
            providers_text(&w.providers, region),
            false,
        );
    }
    e.image = w.poster.clone();
    e.footer = Some(FOOTER_CREDIT.to_string());
    e
}

/// A cast entry as a "character": the actor's photo is labelled as the actor's.
pub fn cast_embed(c: &Cast, work_title: &str, work_url: &str) -> Emb {
    let mut e = Emb::new()
        .title(fit(c.display_name(), 250))
        .url(Some(work_url.to_string()));
    let mut lines = vec![
        format!("{} **{}**", i18n::PLAYED_BY, c.actor),
        format!("📺 [{work_title}]({work_url})"),
        rating::tmdb_person(c.popularity),
    ];
    lines.push(i18n::ACTOR_PHOTO_NOTE.to_string());
    e = e.description(lines.join("\n"));
    e.image = c.photo.clone();
    e.footer = Some(FOOTER_CREDIT.to_string());
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmdb::model::{parse_tv, Providers};
    use serde_json::json;

    fn show() -> Work {
        parse_tv(
            &json!({"id": 1, "name": "Show", "original_name": "원제", "first_air_date": "2020-01-01", "overview": "x".repeat(500), "poster_path": "/p.jpg",
                "vote_average": 8.0, "vote_count": 10, "popularity": 5.0, "status": "Returning Series", "number_of_seasons": 2, "number_of_episodes": 24,
                "origin_country": ["KR"], "genres": [{"name": "Drama"}], "networks": [{"name": "tvN"}],
                "watch/providers": {"results": {"OM": {"flatrate": [{"provider_name": "Netflix"}]}}},
                "aggregate_credits": {"cast": [{"id": 9, "name": "Actor", "roles": [{"character": "Hero"}]}]}}),
            "OM",
        )
    }

    #[test]
    fn the_work_card_has_the_tmdb_credit_poster_stats_and_the_justwatch_credit() {
        let e = work_embed(&show(), "OM", false);
        assert_eq!(e.footer.as_deref(), Some("المصدر: TMDB"));
        assert_eq!(
            e.image.as_deref(),
            Some("https://image.tmdb.org/t/p/w500/p.jpg")
        );
        let titles: Vec<&str> = e.fields.iter().map(|f| f.0.as_str()).collect();
        assert!(
            titles
                .iter()
                .any(|t| t.contains("Streaming data by JustWatch")),
            "{titles:?}"
        );
        let stats = e
            .fields
            .iter()
            .find(|f| f.0 == i18n::F_STATS)
            .expect("stats");
        assert!(stats.1.contains("8.0/10") && stats.1.contains("الشعبية"));
        assert!(
            e.title.as_deref().unwrap_or("").contains("원제")
                && e.title.as_deref().unwrap_or("").contains("2020")
        );
    }

    #[test]
    fn at_most_six_providers_are_listed_subscriptions_first() {
        let p = Providers {
            flatrate: vec!["A".into(), "B".into(), "C".into(), "D".into()],
            rent: vec!["E".into(), "F".into(), "G".into()],
            buy: vec!["H".into()],
        };
        let t = providers_text(&p, "OM");
        for shown in ["A", "B", "C", "D", "E", "F"] {
            assert!(t.contains(shown), "{shown} in {t}");
        }
        assert!(!t.contains('G') && !t.contains('H'), "{t}");
    }

    #[test]
    fn a_region_without_data_says_so_politely() {
        assert!(providers_text(&Providers::default(), "OM").contains("OM"));
        let mut w = show();
        w.providers = Providers::default();
        let e = work_embed(&w, "OM", false);
        assert!(e.fields.iter().any(|f| f.1.contains("لا توجد بيانات")));
    }

    #[test]
    fn long_overviews_are_cut_and_can_be_spoiler_wrapped() {
        let long = "word ".repeat(200);
        let plain = overview(&long, false);
        assert!(plain.chars().count() <= 350 && !plain.starts_with("||"));
        let hidden = overview(&long, true);
        assert!(hidden.starts_with("||") && hidden.ends_with("||"));
        assert_eq!(
            overview("short", true),
            "short",
            "short text is never hidden"
        );
        assert_eq!(overview("  ", true), "");
    }

    #[test]
    fn the_cast_card_names_the_actor_and_labels_the_photo() {
        let w = show();
        let e = cast_embed(&w.cast[0], &w.title, &w.url());
        assert_eq!(e.title.as_deref(), Some("Hero"));
        let d = e.description.as_deref().unwrap_or("");
        assert!(d.contains("Actor") && d.contains(i18n::ACTOR_PHOTO_NOTE) && d.contains("الشعبية"));
        assert_eq!(e.footer.as_deref(), Some("المصدر: TMDB"));
    }
}
