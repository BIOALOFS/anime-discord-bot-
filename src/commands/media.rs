//! The interactive work card (anime, manga, manhwa, manhua, novel) and its cast list.
//! Entry points live in the category commands (`commands::category`) and `/profile`.

use super::{
    buttons,
    character::{CharacterView, Mode as CharMode},
    fail, parse_id, policy, say_ephemeral,
};
use crate::{
    anilist::{
        kind_of,
        models::{Character, Media},
        AniError, ContentPolicy, MediaFilter, MediaKind, SafeAniList,
    },
    db::{
        profile as pdb, reminders,
        works::{self, MediaCacheRow},
    },
    error::{Context, Error},
    i18n,
    ui::{
        autocomplete::to_choices,
        cards,
        format::format_ar,
        pagination::{apply_nav, back_button, close_button, nav_row, parse_nav, BACK, CLOSE},
        select::{cast_choices, media_choices, pick_row, select_row, PICK_ID},
        session::{self, Outcome, Page, UndoFuture, View, Whisper},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::sync::Arc;

const BTN_CHARACTERS: &str = "characters";
const BTN_STAR: &str = "star";
const BTN_REROLL: &str = "reroll";
const CAST_PICK: &str = "cast_pick";
const BTN_REMIND: &str = "remind";
const MAX_HISTORY: usize = 8;

/// Where a card came from; decides which extra buttons it gets.
#[derive(Clone)]
pub enum Source {
    Plain,
    Random {
        kind: MediaKind,
        genre: Option<String>,
        min_score: Option<u8>,
        filter: MediaFilter,
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
    /// Does the viewer have a release reminder on the shown work?
    reminded: bool,
    whisper: Option<Whisper>,
    /// Main characters of the shown card (first page of the cast), for the dropdown.
    cast: Vec<(Character, String)>,
    cast_for: Option<i64>,
    /// A character card opened from the dropdown; Back returns here.
    sub: Option<CharacterView>,
    /// Opened from another view (the recommendation list): Back at the first screen returns to that view.
    from_parent: bool,
    /// Hide the synopsis behind a spoiler tag (server or member setting).
    spoiler: bool,
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
            reminded: false,
            whisper: None,
            cast: Vec::new(),
            cast_for: None,
            sub: None,
            from_parent: false,
            spoiler: crate::db::notify::spoiler_guard(
                &ctx.data().db,
                ctx.guild_id().map(|g| g.get() as i64),
                ctx.author().id.get() as i64,
            )
            .await,
        };
        v.refresh_added().await;
        v.ensure_cast().await;
        v
    }

    /// A work card opened from another view (no command context needed); Back returns to the parent.
    pub async fn child(
        api: Arc<SafeAniList>,
        pool: SqlitePool,
        user: i64,
        policy: ContentPolicy,
        media: Media,
    ) -> Self {
        let pool_for_spoiler = pool.clone();
        let mut v = Self {
            api,
            pool,
            user,
            policy,
            source: Source::Plain,
            mode: Mode::Card(Box::new(media)),
            history: Vec::new(),
            notice: None,
            added: false,
            reminded: false,
            whisper: None,
            cast: Vec::new(),
            cast_for: None,
            sub: None,
            from_parent: true,
            spoiler: crate::db::notify::spoiler_guard(&pool_for_spoiler, None, user).await,
        };
        v.refresh_added().await;
        v.ensure_cast().await;
        v
    }

    /// Back at the first screen of a child view means "return to the parent".
    pub fn wants_to_leave(&self, id: &str) -> bool {
        self.from_parent && id == BACK && self.history.is_empty() && self.sub.is_none()
    }

    /// Load the first page of the cast of the shown card (cached by the API layer); failures leave it empty.
    async fn ensure_cast(&mut self) {
        let id = match &self.mode {
            Mode::Card(m) => m.id,
            _ => return,
        };
        if self.cast_for == Some(id) {
            return;
        }
        self.cast = match self.api.media_characters(id, 1, self.policy).await {
            Ok(p) => p.items,
            Err(e) => {
                tracing::debug!("cast dropdown unavailable: {e}");
                Vec::new()
            }
        };
        self.cast_for = Some(id);
    }

    /// The characters a dropdown on the current screen offers.
    fn shown_cast(&self) -> &[(Character, String)] {
        match &self.mode {
            Mode::Characters { items, .. } => items,
            Mode::Card(_) => &self.cast,
            Mode::Pick(_) => &[],
        }
    }

    async fn open_cast_member(&mut self, values: &[String]) -> Outcome {
        let Some(id) = values.first().and_then(|v| v.parse::<i64>().ok()) else {
            return Outcome::Keep;
        };
        let Some(c) = self
            .shown_cast()
            .iter()
            .find(|(c, _)| c.id == id)
            .map(|(c, _)| c.clone())
        else {
            return Outcome::Keep;
        };
        self.sub = Some(
            CharacterView::new(
                self.api.clone(),
                self.policy,
                CharMode::Card(c),
                self.pool.clone(),
                self.user,
            )
            .with_back(),
        );
        Outcome::Update
    }

    async fn refresh_added(&mut self) {
        self.reminded = match &self.mode {
            Mode::Card(m) => reminders::has(&self.pool, self.user, m.id)
                .await
                .unwrap_or(false),
            _ => false,
        };
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
            filter,
        } = self.source.clone()
        else {
            return Outcome::Keep;
        };
        match pick_random(
            &self.api,
            self.user,
            kind,
            genre.as_deref(),
            min_score,
            &filter,
            self.policy,
        )
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

    /// 🔔 / 🔕: ask for a DM when a new episode is released, or cancel it.
    async fn toggle_reminder(&mut self) -> Outcome {
        let Some(m) = self.current_card() else {
            return Outcome::Keep;
        };
        let (pool, user) = (self.pool.clone(), self.user);
        let text = if self.reminded {
            match reminders::remove(&pool, user, m.id).await {
                Ok(_) => {
                    self.reminded = false;
                    i18n::REMINDER_CANCELLED
                }
                Err(e) => {
                    tracing::warn!("reminder removal failed: {e}");
                    i18n::ERR_GENERIC
                }
            }
        } else {
            match reminders::add(&pool, user, m.id, m.title.main()).await {
                Ok(reminders::AddResult::Added | reminders::AddResult::Already) => {
                    self.reminded = true;
                    i18n::REMINDER_SET
                }
                Ok(reminders::AddResult::Full) => i18n::REMINDER_FULL,
                Err(e) => {
                    tracing::warn!("reminder add failed: {e}");
                    i18n::ERR_GENERIC
                }
            }
        };
        self.whisper = Some(Whisper::text(text));
        Outcome::Update
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
                Ok((
                    crate::features::badges::announce(&pool, user, i18n::WORK_ADDED.to_string())
                        .await,
                    true,
                ))
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
        let mut main = vec![serenity::CreateButton::new(BTN_STAR)
            .label(star_label)
            .emoji(if self.added { '✅' } else { '⭐' })
            .style(star_style)
            .disabled(!enabled)];
        // "أظهر الشخصيات" and "أظهر الطاقم": new messages, never an edit of this one.
        main.extend(crate::components::standard_actions(
            crate::components::ItemKind::Work,
            Some(&format!("anilist:{}", m.id)),
        ));
        // Anime episodes and the chapters of tracked manga can remind.
        if kind_of(m) != MediaKind::Novel {
            main.push(
                serenity::CreateButton::new(BTN_REMIND)
                    .label(if self.reminded {
                        i18n::BTN_REMINDER_CANCEL
                    } else {
                        i18n::BTN_REMIND
                    })
                    .emoji(if self.reminded { '🔕' } else { '🔔' })
                    .style(if self.reminded {
                        serenity::ButtonStyle::Success
                    } else {
                        secondary
                    })
                    .disabled(!enabled),
            );
        }
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
        if !self.history.is_empty() || self.from_parent {
            extra.push(back_button(enabled));
        }
        extra.push(close_button(enabled));
        let mut rows: Vec<_> = select_row(
            CAST_PICK,
            i18n::CAST_PLACEHOLDER,
            cast_choices(&self.cast),
            enabled,
        )
        .into_iter()
        .collect();
        rows.push(buttons(main));
        rows.push(buttons(extra));
        // Watch / read status, for every source (answered globally, like the other universal controls).
        rows.extend(crate::components::status_menu(
            &format!("anilist:{}", m.id),
            kind_of(m).is_written(),
            None,
        ));
        rows
    }
}

impl View for MediaView {
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
        let (embed, mut rows) = match &self.mode {
            Mode::Pick(list) => (
                cards::picker_embed(list),
                pick_row(media_choices(list), enabled)
                    .into_iter()
                    .collect::<Vec<_>>(),
            ),
            Mode::Card(m) => (
                cards::spoiler_wrapped(cards::work_card(m), self.spoiler),
                self.card_rows(m, enabled),
            ),
            Mode::Characters {
                media,
                page,
                pages,
                items,
            } => (
                cards::characters_embed(media, items, *page, *pages),
                select_row(
                    CAST_PICK,
                    i18n::CAST_PLACEHOLDER,
                    cast_choices(items),
                    enabled,
                )
                .into_iter()
                .chain([nav_row(*page, *pages, enabled)])
                .collect(),
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
        if let Some(sub) = self.sub.as_mut() {
            if sub.wants_to_leave(id) {
                self.sub = None;
                return Outcome::Update;
            }
            return sub.on_interaction(id, values).await;
        }
        let outcome = self.handle(id, values).await;
        self.ensure_cast().await;
        outcome
    }
}

impl MediaView {
    async fn handle(&mut self, id: &str, values: &[String]) -> Outcome {
        match id {
            CLOSE => Outcome::Close,
            CAST_PICK => self.open_cast_member(values).await,
            BACK => match self.history.pop() {
                Some(previous) => {
                    self.mode = previous;
                    self.refresh_added().await;
                    Outcome::Update
                }
                None => Outcome::Keep,
            },
            BTN_STAR => self.toggle_favourite().await,
            BTN_REMIND => self.toggle_reminder().await,
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

/// Autocomplete label used by `/suggest`.
pub fn media_label_for_suggest(m: &Media) -> String {
    media_label(m)
}

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

/// THE work search: autocomplete and submit both call this, so they share one query shape and one cache entry.
pub async fn search_works(
    api: &SafeAniList,
    text: &str,
    kind: MediaKind,
    policy: ContentPolicy,
) -> Result<Vec<Media>, crate::anilist::AniError> {
    api.search_media(text.trim(), kind, 10, policy).await
}

/// What autocomplete answers for `q`: the search for two letters or more, the popular works of the kind for an
/// empty or one-letter input (narrowed to the titles containing the letter when any do).
pub async fn work_suggestions(
    api: &SafeAniList,
    kind: MediaKind,
    q: &str,
) -> Vec<(String, String)> {
    let q = q.trim();
    let to_items = |list: Vec<Media>| -> Vec<(String, String)> {
        list.iter()
            .map(|m| (media_label(m), format!("id:{}", m.id)))
            .collect()
    };
    if q.chars().count() >= 2 {
        return match search_works(api, q, kind, ContentPolicy::SAFE).await {
            Ok(list) => to_items(list),
            Err(e) => {
                tracing::debug!("autocomplete search failed: {e}");
                Vec::new()
            }
        };
    }
    let popular = match api.popular_media(kind, 10, ContentPolicy::SAFE).await {
        Ok(list) => to_items(list),
        Err(e) => {
            tracing::debug!("autocomplete popular list failed: {e}");
            return Vec::new();
        }
    };
    let needle = q.to_lowercase();
    let matching: Vec<_> = popular
        .iter()
        .filter(|(l, _)| !needle.is_empty() && l.to_lowercase().contains(&needle))
        .cloned()
        .collect();
    if matching.is_empty() {
        popular
    } else {
        matching
    }
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
        .items_scoped(
            ctx.author().id.get(),
            label,
            "",
            partial,
            0,
            |q| async move { work_suggestions(&api, kind, &q).await },
        )
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

/// A random safe work that matches the filter and is not one of the member's last five picks.
pub async fn pick_random(
    api: &SafeAniList,
    user: i64,
    kind: MediaKind,
    genre: Option<&str>,
    min_score: Option<u8>,
    filter: &MediaFilter,
    policy: ContentPolicy,
) -> Result<Option<Media>, crate::anilist::AniError> {
    let scope = format!("media:{}", kind.key());
    let mut fallback = None;
    for _ in 0..4 {
        let Some(m) = api
            .random_media_with(kind, genre, min_score, filter, policy)
            .await?
        else {
            continue;
        };
        if !super::picks::seen(user, &scope, &work_key_of(&m)) {
            super::picks::remember(user, &scope, &work_key_of(&m));
            return Ok(Some(m));
        }
        fallback.get_or_insert(m);
    }
    Ok(fallback)
}

/// Resolve the typed value (autocomplete id or free text) and run the interactive card, keeping only the works that
/// pass `filter` (format / year / status / origin).
pub async fn run_media_with(
    ctx: Context<'_>,
    kind: MediaKind,
    input: String,
    filter: MediaFilter,
) -> Result<(), Error> {
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
        match search_works(&api, &input, kind, policy).await.map(|v| {
            v.into_iter()
                .filter(|m| filter.matches(m))
                .collect::<Vec<_>>()
        }) {
            Ok(mut v) if v.len() == 1 => Mode::Card(Box::new(v.remove(0))),
            Ok(v) if v.is_empty() => {
                // AniList has nothing: try the (opt-in) fallback catalogs for written works.
                let want = match kind {
                    MediaKind::Anime => crate::wiki::Kind::Anime,
                    MediaKind::Novel => crate::wiki::Kind::Novel,
                    _ => crate::wiki::Kind::Manga,
                };
                if kind.is_written() && super::fallback::run(ctx, input.trim()).await? {
                    return Ok(());
                }
                // The owner's wikis of the right medium answer last.
                if super::category::wiki_fallback(ctx, input.trim(), want, false).await? {
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

#[cfg(test)]
mod suggestion_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn counting_api() -> (SafeAniList, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let (searches, populars) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (s, p) = (searches.clone(), populars.clone());
        let api = SafeAniList::with_fixture(move |query, vars| {
            if vars.get("search").is_some() {
                s.fetch_add(1, Ordering::SeqCst);
            } else {
                p.fetch_add(1, Ordering::SeqCst);
                assert!(
                    query.contains("POPULARITY_DESC"),
                    "the popular list sorts by popularity"
                );
            }
            let doc: serde_json::Value = serde_json::from_str(include_str!(
                "../../tests/fixtures/anilist/search_media.json"
            ))
            .expect("fixture");
            Ok(doc["data"].clone())
        });
        (api, searches, populars)
    }

    #[tokio::test]
    async fn autocomplete_and_submit_share_one_search_and_one_cache_entry() {
        let (api, searches, _) = counting_api();
        let suggested = work_suggestions(&api, MediaKind::Anime, "Safe").await;
        assert!(!suggested.is_empty());
        // submit with the same text (different case and spacing) is a cache hit
        let found = search_works(&api, "  safe ", MediaKind::Anime, ContentPolicy::SAFE)
            .await
            .expect("search");
        assert!(!found.is_empty());
        assert_eq!(
            searches.load(Ordering::SeqCst),
            1,
            "one network call for both"
        );
        let ids: Vec<String> = found.iter().map(|m| format!("id:{}", m.id)).collect();
        assert!(
            suggested.iter().all(|(_, v)| ids.contains(v)),
            "the same results"
        );
    }

    #[tokio::test]
    async fn empty_and_one_letter_input_show_the_popular_list() {
        let (api, searches, populars) = counting_api();
        let empty = work_suggestions(&api, MediaKind::Anime, "").await;
        assert!(!empty.is_empty());
        let one = work_suggestions(&api, MediaKind::Anime, "s").await;
        assert!(!one.is_empty());
        assert_eq!(
            searches.load(Ordering::SeqCst),
            0,
            "no search below two letters"
        );
        assert_eq!(
            populars.load(Ordering::SeqCst),
            1,
            "the popular list is cached"
        );
    }

    #[tokio::test]
    async fn a_failing_search_is_an_empty_answer_not_a_panic() {
        let api = SafeAniList::with_fixture(|_, _| Err(crate::anilist::AniError::Busy));
        assert!(work_suggestions(&api, MediaKind::Manga, "naruto")
            .await
            .is_empty());
    }
}
