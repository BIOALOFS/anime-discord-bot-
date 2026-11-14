//! `/anime`, `/manga` and the interactive media card shared with `/seasonal` and `/random`.

use super::{buttons, fail, parse_id, policy, say_ephemeral};
use crate::{
    anilist::{
        models::{Character, Media},
        AniError, ContentPolicy, MediaKind, SafeAniList, SeasonalQuery,
    },
    error::{Context, Error},
    i18n,
    ui::{
        autocomplete::to_choices,
        cards,
        format::{format_ar, season_ar},
        pagination::{apply_nav, back_button, close_button, nav_row, parse_nav, BACK, CLOSE},
        select::{media_choices, pick_row, select_row, PICK_ID},
        session::{self, Outcome, Page, View},
    },
};
use poise::serenity_prelude as serenity;
use std::sync::Arc;

const BTN_CHARACTERS: &str = "characters";
const BTN_SIMILAR: &str = "similar";
const BTN_RELATED: &str = "related";
const BTN_REROLL: &str = "reroll";
const MAX_HISTORY: usize = 8;
const MAX_RELATED: usize = 10;

/// Where a card came from; decides which extra buttons and paging it has.
#[derive(Clone)]
pub enum Source {
    Plain,
    Random {
        kind: MediaKind,
        genre: Option<String>,
        min_score: Option<u8>,
    },
    Seasonal(SeasonalQuery),
}

#[derive(Clone)]
pub enum Mode {
    Pick(Vec<Media>),
    Card(Media),
    Characters {
        media: Media,
        page: usize,
        pages: usize,
        items: Vec<(Character, String)>,
    },
    Similar {
        media: Media,
        page: usize,
        pages: usize,
        items: Vec<Media>,
    },
    Related {
        media: Media,
        items: Vec<(String, Media)>,
    },
    Seasonal {
        page: usize,
        pages: usize,
        items: Vec<Media>,
    },
}

pub struct MediaView {
    api: Arc<SafeAniList>,
    policy: ContentPolicy,
    source: Source,
    mode: Mode,
    history: Vec<Mode>,
    notice: Option<String>,
}

pub(super) fn pages_of(last_page: Option<usize>, has_next: bool, page: usize) -> usize {
    last_page
        .unwrap_or(page + 1 + usize::from(has_next))
        .max(page + 1)
}

impl MediaView {
    pub fn new(api: Arc<SafeAniList>, policy: ContentPolicy, source: Source, mode: Mode) -> Self {
        Self {
            api,
            policy,
            source,
            mode,
            history: Vec::new(),
            notice: None,
        }
    }

