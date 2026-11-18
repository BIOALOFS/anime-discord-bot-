//! `/top`: the server's rankings. One message with a hero card for the first place, the places 2–10 as a list, a
//! section menu, a time range, pages, and a menu that opens any listed item as a new message. Server only: nothing
//! here ever ranks across servers. Results are cached for five minutes.

use super::{buttons, say_ephemeral};
use crate::{
    components,
    db::notify,
    error::{Context, Error},
    i18n,
    tmdb::model as tm,
    ui::{
        embeds::{fit, Emb},
        pagination::{apply_nav, close_button, nav_row, page_count, parse_nav, CLOSE},
        select::{select_row, Choice},
        session::{self, Outcome, Page, View},
    },
};
use moka::future::Cache;
use sqlx::{Row as _, SqlitePool};
use std::{collections::HashMap, sync::OnceLock, time::Duration};

const SECTION_ID: &str = "top_section";
const RANGE_ID: &str = "top_range";
const PER_PAGE: usize = 10;
/// A top list never needs more than this many entries.
const MAX_ROWS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Anime,
    Manga,
    Novels,
    Drama,
    Series,
    Movies,
    Games,
    Characters,
    Members,
    Events,
}

/// `(key, Arabic label, section)` in the order of the menu.
pub const SECTIONS: &[(&str, &str, Section)] = &[
    ("anime", "أنمي", Section::Anime),
    ("manga", "مانجا", Section::Manga),
    ("novels", "روايات", Section::Novels),
    ("drama", "دراما", Section::Drama),
    ("series", "مسلسلات", Section::Series),
    ("movies", "أفلام", Section::Movies),
    ("games", "ألعاب", Section::Games),
    ("characters", "شخصيات", Section::Characters),
    ("members", "الأعضاء", Section::Members),
    ("events", "نقاط الفعاليات", Section::Events),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Range {
    Week,
    Month,
    All,
}

pub const RANGES: &[(&str, &str, Range)] = &[
    ("week", "هذا الأسبوع", Range::Week),
    ("month", "هذا الشهر", Range::Month),
    ("all", "كل الأوقات", Range::All),
];

impl Range {
    /// Unix time from which additions count (`0` = everything).
    fn since(self, now: i64) -> i64 {
        match self {
            Self::Week => now - 7 * 86_400,
            Self::Month => now - 30 * 86_400,
            Self::All => 0,
        }
    }
}

impl Section {
    fn work_kinds(self) -> &'static [&'static str] {
        match self {
            Self::Anime => &["anime"],
            Self::Manga => &["manga", "manhwa", "manhua"],
            Self::Novels => &["novel"],
            Self::Drama | Self::Series => &["tmdb-tv"],
            Self::Movies => &["tmdb-movie"],
            Self::Games => &["game", "vn"],
            _ => &[],
        }
    }
}

/// One ranked entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Menu value: a work key, a character id / key, or a member id.
    pub key: String,
    pub label: String,
    pub count: i64,
    pub image: Option<String>,
    pub sub: Option<String>,
}

/// Only pictures from hosts that allow embedding are shown on the hero card (game covers are never hotlinked).
pub fn hero_image_ok(url: &str) -> bool {
    url.starts_with("https://s4.anilist.co/") || url.starts_with("https://image.tmdb.org/")
}

/// The menu value that opens a stored character id as a new message.
pub fn character_key(id: i64) -> String {
    if let Some(key) = crate::people::decode_person_id(id) {
        return key;
    }
    if let Some((kind, work, person)) = tm::decode_cast_id(id) {
        return format!(
            "tmdb-cast:{}:{work}:{person}",
            if kind == tm::Kind::Tv { "tv" } else { "movie" }
        );
    }
    if id >= 2_000_000_000_000 {
        return format!("igdb:c{}", id - 2_000_000_000_000);
    }
    if id >= crate::providers::PROVIDER_CHARACTER_MIN {
        return format!("vndb:c{}", id - crate::providers::PROVIDER_CHARACTER_MIN);
    }
    id.to_string()
}

type Cached = std::sync::Arc<Vec<Row>>;

