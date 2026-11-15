use super::*;
use serde_json::json;

fn dex_doc() -> serde_json::Value {
    json!({"data": [
        {"id": "aaa", "attributes": {"title": {"en": "Safe Manga"}, "contentRating": "safe", "year": 2020, "status": "ongoing",
          "tags": [{"attributes": {"group": "genre", "name": {"en": "Action"}}}, {"attributes": {"group": "theme", "name": {"en": "School"}}}]},
         "relationships": [{"type": "cover_art", "attributes": {"fileName": "c.jpg"}}]},
        {"id": "bbb", "attributes": {"title": {"ja": "提案"}, "contentRating": "suggestive", "tags": []}, "relationships": []},
        {"id": "ccc", "attributes": {"title": {"en": "Erotic One"}, "contentRating": "erotica", "tags": []}},
        {"id": "ddd", "attributes": {"title": {"en": "Porn One"}, "contentRating": "pornographic"}},
        {"id": "eee", "attributes": {"title": {"en": "No Rating"}}}
    ]})
}

#[test]
fn mangadex_only_safe_and_suggestive_are_kept() {
    let works = mangadex::parse_search(&dex_doc()).expect("parse");
    let ids: Vec<&str> = works.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["mdex:aaa", "mdex:bbb"],
        "erotica, pornographic and unrated works are dropped"
    );
    assert_eq!(works[0].title, "Safe Manga");
    assert_eq!(
        works[0].genres,
        vec!["Action"],
        "only genre tags are listed"
    );
    assert_eq!(
        works[0].url.as_deref(),
        Some("https://mangadex.org/title/aaa")
    );
    assert_eq!(
        works[1].title, "提案",
        "falls back to the first available title"
    );
    assert!(mangadex::parse_search(&json!({})).is_err());
}

#[test]
fn mangadex_covers_are_downloaded_never_hotlinked() {
    let works = mangadex::parse_search(&dex_doc()).expect("parse");
    let cover = works[0].cover_url.as_deref().expect("cover url");
    assert_eq!(
        cover,
        "https://uploads.mangadex.org/covers/aaa/c.jpg.256.jpg"
    );
    // The URL is only ever fetched by the bot; the embed uses an attachment (see commands::fallback).
    assert!(cover_host_allowed(cover));
    assert!(!cover_host_allowed("https://evil.example/covers/x.jpg"));
    assert!(!cover_host_allowed("http://uploads.mangadex.org/x.jpg"));
    assert!(!cover_host_allowed(
        "https://uploads.mangadex.org.evil.example/x.jpg"
    ));
}

fn mu_doc() -> serde_json::Value {
    json!({"results": [
        {"record": {"series_id": 11, "title": "Clean Series", "url": "https://www.mangaupdates.com/series/11", "year": "2019",
                    "bayesian_rating": 8.1, "image": {"url": {"original": "https://cdn.mangaupdates.com/image/i1.jpg"}},
                    "genres": [{"genre": "Action"}, {"genre": "Comedy"}]}},
        {"record": {"series_id": 12, "title": "Adult Series", "genres": [{"genre": "Adult"}, {"genre": "Drama"}]}},
        {"record": {"series_id": 13, "title": "Unknown Genres", "genres": []}}
    ]})
}

#[test]
fn mangaupdates_hides_adult_and_unverifiable_works() {
    let all = mangaupdates::parse_search(&mu_doc()).expect("parse");
    assert_eq!(
        all.len(),
        3,
        "parsing keeps everything; the safety filter decides"
    );
    let safe: Vec<&str> = all
        .iter()
        .filter(|w| safety::fallback_safe(w))
        .map(|w| w.id.as_str())
        .collect();
    assert_eq!(
        safe,
        vec!["mu:11"],
        "adult genres and works without genre data are hidden"
    );
    assert_eq!(all[0].year, Some(2019));
    assert_eq!(
        all[0].cover_url.as_deref(),
        Some("https://cdn.mangaupdates.com/image/i1.jpg")
    );
    assert!(cover_host_allowed(
        "https://cdn.mangaupdates.com/image/i1.jpg"
    ));
}

#[test]
fn credits_and_prefixes() {
    assert_eq!(Source::MangaDex.credit(), "Source: MangaDex");
    assert_eq!(Source::MangaUpdates.credit(), "Source: MangaUpdates");
    assert_eq!(
        (Source::MangaDex.prefix(), Source::MangaUpdates.prefix()),
        ("mdex", "mu")
    );
}

