//! The seven category commands (`/anime /manga /novel /drama /series /movie /game`). They share one handler: a `type`
//! (work, character, random work, random character), a `name` with suggestions, and the filters of the category.

use super::{
    character::{self, CharacterView, Mode as CharMode},
    fail, fallback, media,
    profile::option_str,
    say_ephemeral, tmdb_view,
};
use crate::{
    anilist::{MediaFilter, MediaKind},
    db::notify,
    error::{Context, Error},
    i18n,
    providers::CharacterSource,
    tmdb::{find, lookup, model as tm},
    ui::{autocomplete::to_choices, session},
};
use poise::serenity_prelude as serenity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Anime,
    Manga,
    Novel,
    Drama,
    Series,
    Movie,
    Game,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Self::Anime,
        Self::Manga,
        Self::Novel,
        Self::Drama,
        Self::Series,
        Self::Movie,
        Self::Game,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Anime => "anime",
            Self::Manga => "manga",
            Self::Novel => "novel",
            Self::Drama => "drama",
            Self::Series => "series",
            Self::Movie => "movie",
            Self::Game => "game",
        }
    }

    fn from_command(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == name)
    }

    fn tmdb(self) -> Option<find::Cat> {
        match self {
            Self::Drama => Some(find::Cat::Drama),
            Self::Series => Some(find::Cat::Series),
            Self::Movie => Some(find::Cat::Movie),
            _ => None,
        }
    }
}

/// What to do: look a work up, look a character up, or pick a random one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum Type {
    #[name = "work"]
    #[name_localized("ar", "عمل")]
    Work,
    #[name = "character"]
    #[name_localized("ar", "شخصية")]
    Character,
    #[name = "random_work"]
    #[name_localized("ar", "عمل عشوائي")]
    RandomWork,
    #[name = "random_character"]
    #[name_localized("ar", "شخصية عشوائية")]
    RandomCharacter,
}

/// The game sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum GameSource {
    #[name = "IGDB"]
    #[name_localized("ar", "ألعاب (IGDB)")]
    Igdb,
    #[name = "VNDB"]
    #[name_localized("ar", "روايات بصرية (VNDB)")]
    Vndb,
}

/// Everything a category command collected.
#[derive(Debug, Default, Clone)]
pub struct Opts {
    pub name: Option<String>,
    pub format: Option<String>,
    pub year: Option<i64>,
    pub status: Option<String>,
    pub country: Option<String>,
    pub genre: Option<String>,
    pub platform: Option<String>,
    pub source: Option<GameSource>,
}

// ---- option tables -------------------------------------------------------------------------

pub const ANIME_FORMATS: &[(&str, &str)] = &[
    ("TV", "مسلسل (TV)"),
    ("TV_SHORT", "مسلسل قصير"),
    ("MOVIE", "فيلم"),
    ("OVA", "OVA"),
    ("ONA", "ONA"),
    ("SPECIAL", "حلقة خاصة"),
    ("MUSIC", "فيديو موسيقي"),
];
pub const MANGA_FORMATS: &[(&str, &str)] = &[
    ("manga", "مانجا يابانية"),
    ("manhwa", "مانهوا كورية"),
    ("manhua", "مانهوا صينية"),
    ("one-shot", "فصل واحد"),
];
pub const ANI_STATUSES: &[(&str, &str)] = &[
    ("RELEASING", "مستمر"),
    ("FINISHED", "منتهٍ"),
    ("NOT_YET_RELEASED", "لم يصدر بعد"),
    ("HIATUS", "متوقف مؤقتًا"),
    ("CANCELLED", "ملغي"),
];
pub const NOVEL_ORIGINS: &[(&str, &str)] = &[("JP", "يابانية"), ("KR", "كورية"), ("CN", "صينية")];
pub const PLATFORMS: &[(&str, &str)] = &[
    ("PC", "الحاسوب (PC)"),
    ("PlayStation", "بلايستيشن"),
    ("Xbox", "إكس بوكس"),
    ("Nintendo Switch", "نينتندو سويتش"),
    ("Android", "أندرويد"),
    ("iOS", "آيفون / آيباد"),
    ("Wii", "وي"),
    ("Nintendo 3DS", "نينتندو 3DS"),
    ("Mac", "ماك"),
    ("Linux", "لينكس"),
];

