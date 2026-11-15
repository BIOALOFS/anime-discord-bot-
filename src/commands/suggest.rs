//! `/suggest`: members connect two works ("if you liked A, try B"). Any provider and type can be mixed. `add`
//! stores a pair with a reason, `random` shows one from this server, `mine` lists and deletes your own.

use super::{buttons, fail, fallback::game_flags, media::media_label_for_suggest, say_ephemeral};
use crate::{
    anilist::ContentPolicy,
    db::{
        admin as adb,
        custom::sanitize_text,
        suggest::{self, AddResult, NewSuggestion, Suggestion, MAX_REASON},
    },
    error::{Context, Error},
    guild, i18n,
    providers::Work,
    ui::{
        autocomplete::to_choices,
        embeds::{fit, Emb},
        pagination::{apply_nav, close_button, nav_row, page_count, parse_nav, CLOSE},
        select::{normalize_choices, select_row, Choice},
        session::{self, Outcome, Page, View},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;

const PAGE: usize = 10;
const ANOTHER: &str = "suggest_another";
const DELETE: &str = "suggest_delete";
const MINE_PICK: &str = "suggest_mine_pick";

/// A resolved work: provider-prefixed id, title and page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picked {
    pub id: String,
    pub title: String,
    pub url: Option<String>,
}

impl Picked {
    fn from_work(w: &Work) -> Self {
        Self {
            id: w.id.clone(),
            title: w.title.clone(),
            url: w.url.clone(),
        }
    }
}

/// The reason, cleaned: no links, mentions or explicit words, at most 300 characters. `None` when nothing is left.
pub fn clean_reason(text: &str) -> Option<String> {
    sanitize_text(text, MAX_REASON)
}

/// Only https links become buttons.
fn link_ok(url: &Option<String>) -> Option<&str> {
    url.as_deref()
        .filter(|u| u.starts_with("https://") && u.len() <= 512)
}

/// The embed of one suggestion; the author is shown as a mention (embeds never ping).
pub fn suggestion_embed(s: &Suggestion) -> Emb {
    let mut e = Emb::new()
        .title(fit(
            &format!("🔗 {} ↔ {}", s.first_title, s.second_title),
            250,
        ))
        .description(format!(
            "{}\n\n{} <@{}>",
            fit(&s.reason, MAX_REASON),
            i18n::SUGGESTED_BY,
            s.author_id
        ));
    e.footer = None;
    e
}

/// Options for deleting one of my suggestions.
pub fn mine_choices(list: &[Suggestion]) -> Vec<Choice> {
    normalize_choices(
        list.iter()
            .map(|s| Choice {
                label: fit(&format!("{} ↔ {}", s.first_title, s.second_title), 100),
                description: Some(fit(&s.reason, 100)),
                value: s.id.to_string(),
            })
            .collect(),
    )
}

// ---- autocomplete and resolving -------------------------------------------------------------

/// Works of every provider and type, with provider-prefixed values.
async fn ac_work(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    let (games_on, mature) = game_flags(ctx).await;
    let data = ctx.data();
    let (api, providers) = (data.anilist.clone(), data.providers.clone());
    let items = data
        .auto
        .items(ctx.author().id.get(), "suggest", partial, |q| async move {
            let (ani, games, vns) = futures::join!(
                async {
                    api.search_any(&q, 8, ContentPolicy::SAFE)
                        .await
                        .unwrap_or_default()
                },
                async {
                    if games_on {
                        providers.search_games(&q, mature).await
                    } else {
                        Vec::new()
                    }
                },
                async {
                    if games_on {
                        providers.search_visual_novels(&q).await
                    } else {
                        Vec::new()
                    }
                },
            );
            let mut out: Vec<(String, String)> = ani
                .iter()
                .map(|m| (media_label_for_suggest(m), format!("anilist:{}", m.id)))
                .collect();
            out.extend(
                games
                    .iter()
                    .chain(vns.iter())
                    .take(6)
                    .map(|w| (provider_label(w), w.id.clone())),
            );
            out
        })
        .await;
    to_choices(items)
}

fn provider_label(w: &Work) -> String {
    let kind = match w.kind {
        crate::providers::Kind::VisualNovel => i18n::KIND_VN,
        _ => i18n::KIND_GAME,
    };
    match w.year {
        Some(y) => format!("{} ({kind} · {y})", w.title),
        None => format!("{} ({kind})", w.title),
    }
}

/// `anilist:5` / `id:5`, `igdb:5`, `vndb:v5` or plain text. Replies and returns `None` when it cannot be resolved.
async fn resolve(ctx: Context<'_>, raw: &str) -> Result<Option<Picked>, Error> {
    let raw = raw.trim();
    let data = ctx.data();
    let (games_on, mature) = game_flags(ctx).await;
    let ani_id = raw
        .strip_prefix("anilist:")
        .or_else(|| raw.strip_prefix("id:"))
        .and_then(|v| v.parse::<i64>().ok());
    if let Some(id) = ani_id {
        return match data.anilist.media_by_id(id, ContentPolicy::SAFE).await {
            Ok(Some(m)) => Ok(Some(Picked {
                id: format!("anilist:{}", m.id),
                title: m.title.main().to_string(),
                url: m.site_url.clone(),
            })),
            Ok(None) => {
                say_ephemeral(ctx, i18n::NOT_FOUND).await?;
                Ok(None)
            }
            Err(e) => {
                fail(ctx, e).await?;
                Ok(None)
            }
        };
    }
    if raw.starts_with("igdb:") || raw.starts_with("vndb:") {
        if !games_on {
            say_ephemeral(ctx, i18n::GAMES_SERVER_OFF).await?;
            return Ok(None);
        }
        return match data.providers.work_by_key(raw, mature).await {
            Some(w) => Ok(Some(Picked::from_work(&w))),
            None => {
                say_ephemeral(ctx, i18n::NOT_FOUND).await?;
                Ok(None)
            }
        };
    }
    // Free text: the best AniList match, then games and visual novels.
    match data.anilist.search_any(raw, 1, ContentPolicy::SAFE).await {
        Ok(mut list) if !list.is_empty() => {
            let m = list.remove(0);
            return Ok(Some(Picked {
                id: format!("anilist:{}", m.id),
                title: m.title.main().to_string(),
                url: m.site_url.clone(),
            }));
        }
        Ok(_) => {}
        Err(e) if !games_on => {
            fail(ctx, e).await?;
            return Ok(None);
        }
        Err(_) => {}
    }
    if games_on {
        let mut found = data.providers.search_games(raw, mature).await;
        found.extend(data.providers.search_visual_novels(raw).await);
        if let Some(w) = found.first() {
            return Ok(Some(Picked::from_work(w)));
        }
    }
    say_ephemeral(ctx, i18n::NO_RESULTS).await?;
    Ok(None)
}

// ---- commands ------------------------------------------------------------------------------

/// Member suggestions: connect two works.
#[poise::command(
    slash_command,
    guild_only,
    subcommands("add", "random", "mine"),
    name_localized("ar", "اقترح"),
    description_localized("ar", "اقتراحات الأعضاء: اربط بين عملين")
)]
pub async fn suggest(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Suggest that people who like one work try another.
#[poise::command(
    slash_command,
    guild_only,
    name_localized("ar", "إضافة"),
    description_localized("ar", "اقترح عملين مرتبطين مع السبب")
)]
pub async fn add(
    ctx: Context<'_>,
    #[description = "The first work (any provider or type)"]
    #[description_localized("ar", "العمل الأول (أي نوع)")]
    #[name_localized("ar", "الأول")]
    #[autocomplete = "ac_work"]
    first: String,
    #[description = "The second work"]
    #[description_localized("ar", "العمل الثاني")]
    #[name_localized("ar", "الثاني")]
    #[autocomplete = "ac_work"]
    second: String,
    #[description = "What connects them (up to 300 characters)"]
    #[description_localized("ar", "ما الذي يربط بينهما (حتى 300 حرف)")]
    #[name_localized("ar", "السبب")]
    reason: String,
) -> Result<(), Error> {
    ctx.defer().await?;
    let Some(guild_id) = ctx.guild_id().map(|g| g.get() as i64) else {
        return say_ephemeral(ctx, i18n::SERVER_ONLY).await;
    };
    if reason.chars().count() > MAX_REASON {
        return say_ephemeral(ctx, i18n::SUGGEST_REASON_LONG).await;
    }
    let Some(reason) = clean_reason(&reason) else {
        return say_ephemeral(ctx, i18n::SUGGEST_REASON_BAD).await;
    };
    let Some(a) = resolve(ctx, &first).await? else {
        return Ok(());
    };
    let Some(b) = resolve(ctx, &second).await? else {
        return Ok(());
    };
    let pool = &ctx.data().db;
    let limit = adb::suggest_limit(pool, guild_id).await?;
    let new = NewSuggestion {
        guild_id,
        author_id: ctx.author().id.get() as i64,
        first_id: a.id,
        second_id: b.id,
        first_title: a.title,
        second_title: b.title,
        first_url: a.url,
        second_url: b.url,
        reason,
    };
    match suggest::add(pool, &new, limit).await? {
        AddResult::Added(id) => {
            let s = suggest::get(pool, guild_id, id).await?;
            let mut embed = s.as_ref().map(suggestion_embed).unwrap_or_default();
            embed.footer = None;
            ctx.send(
                poise::CreateReply::default()
                    .content(i18n::SUGGEST_ADDED)
                    .embed(embed.build()),
            )
            .await?;
            Ok(())
        }
        AddResult::Duplicate { author } => say_ephemeral(ctx, &i18n::suggest_exists(author)).await,
        AddResult::LimitReached => say_ephemeral(ctx, &i18n::suggest_limit(limit)).await,
        AddResult::Same => say_ephemeral(ctx, i18n::SUGGEST_SAME).await,
    }
}

