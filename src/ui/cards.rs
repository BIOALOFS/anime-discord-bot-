//! Pure embed builders for media, characters, lists and airing info.

use super::{
    embeds::{parse_hex_colour, Emb},
    format::*,
};
use crate::{
    anilist::{
        kind_of,
        models::{Character, Media, MediaRef},
        AiringEntry, AiringInfo, MediaKind,
    },
    i18n,
};

fn year_of(m: &Media) -> Option<i32> {
    m.year()
}

fn subtitle(m: &Media) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    let main = m.title.main();
    for t in [&m.title.english, &m.title.native].into_iter().flatten() {
        if t != main && !t.is_empty() && !parts.contains(&t.as_str()) {
            parts.push(t);
        }
    }
    (!parts.is_empty()).then(|| format!("*{}*", parts.join(" / ")))
}

/// Arabic type tag: أنمي، مانجا، مانهوا، مانهوا صينية، رواية.
pub fn kind_label(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Anime => i18n::KIND_ANIME,
        MediaKind::Manga => i18n::KIND_MANGA,
        MediaKind::Manhwa => i18n::KIND_MANHWA,
        MediaKind::Manhua => i18n::KIND_MANHUA,
        MediaKind::Novel => i18n::KIND_NOVEL,
    }
}

pub fn flag(country: Option<&str>) -> &'static str {
    match country {
        Some("JP") => "🇯🇵",
        Some("KR") => "🇰🇷",
        Some("CN") => "🇨🇳",
        Some("TW") => "🇹🇼",
        _ => "",
    }
}

/// "🇰🇷 مانهوا · 2020"
pub fn type_line(m: &Media) -> String {
    let mut parts = vec![format!(
        "{} {}",
        flag(m.country_of_origin.as_deref()),
        kind_label(kind_of(m))
    )
    .trim()
    .to_string()];
    if let Some(y) = m.year() {
        parts.push(y.to_string());
    }
    parts.join(" · ")
}

fn count_text(m: &Media) -> (&'static str, String) {
    if kind_of(m).is_written() {
        let mut v = Vec::new();
        if let Some(c) = m.chapters {
            v.push(i18n::chapters_n(c));
        }
        if let Some(n) = m.volumes {
            v.push(i18n::volumes_n(n));
        }
        (
            i18n::F_CHAPTERS,
            if v.is_empty() {
                i18n::UNKNOWN.to_string()
            } else {
                v.join(" · ")
            },
        )
    } else {
        (
            i18n::F_EPISODES,
            m.episodes
                .map_or(i18n::UNKNOWN.to_string(), i18n::episodes_n),
        )
    }
}

/// The work card: title (link), vertical cover as the image, Arabic status, count, score.
/// Everything else is deliberately subtle; there is no long description.
pub fn work_card(m: &Media) -> Emb {
    let mut e = Emb::new()
        .title(m.title.main())
        .url(m.site_url.clone())
        .author(type_line(m))
        .colour(
            m.cover_image
                .as_ref()
                .and_then(|c| c.color.as_deref())
                .and_then(parse_hex_colour),
        )
        .image(
            m.cover_image
                .as_ref()
                .and_then(|c| c.extra_large.clone().or(c.large.clone())),
        );
    if let Some(sub) = subtitle(m).filter(|s| s.chars().count() <= 60) {
        e = e.description(sub);
    }
    let (count_label, count) = count_text(m);
    e = e
        .field(
            i18n::F_STATUS,
            m.status.as_deref().map_or(i18n::UNKNOWN, status_ar),
            true,
        )
        .field(count_label, count, true)
        .field(i18n::F_SCORE, score_text(m.average_score), true);
    if let Some(n) = &m.next_airing_episode {
        e = e.field(
            i18n::F_NEXT_EPISODE,
            format!(
                "{} · {}",
                i18n::episode_n(n.episode),
                ts_relative(n.airing_at)
            ),
            false,
        );
    }
    e
}