/// Resolve what the member picked or typed against `(value, Arabic label)` pairs.
pub fn resolve(table: &[(&str, &str)], input: Option<&str>) -> Option<String> {
    let t = input?.trim();
    if t.is_empty() {
        return None;
    }
    table
        .iter()
        .find(|(v, l)| v.eq_ignore_ascii_case(t) || *l == t)
        .map(|(v, _)| (*v).to_string())
}

/// The AniList kind and filter a category command's options describe.
pub fn anilist_target(cat: Category, o: &Opts) -> (MediaKind, MediaFilter) {
    let mut f = MediaFilter {
        year: o.year.map(|y| y as i32),
        ..MediaFilter::default()
    };
    match cat {
        Category::Anime => {
            f.format = resolve(ANIME_FORMATS, o.format.as_deref());
            f.status = resolve(ANI_STATUSES, o.status.as_deref());
            (MediaKind::Anime, f)
        }
        Category::Manga => {
            f.status = resolve(ANI_STATUSES, o.status.as_deref());
            let kind = match resolve(MANGA_FORMATS, o.format.as_deref()).as_deref() {
                Some("manhwa") => MediaKind::Manhwa,
                Some("manhua") => MediaKind::Manhua,
                Some("one-shot") => {
                    f.format = Some("ONE_SHOT".into());
                    MediaKind::Manga
                }
                _ => MediaKind::Manga,
            };
            (kind, f)
        }
        _ => {
            f.status = resolve(ANI_STATUSES, o.status.as_deref());
            f.country = resolve(NOVEL_ORIGINS, o.country.as_deref());
            (MediaKind::Novel, f)
        }
    }
}

pub fn tmdb_filter(o: &Opts) -> find::Filter {
    find::Filter {
        year: o.year.map(|y| y as i32),
        country: o
            .country
            .as_deref()
            .and_then(lookup::country_code)
            .map(str::to_string),
        status: o
            .status
            .clone()
            .filter(|s| lookup::tv_status_value(s).is_some()),
        genre: o
            .genre
            .clone()
            .filter(|g| lookup::GENRES.iter().any(|(n, _, _)| n == &g.trim())),
    }
}

// ---- autocomplete ----------------------------------------------------------------------------

fn table_choices(table: &[(&str, &str)], partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let needle = partial.trim().to_lowercase();
    to_choices(
        table
            .iter()
            .filter(|(v, l)| {
                needle.is_empty() || v.to_lowercase().contains(&needle) || l.contains(&needle)
            })
            .map(|(v, l)| ((*l).to_string(), (*v).to_string()))
            .collect(),
    )
}

fn command_category(ctx: Context<'_>) -> Option<Category> {
    Category::from_command(&ctx.command().name)
}

pub async fn ac_name(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    let Some(cat) = command_category(ctx) else {
        return Vec::new();
    };
    let ty = option_str(ctx, "type").unwrap_or_default();
    if ty.starts_with("random") {
        return Vec::new();
    }
    let characters = ty == "character";
    let data = ctx.data();
    match cat {
        Category::Anime | Category::Manga | Category::Novel => {
            if characters {
                return character::character_suggestions(ctx, partial, CharacterSource::AnimeManga)
                    .await;
            }
            let (kind, _) = anilist_target(
                cat,
                &Opts {
                    format: option_str(ctx, "format"),
                    ..Opts::default()
                },
            );
            media::media_autocomplete(ctx, kind, kind.key(), partial).await
        }
        Category::Drama | Category::Series | Category::Movie => {
            let Some(tcat) = cat.tmdb() else {
                return Vec::new();
            };
            if !data.tmdb.enabled() {
                return Vec::new();
            }
            let tmdb = data.tmdb.clone();
            let overrides = guild_overrides(ctx).await;
            let items = data
                .auto
                .items(ctx.author().id.get(), cat.key(), partial, |q| async move {
                    find::search(&tmdb, tcat, &q, &find::Filter::default(), &overrides)
                        .await
                        .iter()
                        .map(|w| {
                            (
                                format!(
                                    "{}{}",
                                    w.title,
                                    w.year.map(|y| format!(" ({y})")).unwrap_or_default()
                                ),
                                w.key(),
                            )
                        })
                        .collect()
                })
                .await;
            to_choices(items)
        }
        Category::Game => {
            let (games_on, mature) = fallback::game_flags(ctx).await;
            if !games_on {
                return Vec::new();
            }
            if characters {
                return character::character_suggestions(ctx, partial, CharacterSource::Games)
                    .await;
            }
            let providers = data.providers.clone();
            let src = option_str(ctx, "source");
            let items = data
                .auto
                .items(
                    ctx.author().id.get(),
                    "game",
                    &format!("{}|{partial}", src.clone().unwrap_or_default()),
                    |q| async move {
                        let text = q.rsplit('|').next().unwrap_or("").to_string();
                        let found = if src.as_deref() == Some("VNDB") {
                            providers.search_visual_novels(&text, mature).await
                        } else {
                            providers.search_games(&text, mature).await
                        };
                        found
                            .iter()
                            .map(|w| {
                                (
                                    match w.year {
                                        Some(y) => format!("{} ({y})", w.title),
                                        None => w.title.clone(),
                                    },
                                    w.title.clone(),
                                )
                            })
                            .collect()
                    },
                )
                .await;
            to_choices(items)
        }
    }
}

