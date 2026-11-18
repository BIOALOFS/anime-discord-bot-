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
        work_key: None,
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

// ---- IGDB / VNDB safety fixtures (content based: themes, genres, keywords, tags, image flags) ----

fn igdb_game(
    id: i64,
    name: &str,
    themes: serde_json::Value,
    genres: serde_json::Value,
    keywords: serde_json::Value,
) -> serde_json::Value {
    json!({"id": id, "name": name, "url": format!("https://www.igdb.com/games/g{id}"), "total_rating_count": 100,
           "cover": {"image_id": "co1abc"}, "themes": themes, "genres": genres, "keywords": keywords})
}

fn ids(v: &[(Work, u64)]) -> Vec<String> {
    v.iter().map(|(w, _)| w.id.clone()).collect()
}

fn allowed_ids(doc: &serde_json::Value, mature: bool) -> Vec<String> {
    ids(&igdb::parse_games(doc, mature))
}

#[test]
fn igdb_erotic_theme_is_hidden() {
    let doc = json!([igdb_game(
        1,
        "Erotic Game",
        json!([{"id": 42, "name": "Erotic"}]),
        json!([]),
        json!([])
    )]);
    assert!(allowed_ids(&doc, true).is_empty());
    // By name too, in case an id is missing from the response.
    let doc = json!([igdb_game(
        2,
        "Erotic By Name",
        json!([{"name": "Erotic"}]),
        json!([]),
        json!([])
    )]);
    assert!(allowed_ids(&doc, true).is_empty());
}

#[test]
fn igdb_blacklisted_keywords_and_adult_genres_are_hidden() {
    let doc = json!([
        igdb_game(
            1,
            "Hentai Tagged",
            json!([]),
            json!([]),
            json!([{"name": "Hentai"}])
        ),
        igdb_game(
            2,
            "Porn Part",
            json!([]),
            json!([]),
            json!([{"name": "interactive porn"}])
        ),
        igdb_game(
            3,
            "Exact Sex",
            json!([]),
            json!([]),
            json!([{"name": "Sex"}])
        ),
        igdb_game(
            4,
            "Adult Genre",
            json!([]),
            json!([{"name": "Adult"}]),
            json!([])
        ),
        igdb_game(
            5,
            "Fine",
            json!([]),
            json!([{"name": "Adventure"}]),
            json!([{"name": "sexuality education"}, {"name": "essex county"}])
        ),
    ]);
    assert_eq!(
        allowed_ids(&doc, true),
        vec!["igdb:5"],
        "only whole-word and listed terms hide a game"
    );
}

#[test]
fn igdb_a_game_without_any_age_rating_and_with_clean_themes_is_allowed() {
    // The response carries no age_ratings at all, and none is ever read.
    let doc = json!([igdb_game(
        7,
        "Unrated Indie",
        json!([{"id": 31, "name": "Drama"}]),
        json!([{"name": "Puzzle"}]),
        json!([])
    )]);
    assert_eq!(allowed_ids(&doc, true), vec!["igdb:7"]);
    assert_eq!(allowed_ids(&doc, false), vec!["igdb:7"]);
}

#[test]
fn igdb_age_ratings_play_no_part_in_the_decision() {
    // An AO / PEGI 18 / CERO Z game with clean content is allowed; an "E for everyone" game with an erotic theme is not.
    let mut ao = igdb_game(1, "Rated AO but clean", json!([]), json!([]), json!([]));
    ao["age_ratings"] = json!([{"category": 1, "rating": 12}, {"category": 2, "rating": 5}, {"category": 3, "rating": 5}]);
    let mut kids = igdb_game(
        2,
        "Rated E but erotic",
        json!([{"id": 42, "name": "Erotic"}]),
        json!([]),
        json!([]),
    );
    kids["age_ratings"] = json!([{"category": 1, "rating": 8}]);
    assert_eq!(allowed_ids(&json!([ao, kids]), true), vec!["igdb:1"]);
}