#[tokio::test]
async fn disabled_providers_make_no_requests_and_return_nothing() {
    let p = Providers::new(reqwest::Client::new(), ProviderConfig::default());
    assert!(!p.enabled());
    assert!(p.search("anything").await.is_empty());
    let w = Work {
        id: "mdex:x".into(),
        source: Source::MangaDex,
        kind: Kind::Manga,
        native: None,
        subtitle: None,
        title: "t".into(),
        year: None,
        status: None,
        rating: None,
        url: None,
        cover_url: Some("https://evil.example/c.jpg".into()),
        genres: vec!["Action".into()],
    };
    assert!(
        p.cover(&w).await.is_none(),
        "a non-allow-listed host is never fetched"
    );
}

#[test]
fn image_sniffing() {
    assert!(looks_like_image(&[0xFF, 0xD8, 0xFF, 0xE0]));
    assert!(looks_like_image(b"\x89PNG\r\n"));
    assert!(!looks_like_image(b"<html>"));
}

// ---- IGDB / VNDB safety fixtures ----

fn igdb_game(
    id: i64,
    name: &str,
    themes: serde_json::Value,
    ratings: serde_json::Value,
) -> serde_json::Value {
    json!({"id": id, "name": name, "url": format!("https://www.igdb.com/games/g{id}"), "total_rating_count": 100,
           "cover": {"image_id": "co1abc"}, "themes": themes, "age_ratings": ratings})
}

fn ids(v: &[(Work, u64)]) -> Vec<String> {
    v.iter().map(|(w, _)| w.id.clone()).collect()
}

#[test]
fn igdb_hides_erotic_theme_and_ao_but_not_violent_m_rated_games() {
    let doc = json!([
        igdb_game(
            1,
            "Erotic Game",
            json!([{"id": 42, "name": "Erotic"}]),
            json!([{"category": 1, "rating": 10}])
        ),
        igdb_game(
            2,
            "AO Game",
            json!([]),
            json!([{"category": 1, "rating": 12}])
        ),
        igdb_game(
            3,
            "Silent Hill 2",
            json!([{"id": 19, "name": "Horror"}]),
            json!([{"category": 1, "rating": 11}, {"category": 2, "rating": 5}])
        ),
        igdb_game(
            4,
            "Teen Game",
            json!([]),
            json!([{"category": 1, "rating": 10}])
        ),
        igdb_game(5, "Unrated Game", json!([]), json!([])),
    ]);
    assert_eq!(
        ids(&igdb::parse_games(&doc, true)),
        vec!["igdb:3", "igdb:4"],
        "erotic, AO and unrated are hidden; M/PEGI 18 horror stays"
    );
    assert_eq!(
        ids(&igdb::parse_games(&doc, false)),
        vec!["igdb:4"],
        "mature-games off also hides M/PEGI 18"
    );
    let w = &igdb::parse_games(&doc, true)[1].0;
    assert_eq!(
        w.cover_url.as_deref(),
        Some("https://images.igdb.com/igdb/image/upload/t_cover_big/co1abc.jpg")
    );
    assert!(cover_host_allowed(w.cover_url.as_deref().unwrap_or("")));
}

#[test]
fn igdb_cero_z_counts_as_mature_and_odd_image_ids_are_dropped() {
    let doc = json!([igdb_game(
        7,
        "Z Game",
        json!([]),
        json!([{"category": 3, "rating": 5}])
    )]);
    assert_eq!(igdb::parse_games(&doc, true).len(), 1);
    assert!(igdb::parse_games(&doc, false).is_empty());
    let mut bad = igdb_game(8, "Odd", json!([]), json!([{"category": 1, "rating": 10}]));
    bad["cover"]["image_id"] = json!("../evil");
    assert_eq!(igdb::parse_games(&json!([bad]), true)[0].0.cover_url, None);
}

#[test]
fn igdb_characters_follow_their_games() {
    let g_ok = json!({"name": "Super Mario Bros.", "total_rating_count": 900, "themes": [], "age_ratings": [{"category": 1, "rating": 8}]});
    let g_bad = json!({"name": "Bad", "total_rating_count": 5, "themes": [{"id": 42}], "age_ratings": [{"category": 1, "rating": 10}]});
    let g_none =
        json!({"name": "Unrated", "total_rating_count": 5, "themes": [], "age_ratings": []});
    let doc = json!([
        {"id": 1, "name": "Mario", "games": [g_ok.clone()], "mug_shot": {"image_id": "m1"}},
        {"id": 2, "name": "Tainted", "games": [g_ok.clone(), g_bad]},
        {"id": 3, "name": "Nobody", "games": []},
        {"id": 4, "name": "Only Unrated", "games": [g_none.clone()]},
        {"id": 5, "name": "Mixed", "games": [g_none, g_ok]},
    ]);
    let found = igdb::parse_characters(&doc, true);
    assert_eq!(ids(&found), vec!["igdb:c1", "igdb:c5"]);
    assert_eq!(found[0].0.subtitle.as_deref(), Some("Super Mario Bros."));
    assert_eq!(found[0].0.character_id(), Some(2_000_000_000_001));
}