    fn fail(&mut self, e: AniError) -> Outcome {
        tracing::warn!("anilist error in media view: {e}");
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
            Mode::Card(m) => Some(m.clone()),
            _ => None,
        }
    }

    fn list_media(&self) -> Vec<Media> {
        match &self.mode {
            Mode::Pick(l) | Mode::Similar { items: l, .. } | Mode::Seasonal { items: l, .. } => {
                l.clone()
            }
            Mode::Related { items, .. } => items.iter().map(|(_, m)| m.clone()).collect(),
            _ => Vec::new(),
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
                    media,
                    page: 0,
                    pages,
                    items: p.items,
                };
                Outcome::Update
            }
            Err(e) => self.fail(e),
        }
    }

    async fn open_similar(&mut self) -> Outcome {
        let Some(media) = self.current_card() else {
            return Outcome::Keep;
        };
        match self.api.recommendations(media.id, 1, self.policy).await {
            Ok(p) => {
                let pages = pages_of(p.last_page, p.has_next, 0);
                self.push_history();
                self.mode = Mode::Similar {
                    media,
                    page: 0,
                    pages,
                    items: p.items,
                };
                Outcome::Update
            }
            Err(e) => self.fail(e),
        }
    }

    async fn open_related(&mut self) -> Outcome {
        let Some(media) = self.current_card() else {
            return Outcome::Keep;
        };
        match self.api.relations(media.id, self.policy).await {
            Ok(mut items) => {
                items.sort_by_key(|(rel, _)| cards::relation_rank(rel));
                items.truncate(MAX_RELATED);
                self.push_history();
                self.mode = Mode::Related { media, items };
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
                self.mode = Mode::Card(m);
            }
            Ok(None) => self.notice = Some(i18n::RANDOM_FAILED.to_string()),
            Err(e) => return self.fail(e),
        }
        Outcome::Update
    }

    fn open_picked(&mut self, values: &[String]) -> Outcome {
        let Some(id) = values.first().and_then(|v| v.parse::<i64>().ok()) else {
            return Outcome::Keep;
        };
        let Some(media) = self.list_media().into_iter().find(|m| m.id == id) else {
            return Outcome::Keep;
        };
        self.push_history();
        self.mode = Mode::Card(media);
        Outcome::Update
    }

    async fn navigate(&mut self, nav: crate::ui::pagination::Nav) -> Outcome {
        enum Target {
            Characters(i64),
            Similar(i64),
            Seasonal,
        }
        let (current, pages, target) = match &self.mode {
            Mode::Characters {
                media, page, pages, ..
            } => (*page, *pages, Target::Characters(media.id)),
            Mode::Similar {
                media, page, pages, ..
            } => (*page, *pages, Target::Similar(media.id)),
            Mode::Seasonal { page, pages, .. } => (*page, *pages, Target::Seasonal),
            _ => return Outcome::Keep,
        };
        let next = apply_nav(nav, current, pages);
        if next == current {
            return Outcome::Keep;
        }
        let api_page = (next + 1) as u32;
        let result: Result<Mode, AniError> = match (&self.mode, target) {
            (Mode::Characters { media, .. }, Target::Characters(id)) => self
                .api
                .media_characters(id, api_page, self.policy)
                .await
                .map(|p| Mode::Characters {
                    media: media.clone(),
                    page: next,
                    pages: pages_of(p.last_page, p.has_next, next).max(pages),
                    items: p.items,
                }),
            (Mode::Similar { media, .. }, Target::Similar(id)) => self
                .api
                .recommendations(id, api_page, self.policy)
                .await
                .map(|p| Mode::Similar {
                    media: media.clone(),
                    page: next,
                    pages: pages_of(p.last_page, p.has_next, next).max(pages),
                    items: p.items,
                }),
            (Mode::Seasonal { .. }, Target::Seasonal) => {
                let Source::Seasonal(q) = &self.source else {
                    return Outcome::Keep;
                };
                self.api
                    .seasonal(q, api_page, self.policy)
                    .await
                    .map(|p| Mode::Seasonal {
                        page: next,
                        pages: pages_of(p.last_page, p.has_next, next).max(pages),
                        items: p.items,
                    })
            }
            _ => return Outcome::Keep,
        };
        match result {
            Ok(mode) => {
                self.mode = mode;
                Outcome::Update
            }
            Err(e) => self.fail(e),
        }
    }

    fn card_rows(&self, m: &Media, enabled: bool) -> Vec<serenity::CreateActionRow> {
        let secondary = serenity::ButtonStyle::Secondary;
        let mut main = vec![
            serenity::CreateButton::new(BTN_CHARACTERS)
                .label(i18n::BTN_CHARACTERS)
                .style(secondary)
                .disabled(!enabled),
            serenity::CreateButton::new(BTN_SIMILAR)
                .label(i18n::BTN_SIMILAR)
                .style(secondary)
                .disabled(!enabled),
            serenity::CreateButton::new(BTN_RELATED)
                .label(i18n::BTN_RELATED)
                .style(secondary)
                .disabled(!enabled),
            // Placeholder until profiles exist (milestone 4).
            serenity::CreateButton::new("favourite")
                .label(i18n::BTN_FAVOURITE)
                .emoji('⭐')
                .style(secondary)
                .disabled(true),
        ];
        if let Some(url) = &m.site_url {
            main.push(
                serenity::CreateButton::new_link(url).label(format!("🔗 {}", i18n::BTN_ANILIST)),
            );
        }
        let mut extra = Vec::new();
        if let Some(url) = m.trailer_url() {
            extra.push(
                serenity::CreateButton::new_link(url).label(format!("▶ {}", i18n::BTN_TRAILER)),
            );
        }
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

    fn actions(&self, enabled: bool) -> serenity::CreateActionRow {
        let mut row = Vec::new();
        if !self.history.is_empty() {
            row.push(back_button(enabled));
        }
        row.push(close_button(enabled));
        buttons(row)
    }

    fn seasonal_title(&self) -> String {
        match &self.source {
            Source::Seasonal(q) => i18n::seasonal_title(season_ar(q.season), q.year),
            _ => String::new(),
        }
    }
}

