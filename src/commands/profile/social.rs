//! export, import, compare and `/top characters`.

use super::util::{begin_private, begin_public, pool, private_reply, require_own, uid};
use crate::{
    commands::say_ephemeral,
    db::{
        profile::{self as db, ExportDoc},
        works,
    },
    error::{Context, Error},
    i18n,
    render::{self, CellSpec},
    ui::{embeds::Emb, styles::style_strs},
};
use poise::serenity_prelude as serenity;

const MAX_IMPORT_BYTES: u32 = 2 * 1024 * 1024;

pub(super) async fn export(ctx: Context<'_>) -> Result<(), Error> {
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

pub(super) async fn import(ctx: Context<'_>, file: serenity::Attachment) -> Result<(), Error> {
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

pub(super) async fn compare(ctx: Context<'_>, user: serenity::User) -> Result<(), Error> {
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
    let shared_works = works::shared_works(&pool, me, other).await?;
    let pct = db::combined_compatibility(
        shared.len(),
        db::count(&pool, me).await? as usize,
        db::count(&pool, other).await? as usize,
        shared_works.len(),
        works::count(&pool, me).await? as usize,
        works::count(&pool, other).await? as usize,
    );
    let mut names = Vec::new();
    let mut cells = Vec::new();
    for id in shared.iter().take(30) {
        if let Some(e) = db::get_entry(&pool, me, *id).await?.filter(|e| e.visible()) {
            names.push(format!("• {}", e.name()));
            if cells.len() < 10 {
                cells.push(CellSpec {
                    label: e.name().to_string(),
                    subtitle: None,
                    rank: None,
                    image_url: e.image_url.clone(),
                });
            }
        }
    }
    let mut work_names = Vec::new();
    for id in shared_works.iter().take(20) {
        if let Some(e) = works::get_entry(&pool, me, id)
            .await?
            .filter(|e| e.visible())
        {
            work_names.push(format!("• {}", e.title()));
            if cells.len() < 10 {
                cells.push(CellSpec {
                    label: e.title().to_string(),
                    subtitle: None,
                    rank: None,
                    image_url: e.cover_url.clone(),
                });
            }
        }
    }
    let mut body = Vec::new();
    if !names.is_empty() {
        body.push(format!(
            "**{}**\n{}",
            i18n::COMPARE_CHARACTERS,
            names.join("\n")
        ));
    }
    if !work_names.is_empty() {
        body.push(format!(
            "**{}**\n{}",
            i18n::COMPARE_WORKS,
            work_names.join("\n")
        ));
    }
    let body = if body.is_empty() {
        i18n::COMPARE_NONE_SHARED.to_string()
    } else {
        body.join("\n\n")
    };
    // Mini collage of what you share (the text lists stay as the fallback).
    let image = ctx
        .data()
        .renderer
        .collage(
            cells,
            None,
            mine.accent_color.map(|c| c as u32),
            render::Theme::by_name(&mine.template),
            0.5,
            render::Images::new(),
        )
        .await;
    let mut embed = Emb::new()
        .title(i18n::COMPARE_TITLE)
        .colour(mine.accent_color.map(|c| c as u32))
        .description(format!(
            "**{}** ↔ **{}**\n{}\n\n{body}",
            ctx.author().display_name(),
            user.display_name(),
            i18n::compat(pct)
        ));
    let mut reply = poise::CreateReply::default();
    if let Some(img) = &image {
        let name = format!("compare.{}", img.ext);
        embed.image = Some(format!("attachment://{name}"));
        reply = reply.attachment(serenity::CreateAttachment::bytes(img.bytes.clone(), name));
    }
    ctx.send(reply.embed(embed.build())).await?;
    Ok(())
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
