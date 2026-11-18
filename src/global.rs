//! Session-less handlers of the universal buttons (`components`): "أظهر العمل", "أظهر الشخصيات", the menus that open
//! an item, "ذكّرني", "أضف لبروفايلي" and the status select. A press never edits the message it came from (except the
//! page buttons of a roster message, which belong to that new message); the answer is a NEW message and anyone may press.

use crate::{
    anilist::{kind_of, CharacterLookup, ContentPolicy, MediaKind},
    commands::media::{media_cache_row, work_key_of},
    components::{self, ItemKind, Press},
    db::{profile as pdb, reminders, status, works},
    error::Error,
    i18n,
    providers::Kind as PKind,
    tmdb::{self, model as tm},
    ui::{cards, embeds::Emb},
    Data,
};
use poise::serenity_prelude as serenity;

/// What a press answers with.
#[derive(Default)]
pub struct Msg {
    pub text: Option<String>,
    pub embed: Option<serenity::CreateEmbed>,
    pub files: Vec<serenity::CreateAttachment>,
    pub rows: Vec<serenity::CreateActionRow>,
}

impl Msg {
    fn text(t: impl Into<String>) -> Self {
        Self {
            text: Some(t.into()),
            ..Self::default()
        }
    }

    fn card(e: Emb, rows: Vec<serenity::CreateActionRow>) -> Self {
        Self {
            embed: Some(e.build()),
            rows,
            ..Self::default()
        }
    }

    fn unavailable() -> Self {
        Self::text(i18n::SHOW_WORK_UNAVAILABLE)
    }
}

fn link(url: &str, label: &str) -> serenity::CreateButton {
    serenity::CreateButton::new_link(url).label(format!("🔗 {label}"))
}

/// The rows under a work card: the standard buttons, add, remind and the source link, then the status select.
fn work_rows(
    key: &str,
    url: Option<&str>,
    credit: &str,
    remindable: bool,
    written: bool,
    current: Option<&str>,
) -> Vec<serenity::CreateActionRow> {
    let mut row = components::standard_actions(ItemKind::Work, Some(key));
    row.push(components::add_button(key));
    if remindable {
        row.push(components::remind_button(key));
    }
    if let Some(u) = url {
        row.push(link(u, credit));
    }
    let mut rows = vec![serenity::CreateActionRow::Buttons(row)];
    rows.extend(components::status_menu(key, written, current));
    rows
}

fn category_of(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Anime => "anime",
        MediaKind::Novel => "novel",
        _ => "manga",
    }
}

fn tmdb_category(
    w: &tm::Work,
    overrides: &std::collections::HashMap<String, tm::Target>,
) -> &'static str {
    match w.kind {
        Some(tm::Kind::Movie) => "movie",
        _ => match tm::classify(&w.countries, overrides) {
            tm::Target::Drama => "drama",
            tm::Target::Series => "series",
        },
    }
}

async fn guild_overrides(
    data: &Data,
    guild: Option<i64>,
) -> std::collections::HashMap<String, tm::Target> {
    match guild {
        Some(g) => crate::db::notify::drama_overrides(&data.db, g)
            .await
            .unwrap_or_default(),
        None => Default::default(),
    }
}

/// The card of a work of any source, as a new message.
/// The server's `mature-themes` switch (on outside servers and when unreadable).
pub async fn mature_for(data: &Data, guild: Option<i64>) -> bool {
    match guild {
        Some(g) => crate::db::custom::mature_themes(&data.db, g)
            .await
            .unwrap_or(true),
        None => true,
    }
}

