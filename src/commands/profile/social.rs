//! Export and import of a profile.

use super::util::{begin_private, pool, private_reply, require_own, uid};
use crate::{
    db::profile::{self as db, ExportDoc},
    error::{Context, Error},
    i18n,
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
    match db::import(&pool(ctx), uid(ctx), &doc).await {
        Ok(r) => private_reply(ctx, i18n::import_report(r.added, r.duplicates, r.invalid)).await,
        Err(db::ProfileError::Invalid(_)) => private_reply(ctx, i18n::IMPORT_BAD_FILE).await,
        Err(e) => Err(e.into()),
    }
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
