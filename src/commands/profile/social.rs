//! export, import, compare and `/top characters`.

use super::util::{begin_private, begin_public, pool, private_reply, require_own, uid};
use crate::{
    commands::{buttons, say_ephemeral},
    db::profile::{self as db, ExportDoc},
    error::{Context, Error},
    i18n,
    ui::{
        embeds::Emb,
        pagination::{apply_nav, close_button, nav_row, page_count, parse_nav, CLOSE},
        session::{self, Outcome, Page, View},
        styles::style_strs,
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;

const MAX_IMPORT_BYTES: u32 = 2 * 1024 * 1024;
const TOP_PER_PAGE: usize = 10;

/// Export your profile as a JSON file.
#[poise::command(
    slash_command,
    name_localized("ar", "تصدير"),
    description_localized("ar", "صدّر بروفايلك كملف JSON")
)]
pub async fn export(ctx: Context<'_>) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let doc = db::export(&pool(ctx), uid(ctx)).await?;
    let json = serde_json::to_vec_pretty(&doc)?;
    ctx.send(
        poise::CreateReply::default()
            .content(i18n::EXPORT_DONE)
            .attachment(serenity::CreateAttachment::bytes(json, "profile.json"))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

/// Only files hosted by Discord's CDN are downloaded.
fn discord_cdn(url: &str) -> bool {
    url.starts_with("https://cdn.discordapp.com/")
        || url.starts_with("https://media.discordapp.net/")
}

/// Parse an uploaded export; `None` for anything that is not a valid export document.
pub fn parse_export(bytes: &[u8]) -> Option<ExportDoc> {
    if bytes.len() > MAX_IMPORT_BYTES as usize {
        return None;
    }
    serde_json::from_slice(bytes).ok()
}

/// Import a profile JSON file.
#[poise::command(
    slash_command,
    name_localized("ar", "استيراد"),
    description_localized("ar", "استورد بروفايلًا من ملف JSON")
)]
pub async fn import(
    ctx: Context<'_>,
    #[description = "JSON file from /profile export"]
    #[description_localized("ar", "ملف JSON من أمر التصدير")]
    #[name_localized("ar", "الملف")]
    file: serenity::Attachment,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    if file.size > MAX_IMPORT_BYTES
        || !file.filename.to_lowercase().ends_with(".json")
        || !discord_cdn(&file.url)
    {
        return private_reply(ctx, i18n::IMPORT_BAD_FILE).await;
    }
    let bytes = match ctx.data().http.get(&file.url).send().await {
        Ok(r) if r.status().is_success() => r.bytes().await.ok(),
        _ => None,
    };
    let Some(doc) = bytes.and_then(|b| parse_export(&b)) else {
        return private_reply(ctx, i18n::IMPORT_BAD_FILE).await;
    };
    match db::import(&pool(ctx), uid(ctx), &doc, &style_strs()).await {
        Ok(r) => private_reply(ctx, i18n::import_report(r.added, r.duplicates, r.invalid)).await,
        Err(db::ProfileError::Invalid(_)) => private_reply(ctx, i18n::IMPORT_BAD_FILE).await,
        Err(e) => Err(e.into()),
    }
}

