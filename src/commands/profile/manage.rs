//! Profile management commands: create, add, bulk-add, remove, clear, move, rank, note, bio, colour, banner,
//! style, privacy.

use super::util::{
    banner_url_ok, begin_private, cache_row, now, own_or_reply, parse_color, pool, private_reply,
    require_own, resolve_character, uid, Resolved,
};
use crate::{
    anilist::{
        models::Character, safety::contains_explicit_term, AniError, ContentPolicy, SafeAniList,
    },
    commands::policy,
    db::profile::{self as db, AddOutcome},
    error::{Context, Error},
    i18n,
    ui::{
        cards::picker_character_embed,
        pagination::{close_button, CLOSE},
        select::{character_choices, pick_row, PICK_ID},
        session::{self, Outcome, Page, View},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::time::Duration;

const MAX_BULK: usize = 30;
const CLEAR_WORDS: &[&str] = &["-", "حذف", "مسح"];

fn is_clear(s: &str) -> bool {
    CLEAR_WORDS.contains(&s.trim())
}

pub(super) async fn fail_private(ctx: Context<'_>, e: AniError) -> Result<(), Error> {
    tracing::warn!(
        command = %ctx.command().name,
        user = %ctx.author().id,
        guild = ?ctx.guild_id(),
        "anilist error: {e}"
    );
    private_reply(ctx, crate::commands::ani_message(&e)).await
}

pub async fn add_and_cache(pool: &SqlitePool, user: i64, c: &Character) -> Result<String, Error> {
    db::upsert_cache(pool, &cache_row(c, now())).await?;
    Ok(match db::add_character(pool, user, c.id).await? {
        AddOutcome::Added(pos) => i18n::added_character(c.display_name(), pos),
        AddOutcome::Duplicate => i18n::ALREADY_ADDED.to_string(),
    })
}

// ---- create -----------------------------------------------------------------------------

// ---- add --------------------------------------------------------------------------------

struct PickAddView {
    pool: SqlitePool,
    user: i64,
    results: Vec<Character>,
    done: Option<String>,
}

impl View for PickAddView {
    fn render(&self, enabled: bool) -> Page {
        let rows = pick_row(character_choices(&self.results), enabled)
            .into_iter()
            .chain([crate::commands::buttons(vec![close_button(enabled)])])
            .collect();
        Page::embed(picker_character_embed(&self.results).build(), rows).notice(self.done.clone())
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        match id {
            CLOSE => Outcome::Close,
            PICK_ID => {
                let pick = values.first().and_then(|v| v.parse::<i64>().ok());
                let Some(c) = self.results.iter().find(|c| Some(c.id) == pick) else {
                    return Outcome::Keep;
                };
                self.done = Some(match add_and_cache(&self.pool, self.user, c).await {
                    Ok(text) => text,
                    Err(e) => {
                        tracing::warn!("could not add character: {e:#}");
                        i18n::ERR_GENERIC.to_string()
                    }
                });
                Outcome::Close
            }
            _ => Outcome::Keep,
        }
    }
}

pub(super) async fn add(ctx: Context<'_>, character: String) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let api = ctx.data().anilist.clone();
    match resolve_character(&api, policy(ctx), &character).await {
        Err(e) => fail_private(ctx, e).await,
        Ok(Resolved::One(c)) => {
            let text = add_and_cache(&pool(ctx), uid(ctx), &c).await?;
            private_reply(ctx, text).await
        }
        Ok(Resolved::Many(results)) => {
            session::run(
                ctx,
                PickAddView {
                    pool: pool(ctx),
                    user: uid(ctx),
                    results,
                    done: None,
                },
            )
            .await
        }
        Ok(Resolved::Hidden) => private_reply(ctx, i18n::CHARACTER_HIDDEN).await,
        Ok(Resolved::NotFound) => private_reply(ctx, i18n::NO_RESULTS).await,
    }
}

#[derive(Debug, poise::Modal)]
#[name = "إضافة عدة شخصيات"]
struct BulkModal {
    #[name = "أسماء أو أرقام AniList (سطر لكل شخصية)"]
    #[paragraph]
    #[max_length = 4000]
    lines: String,
}

pub struct BulkOutcome {
    pub added: Vec<String>,
    pub not_found: Vec<String>,
    pub skipped: Vec<String>,
    pub over_limit: usize,
}

/// Distinct non-empty lines.
pub fn bulk_lines(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && seen.insert(l.to_lowercase()))
        .map(str::to_string)
        .collect()
}

pub(super) async fn bulk_add_lines(
    api: &SafeAniList,
    pool: &SqlitePool,
    user: i64,
    policy: ContentPolicy,
    lines: Vec<String>,
) -> Result<BulkOutcome, Error> {
    let mut out = BulkOutcome {
        added: vec![],
        not_found: vec![],
        skipped: vec![],
        over_limit: 0,
    };
    let total = lines.len();
    for (i, line) in lines.into_iter().enumerate() {
        if i >= MAX_BULK {
            out.over_limit = total - MAX_BULK;
            break;
        }
        // Bare numbers are AniList ids here.
        let input = if line.chars().all(|c| c.is_ascii_digit()) {
            format!("id:{line}")
        } else {
            line.clone()
        };
        match resolve_character(api, policy, &input).await {
            Ok(Resolved::One(c)) => push_add(&mut out, pool, user, &c, &line).await?,
            Ok(Resolved::Many(list)) => {
                if let Some(c) = list.first() {
                    push_add(&mut out, pool, user, c, &line).await?;
                }
            }
            Ok(Resolved::Hidden) => out.skipped.push(line),
            Ok(Resolved::NotFound) => out.not_found.push(line),
            Err(e) => {
                tracing::warn!("bulk add stopped: {e}");
                out.over_limit = total - i;
                break;
            }
        }
    }
    Ok(out)
}

async fn push_add(
    out: &mut BulkOutcome,
    pool: &SqlitePool,
    user: i64,
    c: &Character,
    line: &str,
) -> Result<(), Error> {
    db::upsert_cache(pool, &cache_row(c, now())).await?;
    match db::add_character(pool, user, c.id).await? {
        AddOutcome::Added(_) => out.added.push(c.display_name().to_string()),
        AddOutcome::Duplicate => out.skipped.push(line.to_string()),
    }
    Ok(())
}

pub(super) async fn bulk_add(ctx: Context<'_>) -> Result<(), Error> {
    let Context::Application(app) = ctx else {
        return Ok(());
    };
    if db::get_profile(&pool(ctx), uid(ctx)).await?.is_none() {
        return private_reply(ctx, i18n::PROFILE_NONE_SELF).await;
    }
    let Some(m) =
        poise::execute_modal(app, None::<BulkModal>, Some(Duration::from_secs(600))).await?
    else {
        return Ok(());
    };
    let lines = bulk_lines(&m.lines);
    if lines.is_empty() {
        return private_reply(ctx, i18n::BULK_EMPTY).await;
    }
    let api = ctx.data().anilist.clone();
    let out = bulk_add_lines(&api, &pool(ctx), uid(ctx), policy(ctx), lines).await?;
    private_reply(
        ctx,
        i18n::bulk_report(&out.added, &out.not_found, &out.skipped, out.over_limit),
    )
    .await
}

// ---- remove / clear / move --------------------------------------------------------------

pub(super) async fn remove(ctx: Context<'_>, character: String) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let Some((id, name)) = own_or_reply(ctx, &character).await? else {
        return Ok(());
    };
    db::remove_character(&pool(ctx), uid(ctx), id).await?;
    private_reply(ctx, i18n::removed_character(&name)).await
}

