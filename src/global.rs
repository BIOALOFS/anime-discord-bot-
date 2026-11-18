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
#[derive(Default, Clone)]
pub struct Msg {
    pub text: Option<String>,
    pub embed: Option<serenity::CreateEmbed>,
    pub files: Vec<serenity::CreateAttachment>,
    pub rows: Vec<serenity::CreateActionRow>,
}

impl Msg {
    pub(crate) fn text(t: impl Into<String>) -> Self {
        Self {
            text: Some(t.into()),
            ..Self::default()
        }
    }

    pub(crate) fn card(e: Emb, rows: Vec<serenity::CreateActionRow>) -> Self {
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
    in_profile: bool,
) -> Vec<serenity::CreateActionRow> {
    let mut row = components::standard_actions(ItemKind::Work, Some(key));
    row.push(add_button_for(key, in_profile));
    if remindable {
        row.push(components::remind_button(key));
    }
    if let Some(u) = url {
        row.push(link(u, credit));
    }
    let (mut rows, _links) = components::card_rows(row);
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
    if let Some((wiki, page)) = crate::wiki::parse_key(key) {
        return crate::wiki::msgs::page_msg(&data.wikis, wiki, page, user).await;
    }
    let current = status::get(&data.db, user, key).await.ok().flatten();
    let mine = in_profile(data, user, key).await;
    if let Some(id) = key
        .strip_prefix("anilist:")
        .and_then(|n| n.parse::<i64>().ok())
    {
        return match data.anilist.media_by_id(id, ContentPolicy::SAFE).await {
            Ok(Some(m)) => {
                let kind = kind_of(&m);
                let mut card = cards::work_card(&m);
                // A card that lacks a picture or a text may get them from a matching wiki page (empty fields only).
                if let Some(fill) = data
                    .wikis
                    .fill_within(
                        m.title.main(),
                        None,
                        crate::wiki::Kind::Other,
                        crate::wiki::FILL_BUDGET,
                    )
                    .await
                {
                    crate::wiki::fill::enrich(&mut card, false, &fill);
                }
                Msg::card(
                    card,
                    work_rows(
                        key,
                        m.site_url.as_deref(),
                        i18n::BTN_ANILIST,
                        kind != MediaKind::Novel,
                        kind.is_written(),
                        current.as_deref(),
                        mine,
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
        let mut e = tmdb::card::work_embed(&w, data.tmdb.region(), false);
        if let Some(fill) = data
            .wikis
            .fill_within(
                &w.title,
                None,
                crate::wiki::Kind::Other,
                crate::wiki::FILL_BUDGET,
            )
            .await
        {
            crate::wiki::fill::enrich(&mut e, w.poster.is_some(), &fill);
        }
        return Msg::card(
            e,
            work_rows(
                key,
                Some(&w.url()),
                "TMDB",
                true,
                false,
                current.as_deref(),
                mine,
            ),
        );
    }
    match data.providers.work_by_key(key, mature).await {
        Some(w) => {
            let cover = data.providers.cover(&w).await;
            let name = cover
                .as_ref()
                .map_or("cover.jpg", |c| crate::commands::fallback::cover_name(c));
            let mut e = crate::commands::fallback::card(&w, cover.is_some(), name);
            // A game that lacks a picture or a text may get them from a matching wiki page (empty fields only,
            // within a short budget; the lookup finishes in the background for the next view).
            if let Some(fill) = data
                .wikis
                .fill_within(
                    &w.title,
                    None,
                    crate::wiki::Kind::Game,
                    crate::wiki::FILL_BUDGET,
                )
                .await
            {
                crate::wiki::fill::enrich(&mut e, cover.is_some(), &fill);
            }
            let mut m = Msg::card(
                e,
                work_rows(
                    key,
                    w.url.as_deref(),
                    w.source.credit(),
                    false,
                    false,
                    current.as_deref(),
                    mine,
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

/// Add to a roster menu the wiki characters whose series matches the work strongly (cache only), skipping names the
/// roster already has. A work's roster from AniList, TMDB or a game provider thus also shows what the owner's wikis know.
async fn add_wiki_characters(data: &Data, work_title: &str, items: &mut Vec<(String, String)>) {
    let room = 25usize.saturating_sub(items.len());
    if room == 0 {
        return;
    }
    let now = chrono::Utc::now().timestamp();
    let found = crate::wiki::store::characters_for_work(&data.db, work_title, now, 50)
        .await
        .unwrap_or_default();
    let known: std::collections::HashSet<String> = items
        .iter()
        .map(|(l, _)| crate::wiki::normalize(l))
        .collect();
    items.extend(
        found
            .into_iter()
            .filter(|p| !known.contains(&crate::wiki::normalize(&p.title)))
            .take(room)
            .map(|p| (crate::ui::embeds::fit(&p.title, 100), p.key())),
    );
}

/// One page of the characters of a work (0-based), as a message with a menu of them and page buttons.
pub async fn roster_msg(data: &Data, key: &str, page: usize, mature: bool) -> Msg {
    if let Some((wiki, game)) = crate::wiki::parse_key(key) {
        return crate::wiki::msgs::roster_msg(&data.wikis, wiki, game).await;
    }
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
        let mut items: Vec<(String, String)> = p
            .items
            .iter()
            .map(|(c, _)| {
                (
                    crate::ui::embeds::fit(c.display_name(), 100),
                    c.id.to_string(),
                )
            })
            .collect();
        if page == 0 {
            add_wiki_characters(data, m.title.main(), &mut items).await;
        }
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
        let mut items: Vec<(String, String)> = w
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
        add_wiki_characters(data, &w.title, &mut items).await;
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
    let mut items: Vec<(String, String)> = slice
        .iter()
        .map(|c| (crate::ui::embeds::fit(&c.title, 100), c.id.clone()))
        .collect();
    if page == 0 {
        add_wiki_characters(data, &game.title, &mut items).await;
    }
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

/// Open any item by its key: a person, a work or a character.
pub async fn open_any(data: &Data, key: &str, user: i64, mature: bool) -> Msg {
    if crate::people::parse_key(key).is_some() {
        crate::people::msgs::person_msg(data, key, mature, user).await
    } else if components::is_work_key(key) {
        work_msg(data, key, user, mature).await
    } else {
        character_msg(data, key, mature, user).await
    }
}

async fn search_params(
    data: &Data,
    guild: Option<i64>,
    cat: crate::search::Cat,
    ty: crate::search::Ty,
    src: crate::search::Src,
) -> crate::search::Params {
    let games_on = match guild {
        Some(g) => crate::db::custom::games_enabled(&data.db, g)
            .await
            .unwrap_or(true),
        None => true,
    };
    crate::search::Params {
        cat,
        ty,
        mature: mature_for(data, guild).await,
        games_on,
        live_wikis: true,
        src,
    }
}

/// The result list of the general search for `q` (one page), as it was typed.
pub async fn search_msg(
    data: &Data,
    guild: Option<i64>,
    cat: crate::search::Cat,
    ty: crate::search::Ty,
    src: crate::search::Src,
    q: &str,
    page: usize,
) -> Msg {
    let p = search_params(data, guild, cat, ty, src).await;
    let overrides = guild_overrides(data, guild).await;
    let found =
        crate::search::gather(&crate::people::fetch::Sources::of(data), &overrides, p, q).await;
    crate::search::results_msg(&found, p, q, page)
}

/// The submit of the general search: a picked key opens its card; a typed name with exactly one result opens that
/// result; otherwise the grouped result list.
pub async fn search_or_open(
    data: &Data,
    guild: Option<i64>,
    user: i64,
    cat: crate::search::Cat,
    ty: crate::search::Ty,
    src: crate::search::Src,
    q: &str,
) -> Msg {
    let p = search_params(data, guild, cat, ty, src).await;
    if crate::search::is_key(q) {
        return open_any(data, q.trim(), user, p.mature).await;
    }
    let overrides = guild_overrides(data, guild).await;
    let found =
        crate::search::gather(&crate::people::fetch::Sources::of(data), &overrides, p, q).await;
    match found.entries.as_slice() {
        [one] if found.failed.is_empty() => open_any(data, &one.key, user, p.mature).await,
        _ => crate::search::results_msg(&found, p, q, 0),
    }
}

/// Does anyone voice this character (AniList and VNDB look it up; the answer is cached by the sources)?
async fn has_performers(data: &Data, key: &str, mature: bool) -> bool {
    !crate::people::fetch::performers(&crate::people::fetch::Sources::of(data), key, mature)
        .await
        .is_empty()
}

/// The card of a character of any source. `value` is an AniList character id, a `tmdb-cast:` key or a provider key.
pub async fn character_msg(data: &Data, value: &str, mature: bool, user: i64) -> Msg {
    if let Some((wiki, page)) = crate::wiki::parse_key(value) {
        return crate::wiki::msgs::page_msg(&data.wikis, wiki, page, user).await;
    }
    // A person added to a profile is listed with the characters: its stored id opens the person card.
    if crate::people::parse_key(value).is_some() {
        return crate::people::msgs::person_msg(data, value, mature, user).await;
    }
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
                let add_key = format!("anilist-char:{}", c.id);
                row.push(add_button_for(
                    &add_key,
                    in_profile(data, user, &add_key).await,
                ));
                if has_performers(data, &id.to_string(), mature).await {
                    row.push(components::performer_button(&id.to_string()));
                }
                if let Some(u) = &c.site_url {
                    row.push(link(u, i18n::BTN_ANILIST));
                }
                Msg::card(cards::character_card(&c), components::card_rows(row).0)
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
        row.push(add_button_for(value, in_profile(data, user, value).await));
        // The actor is always known for a cast entry.
        row.push(components::performer_button(value));
        return Msg::card(
            tmdb::card::cast_embed(c, &w.title, &w.url()),
            components::card_rows(row).0,
        );
    }
    match data.providers.character_by_key(value, mature).await {
        Some(w) => {
            let cover = data.providers.cover(&w).await;
            let name = cover
                .as_ref()
                .map_or("cover.jpg", |c| crate::commands::fallback::cover_name(c));
            let mut e = crate::commands::fallback::card(&w, cover.is_some(), name);
            // The same fill for a character, only for a strong match (same name AND same game).
            if let Some(fill) = data
                .wikis
                .fill_within(
                    &w.title,
                    w.subtitle.as_deref(),
                    crate::wiki::Kind::Character,
                    crate::wiki::FILL_BUDGET,
                )
                .await
            {
                crate::wiki::fill::enrich(&mut e, cover.is_some(), &fill);
            }
            // "أظهر العمل": the game (or novel) card, as a new message.
            let mut row = components::standard_actions(ItemKind::Character, w.work_key.as_deref());
            row.push(add_button_for(value, in_profile(data, user, value).await));
            if w.source == crate::providers::Source::Vndb
                && has_performers(data, value, mature).await
            {
                row.push(components::performer_button(value));
            }
            if let Some(u) = &w.url {
                row.push(link(u, w.source.credit()));
            }
            let mut m = Msg::card(e, components::card_rows(row).0);
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
/// What a key stands for in a profile: a character (its stored integer id) or a work (its key).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    Character(i64),
    Work(String),
}

/// The profile slots a key can occupy (a wiki page is a character or a work, so both are checked). Real people are
/// characters in a profile; companies have no slot.
pub fn slots_of(key: &str) -> Vec<Slot> {
    if let Some(n) = key
        .strip_prefix("anilist-char:")
        .and_then(|n| n.parse::<i64>().ok())
    {
        return vec![Slot::Character(n)];
    }
    if let Some((kind, work, person)) = tm::parse_cast_key(key) {
        return tm::encode_cast_id(kind, work, person)
            .map(Slot::Character)
            .into_iter()
            .collect();
    }
    for (prefix, source) in [
        ("igdb:c", crate::providers::Source::Igdb),
        ("vndb:c", crate::providers::Source::Vndb),
    ] {
        if let Some(n) = key.strip_prefix(prefix).and_then(|n| n.parse::<i64>().ok()) {
            return crate::providers::encode_character_id(source, n)
                .map(Slot::Character)
                .into_iter()
                .collect();
        }
    }
    if crate::people::parse_key(key).is_some() {
        return crate::people::encode_person_id(key)
            .map(Slot::Character)
            .into_iter()
            .collect();
    }
    if let Some((w, p)) = crate::wiki::parse_key(key) {
        let mut v = vec![Slot::Work(key.to_string())];
        v.extend(crate::wiki::profile::encode_id(w, p).map(Slot::Character));
        return v;
    }
    if key.parse::<i64>().is_ok() {
        return key.parse().map(Slot::Character).into_iter().collect();
    }
    if crate::components::is_work_key(key) {
        return vec![Slot::Work(key.to_string())];
    }
    Vec::new()
}

/// Is this item in the member's profile? (Drives the add / remove button and the toggle.)
pub async fn in_profile(data: &Data, user: i64, key: &str) -> bool {
    in_profile_pool(&data.db, user, key).await
}

pub async fn in_profile_pool(pool: &sqlx::SqlitePool, user: i64, key: &str) -> bool {
    for slot in slots_of(key) {
        let found = match slot {
            Slot::Character(id) => pdb::get_entry(pool, user, id)
                .await
                .ok()
                .flatten()
                .is_some(),
            Slot::Work(k) => works::get_entry(pool, user, &k)
                .await
                .ok()
                .flatten()
                .is_some(),
        };
        if found {
            return true;
        }
    }
    false
}

/// Take the item out of the profile; the text says what happened.
async fn remove_from_profile(data: &Data, user: i64, key: &str) -> Option<String> {
    for slot in slots_of(key) {
        match slot {
            Slot::Character(id) => {
                if pdb::remove_character(&data.db, user, id)
                    .await
                    .unwrap_or(false)
                {
                    return Some(i18n::CHARACTER_REMOVED.to_string());
                }
            }
            Slot::Work(k) => {
                if works::remove_work(&data.db, user, &k)
                    .await
                    .unwrap_or(false)
                {
                    return Some(i18n::WORK_REMOVED.to_string());
                }
            }
        }
    }
    None
}

/// The add button of a card: "إضافة" or, when the item is already in the profile, "إزالة".
pub fn add_button_for(key: &str, in_profile: bool) -> serenity::CreateButton {
    if in_profile {
        serenity::CreateButton::new(components::add_id(key))
            .label(i18n::BTN_REMOVE_FROM_PROFILE)
            .emoji('➖')
            .style(serenity::ButtonStyle::Secondary)
    } else {
        components::add_button(key)
    }
}

pub async fn add(data: &Data, key: &str, user: i64, mature: bool) -> String {
    // The same button adds or removes, as its label says.
    if in_profile(data, user, key).await {
        if let Some(text) = remove_from_profile(data, user, key).await {
            return text;
        }
    }
    match add_inner(data, key, user, mature).await {
        // A newly earned badge is announced in the same private answer (nothing when the item was already there).
        Ok(t) if t.starts_with("أضفتُ") || t == i18n::WORK_ADDED => {
            crate::features::badges::announce(&data.db, user, t).await
        }
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
    if let Some((w, p)) = crate::wiki::parse_key(key) {
        // A wiki page: a character goes to the characters list, anything else is a work. A snapshot is kept.
        let Some((page, source)) = data.wikis.page(w, p).await else {
            return Ok(i18n::SHOW_WORK_UNAVAILABLE.to_string());
        };
        return Ok(if crate::wiki::profile::is_character(&page) {
            match crate::wiki::profile::add_character(pool, user, &page, &source, now).await? {
                Some(outcome) => added_text(outcome, &page.title),
                None => i18n::ERR_GENERIC.to_string(),
            }
        } else {
            work_added(crate::wiki::profile::add_work(pool, user, &page, &source, now).await?)
        });
    }
    if crate::people::parse_key(key).is_some() {
        // A person is added as a character (companies cannot be).
        if !crate::people::addable(key) {
            return Ok(i18n::PERSON_NOT_ADDABLE.to_string());
        }
        let crate::people::fetch::Card::Found(p) =
            crate::people::fetch::person(&crate::people::fetch::Sources::of(data), key, mature)
                .await
        else {
            return Ok(i18n::SHOW_WORK_UNAVAILABLE.to_string());
        };
        return Ok(
            match crate::people::profile::add(pool, user, &p, now).await? {
                Some(outcome) => added_text(outcome, &p.name),
                None => i18n::PERSON_NOT_ADDABLE.to_string(),
            },
        );
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

/// Send a message built here as the reply of a slash command. It is an interactive message like any other: it gets the
/// same navigation (edit in place with Back for the owner, private views for others) and lifetime.
pub async fn reply(ctx: crate::error::Context<'_>, m: Msg) -> Result<(), Error> {
    crate::ui::session::run(ctx, crate::nav::overlay::MsgView(m)).await
}

/// Is this press a page button of a list that is already open (it replaces the view on screen instead of opening a
/// new one)?
pub fn replaces_view(p: &Press) -> bool {
    matches!(
        p,
        Press::ShowChars(_, Some(_))
            | Press::ShowWorks(_, Some(_))
            | Press::ShowCrew(_, Some(_))
            | Press::SearchPage(..)
    )
}

/// The view a navigation press asks for (`None` for presses that act instead: reminders, add, status, Back).
pub async fn view_msg(data: &Data, press: &Press, user: i64, guild: Option<i64>) -> Option<Msg> {
    let mature = mature_for(data, guild).await;
    Some(match press {
        Press::ShowWork(k) | Press::PickWork(k) => work_msg(data, k, user, mature).await,
        Press::ShowChars(k, page) => roster_msg(data, k, page.unwrap_or(0), mature).await,
        Press::PickAny(k) => open_any(data, k, user, mature).await,
        Press::SearchPage(cat, ty, src, page, q) => {
            search_msg(data, guild, *cat, *ty, *src, q, *page).await
        }
        Press::PickPerson(k) => crate::people::msgs::person_msg(data, k, mature, user).await,
        Press::ShowWorks(k, page) => {
            crate::people::msgs::works_msg(data, k, page.unwrap_or(0), mature).await
        }
        Press::ShowCrew(k, page) => {
            crate::people::msgs::crew_msg(data, k, page.unwrap_or(0), mature).await
        }
        Press::ShowPerformer(k) => crate::people::msgs::performers_msg(data, k, mature, user).await,
        Press::PickChar(v) => character_msg(data, v, mature, user).await,
        Press::ShowWorkOf(k) => crate::wiki::msgs::work_of_msg(data, k, mature, user).await,
        Press::Remind(_) | Press::Add(_) | Press::Status(..) | Press::Back => return None,
    })
}

/// The text answer of a press that acts for the presser (reminder, add to profile, status).
pub async fn act_text(data: &Data, press: &Press, user: i64, guild: Option<i64>) -> Option<String> {
    let mature = mature_for(data, guild).await;
    Some(match press {
        Press::Remind(k) => remind(data, k, user, guild).await,
        Press::Add(k) => add(data, k, user, mature).await,
        Press::Status(k, v) => set_status(data, k, v, user).await,
        _ => return None,
    })
}

#[cfg(test)]
mod wiki_profile_tests {
    use super::*;
    use crate::wiki::{store, Kind, Page};

    async fn data_with_page(kind: Kind) -> (Data, i64) {
        let data = Data::for_tests(|_, _| Err(crate::anilist::AniError::NotFound)).await;
        let id = match store::insert(
            &data.db,
            &store::NewSource {
                name: "Mudae Wiki".into(),
                host: "mudae.fandom.com".into(),
                api_url: "https://mudae.fandom.com/api.php".into(),
                lang: "en".into(),
                license_name: Some("CC BY-SA 3.0".into()),
                license_url: None,
                character_category: Some("Category:Characters".into()),
                game_category: None,
                added_by: None,
            },
        )
        .await
        .expect("source")
        {
            store::Added::New(id) => id,
            other => panic!("{other:?}"),
        };
        data.wikis.invalidate().await;
        store::upsert(
            &data.db,
            &Page {
                wiki_id: id,
                page_id: 77,
                title: "Acerola".into(),
                kind,
                extract: Some("She is the trial captain.".into()),
                image_url: None,
                aliases: vec![],
                parent_title: Some("Pokémon Sun/Moon".into()),
                categories: vec![],
            },
            chrono::Utc::now().timestamp(),
        )
        .await
        .expect("page");
        (data, id)
    }

    #[test]
    fn every_kind_of_key_maps_to_its_profile_slot() {
        assert_eq!(slots_of("anilist-char:10"), vec![Slot::Character(10)]);
        assert_eq!(slots_of("anilist:5"), vec![Slot::Work("anilist:5".into())]);
        assert_eq!(
            slots_of("tmdb-tv:1396"),
            vec![Slot::Work("tmdb-tv:1396".into())]
        );
        assert!(matches!(
            slots_of("tmdb-cast:tv:1396:17419").as_slice(),
            [Slot::Character(_)]
        ));
        assert!(
            matches!(slots_of("vndb:c12").as_slice(), [Slot::Character(n)] if *n > 1_000_000_000_000)
        );
        assert!(
            matches!(slots_of("igdb:c12").as_slice(), [Slot::Character(n)] if *n > 2_000_000_000_000)
        );
        assert!(matches!(
            slots_of("tmdb-person:7").as_slice(),
            [Slot::Character(_)]
        ));
        assert!(
            slots_of("igdb-company:7").is_empty(),
            "companies have no slot"
        );
        let wiki = slots_of("wiki:2:77");
        assert_eq!(
            wiki.len(),
            2,
            "a wiki page is a work or a character, whichever it turns out to be"
        );
        assert!(wiki.contains(&Slot::Work("wiki:2:77".into())));
        assert!(slots_of("nonsense").is_empty());
    }

    #[tokio::test]
    async fn the_same_button_adds_a_wiki_character_and_then_removes_it() {
        let (data, id) = data_with_page(Kind::Character).await;
        let key = crate::wiki::key(id, 77);
        assert!(!in_profile(&data, 5, &key).await);
        let added = add(&data, &key, 5, true).await;
        assert!(added.starts_with("أضفتُ"), "{added}");
        assert!(in_profile(&data, 5, &key).await);
        // the button now says remove, with the same id
        let b = serde_json::to_value(add_button_for(&key, true)).expect("json");
        assert_eq!(b["label"], "إزالة من بروفايلي");
        assert_eq!(add(&data, &key, 5, true).await, i18n::CHARACTER_REMOVED);
        assert!(!in_profile(&data, 5, &key).await);
        // another member's profile is not involved
        assert!(!in_profile(&data, 6, &key).await);
    }

    #[tokio::test]
    async fn a_wiki_game_is_added_as_a_work_and_removed_again() {
        let (data, id) = data_with_page(Kind::Game).await;
        let key = crate::wiki::key(id, 77);
        assert_eq!(add(&data, &key, 5, true).await, i18n::WORK_ADDED);
        assert!(in_profile(&data, 5, &key).await);
        assert_eq!(add(&data, &key, 5, true).await, i18n::WORK_REMOVED);
        assert!(!in_profile(&data, 5, &key).await);
    }

    #[tokio::test]
    async fn the_card_of_an_added_item_survives_the_wiki_through_its_snapshot() {
        let (data, id) = data_with_page(Kind::Character).await;
        let key = crate::wiki::key(id, 77);
        add(&data, &key, 5, true).await;
        // the wiki is switched off for everyone and the cache is emptied
        store::set_enabled(&data.db, id, false).await.expect("off");
        data.wikis.invalidate().await;
        store::clear_cache(&data.db).await.expect("clear");
        let m = crate::wiki::msgs::page_msg(&data.wikis, id, 77, 5).await;
        let e = serde_json::to_value(m.embed.expect("a card from the snapshot")).expect("json");
        assert_eq!(e["title"], "Acerola");
        assert!(
            e["footer"]["text"]
                .as_str()
                .unwrap_or("")
                .contains("Mudae Wiki"),
            "the source is named"
        );
        assert!(e["footer"]["text"]
            .as_str()
            .unwrap_or("")
            .contains(i18n::WIKI_SNAPSHOT_NOTE));
        assert!(e["description"]
            .as_str()
            .unwrap_or("")
            .contains("📚 Pokémon Sun/Moon"));
        // a member who did not add it gets the same card (the snapshot is the source), with the add button
        assert!(!m.rows.is_empty());
        // a page nobody saved is unavailable once the wiki is gone
        let none = crate::wiki::msgs::page_msg(&data.wikis, id, 999, 5).await;
        assert_eq!(none.text.as_deref(), Some(i18n::SHOW_WORK_UNAVAILABLE));
    }
}