/// A random suggestion from this server.
#[poise::command(
    slash_command,
    guild_only,
    name_localized("ar", "عشوائي"),
    description_localized("ar", "اقتراح عشوائي من هذا السيرفر")
)]
pub async fn random(
    ctx: Context<'_>,
    #[description = "Only suggestions that involve this work"]
    #[description_localized("ar", "اقتراحات تخص هذا العمل فقط")]
    #[name_localized("ar", "العمل")]
    #[autocomplete = "ac_work"]
    work: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let Some(guild_id) = ctx.guild_id().map(|g| g.get() as i64) else {
        return say_ephemeral(ctx, i18n::SERVER_ONLY).await;
    };
    let filter = match work.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
        Some(w) => resolve(ctx, w)
            .await?
            .map(|p| p.id)
            .or_else(|| Some(String::new())),
        None => None,
    };
    // An unresolvable filter already got its reply.
    if filter.as_deref() == Some("") {
        return Ok(());
    }
    let admin = guild::can_manage(
        ctx.author_member().await.and_then(|m| m.permissions),
        &ctx.author_member()
            .await
            .map(|m| m.roles.iter().map(|r| r.get() as i64).collect::<Vec<_>>())
            .unwrap_or_default(),
        adb::manager_role(&ctx.data().db, guild_id)
            .await
            .unwrap_or(None),
    );
    let mut view = RandomView {
        pool: ctx.data().db.clone(),
        guild: guild_id,
        user: ctx.author().id.get() as i64,
        admin,
        filter,
        current: None,
        notice: None,
    };
    view.roll().await?;
    if view.current.is_none() {
        return say_ephemeral(ctx, i18n::SUGGEST_NONE).await;
    }
    session::run(ctx, view).await
}

