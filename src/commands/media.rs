//! The interactive work card (anime, manga, manhwa, manhua, novel) and its cast list.
//! Entry points live in `/search`, `/random` and `/profile`.

use super::{buttons, fail, parse_id, policy, say_ephemeral};
use crate::{
    anilist::{
        kind_of,
        models::{Character, Media},
        AniError, ContentPolicy, MediaKind, SafeAniList,
    },
    db::{
        profile as pdb,
        works::{self, MediaCacheRow},
    },
    error::{Context, Error},
    i18n,
    ui::{
        autocomplete::to_choices,
        cards,
        format::format_ar,
        pagination::{apply_nav, back_button, close_button, nav_row, parse_nav, BACK, CLOSE},
        select::{media_choices, pick_row, PICK_ID},
        session::{self, Outcome, Page, UndoFuture, View, Whisper},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::sync::Arc;

const BTN_CHARACTERS: &str = "characters";
const BTN_STAR: &str = "star";
const BTN_REROLL: &str = "reroll";
const MAX_HISTORY: usize = 8;

/// Where a card came from; decides which extra buttons it gets.
#[derive(Clone)]
pub enum Source {
    Plain,
    Random {
        kind: MediaKind,
        genre: Option<String>,
        min_score: Option<u8>,
    },
}

#[derive(Clone)]
pub enum Mode {
    Pick(Vec<Media>),
    Card(Box<Media>),
    Characters {
        media: Box<Media>,
        page: usize,
        pages: usize,
        items: Vec<(Character, String)>,
    },
}

pub struct MediaView {
    api: Arc<SafeAniList>,
    pool: SqlitePool,
    user: i64,
    policy: ContentPolicy,
    source: Source,
    mode: Mode,
    history: Vec<Mode>,
    notice: Option<String>,
    /// Is the shown card in the viewer's favourites (drives the ⭐ / ✅ toggle)?
    added: bool,
    whisper: Option<Whisper>,
}

pub fn pages_of(last_page: Option<usize>, has_next: bool, page: usize) -> usize {
    last_page
        .unwrap_or(page + 1 + usize::from(has_next))
        .max(page + 1)
}

pub fn work_key_of(m: &Media) -> String {
    works::work_key(m.id)
}

/// Lightweight metadata so the profile renders even when AniList is down. The media already passed
/// the safety filter, so it is cached as safe.
pub fn media_cache_row(m: &Media, now: i64) -> MediaCacheRow {
    MediaCacheRow {
        work_id: work_key_of(m),
        title_romaji: m.title.main().to_string(),
        title_english: m.title.english.clone(),
        cover_url: m.cover().map(str::to_string),
        kind: kind_of(m).key().to_string(),
        format: m.format.clone(),
        country: m.country_of_origin.clone(),
        year: m.year().map(i64::from),
        score: m.average_score.map(i64::from),
        status: m.status.clone(),
        is_safe: true,
        source: "anilist".into(),
        fetched_at: now,
    }
}

impl MediaView {
    pub async fn new(ctx: Context<'_>, source: Source, mode: Mode) -> Self {
        let mut v = Self {
            api: ctx.data().anilist.clone(),
            pool: ctx.data().db.clone(),
            user: ctx.author().id.get() as i64,
            policy: policy(ctx),
            source,
            mode,
            history: Vec::new(),
            notice: None,
            added: false,
            whisper: None,
        };
        v.refresh_added().await;
        v
    }

    async fn refresh_added(&mut self) {
        self.added = match &self.mode {
            Mode::Card(m) => works::has_work(&self.pool, self.user, &work_key_of(m))
                .await
                .unwrap_or(false),
            _ => false,
        };
    }

    fn fail(&mut self, e: AniError) -> Outcome {
        tracing::warn!("anilist error in work view: {e}");
        self.notice = Some(super::ani_message(&e).to_string());
        Outcome::Update
    }

    fn push_history(&mut self) {
        if self.history.len() >= MAX_HISTORY {
            self.history.remove(0);
        }
        self.history.push(self.mode.clone());
    }

    fn current_card(&self) -> Option<Media> {
        match &self.mode {
            Mode::Card(m) => Some((**m).clone()),
            _ => None,
        }
    }

    async fn open_characters(&mut self) -> Outcome {
        let Some(media) = self.current_card() else {
            return Outcome::Keep;
        };
        match self.api.media_characters(media.id, 1, self.policy).await {
            Ok(p) => {
                let pages = pages_of(p.last_page, p.has_next, 0);
                self.push_history();
                self.mode = Mode::Characters {
                    media: Box::new(media),
                    page: 0,
                    pages,
                    items: p.items,
                };
                Outcome::Update
            }
            Err(e) => self.fail(e),
        }
    }

    async fn reroll(&mut self) -> Outcome {
        let Source::Random {
            kind,
            genre,
            min_score,
        } = self.source.clone()
        else {
            return Outcome::Keep;
        };
        match self
            .api
            .random_media(kind, genre.as_deref(), min_score, self.policy)
            .await
        {
            Ok(Some(m)) => {
                self.history.clear();
                self.mode = Mode::Card(Box::new(m));
                self.refresh_added().await;
            }
            Ok(None) => self.notice = Some(i18n::RANDOM_FAILED.to_string()),
            Err(e) => return self.fail(e),
        }
        Outcome::Update
    }

    async fn open_picked(&mut self, values: &[String]) -> Outcome {
        let Some(id) = values.first().and_then(|v| v.parse::<i64>().ok()) else {
            return Outcome::Keep;
        };
        let found = match &self.mode {
            Mode::Pick(list) => list.iter().find(|m| m.id == id).cloned(),
            _ => None,
        };
        let Some(media) = found else {
            return Outcome::Keep;
        };
        self.push_history();
        self.mode = Mode::Card(Box::new(media));
        self.refresh_added().await;
        Outcome::Update
    }

    async fn navigate(&mut self, nav: crate::ui::pagination::Nav) -> Outcome {
        let Mode::Characters {
            media, page, pages, ..
        } = &self.mode
        else {
            return Outcome::Keep;
        };
        let (media, current, pages) = ((**media).clone(), *page, *pages);
        let next = apply_nav(nav, current, pages);
        if next == current {
            return Outcome::Keep;
        }
        match self
            .api
            .media_characters(media.id, (next + 1) as u32, self.policy)
            .await
        {
            Ok(p) => {
                self.mode = Mode::Characters {
                    pages: pages_of(p.last_page, p.has_next, next).max(pages),
                    media: Box::new(media),
                    page: next,
                    items: p.items,
                };
                Outcome::Update
            }
            Err(e) => self.fail(e),
        }
    }

    /// ⭐ / ✅: add or remove the work from the viewer's favourites, with an undo button.
    async fn toggle_favourite(&mut self) -> Outcome {
        let Some(m) = self.current_card() else {
            return Outcome::Keep;
        };
        let key = work_key_of(&m);
        let kind = kind_of(&m).key().to_string();
        let (pool, user) = (self.pool.clone(), self.user);
        let result: Result<(String, bool), Error> = async {
            if self.added {
                works::remove_work(&pool, user, &key).await?;
                Ok((i18n::WORK_REMOVED.to_string(), false))
            } else {
                pdb::create_profile(&pool, user).await?;
                works::upsert_media_cache(&pool, &media_cache_row(&m, super::profile::now()))
                    .await?;
                works::add_work(&pool, user, &key, &kind).await?;
                Ok((i18n::WORK_ADDED.to_string(), true))
            }
        }
        .await;
        match result {
            Ok((text, now_added)) => {
                self.added = now_added;
                let (undo_pool, undo_key, undo_kind) = (pool, key, kind);
                let undo: Box<dyn FnOnce() -> UndoFuture + Send + Sync> = Box::new(move || {
                    Box::pin(async move {
                        let r = if now_added {
                            works::remove_work(&undo_pool, user, &undo_key)
                                .await
                                .map(|_| i18n::UNDONE)
                        } else {
                            works::add_work(&undo_pool, user, &undo_key, &undo_kind)
                                .await
                                .map(|_| i18n::UNDONE)
                        };
                        r.unwrap_or(i18n::ERR_GENERIC).to_string()
                    })
                });
                self.whisper = Some(Whisper {
                    text,
                    undo: Some(undo),
                });
            }
            Err(e) => {
                tracing::warn!("favourite toggle failed: {e:#}");
                self.whisper = Some(Whisper::text(i18n::ERR_GENERIC));
            }
        }
        Outcome::Update
    }

    fn card_rows(&self, m: &Media, enabled: bool) -> Vec<serenity::CreateActionRow> {
        let secondary = serenity::ButtonStyle::Secondary;
        let (star_label, star_style) = if self.added {
            (i18n::BTN_ADDED, serenity::ButtonStyle::Success)
        } else {
            (i18n::BTN_FAVOURITE, secondary)
        };
        let mut main = vec![
            serenity::CreateButton::new(BTN_STAR)
                .label(star_label)
                .emoji(if self.added { '✅' } else { '⭐' })
                .style(star_style)
                .disabled(!enabled),
            serenity::CreateButton::new(BTN_CHARACTERS)
                .label(i18n::BTN_CHARACTERS)
                .emoji('👥')
                .style(secondary)
                .disabled(!enabled),
        ];
        if let Some(url) = &m.site_url {
            main.push(
                serenity::CreateButton::new_link(url).label(format!("🔗 {}", i18n::BTN_ANILIST)),
            );
        }
        let mut extra = Vec::new();
        if matches!(self.source, Source::Random { .. }) {
            extra.push(
                serenity::CreateButton::new(BTN_REROLL)
                    .label(i18n::BTN_REROLL)
                    .emoji('🎲')
                    .style(serenity::ButtonStyle::Primary)
                    .disabled(!enabled),
            );
        }
        if !self.history.is_empty() {
            extra.push(back_button(enabled));
        }
        extra.push(close_button(enabled));
        vec![buttons(main), buttons(extra)]
    }
}

impl View for MediaView {
    fn take_whisper(&mut self) -> Option<Whisper> {
        self.whisper.take()
    }

    fn render(&self, enabled: bool) -> Page {
        let (embed, mut rows) = match &self.mode {
            Mode::Pick(list) => (
                cards::picker_embed(list),
                pick_row(media_choices(list), enabled)
                    .into_iter()
                    .collect::<Vec<_>>(),
            ),
            Mode::Card(m) => (cards::work_card(m), self.card_rows(m, enabled)),
            Mode::Characters {
                media,
                page,
                pages,
                items,
            } => (
                cards::characters_embed(media, items, *page, *pages),
                vec![nav_row(*page, *pages, enabled)],
            ),
        };
        if !matches!(self.mode, Mode::Card(_)) {
            let mut row = Vec::new();
            if !self.history.is_empty() {
                row.push(back_button(enabled));
            }
            row.push(close_button(enabled));
            rows.push(buttons(row));
        }
        Page::embed(embed.build(), rows).notice(self.notice.clone())
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        self.notice = None;
        match id {
            CLOSE => Outcome::Close,
            BACK => match self.history.pop() {
                Some(previous) => {
                    self.mode = previous;
                    self.refresh_added().await;
                    Outcome::Update
                }
                None => Outcome::Keep,
            },
            BTN_STAR => self.toggle_favourite().await,
            BTN_CHARACTERS => self.open_characters().await,
            BTN_REROLL => self.reroll().await,
            PICK_ID => self.open_picked(values).await,
            other => match parse_nav(other) {
                Some(nav) => self.navigate(nav).await,
                None => Outcome::Keep,
            },
        }
    }
}

// ---- entry points ------------------------------------------------------------------------

fn media_label(m: &Media) -> String {
    let mut bits = Vec::new();
    if let Some(y) = m.year() {
        bits.push(y.to_string());
    }
    bits.push(cards::kind_label(kind_of(m)).to_string());
    if let Some(f) = m
        .format
        .as_deref()
        .filter(|_| kind_of(m) == MediaKind::Anime)
    {
        bits.push(format_ar(f).to_string());
    }
    format!("{} ({})", m.title.main(), bits.join(" · "))
}

/// Autocomplete over AniList works of `kind` (values are `id:123`).
pub async fn media_autocomplete(
    ctx: Context<'_>,
    kind: MediaKind,
    label: &'static str,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    let data = ctx.data();
    let api = data.anilist.clone();
    let items = data
        .auto
        .items(ctx.author().id.get(), label, partial, |q| async move {
            match api.search_media(&q, kind, 10, ContentPolicy::SAFE).await {
                Ok(list) => list
                    .iter()
                    .map(|m| (media_label(m), format!("id:{}", m.id)))
                    .collect(),
                Err(_) => Vec::new(),
            }
        })
        .await;
    to_choices(items)
}

/// Autocomplete across every work kind at once (profile favourites); values are `id:123`.
pub async fn any_autocomplete(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    let data = ctx.data();
    let api = data.anilist.clone();
    let items = data
        .auto
        .items(ctx.author().id.get(), "work", partial, |q| async move {
            match api.search_any(&q, 10, ContentPolicy::SAFE).await {
                Ok(list) => list
                    .iter()
                    .map(|m| (media_label(m), format!("id:{}", m.id)))
                    .collect(),
                Err(_) => Vec::new(),
            }
        })
        .await;
    to_choices(items)
}

/// Resolve the typed value (autocomplete id or free text) and run the interactive card.
pub async fn run_media(ctx: Context<'_>, kind: MediaKind, input: String) -> Result<(), Error> {
    ctx.defer().await?;
    let api = ctx.data().anilist.clone();
    let policy = policy(ctx);
    let mode = if let Some(id) = parse_id(&input) {
        match api.media_by_id(id, policy).await {
            Ok(Some(m)) => Mode::Card(Box::new(m)),
            Ok(None) => return say_ephemeral(ctx, i18n::NOT_FOUND).await,
            Err(e) => return fail(ctx, e).await,
        }
    } else {
        match api.search_media(input.trim(), kind, 10, policy).await {
            Ok(mut v) if v.len() == 1 => Mode::Card(Box::new(v.remove(0))),
            Ok(v) if v.is_empty() => {
                // AniList has nothing: try the (opt-in) fallback catalogs for written works.
                if kind.is_written() && super::fallback::run(ctx, input.trim()).await? {
                    return Ok(());
                }
                return say_ephemeral(ctx, i18n::NO_RESULTS).await;
            }
            Ok(v) => Mode::Pick(v),
            Err(e) => return fail(ctx, e).await,
        }
    };
    session::run(ctx, MediaView::new(ctx, Source::Plain, mode).await).await
}
