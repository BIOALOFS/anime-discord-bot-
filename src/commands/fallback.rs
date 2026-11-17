//! Cards for works that only a fallback provider (MangaDex, MangaUpdates) has. Never reached unless
//! `ENABLE_FALLBACK_PROVIDERS=true`. The source is credited and the cover is attached, not hotlinked.

use super::buttons;
use crate::{
    db::{profile as db, works},
    error::{Context, Error},
    i18n,
    providers::{Kind, Providers, Work},
    ui::{
        embeds::Emb,
        pagination::{back_button, close_button, BACK, CLOSE},
        select::{select_row, Choice, PICK_ID},
        session::{self, Outcome, Page, UndoFuture, View, Whisper},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::sync::Arc;

pub const BTN_PROVIDER_ADD: &str = "provider_add";

/// Per-server switches for game sources: `(games on, mature games on)`. Outside a server both default on.
pub async fn game_flags(ctx: Context<'_>) -> (bool, bool) {
    let Some(g) = ctx.guild_id() else {
        return (true, true);
    };
    let pool = &ctx.data().db;
    let g = g.get() as i64;
    (
        crate::db::custom::games_enabled(pool, g)
            .await
            .unwrap_or(true),
        crate::db::custom::mature_games(pool, g)
            .await
            .unwrap_or(true),
    )
}

/// The "add to my profile" button of a provider card.
pub fn add_button(enabled: bool, in_profile: bool) -> serenity::CreateButton {
    serenity::CreateButton::new(BTN_PROVIDER_ADD)
        .label(if in_profile {
            i18n::BTN_REMOVE_FROM_PROFILE
        } else {
            i18n::BTN_ADD_TO_PROFILE
        })
        .emoji(if in_profile { '➖' } else { '➕' })
        .style(serenity::ButtonStyle::Secondary)
        .disabled(!enabled)
}

/// Add a provider work or character to the user's profile (with an undo on the confirmation).
pub async fn add_to_profile(pool: &SqlitePool, user: i64, w: &Work) -> Whisper {
    match add_inner(pool, user, w).await {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!("could not add provider item to profile: {e:#}");
            Whisper::text(i18n::ERR_GENERIC)
        }
    }
}

async fn add_inner(pool: &SqlitePool, user: i64, w: &Work) -> Result<Whisper, Error> {
    db::create_profile(pool, user).await?;
    let now = super::profile::now();
    if w.kind == Kind::Character {
        let Some(cid) = w.character_id() else {
            return Ok(Whisper::text(i18n::ERR_GENERIC));
        };
        db::upsert_cache(
            pool,
            &db::CacheRow {
                character_id: cid,
                name_romaji: w.title.clone(),
                name_native: w.native.clone(),
                image_url: w.cover_url.clone(),
                // Provider characters are vetted before they are ever shown.
                is_safe: true,
                main_work: w.subtitle.clone(),
                main_work_url: None,
                favourites: None,
                fetched_at: now,
            },
        )
        .await?;
        return Ok(match db::add_character(pool, user, cid).await? {
            db::AddOutcome::Added(pos) => {
                let undo_pool = pool.clone();
                let undo: Box<dyn FnOnce() -> UndoFuture + Send + Sync> = Box::new(move || {
                    Box::pin(async move {
                        db::remove_character(&undo_pool, user, cid)
                            .await
                            .map_or(i18n::ERR_GENERIC, |_| i18n::UNDONE)
                            .to_string()
                    })
                });
                Whisper {
                    text: i18n::added_character(&w.title, pos),
                    undo: Some(undo),
                }
            }
            db::AddOutcome::Duplicate => Whisper::text(i18n::ALREADY_ADDED),
        });
    }
    let kind = w.kind.work_key();
    works::upsert_media_cache(
        pool,
        &works::MediaCacheRow {
            work_id: w.id.clone(),
            title_romaji: w.title.clone(),
            title_english: None,
            cover_url: w.cover_url.clone(),
            kind: kind.to_string(),
            format: None,
            country: None,
            year: w.year.map(i64::from),
            score: w.rating.map(|r| (r * 10.0).round() as i64),
            status: w.status.clone(),
            is_safe: true,
            source: w.source.prefix().to_string(),
            fetched_at: now,
        },
    )
    .await?;
    Ok(match works::add_work(pool, user, &w.id, kind).await? {
        db::AddOutcome::Added(_) => {
            let (undo_pool, key) = (pool.clone(), w.id.clone());
            let undo: Box<dyn FnOnce() -> UndoFuture + Send + Sync> = Box::new(move || {
                Box::pin(async move {
                    works::remove_work(&undo_pool, user, &key)
                        .await
                        .map_or(i18n::ERR_GENERIC, |_| i18n::UNDONE)
                        .to_string()
                })
            });
            Whisper {
                text: i18n::WORK_ADDED.to_string(),
                undo: Some(undo),
            }
        }
        db::AddOutcome::Duplicate => Whisper::text(i18n::WORK_ALREADY),
    })
}

pub fn cover_name(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "cover.png"
    } else if bytes.starts_with(b"GIF8") {
        "cover.gif"
    } else if bytes.len() > 12 && &bytes[8..12] == b"WEBP" {
        "cover.webp"
    } else {
        "cover.jpg"
    }
}