/// Your suggestions, with a menu to delete one.
#[poise::command(
    slash_command,
    guild_only,
    name_localized("ar", "اقتراحاتي"),
    description_localized("ar", "اقتراحاتك مع إمكانية الحذف")
)]
pub async fn mine(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let Some(guild_id) = ctx.guild_id().map(|g| g.get() as i64) else {
        return say_ephemeral(ctx, i18n::SERVER_ONLY).await;
    };
    let mut view = MineView {
        pool: ctx.data().db.clone(),
        guild: guild_id,
        user: ctx.author().id.get() as i64,
        page: 0,
        total: 0,
        rows: Vec::new(),
        notice: None,
    };
    view.load().await?;
    session::run(ctx, view).await
}

// ---- views ---------------------------------------------------------------------------------

struct RandomView {
    pool: SqlitePool,
    guild: i64,
    user: i64,
    admin: bool,
    filter: Option<String>,
    current: Option<Suggestion>,
    notice: Option<String>,
}

impl RandomView {
    async fn roll(&mut self) -> Result<(), sqlx::Error> {
        let previous = self.current.as_ref().map(|s| s.id);
        let mut next = suggest::random(&self.pool, self.guild, self.filter.as_deref()).await?;
        // Show something different when there is more than one.
        if next.as_ref().map(|s| s.id) == previous {
            next = suggest::random(&self.pool, self.guild, self.filter.as_deref())
                .await?
                .or(next);
        }
        self.current = next;
        Ok(())
    }
}