fn cache() -> &'static Cache<(i64, Section, Range), Cached> {
    static C: OnceLock<Cache<(i64, Section, Range), Cached>> = OnceLock::new();
    C.get_or_init(|| {
        Cache::builder()
            .max_capacity(2_000)
            .time_to_live(Duration::from_secs(300))
            .build()
    })
}

const MEMBERS_OF: &str = "pw.user_id IN (SELECT user_id FROM user_guilds WHERE guild_id = ?1)";

async fn work_rows(
    pool: &SqlitePool,
    guild: i64,
    section: Section,
    since: i64,
    overrides: &HashMap<String, tm::Target>,
) -> Result<Vec<Row>, sqlx::Error> {
    let kinds = section.work_kinds();
    let marks = (0..kinds.len())
        .map(|i| format!("?{}", i + 3))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT pw.work_id AS id, count(*) AS n, mc.title_romaji AS title, mc.cover_url AS cover, mc.country AS country, mc.year AS year \
         FROM profile_works pw JOIN profiles p ON p.user_id = pw.user_id AND p.privacy != 'private' \
         LEFT JOIN media_cache mc ON mc.work_id = pw.work_id \
         WHERE {MEMBERS_OF} AND pw.kind IN ({marks}) AND pw.added_at >= ?2 AND COALESCE(mc.is_safe, 0) = 1 \
         GROUP BY pw.work_id ORDER BY n DESC, pw.work_id LIMIT {MAX_ROWS}"
    );
    let mut q = sqlx::query(&sql).bind(guild).bind(since);
    for k in kinds {
        q = q.bind(*k);
    }
    let mut out = Vec::new();
    for r in q.fetch_all(pool).await? {
        let country: Option<String> = r.get("country");
        // Shows are split between /drama and /series by their country, with the server's overrides.
        if matches!(section, Section::Drama | Section::Series) {
            let target = tm::classify(&country.iter().cloned().collect::<Vec<_>>(), overrides);
            if (section == Section::Drama) != (target == tm::Target::Drama) {
                continue;
            }
        }
        let year: Option<i64> = r.get("year");
        out.push(Row {
            key: r.get("id"),
            label: r.get::<Option<String>, _>("title").unwrap_or_default(),
            count: r.get("n"),
            image: r.get("cover"),
            sub: year.map(|y| y.to_string()),
        });
    }
    Ok(out)
}

async fn character_rows(
    pool: &SqlitePool,
    guild: i64,
    since: i64,
) -> Result<Vec<Row>, sqlx::Error> {
    let rows = sqlx::query(&format!(
        "SELECT pc.character_id AS id, count(*) AS n, cc.name_romaji AS name, cc.image_url AS image, cc.main_work AS work \
         FROM profile_characters pc JOIN profiles p ON p.user_id = pc.user_id AND p.privacy != 'private' \
         JOIN character_cache cc ON cc.character_id = pc.character_id AND cc.is_safe = 1 \
         WHERE pc.user_id IN (SELECT user_id FROM user_guilds WHERE guild_id = ?1) AND pc.added_at >= ?2 \
         GROUP BY pc.character_id ORDER BY n DESC, pc.character_id LIMIT {MAX_ROWS}"
    ))
    .bind(guild)
    .bind(since)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Row {
            key: character_key(r.get("id")),
            label: r.get("name"),
            count: r.get("n"),
            image: r.get("image"),
            sub: r.get("work"),
        })
        .collect())
}

/// Members ranked by how much they added in the range (characters and works together).
async fn member_rows(pool: &SqlitePool, guild: i64, since: i64) -> Result<Vec<Row>, sqlx::Error> {
    let rows = sqlx::query(&format!(
        "SELECT t.user_id AS id, sum(t.n) AS n FROM ( \
           SELECT pc.user_id AS user_id, count(*) AS n FROM profile_characters pc WHERE pc.added_at >= ?2 GROUP BY pc.user_id \
           UNION ALL SELECT pw.user_id AS user_id, count(*) AS n FROM profile_works pw WHERE pw.added_at >= ?2 GROUP BY pw.user_id) t \
         JOIN profiles p ON p.user_id = t.user_id AND p.privacy != 'private' \
         WHERE t.user_id IN (SELECT ug.user_id FROM user_guilds ug WHERE ug.guild_id = ?1) \
         GROUP BY t.user_id ORDER BY n DESC, t.user_id LIMIT {MAX_ROWS}"
    ))
    .bind(guild)
    .bind(since)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Row {
            key: r.get::<i64, _>("id").to_string(),
            label: format!("<@{}>", r.get::<i64, _>("id")),
            count: r.get("n"),
            image: None,
            sub: None,
        })
        .collect())
}

