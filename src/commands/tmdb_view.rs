//! The card of a TMDB show or film (and the list of several matches). Same rules as every work message: the
//! universal buttons, a status select, "عشوائي آخر" when it came from a random pick.

use super::{buttons, say_ephemeral};
use crate::{
    components::{self, ItemKind},
    db::status,
    error::{Context, Error},
    i18n,
    tmdb::{self, card, find, model::Work, Tmdb},
    ui::{
        embeds::{fit, Emb},
        pagination::{back_button, close_button, BACK, CLOSE},
        select::{select_row, Choice, PICK_ID},
        session::{self, Outcome, Page, View},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::{collections::HashMap, sync::Arc};

const BTN_REROLL: &str = "tmdb_reroll";

/// How to roll another random pick.
#[derive(Clone)]
pub struct Reroll {
    pub cat: find::Cat,
    pub filter: find::Filter,
    pub overrides: HashMap<String, tmdb::model::Target>,
}

pub struct TmdbView {
    tmdb: Arc<Tmdb>,
    pool: SqlitePool,
    user: i64,
    spoiler: bool,
    /// Search hits (list entries) while choosing.
    list: Vec<Work>,
    card: Option<Box<Work>>,
    status: Option<String>,
    reroll: Option<Reroll>,
    notice: Option<String>,
    /// Showing this cast entry of `card` instead of the work itself (random characters).
    cast: Option<usize>,
}

impl TmdbView {
    pub fn new(ctx: Context<'_>, spoiler: bool) -> Self {
        let d = ctx.data();
        Self {
            tmdb: d.tmdb.clone(),
            pool: d.db.clone(),
            user: ctx.author().id.get() as i64,
            spoiler,
            list: Vec::new(),
            card: None,
            status: None,
            reroll: None,
            notice: None,
            cast: None,
        }
    }

    pub fn with_list(mut self, list: Vec<Work>) -> Self {
        self.list = list;
        self
    }

    pub async fn with_card(mut self, w: Work) -> Self {
        self.show(w).await;
        self
    }

    /// Start on a cast entry (a character) of `w`.
    pub fn with_cast(mut self, w: Work, index: usize) -> Self {
        self.card = Some(Box::new(w));
        self.cast = Some(index);
        self
    }

    pub fn with_reroll(mut self, r: Reroll) -> Self {
        self.reroll = Some(r);
        self
    }

    async fn show(&mut self, w: Work) {
        self.status = status::get(&self.pool, self.user, &w.key())
            .await
            .ok()
            .flatten();
        self.card = Some(Box::new(w));
    }

    fn cast_page(&self, w: &Work, c: &tmdb::model::Cast, enabled: bool) -> Page {
        let work_key = w.key();
        let kind = w.kind.unwrap_or(tmdb::model::Kind::Tv);
        let mut main = components::standard_actions(ItemKind::Character, Some(&work_key));
        main.push(components::add_button(&c.key(kind, w.id)));
        main.push(components::performer_button(&c.key(kind, w.id)));
        main.push(serenity::CreateButton::new_link(w.url()).label("🔗 TMDB"));
        main.push(close_button(enabled));
        let mut rows = vec![buttons(main)];
        if self.reroll.is_some() {
            rows.push(buttons(vec![serenity::CreateButton::new(BTN_REROLL)
                .label(i18n::BTN_REROLL)
                .emoji('🎲')
                .style(serenity::ButtonStyle::Primary)
                .disabled(!enabled)]));
        }
        Page::embed(card::cast_embed(c, &w.title, &w.url()).build(), rows)
            .notice(self.notice.clone())
    }

    fn card_page(&self, w: &Work, enabled: bool) -> Page {
        if let Some(c) = self.cast.and_then(|i| w.cast.get(i)) {
            return self.cast_page(w, c, enabled);
        }
        let key = w.key();
        let mut main = components::standard_actions(ItemKind::Work, Some(&key));
        main.push(components::add_button(&key));
        main.push(components::remind_button(&key));
        main.push(serenity::CreateButton::new_link(w.url()).label("🔗 TMDB"));
        main.push(close_button(enabled));
        // Too many buttons for one row: the link goes (the title already links to TMDB), then the optional ones move.
        let (mut rows, _links) = components::card_rows(main);
        rows.extend(components::status_menu(&key, false, self.status.as_deref()));
        let mut extra = Vec::new();
        if self.reroll.is_some() {
            extra.push(
                serenity::CreateButton::new(BTN_REROLL)
                    .label(i18n::BTN_REROLL)
                    .emoji('🎲')
                    .style(serenity::ButtonStyle::Primary)
                    .disabled(!enabled),
            );
        }
        if !self.list.is_empty() {
            extra.push(back_button(enabled));
        }
        if !extra.is_empty() {
            rows.push(buttons(extra));
        }
        Page::embed(
            card::work_embed(w, self.tmdb.region(), self.spoiler).build(),
            rows,
        )
        .notice(self.notice.clone())
    }

    fn list_page(&self, enabled: bool) -> Page {
        let body = self
            .list
            .iter()
            .enumerate()
            .map(|(i, w)| {
                format!(
                    "**{}.** {}{}",
                    i + 1,
                    w.title,
                    w.year.map(|y| format!(" ({y})")).unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let choices = self
            .list
            .iter()
            .map(|w| Choice {
                label: fit(&w.title, 100),
                description: w.year.map(|y| y.to_string()),
                value: w.id.to_string(),
            })
            .collect();
        let mut rows: Vec<_> = select_row(PICK_ID, i18n::PICK_PLACEHOLDER, choices, enabled)
            .into_iter()
            .collect();
        rows.push(buttons(vec![close_button(enabled)]));
        let mut e = Emb::new().title(i18n::PICK_PROMPT).description(body);
        e.footer = Some(tmdb::FOOTER_CREDIT.to_string());
        Page::embed(e.build(), rows)
    }

    async fn roll(&mut self) -> Outcome {
        let Some(r) = self.reroll.clone() else {
            return Outcome::Keep;
        };
        if self.cast.is_some() {
            match find::random_cast(&self.tmdb, r.cat, &r.filter, &r.overrides, self.user).await {
                Some((w, i)) => {
                    self.notice = None;
                    self.card = Some(Box::new(w));
                    self.cast = Some(i);
                }
                None => self.notice = Some(i18n::RANDOM_FAILED.to_string()),
            }
            return Outcome::Update;
        }
        match find::random(&self.tmdb, r.cat, &r.filter, &r.overrides, self.user).await {
            Some(w) => {
                self.notice = None;
                self.show(w).await;
            }
            None => self.notice = Some(i18n::RANDOM_FAILED.to_string()),
        }
        Outcome::Update
    }
}

impl View for TmdbView {
    fn render(&self, enabled: bool) -> Page {
        match &self.card {
            Some(w) => self.card_page(w, enabled),
            None => self.list_page(enabled),
        }
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        match id {
            CLOSE => Outcome::Close,
            BTN_REROLL => self.roll().await,
            BACK => {
                self.card = None;
                self.cast = None;
                Outcome::Update
            }
            PICK_ID => {
                let Some(picked) = values
                    .first()
                    .and_then(|v| v.parse::<i64>().ok())
                    .and_then(|id| self.list.iter().find(|w| w.id == id).cloned())
                else {
                    return Outcome::Keep;
                };
                let kind = picked.kind.unwrap_or(tmdb::model::Kind::Tv);
                match self.tmdb.details(kind, picked.id).await {
                    Some(full) => {
                        self.show(full).await;
                        Outcome::Update
                    }
                    None => Outcome::Whisper(i18n::SHOW_WORK_UNAVAILABLE.to_string()),
                }
            }
            _ => Outcome::Keep,
        }
    }
}

/// Show search results: the card for one hit, a select menu for several.
pub async fn show_results(
    ctx: Context<'_>,
    spoiler: bool,
    mut hits: Vec<Work>,
) -> Result<(), Error> {
    if hits.is_empty() {
        return say_ephemeral(ctx, i18n::NO_RESULTS).await;
    }
    let tmdb = ctx.data().tmdb.clone();
    if hits.len() == 1 {
        let w = hits.remove(0);
        // A list entry has no cast or providers: load the details (a `find::search` result may already have them).
        let full = if w.cast.is_empty() && w.providers.is_empty() {
            tmdb.details(w.kind.unwrap_or(tmdb::model::Kind::Tv), w.id)
                .await
                .unwrap_or(w)
        } else {
            w
        };
        return session::run(ctx, TmdbView::new(ctx, spoiler).with_card(full).await).await;
    }
    session::run(ctx, TmdbView::new(ctx, spoiler).with_list(hits)).await
}
