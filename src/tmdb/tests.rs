use super::model::*;
use serde_json::json;
use std::collections::HashMap;

fn tv_doc() -> serde_json::Value {
    json!({
        "id": 1396, "name": "Crash Landing on You", "original_name": "사랑의 불시착", "first_air_date": "2019-12-14",
        "overview": "A paraglider lands in North Korea.", "poster_path": "/p.jpg", "backdrop_path": "/b.jpg",
        "vote_average": 8.7, "vote_count": 1500, "popularity": 120.5, "adult": false, "origin_country": ["KR"],
        "genres": [{"id": 18, "name": "Drama"}, {"id": 10749, "name": "Romance"}],
        "status": "Ended", "number_of_seasons": 1, "number_of_episodes": 16, "episode_run_time": [70],
        "next_episode_to_air": {"air_date": "2026-12-01"},
        "networks": [{"name": "tvN"}],
        "keywords": {"results": [{"name": "north korea"}, {"name": "romance"}]},
        "content_ratings": {"results": [{"iso_3166_1": "US", "rating": "TV-14"}, {"iso_3166_1": "KR", "rating": "15"}]},
        "watch/providers": {"results": {"OM": {"flatrate": [{"provider_name": "Netflix"}], "rent": [{"provider_name": "Apple TV"}]}, "US": {"buy": [{"provider_name": "Amazon"}]}}},
        "aggregate_credits": {"cast": [
            {"id": 11, "name": "Hyun Bin", "profile_path": "/hb.jpg", "popularity": 30.2, "roles": [{"character": "Ri Jeong-hyeok"}]},
            {"id": 12, "name": "Son Ye-jin", "profile_path": null, "popularity": 25.0, "roles": [{"character": ""}]},
            {"id": 13, "name": "Hidden Person", "adult": true, "roles": [{"character": "X"}]}
        ]}
    })
}

#[test]
fn a_tv_show_is_parsed_with_cast_providers_and_next_episode() {
    let w = parse_tv(&tv_doc(), "OM");
    assert_eq!((w.id, w.kind), (1396, Some(Kind::Tv)));
    assert_eq!(w.key(), "tmdb-tv:1396");
    assert_eq!(
        (w.title.as_str(), w.original_title.as_str(), w.year),
        ("Crash Landing on You", "사랑의 불시착", Some(2019))
    );
    assert_eq!(
        w.poster.as_deref(),
        Some("https://image.tmdb.org/t/p/w500/p.jpg")
    );
    assert_eq!(
        (w.seasons, w.episodes, w.runtime),
        (Some(1), Some(16), Some(70))
    );
    assert_eq!(w.next_air.as_deref(), Some("2026-12-01"));
    assert_eq!(w.networks, vec!["tvN"]);
    assert_eq!(w.genres, vec!["Drama", "Romance"]);
    assert_eq!((w.rating, w.votes), (Some(8.7), Some(1500)));
    assert!(!w.blocked());
    assert_eq!(w.providers.flatrate, vec!["Netflix"]);
    assert_eq!(w.providers.rent, vec!["Apple TV"]);
    assert!(w.providers.buy.is_empty(), "only the requested region");
    assert!(
        parse_tv(&tv_doc(), "ZZ").providers.is_empty(),
        "no data for a region: empty, not an error"
    );
}

#[test]
fn cast_entries_map_to_character_cards_with_actor_photos() {
    let w = parse_tv(&tv_doc(), "OM");
    assert_eq!(w.cast.len(), 2, "people flagged adult are hidden");
    assert_eq!(w.cast[0].display_name(), "Ri Jeong-hyeok");
    assert_eq!(w.cast[0].actor, "Hyun Bin");
    assert_eq!(
        w.cast[0].photo.as_deref(),
        Some("https://image.tmdb.org/t/p/h632/hb.jpg")
    );
    assert_eq!(w.cast[0].key(Kind::Tv, 1396), "tmdb-cast:tv:1396:11");
    assert_eq!(
        w.cast[1].display_name(),
        "Son Ye-jin",
        "no character name: the actor's name"
    );
    assert_eq!(
        parse_cast_key("tmdb-cast:tv:1396:11"),
        Some((Kind::Tv, 1396, 11))
    );
    assert_eq!(
        parse_cast_key("tmdb-cast:movie:5:6"),
        Some((Kind::Movie, 5, 6))
    );
    assert_eq!(parse_cast_key("tmdb-cast:book:5:6"), None);
    assert_eq!(parse_work_key("tmdb-movie:77"), Some((Kind::Movie, 77)));
    assert_eq!(parse_work_key("anilist:77"), None);
}

#[test]
fn movies_use_the_region_release_date_and_collect_certifications() {
    let doc = json!({
        "id": 5, "title": "Film", "original_title": "Film", "release_date": "2026-06-01", "runtime": 118, "adult": false,
        "release_dates": {"results": [
            {"iso_3166_1": "US", "release_dates": [{"release_date": "2026-06-01T00:00:00.000Z", "certification": "PG-13"}]},
            {"iso_3166_1": "OM", "release_dates": [{"release_date": "2026-06-20T00:00:00.000Z", "certification": ""}, {"release_date": "2026-06-10T00:00:00.000Z", "certification": "PG"}]}
        ]},
        "keywords": {"keywords": [{"name": "space"}]}, "credits": {"cast": [{"id": 1, "name": "Actor", "character": "Hero"}]}
    });
    let w = parse_movie(&doc, "OM");
    assert_eq!(
        w.release_date.as_deref(),
        Some("2026-06-10"),
        "the region's earliest date"
    );
    assert_eq!(
        parse_movie(&doc, "FR").release_date.as_deref(),
        Some("2026-06-01"),
        "global date when the region has none"
    );
    assert_eq!((w.runtime, w.certifications.len()), (Some(118), 2));
    assert_eq!(w.cast[0].display_name(), "Hero");
}

