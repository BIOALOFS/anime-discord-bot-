use super::*;
use serde_json::json;

fn tracked() -> Tracked {
    Tracked {
        work_id: 100,
        titles: vec!["Solo Leveling".into(), "Na Honjaman Level-Up".into()],
    }
}

fn chapter(source: SourceKind, key: &str, published: Option<i64>) -> Chapter {
    Chapter {
        source,
        work_id: 100,
        key: key.into(),
        number: Some(key.into()),
        title: None,
        url: format!("https://x/{key}"),
        lang: None,
        published,
    }
}

const NOW: i64 = 1_800_000_000;

// ---- numbers and titles --------------------------------------------------------------------------

#[test]
fn chapter_numbers_are_normalised_and_ordinal() {
    assert_eq!(normalize_number("12"), Some("12".into()));
    assert_eq!(normalize_number(" 012.0 "), Some("12".into()));
    assert_eq!(normalize_number("12.5"), Some("12.5".into()));
    assert_eq!(normalize_number(""), None);
    assert_eq!(normalize_number("Extra"), None);
    assert_eq!(normalize_number("-3"), None);
    assert_eq!((ordinal("12"), ordinal("12.5")), (Some(120), Some(125)));
}

#[test]
fn titles_match_loosely_but_not_for_tiny_strings() {
    let w = tracked();
    assert!(title_matches("solo leveling", &w));
    assert!(title_matches("Solo-Leveling!", &w));
    assert!(
        title_matches("Solo Leveling (Official)", &w),
        "the match may be shorter or longer"
    );
    assert!(!title_matches("Tower of God", &w));
    assert!(!title_matches("so", &w), "two characters match everything");
    assert!(!title_matches("", &w));
}

// ---- MangaDex (fixtures: Arabic and English feeds) ----------------------------------------------------

fn dex_feed(lang: &str) -> serde_json::Value {
    json!({"result": "ok", "data": [
        {"id": "c3", "type": "chapter", "attributes": {"chapter": "12", "title": "Return", "translatedLanguage": lang, "publishAt": "2026-11-15T10:00:00+00:00", "externalUrl": null}},
        {"id": "c2", "type": "chapter", "attributes": {"chapter": "11.5", "title": "", "translatedLanguage": lang, "publishAt": "2026-11-08T10:00:00+00:00"}},
        {"id": "c1", "type": "chapter", "attributes": {"chapter": "11", "translatedLanguage": lang, "publishAt": "2026-11-01T10:00:00+00:00", "externalUrl": "https://elsewhere.example/11"}},
        {"id": "c0", "type": "chapter", "attributes": {"chapter": null, "title": "Oneshot", "translatedLanguage": lang, "publishAt": "2026-10-01T10:00:00+00:00"}}
    ]})
}

#[test]
fn mangadex_feed_parses_arabic_and_english_chapters_with_credit_links() {
    for lang in ["ar", "en"] {
        let c = mangadex::parse_feed(&dex_feed(lang), 100).expect("parse");
        assert_eq!(
            c.len(),
            3,
            "the externally hosted chapter is skipped: {c:?}"
        );
        assert_eq!(c[0].key, "12");
        assert_eq!(c[0].url, "https://mangadex.org/chapter/c3");
        assert_eq!(c[0].lang.as_deref(), Some(lang));
        assert_eq!(c[0].title.as_deref(), Some("Return"));
        assert_eq!(c[1].key, "11.5");
        assert_eq!(c[1].title, None, "an empty title is dropped");
        assert_eq!(
            c[2].key, "mdex:c0",
            "a chapter without a number is keyed by its id"
        );
        assert_eq!(c[0].published, Some(1_794_736_800));
        assert!(c.iter().all(
            |c| c.source == SourceKind::MangaDex && c.url.starts_with("https://mangadex.org/")
        ));
    }
    assert!(mangadex::parse_feed(&json!({}), 1).is_err());
}

#[test]
fn only_the_preferred_language_is_kept() {
    let mut mixed = mangadex::parse_feed(&dex_feed("ar"), 100).expect("ar");
    mixed.extend(mangadex::parse_feed(&dex_feed("en"), 100).expect("en"));
    assert_eq!(only_language(mixed.clone(), "ar").len(), 3);
    assert_eq!(only_language(mixed, "en").len(), 3);
    let unlabelled = vec![chapter(SourceKind::Rss, "5", None)];
    assert_eq!(
        only_language(unlabelled, "ar").len(),
        1,
        "sources without a language are kept"
    );
}