impl View for RandomView {
    fn render(&self, enabled: bool) -> Page {
        let Some(s) = &self.current else {
            return Page::embed(
                Emb::new().description(i18n::SUGGEST_NONE).build(),
                vec![buttons(vec![close_button(enabled)])],
            );
        };
        let mut row = vec![serenity::CreateButton::new(ANOTHER)
            .label(i18n::SUGGEST_ANOTHER)
            .emoji('🔀')
            .style(serenity::ButtonStyle::Primary)
            .disabled(!enabled)];
        if let Some(u) = link_ok(&s.first_url) {
            row.push(
                serenity::CreateButton::new_link(u)
                    .label(format!("🔗 {}", fit(&s.first_title, 60))),
            );
        }
        if let Some(u) = link_ok(&s.second_url) {
            row.push(
                serenity::CreateButton::new_link(u)
                    .label(format!("🔗 {}", fit(&s.second_title, 60))),
            );
        }
        if s.author_id == self.user || self.admin {
            row.push(
                serenity::CreateButton::new(DELETE)
                    .label(i18n::BTN_DELETE)
                    .emoji('🗑')
                    .style(serenity::ButtonStyle::Danger)
                    .disabled(!enabled),
            );
        }
        let mut rows = vec![buttons(row)];
        rows.push(buttons(vec![close_button(enabled)]));
        Page::embed(suggestion_embed(s).build(), rows).notice(self.notice.clone())
    }

    async fn on_interaction(&mut self, id: &str, _values: &[String]) -> Outcome {
        self.notice = None;
        match id {
            CLOSE => Outcome::Close,
            ANOTHER => {
                if let Err(e) = self.roll().await {
                    tracing::warn!("suggest roll failed: {e}");
                    self.notice = Some(i18n::ERR_GENERIC.to_string());
                }
                Outcome::Update
            }
            DELETE => {
                let Some(s) = self.current.clone() else {
                    return Outcome::Keep;
                };
                self.notice = Some(
                    match suggest::delete(&self.pool, self.guild, s.id, self.user, self.admin).await
                    {
                        Ok(true) => i18n::SUGGEST_DELETED.to_string(),
                        Ok(false) => i18n::SUGGEST_CANNOT_DELETE.to_string(),
                        Err(e) => {
                            tracing::warn!("suggest delete failed: {e}");
                            i18n::ERR_GENERIC.to_string()
                        }
                    },
                );
                if let Err(e) = self.roll().await {
                    tracing::warn!("suggest roll failed: {e}");
                }
                // The deleted one must not stay on screen.
                if self.current.as_ref().map(|c| c.id) == Some(s.id) {
                    self.current = None;
                }
                Outcome::Update
            }
            _ => Outcome::Keep,
        }
    }
}

struct MineView {
    pool: SqlitePool,
    guild: i64,
    user: i64,
    page: usize,
    total: i64,
    rows: Vec<Suggestion>,
    notice: Option<String>,
}

impl MineView {
    async fn load(&mut self) -> Result<(), sqlx::Error> {
        self.total = suggest::count_by_author(&self.pool, self.guild, self.user).await?;
        self.page = self.page.min(page_count(self.total as usize, PAGE) - 1);
        self.rows = suggest::by_author(
            &self.pool,
            self.guild,
            self.user,
            (self.page * PAGE) as i64,
            PAGE as i64,
        )
        .await?;
        Ok(())
    }
}

impl View for MineView {
    fn ephemeral(&self) -> bool {
        true
    }