struct ConfirmView {
    pool: SqlitePool,
    user: i64,
    text: String,
    /// Clear the favourite works instead of the characters.
    works: bool,
}

impl View for ConfirmView {
    fn render(&self, enabled: bool) -> Page {
        let row = crate::commands::buttons(vec![
            serenity::CreateButton::new("confirm")
                .label(i18n::BTN_CONFIRM)
                .style(serenity::ButtonStyle::Danger)
                .disabled(!enabled),
            serenity::CreateButton::new("cancel")
                .label(i18n::BTN_CANCEL)
                .style(serenity::ButtonStyle::Secondary)
                .disabled(!enabled),
        ]);
        Page {
            content: Some(self.text.clone()),
            embeds: vec![],
            components: vec![row],
            files: vec![],
        }
    }

    async fn on_interaction(&mut self, id: &str, _values: &[String]) -> Outcome {
        match id {
            "confirm" => {
                let cleared = if self.works {
                    crate::db::works::clear(&self.pool, self.user).await
                } else {
                    db::clear(&self.pool, self.user).await
                };
                self.text = match cleared {
                    Ok(n) => i18n::cleared(n),
                    Err(e) => {
                        tracing::warn!("clear failed: {e}");
                        i18n::ERR_GENERIC.to_string()
                    }
                };
                Outcome::Close
            }
            "cancel" => {
                self.text = i18n::CANCELLED.to_string();
                Outcome::Close
            }
            _ => Outcome::Keep,
        }
    }
}