#[test]
fn igdb_silent_hill_is_allowed_and_mature_themes_off_hides_horror_and_war() {
    let doc = json!([
        igdb_game(
            3,
            "Silent Hill 2",
            json!([{"id": 19, "name": "Horror"}]),
            json!([{"name": "Adventure"}]),
            json!([{"name": "survival horror"}])
        ),
        igdb_game(
            4,
            "War Game",
            json!([{"id": 39, "name": "Warfare"}]),
            json!([]),
            json!([])
        ),
        igdb_game(
            5,
            "Keyword Only",
            json!([]),
            json!([]),
            json!([{"name": "Psychological Horror"}])
        ),
        igdb_game(
            6,
            "Teen Game",
            json!([{"id": 1, "name": "Action"}]),
            json!([]),
            json!([])
        ),
    ]);
    assert_eq!(
        allowed_ids(&doc, true),
        vec!["igdb:3", "igdb:4", "igdb:5", "igdb:6"],
        "violent and horror games are allowed by default"
    );
    assert_eq!(
        allowed_ids(&doc, false),
        vec!["igdb:6"],
        "mature-themes off hides Horror, Warfare and survival-horror keywords"
    );
}

#[test]
fn igdb_sexual_content_is_hidden_whatever_mature_themes_says() {
    let doc = json!([igdb_game(
        1,
        "Erotic Horror",
        json!([{"id": 42}, {"id": 19}]),
        json!([]),
        json!([])
    )]);
    assert!(allowed_ids(&doc, true).is_empty() && allowed_ids(&doc, false).is_empty());
}

#[test]
fn igdb_queries_always_ask_for_the_content_fields_and_never_for_age_ratings() {
    for fields in [igdb::GAME_FIELDS, igdb::CHARACTER_FIELDS] {
        for needed in ["themes.id", "themes.name", "genres.name", "keywords.name"] {
            assert!(fields.contains(needed), "{needed} missing from {fields}");
        }
        assert!(
            !fields.contains("age_rating"),
            "age ratings must not be requested"
        );
    }
    assert!(igdb::GAME_FIELDS.contains("name") && igdb::GAME_FIELDS.contains("summary"));
}

#[test]
fn igdb_an_error_response_is_a_failure_not_an_empty_list() {
    let err = json!([{"title": "Syntax Error", "status": 400, "cause": "bad field"}]);
    assert!(
        igdb::ensure_games(&err).is_err(),
        "nothing may be judged from an error body"
    );
    assert!(
        igdb::ensure_games(&json!({"message": "x"})).is_err(),
        "not a list"
    );
    assert!(
        igdb::ensure_games(&json!([])).is_ok(),
        "an empty list is a real answer"
    );
    assert!(
        igdb::ensure_games(&json!([igdb_game(1, "G", json!([]), json!([]), json!([]))])).is_ok()
    );
}

#[test]
fn igdb_odd_image_ids_are_dropped_and_covers_are_allow_listed() {
    let mut bad = igdb_game(8, "Odd", json!([]), json!([]), json!([]));
    bad["cover"]["image_id"] = json!("../evil");
    assert_eq!(igdb::parse_games(&json!([bad]), true)[0].0.cover_url, None);
    let ok = igdb::parse_games(
        &json!([igdb_game(9, "Fine", json!([]), json!([]), json!([]))]),
        true,
    );
    assert!(cover_host_allowed(
        ok[0].0.cover_url.as_deref().unwrap_or("")
    ));
}