pub async fn work_msg(data: &Data, key: &str, user: i64, mature: bool) -> Msg {
    let current = status::get(&data.db, user, key).await.ok().flatten();
    if let Some(id) = key
        .strip_prefix("anilist:")
        .and_then(|n| n.parse::<i64>().ok())
    {
        return match data.anilist.media_by_id(id, ContentPolicy::SAFE).await {
            Ok(Some(m)) => {
                let kind = kind_of(&m);
                Msg::card(
                    cards::work_card(&m),
                    work_rows(
                        key,
                        m.site_url.as_deref(),
                        i18n::BTN_ANILIST,
                        kind != MediaKind::Novel,
                        kind.is_written(),
                        current.as_deref(),
                    ),
                )
            }
            _ => Msg::unavailable(),
        };
    }
    if let Some((kind, id)) = tm::parse_work_key(key) {
        let Some(w) = data.tmdb.details(kind, id).await else {
            return Msg::unavailable();
        };
        let e = tmdb::card::work_embed(&w, data.tmdb.region(), false);
        return Msg::card(
            e,
            work_rows(key, Some(&w.url()), "TMDB", true, false, current.as_deref()),
        );
    }
    match data.providers.work_by_key(key, mature).await {
        Some(w) => {
            let cover = data.providers.cover(&w).await;
            let name = cover
                .as_ref()
                .map_or("cover.jpg", |c| crate::commands::fallback::cover_name(c));
            let e = crate::commands::fallback::card(&w, cover.is_some(), name);
            let mut m = Msg::card(
                e,
                work_rows(
                    key,
                    w.url.as_deref(),
                    w.source.credit(),
                    false,
                    false,
                    current.as_deref(),
                ),
            );
            if let Some(c) = cover {
                m.files.push(serenity::CreateAttachment::bytes(
                    c.to_vec(),
                    name.to_string(),
                ));
            }
            m
        }
        None => Msg::unavailable(),
    }
}

/// One page of the characters of a work (0-based), as a message with a menu of them and page buttons.
pub async fn roster_msg(data: &Data, key: &str, page: usize, mature: bool) -> Msg {
    if let Some(id) = key
        .strip_prefix("anilist:")
        .and_then(|n| n.parse::<i64>().ok())
    {
        let Ok(Some(m)) = data.anilist.media_by_id(id, ContentPolicy::SAFE).await else {
            return Msg::unavailable();
        };
        let Ok(p) = data
            .anilist
            .media_characters(id, page as u32 + 1, ContentPolicy::SAFE)
            .await
        else {
            return Msg::unavailable();
        };
        let pages = crate::commands::media::pages_of(p.last_page, p.has_next, page);
        if p.items.is_empty() {
            return Msg::text(i18n::NO_CHARACTERS);
        }
        let items: Vec<(String, String)> = p
            .items
            .iter()
            .map(|(c, _)| {
                (
                    crate::ui::embeds::fit(c.display_name(), 100),
                    c.id.to_string(),
                )
            })
            .collect();
        let mut rows: Vec<_> = components::character_menu(i18n::PICK_CHARACTER_PLACEHOLDER, &items)
            .into_iter()
            .collect();
        if pages > 1 {
            rows.push(roster_nav(key, page, pages));
        }
        return Msg::card(cards::characters_embed(&m, &p.items, page, pages), rows);
    }
    if let Some((kind, id)) = tm::parse_work_key(key) {
        let Some(w) = data.tmdb.details(kind, id).await else {
            return Msg::unavailable();
        };
        if w.cast.is_empty() {
            return Msg::text(i18n::NO_CHARACTERS);
        }
        let lines: Vec<String> = w
            .cast
            .iter()
            .take(25)
            .map(|c| format!("**{}** — {}", c.display_name(), c.actor))
            .collect();
        let items: Vec<(String, String)> = w
            .cast
            .iter()
            .take(25)
            .map(|c| {
                (
                    crate::ui::embeds::fit(c.display_name(), 100),
                    c.key(kind, id),
                )
            })
            .collect();
        let e = Emb::new()
            .title(format!("{} — {}", i18n::TITLE_CHARACTERS, w.title))
            .url(Some(w.url()))
            .description(lines.join("\n"))
            .footer(tmdb::FOOTER_CREDIT);
        let rows = components::character_menu(i18n::PICK_CHARACTER_PLACEHOLDER, &items)
            .into_iter()
            .collect();
        return Msg::card(e, rows);
    }
    if key.starts_with("igdb:") || key.starts_with("vndb:v") {
        return provider_roster(data, key, page, mature).await;
    }
    Msg::text(i18n::NO_CHARACTERS_SOURCE)
}