/// Event points: this week, the last four weeks, or everything (correct answers).
async fn event_rows(
    pool: &SqlitePool,
    guild: i64,
    range: Range,
    now: i64,
) -> Result<Vec<Row>, sqlx::Error> {
    let weeks: Vec<String> = match range {
        Range::Week => vec![crate::db::game::week_key(now)],
        Range::Month => (0..4)
            .map(|i| crate::db::game::week_key(now - i * 7 * 86_400))
            .collect(),
        Range::All => Vec::new(),
    };
    let rows: Vec<(i64, i64)> = if range == Range::All {
        sqlx::query_as("SELECT user_id, SUM(correct) FROM game_scores WHERE guild_id = ?1 GROUP BY user_id HAVING SUM(correct) > 0 ORDER BY SUM(correct) DESC, user_id LIMIT 300").bind(guild).fetch_all(pool).await?
    } else {
        let marks = (0..weeks.len())
            .map(|i| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT user_id, SUM(points) FROM weekly_scores WHERE guild_id = ?1 AND week IN ({marks}) GROUP BY user_id HAVING SUM(points) > 0 ORDER BY SUM(points) DESC, user_id LIMIT 300");
        let mut q = sqlx::query_as(&sql).bind(guild);
        for w in &weeks {
            q = q.bind(w.clone());
        }
        q.fetch_all(pool).await?
    };
    Ok(rows
        .into_iter()
        .map(|(u, n)| Row {
            key: u.to_string(),
            label: format!("<@{u}>"),
            count: n,
            image: None,
            sub: None,
        })
        .collect())
}

/// The ranked rows of a section for a server (cached for five minutes).
pub async fn rows_for(
    pool: &SqlitePool,
    guild: i64,
    section: Section,
    range: Range,
) -> Result<Cached, sqlx::Error> {
    if let Some(hit) = cache().get(&(guild, section, range)).await {
        return Ok(hit);
    }
    let now = chrono::Utc::now().timestamp();
    let since = range.since(now);
    let rows = match section {
        Section::Characters => character_rows(pool, guild, since).await?,
        Section::Members => member_rows(pool, guild, since).await?,
        Section::Events => event_rows(pool, guild, range, now).await?,
        s => {
            let overrides = notify::drama_overrides(pool, guild)
                .await
                .unwrap_or_default();
            work_rows(pool, guild, s, since, &overrides).await?
        }
    };
    let rows = std::sync::Arc::new(rows);
    cache().insert((guild, section, range), rows.clone()).await;
    Ok(rows)
}

fn medal(rank: usize) -> String {
    match rank {
        1 => "🥇".into(),
        2 => "🥈".into(),
        3 => "🥉".into(),
        n => format!("**{n}.**"),
    }
}

fn unit(section: Section) -> &'static str {
    match section {
        Section::Members => "إضافة",
        Section::Events => "نقطة",
        _ => "عضو",
    }
}

/// The text of one page: the hero card on the first page, then the list.
pub fn page_embed(
    section: Section,
    label: &str,
    range_label: &str,
    rows: &[Row],
    page: usize,
) -> Emb {
    let pages = page_count(rows.len(), PER_PAGE);
    let page = page.min(pages - 1);
    let start = page * PER_PAGE;
    let slice = &rows[start.min(rows.len())..(start + PER_PAGE).min(rows.len())];
    let mut e = Emb::new().title(format!("🏆 {label} — {range_label}"));
    if slice.is_empty() {
        e = e.description(i18n::TOP_EMPTY);
        return e;
    }
    let mut lines = Vec::new();
    for (i, r) in slice.iter().enumerate() {
        let rank = start + i + 1;
        if rank == 1 {
            // The hero card: the winner gets the picture and a line of its own.
            let sub = r
                .sub
                .as_deref()
                .map(|s| format!(" · {s}"))
                .unwrap_or_default();
            e = e.description(format!(
                "👑 **{}**{sub}\n{} {} {}",
                fit(&r.label, 150),
                r.count,
                unit(section),
                medal(1)
            ));
            if let Some(img) = r.image.as_deref().filter(|u| hero_image_ok(u)) {
                e = e.image(Some(img.to_string()));
            }
            continue;
        }
        lines.push(format!(
            "{} {} — {} {}",
            medal(rank),
            fit(&r.label, 80),
            r.count,
            unit(section)
        ));
    }
    if !lines.is_empty() {
        let hero = e.description.take().unwrap_or_default();
        let list = lines.join("\n");
        e.description = Some(if hero.is_empty() {
            list
        } else {
            format!("{hero}\n\n{list}")
        });
    }
    e.footer = Some(format!("{}/{} · {}", page + 1, pages, i18n::TOP_CACHE_NOTE));
    e
}

