//! `/recommend [user]`: suggestions from favourite works and characters' source works, via AniList
//! recommendations. With `user` it finds what both of you would like.

use super::{buttons, fail, media::MediaView, policy, say_ephemeral};
use crate::{
    anilist::{kind_of, models::Media, AniError, ContentPolicy, SafeAniList},
    commands::media::media_cache_row,
    db::{game, profile as pdb, works},
    error::{Context, Error},
    i18n,
    ui::{
        cards::kind_label,
        embeds::Emb,
        format::score_text,
        pagination::{apply_nav, close_button, nav_row, page_count, page_slice, parse_nav, CLOSE},
        select::{select_row, Choice},
        session::{self, Outcome, Page, UndoFuture, View, Whisper},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::{collections::HashMap, sync::Arc};

const PER_PAGE: usize = 5;
const MAX_SEEDS: usize = 4;
const PICK_ID: &str = "rec_pick";
const ADD_ID: &str = "rec_add";
const HIDE_ID: &str = "rec_hide";

/// `https://anilist.co/anime/123` -> 123.
pub fn id_from_url(url: &str) -> Option<i64> {
    let rest = url.split("anilist.co/").nth(1)?;
    let mut parts = rest.split('/');
    matches!(parts.next()?, "anime" | "manga").then_some(())?;
    parts.next()?.split(['?', '#']).next()?.parse().ok()
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub media: Media,
    /// Titles of the favourites that led here (the "why").
    pub because: Vec<String>,
}

/// Higher is better: how many of your favourites point at it, genre overlap, quality, popularity.
pub fn score(c: &Candidate, taste_genres: &HashMap<String, usize>) -> f64 {
    let seeds = c.because.len() as f64;
    let genre: f64 = c
        .media
        .genres
        .iter()
        .map(|g| taste_genres.get(g).copied().unwrap_or(0) as f64)
        .sum();
    let quality = f64::from(c.media.average_score.unwrap_or(60)) / 20.0;
    let popularity = ((c.media.popularity.unwrap_or(0) as f64) + 1.0).ln() / 4.0;
    seeds * 5.0 + genre * 0.5 + quality + popularity
}

/// Best first; anything already known, hidden or seed itself is dropped.
pub fn rank(
    candidates: Vec<Candidate>,
    taste: &HashMap<String, usize>,
    exclude: &[i64],
) -> Vec<Candidate> {
    let mut scored: Vec<(f64, Candidate)> = candidates
        .into_iter()
        .filter(|c| !exclude.contains(&c.media.id))
        .map(|c| (score(&c, taste), c))
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.media.id.cmp(&b.1.media.id)));
    scored.into_iter().map(|(_, c)| c).collect()
}