pub fn card(w: &Work, has_cover: bool, cover_name: &str) -> Emb {
    let mut e = Emb::new().title(&w.title).url(w.url.clone());
    e = e.author(w.source.credit());
    e.footer = Some(w.source.credit().to_string());
    if has_cover {
        e = e.image(Some(format!("attachment://{cover_name}")));
    }
    if w.kind == Kind::Character {
        let mut lines = Vec::new();
        if let Some(n) = &w.native {
            lines.push(n.clone());
        }
        if let Some(from) = &w.subtitle {
            lines.push(format!("{} {from}", i18n::CHARACTER_FROM));
        }
        if !lines.is_empty() {
            e = e.description(lines.join("\n"));
        }
    } else if !w.genres.is_empty() {
        e = e.description(
            w.genres
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(" • "),
        );
    }
    match w.kind {
        Kind::Game => e = e.field(i18n::F_TYPE, i18n::KIND_GAME, true),
        Kind::VisualNovel => e = e.field(i18n::F_TYPE, i18n::KIND_VN, true),
        _ => {}
    }
    if let Some(y) = w.year {
        e = e.field(i18n::F_YEAR, y.to_string(), true);
    }
    if let Some(s) = &w.status {
        e = e.field(i18n::F_STATUS, s, true);
    }
    // The shared rating / popularity lines (`ui::rating`) per source.
    let on_100 = w.rating.map(|r| f64::from(r) * 10.0);
    match (w.kind, w.source) {
        (Kind::Character, _) => {
            e = e.field(i18n::F_STATS, crate::ui::rating::no_popularity(), false)
        }
        (_, crate::providers::Source::Igdb) => {
            e = e.field(
                i18n::F_STATS,
                crate::ui::rating::igdb_game(on_100, None, None),
                false,
            )
        }
        (_, crate::providers::Source::Vndb) => {
            e = e.field(i18n::F_STATS, crate::ui::rating::vndb(on_100, None), false)
        }
        _ => {
            if let Some(r) = w.rating {
                e = e.field(i18n::F_SCORE, format!("{r:.1}/10"), true);
            }
        }
    }
    e
}

struct FallbackView {
    pool: SqlitePool,
    user: i64,
    whisper: Option<Whisper>,
    providers: Arc<Providers>,
    works: Vec<Work>,
    chosen: Option<usize>,
    cover: Option<Arc<Vec<u8>>>,
    from_list: bool,
}

impl FallbackView {
    async fn open(&mut self, i: usize) {
        self.chosen = Some(i);
        self.cover = self.providers.cover(&self.works[i]).await;
    }
}

impl View for FallbackView {
    fn render(&self, enabled: bool) -> Page {
        match self.chosen {
            Some(i) => {
                let w = &self.works[i];
                let name = self.cover.as_ref().map_or("cover.jpg", |c| cover_name(c));
                let mut row = vec![add_button(enabled, false)];
                // Games and visual novels have characters; the written-work fallbacks do not.
                if matches!(
                    w.source,
                    crate::providers::Source::Igdb | crate::providers::Source::Vndb
                ) {
                    row.extend(crate::components::standard_actions(
                        crate::components::ItemKind::Work,
                        Some(&w.id),
                    ));
                }
                if self.from_list {
                    row.push(back_button(enabled));
                }
                if let Some(u) = &w.url {
                    row.push(
                        serenity::CreateButton::new_link(u)
                            .label(format!("🔗 {}", w.source.credit())),
                    );
                }
                row.push(close_button(enabled));
                let page = Page::embed(
                    card(w, self.cover.is_some(), name).build(),
                    vec![buttons(row)],
                );
                match &self.cover {
                    Some(c) => page.with_files(vec![serenity::CreateAttachment::bytes(
                        c.to_vec(),
                        name.to_string(),
                    )]),
                    None => page,
                }
            }
            None => {
                let body = self
                    .works
                    .iter()
                    .enumerate()
                    .map(|(i, w)| format!("**{}.** {} — {}", i + 1, w.title, w.source.credit()))
                    .collect::<Vec<_>>()
                    .join("\n");
                let choices = self
                    .works
                    .iter()
                    .enumerate()
                    .map(|(i, w)| Choice {
                        label: crate::ui::embeds::fit(&w.title, 100),
                        description: Some(w.source.credit().to_string()),
                        value: i.to_string(),
                    })
                    .collect();
                let mut rows: Vec<_> =
                    select_row(PICK_ID, i18n::PICK_PLACEHOLDER, choices, enabled)
                        .into_iter()
                        .collect();
                rows.push(buttons(vec![close_button(enabled)]));
                Page::embed(
                    Emb::new()
                        .title(i18n::PICK_PROMPT)
                        .description(body)
                        .build(),
                    rows,
                )
            }
        }
    }