struct TopView {
    pool: SqlitePool,
    guild: i64,
    section: Section,
    range: Range,
    page: usize,
    rows: Cached,
}

impl TopView {
    async fn reload(&mut self) -> Result<(), sqlx::Error> {
        self.rows = rows_for(&self.pool, self.guild, self.section, self.range).await?;
        self.page = self.page.min(page_count(self.rows.len(), PER_PAGE) - 1);
        Ok(())
    }

    fn label(&self) -> &'static str {
        SECTIONS
            .iter()
            .find(|(_, _, s)| *s == self.section)
            .map_or("", |(_, l, _)| l)
    }

    fn range_label(&self) -> &'static str {
        RANGES
            .iter()
            .find(|(_, _, r)| *r == self.range)
            .map_or("", |(_, l, _)| l)
    }

    fn visible(&self) -> &[Row] {
        let start = (self.page * PER_PAGE).min(self.rows.len());
        &self.rows[start..(start + PER_PAGE).min(self.rows.len())]
    }
}

impl View for TopView {
    fn render(&self, enabled: bool) -> Page {
        let pages = page_count(self.rows.len(), PER_PAGE);
        let embed = page_embed(
            self.section,
            self.label(),
            self.range_label(),
            &self.rows,
            self.page,
        );
        let section_choices = SECTIONS
            .iter()
            .map(|(k, l, s)| Choice {
                label: (*l).to_string(),
                description: (*s == self.section).then(|| "القسم الحالي".to_string()),
                value: (*k).to_string(),
            })
            .collect();
        let range_choices = RANGES
            .iter()
            .map(|(k, l, _)| Choice {
                label: (*l).to_string(),
                description: None,
                value: (*k).to_string(),
            })
            .collect();
        let mut rows: Vec<_> = select_row(
            SECTION_ID,
            i18n::TOP_SECTION_PLACEHOLDER,
            section_choices,
            enabled,
        )
        .into_iter()
        .collect();
        rows.extend(select_row(
            RANGE_ID,
            i18n::TOP_RANGE_PLACEHOLDER,
            range_choices,
            enabled,
        ));
        // Items open as new messages (works, characters); members and points are not items.
        let items: Vec<(String, String)> = self
            .visible()
            .iter()
            .map(|r| (fit(&r.label, 100), r.key.clone()))
            .collect();
        match self.section {
            Section::Characters => rows.extend(components::character_menu(
                i18n::TOP_OPEN_PLACEHOLDER,
                &items,
            )),
            Section::Members | Section::Events => {}
            _ => rows.extend(components::work_menu(i18n::TOP_OPEN_PLACEHOLDER, &items)),
        }
        rows.push(nav_row(self.page, pages, enabled));
        // Discord allows five rows: the close button shares the page row's message only when there is room.
        if rows.len() < 5 {
            rows.push(buttons(vec![close_button(enabled)]));
        }
        Page::embed(embed.build(), rows)
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        match id {
            CLOSE => Outcome::Close,
            SECTION_ID => match values
                .first()
                .and_then(|v| SECTIONS.iter().find(|(k, _, _)| k == v))
            {
                Some((_, _, s)) => {
                    self.section = *s;
                    self.page = 0;
                    self.refresh().await
                }
                None => Outcome::Keep,
            },
            RANGE_ID => match values
                .first()
                .and_then(|v| RANGES.iter().find(|(k, _, _)| k == v))
            {
                Some((_, _, r)) => {
                    self.range = *r;
                    self.page = 0;
                    self.refresh().await
                }
                None => Outcome::Keep,
            },
            other => {
                let Some(nav) = parse_nav(other) else {
                    return Outcome::Keep;
                };
                let next = apply_nav(nav, self.page, page_count(self.rows.len(), PER_PAGE));
                if next == self.page {
                    return Outcome::Keep;
                }
                self.page = next;
                Outcome::Update
            }
        }
    }
}

