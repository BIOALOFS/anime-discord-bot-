//! `/character search` and the interactive character card (also used by `/random character`).

use super::{
    buttons, fail, parse_id, policy,
    roster::{Roster, Step},
    say_ephemeral,
};
use crate::{
    anilist::{
        kind_of,
        models::{Character, Media, MediaRef},
        safety, AniError, CharacterLookup, ContentPolicy, SafeAniList,
    },
    db::{profile as db, works},
    error::{Context, Error},
    i18n,
    providers::{CharacterSource, Providers, Work},
    ui::{
        autocomplete::to_choices,
        cards,
        embeds::Emb,
        pagination::{
            apply_nav, back_button, close_button, nav_row, page_count, page_slice, parse_nav, BACK,
            CLOSE,
        },
        select::{character_choices, PICK_ID},
        select::{select_row, Choice},
        session::{self, ModalSpec, Outcome, Page, UndoFuture, View, Whisper},
        text::clean_description_max,
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::sync::Arc;

const BTN_WORKS: &str = "works";
const BTN_RANDOM: &str = "random_character";
const BTN_ROSTER: &str = "open_roster";
const BTN_DESCRIBE: &str = "describe";
const WORK_PICK: &str = "work_pick";
const WORK_ADD: &str = "work_add";
const WORK_ROSTER: &str = "work_roster";
const BTN_ADD: &str = "add_to_profile";
const DESCRIPTION_CHARS: usize = 200;
const WORKS_PER_PAGE: usize = 10;

#[derive(Clone)]
pub enum Mode {
    Pick(Vec<Character>),
    Card(Character),
    Works {
        character: Character,
        page: usize,
    },
    /// A work picked from the character's works list.
    Work(Box<Media>),
    /// A game or visual-novel character from IGDB / VNDB.
    Provider(Box<Work>),
    /// History marker: going back here re-opens the roster the user came from.
    ResumeRoster,
}

pub struct CharacterView {
    api: Arc<SafeAniList>,
    policy: ContentPolicy,
    mode: Mode,
    history: Vec<Mode>,
    notice: Option<String>,
    pool: SqlitePool,
    user: i64,
    roster: Option<Roster>,
    whisper: Option<Whisper>,
    providers: Option<Arc<Providers>>,
    /// The owner's wikis: a provider character that lacks a picture or text is filled in place when the lookup arrives.
    wikis: Option<Arc<crate::wiki::service::Wikis>>,
    live: crate::wiki::fill::LiveFill,
    /// Game / visual-novel matches shown next to the AniList ones in the pick menu.
    extra: Vec<Work>,
    cover: Option<Arc<Vec<u8>>>,
    /// Opened from another view (work card, /top): always offer Back, which returns to that view.
    from_parent: bool,
    /// Is the shown character already in the viewer's profile? (drives add / remove)
    in_profile: bool,
    /// Rosters left behind while a character picked from them is open.
    suspended: Vec<Roster>,
}

/// Buttons under every AniList character image: add to / remove from my profile, works, source link and back on the
/// first row; description, random, roster and close on the second.
pub fn card_rows(
    link: Option<&str>,
    in_profile: bool,
    show_back: bool,
    work_key: Option<&str>,
    performer: Option<&str>,
    enabled: bool,
) -> Vec<serenity::CreateActionRow> {
    let secondary = serenity::ButtonStyle::Secondary;
    let mut main = vec![
        serenity::CreateButton::new(BTN_ADD)
            .label(if in_profile {
                i18n::BTN_REMOVE_FROM_PROFILE
            } else {
                i18n::BTN_ADD_TO_PROFILE
            })
            .emoji(if in_profile { '➖' } else { '➕' })
            .style(if in_profile {
                serenity::ButtonStyle::Danger
            } else {
                secondary
            })
            .disabled(!enabled),
        serenity::CreateButton::new(BTN_WORKS)
            .label(i18n::BTN_WORKS)
            .style(secondary)
            .disabled(!enabled),
    ];
    if let Some(url) = link {
        main.push(serenity::CreateButton::new_link(url).label(format!("🔗 {}", i18n::BTN_ANILIST)));
    }
    if show_back {
        main.push(back_button(enabled));
    }
    // "أظهر العمل": a new message with the work's card, never an edit of this one.
    main.extend(crate::components::standard_actions(
        crate::components::ItemKind::Character,
        work_key,
    ));
    // "أظهر المؤدي": the voice actor's card as a new message.
    main.extend(performer.map(crate::components::performer_button));
    let mut extra = vec![
        serenity::CreateButton::new(BTN_DESCRIBE)
            .label(i18n::BTN_DESCRIPTION)
            .emoji('📖')
            .style(secondary)
            .disabled(!enabled),
        serenity::CreateButton::new(BTN_RANDOM)
            .label(i18n::BTN_RANDOM_CHARACTER)
            .emoji('🎲')
            .style(serenity::ButtonStyle::Primary)
            .disabled(!enabled),
        serenity::CreateButton::new(BTN_ROSTER)
            .label(i18n::BTN_ROSTER)
            .emoji('👥')
            .style(secondary)
            .disabled(!enabled),
    ];
    extra.push(close_button(enabled));
    // Over five buttons the source link drops out (the card title links to it); the required ones always stay.
    let (mut rows, _links) = crate::components::card_rows(main);
    rows.push(buttons(extra));
    rows
}

/// Appearances that may be shown under the given policy.
fn works_of(c: &Character, policy: ContentPolicy) -> Vec<&MediaRef> {
    c.media
        .as_ref()
        .map(|m| {
            m.nodes
                .iter()
                .filter(|r| policy.allow_adult || !safety::appearance_adult(r))
                .collect()
        })
        .unwrap_or_default()
}

impl CharacterView {
    pub fn new(
        api: Arc<SafeAniList>,
        policy: ContentPolicy,
        mode: Mode,
        pool: SqlitePool,
        user: i64,
    ) -> Self {
        Self {
            api,
            policy,
            mode,
            history: Vec::new(),
            notice: None,
            pool,
            user,
            roster: None,
            whisper: None,
            providers: None,
            wikis: None,
            live: crate::wiki::fill::LiveFill::new(),
            extra: Vec::new(),
            cover: None,
            from_parent: false,
            in_profile: false,
            suspended: Vec::new(),
        }
    }

    /// A short note shown above the card (for example which provider did not answer).
    pub fn set_notice(&mut self, note: Option<String>) {
        self.notice = note;
    }

    /// Mark the view as a child of another view.
    pub fn with_back(mut self) -> Self {
        self.from_parent = true;
        self
    }

    /// Back at the first screen of a child view means "return to the parent".
    pub fn wants_to_leave(&self, id: &str) -> bool {
        self.from_parent && id == BACK && self.history.is_empty() && self.roster.is_none()
    }

    /// Attach the provider handle and game/VN character matches.
    pub fn with_providers(mut self, providers: Arc<Providers>, extra: Vec<Work>) -> Self {
        self.providers = Some(providers);
        self.extra = extra;
        self
    }

    /// Let the view fill gaps of provider characters from the owner's wikis.
    pub fn with_wikis(mut self, wikis: Arc<crate::wiki::service::Wikis>) -> Self {
        self.wikis = Some(wikis);
        self
    }

    /// Start looking up a wiki fill for the provider character on screen (the card renders without waiting).
    fn start_fill(&mut self) {
        if let (Mode::Provider(w), Some(wikis)) = (&self.mode, &self.wikis) {
            self.live.start(
                wikis,
                &w.id,
                &w.title,
                w.subtitle.as_deref(),
                crate::wiki::Kind::Character,
            );
        }
    }

    async fn open_provider(&mut self, w: Work) {
        self.cover = match &self.providers {
            Some(p) => p.cover(&w).await,
            None => None,
        };
        self.history.push(self.mode.clone());
        self.mode = Mode::Provider(Box::new(w));
        self.start_fill();
    }

    /// The id used in the profile tables for the shown character (AniList id or encoded provider id).
    fn shown_character_id(&self) -> Option<i64> {
        match &self.mode {
            Mode::Card(c) => Some(c.id),
            Mode::Provider(w) => w.character_id(),
            _ => None,
        }
    }

    /// Re-read whether the shown character is in the viewer's profile.
    pub async fn refresh_in_profile(&mut self) {
        self.in_profile = match self.shown_character_id() {
            Some(id) => db::get_entry(&self.pool, self.user, id)
                .await
                .ok()
                .flatten()
                .is_some(),
            None => false,
        };
    }

    /// Prepare the first screen: the profile state and a provider picture.
    pub async fn prepare(&mut self) {
        self.refresh_in_profile().await;
        self.load_cover().await;
    }

    /// Add the shown character to the viewer's profile, or remove it when it is already there (with undo).
    async fn toggle_profile(&mut self) -> Outcome {
        let Some(id) = self.shown_character_id() else {
            return Outcome::Keep;
        };
        let (pool, user) = (self.pool.clone(), self.user);
        if self.in_profile {
            let removed = db::remove_character(&pool, user, id).await;
            self.whisper = Some(match removed {
                Ok(_) => {
                    self.in_profile = false;
                    let mode = self.mode.clone();
                    let undo: Box<dyn FnOnce() -> UndoFuture + Send + Sync> = Box::new(move || {
                        Box::pin(async move {
                            let w = match &mode {
                                Mode::Card(c) => match db::create_profile(&pool, user).await {
                                    Ok(_) => super::profile::add_and_cache(&pool, user, c)
                                        .await
                                        .map(|_| i18n::UNDONE.to_string()),
                                    Err(e) => Err(e.into()),
                                },
                                Mode::Provider(w) => {
                                    Ok(super::fallback::add_to_profile(&pool, user, w).await.text)
                                }
                                _ => Ok(i18n::ERR_GENERIC.to_string()),
                            };
                            w.unwrap_or_else(|_| i18n::ERR_GENERIC.to_string())
                        })
                    });
                    Whisper {
                        text: i18n::CHARACTER_REMOVED.to_string(),
                        undo: Some(undo),
                    }
                }
                Err(e) => {
                    tracing::warn!("could not remove character: {e}");
                    Whisper::text(i18n::ERR_GENERIC)
                }
            });
            return Outcome::Update;
        }
        let whisper = match &self.mode {
            Mode::Provider(w) => super::fallback::add_to_profile(&pool, user, w).await,
            Mode::Card(c) => match db::create_profile(&pool, user).await {
                Ok(_) => match super::profile::add_and_cache(&pool, user, c).await {
                    Ok(text) => {
                        let undo_pool = pool.clone();
                        let undo: Box<dyn FnOnce() -> UndoFuture + Send + Sync> =
                            Box::new(move || {
                                Box::pin(async move {
                                    db::remove_character(&undo_pool, user, id)
                                        .await
                                        .map_or(i18n::ERR_GENERIC, |_| i18n::UNDONE)
                                        .to_string()
                                })
                            });
                        Whisper {
                            text,
                            undo: Some(undo),
                        }
                    }
                    Err(e) => {
                        tracing::warn!("could not add character to profile: {e:#}");
                        Whisper::text(i18n::ERR_GENERIC)
                    }
                },
                Err(e) => {
                    tracing::warn!("profile creation failed: {e}");
                    Whisper::text(i18n::ERR_GENERIC)
                }
            },
            _ => return Outcome::Keep,
        };
        self.whisper = Some(whisper);
        self.refresh_in_profile().await;
        Outcome::Update
    }

    /// Fetch the picture of a provider card (downloaded by the bot, never hotlinked).
    pub async fn load_cover(&mut self) {
        if let (Mode::Provider(w), Some(p)) = (&self.mode, &self.providers) {
            self.cover = p.cover(w).await;
        }
        self.start_fill();
    }

    fn fail(&mut self, e: AniError) -> Outcome {
        tracing::warn!("anilist error in character view: {e}");
        self.notice = Some(super::ani_message(&e).to_string());
        Outcome::Update
    }

    fn actions(&self, enabled: bool) -> serenity::CreateActionRow {
        let mut row = Vec::new();
        if !self.history.is_empty() {
            row.push(back_button(enabled));
        }
        row.push(close_button(enabled));
        buttons(row)
    }
}

impl View for CharacterView {
    fn tick_after(&self) -> Option<std::time::Duration> {
        self.live.pending().then_some(crate::wiki::fill::LIVE_POLL)
    }

    /// The wiki lookup finished: draw the card again with what it gave (the same message is edited).
    async fn on_tick(&mut self) -> Outcome {
        if self.live.poll().await {
            Outcome::Update
        } else {
            Outcome::Keep
        }
    }

    fn render(&self, enabled: bool) -> Page {
        if let Some(r) = &self.roster {
            return r.render(enabled);
        }
        let (embed, mut rows) = match &self.mode {
            Mode::Pick(list) => {
                let mut lines: Vec<String> = list
                    .iter()
                    .enumerate()
                    .map(|(i, c)| match c.main_work() {
                        Some(w) => format!("**{}.** {} — {w}", i + 1, c.display_name()),
                        None => format!("**{}.** {}", i + 1, c.display_name()),
                    })
                    .collect();
                lines.extend(self.extra.iter().enumerate().map(|(i, w)| {
                    format!(
                        "**{}.** {} — {} ({})",
                        list.len() + i + 1,
                        w.title,
                        w.subtitle.as_deref().unwrap_or("-"),
                        w.source.credit()
                    )
                }));
                let body = lines.join("\n");
                let mut choices = character_choices(list);
                choices.extend(self.extra.iter().map(|w| Choice {
                    label: w.title.clone(),
                    description: Some(format!(
                        "{} · {}",
                        w.subtitle.as_deref().unwrap_or("-"),
                        w.source.prefix().to_uppercase()
                    )),
                    value: w.id.clone(),
                }));
                let rows = select_row(PICK_ID, i18n::PICK_PLACEHOLDER, choices, enabled)
                    .into_iter()
                    .collect();
                (Emb::new().title(i18n::PICK_PROMPT).description(body), rows)
            }
            Mode::Card(c) => (
                cards::character_card(c),
                card_rows(
                    c.site_url.as_deref(),
                    self.in_profile,
                    !self.history.is_empty() || self.from_parent,
                    c.media
                        .as_ref()
                        .and_then(|m| m.nodes.first())
                        .and_then(|r| r.id)
                        .map(|id| format!("anilist:{id}"))
                        .as_deref(),
                    Some(&c.id.to_string()),
                    enabled,
                ),
            ),
            Mode::Works { character, page } => {
                let works = works_of(character, self.policy);
                let pages = page_count(works.len(), WORKS_PER_PAGE);
                let slice = page_slice(&works, *page, WORKS_PER_PAGE);
                let choices: Vec<Choice> = slice
                    .iter()
                    .filter_map(|r| {
                        let title = r.title.as_ref()?.main().to_string();
                        let mut bits = Vec::new();
                        if let Some(f) = &r.format {
                            bits.push(crate::ui::format::format_ar(f).to_string());
                        }
                        if let Some(y) = r.season_year {
                            bits.push(y.to_string());
                        }
                        Some(Choice {
                            label: title,
                            description: (!bits.is_empty()).then(|| bits.join(" · ")),
                            value: r.id?.to_string(),
                        })
                    })
                    .collect();
                let mut rows: Vec<_> =
                    select_row(WORK_PICK, i18n::WORKS_PICK_PLACEHOLDER, choices, enabled)
                        .into_iter()
                        .collect();
                rows.push(nav_row(*page, pages, enabled));
                (cards::works_embed(character, slice, *page, pages), rows)
            }
            Mode::Work(m) => {
                let secondary = serenity::ButtonStyle::Secondary;
                let mut row = vec![
                    serenity::CreateButton::new(WORK_ADD)
                        .label(i18n::BTN_FAVOURITE)
                        .emoji('⭐')
                        .style(secondary)
                        .disabled(!enabled),
                    serenity::CreateButton::new(WORK_ROSTER)
                        .label(i18n::BTN_CHARACTERS)
                        .emoji('👥')
                        .style(secondary)
                        .disabled(!enabled),
                ];
                if let Some(u) = &m.site_url {
                    row.push(
                        serenity::CreateButton::new_link(u)
                            .label(format!("🔗 {}", i18n::BTN_ANILIST)),
                    );
                }
                (cards::work_card(m), vec![buttons(row)])
            }
            Mode::ResumeRoster => (Emb::new().description(i18n::ERR_GENERIC), Vec::new()),
            Mode::Provider(w) => {
                let name = self
                    .cover
                    .as_ref()
                    .map_or("cover.jpg", |c| super::fallback::cover_name(c));
                // "أظهر العمل" opens the game (or novel) card as a new message.
                let mut row = crate::components::standard_actions(
                    crate::components::ItemKind::Character,
                    w.work_key.as_deref(),
                );
                row.push(super::fallback::add_button(enabled, self.in_profile));
                if let Some(u) = &w.url {
                    row.push(
                        serenity::CreateButton::new_link(u)
                            .label(format!("🔗 {}", w.source.credit())),
                    );
                }
                let mut emb = super::fallback::card(w, self.cover.is_some(), name);
                if let Some(fill) = self.live.for_key(&w.id) {
                    crate::wiki::fill::enrich(&mut emb, self.cover.is_some(), fill);
                }
                (emb, vec![buttons(row)])
            }
        };
        if !matches!(self.mode, Mode::Card(_)) {
            rows.push(self.actions(enabled));
        }
        let page = Page::embed(embed.build(), rows).notice(self.notice.clone());
        match (&self.mode, &self.cover) {
            (Mode::Provider(_), Some(c)) => {
                page.with_files(vec![serenity::CreateAttachment::bytes(
                    c.to_vec(),
                    super::fallback::cover_name(c).to_string(),
                )])
            }
            _ => page,
        }
    }

    fn take_whisper(&mut self) -> Option<Whisper> {
        self.whisper.take()
    }

    fn modal_for(&self, id: &str) -> Option<ModalSpec> {
        self.roster.as_ref().and_then(|r| r.modal_for(id))
    }

    async fn on_modal(&mut self, id: &str, input: String) -> Outcome {
        let step = match self.roster.as_mut() {
            Some(r) => r.on_modal(id, input).await,
            None => return Outcome::Keep,
        };
        step.apply(&mut self.roster)
    }

    async fn on_start(&mut self) {
        self.prepare().await;
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        let outcome = self.handle(id, values).await;
        self.refresh_in_profile().await;
        outcome
    }
}

impl CharacterView {
    async fn handle(&mut self, id: &str, values: &[String]) -> Outcome {
        self.notice = None;
        if let Some(r) = self.roster.as_mut() {
            let step = r.handle(id, values).await;
            if let Step::Open(c) = step {
                // Keep the roster page the user came from; Back returns to it.
                if let Some(r) = self.roster.take() {
                    self.suspended.push(r);
                }
                self.history.push(self.mode.clone());
                self.history.push(Mode::ResumeRoster);
                self.mode = Mode::Card(*c);
                return Outcome::Update;
            }
            return step.apply(&mut self.roster);
        }
        match id {
            CLOSE => Outcome::Close,
            BACK => match self.history.pop() {
                Some(Mode::ResumeRoster) => {
                    self.mode = self.history.pop().unwrap_or(Mode::Pick(Vec::new()));
                    self.roster = self.suspended.pop();
                    Outcome::Update
                }
                Some(m) => {
                    self.mode = m;
                    Outcome::Update
                }
                None => Outcome::Keep,
            },
            BTN_WORKS => {
                let Mode::Card(c) = self.mode.clone() else {
                    return Outcome::Keep;
                };
                self.history.push(self.mode.clone());
                self.mode = Mode::Works {
                    character: c,
                    page: 0,
                };
                Outcome::Update
            }
            BTN_ADD | super::fallback::BTN_PROVIDER_ADD => self.toggle_profile().await,
            WORK_PICK => {
                let Some(id) = values.first().and_then(|v| v.parse::<i64>().ok()) else {
                    return Outcome::Keep;
                };
                match self.api.media_by_id(id, self.policy).await {
                    Ok(Some(m)) => {
                        self.history.push(self.mode.clone());
                        self.mode = Mode::Work(Box::new(m));
                        Outcome::Update
                    }
                    Ok(None) => {
                        self.notice = Some(i18n::NOT_FOUND.to_string());
                        Outcome::Update
                    }
                    Err(e) => self.fail(e),
                }
            }
            WORK_ADD => {
                let Mode::Work(m) = &self.mode else {
                    return Outcome::Keep;
                };
                let (pool, user) = (self.pool.clone(), self.user);
                let key = works::work_key(m.id);
                let kind = kind_of(m).key().to_string();
                let row = super::media::media_cache_row(m, super::profile::now());
                let result = async {
                    db::create_profile(&pool, user).await?;
                    works::upsert_media_cache(&pool, &row).await?;
                    works::add_work(&pool, user, &key, &kind).await
                }
                .await;
                self.whisper = Some(match result {
                    Ok(db::AddOutcome::Added(_)) => {
                        let undo_pool = self.pool.clone();
                        let undo: Box<dyn FnOnce() -> UndoFuture + Send + Sync> =
                            Box::new(move || {
                                Box::pin(async move {
                                    works::remove_work(&undo_pool, user, &key)
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
                    Ok(db::AddOutcome::Duplicate) => Whisper::text(i18n::WORK_ALREADY),
                    Err(_) => Whisper::text(i18n::ERR_GENERIC),
                });
                Outcome::Keep
            }
            WORK_ROSTER => {
                let Mode::Work(m) = &self.mode else {
                    return Outcome::Keep;
                };
                match Roster::for_work(self.api.clone(), self.policy, m.id, m.title.main()).await {
                    Ok(r) => self.roster = Some(r.with_profile(self.pool.clone(), self.user)),
                    Err(e) => return self.fail(e),
                }
                Outcome::Update
            }
            BTN_ROSTER => {
                let Mode::Card(c) = &self.mode else {
                    return Outcome::Keep;
                };
                match Roster::start(self.api.clone(), self.policy, c).await {
                    Ok(Some(r)) => self.roster = Some(r.with_profile(self.pool.clone(), self.user)),
                    Ok(None) => self.notice = Some(i18n::ROSTER_NO_WORKS.to_string()),
                    Err(e) => return self.fail(e),
                }
                Outcome::Update
            }
            BTN_DESCRIBE => {
                let Mode::Card(c) = &self.mode else {
                    return Outcome::Keep;
                };
                let text = c
                    .description
                    .as_deref()
                    .and_then(|d| {
                        clean_description_max(d, c.site_url.as_deref(), DESCRIPTION_CHARS)
                    })
                    .unwrap_or_else(|| i18n::NO_DESCRIPTION.to_string());
                Outcome::Whisper(text)
            }
            BTN_RANDOM => match self.api.random_character(self.policy).await {
                Ok(Some(c)) => {
                    self.history.clear();
                    self.mode = Mode::Card(c);
                    Outcome::Update
                }
                Ok(None) => {
                    self.notice = Some(i18n::RANDOM_FAILED.to_string());
                    Outcome::Update
                }
                Err(e) => self.fail(e),
            },
            PICK_ID => {
                let Mode::Pick(list) = &self.mode else {
                    return Outcome::Keep;
                };
                let picked = values.first();
                let id = picked.and_then(|v| v.parse::<i64>().ok());
                let Some(c) = list.iter().find(|c| Some(c.id) == id).cloned() else {
                    let found =
                        picked.and_then(|v| self.extra.iter().find(|w| &w.id == v).cloned());
                    return match found {
                        Some(w) => {
                            self.open_provider(w).await;
                            Outcome::Update
                        }
                        None => Outcome::Keep,
                    };
                };
                self.history.push(self.mode.clone());
                self.mode = Mode::Card(c);
                Outcome::Update
            }
            other => {
                let (Some(nav), Mode::Works { character, page }) =
                    (parse_nav(other), &mut self.mode)
                else {
                    return Outcome::Keep;
                };
                let pages = page_count(works_of(character, self.policy).len(), WORKS_PER_PAGE);
                let next = apply_nav(nav, *page, pages);
                if next == *page {
                    return Outcome::Keep;
                }
                *page = next;
                Outcome::Update
            }
        }
    }
}

// ---- entry points ------------------------------------------------------------------------

fn source_of(key: Option<&str>) -> CharacterSource {
    match key.map(str::trim) {
        Some("anime") => CharacterSource::AnimeManga,
        Some("games" | "games-only") => CharacterSource::Games,
        _ => CharacterSource::All,
    }
}

/// Does a title match the `work` filter (case-insensitive substring)?
fn title_matches(title: &str, needle: &str) -> bool {
    title.to_lowercase().contains(&needle.trim().to_lowercase())
}

/// Keep AniList characters that appear in a work matching the filter (by id or title text).
fn filter_by_work(list: Vec<Character>, filter: &WorkFilter) -> Vec<Character> {
    match filter {
        WorkFilter::None => list,
        WorkFilter::Id { id, .. } => list
            .into_iter()
            .filter(|c| {
                works_of(c, ContentPolicy::SAFE)
                    .iter()
                    .any(|r| r.id == Some(*id))
            })
            .collect(),
        WorkFilter::Text(t) => list
            .into_iter()
            .filter(|c| {
                works_of(c, ContentPolicy::SAFE).iter().any(|r| {
                    r.title.as_ref().is_some_and(|ti| {
                        [Some(ti.main()), ti.english.as_deref(), ti.native.as_deref()]
                            .into_iter()
                            .flatten()
                            .any(|x| title_matches(x, t))
                    })
                })
            })
            .collect(),
    }
}

fn filter_providers(list: Vec<Work>, filter: &WorkFilter) -> Vec<Work> {
    let text = match filter {
        WorkFilter::None => return list,
        WorkFilter::Text(t) => t.as_str(),
        WorkFilter::Id { title, .. } => title.as_str(),
    };
    list.into_iter()
        .filter(|w| {
            w.subtitle
                .as_deref()
                .is_some_and(|s| title_matches(s, text))
        })
        .collect()
}

/// The optional `work` option: an autocomplete id, free text, or nothing.
enum WorkFilter {
    None,
    Id { id: i64, title: String },
    Text(String),
}

async fn work_filter(api: &SafeAniList, raw: Option<&str>) -> WorkFilter {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else {
        return WorkFilter::None;
    };
    match parse_id(raw) {
        Some(id) => match api.media_by_id(id, ContentPolicy::SAFE).await {
            Ok(Some(m)) => WorkFilter::Id {
                id,
                title: m.title.main().to_string(),
            },
            _ => WorkFilter::Text(raw.to_string()),
        },
        None => WorkFilter::Text(raw.to_string()),
    }
}

/// Autocomplete label for a game / visual-novel character: name, main work, source.
pub fn provider_label(w: &Work) -> String {
    match &w.subtitle {
        Some(s) => format!("{} — {s} ({})", w.title, w.source.prefix().to_uppercase()),
        None => format!("{} ({})", w.title, w.source.prefix().to_uppercase()),
    }
}

/// Name suggestions ranked by popularity, with the main work in the label.
pub(crate) async fn ac_character(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    let source = source_of(super::profile::option_str(ctx, "source").as_deref());
    character_suggestions(ctx, partial, source).await
}

/// The suggestions of one source (the category commands fix it by their category).
pub(crate) async fn character_suggestions(
    ctx: Context<'_>,
    partial: &str,
    source: CharacterSource,
) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    let (games_on, mature) = super::fallback::game_flags(ctx).await;
    let data = ctx.data();
    let api = data.anilist.clone();
    let providers = data.providers.clone();
    let use_games = source != CharacterSource::AnimeManga && games_on && providers.games_enabled();
    let use_ani = source != CharacterSource::Games;
    let wikis = data.wikis.clone();
    let wiki_list = wikis.sources().await;
    let wiki_names: std::collections::HashMap<i64, String> =
        wiki_list.iter().map(|w| (w.id, w.name.clone())).collect();
    let served = wikis_serving(&wiki_list, source);
    // The options that change the answer are part of the cache scope.
    let scope = format!("{source:?}|{mature}");
    let items = data
        .auto
        .items_scoped(
            ctx.author().id.get(),
            "character",
            &scope,
            partial,
            0,
            |text| async move {
                let (ani, prov) = futures::join!(
                    async {
                        if !use_ani {
                            return Vec::new();
                        }
                        let list = if text.chars().count() >= 2 {
                            api.search_characters(&text, 25, ContentPolicy::SAFE).await
                        } else {
                            api.popular_characters(10, ContentPolicy::SAFE).await
                        };
                        list.unwrap_or_default()
                    },
                    async {
                        if use_games && text.chars().count() >= 2 {
                            // Characters by name, and the games whose rosters the member may mean.
                            let (chars, games) = futures::join!(
                                providers.search_characters_found(&text, mature),
                                providers.search_games_found(&text, None, mature, None)
                            );
                            (chars.works, games.works)
                        } else {
                            (Vec::new(), Vec::new())
                        }
                    }
                );
                let mut out: Vec<(String, String)> = ani
                    .iter()
                    .map(|c| {
                        let label = match c.main_work() {
                            Some(w) => format!("{} — {w}", c.display_name()),
                            None => c.display_name().to_string(),
                        };
                        (label, format!("id:{}", c.id))
                    })
                    .collect();
                out.extend(prov.0.iter().map(|w| (provider_label(w), w.id.clone())));
                // "شخصيات لعبة X": typing a game where a character is expected still leads somewhere.
                out.extend(
                    prov.1
                        .iter()
                        .take(3)
                        .map(|g| (i18n::roster_of_game(&g.title), format!("roster:{}", g.id))),
                );
                // The owner's wikis, from the local index only. The same character (same name and same game) that a main
                // source already listed is not listed again.
                if text.chars().count() >= 2 {
                    let mut known: Vec<(String, Option<String>)> = ani
                        .iter()
                        .map(|c| {
                            (
                                c.display_name().to_string(),
                                c.main_work().map(str::to_string),
                            )
                        })
                        .collect();
                    known.extend(prov.0.iter().map(|w| (w.title.clone(), w.subtitle.clone())));
                    for pg in wikis
                        .suggest(&text, Some(crate::wiki::Kind::Character), None, 8)
                        .await
                    {
                        if !served.contains(&pg.wiki_id) {
                            continue;
                        }
                        if known.iter().any(|(n, g)| {
                            crate::wiki::same_identity(
                                n,
                                g.as_deref(),
                                &pg.title,
                                pg.parent_title.as_deref(),
                            )
                        }) {
                            continue;
                        }
                        let wiki = wiki_names.get(&pg.wiki_id).cloned().unwrap_or_default();
                        let label = match &pg.parent_title {
                            Some(g) => format!("{} — {g} ({wiki})", pg.title),
                            None => format!("{} ({wiki})", pg.title),
                        };
                        out.push((label, pg.key()));
                    }
                }
                out
            },
        )
        .await;
    to_choices(items)
}

/// The wikis whose characters answer this command: a wiki of a known medium serves only its own (a manhwa wiki serves
/// /manga and /anime, a game wiki /game), a mixed wiki serves all.
fn wikis_serving(
    list: &[crate::wiki::Source],
    source: CharacterSource,
) -> std::collections::HashSet<i64> {
    use crate::wiki::Kind;
    list.iter()
        .filter(|w| match crate::wiki::parse::wiki_medium(w) {
            None => true,
            Some(m) => match source {
                CharacterSource::AnimeManga => matches!(m, Kind::Anime | Kind::Manga | Kind::Novel),
                CharacterSource::Games => m == Kind::Game,
                CharacterSource::All => true,
            },
        })
        .map(|w| w.id)
        .collect()
}

/// Resolve the typed or picked value and open the card (or a select menu for several matches).
pub async fn search_character(
    ctx: Context<'_>,
    name: String,
    work: Option<String>,
    source: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let api = ctx.data().anilist.clone();
    let providers = ctx.data().providers.clone();
    let policy = policy(ctx);
    // "games-only" (a provider was chosen with `source`) keeps the wikis out of the fallback.
    let wikis_allowed = source.as_deref() != Some("games-only");
    let source = source_of(source.as_deref());
    let (games_on, mature) = super::fallback::game_flags(ctx).await;
    let use_games = source != CharacterSource::AnimeManga && games_on && providers.games_enabled();
    let (pool, user) = (ctx.data().db.clone(), ctx.author().id.get() as i64);
    let new_view = |mode: Mode| CharacterView::new(api.clone(), policy, mode, pool.clone(), user);

    // A suggestion that was picked from the autocomplete list.
    if let Some(id) = parse_id(&name) {
        let mode = match api.character_by_id(id, policy).await {
            Ok(CharacterLookup::Found(c)) => Mode::Card(*c),
            Ok(CharacterLookup::Hidden) => return say_ephemeral(ctx, i18n::HIDDEN_NOTICE).await,
            Ok(CharacterLookup::NotFound) => return say_ephemeral(ctx, i18n::NOT_FOUND).await,
            Err(e) => return fail(ctx, e).await,
        };
        return session::run(ctx, new_view(mode)).await;
    }
    // "شخصيات لعبة X" picked from the suggestions: the game's roster.
    if let Some(game) = name.strip_prefix("roster:") {
        if !use_games {
            return say_ephemeral(ctx, game_notice(games_on, &providers)).await;
        }
        let msg = crate::global::roster_msg(ctx.data(), game.trim(), 0, mature).await;
        return crate::global::reply(ctx, msg).await;
    }
    // A page of a wiki picked from the suggestions.
    if let Some((wiki, page)) = crate::wiki::parse_key(name.trim()) {
        let msg = crate::wiki::msgs::page_msg(
            &ctx.data().wikis,
            wiki,
            page,
            ctx.author().id.get() as i64,
        )
        .await;
        return crate::global::reply(ctx, msg).await;
    }
    if name.starts_with("igdb:c") || name.starts_with("vndb:c") {
        if !use_games {
            return say_ephemeral(ctx, game_notice(games_on, &providers)).await;
        }
        return match providers.character_by_key(name.trim(), mature).await {
            Some(w) => {
                let mut view = new_view(Mode::Provider(Box::new(w)))
                    .with_providers(providers, Vec::new())
                    .with_wikis(ctx.data().wikis.clone());
                view.load_cover().await;
                session::run(ctx, view).await
            }
            None => say_ephemeral(ctx, i18n::NOT_FOUND).await,
        };
    }

    let query = name.trim();
    let filter = work_filter(&api, work.as_deref()).await;
    let (ani, prov) = futures::join!(
        async {
            if source == CharacterSource::Games {
                return Ok(Vec::new());
            }
            api.search_characters(query, 10, policy).await
        },
        async {
            if use_games {
                providers.search_characters_found(query, mature).await
            } else {
                crate::providers::Found::default()
            }
        }
    );
    let prov_found = prov;
    let prov = filter_providers(prov_found.works.clone(), &filter);
    let ani = match ani {
        Ok(list) => filter_by_work(list, &filter),
        // A dead AniList must not hide game results.
        Err(e) if prov.is_empty() => return fail(ctx, e).await,
        Err(e) => {
            tracing::warn!("anilist character search failed, showing providers only: {e}");
            Vec::new()
        }
    };
    if ani.is_empty() && prov.is_empty() {
        if source == CharacterSource::Games && !use_games {
            return say_ephemeral(ctx, game_notice(games_on, &providers)).await;
        }
        // The owner's wikis come in when the main sources have nothing: shown directly, no hint, no confirmation.
        if wikis_allowed && matches!(filter, WorkFilter::None) {
            let list = ctx.data().wikis.sources().await;
            let served = wikis_serving(&list, source);
            let mut found = ctx
                .data()
                .wikis
                .search(query, Some(crate::wiki::Kind::Character), None)
                .await;
            found.pages.retain(|p| served.contains(&p.wiki_id));
            if let Some(msg) = crate::wiki::msgs::show(
                &ctx.data().wikis,
                found,
                query,
                ctx.author().id.get() as i64,
            )
            .await
            {
                return crate::global::reply(ctx, msg).await;
            }
        }
        // No character has that name: the member may have typed a game ("Silent Hill"). Search the games and show the
        // roster of the best match (never when every provider failed: that is "unavailable", not "no such game").
        if use_games && !prov_found.all_failed() && matches!(filter, WorkFilter::None) {
            let games = providers
                .search_games_found(query, None, mature, None)
                .await;
            let best = games
                .works
                .iter()
                .find(|g| g.title.eq_ignore_ascii_case(query))
                .or_else(|| games.works.first());
            if let Some(game) = best {
                let mut msg = crate::global::roster_msg(ctx.data(), &game.id, 0, mature).await;
                if let Some(note) = prov_found.partial_note().or_else(|| games.partial_note()) {
                    msg.text = Some(note);
                }
                return crate::global::reply(ctx, msg).await;
            }
            if games.all_failed() {
                return say_ephemeral(ctx, i18n::GAMES_DOWN).await;
            }
        }
        let text = if use_games {
            prov_found.empty_message(i18n::NO_RESULTS)
        } else {
            i18n::NO_RESULTS
        };
        return say_ephemeral(ctx, text).await;
    }
    let mut ani = ani;
    let (mode, extra) = match (ani.len(), prov.len()) {
        (1, 0) => (Mode::Card(ani.remove(0)), Vec::new()),
        (0, 1) => (Mode::Provider(Box::new(prov[0].clone())), Vec::new()),
        _ => (Mode::Pick(ani), prov),
    };
    let mut view = new_view(mode)
        .with_providers(providers, extra)
        .with_wikis(ctx.data().wikis.clone());
    view.load_cover().await;
    view.set_notice(prov_found.partial_note());
    session::run(ctx, view).await
}

fn game_notice(games_on: bool, providers: &Providers) -> &'static str {
    if !providers.games_enabled() {
        i18n::GAMES_UNAVAILABLE
    } else if !games_on {
        i18n::GAMES_SERVER_OFF
    } else {
        i18n::NO_RESULTS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{Kind, Source};

    fn prov(title: &str, from: &str) -> Work {
        Work {
            work_key: None,
            id: "igdb:c1".into(),
            source: Source::Igdb,
            kind: Kind::Character,
            title: title.into(),
            native: None,
            subtitle: Some(from.into()),
            year: None,
            status: None,
            rating: None,
            url: None,
            cover_url: None,
            genres: Vec::new(),
        }
    }

    fn labels(rows: &[serenity::CreateActionRow]) -> Vec<String> {
        rows.iter()
            .flat_map(|r| {
                serde_json::to_value(r).expect("json")["components"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            })
            .map(|b| b["label"].as_str().unwrap_or("").to_string())
            .collect()
    }

    #[test]
    fn every_character_image_has_add_works_link_and_back() {
        let rows = card_rows(
            Some("https://anilist.co/character/1"),
            false,
            true,
            Some("anilist:5"),
            Some("5"),
            true,
        );
        let l = labels(&rows);
        for want in [
            i18n::BTN_ADD_TO_PROFILE,
            i18n::BTN_WORKS,
            i18n::BTN_BACK,
            crate::components::LABEL_SHOW_WORK,
            crate::components::LABEL_SHOW_PERFORMER,
        ] {
            assert!(l.iter().any(|x| x == want), "{want} missing in {l:?}");
        }
        // Six buttons do not fit one row: the source link is the first to go.
        assert!(
            !l.iter().any(|x| x.contains(i18n::BTN_ANILIST)),
            "the link drops out of a crowded row"
        );
        assert!(
            rows.len() <= 5
                && rows
                    .iter()
                    .all(|r| serde_json::to_value(r).expect("json")["components"]
                        .as_array()
                        .map_or(0, Vec::len)
                        <= 5)
        );
    }

    #[test]
    fn the_add_button_becomes_remove_when_the_character_is_in_my_profile() {
        let l = labels(&card_rows(None, true, false, None, None, true));
        assert!(l.iter().any(|x| x == i18n::BTN_REMOVE_FROM_PROFILE));
        assert!(!l.iter().any(|x| x == i18n::BTN_ADD_TO_PROFILE));
        assert!(
            !l.iter().any(|x| x == i18n::BTN_BACK),
            "no back button without a parent"
        );
        let provider = |in_profile| {
            serde_json::to_value(super::super::fallback::add_button(true, in_profile))
                .expect("json")["label"]
                .as_str()
                .unwrap_or("")
                .to_string()
        };
        assert_eq!(provider(false), i18n::BTN_ADD_TO_PROFILE);
        assert_eq!(
            provider(true),
            i18n::BTN_REMOVE_FROM_PROFILE,
            "game characters toggle too"
        );
    }

    #[test]
    fn source_keys_map_to_sources() {
        assert_eq!(source_of(None), CharacterSource::All);
        assert_eq!(source_of(Some("all")), CharacterSource::All);
        assert_eq!(source_of(Some("anime")), CharacterSource::AnimeManga);
        assert_eq!(source_of(Some(" games ")), CharacterSource::Games);
    }

    #[test]
    fn labels_show_the_main_work_and_source() {
        assert_eq!(
            provider_label(&prov("Mario", "Super Mario Bros.")),
            "Mario — Super Mario Bros. (IGDB)"
        );
        let mut w = prov("Mario", "x");
        w.subtitle = None;
        assert_eq!(provider_label(&w), "Mario (IGDB)");
    }

    #[test]
    fn work_filter_applies_to_game_characters() {
        let list = vec![
            prov("Mario", "Super Mario Bros."),
            prov("Pyramid Head", "Silent Hill 2"),
        ];
        let kept = filter_providers(list.clone(), &WorkFilter::Text("silent".into()));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].title, "Pyramid Head");
        assert_eq!(filter_providers(list.clone(), &WorkFilter::None).len(), 2);
        let by_id = WorkFilter::Id {
            id: 1,
            title: "Super Mario".into(),
        };
        assert_eq!(filter_providers(list, &by_id)[0].title, "Mario");
    }
}
