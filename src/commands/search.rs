//! `/search type name`: works only (anime, manga, manhwa, manhua, novel, and games / visual novels when those
//! providers are enabled). Characters live in `/character`.

use super::{
    fallback,
    media::{media_autocomplete, run_media},
    profile::option_str,
    say_ephemeral,
};
use crate::{
    anilist::MediaKind,
    error::{Context, Error},
    i18n,
    providers::Work,
    ui::autocomplete::to_choices,
};
use poise::serenity_prelude as serenity;

/// `(label, key)` pairs of the AniList-backed types; keys are `MediaKind::key()`.
pub fn search_types() -> Vec<(&'static str, &'static str)> {
    vec![
        ("أنمي", "anime"),
        ("مانجا", "manga"),
        ("مانهوا", "manhwa"),
        ("مانهوا صينية", "manhua"),
        ("رواية", "novel"),
    ]
}

/// Types added by the optional game providers.
pub fn game_types(igdb: bool, vndb: bool) -> Vec<(&'static str, &'static str)> {
    let mut v = Vec::new();
    if igdb {
        v.push(("لعبة", "game"));
    }
    if vndb {
        v.push(("رواية بصرية", "vn"));
    }
    v
}

async fn ac_type(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    let p = &ctx.data().providers;
    let (games_on, _) = fallback::game_flags(ctx).await;
    let mut all = search_types();
    if games_on {
        all.extend(game_types(p.igdb_enabled(), p.vndb_enabled()));
    }
    let needle = partial.trim().to_lowercase();
    to_choices(
        all.into_iter()
            .filter(|(l, k)| needle.is_empty() || l.contains(&needle) || k.contains(&needle))
            .map(|(l, k)| (l.to_string(), k.to_string()))
            .collect(),
    )
}

/// Game / visual-novel suggestions: the title is the value, so the run step can find the exact entry.
async fn game_autocomplete(
    ctx: Context<'_>,
    kind: &'static str,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    let (games_on, mature) = fallback::game_flags(ctx).await;
    if !games_on {
        return Vec::new();
    }
    let providers = ctx.data().providers.clone();
    let items = ctx
        .data()
        .auto
        .items(ctx.author().id.get(), kind, partial, |q| async move {
            let found = if kind == "game" {
                providers.search_games(&q, mature).await
            } else {
                providers.search_visual_novels(&q).await
            };
            found
                .iter()
                .map(|w| {
                    let label = match w.year {
                        Some(y) => format!("{} ({y})", w.title),
                        None => w.title.clone(),
                    };
                    (label, w.title.clone())
                })
                .collect()
        })
        .await;
    to_choices(items)
}

/// The name suggestions depend on the chosen type.
async fn ac_name(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    match option_str(ctx, "type").as_deref() {
        Some("game") => game_autocomplete(ctx, "game", partial).await,
        Some("vn") => game_autocomplete(ctx, "vn", partial).await,
        Some(key) => match MediaKind::from_key(key) {
            Some(kind) => media_autocomplete(ctx, kind, kind.key(), partial).await,
            None => Vec::new(),
        },
        None => media_autocomplete(ctx, MediaKind::Anime, "anime", partial).await,
    }
}

/// Exact title matches win; otherwise every result is offered.
fn narrow(mut works: Vec<Work>, query: &str) -> Vec<Work> {
    let exact: Vec<Work> = works
        .iter()
        .filter(|w| w.title.eq_ignore_ascii_case(query.trim()))
        .cloned()
        .collect();
    if !exact.is_empty() {
        works = exact;
    }
    works.truncate(10);
    works
}

async fn run_game(ctx: Context<'_>, kind: &str, name: &str) -> Result<(), Error> {
    ctx.defer().await?;
    let (games_on, mature) = fallback::game_flags(ctx).await;
    let providers = ctx.data().providers.clone();
    let enabled = if kind == "game" {
        providers.igdb_enabled()
    } else {
        providers.vndb_enabled()
    };
    if !enabled {
        return say_ephemeral(ctx, i18n::GAMES_UNAVAILABLE).await;
    }
    if !games_on {
        return say_ephemeral(ctx, i18n::GAMES_SERVER_OFF).await;
    }
    let found = if kind == "game" {
        providers.search_games(name.trim(), mature).await
    } else {
        providers.search_visual_novels(name.trim()).await
    };
    let works = narrow(found, name);
    if works.is_empty() {
        return say_ephemeral(ctx, i18n::NO_RESULTS).await;
    }
    fallback::show(ctx, works).await
}

/// Search for an anime, manga, manhwa, manhua, novel (and games / visual novels when enabled).
#[poise::command(
    slash_command,
    name_localized("ar", "بحث"),
    description_localized("ar", "ابحث عن عمل: أنمي أو مانجا أو مانهوا أو رواية أو لعبة")
)]
pub async fn search(
    ctx: Context<'_>,
    #[description = "What to search for"]
    #[description_localized("ar", "نوع العمل")]
    #[name_localized("ar", "النوع")]
    #[autocomplete = "ac_type"]
    r#type: String,
    #[description = "Name"]
    #[description_localized("ar", "الاسم")]
    #[name_localized("ar", "الاسم")]
    #[autocomplete = "ac_name"]
    name: String,
) -> Result<(), Error> {
    let key = r#type.trim();
    if key == "game" || key == "vn" {
        return run_game(ctx, key, &name).await;
    }
    let kind = MediaKind::from_key(key).unwrap_or(MediaKind::Anime);
    run_media(ctx, kind, name).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{Kind, Source};

    fn work(title: &str) -> Work {
        Work {
            id: format!("igdb:{title}"),
            source: Source::Igdb,
            kind: Kind::Game,
            title: title.into(),
            native: None,
            subtitle: None,
            year: None,
            status: None,
            rating: None,
            url: None,
            cover_url: None,
            genres: Vec::new(),
        }
    }

    #[test]
    fn search_covers_works_only() {
        for (label, key) in search_types() {
            assert!(!label.is_empty());
            assert!(MediaKind::from_key(key).is_some(), "{key}");
        }
        assert_eq!(search_types().len(), 5, "characters moved to /character");
        assert!(!search_types().iter().any(|(_, k)| *k == "character"));
    }

    #[test]
    fn game_types_appear_only_when_enabled() {
        assert!(game_types(false, false).is_empty());
        assert_eq!(game_types(true, false), vec![("لعبة", "game")]);
        assert_eq!(game_types(false, true), vec![("رواية بصرية", "vn")]);
        assert_eq!(game_types(true, true).len(), 2);
    }

    #[test]
    fn exact_title_wins_and_results_are_capped() {
        let list = vec![
            work("Silent Hill 2"),
            work("Silent Hill"),
            work("Silent Hill 3"),
        ];
        let n = narrow(list, "silent hill");
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].title, "Silent Hill");
        let many: Vec<Work> = (0..30).map(|i| work(&format!("g{i}"))).collect();
        assert_eq!(narrow(many, "zzz").len(), 10);
    }
}