pub(super) async fn clear(ctx: Context<'_>) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let view = ConfirmView {
        pool: pool(ctx),
        user: uid(ctx),
        text: i18n::CLEAR_CONFIRM.to_string(),
        works: false,
    };
    session::run(ctx, view).await
}

pub(super) async fn move_to(
    ctx: Context<'_>,
    character: String,
    position: i64,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let Some((id, name)) = own_or_reply(ctx, &character).await? else {
        return Ok(());
    };
    let to = db::move_character(&pool(ctx), uid(ctx), id, position).await?;
    private_reply(ctx, i18n::moved_character(&name, to)).await
}

// ---- rank / note ------------------------------------------------------------------------

pub(super) async fn rank(ctx: Context<'_>, character: String, tier: String) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let Some((id, name)) = own_or_reply(ctx, &character).await? else {
        return Ok(());
    };
    if is_clear(&tier) {
        db::set_tier(&pool(ctx), uid(ctx), id, None).await?;
        return private_reply(ctx, i18n::tier_cleared(&name)).await;
    }
    db::set_tier(&pool(ctx), uid(ctx), id, Some(&tier)).await?;
    private_reply(ctx, i18n::tier_set(&name, tier.trim())).await
}

pub(super) async fn note(ctx: Context<'_>, character: String, text: String) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let Some((id, _)) = own_or_reply(ctx, &character).await? else {
        return Ok(());
    };
    if is_clear(&text) {
        db::set_note(&pool(ctx), uid(ctx), id, None).await?;
        return private_reply(ctx, i18n::NOTE_CLEARED).await;
    }
    db::set_note(&pool(ctx), uid(ctx), id, Some(&text)).await?;
    private_reply(ctx, i18n::NOTE_SET).await
}

// ---- bio / colour / banner / style / privacy --------------------------------------------

pub(super) async fn set_bio(ctx: Context<'_>, text: String) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    if contains_explicit_term(&text) {
        return private_reply(ctx, i18n::CHARACTER_HIDDEN).await;
    }
    db::set_bio(
        &pool(ctx),
        uid(ctx),
        (!is_clear(&text)).then_some(text.as_str()),
    )
    .await?;
    private_reply(ctx, i18n::BIO_SET).await
}

pub(super) async fn set_color(ctx: Context<'_>, color: String) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let value = if is_clear(&color) {
        None
    } else {
        match parse_color(&color) {
            Some(c) => Some(c),
            None => return private_reply(ctx, i18n::COLOR_INVALID).await,
        }
    };
    db::set_color(&pool(ctx), uid(ctx), value).await?;
    private_reply(ctx, i18n::COLOR_SET).await
}

/// Best-effort size check: reject images over 8 MB when the host reports a length.
async fn banner_size_ok(http: &reqwest::Client, url: &str) -> bool {
    match http.head(url).timeout(Duration::from_secs(5)).send().await {
        Ok(r) if r.status().is_success() => r.content_length().is_none_or(|n| n <= 8 * 1024 * 1024),
        _ => false,
    }
}

pub(super) async fn set_banner(ctx: Context<'_>, source: String) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let (pool, user) = (pool(ctx), uid(ctx));
    if is_clear(&source) {
        db::set_banner(&pool, user, None).await?;
        return private_reply(ctx, i18n::BANNER_SET).await;
    }
    let url = if source.trim().starts_with("https://") {
        let url = source.trim().to_string();
        if !banner_url_ok(&url) || !banner_size_ok(&ctx.data().http, &url).await {
            return private_reply(ctx, i18n::BANNER_INVALID).await;
        }
        url
    } else {
        let api = ctx.data().anilist.clone();
        let character: Option<Character> = match resolve_character(&api, policy(ctx), &source).await
        {
            Err(e) => return fail_private(ctx, e).await,
            Ok(Resolved::One(c)) => Some(*c),
            Ok(Resolved::Many(mut l)) => (!l.is_empty()).then(|| l.remove(0)),
            Ok(Resolved::Hidden) => return private_reply(ctx, i18n::CHARACTER_HIDDEN).await,
            Ok(Resolved::NotFound) => None,
        };
        match character.as_ref().and_then(|c| c.image_url()) {
            Some(u) => u.to_string(),
            None => return private_reply(ctx, i18n::NO_RESULTS).await,
        }
    };
    db::set_banner(&pool, user, Some(&url)).await?;
    private_reply(ctx, i18n::BANNER_SET).await
}