    fn render(&self, enabled: bool) -> Page {
        let pages = page_count(self.total as usize, PAGE);
        let body = if self.rows.is_empty() {
            i18n::SUGGEST_NONE_MINE.to_string()
        } else {
            self.rows
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    format!(
                        "**{}.** {} ↔ {} — {}",
                        self.page * PAGE + i + 1,
                        s.first_title,
                        s.second_title,
                        fit(&s.reason, 60)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut embed = Emb::new().title(i18n::SUGGEST_MINE_TITLE).description(body);
        embed.footer = None;
        let mut rows: Vec<_> = select_row(
            MINE_PICK,
            i18n::SUGGEST_DELETE_PLACEHOLDER,
            mine_choices(&self.rows),
            enabled,
        )
        .into_iter()
        .collect();
        if pages > 1 {
            rows.push(nav_row(self.page, pages, enabled));
        }
        rows.push(buttons(vec![close_button(enabled)]));
        Page::embed(embed.build(), rows).notice(self.notice.clone())
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        self.notice = None;
        match id {
            CLOSE => return Outcome::Close,
            MINE_PICK => {
                let Some(sid) = values
                    .first()
                    .and_then(|v| v.parse::<i64>().ok())
                    .filter(|i| self.rows.iter().any(|s| s.id == *i))
                else {
                    return Outcome::Keep;
                };
                self.notice = Some(
                    match suggest::delete(&self.pool, self.guild, sid, self.user, false).await {
                        Ok(true) => i18n::SUGGEST_DELETED.to_string(),
                        Ok(false) => i18n::SUGGEST_CANNOT_DELETE.to_string(),
                        Err(e) => {
                            tracing::warn!("suggest delete failed: {e}");
                            i18n::ERR_GENERIC.to_string()
                        }
                    },
                );
            }
            other => {
                let Some(nav) = parse_nav(other) else {
                    return Outcome::Keep;
                };
                let next = apply_nav(nav, self.page, page_count(self.total as usize, PAGE));
                if next == self.page {
                    return Outcome::Keep;
                }
                self.page = next;
            }
        }
        if let Err(e) = self.load().await {
            tracing::warn!("suggest list reload failed: {e}");
            self.notice = Some(i18n::ERR_GENERIC.to_string());
        }
        Outcome::Update
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sug(id: i64, reason: &str) -> Suggestion {
        Suggestion {
            id,
            guild_id: 1,
            author_id: 7,
            first_id: "anilist:1".into(),
            second_id: "igdb:2".into(),
            first_title: "Anime".into(),
            second_title: "Game".into(),
            first_url: Some("https://anilist.co/anime/1".into()),
            second_url: Some("http://insecure.example".into()),
            reason: reason.into(),
            created_at: 0,
        }
    }

    #[test]
    fn reasons_lose_links_and_mass_mentions_and_are_capped() {
        assert_eq!(
            clean_reason("same vibe https://evil.example @everyone <@123> great"),
            Some("same vibe great".to_string())
        );
        assert_eq!(clean_reason("   "), None);
        assert_eq!(clean_reason("https://only.a.link"), None);
        assert_eq!(
            clean_reason(&"x".repeat(500)).map(|r| r.chars().count()),
            Some(MAX_REASON)
        );
        assert_eq!(
            clean_reason("a sexual scene"),
            None,
            "explicit text is refused"
        );
    }

    #[test]
    fn the_embed_mentions_the_author_and_both_titles() {
        let e = suggestion_embed(&sug(1, "similar tone"));
        assert!(
            e.title.as_deref().unwrap_or("").contains("Anime")
                && e.title.as_deref().unwrap_or("").contains("Game")
        );
        let d = e.description.as_deref().unwrap_or("");
        assert!(d.contains("similar tone") && d.contains("<@7>"));
    }

    #[test]
    fn only_https_links_become_buttons() {
        let s = sug(1, "r");
        assert!(link_ok(&s.first_url).is_some());
        assert!(link_ok(&s.second_url).is_none());
        assert!(link_ok(&None).is_none());
    }

    #[test]
    fn delete_options_are_unique_short_and_capped() {
        let list: Vec<Suggestion> = (0..40)
            .map(|i| sug(i % 30, &"reason ".repeat(40)))
            .collect();
        let c = mine_choices(&list);
        assert_eq!(c.len(), 25);
        let mut seen = std::collections::HashSet::new();
        assert!(c
            .iter()
            .all(|c| seen.insert(c.value.clone()) && c.label.chars().count() <= 100));
        assert!(c.iter().all(|c| c
            .description
            .as_deref()
            .is_some_and(|d| d.chars().count() <= 100)));
    }
}
