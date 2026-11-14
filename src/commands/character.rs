//! `/character search` and the interactive character card (also used by `/random character`).

use super::{buttons, fail, parse_id, policy, say_ephemeral};
use crate::{
    anilist::{
        models::{Character, MediaRef},
        safety, AniError, CharacterLookup, ContentPolicy, SafeAniList,
    },
    error::{Context, Error},
    i18n,
    ui::{
        autocomplete::to_choices,
        cards,
        embeds::Emb,
        pagination::{
            apply_nav, back_button, close_button, nav_row, page_count, page_slice, parse_nav, BACK,
            CLOSE,
        },
        select::{character_choices, pick_row, PICK_ID},
        session::{self, Outcome, Page, View},
    },
};
use poise::serenity_prelude as serenity;
use std::sync::Arc;

const BTN_WORKS: &str = "works";
const BTN_RANDOM: &str = "random_character";
const WORKS_PER_PAGE: usize = 10;

#[derive(Clone)]
pub enum Mode {
    Pick(Vec<Character>),
    Card(Character),
    Works { character: Character, page: usize },
}

pub struct CharacterView {
    api: Arc<SafeAniList>,
    policy: ContentPolicy,
    mode: Mode,
    history: Vec<Mode>,
    notice: Option<String>,
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
    pub fn new(api: Arc<SafeAniList>, policy: ContentPolicy, mode: Mode) -> Self {
        Self {
            api,
            policy,
            mode,
            history: Vec::new(),
            notice: None,
        }
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
    fn render(&self, enabled: bool) -> Page {
        let (embed, mut rows) = match &self.mode {
            Mode::Pick(list) => {
                let body = list
                    .iter()
                    .enumerate()
                    .map(|(i, c)| match c.main_work() {
                        Some(w) => format!("**{}.** {} — {w}", i + 1, c.display_name()),
                        None => format!("**{}.** {}", i + 1, c.display_name()),
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let rows = pick_row(character_choices(list), enabled)
                    .into_iter()
                    .collect();
                (Emb::new().title(i18n::PICK_PROMPT).description(body), rows)
            }
            Mode::Card(c) => {
                let works = works_of(c, self.policy);
                let secondary = serenity::ButtonStyle::Secondary;
                let mut main = vec![
                    serenity::CreateButton::new(BTN_WORKS)
                        .label(i18n::BTN_WORKS)
                        .style(secondary)
                        .disabled(!enabled),
                    // Placeholder until profiles exist (milestone 4).
                    serenity::CreateButton::new("add_to_profile")
                        .label(i18n::BTN_ADD_TO_PROFILE)
                        .emoji('➕')
                        .style(secondary)
                        .disabled(true),
                    serenity::CreateButton::new(BTN_RANDOM)
                        .label(i18n::BTN_RANDOM_CHARACTER)
                        .emoji('🎲')
                        .style(serenity::ButtonStyle::Primary)
                        .disabled(!enabled),
                ];
                if let Some(url) = &c.site_url {
                    main.push(
                        serenity::CreateButton::new_link(url)
                            .label(format!("🔗 {}", i18n::BTN_ANILIST)),
                    );
                }
                let mut extra = Vec::new();
                if !self.history.is_empty() {
                    extra.push(back_button(enabled));
                }
                extra.push(close_button(enabled));
                // Server favourites become real in milestone 4.
                (
                    cards::character_embed(c, &works, 0),
                    vec![buttons(main), buttons(extra)],
                )
            }
            Mode::Works { character, page } => {
                let works = works_of(character, self.policy);
                let pages = page_count(works.len(), WORKS_PER_PAGE);
                let slice = page_slice(&works, *page, WORKS_PER_PAGE);
                (
                    cards::works_embed(character, slice, *page, pages),
                    vec![nav_row(*page, pages, enabled)],
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
                let id = values.first().and_then(|v| v.parse::<i64>().ok());
                let Some(c) = list.iter().find(|c| Some(c.id) == id).cloned() else {
                    return Outcome::Keep;
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

async fn ac_character(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let data = ctx.data();
    let api = data.anilist.clone();
    let items = data
        .auto
        .items(
            ctx.author().id.get(),
            "character",
            partial,
            |q| async move {
                match api.search_characters(&q, 10, ContentPolicy::SAFE).await {
                    Ok(list) => list
                        .iter()
                        .map(|c| {
                            let label = match c.main_work() {
                                Some(w) => format!("{} — {w}", c.display_name()),
                                None => c.display_name().to_string(),
                            };
                            (label, format!("id:{}", c.id))
                        })
                        .collect(),
                    Err(_) => Vec::new(),
                }
            },
        )
        .await;
    to_choices(items)
}

/// Character commands.
#[poise::command(
    slash_command,
    subcommands("search"),
    name_localized("ar", "شخصية"),
    description_localized("ar", "أوامر الشخصيات")
)]
pub async fn character(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Search for a character.
#[poise::command(
    slash_command,
    name_localized("ar", "بحث"),
    description_localized("ar", "ابحث عن شخصية")
)]
pub async fn search(
    ctx: Context<'_>,
    #[description = "Character name"]
    #[description_localized("ar", "اسم الشخصية")]
    #[name_localized("ar", "الاسم")]
    #[autocomplete = "ac_character"]
    name: String,
) -> Result<(), Error> {
    ctx.defer().await?;
    let api = ctx.data().anilist.clone();
    let policy = policy(ctx);
    let mode = if let Some(id) = parse_id(&name) {
        match api.character_by_id(id, policy).await {
            Ok(CharacterLookup::Found(c)) => Mode::Card(*c),
            Ok(CharacterLookup::Hidden) => return say_ephemeral(ctx, i18n::HIDDEN_NOTICE).await,
            Ok(CharacterLookup::NotFound) => return say_ephemeral(ctx, i18n::NOT_FOUND).await,
            Err(e) => return fail(ctx, e).await,
        }
    } else {
        match api.search_characters(name.trim(), 10, policy).await {
            Ok(mut v) if v.len() == 1 => Mode::Card(v.remove(0)),
            Ok(v) if v.is_empty() => return say_ephemeral(ctx, i18n::NO_RESULTS).await,
            Ok(v) => Mode::Pick(v),
            Err(e) => return fail(ctx, e).await,
        }
    };
    session::run(ctx, CharacterView::new(api, policy, mode)).await
}