pub async fn ac_format(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    match command_category(ctx) {
        Some(Category::Anime) => table_choices(ANIME_FORMATS, partial),
        Some(Category::Manga) => table_choices(MANGA_FORMATS, partial),
        _ => Vec::new(),
    }
}

pub async fn ac_status(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    match command_category(ctx) {
        Some(Category::Drama | Category::Series) => {
            let t: Vec<(&str, &str)> = lookup::TV_STATUSES.iter().map(|(n, _)| (*n, *n)).collect();
            table_choices(&t, partial)
        }
        Some(Category::Anime | Category::Manga | Category::Novel) => {
            table_choices(ANI_STATUSES, partial)
        }
        _ => Vec::new(),
    }
}

pub async fn ac_origin(_ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    table_choices(NOVEL_ORIGINS, partial)
}

pub async fn ac_country(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let asian = tm::ASIAN_DRAMA_COUNTRIES;
    let list: Vec<(&str, &str)> = lookup::COUNTRIES
        .iter()
        .filter(|(c, _)| match command_category(ctx) {
            Some(Category::Drama) => asian.contains(c),
            Some(Category::Series) => !asian.contains(c),
            _ => true,
        })
        .map(|(c, n)| (*c, *n))
        .collect();
    table_choices(&list, partial)
}

pub async fn ac_genre(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let kind = if command_category(ctx) == Some(Category::Movie) {
        tm::Kind::Movie
    } else {
        tm::Kind::Tv
    };
    let list: Vec<(&str, &str)> = lookup::genre_names(kind)
        .into_iter()
        .map(|n| (n, n))
        .collect();
    table_choices(&list, partial)
}

pub async fn ac_platform(_ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    table_choices(PLATFORMS, partial)
}

async fn guild_overrides(ctx: Context<'_>) -> std::collections::HashMap<String, tm::Target> {
    match ctx.guild_id() {
        Some(g) => notify::drama_overrides(&ctx.data().db, g.get() as i64)
            .await
            .unwrap_or_default(),
        None => Default::default(),
    }
}

// ---- the shared handler ------------------------------------------------------------------

/// Run a category command.
pub async fn run(ctx: Context<'_>, cat: Category, ty: Type, o: Opts) -> Result<(), Error> {
    let name = o
        .name
        .clone()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    if matches!(ty, Type::Work | Type::Character) && name.is_none() {
        ctx.defer_ephemeral().await?;
        return say_ephemeral(ctx, i18n::CATEGORY_NEEDS_NAME).await;
    }
    match cat {
        Category::Anime | Category::Manga | Category::Novel => {
            run_anilist(ctx, cat, ty, &o, name).await
        }
        Category::Drama | Category::Series | Category::Movie => {
            run_tmdb(ctx, cat, ty, &o, name).await
        }
        Category::Game => run_game(ctx, ty, &o, name).await,
    }
}