pub fn characters_embed(
    m: &Media,
    items: &[(Character, String)],
    page: usize,
    pages: usize,
) -> Emb {
    let mut lines = Vec::new();
    for (c, role) in items {
        let name = match &c.site_url {
            Some(u) => format!("[{}]({u})", c.display_name()),
            None => c.display_name().to_string(),
        };
        let native = c
            .name
            .native
            .as_deref()
            .filter(|n| !n.is_empty())
            .map(|n| format!(" ({n})"));
        let favs = c
            .favourites
            .filter(|f| *f > 0)
            .map(|f| format!(" · ❤ {}", thousands(f)));
        lines.push(format!(
            "**{name}**{}\n{}{}",
            native.unwrap_or_default(),
            role_ar(role),
            favs.unwrap_or_default()
        ));
    }
    let body = if lines.is_empty() {
        i18n::EMPTY_LIST.to_string()
    } else {
        lines.join("\n\n")
    };
    Emb::new()
        .title(format!("{} — {}", i18n::TITLE_CHARACTERS, m.title.main()))
        .url(m.site_url.clone())
        .colour(
            m.cover_image
                .as_ref()
                .and_then(|c| c.color.as_deref())
                .and_then(parse_hex_colour),
        )
        .thumbnail(m.cover().map(str::to_string))
        .description(body)
        .footer(format!("{} · {}/{}", i18n::POWERED_BY, page + 1, pages))
}

fn media_line(m: &Media) -> String {
    let title = match &m.site_url {
        Some(u) => format!("[{}]({u})", m.title.main()),
        None => m.title.main().to_string(),
    };
    let mut meta = Vec::new();
    if let Some(f) = &m.format {
        meta.push(format_ar(f).to_string());
    }
    if let Some(y) = year_of(m) {
        meta.push(y.to_string());
    }
    if let Some(s) = m.average_score {
        meta.push(format!("⭐ {:.1}", s as f32 / 10.0));
    }
    if meta.is_empty() {
        title
    } else {
        format!("{title}\n{}", meta.join(" · "))
    }
}

pub fn picker_embed(list: &[Media]) -> Emb {
    let body = list
        .iter()
        .enumerate()
        .map(|(i, m)| format!("**{}.** {}", i + 1, media_line(m).replace('\n', " · ")))
        .collect::<Vec<_>>()
        .join("\n");
    Emb::new().title(i18n::PICK_PROMPT).description(body)
}

pub fn picker_character_embed(list: &[Character]) -> Emb {
    let body = list
        .iter()
        .enumerate()
        .map(|(i, c)| match c.main_work() {
            Some(w) => format!("**{}.** {} — {w}", i + 1, c.display_name()),
            None => format!("**{}.** {}", i + 1, c.display_name()),
        })
        .collect::<Vec<_>>()
        .join("\n");
    Emb::new().title(i18n::PICK_PROMPT).description(body)
}

fn work_line(r: &MediaRef) -> Option<String> {
    let title = r.title.as_ref()?.main();
    let mut meta = Vec::new();
    if let Some(f) = &r.format {
        meta.push(format_ar(f).to_string());
    }
    if let Some(y) = r.season_year {
        meta.push(y.to_string());
    }
    let label = match &r.site_url {
        Some(u) => format!("[{title}]({u})"),
        None => title.to_string(),
    };
    Some(if meta.is_empty() {
        format!("• {label}")
    } else {
        format!("• {label} ({})", meta.join(" · "))
    })
}

/// Up to 5 nicknames: plain alternatives first, then spoiler ones wrapped in `||`.
pub fn nicknames(c: &Character) -> Vec<String> {
    c.name
        .alternative
        .iter()
        .filter(|a| !a.is_empty())
        .cloned()
        .chain(
            c.name
                .alternative_spoiler
                .iter()
                .filter(|a| !a.is_empty())
                .map(|a| format!("||{a}||")),
        )
        .take(5)
        .collect()
}