/// Characters per page of a game roster (one select menu holds 25).
pub const ROSTER_PAGE: usize = 25;

/// The characters of a game or visual novel: a select menu and page buttons. A failed source is said so; a game with no
/// characters in the sources gets a polite note that suggests searching by the character's name.
async fn provider_roster(data: &Data, key: &str, page: usize, mature: bool) -> Msg {
    // The game itself must pass the safety rules before its roster is shown.
    let Some(game) = data.providers.work_by_key(key, mature).await else {
        return Msg::text(i18n::SHOW_WORK_UNAVAILABLE);
    };
    let list = match data.providers.characters_of(key, mature).await {
        Ok(l) => l,
        Err(_) => return Msg::text(i18n::GAMES_DOWN),
    };
    if list.is_empty() {
        return Msg::text(i18n::game_without_characters(&game.title));
    }
    let pages = list.len().div_ceil(ROSTER_PAGE);
    let page = page.min(pages - 1);
    let slice = &list[page * ROSTER_PAGE..((page + 1) * ROSTER_PAGE).min(list.len())];
    let lines: Vec<String> = slice.iter().map(|c| format!("**{}**", c.title)).collect();
    let items: Vec<(String, String)> = slice
        .iter()
        .map(|c| (crate::ui::embeds::fit(&c.title, 100), c.id.clone()))
        .collect();
    let e = Emb::new()
        .title(format!("{} — {}", i18n::TITLE_CHARACTERS, game.title))
        .description(lines.join("\n"))
        .footer(format!("{} · {}/{}", game.source.credit(), page + 1, pages));
    let mut rows: Vec<_> = components::character_menu(i18n::PICK_CHARACTER_PLACEHOLDER, &items)
        .into_iter()
        .collect();
    if pages > 1 {
        rows.push(roster_nav(key, page, pages));
    }
    Msg::card(e, rows)
}

fn roster_nav(key: &str, page: usize, pages: usize) -> serenity::CreateActionRow {
    let btn = |label: &str, to: usize, off: bool| {
        serenity::CreateButton::new(components::roster_nav_id(key, to))
            .label(label)
            .style(serenity::ButtonStyle::Secondary)
            .disabled(off)
    };
    serenity::CreateActionRow::Buttons(vec![
        btn("◀", page.saturating_sub(1), page == 0),
        serenity::CreateButton::new("nav:indicator")
            .label(format!("{}/{}", page + 1, pages))
            .style(serenity::ButtonStyle::Secondary)
            .disabled(true),
        btn("▶", (page + 1).min(pages - 1), page + 1 >= pages),
    ])
}

/// The card of a character of any source. `value` is an AniList character id, a `tmdb-cast:` key or a provider key.
pub async fn character_msg(data: &Data, value: &str, mature: bool) -> Msg {
    if let Ok(id) = value.parse::<i64>() {
        return match data.anilist.character_by_id(id, ContentPolicy::SAFE).await {
            Ok(CharacterLookup::Found(c)) => {
                let work = c
                    .media
                    .as_ref()
                    .and_then(|m| m.nodes.first())
                    .and_then(|n| n.id)
                    .map(|id| format!("anilist:{id}"));
                let mut row = components::standard_actions(ItemKind::Character, work.as_deref());
                row.push(components::add_button(&format!("anilist-char:{}", c.id)));
                if let Some(u) = &c.site_url {
                    row.push(link(u, i18n::BTN_ANILIST));
                }
                Msg::card(
                    cards::character_card(&c),
                    vec![serenity::CreateActionRow::Buttons(row)],
                )
            }
            Ok(CharacterLookup::Hidden) => Msg::text(i18n::HIDDEN_NOTICE),
            _ => Msg::unavailable(),
        };
    }
    if let Some((kind, work, person)) = tm::parse_cast_key(value) {
        let Some(w) = data.tmdb.details(kind, work).await else {
            return Msg::unavailable();
        };
        let Some(c) = w.cast.iter().find(|c| c.person_id == person) else {
            return Msg::unavailable();
        };
        let work_key = w.key();
        let mut row = components::standard_actions(ItemKind::Character, Some(&work_key));
        row.push(components::add_button(value));
        return Msg::card(
            tmdb::card::cast_embed(c, &w.title, &w.url()),
            vec![serenity::CreateActionRow::Buttons(row)],
        );
    }
    match data.providers.character_by_key(value, mature).await {
        Some(w) => {
            let cover = data.providers.cover(&w).await;
            let name = cover
                .as_ref()
                .map_or("cover.jpg", |c| crate::commands::fallback::cover_name(c));
            let e = crate::commands::fallback::card(&w, cover.is_some(), name);
            // "أظهر العمل": the game (or novel) card, as a new message.
            let mut row = components::standard_actions(ItemKind::Character, w.work_key.as_deref());
            row.push(components::add_button(value));
            if let Some(u) = &w.url {
                row.push(link(u, w.source.credit()));
            }
            let mut m = Msg::card(e, vec![serenity::CreateActionRow::Buttons(row)]);
            if let Some(c) = cover {
                m.files.push(serenity::CreateAttachment::bytes(
                    c.to_vec(),
                    name.to_string(),
                ));
            }
            m
        }
        None => Msg::unavailable(),
    }
}