/// A user's seeds: favourite works first, then the source works of favourite characters.
async fn seeds_of(pool: &SqlitePool, user: i64) -> Result<Vec<i64>, Error> {
    let mut out: Vec<i64> = Vec::new();
    for w in works::list_page(pool, user, None, None, 0, 3).await? {
        if let Some(id) = w.anilist_id().filter(|_| w.visible()) {
            out.push(id);
        }
    }
    for c in pdb::list_page(pool, user, None, 0, 5).await? {
        if let Some(id) = c
            .main_work_url
            .as_deref()
            .and_then(id_from_url)
            .filter(|_| c.visible())
        {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out.truncate(MAX_SEEDS / 2 + 1);
    Ok(out)
}

/// Where the recommendations come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum RecSource {
    #[name = "profile"]
    #[name_localized("ar", "من بروفايلي")]
    Profile,
    #[name = "general"]
    #[name_localized("ar", "عامة")]
    General,
}

/// Without a choice: from the profile when it has at least three items, otherwise general.
pub fn default_source(profile_items: i64) -> RecSource {
    if profile_items >= 3 {
        RecSource::Profile
    } else {
        RecSource::General
    }
}

/// A show or film recommended through TMDB.
#[derive(Debug, Clone, PartialEq)]
pub struct TmdbCandidate {
    pub key: String,
    pub title: String,
    pub year: Option<i32>,
    pub rating: Option<f32>,
    /// Titles of the profile items that led here.
    pub because: Vec<String>,
}

/// Merge the recommendations of several seeds: one entry per work, remembering every seed that pointed at it; works
/// already in the profile (or the seeds themselves) are dropped. Most pointed-at first, then best rated.
pub fn merge_tmdb(
    per_seed: Vec<(String, Vec<crate::tmdb::Work>)>,
    exclude: &std::collections::HashSet<String>,
) -> Vec<TmdbCandidate> {
    let mut by_key: HashMap<String, TmdbCandidate> = HashMap::new();
    for (seed_title, list) in per_seed {
        for w in list {
            let key = w.key();
            if exclude.contains(&key) {
                continue;
            }
            by_key
                .entry(key.clone())
                .and_modify(|c| c.because.push(seed_title.clone()))
                .or_insert(TmdbCandidate {
                    key,
                    title: w.title.clone(),
                    year: w.year,
                    rating: w.rating,
                    because: vec![seed_title.clone()],
                });
        }
    }
    let mut out: Vec<TmdbCandidate> = by_key.into_values().collect();
    out.sort_by(|a, b| {
        b.because
            .len()
            .cmp(&a.because.len())
            .then(b.rating.unwrap_or(0.0).total_cmp(&a.rating.unwrap_or(0.0)))
            .then(a.key.cmp(&b.key))
    });
    out
}

/// TMDB works in a member's profile: `(key, title)` of the first few (the seeds), and every key (to exclude).
async fn tmdb_profile(
    pool: &SqlitePool,
    user: i64,
) -> Result<(Vec<(String, String)>, std::collections::HashSet<String>), sqlx::Error> {
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT pw.work_id, mc.title_romaji FROM profile_works pw LEFT JOIN media_cache mc ON mc.work_id = pw.work_id WHERE pw.user_id = ?1 AND pw.work_id LIKE 'tmdb-%' ORDER BY pw.position",
    )
    .bind(user)
    .fetch_all(pool)
    .await?;
    let all = rows.iter().map(|(k, _)| k.clone()).collect();
    let seeds = rows
        .into_iter()
        .take(4)
        .map(|(k, t)| (k.clone(), t.unwrap_or(k)))
        .collect();
    Ok((seeds, all))
}

struct RecView {
    /// Shows and films from TMDB (opened from a menu as new messages).
    tmdb: Vec<TmdbCandidate>,
    /// General (not profile based) recommendations.
    general: bool,
    api: Arc<SafeAniList>,
    policy: ContentPolicy,
    /// The work card opened from the dropdown; Back returns to the list.
    sub: Option<MediaView>,
    pool: SqlitePool,
    user: i64,
    items: Vec<Candidate>,
    together: bool,
    page: usize,
    selected: Option<i64>,
    whisper: Option<Whisper>,
}

impl RecView {
    fn why(&self, c: &Candidate) -> String {
        if self.general {
            return i18n::REC_GENERAL_WHY.to_string();
        }
        let first = c.because.first().map_or("", String::as_str);
        if self.together {
            i18n::rec_why_together(first)
        } else {
            i18n::rec_why(first)
        }
    }
}

impl View for RecView {
    fn take_whisper(&mut self) -> Option<Whisper> {
        match self.sub.as_mut() {
            Some(sub) => sub.take_whisper(),
            None => self.whisper.take(),
        }
    }