/// The simplified character card: image, name (+ native), main work link, nicknames, footer.
pub fn character_card(c: &Character) -> Emb {
    let title = match c.name.native.as_deref().filter(|n| !n.is_empty()) {
        Some(n) => format!("{} ({n})", c.display_name()),
        None => c.display_name().to_string(),
    };
    let mut lines = Vec::new();
    if let Some(work) = c.media.as_ref().and_then(|m| m.nodes.first()) {
        if let Some(t) = &work.title {
            lines.push(match &work.site_url {
                Some(u) => format!("📺 [{}]({u})", t.main()),
                None => format!("📺 {}", t.main()),
            });
        }
    }
    let nicks = nicknames(c);
    if !nicks.is_empty() {
        lines.push(format!("**{}:** {}", i18n::NICKNAMES, nicks.join("، ")));
    }
    let mut e = Emb::new()
        .title(title)
        .url(c.site_url.clone())
        .image(c.image_url().map(str::to_string));
    if !lines.is_empty() {
        e = e.description(lines.join("\n"));
    }
    e
}

pub fn works_embed(c: &Character, works: &[&MediaRef], page: usize, pages: usize) -> Emb {
    let body = works
        .iter()
        .filter_map(|r| work_line(r))
        .collect::<Vec<_>>()
        .join("\n");
    Emb::new()
        .title(format!("{} — {}", i18n::TITLE_WORKS, c.display_name()))
        .url(c.site_url.clone())
        .thumbnail(c.image_url().map(str::to_string))
        .description(if body.is_empty() {
            i18n::EMPTY_LIST.to_string()
        } else {
            body
        })
        .footer(format!("{} · {}/{}", i18n::POWERED_BY, page + 1, pages))
}

pub fn airing_info_embed(info: &AiringInfo) -> Emb {
    let m = &info.media;
    let mut e = Emb::new()
        .title(m.title.main())
        .url(m.site_url.clone())
        .colour(
            m.cover_image
                .as_ref()
                .and_then(|c| c.color.as_deref())
                .and_then(parse_hex_colour),
        )
        .thumbnail(m.cover().map(str::to_string));
    match &m.next_airing_episode {
        Some(n) => {
            e = e.field(
                i18n::F_NEXT_EPISODE,
                format!(
                    "{}\n{}\n{}",
                    i18n::episode_n(n.episode),
                    ts_relative(n.airing_at),
                    ts_full(n.airing_at)
                ),
                false,
            );
        }
        None if m.status.as_deref() == Some("FINISHED") => e = e.description(i18n::AIRING_FINISHED),
        None => e = e.description(i18n::AIRING_UNSCHEDULED),
    }
    if let Some(prev) = info.previous_at {
        e = e.field(i18n::F_PREV_EPISODE, ts_full(prev), true);
    }
    if let Some(total) = m.episodes {
        e = e.field(i18n::F_TOTAL_EPISODES, thousands(i64::from(total)), true);
    }
    if let Some(s) = &m.status {
        e = e.field(i18n::F_STATUS, status_ar(s), true);
    }
    e
}