async fn run_anilist(
    ctx: Context<'_>,
    cat: Category,
    ty: Type,
    o: &Opts,
    name: Option<String>,
) -> Result<(), Error> {
    let (kind, filter) = anilist_target(cat, o);
    match ty {
        Type::Work => media::run_media_with(ctx, kind, name.unwrap_or_default(), filter).await,
        Type::Character => {
            character::search_character(ctx, name.unwrap_or_default(), None, Some("anime".into()))
                .await
        }
        Type::RandomWork => {
            ctx.defer().await?;
            let api = ctx.data().anilist.clone();
            let pol = super::policy(ctx);
            let user = ctx.author().id.get() as i64;
            match media::pick_random(&api, user, kind, o.genre.as_deref(), None, &filter, pol).await
            {
                Ok(Some(m)) => {
                    let src = media::Source::Random {
                        kind,
                        genre: o.genre.clone(),
                        min_score: None,
                        filter,
                    };
                    let view =
                        media::MediaView::new(ctx, src, media::Mode::Card(Box::new(m))).await;
                    session::run(ctx, view).await
                }
                Ok(None) => say_ephemeral(ctx, i18n::RANDOM_FAILED).await,
                Err(e) => fail(ctx, e).await,
            }
        }
        Type::RandomCharacter => {
            ctx.defer().await?;
            let api = ctx.data().anilist.clone();
            let pol = super::policy(ctx);
            match api.random_character(pol).await {
                Ok(Some(c)) => {
                    let (db, user) = (ctx.data().db.clone(), ctx.author().id.get() as i64);
                    let mut view = CharacterView::new(api, pol, CharMode::Card(c), db, user);
                    view.prepare().await;
                    session::run(ctx, view).await
                }
                Ok(None) => say_ephemeral(ctx, i18n::RANDOM_FAILED).await,
                Err(e) => fail(ctx, e).await,
            }
        }
    }
}

async fn spoiler(ctx: Context<'_>) -> bool {
    notify::spoiler_guard(
        &ctx.data().db,
        ctx.guild_id().map(|g| g.get() as i64),
        ctx.author().id.get() as i64,
    )
    .await
}

async fn run_tmdb(
    ctx: Context<'_>,
    cat: Category,
    ty: Type,
    o: &Opts,
    name: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let Some(tcat) = cat.tmdb() else {
        return Ok(());
    };
    let tmdb = ctx.data().tmdb.clone();
    if !tmdb.enabled() {
        return say_ephemeral(ctx, i18n::TMDB_UNAVAILABLE).await;
    }
    let filter = tmdb_filter(o);
    let overrides = guild_overrides(ctx).await;
    let user = ctx.author().id.get() as i64;
    let sp = spoiler(ctx).await;
    match ty {
        Type::Work | Type::Character => {
            let name = name.unwrap_or_default();
            let hits = match tm::parse_work_key(&name) {
                Some((kind, id)) => tmdb.details(kind, id).await.into_iter().collect(),
                None => find::search(&tmdb, tcat, &name, &filter, &overrides).await,
            };
            if ty == Type::Work {
                return tmdb_view::show_results(ctx, sp, hits).await;
            }
            // A character is looked up through its work: the cast of the best match.
            let Some(first) = hits.into_iter().next() else {
                return say_ephemeral(ctx, i18n::NO_RESULTS).await;
            };
            let full = if first.cast.is_empty() {
                tmdb.details(first.kind.unwrap_or(tm::Kind::Tv), first.id)
                    .await
                    .unwrap_or(first)
            } else {
                first
            };
            let key = full.key();
            let data = ctx.data();
            let msg = crate::global::roster_msg(data, &key, 0).await;
            send_msg(ctx, msg).await
        }
        Type::RandomWork => match find::random(&tmdb, tcat, &filter, &overrides, user).await {
            Some(w) => {
                let r = tmdb_view::Reroll {
                    cat: tcat,
                    filter,
                    overrides,
                };
                session::run(
                    ctx,
                    tmdb_view::TmdbView::new(ctx, sp)
                        .with_card(w)
                        .await
                        .with_reroll(r),
                )
                .await
            }
            None => say_ephemeral(ctx, i18n::RANDOM_FAILED).await,
        },
        Type::RandomCharacter => {
            match find::random_cast(&tmdb, tcat, &filter, &overrides, user).await {
                Some((w, i)) => {
                    let r = tmdb_view::Reroll {
                        cat: tcat,
                        filter,
                        overrides,
                    };
                    session::run(
                        ctx,
                        tmdb_view::TmdbView::new(ctx, sp)
                            .with_cast(w, i)
                            .with_reroll(r),
                    )
                    .await
                }
                None => say_ephemeral(ctx, i18n::RANDOM_FAILED).await,
            }
        }
    }
}

