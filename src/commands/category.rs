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
    ui::{
        autocomplete::{to_choices, MIN_CHARS},
        session,
    },
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
    #[name = "person"]
    #[name_localized("ar", "شخص (ممثلون ومؤدو أصوات وطاقم)")]
    Person,
    #[name = "random_work"]
    #[name_localized("ar", "عمل عشوائي")]
    RandomWork,
    #[name = "random_character"]
    #[name_localized("ar", "شخصية عشوائية")]
    RandomCharacter,
}

/// What the `source` option of `/game` asks for: nothing (every source, the owner's wikis included), one provider,
/// every wiki (and nothing else) or a single wiki.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceChoice {
    Default,
    /// The command's own main source (AniList, TMDB): the wikis stay out of it.
    Primary,
    Igdb,
    Vndb,
    AllWikis,
    Wiki(i64),
}

pub const SOURCE_IGDB: (&str, &str) = ("ألعاب (IGDB)", "IGDB");
pub const SOURCE_VNDB: (&str, &str) = ("روايات بصرية (VNDB)", "VNDB");
pub const SOURCE_ALL_WIKIS: (&str, &str) = ("كل الويكيات", "wikis");
pub const SOURCE_ANILIST: (&str, &str) = ("AniList", "anilist");
pub const SOURCE_TMDB: (&str, &str) = ("TMDB", "tmdb");
pub const SOURCE_PRIMARY: (&str, &str) = ("المصادر الأساسية", "providers");

impl SourceChoice {
    /// What the member picked from the suggestions or typed: a value (`IGDB`, `wikis`, `wiki:3`) or its label.
    pub fn parse(input: Option<&str>, wikis: &[crate::wiki::Source]) -> Self {
        let Some(t) = input.map(str::trim).filter(|t| !t.is_empty()) else {
            return Self::Default;
        };
        let same = |a: &str| a.eq_ignore_ascii_case(t);
        if same(SOURCE_IGDB.1) || same(SOURCE_IGDB.0) {
            return Self::Igdb;
        }
        if same(SOURCE_VNDB.1) || same(SOURCE_VNDB.0) {
            return Self::Vndb;
        }
        if same(SOURCE_ALL_WIKIS.1) || same(SOURCE_ALL_WIKIS.0) {
            return Self::AllWikis;
        }
        if [SOURCE_ANILIST, SOURCE_TMDB, SOURCE_PRIMARY]
            .iter()
            .any(|s| same(s.1) || same(s.0))
        {
            return Self::Primary;
        }
        let by_id = t.strip_prefix("wiki:").and_then(|n| n.parse::<i64>().ok());
        wikis
            .iter()
            .find(|w| Some(w.id) == by_id || same(&w.name))
            .map_or(Self::Default, |w| Self::Wiki(w.id))
    }

    pub fn wikis_only(self) -> bool {
        matches!(self, Self::AllWikis | Self::Wiki(_))
    }

    /// Do the main providers take part?
    pub fn providers(self) -> bool {
        !self.wikis_only()
    }

    /// Do the wikis take part (every wiki, or the one chosen)?
    pub fn wikis(self) -> bool {
        matches!(self, Self::Default | Self::AllWikis | Self::Wiki(_))
    }

    pub fn only_wiki(self) -> Option<i64> {
        match self {
            Self::Wiki(id) => Some(id),
            _ => None,
        }
    }

    /// The provider key `search_games_found` takes.
    pub fn provider(self) -> Option<&'static str> {
        match self {
            Self::Igdb => Some("igdb"),
            Self::Vndb => Some("vndb"),
            _ => None,
        }
    }
}

/// The main sources a command offers in its `source` menu: `(label, value)`.
pub fn primary_options(cat: Option<Category>) -> Vec<(String, String)> {
    let own = |l: (&str, &str)| (l.0.to_string(), l.1.to_string());
    match cat {
        Some(Category::Anime | Category::Manga | Category::Novel) => vec![own(SOURCE_ANILIST)],
        Some(Category::Drama | Category::Series | Category::Movie) => vec![own(SOURCE_TMDB)],
        Some(Category::Game) => vec![own(SOURCE_IGDB), own(SOURCE_VNDB)],
        None => vec![own(SOURCE_PRIMARY)],
    }
}