#[test]
fn mangadex_search_matches_titles_and_alt_titles_and_only_allowed_ratings() {
    let doc = json!({"data": [
        {"id": "wrong", "attributes": {"title": {"en": "Another Series"}, "contentRating": "safe"}},
        {"id": "adult", "attributes": {"title": {"en": "Solo Leveling"}, "contentRating": "pornographic"}},
        {"id": "right", "attributes": {"title": {"en": "Solo Leveling"}, "altTitles": [{"ko": "나 혼자만 레벨업"}], "contentRating": "suggestive"}}
    ]});
    assert_eq!(
        mangadex::match_manga(&doc, &tracked()).as_deref(),
        Some("right")
    );
    let by_alt = json!({"data": [{"id": "alt", "attributes": {"title": {"en": "Different"}, "altTitles": [{"en": "Na Honjaman Level-Up"}], "contentRating": "safe"}}]});
    assert_eq!(
        mangadex::match_manga(&by_alt, &tracked()).as_deref(),
        Some("alt")
    );
    let none = json!({"data": [{"id": "x", "attributes": {"title": {"en": "Unrelated"}, "contentRating": "safe"}}]});
    assert_eq!(mangadex::match_manga(&none, &tracked()), None);
    assert_eq!(
        mangadex::match_manga(
            &json!({"data": [{"id": "e", "attributes": {"title": {"en": "Solo Leveling"}, "contentRating": "erotica"}}]}),
            &tracked()
        ),
        None
    );
}

// ---- MangaUpdates ----------------------------------------------------------------------------------------

#[test]
fn mangaupdates_releases_keep_only_the_series_and_parse_numbers_and_dates() {
    let doc = json!({"results": [
        {"record": {"id": 1, "title": "Solo Leveling", "chapter": "201", "volume": "", "release_date": "2026-11-14"}},
        {"record": {"id": 2, "title": "Solo Leveling", "chapter": "198-200", "release_date": "2026-11-10"}},
        {"record": {"id": 3, "title": "Solo Leveling: Ragnarok", "chapter": "5", "release_date": "2026-11-12"}},
        {"record": {"id": 4, "title": "Solo Leveling", "release_date": "2026-11-01"}}
    ]});
    let c = mangaupdates::parse_releases(&doc, &tracked()).expect("parse");
    assert_eq!(
        c.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(),
        vec!["201", "200", "mu:4"],
        "a different series is not matched; ranges give the last chapter"
    );
    assert!(c.iter().all(|c| c.source == SourceKind::MangaUpdates
        && c.url.starts_with("https://www.mangaupdates.com/")));
    assert_eq!(c[0].published, Some(1_794_614_400));
    assert!(mangaupdates::parse_releases(&json!({"x": 1}), &tracked()).is_err());
}

// ---- RSS / Atom -------------------------------------------------------------------------------------------

const RSS: &str = r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Feed</title>
<item><title>Solo Leveling &amp; friends - Chapter 12</title><link>https://scans.example/solo/12</link><guid isPermaLink="false">g-12</guid><pubDate>Sun, 15 Nov 2026 10:00:00 +0000</pubDate></item>
<item><title><![CDATA[Solo Leveling ch. 11.5]]></title><link>https://scans.example/solo/11-5</link><guid>g-11-5</guid></item>
<item><title>Insecure</title><link>http://scans.example/solo/10</link></item>
<item><title>No link at all</title></item>
</channel></rss>"#;

const ATOM: &str = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>A</title>
<entry><title>فصل 7 من العمل</title><link rel="alternate" href="https://atom.example/c/7"/><id>tag:atom,7</id><updated>2026-11-14T09:00:00Z</updated></entry></feed>"#;

#[test]
fn rss_entries_are_parsed_unescaped_and_insecure_links_dropped() {
    let e = rss::parse_entries(RSS);
    assert_eq!(
        e.len(),
        2,
        "http links and entries without a link are dropped: {e:?}"
    );
    assert_eq!(e[0].title, "Solo Leveling & friends - Chapter 12");
    assert_eq!(e[0].id, "g-12");
    assert_eq!(e[0].published, Some(1_794_736_800));
    assert_eq!(e[1].title, "Solo Leveling ch. 11.5", "CDATA is unwrapped");
    assert!(rss::parse_entries("<html><body>not a feed</body></html>").is_empty());
    assert!(rss::parse_entries("").is_empty());
}