    fn take_whisper(&mut self) -> Option<Whisper> {
        self.whisper.take()
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        match id {
            CLOSE => Outcome::Close,
            BTN_PROVIDER_ADD => {
                if let Some(w) = self.chosen.and_then(|i| self.works.get(i)) {
                    self.whisper = Some(add_to_profile(&self.pool, self.user, w).await);
                }
                Outcome::Keep
            }
            BACK => {
                self.chosen = None;
                self.cover = None;
                Outcome::Update
            }
            PICK_ID => match values
                .first()
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|i| *i < self.works.len())
            {
                Some(i) => {
                    self.open(i).await;
                    Outcome::Update
                }
                None => Outcome::Keep,
            },
            _ => Outcome::Keep,
        }
    }
}

/// Search the fallback providers; `Ok(false)` when disabled or nothing was found.
pub async fn run(ctx: Context<'_>, query: &str) -> Result<bool, Error> {
    let providers = ctx.data().providers.clone();
    if !providers.enabled() {
        return Ok(false);
    }
    let works = providers.search(query).await;
    if works.is_empty() {
        return Ok(false);
    }
    show(ctx, works).await?;
    Ok(true)
}

/// Show provider results: a select menu for several, the card for one.
pub async fn show(ctx: Context<'_>, works: Vec<Work>) -> Result<(), Error> {
    let mut view = FallbackView {
        pool: ctx.data().db.clone(),
        user: ctx.author().id.get() as i64,
        whisper: None,
        providers: ctx.data().providers.clone(),
        from_list: works.len() > 1,
        works,
        chosen: None,
        cover: None,
    };
    if view.works.len() == 1 {
        view.open(0).await;
    }
    session::run(ctx, view).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::Source;

    fn work() -> Work {
        Work {
            id: "mdex:a".into(),
            source: Source::MangaDex,
            kind: crate::providers::Kind::Manga,
            native: None,
            subtitle: None,
            title: "Some Series".into(),
            year: Some(2021),
            status: Some("ongoing".into()),
            rating: Some(8.25),
            url: Some("https://mangadex.org/title/a".into()),
            cover_url: Some("https://uploads.mangadex.org/covers/a/c.jpg".into()),
            genres: vec!["Action".into(), "Drama".into()],
        }
    }

    #[test]
    fn the_card_credits_the_source_and_attaches_the_cover() {
        let e = card(&work(), true, "cover.jpg");
        assert_eq!(e.footer.as_deref(), Some("Source: MangaDex"));
        assert_eq!(e.author.as_deref(), Some("Source: MangaDex"));
        assert_eq!(
            e.image.as_deref(),
            Some("attachment://cover.jpg"),
            "an attachment, never the provider's URL"
        );
        assert!(!e.image.as_deref().unwrap_or("").contains("mangadex.org"));
        let no_cover = card(&work(), false, "cover.jpg");
        assert!(no_cover.image.is_none());
        assert!(e.fields.iter().any(|f| f.1 == "2021"));
    }

    #[test]
    fn cover_file_names_follow_the_bytes() {
        assert_eq!(cover_name(b"\x89PNG..."), "cover.png");
        assert_eq!(cover_name(b"GIF89a"), "cover.gif");
        assert_eq!(cover_name(b"RIFF\0\0\0\0WEBPVP8"), "cover.webp");
        assert_eq!(cover_name(&[0xFF, 0xD8, 0xFF]), "cover.jpg");
    }
}
