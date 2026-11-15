//! `/profile reorder`: select a character, then move it with buttons.

use super::util::{begin_private, pool, require_own, uid};
use crate::{
    commands::buttons,
    db::profile::{self as db, Entry},
    error::{Context, Error},
    i18n,
    ui::{
        embeds::Emb,
        pagination::{apply_nav, nav_row, page_count, parse_nav, CLOSE},
        select::{Choice, PICK_ID},
        session::{self, Outcome, Page, View},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;

const PER_PAGE: usize = 10;

struct ReorderView {
    pool: SqlitePool,
    user: i64,
    page: usize,
    total: i64,
    selected: Option<i64>,
    entries: Vec<Entry>,
    notice: Option<String>,
}

fn label(e: &Entry) -> String {
    if e.visible() {
        e.name().to_string()
    } else {
        "—".to_string()
    }
}

impl ReorderView {
    async fn load(&mut self) -> Result<(), db::ProfileError> {
        self.total = db::count(&self.pool, self.user).await?;
        let pages = page_count(self.total as usize, PER_PAGE);
        self.page = self.page.min(pages - 1);
        self.entries = db::list_page(
            &self.pool,
            self.user,
            None,
            (self.page * PER_PAGE) as i64,
            PER_PAGE as i64,
        )
        .await?;
        Ok(())
    }

    async fn move_selected(&mut self, target: impl FnOnce(i64, i64) -> i64) -> Outcome {
        let Some(id) = self.selected else {
            self.notice = Some(i18n::REORDER_HINT.to_string());
            return Outcome::Update;
        };
        let result = async {
            let pos = db::position_of(&self.pool, self.user, id)
                .await?
                .unwrap_or(1);
            let new =
                db::move_character(&self.pool, self.user, id, target(pos, self.total)).await?;
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
            .entries
            .iter()
            .map(|e| {
                let mark = if Some(e.character_id) == self.selected {
                    "▶ "
                } else {
                    ""
                };
                format!("{mark}**{}.** {}", e.position, label(e))
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
            .entries
            .iter()
            .map(|e| Choice {
                label: format!("{}. {}", e.position, label(e)),
                description: None,
                value: e.character_id.to_string(),
            })
            .collect();
        let btn = |id: &str, text: &str| {
            serenity::CreateButton::new(id)
                .label(text)
                .style(serenity::ButtonStyle::Secondary)
                .disabled(!enabled || self.selected.is_none())
        };
        let mut rows: Vec<_> =
            crate::ui::select::select_row(PICK_ID, i18n::REORDER_SELECT, choices, enabled)
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
                self.selected = values.first().and_then(|v| v.parse().ok());
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

/// Reorder your characters interactively.
#[poise::command(
    slash_command,
    name_localized("ar", "ترتيب"),
    description_localized("ar", "رتّب شخصياتك بالأزرار")
)]
pub async fn reorder(ctx: Context<'_>) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let mut view = ReorderView {
        pool: pool(ctx),
        user: uid(ctx),
        page: 0,
        total: 0,
        selected: None,
        entries: Vec::new(),
        notice: None,
    };
    view.load().await?;
    session::run(ctx, view).await
}
