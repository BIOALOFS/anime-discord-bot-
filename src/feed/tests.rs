use super::*;
use serde_json::json;

const NOW: i64 = 1_800_000_000;

fn cfg() -> FeedConfig {
    FeedConfig {
        guild_id: 1,
        channel_id: Some(5),
        enabled: true,
        min_popularity: 1500,
        formats: vec!["TV".into(), "TV_SHORT".into(), "ONA".into(), "MOVIE".into()],
        excluded_genres: vec!["Ecchi".into()],
        digest_hours: 0,
        daily_cap: 0,
        role_id: None,
        only_listed: false,
        last_digest_at: 0,
    }
}

fn entry(at: i64, pop: i64, format: &str, genres: &[&str]) -> AiringEntry {
    AiringEntry {
        airing_at: at,
        episode: 5,
        media: serde_json::from_value(json!({"id": 10, "title": {"romaji": "Show"}, "popularity": pop, "format": format, "genres": genres}))
            .expect("media"),
    }
}

fn due(c: &FeedConfig, e: &AiringEntry) -> Decision {
    decide(e, c, NOW, None, false, 0)
}

#[test]
fn only_due_episodes_are_posted() {
    let c = cfg();
    assert_eq!(due(&c, &entry(NOW - 60, 5000, "TV", &[])), Decision::Post);
    assert_eq!(
        due(&c, &entry(NOW, 5000, "TV", &[])),
        Decision::Post,
        "exactly now is due"
    );
    assert_eq!(
        due(&c, &entry(NOW + 1, 5000, "TV", &[])),
        Decision::Skip(Skip::NotDue),
        "AniList moved it later"
    );
}

#[test]
fn restart_catch_up_is_limited_to_six_hours() {
    let c = cfg();
    assert_eq!(
        due(&c, &entry(NOW - CATCH_UP_SECS, 5000, "TV", &[])),
        Decision::Post
    );
    assert_eq!(
        due(&c, &entry(NOW - CATCH_UP_SECS - 1, 5000, "TV", &[])),
        Decision::Skip(Skip::TooOld)
    );
}

#[test]
fn already_posted_episodes_are_never_repeated() {
    let c = cfg();
    assert_eq!(
        decide(&entry(NOW - 10, 9999, "TV", &[]), &c, NOW, None, true, 0),
        Decision::Skip(Skip::AlreadyPosted)
    );
}

#[test]
fn popularity_format_and_genre_filters() {
    let c = cfg();
    assert_eq!(
        due(&c, &entry(NOW - 10, 1499, "TV", &[])),
        Decision::Skip(Skip::BelowPopularity)
    );
    assert_eq!(due(&c, &entry(NOW - 10, 1500, "TV", &[])), Decision::Post);
    assert_eq!(
        due(&c, &entry(NOW - 10, 9999, "OVA", &[])),
        Decision::Skip(Skip::WrongFormat)
    );
    let mut no_format = entry(NOW - 10, 9999, "TV", &[]);
    no_format.media.format = None;
    assert_eq!(
        due(&c, &no_format),
        Decision::Skip(Skip::WrongFormat),
        "unknown format is not announced"
    );
    assert_eq!(
        due(&c, &entry(NOW - 10, 9999, "TV", &["Action", "ecchi"])),
        Decision::Skip(Skip::ExcludedGenre),
        "case-insensitive"
    );
}

#[test]
fn muted_and_pinned_lists() {
    let c = cfg();
    let small = entry(NOW - 10, 10, "OVA", &["Ecchi"]);
    assert_eq!(
        decide(&small, &c, NOW, Some(ListMode::Muted), false, 0),
        Decision::Skip(Skip::Muted)
    );
    assert_eq!(
        decide(
            &entry(NOW - 10, 99_999, "TV", &[]),
            &c,
            NOW,
            Some(ListMode::Muted),
            false,
            0
        ),
        Decision::Skip(Skip::Muted)
    );
    assert_eq!(
        decide(&small, &c, NOW, Some(ListMode::Pinned), false, 0),
        Decision::Post,
        "pins are always posted"
    );
    let listed_small = entry(NOW - 10, 10, "TV", &[]);
    assert_eq!(
        decide(&listed_small, &c, NOW, Some(ListMode::Listed), false, 0),
        Decision::Post,
        "listed shows skip the threshold"
    );
}