/// Toggle the reminder of a work; the answer says what happened.
pub async fn remind(data: &Data, key: &str, user: i64, guild: Option<i64>) -> String {
    let overrides = guild_overrides(data, guild).await;
    let (work_id, title, category) = if let Some(id) = key
        .strip_prefix("anilist:")
        .and_then(|n| n.parse::<i64>().ok())
    {
        match data.anilist.media_by_id(id, ContentPolicy::SAFE).await {
            Ok(Some(m)) => (
                id,
                m.title.main().to_string(),
                category_of(kind_of(&m)).to_string(),
            ),
            _ => return i18n::SHOW_WORK_UNAVAILABLE.to_string(),
        }
    } else if let Some((kind, id)) = tm::parse_work_key(key) {
        match data.tmdb.details(kind, id).await {
            Some(w) => (
                tm::reminder_id(kind, id),
                w.title.clone(),
                tmdb_category(&w, &overrides).to_string(),
            ),
            None => return i18n::SHOW_WORK_UNAVAILABLE.to_string(),
        }
    } else {
        return i18n::REMINDER_UNSUPPORTED.to_string();
    };
    let r = match reminders::has(&data.db, user, work_id).await {
        Ok(true) => reminders::remove(&data.db, user, work_id)
            .await
            .map(|_| i18n::REMINDER_CANCELLED),
        Ok(false) => reminders::add_keyed(&data.db, user, work_id, &title, key, &category)
            .await
            .map(|r| match r {
                reminders::AddResult::Full => i18n::REMINDER_FULL,
                _ => i18n::REMINDER_SET,
            }),
        Err(e) => Err(e),
    };
    r.unwrap_or_else(|e| {
        tracing::warn!("reminder failed: {e}");
        i18n::ERR_GENERIC
    })
    .to_string()
}

/// Add a work or a character to the member's profile (or say it is already there).
pub async fn add(data: &Data, key: &str, user: i64, mature: bool) -> String {
    match add_inner(data, key, user, mature).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("add to profile failed: {e:#}");
            i18n::ERR_GENERIC.to_string()
        }
    }
}

