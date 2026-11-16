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

struct RecView {
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
            ))
            .build();
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
        Page::embed(embed, rows)
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
                let Some(c) = self.selected.and_then(|s| self.items.iter().find(|c| c.media.id == s)).cloned() else {
                    return Outcome::Keep;
                };
                self.sub = Some(
                    MediaView::child(self.api.clone(), self.pool.clone(), self.user, self.policy, c.media).await,
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

/// Suggestions from your favourites; with `user`, suggestions you would both like.
#[poise::command(
    slash_command,
    name_localized("ar", "توصية"),
    description_localized("ar", "توصيات من مفضلاتك، أو مشتركة مع عضو آخر")
)]
pub async fn recommend(
    ctx: Context<'_>,
    #[description = "Find what you would both like"]
    #[description_localized("ar", "عضو آخر للحصول على توصيات مشتركة")]
    #[name_localized("ar", "المستخدم")]
    user: Option<serenity::User>,
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
    let mut seeds = seeds_of(&pool, me).await?;
    if let Some(o) = other {
        for s in seeds_of(&pool, o).await? {
            if !seeds.contains(&s) {
                seeds.push(s);
            }
        }
    }
    if seeds.is_empty() {
        return say_ephemeral(ctx, i18n::REC_NO_SEEDS).await;
    }
    let (cands, taste) = match gather(&api, pol, &seeds).await {
        Ok(v) => v,
        Err(e) => return fail(ctx, e).await,
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
    if items.is_empty() {
        return say_ephemeral(ctx, i18n::REC_NONE).await;
    }
    session::run(
        ctx,
        RecView {
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
}