    fn render(&self, enabled: bool) -> Page {
        if let Some(sub) = &self.sub {
            return sub.render(enabled);
        }
        let pages = page_count(self.items.len(), PER_PAGE);
        let slice = page_slice(&self.items, self.page, PER_PAGE);
        let body = slice
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let m = &c.media;
                let link = m.site_url.as_ref().map_or(m.title.main().to_string(), |u| {
                    format!("[{}]({u})", m.title.main())
                });
                let genres = m
                    .genres
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" • ");
                format!(
                    "**{}.** {link} · {}\n{} · {genres}\n*{}*",
                    self.page * PER_PAGE + i + 1,
                    kind_label(kind_of(m)),
                    score_text(m.average_score),
                    self.why(c)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let embed = Emb::new()
            .title(if self.together {
                i18n::REC_TITLE_TOGETHER
            } else {
                i18n::REC_TITLE
            })
            .description(body)
            .footer(format!(
                "{} · {}/{}",
                i18n::POWERED_BY,
                self.page + 1,
                pages
            ));
        let choices: Vec<Choice> = slice
            .iter()
            .map(|c| Choice {
                label: crate::ui::embeds::fit(c.media.title.main(), 100),
                description: None,
                value: c.media.id.to_string(),
            })
            .collect();
        let mut rows: Vec<_> = select_row(PICK_ID, i18n::REC_PICK, choices, enabled)
            .into_iter()
            .collect();
        let sel = self.selected.is_none();
        rows.push(buttons(vec![
            serenity::CreateButton::new(ADD_ID)
                .label(i18n::BTN_FAVOURITE)
                .emoji('⭐')
                .style(serenity::ButtonStyle::Success)
                .disabled(!enabled || sel),
            serenity::CreateButton::new(HIDE_ID)
                .label(i18n::BTN_NOT_FOR_ME)
                .emoji('🔕')
                .style(serenity::ButtonStyle::Secondary)
                .disabled(!enabled || sel),
            close_button(enabled),
        ]));
        rows.push(nav_row(self.page, pages, enabled));
        // Shows and films come from TMDB: listed in the embed and opened from their own menu (new messages).
        let mut embed = embed;
        if !self.tmdb.is_empty() {
            let lines: Vec<String> = self
                .tmdb
                .iter()
                .take(5)
                .map(|t| {
                    format!(
                        "• **{}**{} · {}",
                        crate::ui::embeds::fit(&t.title, 60),
                        t.year.map(|y| format!(" ({y})")).unwrap_or_default(),
                        if self.general {
                            i18n::REC_GENERAL_WHY.to_string()
                        } else {
                            i18n::rec_why(t.because.first().map_or("", String::as_str))
                        }
                    )
                })
                .collect();
            embed
                .fields
                .push((i18n::REC_TMDB_FIELD.to_string(), lines.join("\n"), false));
            let items: Vec<(String, String)> = self
                .tmdb
                .iter()
                .take(25)
                .map(|t| (crate::ui::embeds::fit(&t.title, 100), t.key.clone()))
                .collect();
            if rows.len() < 5 {
                rows.extend(crate::components::work_menu(
                    i18n::REC_TMDB_PLACEHOLDER,
                    &items,
                ));
            }
        }
        Page::embed(embed.build(), rows)
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        if let Some(sub) = self.sub.as_mut() {
            if sub.wants_to_leave(id) {
                self.sub = None;
                return Outcome::Update;
            }
            return sub.on_interaction(id, values).await;
        }
        match id {
            CLOSE => Outcome::Close,
            PICK_ID => {
                // Choosing a work opens its card (cover, status, count, score, add and remind buttons).
                self.selected = values.first().and_then(|v| v.parse().ok());
                let Some(c) = self
                    .selected
                    .and_then(|s| self.items.iter().find(|c| c.media.id == s))
                    .cloned()
                else {
                    return Outcome::Keep;
                };
                self.sub = Some(
                    MediaView::child(
                        self.api.clone(),
                        self.pool.clone(),
                        self.user,
                        self.policy,
                        c.media,
                    )
                    .await,
                );
                Outcome::Update
            }
            ADD_ID => {
                let Some(c) = self
                    .selected
                    .and_then(|s| self.items.iter().find(|c| c.media.id == s))
                    .cloned()
                else {
                    return Outcome::Keep;
                };
                let (pool, user) = (self.pool.clone(), self.user);
                let key = works::work_key(c.media.id);
                let kind = kind_of(&c.media).key().to_string();
                let result = async {
                    pdb::create_profile(&pool, user).await?;
                    works::upsert_media_cache(
                        &pool,
                        &media_cache_row(&c.media, super::profile::now()),
                    )
                    .await?;
                    works::add_work(&pool, user, &key, &kind).await
                }
                .await;
                self.whisper = Some(match result {
                    Ok(_) => {
                        let (undo_pool, undo_key) = (self.pool.clone(), key);
                        let undo: Box<dyn FnOnce() -> UndoFuture + Send + Sync> =
                            Box::new(move || {
                                Box::pin(async move {
                                    works::remove_work(&undo_pool, user, &undo_key)
                                        .await
                                        .map_or(i18n::ERR_GENERIC, |_| i18n::UNDONE)
                                        .to_string()
                                })
                            });
                        Whisper {
                            text: i18n::WORK_ADDED.to_string(),
                            undo: Some(undo),
                        }
                    }
                    Err(e) => {
                        tracing::warn!("recommendation add failed: {e}");
                        Whisper::text(i18n::ERR_GENERIC)
                    }
                });
                Outcome::Keep
            }
            HIDE_ID => {
                let Some(sel) = self.selected else {
                    return Outcome::Keep;
                };
                if let Err(e) = game::hide_recommendation(&self.pool, self.user, sel).await {
                    tracing::warn!("hide recommendation failed: {e}");
                    return Outcome::Keep;
                }
                self.items.retain(|c| c.media.id != sel);
                self.selected = None;
                self.page = self.page.min(page_count(self.items.len(), PER_PAGE) - 1);
                self.whisper = Some(Whisper::text(i18n::REC_HIDDEN));
                Outcome::Update
            }
            other => {
                let Some(nav) = parse_nav(other) else {
                    return Outcome::Keep;
                };
                let next = apply_nav(nav, self.page, page_count(self.items.len(), PER_PAGE));
                if next == self.page {
                    return Outcome::Keep;
                }
                self.page = next;
                Outcome::Update
            }
        }
    }
}