#[test]
fn atom_entries_use_href_and_updated() {
    let e = rss::parse_entries(ATOM);
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].link, "https://atom.example/c/7");
    assert_eq!(e[0].id, "tag:atom,7");
    assert_eq!(e[0].published, Some(1_794_646_800));
    assert_eq!(
        rss::number_in_title(&e[0].title).as_deref(),
        Some("7"),
        "Arabic marker"
    );
}

#[test]
fn chapter_numbers_come_from_titles() {
    assert_eq!(
        rss::number_in_title("Solo Leveling Chapter 12").as_deref(),
        Some("12")
    );
    assert_eq!(
        rss::number_in_title("Solo Leveling ch. 11.5").as_deref(),
        Some("11.5")
    );
    assert_eq!(
        rss::number_in_title("Solo Leveling 201").as_deref(),
        Some("201"),
        "the last number"
    );
    assert_eq!(rss::number_in_title("A title without numbers"), None);
    let c = rss::to_chapters(&rss::parse_entries(RSS), 100);
    assert_eq!(c[0].key, "12");
    assert!(c
        .iter()
        .all(|c| c.source == SourceKind::Rss && c.url.starts_with("https://")));
}

// ---- dedupe and the first-run baseline ----------------------------------------------------------------

#[test]
fn a_chapter_is_announced_once_even_when_two_sources_report_it() {
    let both = vec![
        chapter(SourceKind::MangaDex, "12", Some(NOW - 600)),
        chapter(SourceKind::Rss, "12", Some(NOW - 300)),
        chapter(SourceKind::MangaUpdates, "13", Some(NOW - 100)),
    ];
    let posted: HashSet<String> = ["11".to_string()].into();
    let p = plan(both, &posted, NOW);
    assert_eq!(
        p.announce
            .iter()
            .map(|c| (c.key.as_str(), c.source))
            .collect::<Vec<_>>(),
        vec![
            ("12", SourceKind::MangaDex),
            ("13", SourceKind::MangaUpdates)
        ],
        "the first source wins"
    );
    assert!(p.record_only.is_empty());
}

#[test]
fn already_announced_chapters_are_never_repeated() {
    let posted: HashSet<String> = ["12".to_string(), "13".to_string()].into();
    let p = plan(
        vec![
            chapter(SourceKind::MangaDex, "12", Some(NOW)),
            chapter(SourceKind::MangaDex, "13", Some(NOW)),
        ],
        &posted,
        NOW,
    );
    assert!(p.announce.is_empty() && p.record_only.is_empty());
}

#[test]
fn the_first_check_records_old_chapters_silently_and_announces_only_fresh_ones() {
    let old = chapter(SourceKind::MangaDex, "10", Some(NOW - 30 * 86_400));
    let fresh = chapter(SourceKind::MangaDex, "12", Some(NOW - 3600));
    let undated = chapter(SourceKind::Rss, "13", None);
    let p = plan(vec![fresh, old, undated], &HashSet::new(), NOW);
    assert_eq!(
        p.announce
            .iter()
            .map(|c| c.key.as_str())
            .collect::<Vec<_>>(),
        vec!["12", "13"]
    );
    assert_eq!(
        p.record_only
            .iter()
            .map(|c| c.key.as_str())
            .collect::<Vec<_>>(),
        vec!["10"],
        "enabling a source never floods the channel"
    );
}

#[test]
fn announcements_are_oldest_first() {
    let posted: HashSet<String> = ["1".to_string()].into();
    let p = plan(
        vec![
            chapter(SourceKind::MangaDex, "3", Some(NOW - 10)),
            chapter(SourceKind::MangaDex, "2", Some(NOW - 500)),
        ],
        &posted,
        NOW,
    );
    assert_eq!(
        p.announce
            .iter()
            .map(|c| c.key.as_str())
            .collect::<Vec<_>>(),
        vec!["2", "3"]
    );
}

#[test]
fn sources_have_a_fixed_priority_and_credits() {
    assert_eq!(
        SourceKind::ORDER,
        [
            SourceKind::MangaDex,
            SourceKind::MangaUpdates,
            SourceKind::Rss
        ]
    );
    assert_eq!(SourceKind::MangaDex.credit(), "MangaDex");
    assert_eq!(SourceKind::MangaUpdates.credit(), "MangaUpdates");
}