/// Send a global-handler message as the command's reply.
async fn send_msg(ctx: Context<'_>, m: crate::global::Msg) -> Result<(), Error> {
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

async fn run_game(ctx: Context<'_>, ty: Type, o: &Opts, name: Option<String>) -> Result<(), Error> {
    ctx.defer().await?;
    let providers = ctx.data().providers.clone();
    let (games_on, mature) = fallback::game_flags(ctx).await;
    if !providers.games_enabled() {
        return say_ephemeral(ctx, i18n::GAMES_UNAVAILABLE).await;
    }
    if !games_on {
        return say_ephemeral(ctx, i18n::GAMES_SERVER_OFF).await;
    }
    let src = match o.source {
        Some(GameSource::Igdb) => Some("igdb"),
        Some(GameSource::Vndb) => Some("vndb"),
        None => None,
    };
    let platform = o
        .platform
        .as_deref()
        .and_then(|p| resolve(PLATFORMS, Some(p)).or_else(|| Some(p.trim().to_string())))
        .filter(|p| !p.is_empty());
    match ty {
        Type::Work => {
            let q = name.unwrap_or_default();
            let (games, vns) = futures::join!(
                async {
                    if src == Some("vndb") {
                        Vec::new()
                    } else if let Some(p) = &platform {
                        providers.search_games_on(&q, p, mature).await
                    } else {
                        providers.search_games(&q, mature).await
                    }
                },
                async {
                    if src == Some("igdb") || platform.is_some() {
                        Vec::new()
                    } else {
                        providers.search_visual_novels(&q, mature).await
                    }
                }
            );
            let mut all: Vec<_> = games.into_iter().chain(vns).collect();
            // An exact title wins over partial matches.
            if let Some(exact) = all.iter().position(|w| w.title.eq_ignore_ascii_case(&q)) {
                all = vec![all.remove(exact)];
            }
            all.truncate(10);
            if all.is_empty() {
                return say_ephemeral(ctx, empty_reason(&providers, i18n::NO_RESULTS)).await;
            }
            fallback::show(ctx, all).await
        }
        Type::Character => {
            character::search_character(ctx, name.unwrap_or_default(), None, Some("games".into()))
                .await
        }
        Type::RandomWork => match providers
            .random_work(src, platform.as_deref(), mature)
            .await
        {
            Some(w) => fallback::show(ctx, vec![w]).await,
            None => say_ephemeral(ctx, empty_reason(&providers, i18n::RANDOM_FAILED)).await,
        },
        Type::RandomCharacter => match providers.random_character(src, mature).await {
            Some(w) => {
                let (api, db, user) = (
                    ctx.data().anilist.clone(),
                    ctx.data().db.clone(),
                    ctx.author().id.get() as i64,
                );
                let mut view = CharacterView::new(
                    api,
                    super::policy(ctx),
                    CharMode::Provider(Box::new(w)),
                    db,
                    user,
                )
                .with_providers(providers, Vec::new());
                view.prepare().await;
                view.load_cover().await;
                session::run(ctx, view).await
            }
            None => say_ephemeral(ctx, empty_reason(&providers, i18n::RANDOM_FAILED)).await,
        },
    }
}

/// An empty game answer after a failed request is "the source is unavailable", not "nothing found".
pub fn empty_reason(
    providers: &crate::providers::Providers,
    nothing: &'static str,
) -> &'static str {
    if providers.games_unavailable() {
        i18n::GAMES_DOWN
    } else {
        nothing
    }
}

// ---- the seven commands --------------------------------------------------------------------

/// Anime: look one up, find a character, or get a random one.
#[poise::command(
    slash_command,
    name_localized("ar", "أنمي"),
    description_localized("ar", "ابحث عن أنمي أو شخصية، أو اختر واحدًا عشوائيًا")
)]
pub async fn anime(
    ctx: Context<'_>,
    #[description = "What to do"]
    #[description_localized("ar", "ماذا تريد: عمل، شخصية، أو عشوائي")]
    #[name_localized("ar", "النوع")]
    r#type: Type,
    #[description = "Name (optional for random picks)"]
    #[description_localized("ar", "الاسم (اختياري مع العشوائي)")]
    #[name_localized("ar", "الاسم")]
    #[autocomplete = "ac_name"]
    name: Option<String>,
    #[description = "Format: TV, movie, OVA..."]
    #[description_localized("ar", "الصيغة: مسلسل، فيلم، OVA…")]
    #[name_localized("ar", "الصيغة")]
    #[autocomplete = "ac_format"]
    format: Option<String>,
    #[description = "Release year"]
    #[description_localized("ar", "سنة العرض")]
    #[name_localized("ar", "السنة")]
    #[min = 1950]
    #[max = 2100]
    year: Option<i64>,
    #[description = "Status"]
    #[description_localized("ar", "الحالة")]
    #[name_localized("ar", "الحالة")]
    #[autocomplete = "ac_status"]
    status: Option<String>,
) -> Result<(), Error> {
    run(
        ctx,
        Category::Anime,
        r#type,
        Opts {
            name,
            format,
            year,
            status,
            ..Opts::default()
        },
    )
    .await
}