/// Collect candidates for these seeds: each seed's AniList recommendations.
async fn gather(
    api: &SafeAniList,
    policy: ContentPolicy,
    seeds: &[i64],
) -> Result<(Vec<Candidate>, HashMap<String, usize>), AniError> {
    let mut by_id: HashMap<i64, Candidate> = HashMap::new();
    let mut taste: HashMap<String, usize> = HashMap::new();
    for seed in seeds {
        let Some(seed_media) = api.media_by_id(*seed, policy).await? else {
            continue;
        };
        for g in &seed_media.genres {
            *taste.entry(g.clone()).or_default() += 1;
        }
        let title = seed_media.title.main().to_string();
        for m in api.recommendations(*seed, 1, policy).await?.items {
            by_id
                .entry(m.id)
                .and_modify(|c| c.because.push(title.clone()))
                .or_insert(Candidate {
                    media: m,
                    because: vec![title.clone()],
                });
        }
    }
    Ok((by_id.into_values().collect(), taste))
}

/// Suggestions from your profile (or general ones); with `user`, suggestions you would both like.
#[poise::command(
    slash_command,
    name_localized("ar", "توصية"),
    description_localized("ar", "توصيات من بروفايلك أو عامة، أو مشتركة مع عضو آخر")
)]
pub async fn recommend(
    ctx: Context<'_>,
    #[description = "Find what you would both like"]
    #[description_localized("ar", "عضو آخر للحصول على توصيات مشتركة")]
    #[name_localized("ar", "المستخدم")]
    user: Option<serenity::User>,
    #[description = "Where recommendations come from: your profile or general"]
    #[description_localized("ar", "مصدر التوصيات: من بروفايلك أو عامة")]
    #[name_localized("ar", "المصدر")]
    source: Option<RecSource>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let (pool, api, pol) = (
        ctx.data().db.clone(),
        ctx.data().anilist.clone(),
        policy(ctx),
    );
    let me = ctx.author().id.get() as i64;
    let other = user
        .as_ref()
        .map(|u| u.id.get() as i64)
        .filter(|o| *o != me);
    let items_in_profile = pdb::count(&pool, me).await? + works::count(&pool, me).await?;
    let source = source.unwrap_or_else(|| default_source(items_in_profile));
    let general = source == RecSource::General && other.is_none();
    let mut seeds = if general {
        Vec::new()
    } else {
        seeds_of(&pool, me).await?
    };
    if let Some(o) = other {
        for s in seeds_of(&pool, o).await? {
            if !seeds.contains(&s) {
                seeds.push(s);
            }
        }
    }
    // Shows and films: TMDB recommendations of the profile's own TMDB items (yours, and theirs together).
    let (mut tmdb_seeds, mut tmdb_known) = (Vec::new(), std::collections::HashSet::new());
    if !general {
        for uid in std::iter::once(me).chain(other) {
            let (s, all) = tmdb_profile(&pool, uid).await?;
            tmdb_seeds.extend(s);
            tmdb_known.extend(all);
        }
    }
    if seeds.is_empty() && tmdb_seeds.is_empty() && !general {
        return say_ephemeral(ctx, i18n::REC_NO_SEEDS).await;
    }
    let tmdb = ctx.data().tmdb.clone();
    let mut tmdb_items = Vec::new();
    if tmdb.enabled() {
        if general {
            let mut list = Vec::new();
            for kind in [crate::tmdb::Kind::Tv, crate::tmdb::Kind::Movie] {
                list.extend(tmdb.discover(kind, &[], rand::random_range(1..=5)).await);
            }
            tmdb_items = merge_tmdb(vec![(String::new(), list)], &tmdb_known);
        } else {
            let mut per_seed = Vec::new();
            for (key, title) in &tmdb_seeds {
                if let Some((kind, id)) = crate::tmdb::model::parse_work_key(key) {
                    per_seed.push((title.clone(), tmdb.recommendations(kind, id).await));
                }
            }
            let mut exclude = tmdb_known.clone();
            exclude.extend(tmdb_seeds.iter().map(|(k, _)| k.clone()));
            tmdb_items = merge_tmdb(per_seed, &exclude);
        }
    }
    let (cands, taste) = if general {
        match general_candidates(&api, pol).await {
            Ok(v) => v,
            Err(e) => return fail(ctx, e).await,
        }
    } else {
        match gather(&api, pol, &seeds).await {
            Ok(v) => v,
            Err(e) => return fail(ctx, e).await,
        }
    };
    // Already yours (and, together, theirs), hidden, or one of the seeds.
    let mut exclude: Vec<i64> = seeds.clone();
    exclude.extend(game::hidden_recommendations(&pool, me).await?);
    for uid in std::iter::once(me).chain(other) {
        for w in works::list_page(&pool, uid, None, None, 0, 500).await? {
            exclude.extend(w.anilist_id());
        }
    }
    let items = rank(cands, &taste, &exclude);
    if items.is_empty() && tmdb_items.is_empty() {
        return say_ephemeral(ctx, i18n::REC_NONE).await;
    }
    session::run(
        ctx,
        RecView {
            tmdb: tmdb_items,
            general,
            api: ctx.data().anilist.clone(),
            policy: policy(ctx),
            sub: None,
            pool,
            user: me,
            items,
            together: other.is_some(),
            page: 0,
            selected: None,
            whisper: None,
        },
    )
    .await
}