#[test]
fn only_listed_mode_ignores_everything_else() {
    let mut c = cfg();
    c.only_listed = true;
    assert_eq!(
        due(&c, &entry(NOW - 10, 99_999, "TV", &[])),
        Decision::Skip(Skip::NotListed)
    );
    assert_eq!(
        decide(
            &entry(NOW - 10, 1, "TV", &[]),
            &c,
            NOW,
            Some(ListMode::Listed),
            false,
            0
        ),
        Decision::Post
    );
    assert_eq!(
        decide(
            &entry(NOW - 10, 1, "OVA", &[]),
            &c,
            NOW,
            Some(ListMode::Pinned),
            false,
            0
        ),
        Decision::Post
    );
}

#[test]
fn daily_cap_and_digest() {
    let mut c = cfg();
    c.daily_cap = 3;
    let e = entry(NOW - 10, 5000, "TV", &[]);
    assert_eq!(decide(&e, &c, NOW, None, false, 2), Decision::Post);
    assert_eq!(
        decide(&e, &c, NOW, None, false, 3),
        Decision::Skip(Skip::DailyCap)
    );
    assert_eq!(
        decide(&e, &c, NOW, Some(ListMode::Pinned), false, 99),
        Decision::Post,
        "pins ignore the cap"
    );
    c.daily_cap = 0;
    c.digest_hours = 6;
    assert_eq!(decide(&e, &c, NOW, None, false, 0), Decision::Digest);
}

#[test]
fn poll_window_math() {
    assert_eq!(
        poll_window(0, NOW),
        (NOW - 900, NOW),
        "first run: just a short look back"
    );
    assert_eq!(
        poll_window(NOW - 300, NOW),
        (NOW - 300 - 900, NOW),
        "normal tick overlaps the last check"
    );
    assert_eq!(
        poll_window(NOW - 20 * 3600, NOW).0,
        NOW - CATCH_UP_SECS,
        "a long outage is capped at six hours"
    );
}

#[test]
fn day_start_and_digest_timing() {
    assert_eq!(day_start(86_400 * 3 + 5), 86_400 * 3);
    assert_eq!(day_start(86_400 * 3), 86_400 * 3);
    assert!(digest_due(6, NOW - 6 * 3600, NOW, 1));
    assert!(!digest_due(6, NOW - 6 * 3600 + 1, NOW, 1));
    assert!(!digest_due(6, 0, NOW, 0), "nothing queued, nothing to send");
    assert!(!digest_due(0, 0, NOW, 5), "digest mode off");
}

#[test]
fn embeds_show_episode_time_cover_and_streaming_link() {
    let m: Media = serde_json::from_value(json!({
        "id": 10, "title": {"romaji": "Shingeki", "english": "Attack"}, "averageScore": 85, "genres": ["Action", "Drama"],
        "coverImage": {"large": "https://c/l.png", "extraLarge": "https://c/xl.png", "color": "#e4a15d"},
        "siteUrl": "https://anilist.co/anime/10",
        "externalLinks": [{"url": "https://x.example/a", "site": "Wiki", "type": "INFO"}, {"url": "https://stream.example/a", "site": "S", "type": "STREAMING"}]
    }))
    .expect("media");
    let e = episode_embed(&m, 7, 1_800_000_000);
    assert_eq!(e.image.as_deref(), Some("https://c/xl.png"));
    assert_eq!(e.author.as_deref(), Some("Attack"));
    let d = e.description.as_deref().expect("d");
    assert!(
        d.contains("حلقة 7")
            && d.contains("<t:1800000000:F>")
            && d.contains("8.5/10")
            && d.contains("Action • Drama")
    );
    assert_eq!(
        m.streaming_link(),
        Some("https://stream.example/a"),
        "only streaming links are offered"
    );
    let digest = digest_embed(&[(10, 7, "Shingeki".into(), 1_800_000_000)]);
    assert!(digest
        .description
        .as_deref()
        .is_some_and(|d| d.contains("anilist.co/anime/10")));
}

#[test]
fn skipped_adult_media_never_reaches_the_decision() {
    // The shared fetch is safe-only; this documents that `decide` itself has no adult path.
    let c = cfg();
    let mut e = entry(NOW - 10, 99_999, "TV", &[]);
    e.media.is_adult = true;
    assert_eq!(
        due(&c, &e),
        Decision::Post,
        "filtering happens in SafeAniList::airing_between (tested there)"
    );
}