/// Compare your profile with someone else's.
#[poise::command(
    slash_command,
    name_localized("ar", "مقارنة"),
    description_localized("ar", "قارن بروفايلك ببروفايل شخص آخر")
)]
pub async fn compare(
    ctx: Context<'_>,
    #[description = "The other user"]
    #[description_localized("ar", "المستخدم الآخر")]
    #[name_localized("ar", "المستخدم")]
    user: serenity::User,
) -> Result<(), Error> {
    begin_public(ctx).await?;
    let pool = pool(ctx);
    let (me, other) = (uid(ctx), user.id.get() as i64);
    let Some(mine) = db::get_profile(&pool, me).await? else {
        return say_ephemeral(ctx, i18n::PROFILE_NONE_SELF).await;
    };
    let Some(theirs) = db::get_profile(&pool, other).await? else {
        return say_ephemeral(ctx, i18n::PROFILE_NONE_OTHER).await;
    };
    if !db::can_view(&pool, me, &theirs).await? {
        return say_ephemeral(ctx, i18n::PROFILE_PRIVATE).await;
    }
    let shared = db::shared_characters(&pool, me, other).await?;
    let pct = db::compatibility(
        shared.len(),
        db::count(&pool, me).await? as usize,
        db::count(&pool, other).await? as usize,
    );
    let mut names = Vec::new();
    for id in shared.iter().take(30) {
        if let Some(e) = db::get_entry(&pool, me, *id).await?.filter(|e| e.visible()) {
            names.push(format!("• {}", e.name()));
        }
    }
    let body = if names.is_empty() {
        i18n::COMPARE_NONE_SHARED.to_string()
    } else {
        names.join("\n")
    };
    let embed = Emb::new()
        .title(i18n::COMPARE_TITLE)
        .colour(mine.accent_color.map(|c| c as u32))
        .description(format!(
            "**{}** ↔ **{}**\n{}\n\n{body}",
            ctx.author().display_name(),
            user.display_name(),
            i18n::compat(pct)
        ))
        .build();
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

// ---- /top characters -------------------------------------------------------------------

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum ScopeChoice {
    #[name = "السيرفر"]
    #[name_localized("en-US", "Server")]
    Server,
    #[name = "عالميًا"]
    #[name_localized("en-US", "Global")]
    Global,
}

struct TopView {
    pool: SqlitePool,
    guild: Option<i64>,
    page: usize,
    total: i64,
    rows: Vec<(i64, i64, String)>,
}

impl TopView {
    async fn load(&mut self) -> Result<(), db::ProfileError> {
        self.total = db::top_character_count(&self.pool, self.guild).await?;
        let pages = page_count(self.total as usize, TOP_PER_PAGE);
        self.page = self.page.min(pages - 1);
        let top = db::top_characters(
            &self.pool,
            self.guild,
            (self.page * TOP_PER_PAGE) as i64,
            TOP_PER_PAGE as i64,
        )
        .await?;
        self.rows.clear();
        for (id, n) in top {
            // Only verified-safe characters are ever listed.
            if let Some(c) = db::cache_get(&self.pool, id).await?.filter(|c| c.is_safe) {
                self.rows.push((id, n, c.name_romaji));
            }
        }
        Ok(())
    }
}

impl View for TopView {
    fn render(&self, enabled: bool) -> Page {
        let pages = page_count(self.total as usize, TOP_PER_PAGE);
        let body = if self.rows.is_empty() {
            i18n::EMPTY_LIST.to_string()
        } else {
            self.rows
                .iter()
                .enumerate()
                .map(|(i, (_, n, name))| {
                    format!("**{}.** {name} — ❤ {n}", self.page * TOP_PER_PAGE + i + 1)
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let scope = if self.guild.is_some() {
            i18n::SCOPE_SERVER
        } else {
            i18n::SCOPE_GLOBAL
        };
        let embed = Emb::new()
            .title(format!("{} — {scope}", i18n::TOP_TITLE))
            .description(body)
            .footer(format!(
                "{} · {}/{}",
                i18n::POWERED_BY,
                self.page + 1,
                pages
            ))
            .build();
        let mut rows = vec![nav_row(self.page, pages, enabled)];
        rows.push(buttons(vec![close_button(enabled)]));
        Page::embed(embed, rows)
    }

    async fn on_interaction(&mut self, id: &str, _values: &[String]) -> Outcome {
        if id == CLOSE {
            return Outcome::Close;
        }
        let Some(nav) = parse_nav(id) else {
            return Outcome::Keep;
        };
        let next = apply_nav(
            nav,
            self.page,
            page_count(self.total as usize, TOP_PER_PAGE),
        );
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

/// Leaderboards.
#[poise::command(
    slash_command,
    subcommands("top_characters"),
    name_localized("ar", "الأفضل"),
    description_localized("ar", "لوحات الصدارة")
)]
pub async fn top(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Most favourited characters.
#[poise::command(
    slash_command,
    rename = "characters",
    name_localized("ar", "الشخصيات"),
    description_localized("ar", "أكثر الشخصيات تفضيلًا")
)]
pub async fn top_characters(
    ctx: Context<'_>,
    #[description = "Scope (default: server)"]
    #[description_localized("ar", "النطاق (الافتراضي: السيرفر)")]
    #[name_localized("ar", "النطاق")]
    scope: Option<ScopeChoice>,
) -> Result<(), Error> {
    begin_public(ctx).await?;
    let guild = match (scope.unwrap_or(ScopeChoice::Server), ctx.guild_id()) {
        (ScopeChoice::Server, Some(g)) => Some(g.get() as i64),
        _ => None,
    };
    let mut view = TopView {
        pool: pool(ctx),
        guild,
        page: 0,
        total: 0,
        rows: Vec::new(),
    };
    view.load().await?;
    session::run(ctx, view).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_discord_cdn_is_downloaded() {
        assert!(discord_cdn(
            "https://cdn.discordapp.com/attachments/1/2/profile.json"
        ));
        assert!(!discord_cdn("https://evil.example/profile.json"));
        assert!(!discord_cdn("http://cdn.discordapp.com/x.json"));
        assert!(!discord_cdn(
            "https://cdn.discordapp.com.evil.example/x.json"
        ));
    }

    #[test]
    fn parse_export_validates() {
        assert!(parse_export(b"not json").is_none());
        assert!(parse_export(b"{}").is_none());
        assert!(parse_export(&vec![b' '; (MAX_IMPORT_BYTES as usize) + 1]).is_none());
        let doc = r#"{"version":1,"bio":null,"accent_color":null,"quote":null,"banner":null,
            "default_style":"names","privacy":"public","groups":[],"characters":[{"id":5,"group":null,"tier":null,"note":null}]}"#;
        assert_eq!(
            parse_export(doc.as_bytes()).map(|d| d.characters.len()),
            Some(1)
        );
    }
}