#[test]
fn igdb_queries_are_escaped() {
    assert_eq!(igdb::quote("a\"b\\c\nd"), "\"a\\\"b\\\\cd\"");
    assert_eq!(
        igdb::parse_games(&json!({}), true).len(),
        0,
        "not an array -> nothing"
    );
}

fn vn(id: &str, tags: serde_json::Value, sexual: serde_json::Value) -> serde_json::Value {
    json!({"id": id, "title": format!("VN {id}"), "alttitle": "原題", "released": "2016-05-01", "rating": 82.0,
           "image": {"url": "https://t.vndb.org/cv/aa/1.jpg", "sexual": sexual}, "tags": tags})
}

#[test]
fn vndb_hides_erotic_tags_flagged_images_and_unknown_verdicts() {
    let doc = json!({"results": [
        vn("v1", json!([{"category": "cont", "rating": 2.0}]), json!(0)),
        vn("v2", json!([{"category": "ero", "rating": 1.0}]), json!(0)),
        vn("v3", json!([{"category": "cont", "rating": 2.0}]), json!(2)),
        vn("v4", json!([]), json!(0)),
        {"id": "v5", "title": "No tags field"},
    ]});
    let found = vndb::parse_vns(&doc);
    assert_eq!(
        found.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
        vec!["vndb:v1"]
    );
    assert_eq!(found[0].year, Some(2016));
    assert_eq!(found[0].url.as_deref(), Some("https://vndb.org/v1"));
    assert!((found[0].rating.unwrap_or(0.0) - 8.2).abs() < 0.01);
    assert!(cover_host_allowed(
        found[0].cover_url.as_deref().unwrap_or("")
    ));
}

#[test]
fn vndb_characters_need_a_safe_picture_and_verified_novels() {
    let chars = json!({"results": [
        {"id": "c1", "name": "Ok", "original": "名", "image": {"url": "https://t.vndb.org/ch/a/1.jpg", "sexual": 0}, "vns": [{"id": "v1", "title": "VN v1"}]},
        {"id": "c2", "name": "Flagged picture", "image": {"sexual": 2}, "vns": [{"id": "v1", "title": "x"}]},
        {"id": "c3", "name": "Erotic novel", "image": {"sexual": 0}, "vns": [{"id": "v2", "title": "x"}]},
        {"id": "c4", "name": "No novels", "image": {"sexual": 0}, "vns": []},
        {"id": "c5", "name": "Unchecked novel", "image": {"sexual": 0}, "vns": [{"id": "v9", "title": "x"}]},
    ]});
    let vns = json!({"results": [
        vn("v1", json!([{"category": "cont", "rating": 1.0}]), json!(0)),
        vn("v2", json!([{"category": "ero", "rating": 1.0}]), json!(0)),
    ]});
    let verdicts = vndb::vn_verdicts(&vns);
    let found = vndb::parse_characters(&chars, &verdicts);
    assert_eq!(
        found.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
        vec!["vndb:c1"]
    );
    assert_eq!(found[0].subtitle.as_deref(), Some("VN v1"));
    assert_eq!(found[0].character_id(), Some(1_000_000_000_001));
}

#[test]
fn character_ids_round_trip_and_never_touch_anilist_ids() {
    for (s, n) in [(Source::Vndb, 5), (Source::Igdb, 77)] {
        let id = encode_character_id(s, n).expect("encodes");
        assert!(id >= PROVIDER_CHARACTER_MIN);
        assert_eq!(
            id - PROVIDER_CHARACTER_MIN * if s == Source::Igdb { 2 } else { 1 },
            n
        );
    }
    assert_eq!(encode_character_id(Source::MangaDex, 1), None);
}

#[test]
fn games_footers_and_providers_toggle() {
    assert_eq!(Source::Igdb.credit(), "Data from IGDB.com");
    assert_eq!(Source::Vndb.credit(), "Data from VNDB");
    let p = Providers::new(reqwest::Client::new(), ProviderConfig::default());
    assert!(!p.games_enabled() && !p.igdb_enabled() && !p.vndb_enabled());
    let p = Providers::new(
        reqwest::Client::new(),
        ProviderConfig {
            vndb: true,
            ..ProviderConfig::default()
        },
    );
    assert!(p.games_enabled());
}