/// Manga, manhwa and manhua.
#[poise::command(
    slash_command,
    name_localized("ar", "مانجا"),
    description_localized("ar", "ابحث عن مانجا أو مانهوا أو شخصية، أو اختر عشوائيًا")
)]
pub async fn manga(
    ctx: Context<'_>,
    #[description = "What to do"]
    #[description_localized("ar", "ماذا تريد: عمل، شخصية، أو عشوائي")]
    #[name_localized("ar", "النوع")]
    r#type: Type,
    #[description = "Name (optional for random picks)"]
    #[description_localized("ar", "الاسم (اختياري مع العشوائي)")]
    #[name_localized("ar", "الاسم")]
    #[autocomplete = "ac_name"]
    name: Option<String>,
    #[description = "Format: manga, manhwa, manhua, one-shot"]
    #[description_localized("ar", "الصيغة: مانجا، مانهوا، مانهوا صينية، فصل واحد")]
    #[name_localized("ar", "الصيغة")]
    #[autocomplete = "ac_format"]
    format: Option<String>,
    #[description = "Status"]
    #[description_localized("ar", "الحالة")]
    #[name_localized("ar", "الحالة")]
    #[autocomplete = "ac_status"]
    status: Option<String>,
) -> Result<(), Error> {
    run(
        ctx,
        Category::Manga,
        r#type,
        Opts {
            name,
            format,
            status,
            ..Opts::default()
        },
    )
    .await
}

/// Light novels and novels.
#[poise::command(
    slash_command,
    name_localized("ar", "رواية"),
    description_localized("ar", "ابحث عن رواية أو شخصية، أو اختر واحدة عشوائيًا")
)]
pub async fn novel(
    ctx: Context<'_>,
    #[description = "What to do"]
    #[description_localized("ar", "ماذا تريد: عمل، شخصية، أو عشوائي")]
    #[name_localized("ar", "النوع")]
    r#type: Type,
    #[description = "Name (optional for random picks)"]
    #[description_localized("ar", "الاسم (اختياري مع العشوائي)")]
    #[name_localized("ar", "الاسم")]
    #[autocomplete = "ac_name"]
    name: Option<String>,
    #[description = "Origin: Japanese, Korean, Chinese"]
    #[description_localized("ar", "الأصل: يابانية، كورية، صينية")]
    #[name_localized("ar", "الأصل")]
    #[autocomplete = "ac_origin"]
    origin: Option<String>,
    #[description = "Status"]
    #[description_localized("ar", "الحالة")]
    #[name_localized("ar", "الحالة")]
    #[autocomplete = "ac_status"]
    status: Option<String>,
) -> Result<(), Error> {
    run(
        ctx,
        Category::Novel,
        r#type,
        Opts {
            name,
            country: origin,
            status,
            ..Opts::default()
        },
    )
    .await
}

/// Asian dramas (TMDB).
#[poise::command(
    slash_command,
    name_localized("ar", "دراما"),
    description_localized("ar", "ابحث عن دراما آسيوية أو شخصية، أو اختر واحدة عشوائيًا")
)]
pub async fn drama(
    ctx: Context<'_>,
    #[description = "What to do"]
    #[description_localized("ar", "ماذا تريد: عمل، شخصية، أو عشوائي")]
    #[name_localized("ar", "النوع")]
    r#type: Type,
    #[description = "Name (optional for random picks)"]
    #[description_localized("ar", "الاسم (اختياري مع العشوائي)")]
    #[name_localized("ar", "الاسم")]
    #[autocomplete = "ac_name"]
    name: Option<String>,
    #[description = "Country"]
    #[description_localized("ar", "البلد")]
    #[name_localized("ar", "البلد")]
    #[autocomplete = "ac_country"]
    country: Option<String>,
    #[description = "First air year"]
    #[description_localized("ar", "سنة العرض")]
    #[name_localized("ar", "السنة")]
    #[min = 1950]
    #[max = 2100]
    year: Option<i64>,
    #[description = "Status"]
    #[description_localized("ar", "الحالة")]
    #[name_localized("ar", "الحالة")]
    #[autocomplete = "ac_status"]
    status: Option<String>,
) -> Result<(), Error> {
    run(
        ctx,
        Category::Drama,
        r#type,
        Opts {
            name,
            country,
            year,
            status,
            ..Opts::default()
        },
    )
    .await
}