/// The options of the `source` menu: the command's own sources, "كل الويكيات" and one entry per wiki, within Discord's
/// 25 (further wikis are still reachable through "كل الويكيات"). `(label, value)`. Read from the cached wiki list, so a
/// wiki added or removed shows up in every server at once without registering the commands again.
pub fn source_options(
    wikis: &[crate::wiki::Source],
    cat: Option<Category>,
) -> Vec<(String, String)> {
    let mut out = primary_options(cat);
    if !wikis.is_empty() {
        out.push((
            SOURCE_ALL_WIKIS.0.to_string(),
            SOURCE_ALL_WIKIS.1.to_string(),
        ));
    }
    let room = 25usize.saturating_sub(out.len());
    out.extend(
        wikis
            .iter()
            .take(room)
            .map(|w| (w.name.clone(), format!("wiki:{}", w.id))),
    );
    out
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
    pub source: Option<String>,
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
    let target = AcTarget::of(&option_str(ctx, "type").unwrap_or_default());
    match target {
        AcTarget::Random => return Vec::new(),
        AcTarget::Person => return person_suggestions(ctx, cat, partial).await,
        _ => {}
    }
    let characters = target == AcTarget::Character;
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
            let wikis = data.wikis.clone();
            let wiki_list = wikis.sources().await;
            let src = option_str(ctx, "source");
            let choice = SourceChoice::parse(src.as_deref(), &wiki_list);
            let items = data
                .auto
                .items_scoped(
                    ctx.author().id.get(),
                    "game",
                    &format!("{}|{mature}", src.clone().unwrap_or_default()),
                    partial,
                    MIN_CHARS,
                    |text| async move {
                        let found = match choice {
                            SourceChoice::Vndb => {
                                providers.search_visual_novels(&text, mature).await
                            }
                            c if c.providers() && c != SourceChoice::Vndb => {
                                providers.search_games(&text, mature).await
                            }
                            _ => Vec::new(),
                        };
                        let mut out: Vec<(String, String)> = found
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
                            .collect();
                        // The wikis of the owner, from the local index only (never a live call: three seconds).
                        if choice.wikis() {
                            let known: std::collections::HashSet<String> = found
                                .iter()
                                .map(|w| crate::wiki::normalize(&w.title))
                                .collect();
                            let names: std::collections::HashMap<i64, String> =
                                wiki_list.iter().map(|w| (w.id, w.name.clone())).collect();
                            for p in wikis
                                .suggest(
                                    &text,
                                    Some(crate::wiki::Kind::Game),
                                    choice.only_wiki(),
                                    10,
                                )
                                .await
                            {
                                if known.contains(&crate::wiki::normalize(&p.title)) {
                                    continue;
                                }
                                let wiki = names.get(&p.wiki_id).cloned().unwrap_or_default();
                                out.push((format!("{} ({wiki})", p.title), p.key()));
                            }
                        }
                        out
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

/// The `source` menu of `/game`: IGDB, VNDB, every wiki and each wiki by name.
pub async fn ac_source(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let wikis = ctx.data().wikis.sources().await;
    let needle = partial.trim().to_lowercase();
    to_choices(
        source_options(&wikis, command_category(ctx))
            .into_iter()
            .filter(|(label, value)| {
                needle.is_empty()
                    || label.to_lowercase().contains(&needle)
                    || value.to_lowercase().contains(&needle)
            })
            .collect(),
    )
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

/// `source` set to a wiki on a command that normally uses another source: only the wikis are searched.
async fn run_wiki_only(
    ctx: Context<'_>,
    ty: Type,
    q: String,
    choice: SourceChoice,
) -> Result<(), Error> {
    ctx.defer().await?;
    let wikis = ctx.data().wikis.clone();
    let user = ctx.author().id.get() as i64;
    if let Some((wiki, page)) = crate::wiki::parse_key(q.trim()) {
        let msg = crate::wiki::msgs::page_msg(&wikis, wiki, page, user).await;
        return crate::global::reply(ctx, msg).await;
    }
    let kind = (ty == Type::Character).then_some(crate::wiki::Kind::Character);
    let mut found = wikis.search(&q, kind, choice.only_wiki()).await;
    if kind.is_none() {
        // A work search: the pages that are not characters.
        found
            .pages
            .retain(|p| p.kind != crate::wiki::Kind::Character);
    }
    match crate::wiki::msgs::show(&wikis, found, &q, user).await {
        Some(msg) => crate::global::reply(ctx, msg).await,
        None => say_ephemeral(ctx, i18n::NO_RESULTS).await,
    }
}

// ---- people ------------------------------------------------------------------------------------

/// What the `type` option chosen so far asks the `name` autocomplete for (a command not yet given a type: works).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcTarget {
    Work,
    Character,
    Person,
    /// A random pick has no name.
    Random,
}

impl AcTarget {
    pub fn of(ty: &str) -> Self {
        match ty.trim() {
            "character" => Self::Character,
            "person" => Self::Person,
            t if t.starts_with("random") => Self::Random,
            _ => Self::Work,
        }
    }
}

/// The people sources of a category: AniList staff for anime / manga / novels, TMDB persons for drama, series and
/// movies, IGDB companies and VNDB staff for games.
pub fn people_scope(cat: Category) -> crate::people::fetch::Scope {
    use crate::people::fetch::Scope;
    match cat {
        Category::Anime | Category::Manga | Category::Novel => Scope::AnimeManga,
        Category::Drama | Category::Series | Category::Movie => Scope::Screen,
        Category::Game => Scope::Games,
    }
}

async fn person_suggestions(
    ctx: Context<'_>,
    cat: Category,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    let scope = people_scope(cat);
    if scope == crate::people::fetch::Scope::Games && !fallback::game_flags(ctx).await.0 {
        return Vec::new();
    }
    let sources = crate::people::fetch::Sources::of(ctx.data());
    let items = ctx
        .data()
        .auto
        .items_scoped(
            ctx.author().id.get(),
            "person",
            &format!("{scope:?}"),
            partial,
            0,
            |q| async move {
                crate::people::fetch::search(&sources, scope, &q, true)
                    .await
                    .hits
                    .iter()
                    .map(|h| (h.label(), h.key.clone()))
                    .collect()
            },
        )
        .await;
    to_choices(items)
}

/// `type: شخص`: open the card of the person picked from the suggestions, or search the typed name.
async fn run_person(ctx: Context<'_>, cat: Category, name: String) -> Result<(), Error> {
    ctx.defer().await?;
    let scope = people_scope(cat);
    let data = ctx.data();
    match scope {
        crate::people::fetch::Scope::Screen if !data.tmdb.enabled() => {
            return say_ephemeral(ctx, i18n::TMDB_UNAVAILABLE).await;
        }
        crate::people::fetch::Scope::Games => {
            if !data.providers.games_enabled() {
                return say_ephemeral(ctx, i18n::GAMES_UNAVAILABLE).await;
            }
            if !fallback::game_flags(ctx).await.0 {
                return say_ephemeral(ctx, i18n::GAMES_SERVER_OFF).await;
            }
        }
        _ => {}
    }
    let mature = crate::global::mature_for(data, ctx.guild_id().map(|g| g.get() as i64)).await;
    let key = if crate::people::parse_key(name.trim()).is_some() {
        name.trim().to_string()
    } else {
        let found = crate::people::fetch::search(
            &crate::people::fetch::Sources::of(data),
            scope,
            &name,
            mature,
        )
        .await;
        let note = found.partial_note();
        let exact = found
            .hits
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name.trim()))
            .map(|h| h.key.clone());
        match (found.hits.as_slice(), exact) {
            ([], _) => {
                let text = note.unwrap_or_else(|| i18n::NO_RESULTS.to_string());
                return say_ephemeral(ctx, &text).await;
            }
            ([one], _) => one.key.clone(),
            (_, Some(key)) => key,
            (many, None) => {
                return crate::global::reply(ctx, crate::people::msgs::hits_list(many, note)).await;
            }
        }
    };
    let msg =
        crate::people::msgs::person_msg(data, &key, mature, ctx.author().id.get() as i64).await;
    crate::global::reply(ctx, msg).await
}

// ---- the shared handler ------------------------------------------------------------------

/// Run a category command.
pub async fn run(ctx: Context<'_>, cat: Category, ty: Type, o: Opts) -> Result<(), Error> {
    let name = o
        .name
        .clone()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    if matches!(ty, Type::Work | Type::Character | Type::Person) && name.is_none() {
        ctx.defer_ephemeral().await?;
        return say_ephemeral(ctx, i18n::CATEGORY_NEEDS_NAME).await;
    }
    // A wiki (or all wikis) chosen as the source: the search is restricted to wikis, for every command.
    if cat != Category::Game && matches!(ty, Type::Work | Type::Character) {
        let wiki_list = ctx.data().wikis.sources().await;
        let choice = SourceChoice::parse(o.source.as_deref(), &wiki_list);
        if choice.wikis_only() {
            return run_wiki_only(ctx, ty, name.unwrap_or_default(), choice).await;
        }
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
        Type::Person => run_person(ctx, cat, name.unwrap_or_default()).await,
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
    if ty == Type::Person {
        return run_person(ctx, cat, name.unwrap_or_default()).await;
    }
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
            let mature =
                crate::global::mature_for(data, ctx.guild_id().map(|g| g.get() as i64)).await;
            let msg = crate::global::roster_msg(data, &key, 0, mature).await;
            crate::global::reply(ctx, msg).await
        }
        Type::Person => Ok(()),
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

async fn run_game(ctx: Context<'_>, ty: Type, o: &Opts, name: Option<String>) -> Result<(), Error> {
    ctx.defer().await?;
    let providers = ctx.data().providers.clone();
    let wikis = ctx.data().wikis.clone();
    let wiki_list = wikis.sources().await;
    let (games_on, mature) = fallback::game_flags(ctx).await;
    if !providers.games_enabled() && wiki_list.is_empty() {
        return say_ephemeral(ctx, i18n::GAMES_UNAVAILABLE).await;
    }
    if !games_on {
        return say_ephemeral(ctx, i18n::GAMES_SERVER_OFF).await;
    }
    let choice = SourceChoice::parse(o.source.as_deref(), &wiki_list);
    let src = choice.provider();
    let platform = o
        .platform
        .as_deref()
        .and_then(|p| resolve(PLATFORMS, Some(p)).or_else(|| Some(p.trim().to_string())))
        .filter(|p| !p.is_empty());
    // A page of a wiki picked from the suggestions opens directly, whatever the type.
    if let Some((wiki, page)) = name
        .as_deref()
        .and_then(|n| crate::wiki::parse_key(n.trim()))
    {
        let msg =
            crate::wiki::msgs::page_msg(&wikis, wiki, page, ctx.author().id.get() as i64).await;
        return crate::global::reply(ctx, msg).await;
    }
    match ty {
        Type::Person => run_person(ctx, Category::Game, name.unwrap_or_default()).await,
        Type::Work => {
            let q = name.unwrap_or_default();
            // The main providers and the wikis are asked together; a slow or failing one never holds back the other.
            let (found, wiki_found) = futures::join!(
                async {
                    if choice.providers() {
                        providers
                            .search_games_found(&q, platform.as_deref(), mature, src)
                            .await
                    } else {
                        crate::providers::Found::default()
                    }
                },
                async {
                    if choice.wikis() {
                        wikis
                            .search(&q, Some(crate::wiki::Kind::Game), choice.only_wiki())
                            .await
                    } else {
                        crate::wiki::service::Found::default()
                    }
                }
            );
            let note = found.partial_note();
            let mut all = found.works.clone();
            // An exact title wins over partial matches.
            if let Some(exact) = all.iter().position(|w| w.title.eq_ignore_ascii_case(&q)) {
                all = vec![all.remove(exact)];
            }
            all.truncate(10);
            if !all.is_empty() && choice.providers() {
                return fallback::show_with_note(ctx, all, note).await;
            }
            // The main providers have nothing (or a wiki was asked for): the wiki results are shown directly.
            if let Some(msg) =
                crate::wiki::msgs::show(&wikis, wiki_found, &q, ctx.author().id.get() as i64).await
            {
                return crate::global::reply(ctx, msg).await;
            }
            say_ephemeral(ctx, found.empty_message(i18n::NO_RESULTS)).await
        }
        Type::Character => {
            let q = name.unwrap_or_default();
            if choice.wikis_only() {
                let found = wikis
                    .search(&q, Some(crate::wiki::Kind::Character), choice.only_wiki())
                    .await;
                return match crate::wiki::msgs::show(
                    &wikis,
                    found,
                    &q,
                    ctx.author().id.get() as i64,
                )
                .await
                {
                    Some(msg) => crate::global::reply(ctx, msg).await,
                    None => say_ephemeral(ctx, i18n::NO_RESULTS).await,
                };
            }
            // With no source chosen and wikis present, wiki characters join the very same list as the other sources'
            // (the general search with the type set to characters: one ranked, de-duplicated list, the source of each
            // option in its description), always, not only when the main sources found nothing.
            if choice == SourceChoice::Default && !wiki_list.is_empty() {
                let msg = crate::global::search_or_open(
                    ctx.data(),
                    ctx.guild_id().map(|g| g.get() as i64),
                    ctx.author().id.get() as i64,
                    crate::search::Cat::Game,
                    crate::search::Ty::Character,
                    crate::search::Src::Auto,
                    &q,
                )
                .await;
                return crate::global::reply(ctx, msg).await;
            }
            // Only one provider asked for: the wikis stay out of it.
            let scope = if matches!(choice, SourceChoice::Igdb | SourceChoice::Vndb) {
                "games-only"
            } else {
                "games"
            };
            character::search_character(ctx, q, None, Some(scope.into())).await
        }
        Type::RandomWork => {
            let user = ctx.author().id.get() as i64;
            let found = providers
                .random_work_found(src, platform.as_deref(), mature, user)
                .await;
            let note = found.partial_note();
            match found.works.first().cloned() {
                Some(w) => fallback::show_with_note(ctx, vec![w], note).await,
                None => say_ephemeral(ctx, found.empty_message(i18n::RANDOM_FAILED)).await,
            }
        }
        Type::RandomCharacter => {
            let user = ctx.author().id.get() as i64;
            let found = providers.random_character_found(src, mature, user).await;
            let note = found.partial_note();
            match found.works.first().cloned() {
                Some(w) => {
                    let (api, db) = (ctx.data().anilist.clone(), ctx.data().db.clone());
                    let mut view = CharacterView::new(
                        api,
                        super::policy(ctx),
                        CharMode::Provider(Box::new(w)),
                        db,
                        user,
                    )
                    .with_providers(providers, Vec::new())
                    .with_wikis(ctx.data().wikis.clone());
                    view.prepare().await;
                    view.load_cover().await;
                    view.set_notice(note);
                    session::run(ctx, view).await
                }
                None => say_ephemeral(ctx, found.empty_message(i18n::RANDOM_FAILED)).await,
            }
        }
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
    #[description = "Source (optional): the main source, all wikis or one wiki"]
    #[description_localized("ar", "المصدر (اختياري): المصدر الأساسي أو كل الويكيات أو ويكي بعينها")]
    #[name_localized("ar", "المصدر")]
    #[autocomplete = "ac_source"]
    source: Option<String>,
) -> Result<(), Error> {
    run(
        ctx,
        Category::Anime,
        r#type,
        Opts {
            name,
            source,
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
    #[description = "Source (optional): the main source, all wikis or one wiki"]
    #[description_localized("ar", "المصدر (اختياري): المصدر الأساسي أو كل الويكيات أو ويكي بعينها")]
    #[name_localized("ar", "المصدر")]
    #[autocomplete = "ac_source"]
    source: Option<String>,
) -> Result<(), Error> {
    run(
        ctx,
        Category::Manga,
        r#type,
        Opts {
            name,
            source,
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
    #[description = "Source (optional): the main source, all wikis or one wiki"]
    #[description_localized("ar", "المصدر (اختياري): المصدر الأساسي أو كل الويكيات أو ويكي بعينها")]
    #[name_localized("ar", "المصدر")]
    #[autocomplete = "ac_source"]
    source: Option<String>,
) -> Result<(), Error> {
    run(
        ctx,
        Category::Novel,
        r#type,
        Opts {
            name,
            source,
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
    #[description = "Source (optional): the main source, all wikis or one wiki"]
    #[description_localized("ar", "المصدر (اختياري): المصدر الأساسي أو كل الويكيات أو ويكي بعينها")]
    #[name_localized("ar", "المصدر")]
    #[autocomplete = "ac_source"]
    source: Option<String>,
) -> Result<(), Error> {
    run(
        ctx,
        Category::Drama,
        r#type,
        Opts {
            name,
            source,
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
#[allow(clippy::too_many_arguments)]
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
    #[description = "Source (optional): the main source, all wikis or one wiki"]
    #[description_localized("ar", "المصدر (اختياري): المصدر الأساسي أو كل الويكيات أو ويكي بعينها")]
    #[name_localized("ar", "المصدر")]
    #[autocomplete = "ac_source"]
    source: Option<String>,
) -> Result<(), Error> {
    run(
        ctx,
        Category::Series,
        r#type,
        Opts {
            name,
            source,
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
    #[description = "Source (optional): the main source, all wikis or one wiki"]
    #[description_localized("ar", "المصدر (اختياري): المصدر الأساسي أو كل الويكيات أو ويكي بعينها")]
    #[name_localized("ar", "المصدر")]
    #[autocomplete = "ac_source"]
    source: Option<String>,
) -> Result<(), Error> {
    run(
        ctx,
        Category::Movie,
        r#type,
        Opts {
            name,
            source,
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
    #[autocomplete = "ac_source"]
    source: Option<String>,
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

    #[test]
    fn the_type_choices_include_person_and_no_random_person() {
        use poise::ChoiceParameter;
        let names: Vec<String> = Type::list().iter().map(|c| c.name.clone()).collect();
        assert_eq!(
            names,
            [
                "work",
                "character",
                "person",
                "random_work",
                "random_character"
            ]
        );
        let ar: Vec<String> = Type::list()
            .iter()
            .map(|c| c.localizations.get("ar").cloned().unwrap_or_default())
            .collect();
        assert_eq!(ar[2], "شخص (ممثلون ومؤدو أصوات وطاقم)");
        assert!(ar.iter().all(|n| !n.is_empty() && n.chars().count() <= 100));
        assert!(!names.iter().any(|n| n == "random_person"));
    }

    #[test]
    fn the_name_autocomplete_follows_the_chosen_type() {
        assert_eq!(AcTarget::of("work"), AcTarget::Work);
        assert_eq!(AcTarget::of(""), AcTarget::Work, "no type yet means works");
        assert_eq!(AcTarget::of("character"), AcTarget::Character);
        assert_eq!(AcTarget::of("person"), AcTarget::Person);
        assert_eq!(AcTarget::of("random_work"), AcTarget::Random);
        assert_eq!(AcTarget::of("random_character"), AcTarget::Random);
    }

    #[test]
    fn each_category_searches_its_own_people_sources() {
        use crate::people::fetch::Scope;
        for c in [Category::Anime, Category::Manga, Category::Novel] {
            assert_eq!(people_scope(c), Scope::AnimeManga);
        }
        for c in [Category::Drama, Category::Series, Category::Movie] {
            assert_eq!(people_scope(c), Scope::Screen);
        }
        assert_eq!(people_scope(Category::Game), Scope::Games);
    }

    fn wiki(id: i64, name: &str) -> crate::wiki::Source {
        crate::wiki::Source {
            id,
            name: name.into(),
            host: format!("w{id}.example.com"),
            api_url: format!("https://w{id}.example.com/api.php"),
            lang: "en".into(),
            license_name: None,
            license_url: None,
            character_category: None,
            work_category: None,
            kind_hint: None,
            enabled: true,
            added_by: None,
            created_at: 0,
            last_checked_at: None,
            last_error: None,
        }
    }

    #[test]
    fn the_source_menu_lists_the_commands_own_sources_all_wikis_and_each_wiki() {
        let none = source_options(&[], Some(Category::Anime));
        assert_eq!(
            none.iter().map(|o| o.1.as_str()).collect::<Vec<_>>(),
            vec!["anilist"],
            "no wikis, no wiki options"
        );
        let wikis = [wiki(1, "Silent Hill Wiki"), wiki(2, "Mudae Wiki")];
        let anime = source_options(&wikis, Some(Category::Anime));
        assert_eq!(
            anime.iter().map(|o| o.1.as_str()).collect::<Vec<_>>(),
            vec!["anilist", "wikis", "wiki:1", "wiki:2"]
        );
        assert_eq!(source_options(&wikis, Some(Category::Movie))[0].1, "tmdb");
        assert_eq!(
            source_options(&wikis, Some(Category::Game))[..2]
                .iter()
                .map(|o| o.1.as_str())
                .collect::<Vec<_>>(),
            vec!["IGDB", "VNDB"]
        );
        assert_eq!(source_options(&wikis, None)[0].1, "providers", "/search");
        // a newly added wiki shows up in the very next call: the options come from the list, not from the definition
        let more = [wikis[0].clone(), wikis[1].clone(), wiki(3, "New Wiki")];
        assert!(source_options(&more, Some(Category::Drama))
            .iter()
            .any(|o| o.1 == "wiki:3"));
    }

    #[test]
    fn the_source_menu_never_exceeds_discords_25_and_keeps_all_wikis() {
        let wikis: Vec<_> = (1..=40).map(|i| wiki(i, &format!("Wiki {i}"))).collect();
        for cat in [Some(Category::Anime), Some(Category::Game), None] {
            let o = source_options(&wikis, cat);
            assert_eq!(o.len(), 25, "{cat:?}");
            assert!(
                o.iter().any(|x| x.1 == "wikis"),
                "the extra wikis are still reachable through all wikis"
            );
            let mut seen = std::collections::HashSet::new();
            assert!(o
                .iter()
                .all(|x| seen.insert(x.1.clone()) && x.0.chars().count() <= 100));
        }
    }

    #[test]
    fn a_chosen_source_restricts_the_search() {
        let wikis = [wiki(1, "Silent Hill Wiki"), wiki(2, "Mudae Wiki")];
        let p = |s: &str| SourceChoice::parse(Some(s), &wikis);
        assert_eq!(SourceChoice::parse(None, &wikis), SourceChoice::Default);
        assert!(
            SourceChoice::Default.providers() && SourceChoice::Default.wikis(),
            "the default asks everything"
        );
        assert!(
            p("anilist").providers() && !p("anilist").wikis(),
            "the main source only"
        );
        assert!(p("tmdb").providers() && !p("tmdb").wikis());
        assert!(
            !p("wikis").providers() && p("wikis").wikis() && p("wikis").only_wiki().is_none(),
            "all wikis means wikis only"
        );
        assert_eq!(p("wiki:2"), SourceChoice::Wiki(2));
        assert_eq!(
            p("Mudae Wiki"),
            SourceChoice::Wiki(2),
            "a typed name works too"
        );
        assert!(!p("wiki:2").providers() && p("wiki:2").only_wiki() == Some(2));
        assert_eq!(p("IGDB").provider(), Some("igdb"));
        assert!(!p("IGDB").wikis(), "one provider keeps the wikis out");
        assert_eq!(
            p("wiki:99"),
            SourceChoice::Default,
            "a wiki that is gone is not a restriction"
        );
        assert_eq!(
            crate::search::Src::from_choice(p("wikis")),
            crate::search::Src::AllWikis
        );
        assert_eq!(
            crate::search::Src::from_choice(p("wiki:1")),
            crate::search::Src::Wiki(1)
        );
        assert_eq!(
            crate::search::Src::from_choice(p("anilist")),
            crate::search::Src::Providers
        );
    }
}