async fn add_inner(data: &Data, key: &str, user: i64, mature: bool) -> Result<String, Error> {
    let pool = &data.db;
    pdb::create_profile(pool, user).await?;
    let now = crate::commands::profile::now();
    if let Some(id) = key
        .strip_prefix("anilist-char:")
        .and_then(|n| n.parse::<i64>().ok())
    {
        let crate::anilist::CharacterLookup::Found(c) = data
            .anilist
            .character_by_id(id, ContentPolicy::SAFE)
            .await?
        else {
            return Ok(i18n::SHOW_WORK_UNAVAILABLE.to_string());
        };
        pdb::upsert_cache(
            pool,
            &pdb::CacheRow {
                character_id: c.id,
                name_romaji: c.display_name().to_string(),
                name_native: c.name.native.clone(),
                image_url: c.image_url().map(str::to_string),
                is_safe: true,
                main_work: c.main_work().map(str::to_string),
                main_work_url: None,
                favourites: c.favourites,
                fetched_at: now,
            },
        )
        .await?;
        return Ok(added_text(
            pdb::add_character(pool, user, c.id).await?,
            c.display_name(),
        ));
    }
    if let Some((kind, work, person)) = tm::parse_cast_key(key) {
        let Some(w) = data.tmdb.details(kind, work).await else {
            return Ok(i18n::SHOW_WORK_UNAVAILABLE.to_string());
        };
        let (Some(c), Some(cid)) = (
            w.cast.iter().find(|c| c.person_id == person),
            tm::encode_cast_id(kind, work, person),
        ) else {
            return Ok(i18n::SHOW_WORK_UNAVAILABLE.to_string());
        };
        pdb::upsert_cache(
            pool,
            &pdb::CacheRow {
                character_id: cid,
                name_romaji: c.display_name().to_string(),
                name_native: None,
                image_url: c.photo.clone(),
                is_safe: true,
                main_work: Some(w.title.clone()),
                main_work_url: Some(w.url()),
                favourites: None,
                fetched_at: now,
            },
        )
        .await?;
        return Ok(added_text(
            pdb::add_character(pool, user, cid).await?,
            c.display_name(),
        ));
    }
    if key.starts_with("igdb:c") || key.starts_with("vndb:c") {
        return match data.providers.character_by_key(key, mature).await {
            Some(w) => Ok(crate::commands::fallback::add_to_profile(pool, user, &w)
                .await
                .text),
            None => Ok(i18n::SHOW_WORK_UNAVAILABLE.to_string()),
        };
    }
    if let Some(id) = key
        .strip_prefix("anilist:")
        .and_then(|n| n.parse::<i64>().ok())
    {
        let Some(m) = data.anilist.media_by_id(id, ContentPolicy::SAFE).await? else {
            return Ok(i18n::SHOW_WORK_UNAVAILABLE.to_string());
        };
        works::upsert_media_cache(pool, &media_cache_row(&m, now)).await?;
        let kind = kind_of(&m).key().to_string();
        return Ok(work_added(
            works::add_work(pool, user, &work_key_of(&m), &kind).await?,
        ));
    }
    if let Some((kind, id)) = tm::parse_work_key(key) {
        let Some(w) = data.tmdb.details(kind, id).await else {
            return Ok(i18n::SHOW_WORK_UNAVAILABLE.to_string());
        };
        works::upsert_media_cache(pool, &tmdb_cache_row(&w, now)).await?;
        return Ok(work_added(
            works::add_work(pool, user, key, kind.key()).await?,
        ));
    }
    if let Some(w) = data.providers.work_by_key(key, mature).await {
        if w.kind != PKind::Character {
            return Ok(crate::commands::fallback::add_to_profile(pool, user, &w)
                .await
                .text);
        }
    }
    Ok(i18n::SHOW_WORK_UNAVAILABLE.to_string())
}

fn added_text(outcome: pdb::AddOutcome, name: &str) -> String {
    match outcome {
        pdb::AddOutcome::Added(pos) => i18n::added_character(name, pos),
        pdb::AddOutcome::Duplicate => i18n::ALREADY_ADDED.to_string(),
    }
}

fn work_added(outcome: pdb::AddOutcome) -> String {
    match outcome {
        pdb::AddOutcome::Added(_) => i18n::WORK_ADDED.to_string(),
        pdb::AddOutcome::Duplicate => i18n::WORK_ALREADY.to_string(),
    }
}

/// The lightweight metadata the profile shows when TMDB is unreachable.
pub fn tmdb_cache_row(w: &tm::Work, now: i64) -> works::MediaCacheRow {
    works::MediaCacheRow {
        work_id: w.key(),
        title_romaji: w.title.clone(),
        title_english: None,
        cover_url: w.poster.clone(),
        kind: w.kind.map_or("tmdb-tv", |k| k.key()).to_string(),
        format: None,
        country: w.countries.first().cloned(),
        year: w.year.map(i64::from),
        score: w.rating.map(|r| (r * 10.0).round() as i64),
        status: w.status.clone(),
        is_safe: true,
        source: "tmdb".into(),
        fetched_at: now,
    }
}