/// Series from the rest of the world (TMDB).
#[poise::command(
    slash_command,
    name_localized("ar", "مسلسل"),
    description_localized("ar", "ابحث عن مسلسل أو شخصية، أو اختر واحدًا عشوائيًا")
)]
pub async fn series(
    ctx: Context<'_>,
    #[description = "What to do"]
    #[description_localized("ar", "ماذا تريد: عمل، شخصية، أو عشوائي")]
    #[name_localized("ar", "النوع")]
    r#type: Type,
    #[description = "Name (optional for random picks)"]
    #[description_localized("ar", "الاسم (اختياري مع العشوائي)")]
    #[name_localized("ar", "الاسم")]
    #[autocomplete = "ac_name"]
    name: Option<String>,
    #[description = "Country"]
    #[description_localized("ar", "البلد")]
    #[name_localized("ar", "البلد")]
    #[autocomplete = "ac_country"]
    country: Option<String>,
    #[description = "First air year"]
    #[description_localized("ar", "سنة العرض")]
    #[name_localized("ar", "السنة")]
    #[min = 1950]
    #[max = 2100]
    year: Option<i64>,
    #[description = "Status"]
    #[description_localized("ar", "الحالة")]
    #[name_localized("ar", "الحالة")]
    #[autocomplete = "ac_status"]
    status: Option<String>,
    #[description = "Genre"]
    #[description_localized("ar", "النوع الفني")]
    #[name_localized("ar", "التصنيف")]
    #[autocomplete = "ac_genre"]
    genre: Option<String>,
) -> Result<(), Error> {
    run(
        ctx,
        Category::Series,
        r#type,
        Opts {
            name,
            country,
            year,
            status,
            genre,
            ..Opts::default()
        },
    )
    .await
}

/// Films (TMDB).
#[poise::command(
    slash_command,
    name_localized("ar", "فيلم"),
    description_localized("ar", "ابحث عن فيلم أو شخصية، أو اختر واحدًا عشوائيًا")
)]
pub async fn movie(
    ctx: Context<'_>,
    #[description = "What to do"]
    #[description_localized("ar", "ماذا تريد: عمل، شخصية، أو عشوائي")]
    #[name_localized("ar", "النوع")]
    r#type: Type,
    #[description = "Name (optional for random picks)"]
    #[description_localized("ar", "الاسم (اختياري مع العشوائي)")]
    #[name_localized("ar", "الاسم")]
    #[autocomplete = "ac_name"]
    name: Option<String>,
    #[description = "Release year"]
    #[description_localized("ar", "سنة الإصدار")]
    #[name_localized("ar", "السنة")]
    #[min = 1900]
    #[max = 2100]
    year: Option<i64>,
    #[description = "Genre"]
    #[description_localized("ar", "النوع الفني")]
    #[name_localized("ar", "التصنيف")]
    #[autocomplete = "ac_genre"]
    genre: Option<String>,
    #[description = "Country"]
    #[description_localized("ar", "البلد")]
    #[name_localized("ar", "البلد")]
    #[autocomplete = "ac_country"]
    country: Option<String>,
) -> Result<(), Error> {
    run(
        ctx,
        Category::Movie,
        r#type,
        Opts {
            name,
            year,
            genre,
            country,
            ..Opts::default()
        },
    )
    .await
}