impl TopView {
    async fn refresh(&mut self) -> Outcome {
        match self.reload().await {
            Ok(()) => Outcome::Update,
            Err(e) => {
                tracing::warn!("top reload failed: {e}");
                Outcome::Whisper(i18n::ERR_GENERIC.to_string())
            }
        }
    }
}

/// The server's rankings: works, characters, members and event points.
#[poise::command(
    slash_command,
    guild_only,
    name_localized("ar", "الأفضل"),
    description_localized("ar", "ترتيب السيرفر: الأعمال والشخصيات والأعضاء ونقاط الفعاليات")
)]
pub async fn top(ctx: Context<'_>) -> Result<(), Error> {
    let Some(guild) = ctx.guild_id().map(|g| g.get() as i64) else {
        return say_ephemeral(ctx, i18n::SERVER_ONLY).await;
    };
    super::profile::begin_public_for_top(ctx).await?;
    let pool = ctx.data().db.clone();
    let rows = rows_for(&pool, guild, Section::Anime, Range::All).await?;
    session::run(
        ctx,
        TopView {
            pool,
            guild,
            section: Section::Anime,
            range: Range::All,
            page: 0,
            rows,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> SqlitePool {
        let p = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("db");
        sqlx::migrate!("./migrations")
            .run(&p)
            .await
            .expect("migrate");
        p
    }

    async fn member(p: &SqlitePool, guild: i64, user: i64) {
        sqlx::query("INSERT OR IGNORE INTO profiles (user_id) VALUES (?1)")
            .bind(user)
            .execute(p)
            .await
            .expect("profile");
        sqlx::query("INSERT OR IGNORE INTO user_guilds (guild_id, user_id) VALUES (?1, ?2)")
            .bind(guild)
            .bind(user)
            .execute(p)
            .await
            .expect("guild");
    }

    async fn work(
        p: &SqlitePool,
        user: i64,
        key: &str,
        kind: &str,
        title: &str,
        country: Option<&str>,
        added: i64,
    ) {
        sqlx::query("INSERT OR REPLACE INTO media_cache (work_id, title_romaji, kind, country, is_safe, source, fetched_at) VALUES (?1, ?2, ?3, ?4, 1, 'x', 0)")
            .bind(key).bind(title).bind(kind).bind(country).execute(p).await.expect("cache");
        sqlx::query("INSERT INTO profile_works (user_id, work_id, kind, position, added_at) VALUES (?1, ?2, ?3, 1, ?4)")
            .bind(user).bind(key).bind(kind).bind(added).execute(p).await.expect("work");
    }

    #[tokio::test]
    async fn a_server_ranks_its_members_works_by_section_and_never_another_servers() {
        let p = pool().await;
        for u in [1, 2, 3] {
            member(&p, 10, u).await;
        }
        member(&p, 99, 4).await;
        let now = chrono::Utc::now().timestamp();
        work(&p, 1, "anilist:1", "anime", "Popular", Some("JP"), now).await;
        work(&p, 2, "anilist:1", "anime", "Popular", Some("JP"), now).await;
        work(&p, 3, "anilist:2", "anime", "Single", Some("JP"), now).await;
        work(&p, 4, "anilist:9", "anime", "Elsewhere", Some("JP"), now).await;
        let rows = rows_for(&p, 10, Section::Anime, Range::All)
            .await
            .expect("rows");
        let titles: Vec<_> = rows.iter().map(|r| (r.label.as_str(), r.count)).collect();
        assert_eq!(
            titles,
            [("Popular", 2), ("Single", 1)],
            "another server's pick is not counted"
        );
    }

    #[tokio::test]
    async fn dramas_and_series_are_split_by_country_and_the_servers_override() {
        let p = pool().await;
        member(&p, 20, 1).await;
        let now = chrono::Utc::now().timestamp();
        work(
            &p,
            1,
            "tmdb-tv:1",
            "tmdb-tv",
            "Korean show",
            Some("KR"),
            now,
        )
        .await;
        work(
            &p,
            1,
            "tmdb-tv:2",
            "tmdb-tv",
            "American show",
            Some("US"),
            now,
        )
        .await;
        let drama = rows_for(&p, 20, Section::Drama, Range::All)
            .await
            .expect("rows");
        let series = rows_for(&p, 20, Section::Series, Range::All)
            .await
            .expect("rows");
        assert_eq!(
            drama.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(),
            ["Korean show"]
        );
        assert_eq!(
            series.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(),
            ["American show"]
        );
        // The override moves Korea to series (a new server avoids the five-minute cache).
        member(&p, 21, 1).await;
        notify::set_drama_override(&p, 21, "KR", "series")
            .await
            .expect("override");
        let moved = rows_for(&p, 21, Section::Series, Range::All)
            .await
            .expect("rows");
        assert_eq!(moved.len(), 2);
    }

    #[tokio::test]
    async fn the_time_range_counts_only_recent_additions_and_private_profiles_are_left_out() {
        let p = pool().await;
        member(&p, 30, 1).await;
        member(&p, 30, 2).await;
        sqlx::query("UPDATE profiles SET privacy = 'private' WHERE user_id = 2")
            .execute(&p)
            .await
            .expect("privacy");
        let now = chrono::Utc::now().timestamp();
        work(
            &p,
            1,
            "anilist:5",
            "anime",
            "Old",
            Some("JP"),
            now - 40 * 86_400,
        )
        .await;
        work(&p, 1, "anilist:6", "anime", "New", Some("JP"), now - 86_400).await;
        work(&p, 2, "anilist:7", "anime", "Hidden", Some("JP"), now).await;
        let week = rows_for(&p, 30, Section::Anime, Range::Week)
            .await
            .expect("rows");
        assert_eq!(
            week.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(),
            ["New"]
        );
        let all = rows_for(&p, 30, Section::Anime, Range::All)
            .await
            .expect("rows");
        assert_eq!(all.len(), 2, "private profiles never count");
    }

    #[test]
    fn the_first_place_gets_a_hero_card_and_the_rest_a_ranked_list() {
        let rows: Vec<Row> = (1..=12)
            .map(|i| Row {
                key: format!("anilist:{i}"),
                label: format!("Work {i}"),
                count: 20 - i,
                image: (i == 1).then(|| "https://s4.anilist.co/x.jpg".to_string()),
                sub: Some("2020".into()),
            })
            .collect();
        let e = page_embed(Section::Anime, "أنمي", "كل الأوقات", &rows, 0);
        let d = e.description.as_deref().unwrap_or("");
        assert!(
            d.starts_with("👑 **Work 1**")
                && d.contains("🥈 Work 2")
                && d.contains("🥉 Work 3")
                && d.contains("**10.** Work 10")
                && !d.contains("Work 11"),
            "{d}"
        );
        assert_eq!(e.image.as_deref(), Some("https://s4.anilist.co/x.jpg"));
        let second = page_embed(Section::Anime, "أنمي", "كل الأوقات", &rows, 1);
        let d2 = second.description.as_deref().unwrap_or("");
        assert!(
            d2.contains("**11.** Work 11") && !d2.contains("👑") && second.image.is_none(),
            "{d2}"
        );
    }

    #[test]
    fn an_empty_section_says_so_and_game_covers_are_never_hotlinked() {
        let e = page_embed(Section::Games, "ألعاب", "هذا الأسبوع", &[], 0);
        assert_eq!(e.description.as_deref(), Some(i18n::TOP_EMPTY));
        assert!(
            hero_image_ok("https://image.tmdb.org/t/p/w500/a.jpg")
                && hero_image_ok("https://s4.anilist.co/a.png")
        );
        assert!(
            !hero_image_ok("https://images.igdb.com/a.jpg")
                && !hero_image_ok("https://t.vndb.org/a.jpg")
                && !hero_image_ok("http://s4.anilist.co/a")
        );
    }

    #[test]
    fn stored_character_ids_open_as_the_right_key_for_every_source() {
        assert_eq!(character_key(1234), "1234");
        assert_eq!(character_key(1_000_000_000_000 + 55), "vndb:c55");
        assert_eq!(character_key(2_000_000_000_000 + 77), "igdb:c77");
        let id = tm::encode_cast_id(tm::Kind::Movie, 603, 6384).expect("id");
        assert_eq!(character_key(id), "tmdb-cast:movie:603:6384");
    }

    #[test]
    fn every_section_and_range_is_listed_once_and_fits_a_select_menu() {
        assert_eq!(SECTIONS.len(), 10);
        let labels: Vec<_> = SECTIONS.iter().map(|s| s.1).collect();
        assert_eq!(
            labels,
            [
                "أنمي",
                "مانجا",
                "روايات",
                "دراما",
                "مسلسلات",
                "أفلام",
                "ألعاب",
                "شخصيات",
                "الأعضاء",
                "نقاط الفعاليات"
            ]
        );
        assert_eq!(RANGES.len(), 3);
    }

    #[tokio::test]
    async fn people_in_profiles_count_as_characters_in_the_top_and_open_by_their_person_key() {
        let p = pool().await;
        for u in [1, 2] {
            member(&p, 77, u).await;
        }
        let now = chrono::Utc::now().timestamp();
        let id = crate::people::encode_person_id("tmdb-person:17419").expect("id");
        sqlx::query("INSERT INTO character_cache (character_id, name_romaji, main_work, is_safe, fetched_at) VALUES (?1, 'Bryan Cranston', 'شخص حقيقي · ممثل', 1, 0)")
            .bind(id).execute(&p).await.expect("cache");
        for u in [1, 2] {
            sqlx::query("INSERT INTO profile_characters (user_id, character_id, position, added_at) VALUES (?1, ?2, 1, ?3)")
                .bind(u).bind(id).bind(now).execute(&p).await.expect("person");
        }
        let chars = rows_for(&p, 77, Section::Characters, Range::All)
            .await
            .expect("rows");
        assert_eq!(chars.len(), 1);
        assert_eq!(
            (chars[0].label.as_str(), chars[0].count),
            ("Bryan Cranston", 2)
        );
        assert_eq!(
            chars[0].key, "tmdb-person:17419",
            "the menu opens the person card"
        );
    }

    #[tokio::test]
    async fn members_characters_and_event_points_are_ranked_per_server() {
        let p = pool().await;
        for u in [1, 2] {
            member(&p, 40, u).await;
        }
        let now = chrono::Utc::now().timestamp();
        sqlx::query("INSERT INTO character_cache (character_id, name_romaji, is_safe, fetched_at) VALUES (7, 'Hero', 1, 0), (8, 'Villain', 0, 0)").execute(&p).await.expect("cache");
        for (u, c) in [(1, 7), (2, 7), (1, 8)] {
            sqlx::query("INSERT INTO profile_characters (user_id, character_id, position, added_at) VALUES (?1, ?2, 1, ?3)").bind(u).bind(c).bind(now).execute(&p).await.expect("char");
        }
        let chars = rows_for(&p, 40, Section::Characters, Range::All)
            .await
            .expect("rows");
        assert_eq!(
            chars
                .iter()
                .map(|r| (r.label.as_str(), r.count))
                .collect::<Vec<_>>(),
            [("Hero", 2)],
            "unsafe characters are never listed"
        );
        work(&p, 1, "anilist:1", "anime", "A", None, now).await;
        let members = rows_for(&p, 40, Section::Members, Range::All)
            .await
            .expect("rows");
        assert_eq!(
            (members[0].key.as_str(), members[0].count),
            ("1", 3),
            "member 1 added two characters and a work"
        );
        assert!(members[0].label.starts_with("<@"));
        sqlx::query("INSERT INTO weekly_scores (guild_id, user_id, week, points, correct) VALUES (40, 2, ?1, 30, 3), (40, 1, ?1, 10, 1), (41, 1, ?1, 99, 9)")
            .bind(crate::db::game::week_key(now)).execute(&p).await.expect("scores");
        let events = rows_for(&p, 40, Section::Events, Range::Week)
            .await
            .expect("rows");
        assert_eq!(
            events
                .iter()
                .map(|r| (r.key.as_str(), r.count))
                .collect::<Vec<_>>(),
            [("2", 30), ("1", 10)]
        );
    }
}