#[test]
fn igdb_characters_follow_their_games_content() {
    let clean = |name: &str| json!({"name": name, "total_rating_count": 900, "themes": [], "genres": [], "keywords": []});
    let erotic = json!({"name": "Bad", "total_rating_count": 5, "themes": [{"id": 42}]});
    let keyword =
        json!({"name": "Tagged", "total_rating_count": 5, "keywords": [{"name": "hentai"}]});
    let horror = json!({"name": "Silent Hill 2", "total_rating_count": 50, "themes": [{"id": 19, "name": "Horror"}]});
    let doc = json!([
        {"id": 1, "name": "Mario", "games": [clean("Super Mario Bros.")], "mug_shot": {"image_id": "m1"}},
        {"id": 2, "name": "Tainted", "games": [clean("Fine"), erotic]},
        {"id": 3, "name": "Nobody", "games": []},
        {"id": 4, "name": "Tagged", "games": [keyword]},
        {"id": 5, "name": "James", "games": [horror]},
    ]);
    let found = igdb::parse_characters(&doc, true);
    assert_eq!(ids(&found), vec!["igdb:c1", "igdb:c5"]);
    assert_eq!(found[0].0.subtitle.as_deref(), Some("Super Mario Bros."));
    assert_eq!(found[0].0.character_id(), Some(2_000_000_000_001));
    assert_eq!(
        ids(&igdb::parse_characters(&doc, false)),
        vec!["igdb:c1"],
        "mature-themes off hides a horror-only character"
    );
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

#[test]
fn random_offsets_are_distinct_inside_the_first_500_and_at_least_ten() {
    for _ in 0..50 {
        let igdb = offset_plan(RANDOM_TRIES, Source::Igdb);
        assert!(igdb.len() >= 10 && RANDOM_TRIES >= 10);
        let mut seen = std::collections::HashSet::new();
        assert!(
            igdb.iter().all(|o| seen.insert(*o) && *o + 40 <= 500 + 20),
            "{igdb:?}"
        );
        let vndb = offset_plan(RANDOM_TRIES, Source::Vndb);
        assert!(vndb.len() >= 10 && vndb.iter().all(|p| (1..=25).contains(p)));
    }
}

fn game(id: &str) -> Work {
    Work {
        work_key: None,
        id: id.into(),
        source: Source::Igdb,
        kind: Kind::Game,
        title: id.into(),
        native: None,
        subtitle: None,
        year: None,
        status: None,
        rating: None,
        url: None,
        cover_url: None,
        genres: Vec::new(),
    }
}

#[test]
fn a_random_pick_skips_the_last_five_and_is_remembered() {
    let user = 777_001;
    let all: Vec<Work> = (0..8).map(|i| game(&format!("igdb:{i}"))).collect();
    let mut picked = std::collections::HashSet::new();
    for _ in 0..5 {
        let w = pick_unseen(all.clone(), user, "t-random").expect("a pick");
        assert!(picked.insert(w.id), "no repeat inside the last five");
    }
    // Only the five remembered ones are excluded: with three left the next picks are among them.
    let next = pick_unseen(all.clone(), user, "t-random").expect("a pick");
    assert!(!picked.contains(&next.id));
    // When every candidate was seen the page is not usable: the caller widens to another offset.
    let seen_only: Vec<Work> = picked
        .iter()
        .map(|id| game(id))
        .filter(|w| crate::commands::picks::seen(user, "t-random", &w.id))
        .collect();
    assert!(pick_unseen(seen_only, user, "t-random").is_none());
}

#[test]
fn a_failed_provider_never_blocks_the_other_and_says_so() {
    let ok = || -> Option<Result<Vec<Work>, ProviderError>> { Some(Ok(vec![game("igdb:1")])) };
    let failed: Option<Result<Vec<Work>, ProviderError>> =
        Some(Err(ProviderError::Http("400 Bad Request".into())));
    let partial = Found::from_results([(Source::Igdb, ok()), (Source::Vndb, failed)]);
    assert_eq!(partial.works.len(), 1);
    assert!(!partial.all_failed());
    assert_eq!(
        partial.partial_note().as_deref(),
        Some("لم أستطع جلب نتائج من VNDB الآن، هذه نتائج IGDB فقط.")
    );
    assert_eq!(
        partial.empty_message("لا شيء"),
        "لا شيء",
        "an empty answer after a partial failure is not 'unavailable'"
    );

    let all_down = Found::from_results([
        (Source::Igdb, Some(Err(ProviderError::Http("500".into())))),
        (Source::Vndb, Some(Err(ProviderError::Http("400".into())))),
    ]);
    assert!(
        all_down.all_failed() && all_down.works.is_empty() && all_down.partial_note().is_none()
    );
    assert_eq!(all_down.empty_message("لا شيء"), crate::i18n::GAMES_DOWN);

    let clean = Found::from_results([(Source::Igdb, ok()), (Source::Vndb, None)]);
    assert!(
        clean.partial_note().is_none() && !clean.all_failed(),
        "a provider that was not asked is not a failure"
    );
    assert!(Found::default().partial_note().is_none() && !Found::default().all_failed());
}

fn vn(id: &str, tags: serde_json::Value, sexual: serde_json::Value) -> serde_json::Value {
    json!({"id": id, "title": format!("VN {id}"), "alttitle": "原題", "released": "2016-05-01", "rating": 82.0,
           "image": {"url": "https://t.vndb.org/cv/aa/1.jpg", "sexual": sexual, "violence": 0}, "tags": tags})
}

fn vn_ids(doc: &serde_json::Value, mature: bool) -> Vec<String> {
    vndb::parse_vns(doc, mature)
        .iter()
        .map(|w| w.id.clone())
        .collect()
}

#[test]
fn vndb_ero_tags_are_hidden_and_a_clean_item_is_allowed() {
    let doc = json!({"results": [
        vn("v1", json!([{"id": "g1", "name": "School Life", "category": "cont", "rating": 2.0}]), json!(0)),
        vn("v2", json!([{"id": "g2", "name": "Whatever", "category": "ero", "rating": 1.0}]), json!(0)),
        vn("v3", json!([{"id": "g3", "name": "Sexual Content", "category": "cont"}]), json!(0)),
        vn("v4", json!([]), json!(0)),
        {"id": "v5", "title": "No tags field"},
    ]});
    assert_eq!(vn_ids(&doc, true), vec!["vndb:v1", "vndb:v4"], "ero category or name hides; empty tags is a real (clean) answer; a missing tag list gives no verdict");
    let found = vndb::parse_vns(&doc, true);
    assert_eq!(found[0].year, Some(2016));
    assert_eq!(found[0].url.as_deref(), Some("https://vndb.org/v1"));
    assert!((found[0].rating.unwrap_or(0.0) - 8.2).abs() < 0.01);
    assert!(cover_host_allowed(
        found[0].cover_url.as_deref().unwrap_or("")
    ));
}

#[test]
fn vndb_an_image_above_safe_hides_the_novel_whether_cover_or_screenshot() {
    let mut suggestive = vn("v1", json!([]), json!(1));
    let mut shot = vn("v2", json!([]), json!(0));
    shot["screenshots"] = json!([{"sexual": 0, "violence": 0}, {"sexual": 2, "violence": 0}]);
    let clean = vn("v3", json!([]), json!(0));
    suggestive["screenshots"] = json!([]);
    let doc = json!({"results": [suggestive, shot, clean]});
    assert_eq!(vn_ids(&doc, true), vec!["vndb:v3"]);
}

#[test]
fn vndb_violence_is_allowed_unless_mature_themes_is_off() {
    let mut brutal = vn(
        "v1",
        json!([{"category": "cont", "name": "Gore"}]),
        json!(0),
    );
    brutal["image"]["violence"] = json!(2);
    let mut shot = vn("v2", json!([]), json!(0));
    shot["screenshots"] = json!([{"sexual": 0, "violence": 2}]);
    let mild = vn("v3", json!([]), json!(0));
    let doc = json!({"results": [brutal, shot, mild]});
    assert_eq!(
        vn_ids(&doc, true),
        vec!["vndb:v1", "vndb:v2", "vndb:v3"],
        "dark themes are allowed by default"
    );
    assert_eq!(
        vn_ids(&doc, false),
        vec!["vndb:v3"],
        "mature-themes off hides high violence"
    );
}

#[test]
fn vndb_characters_need_a_clean_picture_traits_and_verified_novels() {
    let chars = json!({"results": [
        {"id": "c1", "name": "Ok", "original": "名", "image": {"url": "https://t.vndb.org/ch/a/1.jpg", "sexual": 0, "violence": 0}, "vns": [{"id": "v1", "title": "VN v1"}]},
        {"id": "c2", "name": "Flagged picture", "image": {"sexual": 2}, "vns": [{"id": "v1", "title": "x"}]},
        {"id": "c3", "name": "Erotic novel", "image": {"sexual": 0}, "vns": [{"id": "v2", "title": "x"}]},
        {"id": "c4", "name": "No novels", "image": {"sexual": 0}, "vns": []},
        {"id": "c5", "name": "Unchecked novel", "image": {"sexual": 0}, "vns": [{"id": "v9", "title": "x"}]},
        {"id": "c6", "name": "Sexual trait", "image": {"sexual": 0}, "traits": [{"name": "Whatever", "group_name": "Sexual Traits"}], "vns": [{"id": "v1", "title": "x"}]},
        {"id": "c7", "name": "Harmless traits", "image": {"sexual": 0}, "traits": [{"name": "Tall", "group_name": "Body"}], "vns": [{"id": "v1", "title": "x"}]},
        {"id": "c8", "name": "Brutal picture", "image": {"sexual": 0, "violence": 2}, "vns": [{"id": "v1", "title": "x"}]},
    ]});
    let vns = json!({"results": [
        vn("v1", json!([{"category": "cont", "rating": 1.0}]), json!(0)),
        vn("v2", json!([{"category": "ero", "rating": 1.0}]), json!(0)),
    ]});
    let found = |mature: bool| -> Vec<String> {
        let verdicts = vndb::vn_verdicts(&vns, mature);
        vndb::parse_characters(&chars, &verdicts, mature)
            .iter()
            .map(|w| w.id.clone())
            .collect()
    };
    assert_eq!(found(true), vec!["vndb:c1", "vndb:c7", "vndb:c8"]);
    assert_eq!(
        found(false),
        vec!["vndb:c1", "vndb:c7"],
        "mature-themes off also hides a brutal picture"
    );
    let first = vndb::parse_characters(&chars, &vndb::vn_verdicts(&vns, true), true);
    assert_eq!(first[0].subtitle.as_deref(), Some("VN v1"));
    assert_eq!(first[0].character_id(), Some(1_000_000_000_001));
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

// ---- VNDB request bodies: the exact JSON, and every field checked against the documented ones ----

const VN_ROOTS: &[&str] = &[
    "id",
    "title",
    "alttitle",
    "titles",
    "aliases",
    "olang",
    "devstatus",
    "released",
    "languages",
    "platforms",
    "image",
    "length",
    "length_minutes",
    "length_votes",
    "description",
    "average",
    "rating",
    "votecount",
    "screenshots",
    "relations",
    "tags",
    "developers",
    "editions",
    "staff",
    "va",
    "extlinks",
];
const CHARACTER_ROOTS: &[&str] = &[
    "id",
    "name",
    "original",
    "aliases",
    "description",
    "image",
    "blood_type",
    "height",
    "weight",
    "bust",
    "waist",
    "hips",
    "cup",
    "age",
    "birthday",
    "sex",
    "vns",
    "traits",
];

fn nested_ok(root: &str, sub: &str) -> bool {
    let image = [
        "id",
        "url",
        "dims",
        "sexual",
        "violence",
        "votecount",
        "thumbnail",
        "thumbnail_dims",
    ];
    match root {
        "image" => image.contains(&sub),
        "screenshots" => image.contains(&sub) || sub.starts_with("release"),
        "tags" => [
            "id",
            "name",
            "aliases",
            "description",
            "category",
            "searchable",
            "applicable",
            "vn_count",
            "rating",
            "spoiler",
            "lie",
        ]
        .contains(&sub),
        "vns" => [
            "id",
            "title",
            "alttitle",
            "role",
            "spoiler",
            "released",
            "rating",
            "votecount",
        ]
        .contains(&sub),
        "traits" => [
            "id",
            "name",
            "aliases",
            "description",
            "searchable",
            "applicable",
            "group_id",
            "group_name",
            "char_count",
            "spoiler",
            "lie",
        ]
        .contains(&sub),
        _ => false,
    }
}

#[test]
fn vndb_request_fields_are_all_documented_ones() {
    for (endpoint, what, body) in vndb::all_bodies_for_tests() {
        let roots = if endpoint == "vn" {
            VN_ROOTS
        } else {
            CHARACTER_ROOTS
        };
        for field in body["fields"].as_str().expect("fields").split(',') {
            let (root, sub) = field
                .split_once('.')
                .map_or((field, None), |(r, s)| (r, Some(s)));
            assert!(
                roots.contains(&root),
                "/{endpoint} {what}: unknown field {field}"
            );
            if let Some(sub) = sub {
                assert!(
                    nested_ok(root, sub),
                    "/{endpoint} {what}: unknown nested field {field}"
                );
            }
        }
        assert!(
            body["results"].as_u64().unwrap_or(0) <= 100,
            "/{endpoint} {what}: results above 100"
        );
        let searching = body["filters"].get(0).and_then(|f| f.as_str()) == Some("search");
        assert!(
            !(searching && body.get("sort").is_some()),
            "/{endpoint} {what}: sort with a search filter"
        );
        assert!(body["filters"].is_array() && body.get("fields").is_some());
    }
    assert!(
        !vndb::all_bodies_for_tests()
            .iter()
            .any(|(_, _, b)| b["fields"].as_str().unwrap_or("").contains("releases")),
        "release data lives on /release, not /vn"
    );
}

#[test]
fn vndb_bodies_have_the_exact_documented_shape() {
    assert_eq!(
        vndb::vn_search_body("silent hill"),
        json!({"filters": ["search", "=", "silent hill"], "results": 20,
               "fields": "id,title,alttitle,image.url,image.sexual,image.violence,rating,votecount,released,devstatus,description,screenshots.sexual,screenshots.violence,tags.id,tags.name,tags.category,tags.rating"})
    );
    assert_eq!(
        vndb::vn_by_id_body("v17")["filters"],
        json!(["id", "=", "v17"])
    );
    assert_eq!(vndb::vn_by_id_body("v17")["results"], json!(1));
    let pop = vndb::vn_popular_body(0);
    assert_eq!(
        (
            pop["page"].clone(),
            pop["sort"].clone(),
            pop["reverse"].clone()
        ),
        (json!(1), json!("votecount"), json!(true))
    );
    assert_eq!(
        vndb::character_search_body("alice"),
        json!({"filters": ["search", "=", "alice"], "results": 20,
               "fields": "id,name,original,image.url,image.sexual,image.violence,vns.id,vns.title,traits.name,traits.group_name"})
    );
    assert_eq!(
        vndb::character_by_id_body(5)["filters"],
        json!(["id", "=", "c5"])
    );
    assert_eq!(
        vndb::characters_of_vn_body("v17")["filters"],
        json!(["vn", "=", ["id", "=", "v17"]])
    );
    let verdict = vndb::vn_verdict_body(&["v1".into(), "v2".into()]);
    assert_eq!(
        verdict["filters"],
        json!(["or", ["id", "=", "v1"], ["id", "=", "v2"]])
    );
    assert!(verdict["fields"]
        .as_str()
        .unwrap_or("")
        .contains("tags.category"));
}

#[test]
fn vndb_parses_a_response_shaped_like_the_real_api() {
    let doc = json!({"more": false, "results": [{
        "id": "v7", "title": "Silent Hill: The Novel", "alttitle": "サイレントヒル", "released": "1999-01-01", "devstatus": 0,
        "rating": 77.5, "votecount": 1200, "description": "A town in fog.",
        "image": {"url": "https://t.vndb.org/cv/12/34.jpg", "sexual": 0, "violence": 1},
        "screenshots": [{"sexual": 0, "violence": 1}],
        "tags": [{"id": "g5", "name": "Horror", "category": "cont", "rating": 2.9}]
    }]});
    let w = vndb::parse_vns(&doc, true);
    assert_eq!(w.len(), 1);
    assert_eq!(
        (w[0].id.as_str(), w[0].title.as_str(), w[0].year),
        ("vndb:v7", "Silent Hill: The Novel", Some(1999))
    );
    assert_eq!(w[0].status.as_deref(), Some("finished"));
}

// ---- IGDB request bodies (exact text) ----

#[test]
fn igdb_bodies_have_the_exact_documented_shape() {
    assert_eq!(
        igdb::characters_search_body("james sunderland"),
        format!(
            "search \"james sunderland\"; fields {}; limit 20;",
            igdb::CHARACTER_FIELDS
        )
    );
    assert!(igdb::CHARACTER_FIELDS
        .starts_with("name,akas,url,mug_shot.image_id,games.name,games.cover.image_id"));
    assert_eq!(
        igdb::characters_of_game_body(1234),
        format!(
            "fields {}; where games = (1234); limit 50;",
            igdb::CHARACTER_FIELDS
        )
    );
    assert_eq!(
        igdb::character_by_id_body(9),
        format!("fields {}; where id = 9; limit 1;", igdb::CHARACTER_FIELDS)
    );
    let search = igdb::games_search_body("silent hill", None);
    assert!(
        search.starts_with("search \"silent hill\"; fields name,summary,url,"),
        "{search}"
    );
    assert!(
        search.contains("where version_parent = null & parent_game = null;")
            && search.ends_with("limit 50;")
    );
    assert!(igdb::games_search_body("x", Some("PlayStation"))
        .contains("platforms.name ~ *\"PlayStation\"*"));
    let popular = igdb::popular_games_body(80, 5, None);
    assert!(
        popular.contains("total_rating_count > 5")
            && popular.contains("sort total_rating_count desc; limit 40; offset 80;"),
        "{popular}"
    );
    assert!(
        popular.contains("themes.id")
            && popular.contains("keywords.name")
            && popular.contains("genres.name")
            && popular.contains("cover.image_id")
    );
    for body in [
        popular,
        search,
        igdb::characters_search_body("x"),
        igdb::characters_of_game_body(1),
    ] {
        assert!(
            !body.contains("age_rating") && !body.contains("category"),
            "no age ratings, no deprecated category: {body}"
        );
    }
}

#[test]
fn igdb_characters_of_a_game_come_back_with_their_games_and_the_main_games_key() {
    let silent_hill = json!({"id": 5, "name": "Silent Hill 2", "total_rating_count": 900, "themes": [{"id": 19, "name": "Horror"}], "cover": {"image_id": "co1"}});
    let other = json!({"id": 6, "name": "Silent Hill 3", "total_rating_count": 300, "themes": [{"id": 19, "name": "Horror"}]});
    let doc = json!([
        {"id": 11, "name": "James Sunderland", "mug_shot": {"image_id": "mug1"}, "games": [other, silent_hill]},
        {"id": 12, "name": "No Picture", "games": [{"id": 5, "name": "Silent Hill 2", "total_rating_count": 900}]},
    ]);
    let found = igdb::parse_characters(&doc, true);
    assert_eq!(
        ids(&found),
        vec!["igdb:c11", "igdb:c12"],
        "ranked by the games' popularity"
    );
    let james = &found[0].0;
    assert_eq!(
        james.subtitle.as_deref(),
        Some("Silent Hill 2 · Silent Hill 3")
    );
    assert_eq!(
        james.work_key.as_deref(),
        Some("igdb:5"),
        "the most rated game opens from the character card"
    );
    assert!(
        james.cover_url.is_some() && found[1].0.cover_url.is_none(),
        "a missing mug shot is fine"
    );
}
