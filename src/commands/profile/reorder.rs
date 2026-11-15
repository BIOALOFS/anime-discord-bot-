//! Interactive reordering of characters or works: select an item, then move it with buttons.

use super::{
    items::{self, Item, Section},
    util::{begin_private, pool, require_own, uid},
};
use crate::{
    commands::buttons,
    db::{profile as pdb, works},
    error::{Context, Error},
    i18n,
    ui::{
        embeds::Emb,
        pagination::{apply_nav, nav_row, page_count, parse_nav, CLOSE},
        select::{select_row, Choice, PICK_ID},
        session::{self, Outcome, Page, View},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;

const PER_PAGE: usize = 10;

struct ReorderView {
    pool: SqlitePool,
    user: i64,
    section: Section,
    page: usize,
    total: i64,
    selected: Option<String>,
    items: Vec<Item>,
    notice: Option<String>,
}

fn label(i: &Item) -> String {
    if i.visible {
        i.name.clone()
    } else {
        "—".to_string()
    }
}

impl ReorderView {
    async fn load(&mut self) -> Result<(), pdb::ProfileError> {
        self.total = items::count(&self.pool, self.user, self.section, None, None).await?;
        let pages = page_count(self.total as usize, PER_PAGE);
        self.page = self.page.min(pages - 1);
        self.items = items::load_page(
            &self.pool,
            self.user,
            self.section,
            None,
            None,
            (self.page * PER_PAGE) as i64,
            PER_PAGE as i64,
        )
        .await?;
        Ok(())
    }

    async fn position_of(&self, key: &str) -> Result<Option<i64>, pdb::ProfileError> {
        match self.section {
            Section::Characters => match key.parse::<i64>() {
                Ok(id) => pdb::position_of(&self.pool, self.user, id).await,
                Err(_) => Ok(None),
            },
            Section::Works => works::position_of(&self.pool, self.user, key).await,
        }
    }

    async fn move_to(&self, key: &str, target: i64) -> Result<i64, pdb::ProfileError> {
        match self.section {
            Section::Characters => {
                let id = key
                    .parse::<i64>()
                    .map_err(|_| pdb::ProfileError::NotFound)?;
                pdb::move_character(&self.pool, self.user, id, target).await
            }
            Section::Works => works::move_work(&self.pool, self.user, key, target).await,
        }
    }

    async fn move_selected(&mut self, target: impl FnOnce(i64, i64) -> i64) -> Outcome {
        let Some(key) = self.selected.clone() else {
            self.notice = Some(i18n::REORDER_HINT.to_string());
            return Outcome::Update;
        };
        let result = async {
            let pos = self.position_of(&key).await?.unwrap_or(1);
            let new = self.move_to(&key, target(pos, self.total)).await?;
            self.page = ((new - 1).max(0) as usize) / PER_PAGE;
            self.load().await
        }
        .await;
        if let Err(e) = result {
            tracing::warn!("reorder failed: {e}");
            self.notice = Some(i18n::ERR_GENERIC.to_string());
        }
        Outcome::Update
    }
}

impl View for ReorderView {
    fn render(&self, enabled: bool) -> Page {
        let lines: Vec<String> = self
            .items
            .iter()
            .map(|i| {
                let mark = if Some(&i.key) == self.selected.as_ref() {
                    "▶ "
                } else {
                    ""
                };
                format!("{mark}**{}.** {}", i.position, label(i))
            })
            .collect();
        let body = if lines.is_empty() {
            i18n::PROFILE_EMPTY.to_string()
        } else {
            lines.join("\n")
        };
        let pages = page_count(self.total as usize, PER_PAGE);
        let embed = Emb::new()
            .title(i18n::REORDER_TITLE)
            .description(format!("{}\n\n{body}", i18n::REORDER_HINT))
            .footer(format!(
                "{} · {}/{}",
                i18n::POWERED_BY,
                self.page + 1,
                pages
            ));

        let choices: Vec<Choice> = self
            .items
            .iter()
            .map(|i| Choice {
                label: format!("{}. {}", i.position, label(i)),
                description: None,
                value: i.key.clone(),
            })
            .collect();
        let btn = |id: &str, text: &str| {
            serenity::CreateButton::new(id)
                .label(text)
                .style(serenity::ButtonStyle::Secondary)
                .disabled(!enabled || self.selected.is_none())
        };
        let mut rows: Vec<_> = select_row(PICK_ID, i18n::REORDER_SELECT, choices, enabled)
            .into_iter()
            .collect();
        rows.push(buttons(vec![
            btn("mv_top", "⏫"),
            btn("mv_up", "⬆"),
            btn("mv_down", "⬇"),
            btn("mv_bottom", "⏬"),
            crate::ui::pagination::close_button(enabled),
        ]));
        rows.push(nav_row(self.page, pages, enabled));
        Page::embed(embed.build(), rows).notice(self.notice.clone())
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        self.notice = None;
        match id {
            CLOSE => Outcome::Close,
            PICK_ID => {
                self.selected = values.first().cloned();
                Outcome::Update
            }
            "mv_top" => self.move_selected(|_, _| 1).await,
            "mv_up" => self.move_selected(|pos, _| pos - 1).await,
            "mv_down" => self.move_selected(|pos, _| pos + 1).await,
            "mv_bottom" => self.move_selected(|_, total| total).await,
            other => {
                let Some(nav) = parse_nav(other) else {
                    return Outcome::Keep;
                };
                let pages = page_count(self.total as usize, PER_PAGE);
                let next = apply_nav(nav, self.page, pages);
                if next == self.page {
                    return Outcome::Keep;
                }
                self.page = next;
                if let Err(e) = self.load().await {
                    tracing::warn!("reorder page load failed: {e}");
                    self.notice = Some(i18n::ERR_GENERIC.to_string());
                }
                Outcome::Update
            }
        }
    }
}

/// Reorder your characters or works interactively.
pub(super) async fn reorder(ctx: Context<'_>, section: Section) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let mut view = ReorderView {
        pool: pool(ctx),
        user: uid(ctx),
        section,
        page: 0,
        total: 0,
        selected: None,
        items: Vec::new(),
        notice: None,
    };
    view.load().await?;
    session::run(ctx, view).await
}