/// Delete all favourite works (asks for confirmation).
pub(super) async fn clear_works(ctx: Context<'_>) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let view = ConfirmView {
        pool: pool(ctx),
        user: uid(ctx),
        text: i18n::CLEAR_WORKS_CONFIRM.to_string(),
        works: true,
    };
    session::run(ctx, view).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bulk_lines_dedupe_and_trim() {
        let l = bulk_lines("  Naruto \n\nnaruto\n123\n\n  Sasuke\n123");
        assert_eq!(l, vec!["Naruto", "123", "Sasuke"]);
        assert!(bulk_lines("  \n \n").is_empty());
    }

    #[test]
    fn clear_words() {
        assert!(is_clear("-") && is_clear(" حذف ") && !is_clear("S"));
    }

    use crate::{anilist::AniError, db::connect_memory};
    use serde_json::Value;

    fn api() -> SafeAniList {
        let get = |name: &str| -> Value {
            let text = match name {
                "safe" => include_str!("../../../tests/fixtures/anilist/character_safe.json"),
                "adult" => include_str!("../../../tests/fixtures/anilist/character_adult.json"),
                _ => include_str!("../../../tests/fixtures/anilist/search_character.json"),
            };
            serde_json::from_str::<Value>(text).expect("fixture")["data"].clone()
        };
        SafeAniList::with_fixture(move |_, vars| {
            if let Some(id) = vars["id"].as_i64() {
                return match id {
                    10 => Ok(get("safe")),
                    11 => Ok(get("adult")),
                    _ => Err(AniError::NotFound),
                };
            }
            Ok(get("search"))
        })
    }

    #[tokio::test]
    async fn bulk_add_reports_added_skipped_and_missing() {
        let pool = connect_memory().await;
        db::create_profile(&pool, 1).await.expect("profile");
        let lines = bulk_lines("10\n11\n999\nHigh Favs Safe\n10");
        let out = bulk_add_lines(&api(), &pool, 1, ContentPolicy::SAFE, lines)
            .await
            .expect("bulk");
        // 10 added; 11 hidden (adult) -> skipped; 999 unknown; the name search adds the best safe match (14).
        assert_eq!(out.added.len(), 2, "{:?}", out.added);
        assert_eq!(out.skipped, vec!["11"]);
        assert_eq!(out.not_found, vec!["999"]);
        assert_eq!(out.over_limit, 0);
        assert_eq!(db::count(&pool, 1).await.expect("n"), 2);
        let again = bulk_add_lines(&api(), &pool, 1, ContentPolicy::SAFE, bulk_lines("10"))
            .await
            .expect("bulk");
        assert_eq!(again.skipped, vec!["10"], "duplicates are skipped politely");
    }

    #[tokio::test]
    async fn bulk_add_is_capped_per_run() {
        let pool = connect_memory().await;
        db::create_profile(&pool, 1).await.expect("profile");
        let text: String = (1000..1045).map(|i| format!("{i}\n")).collect();
        let out = bulk_add_lines(&api(), &pool, 1, ContentPolicy::SAFE, bulk_lines(&text))
            .await
            .expect("bulk");
        assert_eq!(out.not_found.len(), MAX_BULK);
        assert_eq!(out.over_limit, 45 - MAX_BULK);
    }

    #[tokio::test]
    async fn add_and_cache_is_idempotent() {
        let pool = connect_memory().await;
        db::create_profile(&pool, 1).await.expect("profile");
        let c: Character = serde_json::from_value(
            serde_json::from_str::<Value>(include_str!(
                "../../../tests/fixtures/anilist/character_safe.json"
            ))
            .expect("fixture")["data"]["Character"]
                .clone(),
        )
        .expect("character");
        let first = add_and_cache(&pool, 1, &c).await.expect("first");
        assert!(first.contains("المركز 1"));
        assert_eq!(
            add_and_cache(&pool, 1, &c).await.expect("second"),
            i18n::ALREADY_ADDED
        );
        let e = db::get_entry(&pool, 1, 10).await.expect("e").expect("some");
        assert!(e.visible(), "an added character is cached as verified safe");
    }
}
