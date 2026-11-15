//! `/top kind scope type`: most favourited characters or works, per server or global.

use super::buttons;
use crate::{
    anilist::MediaKind,
    db::{profile as pdb, works},
    error::{Context, Error},
    i18n,
    ui::{
        embeds::Emb,
        pagination::{apply_nav, close_button, nav_row, page_count, parse_nav, CLOSE},
        session::{self, Outcome, Page, View},
    },
};
use sqlx::SqlitePool;

const PER_PAGE: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum KindChoice {
    #[name = "الشخصيات"]
    #[name_localized("en-US", "Characters")]
    Characters,
    #[name = "الأعمال"]
    #[name_localized("en-US", "Works")]
    Works,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum ScopeChoice {
    #[name = "السيرفر"]
    #[name_localized("en-US", "Server")]
    Server,
    #[name = "عالميًا"]
    #[name_localized("en-US", "Global")]
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum TypeChoice {
    #[name = "الكل"]
    #[name_localized("en-US", "All")]
    All,
    #[name = "أنمي"]
    #[name_localized("en-US", "Anime")]
    Anime,
    #[name = "مانجا"]
    #[name_localized("en-US", "Manga")]
    Manga,
    #[name = "مانهوا"]
    #[name_localized("en-US", "Manhwa")]
    Manhwa,
    #[name = "مانهوا صينية"]
    #[name_localized("en-US", "Manhua")]
    Manhua,
    #[name = "رواية"]
    #[name_localized("en-US", "Novel")]
    Novel,
}

impl TypeChoice {
    fn key(self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Anime => Some(MediaKind::Anime.key()),
            Self::Manga => Some(MediaKind::Manga.key()),
            Self::Manhwa => Some(MediaKind::Manhwa.key()),
            Self::Manhua => Some(MediaKind::Manhua.key()),
            Self::Novel => Some(MediaKind::Novel.key()),
        }
    }
}

struct TopView {
    pool: SqlitePool,
    kind: KindChoice,
    guild: Option<i64>,
    work_kind: Option<&'static str>,
    page: usize,
    total: i64,
    /// `(count, name)`; only verified-safe entries are ever listed.
    rows: Vec<(i64, String)>,
}

impl TopView {
    async fn load(&mut self) -> Result<(), pdb::ProfileError> {
        self.rows.clear();
        match self.kind {
            KindChoice::Characters => {
                self.total = pdb::top_character_count(&self.pool, self.guild).await?;
                self.page = self.page.min(page_count(self.total as usize, PER_PAGE) - 1);
                for (id, n) in pdb::top_characters(
                    &self.pool,
                    self.guild,
                    (self.page * PER_PAGE) as i64,
                    PER_PAGE as i64,
                )
                .await?
                {
                    if let Some(c) = pdb::cache_get(&self.pool, id).await?.filter(|c| c.is_safe) {
                        self.rows.push((n, c.name_romaji));
                    }
                }
            }
            KindChoice::Works => {
                self.total = works::top_work_count(&self.pool, self.guild, self.work_kind).await?;
                self.page = self.page.min(page_count(self.total as usize, PER_PAGE) - 1);
                for (id, n) in works::top_works(
                    &self.pool,
                    self.guild,
                    self.work_kind,
                    (self.page * PER_PAGE) as i64,
                    PER_PAGE as i64,
                )
                .await?
                {
                    if let Some(w) = works::media_cache_get(&self.pool, &id)
                        .await?
                        .filter(|w| w.is_safe)
                    {
                        self.rows.push((n, w.title_romaji));
                    }
                }
            }
        }
        Ok(())
    }
}

impl View for TopView {
    fn render(&self, enabled: bool) -> Page {
        let pages = page_count(self.total as usize, PER_PAGE);
        let body = if self.rows.is_empty() {
            i18n::EMPTY_LIST.to_string()
        } else {
            self.rows
                .iter()
                .enumerate()
                .map(|(i, (n, name))| {
                    format!("**{}.** {name} — ❤ {n}", self.page * PER_PAGE + i + 1)
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let scope = if self.guild.is_some() {
            i18n::SCOPE_SERVER
        } else {
            i18n::SCOPE_GLOBAL
        };
        let what = match self.kind {
            KindChoice::Characters => i18n::TOP_CHARACTERS,
            KindChoice::Works => i18n::TOP_WORKS,
        };
        let embed = Emb::new()
            .title(format!("{what} — {scope}"))
            .description(body)
            .footer(format!(
                "{} · {}/{}",
                i18n::POWERED_BY,
                self.page + 1,
                pages
            ))
            .build();
        Page::embed(
            embed,
            vec![
                nav_row(self.page, pages, enabled),
                buttons(vec![close_button(enabled)]),
            ],
        )
    }

    async fn on_interaction(&mut self, id: &str, _values: &[String]) -> Outcome {
        if id == CLOSE {
            return Outcome::Close;
        }
        let Some(nav) = parse_nav(id) else {
            return Outcome::Keep;
        };
        let next = apply_nav(nav, self.page, page_count(self.total as usize, PER_PAGE));
        if next == self.page {
            return Outcome::Keep;
        }
        self.page = next;
        if let Err(e) = self.load().await {
            tracing::warn!("top reload failed: {e}");
        }
        Outcome::Update
    }
}

/// Leaderboards of favourite characters and works.
#[poise::command(
    slash_command,
    name_localized("ar", "الأفضل"),
    description_localized("ar", "أكثر الشخصيات أو الأعمال تفضيلًا")
)]
pub async fn top(
    ctx: Context<'_>,
    #[description = "Characters or works"]
    #[description_localized("ar", "شخصيات أم أعمال")]
    #[name_localized("ar", "النوع")]
    kind: KindChoice,
    #[description = "Server or global (default: server)"]
    #[description_localized("ar", "السيرفر أم عالميًا (الافتراضي: السيرفر)")]
    #[name_localized("ar", "النطاق")]
    scope: Option<ScopeChoice>,
    #[description = "Kind of work (works only)"]
    #[description_localized("ar", "نوع العمل (للأعمال فقط)")]
    #[name_localized("ar", "الفئة")]
    work_type: Option<TypeChoice>,
) -> Result<(), Error> {
    super::profile::begin_public_for_top(ctx).await?;
    let guild = match (scope.unwrap_or(ScopeChoice::Server), ctx.guild_id()) {
        (ScopeChoice::Server, Some(g)) => Some(g.get() as i64),
        _ => None,
    };
    let mut view = TopView {
        pool: ctx.data().db.clone(),
        kind,
        guild,
        work_kind: work_type.and_then(TypeChoice::key),
        page: 0,
        total: 0,
        rows: Vec::new(),
    };
    view.load().await?;
    session::run(ctx, view).await
}