impl View for MediaView {
    fn render(&self, enabled: bool) -> Page {
        let (embed, mut rows) = match &self.mode {
            Mode::Pick(list) => (
                cards::picker_embed(list),
                pick_row(media_choices(list), enabled).into_iter().collect(),
            ),
            Mode::Card(m) => (cards::media_embed(m), self.card_rows(m, enabled)),
            Mode::Characters {
                media,
                page,
                pages,
                items,
            } => (
                cards::characters_embed(media, items, *page, *pages),
                vec![nav_row(*page, *pages, enabled)],
            ),
            Mode::Similar {
                media,
                page,
                pages,
                items,
            } => {
                let mut rows: Vec<_> = pick_row(media_choices(items), enabled)
                    .into_iter()
                    .collect();
                rows.push(nav_row(*page, *pages, enabled));
                (cards::similar_embed(media, items, *page, *pages), rows)
            }
            Mode::Related { media, items } => {
                let list: Vec<Media> = items.iter().map(|(_, m)| m.clone()).collect();
                (
                    cards::related_embed(media, items),
                    pick_row(media_choices(&list), enabled)
                        .into_iter()
                        .collect(),
                )
            }
            Mode::Seasonal { page, pages, items } => {
                let mut rows: Vec<_> = select_row(
                    PICK_ID,
                    i18n::SEASONAL_PLACEHOLDER,
                    media_choices(items),
                    enabled,
                )
                .into_iter()
                .collect();
                rows.push(nav_row(*page, *pages, enabled));
                (
                    cards::seasonal_embed(&self.seasonal_title(), items, *page, *pages),
                    rows,
                )
            }
        };
        if !matches!(self.mode, Mode::Card(_)) {
            rows.push(self.actions(enabled));
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
                    Outcome::Update
                }
                None => Outcome::Keep,
            },
            BTN_CHARACTERS => self.open_characters().await,
            BTN_SIMILAR => self.open_similar().await,
            BTN_RELATED => self.open_related().await,
            BTN_REROLL => self.reroll().await,
            PICK_ID => self.open_picked(values),
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
    if let Some(y) = m.season_year {
        bits.push(y.to_string());
    }
    if let Some(f) = &m.format {
        bits.push(format_ar(f).to_string());
    }
    if bits.is_empty() {
        m.title.main().to_string()
    } else {
        format!("{} ({})", m.title.main(), bits.join(" · "))
    }
}

pub async fn media_autocomplete(
    ctx: Context<'_>,
    kind: MediaKind,
    label: &'static str,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
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

async fn ac_anime(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    media_autocomplete(ctx, MediaKind::Anime, "anime", partial).await
}

async fn ac_manga(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    media_autocomplete(ctx, MediaKind::Manga, "manga", partial).await
}

/// Resolve the typed value (autocomplete id or free text) and run the interactive card.
async fn run_media(ctx: Context<'_>, kind: MediaKind, input: String) -> Result<(), Error> {
    ctx.defer().await?;
    let api = ctx.data().anilist.clone();
    let policy = policy(ctx);
    let mode = if let Some(id) = parse_id(&input) {
        match api.media_by_id(id, policy).await {
            Ok(Some(m)) => Mode::Card(m),
            Ok(None) => return say_ephemeral(ctx, i18n::NOT_FOUND).await,
            Err(e) => return fail(ctx, e).await,
        }
    } else {
        match api.search_media(input.trim(), kind, 10, policy).await {
            Ok(mut v) if v.len() == 1 => Mode::Card(v.remove(0)),
            Ok(v) if v.is_empty() => return say_ephemeral(ctx, i18n::NO_RESULTS).await,
            Ok(v) => Mode::Pick(v),
            Err(e) => return fail(ctx, e).await,
        }
    };
    session::run(ctx, MediaView::new(api, policy, Source::Plain, mode)).await
}

/// Search for an anime.
#[poise::command(
    slash_command,
    name_localized("ar", "أنمي"),
    description_localized("ar", "ابحث عن أنمي")
)]
pub async fn anime(
    ctx: Context<'_>,
    #[description = "Anime title"]
    #[description_localized("ar", "اسم الأنمي")]
    #[name_localized("ar", "الاسم")]
    #[autocomplete = "ac_anime"]
    name: String,
) -> Result<(), Error> {
    run_media(ctx, MediaKind::Anime, name).await
}

/// Search for a manga.
#[poise::command(
    slash_command,
    name_localized("ar", "مانغا"),
    description_localized("ar", "ابحث عن مانغا")
)]
pub async fn manga(
    ctx: Context<'_>,
    #[description = "Manga title"]
    #[description_localized("ar", "اسم المانغا")]
    #[name_localized("ar", "الاسم")]
    #[autocomplete = "ac_manga"]
    name: String,
) -> Result<(), Error> {
    run_media(ctx, MediaKind::Manga, name).await
}
