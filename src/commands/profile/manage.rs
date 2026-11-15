//! Profile management commands: create, add, bulk-add, remove, clear, move, rank, note, bio, colour, banner,
//! style, privacy.

use super::{
    util::{
        ac_own, banner_url_ok, begin_private, cache_row, now, own_or_reply, parse_color, pool,
        private_reply, require_own, resolve_character, uid, Resolved,
    },
    PrivacyChoice, StyleChoice,
};
use crate::{
    anilist::{
        models::Character, safety::contains_explicit_term, AniError, ContentPolicy, SafeAniList,
    },
    commands::{character::ac_character, policy},
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

pub async fn fail_private(ctx: Context<'_>, e: AniError) -> Result<(), Error> {
    tracing::warn!(
        command = %ctx.command().name,
        user = %ctx.author().id,
        guild = ?ctx.guild_id(),
        "anilist error: {e}"
    );
    private_reply(ctx, crate::commands::ani_message(&e)).await
}

/// Cache the (already safety-checked) character, then add it to the profile.
pub async fn add_and_cache(pool: &SqlitePool, user: i64, c: &Character) -> Result<String, Error> {
    db::upsert_cache(pool, &cache_row(c, now())).await?;
    Ok(match db::add_character(pool, user, c.id).await? {
        AddOutcome::Added(pos) => i18n::added_character(c.display_name(), pos),
        AddOutcome::Duplicate => i18n::ALREADY_ADDED.to_string(),
    })
}

// ---- create -----------------------------------------------------------------------------

#[derive(Debug, poise::Modal)]
#[name = "تخصيص البروفايل"]
struct CreateModal {
    #[name = "النبذة"]
    #[paragraph]
    #[max_length = 500]
    bio: Option<String>,
    #[name = "اللون (مثل #3DB4F2)"]
    #[max_length = 7]
    color: Option<String>,
    #[name = "اقتباس مفضل"]
    #[max_length = 200]
    quote: Option<String>,
}

/// Create your profile.
#[poise::command(
    slash_command,
    name_localized("ar", "إنشاء"),
    description_localized("ar", "أنشئ بروفايلك")
)]
pub async fn create(
    ctx: Context<'_>,
    #[description = "Customize bio, colour and quote now"]
    #[description_localized("ar", "خصّص النبذة واللون والاقتباس الآن")]
    #[name_localized("ar", "تخصيص")]
    customize: Option<bool>,
) -> Result<(), Error> {
    let pool = pool(ctx);
    let user = uid(ctx);
    let mut modal = None;
    if customize == Some(true) {
        if let Context::Application(app) = ctx {
            modal = poise::execute_modal(app, None::<CreateModal>, Some(Duration::from_secs(300)))
                .await?;
            if modal.is_none() {
                return Ok(());
            }
        }
    } else {
        begin_private(ctx).await?;
    }
    let created = db::create_profile(&pool, user).await?;
    if let Some(m) = modal {
        if let Some(bio) = m.bio.as_deref() {
            db::set_bio(&pool, user, Some(bio)).await?;
        }
        if let Some(q) = m.quote.as_deref() {
            db::set_quote(&pool, user, Some(q)).await?;
        }
        if let Some(c) = m.color.as_deref().filter(|c| !c.trim().is_empty()) {
            match parse_color(c) {
                Some(col) => db::set_color(&pool, user, Some(col)).await?,
                None => return private_reply(ctx, i18n::COLOR_INVALID).await,
            }
        }
    }
    private_reply(
        ctx,
        if created {
            i18n::PROFILE_CREATED
        } else {
            i18n::PROFILE_EXISTS
        },
    )
    .await
}

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

/// Add a character to your profile.
#[poise::command(
    slash_command,
    name_localized("ar", "إضافة"),
    description_localized("ar", "أضف شخصية إلى بروفايلك")
)]
pub async fn add(
    ctx: Context<'_>,
    #[description = "Character name"]
    #[description_localized("ar", "اسم الشخصية")]
    #[name_localized("ar", "الشخصية")]
    #[autocomplete = "ac_character"]
    character: String,
) -> Result<(), Error> {
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

/// Add each line (id, `id:N` or name -> best match). At most `MAX_BULK` lookups per run.
pub async fn bulk_add_lines(
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

/// Add several characters at once.
#[poise::command(
    slash_command,
    rename = "bulk-add",
    name_localized("ar", "إضافة-متعددة"),
    description_localized("ar", "أضف عدة شخصيات دفعة واحدة")
)]
pub async fn bulk_add(ctx: Context<'_>) -> Result<(), Error> {
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

/// Remove a character from your profile.
#[poise::command(
    slash_command,
    name_localized("ar", "إزالة"),
    description_localized("ar", "أزل شخصية من بروفايلك")
)]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "Character"]
    #[description_localized("ar", "الشخصية")]
    #[name_localized("ar", "الشخصية")]
    #[autocomplete = "ac_own"]
    character: String,
) -> Result<(), Error> {
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
                self.text = match db::clear(&self.pool, self.user).await {
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

/// Delete all characters from your profile (asks for confirmation).
#[poise::command(
    slash_command,
    name_localized("ar", "مسح"),
    description_localized("ar", "احذف كل شخصيات بروفايلك (مع تأكيد)")
)]
pub async fn clear(ctx: Context<'_>) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    let view = ConfirmView {
        pool: pool(ctx),
        user: uid(ctx),
        text: i18n::CLEAR_CONFIRM.to_string(),
    };
    session::run(ctx, view).await
}