pub fn airing_day_embed(
    day_label: &str,
    entries: &[&AiringEntry],
    page: usize,
    pages: usize,
) -> Emb {
    let body = entries
        .iter()
        .map(|a| {
            let title = match &a.media.site_url {
                Some(u) => format!("[{}]({u})", a.media.title.main()),
                None => a.media.title.main().to_string(),
            };
            format!(
                "{} — {title} · {}",
                ts_time(a.airing_at),
                i18n::episode_n(a.episode)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Emb::new()
        .title(format!("{} — {day_label}", i18n::AIRING_WEEK_TITLE))
        .description(format!("{body}\n\n*{}*", i18n::AIRING_TZ_NOTE))
        .footer(format!("{} · {}/{}", i18n::POWERED_BY, page + 1, pages))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn media(v: serde_json::Value) -> Media {
        serde_json::from_value(v).expect("media")
    }

    #[test]
    fn simple_card_has_only_the_essentials() {
        let c: Character = serde_json::from_value(json!({
            "id": 5, "name": {"full": "Hero", "native": "勇者", "alternative": ["A", "B", "C", "D"], "alternativeSpoiler": ["S1", "S2"]},
            "image": {"large": "https://img"}, "description": "A very long description that must not appear",
            "age": "17", "favourites": 99, "siteUrl": "https://anilist.co/character/5",
            "media": {"nodes": [{"id": 1, "title": {"romaji": "Show"}, "siteUrl": "https://anilist.co/anime/1", "isAdult": false, "genres": []}]}
        }))
        .expect("character");
        let e = character_card(&c);
        assert_eq!(e.title.as_deref(), Some("Hero (勇者)"));
        assert_eq!(e.image.as_deref(), Some("https://img"));
        assert!(e.fields.is_empty(), "no fields");
        let d = e.description.as_deref().expect("desc");
        assert!(d.contains("📺 [Show](https://anilist.co/anime/1)"));
        assert!(!d.contains("long description"), "no description text");
        assert!(
            d.contains("A، B، C، D، ||S1||") && !d.contains("S2"),
            "max 5 nicknames, spoilers wrapped"
        );
        assert_eq!(e.footer.as_deref(), Some("Powered by AniList"));
    }

    #[test]
    fn work_card_layout() {
        let m = media(json!({
            "id": 1, "title": {"romaji": "Solo Leveling", "english": "Solo Leveling", "native": "나 혼자만 레벨업"},
            "description": "A very long description that must never appear on the card",
            "coverImage": {"large": "https://c/l.png", "extraLarge": "https://c/xl.png", "color": "#e4a15d"},
            "bannerImage": "https://banner", "averageScore": 85, "chapters": 179, "volumes": 14, "status": "FINISHED",
            "format": "MANGA", "countryOfOrigin": "KR", "startDate": {"year": 2018}, "siteUrl": "https://anilist.co/manga/1"
        }));
        let e = work_card(&m);
        assert_eq!(e.author.as_deref(), Some("🇰🇷 مانهوا · 2018"));
        assert_eq!(
            e.image.as_deref(),
            Some("https://c/xl.png"),
            "the vertical cover is the embed image, not the banner"
        );
        assert!(e.thumbnail.is_none());
        assert_eq!(e.colour, Some(0xe4a15d));
        let get = |n: &str| e.fields.iter().find(|f| f.0 == n).map(|f| f.1.clone());
        assert_eq!(get("الحالة").as_deref(), Some("مكتمل"));
        assert_eq!(get("الفصول").as_deref(), Some("179 فصل · 14 مجلد"));
        assert_eq!(get("التقييم").as_deref(), Some("8.5/10 ▰▰▰▰▰▰▰▰▰▱"));
        assert!(!e
            .description
            .as_deref()
            .unwrap_or("")
            .contains("long description"));
        assert!(e.fields.iter().all(|f| f.0 != "الحلقات"));
    }

    #[test]
    fn anime_card_counts_episodes_and_shows_the_countdown() {
        let m = media(json!({
            "id": 2, "title": {"romaji": "Show"}, "episodes": 12, "status": "RELEASING", "format": "TV", "countryOfOrigin": "JP",
            "seasonYear": 2026, "nextAiringEpisode": {"airingAt": 1_800_000_000, "episode": 5}
        }));
        let e = work_card(&m);
        assert_eq!(e.author.as_deref(), Some("🇯🇵 أنمي · 2026"));
        let get = |n: &str| e.fields.iter().find(|f| f.0 == n).map(|f| f.1.clone());
        assert_eq!(get("الحلقات").as_deref(), Some("12 حلقة"));
        assert_eq!(get("الحالة").as_deref(), Some("مستمر"));
        assert!(get("الحلقة القادمة").is_some_and(|v| v.contains("<t:1800000000:R>")));
    }

    #[test]
    fn unknown_values_never_break_the_card() {
        let m = media(json!({"id": 3, "title": {"romaji": "x".repeat(400)}}));
        let mut e = work_card(&m);
        e.enforce();
        assert!(e.total_len() <= super::super::embeds::MAX_TOTAL);
        assert!(e.fields.iter().any(|f| f.1 == "غير معروف"));
    }

    #[test]
    fn kind_labels_and_flags() {
        assert_eq!(kind_label(MediaKind::Manhua), "مانهوا صينية");
        assert_eq!(kind_label(MediaKind::Novel), "رواية");
        assert_eq!(flag(Some("KR")), "🇰🇷");
        assert_eq!(flag(None), "");
    }
}