#[test]
fn adult_flagged_keyworded_or_nc17_results_never_come_out() {
    let list = json!({"results": [
        {"id": 1, "name": "Clean", "adult": false},
        {"id": 2, "name": "Flagged", "adult": true},
        {"id": 3, "overview": "no title"},
        {"id": 0, "name": "Zero id"}
    ]});
    let w = parse_list(&list, Kind::Tv);
    assert_eq!(w.iter().map(|w| w.id).collect::<Vec<_>>(), vec![1]);
    let mut doc = tv_doc();
    doc["keywords"] = json!({"results": [{"name": "softcore"}]});
    assert!(parse_tv(&doc, "OM").blocked(), "blacklisted keyword");
    let mut doc = tv_doc();
    doc["content_ratings"] = json!({"results": [{"iso_3166_1": "US", "rating": "NC-17"}]});
    assert!(parse_tv(&doc, "OM").blocked(), "NC-17");
    assert!(!parse_tv(&tv_doc(), "OM").blocked(), "a clean item");
}

#[test]
fn people_flagged_adult_are_hidden() {
    let doc = json!({"results": [{"id": 1, "name": "A", "adult": false, "popularity": 3.0}, {"id": 2, "name": "B", "adult": true}]});
    let p = parse_people(&doc);
    assert_eq!(p.len(), 1);
    assert_eq!(
        p[0].display_name(),
        "A",
        "an actor entry shows the actor's name"
    );
}

#[test]
fn dramas_and_series_are_separated_with_admin_overrides() {
    let none = HashMap::new();
    let c = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    assert_eq!(classify(&c(&["KR"]), &none), Target::Drama);
    assert_eq!(classify(&c(&["JP", "US"]), &none), Target::Drama);
    assert_eq!(classify(&c(&["US"]), &none), Target::Series);
    assert_eq!(
        classify(&c(&["TR"]), &none),
        Target::Series,
        "Turkish dramas are series unless moved"
    );
    assert_eq!(
        classify(&c(&["IN"]), &none),
        Target::Series,
        "India is not in the default list"
    );
    assert_eq!(classify(&[], &none), Target::Series);
    let moved = HashMap::from([
        ("TR".to_string(), Target::Drama),
        ("KR".to_string(), Target::Series),
    ]);
    assert_eq!(
        classify(&c(&["TR"]), &moved),
        Target::Drama,
        "an admin moved Turkey to /drama"
    );
    assert_eq!(
        classify(&c(&["KR"]), &moved),
        Target::Series,
        "and Korea to /series"
    );
    // Exactly one command: the two classifications are complementary.
    for origin in [c(&["KR"]), c(&["US"]), c(&["TR", "KR"])] {
        let t = classify(&origin, &moved);
        assert!(t == Target::Drama || t == Target::Series);
    }
}

#[test]
fn retries_after_429_honour_retry_after_within_bounds() {
    assert_eq!(super::retry_wait(Some("3")), 3);
    assert_eq!(super::retry_wait(Some("999")), 10);
    assert_eq!(super::retry_wait(Some("0")), 1);
    assert_eq!(super::retry_wait(Some("soon")), 2);
    assert_eq!(super::retry_wait(None), 2);
}

#[test]
fn attribution_texts_are_the_required_ones() {
    assert_eq!(
        super::ATTRIBUTION_AR,
        "هذا المنتج يستخدم TMDB API لكنه غير معتمد أو مصدّق من TMDB."
    );
    assert_eq!(
        super::ATTRIBUTION_EN,
        "This product uses the TMDB API but is not endorsed or certified by TMDB."
    );
    assert_eq!(super::JUSTWATCH_CREDIT, "Streaming data by JustWatch");
    assert!(super::FOOTER_CREDIT.contains("TMDB"));
}

#[tokio::test]
async fn without_a_key_every_call_answers_nothing() {
    let t = super::Tmdb::new(
        reqwest::Client::new(),
        super::Config {
            region: "OM".into(),
            rate_per_10s: 30,
            ..Default::default()
        },
    );
    assert!(!t.enabled());
    assert!(t.search(Kind::Tv, "x", 1).await.is_empty());
    assert!(t.details(Kind::Movie, 1).await.is_none());
    assert!(t.discover(Kind::Tv, &[], 1).await.is_empty());
}

#[test]
fn cast_ids_round_trip_through_the_integer_character_space() {
    use crate::tmdb::model::{decode_cast_id, encode_cast_id, CAST_ID_BASE};
    for (k, w, p) in [
        (Kind::Tv, 1, 1),
        (Kind::Movie, 4_000_000, 33_000_000),
        (Kind::Tv, 12_345, 987_654),
    ] {
        let id = encode_cast_id(k, w, p).expect("fits");
        assert!(id >= CAST_ID_BASE);
        assert_eq!(decode_cast_id(id), Some((k, w, p)));
    }
    assert_ne!(
        encode_cast_id(Kind::Tv, 5, 6),
        encode_cast_id(Kind::Movie, 5, 6)
    );
    assert!(
        encode_cast_id(Kind::Tv, 1 << 22, 1).is_none(),
        "too large is refused, not truncated"
    );
    assert!(decode_cast_id(5).is_none() && decode_cast_id(2_500_000_000_000).is_none());
}