/// Move a character to a position.
#[poise::command(
    slash_command,
    rename = "move",
    name_localized("ar", "نقل"),
    description_localized("ar", "انقل شخصية إلى مركز معين")
)]
pub async fn move_to(
    ctx: Context<'_>,
    #[description = "Character"]
    #[description_localized("ar", "الشخصية")]
    #[name_localized("ar", "الشخصية")]
    #[autocomplete = "ac_own"]
    character: String,
    #[description = "New position (1 = first)"]
    #[description_localized("ar", "المركز الجديد (1 = الأول)")]
    #[name_localized("ar", "المركز")]
    #[min = 1]
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

async fn ac_tier(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let mut options: Vec<String> = ["S", "A", "B", "C"].iter().map(|s| s.to_string()).collect();
    if let Ok(groups) = db::list_groups(&ctx.data().db, uid(ctx)).await {
        options.extend(groups.into_iter().map(|g| g.name));
    }
    options.push("حذف".into());
    let needle = partial.trim().to_lowercase();
    options
        .into_iter()
        .filter(|o| o.to_lowercase().contains(&needle))
        .take(25)
        .map(|o| serenity::AutocompleteChoice::new(o.clone(), o))
        .collect()
}

/// Rank a character in a tier (S/A/B/C or a custom name).
#[poise::command(
    slash_command,
    name_localized("ar", "تصنيف"),
    description_localized("ar", "ضع شخصية في فئة (S/A/B/C أو اسم مخصص)")
)]
pub async fn rank(
    ctx: Context<'_>,
    #[description = "Character"]
    #[description_localized("ar", "الشخصية")]
    #[name_localized("ar", "الشخصية")]
    #[autocomplete = "ac_own"]
    character: String,
    #[description = "Tier, or '-' to clear"]
    #[description_localized("ar", "الفئة، أو - للحذف")]
    #[name_localized("ar", "الفئة")]
    #[autocomplete = "ac_tier"]
    tier: String,
) -> Result<(), Error> {
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

/// Add a private note to a character.
#[poise::command(
    slash_command,
    name_localized("ar", "ملاحظة"),
    description_localized("ar", "أضف ملاحظة خاصة على شخصية")
)]
pub async fn note(
    ctx: Context<'_>,
    #[description = "Character"]
    #[description_localized("ar", "الشخصية")]
    #[name_localized("ar", "الشخصية")]
    #[autocomplete = "ac_own"]
    character: String,
    #[description = "Note, or '-' to clear"]
    #[description_localized("ar", "الملاحظة، أو - للحذف")]
    #[name_localized("ar", "النص")]
    text: String,
) -> Result<(), Error> {
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

/// Set your bio.
#[poise::command(
    slash_command,
    rename = "set-bio",
    name_localized("ar", "تعيين-النبذة"),
    description_localized("ar", "غيّر نبذة بروفايلك")
)]
pub async fn set_bio(
    ctx: Context<'_>,
    #[description = "Bio, or '-' to clear"]
    #[description_localized("ar", "النبذة، أو - للحذف")]
    #[name_localized("ar", "النص")]
    text: String,
) -> Result<(), Error> {
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

/// Set your accent colour.
#[poise::command(
    slash_command,
    rename = "set-color",
    name_localized("ar", "تعيين-اللون"),
    description_localized("ar", "غيّر لون بروفايلك")
)]
pub async fn set_color(
    ctx: Context<'_>,
    #[description = "Hex colour like #3DB4F2, or '-' to reset"]
    #[description_localized("ar", "لون مثل #3DB4F2، أو - للإعادة")]
    #[name_localized("ar", "اللون")]
    color: String,
) -> Result<(), Error> {
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

/// Set your banner from a character or an image link.
#[poise::command(
    slash_command,
    rename = "set-banner",
    name_localized("ar", "تعيين-الغلاف"),
    description_localized("ar", "اختر غلاف بروفايلك من شخصية أو رابط صورة")
)]
pub async fn set_banner(
    ctx: Context<'_>,
    #[description = "Character or image link ('-' to clear)"]
    #[description_localized("ar", "شخصية أو رابط صورة (- للحذف)")]
    #[name_localized("ar", "المصدر")]
    #[autocomplete = "ac_character"]
    source: String,
) -> Result<(), Error> {
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

/// Set your default display style.
#[poise::command(
    slash_command,
    name_localized("ar", "نمط"),
    description_localized("ar", "اختر نمط عرض بروفايلك الافتراضي")
)]
pub async fn style(
    ctx: Context<'_>,
    #[description = "Style"]
    #[description_localized("ar", "النمط")]
    #[name_localized("ar", "النمط")]
    style: StyleChoice,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    db::set_style(&pool(ctx), uid(ctx), style.style().as_str()).await?;
    private_reply(ctx, i18n::STYLE_SET).await
}

/// Choose who can see your profile.
#[poise::command(
    slash_command,
    name_localized("ar", "الخصوصية"),
    description_localized("ar", "من يستطيع رؤية بروفايلك")
)]
pub async fn privacy(
    ctx: Context<'_>,
    #[description = "Visibility"]
    #[description_localized("ar", "مستوى الخصوصية")]
    #[name_localized("ar", "المستوى")]
    level: PrivacyChoice,
) -> Result<(), Error> {
    begin_private(ctx).await?;
    if require_own(ctx).await?.is_none() {
        return Ok(());
    }
    db::set_privacy(&pool(ctx), uid(ctx), level.privacy()).await?;
    private_reply(ctx, i18n::PRIVACY_SET).await
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
