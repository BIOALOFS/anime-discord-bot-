//! End-to-end safety tests through the public `SafeAniList` API using recorded-style JSON fixtures.

use super::*;
use serde_json::Value;

macro_rules! fixture {
    ($name:literal) => {
        serde_json::from_str::<Value>(include_str!(concat!(
            "../../../tests/fixtures/anilist/",
            $name
        )))
        .expect("fixture json")
    };
}

fn api(f: impl Fn(&str, &Value) -> Result<Value, AniError> + Send + Sync + 'static) -> SafeAniList {
    SafeAniList::with_mock(Box::new(f))
}

fn ids(list: &[Media]) -> Vec<i64> {
    list.iter().map(|m| m.id).collect()
}

#[tokio::test]
async fn search_media_hides_adult_and_hentai_and_requests_safe_vars() {
    let api = api(|_, vars| {
        assert_eq!(vars["isAdult"], json!(false));
        assert_eq!(vars["genreNotIn"], json!(["Hentai"]));
        Ok(fixture!("search_media.json")["data"].clone())
    });
    let out = api
        .search_media("safe", MediaKind::Anime, 10, ContentPolicy::SAFE)
        .await
        .expect("ok");
    assert_eq!(ids(&out), vec![1]);
}

#[tokio::test]
async fn search_media_allows_adult_only_when_policy_says_so() {
    let api = api(|_, vars| {
        assert!(vars.get("isAdult").is_none());
        Ok(fixture!("search_media.json")["data"].clone())
    });
    let policy = ContentPolicy { allow_adult: true };
    let out = api
        .search_media("x", MediaKind::Anime, 10, policy)
        .await
        .expect("ok");
    assert_eq!(ids(&out), vec![1, 2, 3]);
}

#[tokio::test]
async fn search_characters_filters_and_ranks_by_favourites() {
    let api = api(|_, _| Ok(fixture!("search_character.json")["data"].clone()));
    let out = api
        .search_characters("x", 10, ContentPolicy::SAFE)
        .await
        .expect("ok");
    // Adult-linked, mixed and zero-appearance characters are gone; the rest is ranked by favourites.
    let got: Vec<i64> = out.iter().map(|c| c.id).collect();
    assert_eq!(got, vec![14, 10]);
}

#[tokio::test]
async fn character_by_id_reports_hidden() {
    let adult = api(|_, _| Ok(fixture!("character_adult.json")["data"].clone()));
    assert!(matches!(
        adult
            .character_by_id(11, ContentPolicy::SAFE)
            .await
            .expect("ok"),
        CharacterLookup::Hidden
    ));
    let safe = api(|_, _| Ok(fixture!("character_safe.json")["data"].clone()));
    assert!(matches!(
        safe.character_by_id(10, ContentPolicy::SAFE)
            .await
            .expect("ok"),
        CharacterLookup::Found(_)
    ));
    let missing = api(|_, _| Err(AniError::NotFound));
    assert!(matches!(
        missing
            .character_by_id(1, ContentPolicy::SAFE)
            .await
            .expect("ok"),
        CharacterLookup::NotFound
    ));
}

#[tokio::test]
async fn media_characters_filters_each_character() {
    let api = api(|_, _| Ok(fixture!("media_characters.json")["data"].clone()));
    let page = api
        .media_characters(1, 1, ContentPolicy::SAFE)
        .await
        .expect("ok");
    assert!(page.has_next);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].0.id, 20);
    assert_eq!(page.items[0].1, "MAIN");
}

#[tokio::test]
async fn recommendations_and_relations_are_filtered() {
    let rec = api(|_, _| Ok(fixture!("recommendations.json")["data"].clone()));
    let out = rec
        .recommendations(1, 1, ContentPolicy::SAFE)
        .await
        .expect("ok");
    assert_eq!(ids(&out.items), vec![30]);

    let rel = api(|_, _| Ok(fixture!("relations.json")["data"].clone()));
    let out = rel.relations(1, ContentPolicy::SAFE).await.expect("ok");
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, "SEQUEL");
}

#[tokio::test]
async fn airing_is_filtered_and_sorted() {
    let api = api(|_, _| Ok(fixture!("airing_schedule.json")["data"].clone()));
    let out = api
        .airing_between(0, 9999, ContentPolicy::SAFE)
        .await
        .expect("ok");
    let got: Vec<i64> = out.iter().map(|e| e.media.id).collect();
    assert_eq!(got, vec![52, 50]);
}

#[tokio::test]
async fn random_media_never_returns_adult() {
    let only_adult = api(|_, _| Ok(fixture!("random_media_adult_only.json")["data"].clone()));
    let out = only_adult
        .random_media(MediaKind::Anime, None, None, ContentPolicy::SAFE)
        .await
        .expect("ok");
    assert!(out.is_none(), "all retries filtered out -> None");

    let mixed = api(|_, _| Ok(fixture!("random_media.json")["data"].clone()));
    for _ in 0..10 {
        let m = mixed
            .random_media(
                MediaKind::Anime,
                Some("Action"),
                Some(60),
                ContentPolicy::SAFE,
            )
            .await
            .expect("ok")
            .expect("some");
        assert_eq!(m.id, 62);
    }
}

#[tokio::test]
async fn random_character_never_returns_adult() {
    let api = api(|_, _| Ok(fixture!("search_character.json")["data"].clone()));
    for _ in 0..10 {
        let c = api
            .random_character(ContentPolicy::SAFE)
            .await
            .expect("ok")
            .expect("some");
        assert!([10, 14].contains(&c.id));
    }
}

#[tokio::test]
async fn genres_exclude_hentai() {
    let api = api(|_, _| Ok(json!({"GenreCollection": ["Action", "Hentai", "Comedy"]})));
    assert_eq!(api.genres().await.expect("ok"), vec!["Action", "Comedy"]);
}

#[test]
fn previous_airing_picks_latest_aired() {
    let nodes = vec![
        AiringNode { airing_at: 100 },
        AiringNode { airing_at: 200 },
        AiringNode { airing_at: 300 },
    ];
    assert_eq!(previous_airing(&nodes, 250), Some(200));
    assert_eq!(previous_airing(&nodes, 50), None);
}

#[test]
fn the_media_filter_matches_format_year_status_and_country() {
    let m: Media = serde_json::from_value(serde_json::json!({
        "id": 1, "title": {"romaji": "X"}, "format": "TV", "status": "FINISHED", "countryOfOrigin": "JP",
        "startDate": {"year": 2011}
    }))
    .expect("media");
    let f = MediaFilter {
        format: Some("tv".into()),
        year: Some(2011),
        status: Some("FINISHED".into()),
        country: Some("JP".into()),
    };
    assert!(
        f.matches(&m)
            && MediaFilter::default().matches(&m)
            && MediaFilter::default().is_empty()
            && !f.is_empty()
    );
    for bad in [
        MediaFilter {
            format: Some("MOVIE".into()),
            ..f.clone()
        },
        MediaFilter {
            year: Some(2012),
            ..f.clone()
        },
        MediaFilter {
            status: Some("RELEASING".into()),
            ..f.clone()
        },
        MediaFilter {
            country: Some("KR".into()),
            ..f.clone()
        },
    ] {
        assert!(!bad.matches(&m), "{bad:?}");
    }
    assert_ne!(f.cache_key(), MediaFilter::default().cache_key());
}
