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
        .filter(|w| is_safe(w))
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
    let p = Providers::new(reqwest::Client::new(), false);
    assert!(!p.enabled());
    assert!(p.search("anything").await.is_empty());
    let w = Work {
        id: "mdex:x".into(),
        source: Source::MangaDex,
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
