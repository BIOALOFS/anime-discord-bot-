//! Pure embed builders for media, characters, lists and airing info.

use super::{
    embeds::{parse_hex_colour, Emb},
    format::*,
    text::clean_description,
};
use crate::{
    anilist::{
        models::{Character, Media, MediaRef},
        AiringEntry, AiringInfo,
    },
    i18n,
};

fn year_of(m: &Media) -> Option<i32> {
    m.season_year
}

fn is_manga(m: &Media) -> bool {
    matches!(m.format.as_deref(), Some("MANGA" | "NOVEL" | "ONE_SHOT"))
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

fn author_or_studio(m: &Media) -> Option<(&'static str, String)> {
    if is_manga(m) {
        let edges = &m.staff.as_ref()?.edges;
        let story = edges
            .iter()
            .filter(|e| e.role.as_deref().is_some_and(|r| r.contains("Story")))
            .chain(edges.iter())
            .filter_map(|e| e.node.name.full.clone())
            .take(2)
            .collect::<Vec<_>>();
        let mut story = story;
        story.dedup();
        (!story.is_empty()).then(|| (i18n::F_AUTHOR, story.join("، ")))
    } else {
        let names: Vec<_> = m
            .studios
            .as_ref()?
            .nodes
            .iter()
            .take(2)
            .map(|s| s.name.clone())
            .collect();
        (!names.is_empty()).then(|| (i18n::F_STUDIO, names.join("، ")))
    }
}

pub fn media_embed(m: &Media) -> Emb {
    let mut e = Emb::new()
        .title(m.title.main())
        .url(m.site_url.clone())
        .colour(
            m.cover_image
                .as_ref()
                .and_then(|c| c.color.as_deref())
                .and_then(parse_hex_colour),
        );

    let mut author = Vec::new();
    if let Some(f) = &m.format {
        author.push(format_ar(f).to_string());
    }
    if let Some(y) = year_of(m) {
        author.push(y.to_string());
    }
    if !author.is_empty() {
        e = e.author(author.join(" · "));
    }

    let mut desc = subtitle(m).unwrap_or_default();
    if let Some(d) = m
        .description
        .as_deref()
        .and_then(|d| clean_description(d, m.site_url.as_deref()))
    {
        if !desc.is_empty() {
            desc.push_str("\n\n");
        }
        desc.push_str(&d);
    }
    if !desc.is_empty() {
        e = e.description(desc);
    }

    // Banner as the large image when there is one, otherwise the cover as a thumbnail.
    e = match &m.banner_image {
        Some(b) => e.image(Some(b.clone())),
        None => e.thumbnail(m.cover().map(str::to_string)),
    };

    e = e
        .field(i18n::F_SCORE, score_text(m.average_score), true)
        .field(
            i18n::F_POPULARITY,
            m.popularity.map_or(i18n::NONE.to_string(), thousands),
            true,
        );
    let (count_label, count) = if is_manga(m) {
        (i18n::F_CHAPTERS, m.chapters)
    } else {
        (i18n::F_EPISODES, m.episodes)
    };
    if let Some(n) = count {
        e = e.field(count_label, thousands(i64::from(n)), true);
    }
    if let Some(s) = &m.status {
        e = e.field(i18n::F_STATUS, status_ar(s), true);
    }
    if let Some(s) = &m.season {
        let v = match year_of(m) {
            Some(y) => format!("{} {y}", season_ar(s)),
            None => season_ar(s).to_string(),
        };
        e = e.field(i18n::F_SEASON, v, true);
    }
    if let Some((label, v)) = author_or_studio(m) {
        e = e.field(label, v, true);
    }
    if let Some(d) = m.duration {
        e = e.field(i18n::F_DURATION, i18n::minutes(d), true);
    }
    if let Some(s) = &m.source {
        e = e.field(i18n::F_SOURCE, source_ar(s), true);
    }
    if !m.genres.is_empty() {
        let tags: Vec<_> = m.genres.iter().take(6).cloned().collect();
        e = e.field(i18n::F_GENRES, tags.join(" • "), false);
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

pub fn similar_embed(m: &Media, items: &[Media], page: usize, pages: usize) -> Emb {
    let body = if items.is_empty() {
        i18n::EMPTY_LIST.to_string()
    } else {
        items
            .iter()
            .map(media_line)
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    Emb::new()
        .title(format!("{} {}", i18n::TITLE_SIMILAR, m.title.main()))
        .thumbnail(m.cover().map(str::to_string))
        .description(body)
        .footer(format!("{} · {}/{}", i18n::POWERED_BY, page + 1, pages))
}

pub fn related_embed(m: &Media, items: &[(String, Media)]) -> Emb {
    let body = if items.is_empty() {
        i18n::EMPTY_LIST.to_string()
    } else {
        items
            .iter()
            .map(|(rel, media)| format!("**{}**\n{}", relation_ar(rel), media_line(media)))
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    Emb::new()
        .title(format!("{} {}", i18n::TITLE_RELATED, m.title.main()))
        .thumbnail(m.cover().map(str::to_string))
        .description(body)
}

/// Relation types shown first, in this order.
pub fn relation_rank(rel: &str) -> usize {
    [
        "PREQUEL",
        "SEQUEL",
        "PARENT",
        "SIDE_STORY",
        "SPIN_OFF",
        "ALTERNATIVE",
        "ADAPTATION",
        "SOURCE",
    ]
    .iter()
    .position(|r| *r == rel)
    .unwrap_or(usize::MAX)
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

pub fn seasonal_embed(title: &str, items: &[Media], page: usize, pages: usize) -> Emb {
    let body = if items.is_empty() {
        i18n::EMPTY_LIST.to_string()
    } else {
        items
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let mut bits = Vec::new();
                bits.push(format!(
                    "⭐ {}",
                    m.average_score
                        .map_or("—".into(), |s| format!("{:.1}", s as f32 / 10.0))
                ));
                if let Some(n) = m.episodes {
                    bits.push(i18n::episodes_n(n));
                }
                if let Some(g) = m.genres.first() {
                    bits.push(g.clone());
                }
                let title = match &m.site_url {
                    Some(u) => format!("[{}]({u})", m.title.main()),
                    None => m.title.main().to_string(),
                };
                format!("**{}.** {title}\n{}", page * 10 + i + 1, bits.join(" · "))
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    Emb::new().title(title).description(body).footer(format!(
        "{} · {}/{}",
        i18n::POWERED_BY,
        page + 1,
        pages
    ))
}

fn birthday(c: &Character) -> Option<String> {
    let d = c.date_of_birth.as_ref()?;
    let mut parts = Vec::new();
    if let Some(day) = d.day {
        parts.push(day.to_string());
    }
    if let Some(m) = d.month.map(month_ar).filter(|m| !m.is_empty()) {
        parts.push(m.to_string());
    }
    if let Some(y) = d.year {
        parts.push(y.to_string());
    }
    (!parts.is_empty()).then(|| parts.join(" "))
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

/// `server_favourites` is a placeholder (0) until profiles exist in milestone 4.
pub fn character_embed(c: &Character, works: &[&MediaRef], server_favourites: i64) -> Emb {
    let mut names = vec![format!("**{}**", c.display_name())];
    if let Some(n) = c.name.native.as_deref().filter(|n| !n.is_empty()) {
        names.push(n.to_string());
    }
    let mut alts: Vec<String> = c
        .name
        .alternative
        .iter()
        .filter(|a| !a.is_empty())
        .cloned()
        .collect();
    alts.extend(
        c.name
            .alternative_spoiler
            .iter()
            .filter(|a| !a.is_empty())
            .map(|a| format!("||{a}||")),
    );
    let mut desc = names.join(" / ");
    if !alts.is_empty() {
        desc.push_str(&format!("\n*{}*", alts.join("، ")));
    }
    if let Some(d) = c
        .description
        .as_deref()
        .and_then(|d| clean_description(d, c.site_url.as_deref()))
    {
        desc.push_str("\n\n");
        desc.push_str(&d);
    }
    let mut e = Emb::new()
        .title(c.display_name())
        .url(c.site_url.clone())
        .description(desc)
        .image(c.image_url().map(str::to_string));
    if let Some(a) = c.age.as_deref().filter(|a| !a.is_empty()) {
        e = e.field(i18n::F_AGE, a, true);
    }
    if let Some(g) = c.gender.as_deref().filter(|g| !g.is_empty()) {
        e = e.field(i18n::F_GENDER, gender_ar(g), true);
    }
    if let Some(b) = birthday(c) {
        e = e.field(i18n::F_BIRTHDAY, b, true);
    }
    if let Some(b) = c.blood_type.as_deref().filter(|b| !b.is_empty()) {
        e = e.field(i18n::F_BLOOD, b, true);
    }
    let top: Vec<String> = works.iter().filter_map(|r| work_line(r)).take(5).collect();
    if !top.is_empty() {
        e = e.field(i18n::F_APPEARANCES, top.join("\n"), false);
    }
    e = e.field(
        i18n::F_ANILIST_FAVS,
        c.favourites.map_or(i18n::NONE.to_string(), thousands),
        true,
    );
    e.field(
        i18n::F_SERVER_FAVS,
        i18n::server_favs(server_favourites),
        true,
    )
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
    fn anime_card_layout() {
        let m = media(json!({
            "id": 1, "title": {"romaji": "Shingeki", "english": "Attack", "native": "進撃"},
            "description": "Hello<br>world", "bannerImage": "https://b", "coverImage": {"large": "https://c", "color": "#e4a15d"},
            "averageScore": 85, "popularity": 1234567, "episodes": 25, "status": "FINISHED", "season": "SPRING",
            "seasonYear": 2013, "format": "TV", "duration": 24, "source": "MANGA", "siteUrl": "https://anilist.co/anime/1",
            "genres": ["Action", "Drama", "Fantasy", "Mystery", "A", "B", "C", "D"],
            "studios": {"nodes": [{"name": "Wit"}]}
        }));
        let e = media_embed(&m);
        assert_eq!(e.author.as_deref(), Some("مسلسل · 2013"));
        assert_eq!(e.colour, Some(0xe4a15d));
        assert!(
            e.image.is_some() && e.thumbnail.is_none(),
            "banner wins over cover"
        );
        assert!(e
            .description
            .as_deref()
            .is_some_and(|d| d.starts_with("*Attack / 進撃*")));
        let names: Vec<&str> = e.fields.iter().map(|f| f.0.as_str()).collect();
        for want in [
            "التقييم",
            "الشعبية",
            "الحلقات",
            "الحالة",
            "الموسم",
            "الاستوديو",
            "المدة",
            "المصدر",
            "التصنيفات",
        ] {
            assert!(names.contains(&want), "missing field {want}");
        }
        let genres = e
            .fields
            .iter()
            .find(|f| f.0 == "التصنيفات")
            .expect("genres");
        assert_eq!(genres.1.matches('•').count(), 5, "at most 6 tags");
        assert_eq!(e.footer.as_deref(), Some("Powered by AniList"));
    }

    #[test]
    fn cover_is_thumbnail_without_banner_and_manga_uses_author() {
        let m = media(json!({
            "id": 2, "title": {"romaji": "Manga"}, "coverImage": {"large": "https://c"}, "format": "MANGA", "chapters": 100,
            "staff": {"edges": [{"role": "Art", "node": {"name": {"full": "Artist"}}}, {"role": "Story", "node": {"name": {"full": "Writer"}}}]}
        }));
        let e = media_embed(&m);
        assert!(e.image.is_none() && e.thumbnail.as_deref() == Some("https://c"));
        let author = e
            .fields
            .iter()
            .find(|f| f.0 == "المؤلف")
            .expect("author field");
        assert!(author.1.starts_with("Writer"));
        assert!(e.fields.iter().any(|f| f.0 == "الفصول"));
        assert!(!e.fields.iter().any(|f| f.0 == "الاستوديو"));
    }

    #[test]
    fn card_survives_enforcement() {
        let m = media(
            json!({"id": 3, "title": {"romaji": "x".repeat(400)}, "genres": ["g".repeat(2000)]}),
        );
        let mut e = media_embed(&m);
        e.enforce();
        assert!(e.total_len() <= super::super::embeds::MAX_TOTAL);
    }

    #[test]
    fn character_card_has_spoilers_and_placeholder_count() {
        let c: Character = serde_json::from_value(json!({
            "id": 5, "name": {"full": "Hero", "native": "勇者", "alternative": ["Yuu"], "alternativeSpoiler": ["Secret"]},
            "age": "17", "gender": "Female", "dateOfBirth": {"year": null, "month": 11, "day": 14},
            "bloodType": "A", "favourites": 4321, "description": "Brave ~!and strong!~",
            "media": {"nodes": [{"id": 1, "title": {"romaji": "Show"}, "format": "TV", "seasonYear": 2020, "isAdult": false, "genres": []}]}
        }))
        .expect("character");
        let works: Vec<&MediaRef> = c.media.as_ref().expect("m").nodes.iter().collect();
        let e = character_embed(&c, &works, 0);
        let d = e.description.as_deref().expect("desc");
        assert!(d.contains("||Secret||") && d.contains("||and strong||"));
        let get = |n: &str| e.fields.iter().find(|f| f.0 == n).map(|f| f.1.clone());
        assert_eq!(get("الجنس").as_deref(), Some("أنثى"));
        assert_eq!(get("تاريخ الميلاد").as_deref(), Some("14 نوفمبر"));
        assert_eq!(get("المفضلة على AniList").as_deref(), Some("4,321"));
        assert_eq!(
            get("في مفضلة السيرفر").as_deref(),
            Some("مفضلة عند 0 من أعضاء السيرفر")
        );
        assert!(get("أبرز الأعمال").is_some_and(|w| w.contains("Show")));
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
    fn relation_ordering() {
        assert!(relation_rank("PREQUEL") < relation_rank("SEQUEL"));
        assert!(relation_rank("SEQUEL") < relation_rank("SIDE_STORY"));
        assert_eq!(relation_rank("OTHER"), usize::MAX);
    }
}