pub async fn set_status(data: &Data, key: &str, value: &str, user: i64) -> String {
    let written = match key
        .strip_prefix("anilist:")
        .and_then(|n| n.parse::<i64>().ok())
    {
        Some(id) => {
            matches!(data.anilist.media_by_id(id, ContentPolicy::SAFE).await, Ok(Some(m)) if kind_of(&m).is_written())
        }
        None => false,
    };
    let r = if value == "none" {
        status::clear(&data.db, user, key).await
    } else {
        status::set(&data.db, user, key, value).await
    };
    match r {
        Ok(()) if value == "none" => i18n::STATUS_CLEARED.to_string(),
        Ok(()) => i18n::status_saved(status::label(value, written)),
        Err(e) => {
            tracing::warn!("status failed: {e}");
            i18n::ERR_GENERIC.to_string()
        }
    }
}

/// Send a message built here as the reply of a slash command.
pub async fn reply(ctx: crate::error::Context<'_>, m: Msg) -> Result<(), Error> {
    let mut reply = poise::CreateReply::default();
    if let Some(t) = m.text {
        reply = reply.content(t);
    }
    if let Some(e) = m.embed {
        reply = reply.embed(e);
    }
    for f in m.files {
        reply = reply.attachment(f);
    }
    if !m.rows.is_empty() {
        reply = reply.components(m.rows);
    }
    ctx.send(reply).await?;
    Ok(())
}

fn followup(m: Msg, ephemeral: bool) -> serenity::CreateInteractionResponseFollowup {
    let mut f = serenity::CreateInteractionResponseFollowup::new().ephemeral(ephemeral);
    if let Some(t) = m.text {
        f = f.content(t);
    }
    if let Some(e) = m.embed {
        f = f.embed(e);
    }
    for file in m.files {
        f = f.add_file(file);
    }
    if !m.rows.is_empty() {
        f = f.components(m.rows);
    }
    f
}

/// Handle one press. Everything but a roster page is answered with a new message.
pub async fn handle(
    ctx: &serenity::Context,
    i: &serenity::ComponentInteraction,
    data: &Data,
    press: Press,
    ephemeral: bool,
) -> Result<(), Error> {
    let user = i.user.id.get() as i64;
    let guild = i.guild_id.map(|g| g.get() as i64);
    let mature = mature_for(data, guild).await;
    if let Press::ShowChars(key, Some(page)) = &press {
        // Page buttons of a roster message edit that (new) message.
        let m = roster_msg(data, key, *page, mature).await;
        let mut r = serenity::CreateInteractionResponseMessage::new().components(m.rows);
        r = match (m.embed, m.text) {
            (Some(e), _) => r.embed(e),
            (None, Some(t)) => r.content(t),
            _ => r,
        };
        return Ok(i
            .create_response(ctx, serenity::CreateInteractionResponse::UpdateMessage(r))
            .await?);
    }
    let defer = serenity::CreateInteractionResponse::Defer(
        serenity::CreateInteractionResponseMessage::new().ephemeral(ephemeral),
    );
    i.create_response(ctx, defer).await?;
    let (msg, private) = match press {
        Press::ShowWork(k) | Press::PickWork(k) => {
            (work_msg(data, &k, user, mature).await, ephemeral)
        }
        Press::ShowChars(k, _) => (roster_msg(data, &k, 0, mature).await, ephemeral),
        Press::PickChar(v) => (character_msg(data, &v, mature).await, ephemeral),
        Press::Remind(k) => (Msg::text(remind(data, &k, user, guild).await), true),
        Press::Add(k) => (Msg::text(add(data, &k, user, mature).await), true),
        Press::Status(k, v) => (Msg::text(set_status(data, &k, &v, user).await), true),
    };
    i.create_followup(ctx, followup(msg, private)).await?;
    Ok(())
}