/// General recommendations: a handful of well-liked, safe anime and manga (nothing about your profile).
async fn general_candidates(
    api: &SafeAniList,
    policy: ContentPolicy,
) -> Result<(Vec<Candidate>, HashMap<String, usize>), AniError> {
    let mut by_id: HashMap<i64, Candidate> = HashMap::new();
    for kind in [
        crate::anilist::MediaKind::Anime,
        crate::anilist::MediaKind::Anime,
        crate::anilist::MediaKind::Manga,
        crate::anilist::MediaKind::Manhwa,
    ] {
        if let Some(m) = api.random_media(kind, None, Some(70), policy).await? {
            by_id.entry(m.id).or_insert(Candidate {
                media: m,
                because: vec![String::new()],
            });
        }
    }
    Ok((by_id.into_values().collect(), HashMap::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn media(id: i64, genres: &[&str], score: i32, pop: i64) -> Media {
        serde_json::from_value(json!({"id": id, "title": {"romaji": format!("M{id}")}, "genres": genres, "averageScore": score, "popularity": pop})).expect("media")
    }

    fn cand(id: i64, genres: &[&str], score: i32, pop: i64, because: &[&str]) -> Candidate {
        Candidate {
            media: media(id, genres, score, pop),
            because: because.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn urls_give_seed_ids() {
        assert_eq!(id_from_url("https://anilist.co/anime/123"), Some(123));
        assert_eq!(
            id_from_url("https://anilist.co/manga/77/Some-Title"),
            Some(77)
        );
        assert_eq!(id_from_url("https://anilist.co/character/5"), None);
        assert_eq!(id_from_url("https://example.com/anime/1"), None);
        assert_eq!(id_from_url(""), None);
    }

    #[test]
    fn more_seeds_beat_quality_and_genres_break_ties() {
        let taste: HashMap<String, usize> = [("Action".to_string(), 3)].into();
        let two_seeds = cand(1, &[], 60, 100, &["A", "B"]);
        let one_seed_great = cand(2, &["Action"], 95, 100_000, &["A"]);
        assert!(
            score(&two_seeds, &taste) > score(&one_seed_great, &taste),
            "agreement between favourites dominates"
        );
        let a = cand(3, &["Action"], 70, 1000, &["A"]);
        let b = cand(4, &["Romance"], 70, 1000, &["A"]);
        assert!(
            score(&a, &taste) > score(&b, &taste),
            "matching genres win ties"
        );
    }

    #[test]
    fn ranking_excludes_known_and_hidden_and_is_deterministic() {
        let taste = HashMap::new();
        let list = vec![
            cand(1, &[], 80, 10, &["A"]),
            cand(2, &[], 80, 10, &["A"]),
            cand(3, &[], 90, 10, &["A"]),
        ];
        let ranked = rank(list.clone(), &taste, &[3]);
        assert_eq!(
            ranked.iter().map(|c| c.media.id).collect::<Vec<_>>(),
            vec![1, 2],
            "excluded id is gone, ties broken by id"
        );
        assert_eq!(rank(list, &taste, &[]).first().map(|c| c.media.id), Some(3));
        assert!(rank(Vec::new(), &taste, &[]).is_empty());
    }

    #[test]
    fn missing_scores_do_not_panic() {
        let c = Candidate {
            media: serde_json::from_value(json!({"id": 9})).expect("m"),
            because: vec![],
        };
        assert!(score(&c, &HashMap::new()).is_finite());
    }

    fn show(id: i64, title: &str, rating: f32) -> crate::tmdb::Work {
        crate::tmdb::Work {
            id,
            kind: Some(crate::tmdb::Kind::Tv),
            title: title.into(),
            rating: Some(rating),
            ..crate::tmdb::Work::default()
        }
    }

    #[test]
    fn the_default_source_is_the_profile_from_three_items_and_general_before() {
        assert_eq!(default_source(0), RecSource::General);
        assert_eq!(default_source(2), RecSource::General);
        assert_eq!(default_source(3), RecSource::Profile);
        assert_eq!(default_source(200), RecSource::Profile);
    }

    #[test]
    fn tmdb_recommendations_are_merged_deduplicated_and_exclude_what_the_profile_has() {
        let per_seed = vec![
            (
                "Seed A".to_string(),
                vec![
                    show(1, "Shared", 7.0),
                    show(2, "Only A", 9.0),
                    show(3, "Owned", 8.0),
                ],
            ),
            (
                "Seed B".to_string(),
                vec![show(1, "Shared", 7.0), show(4, "Only B", 6.0)],
            ),
        ];
        let exclude: std::collections::HashSet<String> = ["tmdb-tv:3".to_string()].into();
        let out = merge_tmdb(per_seed, &exclude);
        let keys: Vec<&str> = out.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(
            keys,
            ["tmdb-tv:1", "tmdb-tv:2", "tmdb-tv:4"],
            "the one two seeds point at first, then by rating; the owned one is gone"
        );
        assert_eq!(
            out[0].because,
            vec!["Seed A".to_string(), "Seed B".to_string()]
        );
        assert!(merge_tmdb(Vec::new(), &exclude).is_empty());
    }

    #[tokio::test]
    async fn the_tmdb_seeds_of_a_profile_come_with_their_titles_and_every_key_is_excluded() {
        let pool = crate::db::connect_memory().await;
        sqlx::query("INSERT INTO profiles (user_id) VALUES (1)")
            .execute(&pool)
            .await
            .expect("p");
        sqlx::query("INSERT INTO media_cache (work_id, title_romaji, kind, is_safe, source, fetched_at) VALUES ('tmdb-tv:5', 'Crash Landing', 'tmdb-tv', 1, 'tmdb', 0)").execute(&pool).await.expect("c");
        for (i, k) in ["tmdb-tv:5", "tmdb-movie:6", "anilist:7"]
            .iter()
            .enumerate()
        {
            sqlx::query("INSERT INTO profile_works (user_id, work_id, kind, position) VALUES (1, ?1, 'x', ?2)").bind(k).bind(i as i64).execute(&pool).await.expect("w");
        }
        let (seeds, all) = tmdb_profile(&pool, 1).await.expect("seeds");
        assert_eq!(
            seeds[0],
            ("tmdb-tv:5".to_string(), "Crash Landing".to_string())
        );
        assert_eq!(
            seeds[1].1, "tmdb-movie:6",
            "without a cached title the key stands in"
        );
        assert_eq!(all.len(), 2, "AniList works are not TMDB seeds");
    }
}