/// Games and visual novels (IGDB, VNDB).
#[poise::command(
    slash_command,
    name_localized("ar", "لعبة"),
    description_localized("ar", "ابحث عن لعبة أو شخصية، أو اختر واحدة عشوائيًا")
)]
pub async fn game(
    ctx: Context<'_>,
    #[description = "What to do"]
    #[description_localized("ar", "ماذا تريد: عمل، شخصية، أو عشوائي")]
    #[name_localized("ar", "النوع")]
    r#type: Type,
    #[description = "Name (optional for random picks)"]
    #[description_localized("ar", "الاسم (اختياري مع العشوائي)")]
    #[name_localized("ar", "الاسم")]
    #[autocomplete = "ac_name"]
    name: Option<String>,
    #[description = "Platform"]
    #[description_localized("ar", "المنصة")]
    #[name_localized("ar", "المنصة")]
    #[autocomplete = "ac_platform"]
    platform: Option<String>,
    #[description = "Source: IGDB games or VNDB visual novels"]
    #[description_localized("ar", "المصدر: ألعاب IGDB أو روايات بصرية VNDB")]
    #[name_localized("ar", "المصدر")]
    source: Option<GameSource>,
) -> Result<(), Error> {
    run(
        ctx,
        Category::Game,
        r#type,
        Opts {
            name,
            platform,
            source,
            ..Opts::default()
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_resolve_by_value_or_arabic_label_and_ignore_nonsense() {
        assert_eq!(
            resolve(ANIME_FORMATS, Some("movie")).as_deref(),
            Some("MOVIE")
        );
        assert_eq!(
            resolve(ANIME_FORMATS, Some("فيلم")).as_deref(),
            Some("MOVIE")
        );
        assert_eq!(resolve(ANIME_FORMATS, Some("zzz")), None);
        assert_eq!(resolve(ANIME_FORMATS, Some("  ")), None);
        assert_eq!(resolve(ANIME_FORMATS, None), None);
    }

    #[test]
    fn the_manga_format_picks_the_kind_and_one_shot_sets_the_format() {
        let o = |f: &str| Opts {
            format: Some(f.into()),
            ..Opts::default()
        };
        assert_eq!(
            anilist_target(Category::Manga, &o("manhwa")).0,
            MediaKind::Manhwa
        );
        assert_eq!(
            anilist_target(Category::Manga, &o("manhua")).0,
            MediaKind::Manhua
        );
        let (k, f) = anilist_target(Category::Manga, &o("one-shot"));
        assert_eq!(
            (k, f.format.as_deref()),
            (MediaKind::Manga, Some("ONE_SHOT"))
        );
        assert_eq!(
            anilist_target(Category::Manga, &Opts::default()).0,
            MediaKind::Manga
        );
    }

    #[test]
    fn anime_and_novel_filters_are_read_from_the_options() {
        let o = Opts {
            format: Some("OVA".into()),
            year: Some(2015),
            status: Some("منتهٍ".into()),
            country: Some("KR".into()),
            ..Opts::default()
        };
        let (k, f) = anilist_target(Category::Anime, &o);
        assert_eq!(
            (k, f.format.as_deref(), f.year, f.status.as_deref()),
            (MediaKind::Anime, Some("OVA"), Some(2015), Some("FINISHED"))
        );
        let (k, f) = anilist_target(Category::Novel, &o);
        assert_eq!(
            (k, f.country.as_deref(), f.format),
            (MediaKind::Novel, Some("KR"), None)
        );
    }

    #[test]
    fn tmdb_filters_keep_only_known_values() {
        let o = Opts {
            year: Some(2020),
            country: Some("اليابان".into()),
            status: Some("منتهٍ".into()),
            genre: Some("دراما".into()),
            ..Opts::default()
        };
        let f = tmdb_filter(&o);
        assert_eq!(
            (
                f.year,
                f.country.as_deref(),
                f.status.as_deref(),
                f.genre.as_deref()
            ),
            (Some(2020), Some("JP"), Some("منتهٍ"), Some("دراما"))
        );
        let bad = tmdb_filter(&Opts {
            country: Some("nowhere".into()),
            status: Some("؟".into()),
            genre: Some("؟".into()),
            ..Opts::default()
        });
        assert_eq!(bad, find::Filter::default());
    }

    #[test]
    fn every_category_has_a_command_name_and_the_tmdb_ones_map_to_the_finder() {
        let keys: Vec<_> = Category::ALL.iter().map(|c| c.key()).collect();
        assert_eq!(
            keys,
            ["anime", "manga", "novel", "drama", "series", "movie", "game"]
        );
        assert_eq!(Category::from_command("drama"), Some(Category::Drama));
        assert_eq!(Category::from_command("top"), None);
        assert_eq!(
            Category::ALL.iter().filter(|c| c.tmdb().is_some()).count(),
            3
        );
    }
}
