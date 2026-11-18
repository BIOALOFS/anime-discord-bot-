//! The general search (`/search`): works, characters and people from every enabled source in one list. Autocomplete
//! and the free-text submit share `gather`, so a name that shows up while typing is the one the submit finds, from the
//! same caches. Values are provider-prefixed keys, so picking one opens the right card directly.

use crate::{
    anilist::{kind_of, ContentPolicy, MediaKind},
    components,
    global::Msg,
    i18n,
    people::{
        self,
        fetch::{Scope, Sources},
    },
    tmdb::{find, model as tm},
    ui::{
        cards,
        embeds::{fit, Emb},
    },
};
use poise::serenity_prelude as serenity;

/// The category filter of `/search`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum Cat {
    #[name = "all"]
    #[name_localized("ar", "الكل")]
    All,
    #[name = "anime"]
    #[name_localized("ar", "أنمي")]
    Anime,
    #[name = "manga"]
    #[name_localized("ar", "مانجا")]
    Manga,
    #[name = "novel"]
    #[name_localized("ar", "روايات")]
    Novel,
    #[name = "drama"]
    #[name_localized("ar", "دراما")]
    Drama,
    #[name = "series"]
    #[name_localized("ar", "مسلسلات")]
    Series,
    #[name = "movie"]
    #[name_localized("ar", "أفلام")]
    Movie,
    #[name = "game"]
    #[name_localized("ar", "ألعاب")]
    Game,
}

impl Cat {
    pub fn key(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Anime => "anime",
            Self::Manga => "manga",
            Self::Novel => "novel",
            Self::Drama => "drama",
            Self::Series => "series",
            Self::Movie => "movie",
            Self::Game => "game",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        [
            Self::All,
            Self::Anime,
            Self::Manga,
            Self::Novel,
            Self::Drama,
            Self::Series,
            Self::Movie,
            Self::Game,
        ]
        .into_iter()
        .find(|c| c.key() == k)
    }

    fn anilist(self) -> bool {
        matches!(self, Self::All | Self::Anime | Self::Manga | Self::Novel)
    }

    fn tmdb(self) -> bool {
        matches!(self, Self::All | Self::Drama | Self::Series | Self::Movie)
    }

    fn games(self) -> bool {
        matches!(self, Self::All | Self::Game)
    }

    pub fn people_scope(self) -> Scope {
        match self {
            Self::All => Scope::All,
            Self::Anime | Self::Manga | Self::Novel => Scope::AnimeManga,
            Self::Drama | Self::Series | Self::Movie => Scope::Screen,
            Self::Game => Scope::Games,
        }
    }
}

/// The type filter of `/search`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum Ty {
    #[name = "all"]
    #[name_localized("ar", "الكل")]
    All,
    #[name = "work"]
    #[name_localized("ar", "عمل")]
    Work,
    #[name = "character"]
    #[name_localized("ar", "شخصية")]
    Character,
    #[name = "person"]
    #[name_localized("ar", "شخص")]
    Person,
}

impl Ty {
    pub fn key(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Work => "work",
            Self::Character => "character",
            Self::Person => "person",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        [Self::All, Self::Work, Self::Character, Self::Person]
            .into_iter()
            .find(|t| t.key() == k)
    }

    fn includes(self, kind: Group) -> bool {
        self == Self::All
            || matches!(
                (self, kind),
                (Self::Work, Group::Work)
                    | (Self::Character, Group::Character)
                    | (Self::Person, Group::Person)
            )
    }
}

/// Which sources a search uses (the `source` option): the default (everything appropriate, wikis included), only the
/// main providers, only wikis (all of them) or only one wiki.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Src {
    #[default]
    Auto,
    Providers,
    AllWikis,
    Wiki(i64),
}

impl Src {
    /// A short code for button ids: `a`, `p`, `w`, `w12`.
    pub fn code(self) -> String {
        match self {
            Self::Auto => "a".into(),
            Self::Providers => "p".into(),
            Self::AllWikis => "w".into(),
            Self::Wiki(id) => format!("w{id}"),
        }
    }

    pub fn from_code(c: &str) -> Option<Self> {
        match c {
            "a" => Some(Self::Auto),
            "p" => Some(Self::Providers),
            "w" => Some(Self::AllWikis),
            _ => c
                .strip_prefix('w')
                .and_then(|n| n.parse().ok())
                .map(Self::Wiki),
        }
    }

    fn providers(self) -> bool {
        matches!(self, Self::Auto | Self::Providers)
    }

    fn wikis(self) -> bool {
        !matches!(self, Self::Providers)
    }

    fn only_wiki(self) -> Option<i64> {
        match self {
            Self::Wiki(id) => Some(id),
            _ => None,
        }
    }

    /// What the `source` option said.
    pub fn from_choice(c: crate::commands::category::SourceChoice) -> Self {
        use crate::commands::category::SourceChoice as C;
        match c {
            C::Default => Self::Auto,
            C::AllWikis => Self::AllWikis,
            C::Wiki(id) => Self::Wiki(id),
            C::Primary | C::Igdb | C::Vndb => Self::Providers,
        }
    }
}

/// The three kinds of result, in the order they are grouped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Work,
    Character,
    Person,
}

/// One result: what to show and the key that opens it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub group: Group,
    pub key: String,
    pub name: String,
    /// "عمل · أنمي · 2019", "شخصية · من Silent Hill", "شخص · مؤدي أصوات".
    pub tail: String,
    pub popularity: i64,
    /// The game or work a character belongs to (used to tell two characters of the same name apart).
    pub parent: Option<String>,
}

impl Entry {
    pub fn icon(&self) -> &'static str {
        match self.group {
            Group::Work => "🎬",
            Group::Character => "👤",
            Group::Person => "🎙️",
        }
    }

    /// The label within Discord's 100 characters: the name gives way, never the kind.
    pub fn label(&self) -> String {
        let head = format!("{} ", self.icon());
        let tail = format!(" · {}", self.tail);
        let room = 100usize.saturating_sub(head.chars().count() + tail.chars().count());
        format!("{head}{}{tail}", fit(&self.name, room.max(10)))
    }
}

/// What a search asked for and what the server allows.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    pub cat: Cat,
    pub ty: Ty,
    pub mature: bool,
    pub games_on: bool,
    /// Ask the wikis live (a submitted search) or read only their local index (autocomplete, which may not wait).
    pub live_wikis: bool,
    pub src: Src,
}

/// Everything found, and the sources that did not answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gathered {
    pub entries: Vec<Entry>,
    pub failed: Vec<String>,
}

impl Gathered {
    pub fn partial_note(&self) -> Option<String> {
        if self.failed.is_empty() {
            None
        } else {
            Some(format!(
                "⚠️ لم يصل رد من {} الآن، فالنتائج ناقصة.",
                self.failed.join(" و")
            ))
        }
    }
}

fn work_tail(kind: &str, year: Option<i32>) -> String {
    match year {
        Some(y) => format!("عمل · {kind} · {y}"),
        None => format!("عمل · {kind}"),
    }
}

/// THE search: every enabled source of the category, in parallel, each through the same functions and caches as the
/// category commands. Lookups never panic and a failed source only appears in `failed`.
pub async fn gather(
    src: &Sources,
    overrides: &std::collections::HashMap<String, tm::Target>,
    p: Params,
    q: &str,
) -> Gathered {
    let q = q.trim();
    let (want_works, want_chars, want_people) = (
        p.ty.includes(Group::Work),
        p.ty.includes(Group::Character),
        p.ty.includes(Group::Person),
    );
    // The `source` option: only the main providers, or only wikis, or the default of both.
    let providers_on = p.src.providers();
    let (want_works_p, want_chars_p, want_people_p) = (
        want_works && providers_on,
        want_chars && providers_on,
        want_people && providers_on,
    );
    let ani_works = async {
        if !(want_works_p && p.cat.anilist()) || q.chars().count() < 2 {
            return Vec::new();
        }
        let kind = match p.cat {
            Cat::Anime => Some(MediaKind::Anime),
            Cat::Manga => Some(MediaKind::Manga),
            Cat::Novel => Some(MediaKind::Novel),
            _ => None,
        };
        let list = match kind {
            Some(k) => {
                src.anilist
                    .search_media(q, k, 10, ContentPolicy::SAFE)
                    .await
            }
            None => src.anilist.search_any(q, 10, ContentPolicy::SAFE).await,
        };
        match list {
            Ok(l) => l
                .iter()
                .map(|m| Entry {
                    group: Group::Work,
                    key: format!("anilist:{}", m.id),
                    name: m.title.main().to_string(),
                    tail: work_tail(cards::kind_label(kind_of(m)), m.year()),
                    popularity: m.popularity.unwrap_or(0),
                    parent: None,
                })
                .collect(),
            Err(e) => {
                tracing::debug!("search: anilist works failed: {e}");
                Vec::new()
            }
        }
    };
    let ani_chars = async {
        if !(want_chars_p && p.cat.anilist()) || q.chars().count() < 2 {
            return Vec::new();
        }
        match src
            .anilist
            .search_characters(q, 10, ContentPolicy::SAFE)
            .await
        {
            Ok(l) => l
                .iter()
                .map(|c| Entry {
                    group: Group::Character,
                    key: c.id.to_string(),
                    name: c.display_name().to_string(),
                    tail: match c.main_work() {
                        Some(w) => format!("شخصية · من {w}"),
                        None => "شخصية".to_string(),
                    },
                    popularity: c.favourites.unwrap_or(0),
                    parent: c.main_work().map(str::to_string),
                })
                .collect(),
            Err(e) => {
                tracing::debug!("search: anilist characters failed: {e}");
                Vec::new()
            }
        }
    };
    let tmdb_works = async {
        if !(want_works_p && p.cat.tmdb()) || !src.tmdb.enabled() || q.chars().count() < 2 {
            return Vec::new();
        }
        let list: Vec<tm::Work> = match p.cat {
            Cat::Drama => {
                find::search(
                    &src.tmdb,
                    find::Cat::Drama,
                    q,
                    &find::Filter::default(),
                    overrides,
                )
                .await
            }
            Cat::Series => {
                find::search(
                    &src.tmdb,
                    find::Cat::Series,
                    q,
                    &find::Filter::default(),
                    overrides,
                )
                .await
            }
            Cat::Movie => {
                find::search(
                    &src.tmdb,
                    find::Cat::Movie,
                    q,
                    &find::Filter::default(),
                    overrides,
                )
                .await
            }
            _ => {
                let (tv, movie) = futures::join!(
                    src.tmdb.search(tm::Kind::Tv, q, 1),
                    src.tmdb.search(tm::Kind::Movie, q, 1)
                );
                tv.into_iter().chain(movie).collect()
            }
        };
        list.iter()
            .filter(|w| !w.blocked())
            .map(|w| Entry {
                group: Group::Work,
                key: w.key(),
                name: w.title.clone(),
                tail: work_tail(
                    match w.kind {
                        Some(tm::Kind::Movie) => i18n::KIND_MOVIE,
                        _ => i18n::KIND_SERIES,
                    },
                    w.year,
                ),
                popularity: w.popularity.map_or(0, |x| (x * 100.0) as i64),
                parent: None,
            })
            .collect()
    };
    let games = async {
        if !(want_works_p && p.cat.games() && p.games_on) || q.chars().count() < 2 {
            return (Vec::new(), Vec::new());
        }
        let found = src
            .providers
            .search_games_found(q, None, p.mature, None)
            .await;
        let failed: Vec<&'static str> = found.failed.iter().map(|s| s.credit()).collect();
        let entries = found
            .works
            .iter()
            .map(|w| Entry {
                group: Group::Work,
                key: w.id.clone(),
                name: w.title.clone(),
                tail: work_tail(
                    if w.source == crate::providers::Source::Vndb {
                        i18n::KIND_VN
                    } else {
                        i18n::KIND_GAME
                    },
                    w.year,
                ),
                popularity: w.rating.map_or(0, |r| (r * 100.0) as i64),
                parent: None,
            })
            .collect();
        (entries, failed)
    };
    let game_chars = async {
        if !(want_chars_p && p.cat.games() && p.games_on) || q.chars().count() < 2 {
            return Vec::new();
        }
        src.providers
            .search_characters_found(q, p.mature)
            .await
            .works
            .iter()
            .map(|w| Entry {
                group: Group::Character,
                key: w.id.clone(),
                name: w.title.clone(),
                tail: match &w.subtitle {
                    Some(s) => format!("شخصية · من {s}"),
                    None => "شخصية".to_string(),
                },
                popularity: 0,
                parent: w.subtitle.clone(),
            })
            .collect()
    };
    let persons = async {
        if !want_people_p || q.chars().count() < 2 {
            return (Vec::new(), Vec::new());
        }
        let found = people::fetch::search(src, p.cat.people_scope(), q, p.mature).await;
        let failed: Vec<&'static str> = found.failed.iter().map(|s| s.credit()).collect();
        let entries = found
            .hits
            .iter()
            .map(|h| Entry {
                group: Group::Person,
                key: h.key.clone(),
                name: h.name.clone(),
                tail: if h.hint.is_empty() {
                    "شخص".to_string()
                } else {
                    format!("شخص · {}", h.hint)
                },
                popularity: h.popularity,
                parent: None,
            })
            .collect();
        (entries, failed)
    };
    // The wikis of the owner (global): searched live for a submitted search, from the local index for autocomplete.
    let wikis = async {
        // By default a wiki joins the searches that can concern its pages (games and the general one); chosen through
        // `source` it is searched whatever the category.
        let relevant = p.src != Src::Auto || p.cat.games();
        if !p.src.wikis() || !relevant || !(want_works || want_chars) || q.chars().count() < 2 {
            return (Vec::new(), Vec::new());
        }
        let found = if p.live_wikis {
            src.wikis.search(q, None, p.src.only_wiki()).await
        } else {
            crate::wiki::service::Found {
                pages: src.wikis.suggest(q, None, p.src.only_wiki(), 10).await,
                failed: Vec::new(),
            }
        };
        let names: std::collections::HashMap<i64, String> = src
            .wikis
            .sources()
            .await
            .iter()
            .map(|w| (w.id, w.name.clone()))
            .collect();
        let entries = found
            .pages
            .iter()
            .filter_map(|pg| {
                let wiki = names.get(&pg.wiki_id).cloned().unwrap_or_default();
                match pg.kind {
                    crate::wiki::Kind::Character if want_chars => Some(Entry {
                        group: Group::Character,
                        key: pg.key(),
                        name: pg.title.clone(),
                        tail: match &pg.parent_title {
                            Some(g) => format!("شخصية · من {g} · {wiki}"),
                            None => format!("شخصية · {wiki}"),
                        },
                        popularity: 0,
                        parent: pg.parent_title.clone(),
                    }),
                    crate::wiki::Kind::Game if want_works => Some(Entry {
                        group: Group::Work,
                        key: pg.key(),
                        name: pg.title.clone(),
                        tail: format!("عمل · {} · {wiki}", i18n::KIND_GAME),
                        popularity: 0,
                        parent: None,
                    }),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        (entries, found.failed)
    };
    let (aw, ac, tw, (gw, gf), gc, (pe, pf), (we, wf)) =
        futures::join!(ani_works, ani_chars, tmdb_works, games, game_chars, persons, wikis);
    let mut out = Gathered::default();
    for list in [aw, tw, gw, ac, gc, pe, we] {
        out.entries.extend(list);
    }
    out.failed.extend(gf.iter().map(|n| n.to_string()));
    out.failed.extend(pf.iter().map(|n| n.to_string()));
    out.failed.extend(wf);
    out.failed.dedup();
    out.entries = dedupe_characters(rank(out.entries, q));
    out
}

/// The same character from several sources is one entry: the same name AND the same game (both known) merge, keeping
/// the first (the best ranked); a name alone never merges two characters.
pub fn dedupe_characters(entries: Vec<Entry>) -> Vec<Entry> {
    let mut kept: Vec<Entry> = Vec::with_capacity(entries.len());
    for e in entries {
        let dup = e.group == Group::Character
            && kept.iter().any(|k| {
                k.group == Group::Character
                    && crate::wiki::same_identity(
                        &k.name,
                        k.parent.as_deref(),
                        &e.name,
                        e.parent.as_deref(),
                    )
            });
        if !dup {
            kept.push(e);
        }
    }
    kept
}

/// Exact name first, then popularity; one entry per key.
pub fn rank(mut entries: Vec<Entry>, q: &str) -> Vec<Entry> {
    let q = q.trim().to_lowercase();
    entries.sort_by_key(|e| {
        (
            std::cmp::Reverse(e.name.to_lowercase() == q),
            std::cmp::Reverse(e.popularity),
        )
    });
    let mut seen = std::collections::HashSet::new();
    entries.retain(|e| seen.insert(e.key.clone()));
    entries
}

/// The entries with the kinds taking turns (works, characters, people), each kind keeping its own order, so no kind
/// can crowd the others out of the first page.
pub fn interleave(entries: &[Entry]) -> Vec<Entry> {
    let mut groups: Vec<std::collections::VecDeque<&Entry>> =
        [Group::Work, Group::Character, Group::Person]
            .iter()
            .map(|g| entries.iter().filter(|e| e.group == *g).collect())
            .collect();
    let mut out = Vec::with_capacity(entries.len());
    while groups.iter().any(|g| !g.is_empty()) {
        for g in groups.iter_mut() {
            if let Some(e) = g.pop_front() {
                out.push(e.clone());
            }
        }
    }
    out
}

/// A balanced mix of at most `n` entries, grouped by kind. A single kind (a `type` filter) just takes the first `n`.
pub fn balance(entries: &[Entry], n: usize) -> Vec<Entry> {
    let mut picked: Vec<Entry> = interleave(entries).into_iter().take(n).collect();
    picked.sort_by_key(|e| e.group);
    picked
}

/// Autocomplete choices of the search: `(label, key)`.
pub fn choices(entries: &[Entry]) -> Vec<(String, String)> {
    balance(entries, crate::ui::autocomplete::MAX_CHOICES)
        .iter()
        .map(|e| (e.label(), e.key.clone()))
        .collect()
}

/// Can this typed value be opened directly (a key picked from the suggestions)?
pub fn is_key(v: &str) -> bool {
    let v = v.trim();
    components::is_work_key(v)
        || people::parse_key(v).is_some()
        || tm::parse_cast_key(v).is_some()
        || v.strip_prefix("igdb:c")
            .or_else(|| v.strip_prefix("vndb:c"))
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        || (!v.is_empty() && v.len() <= 12 && v.chars().all(|c| c.is_ascii_digit()))
}

const PAGE: usize = 25;
const QUERY_IN_ID: usize = 50;

/// The id of a page button: `searchpage:{cat}:{type}:{source}:{page}:{text}` (the text is cut so the id fits 100
/// characters).
pub fn page_id(cat: Cat, ty: Ty, src: Src, page: usize, q: &str) -> String {
    let q: String = q.trim().chars().take(QUERY_IN_ID).collect();
    format!(
        "{}:{}:{}:{}:{page}:{q}",
        components::SEARCH_PAGE,
        cat.key(),
        ty.key(),
        src.code()
    )
}

/// Parse a page id back into `(cat, type, source, page, text)`.
pub fn parse_page_id(id: &str) -> Option<(Cat, Ty, Src, usize, String)> {
    let rest = id.strip_prefix(&format!("{}:", components::SEARCH_PAGE))?;
    let mut p = rest.splitn(5, ':');
    let cat = Cat::from_key(p.next()?)?;
    let ty = Ty::from_key(p.next()?)?;
    let src = Src::from_code(p.next()?)?;
    let page = p.next()?.parse().ok()?;
    let q = p.next()?.to_string();
    (!q.is_empty()).then_some((cat, ty, src, page, q))
}

/// The result message of a free-text search: the entries grouped by kind, a select menu of the page and page buttons.
pub fn results_msg(g: &Gathered, p: Params, q: &str, page: usize) -> Msg {
    if g.entries.is_empty() {
        return Msg::text(
            g.partial_note()
                .unwrap_or_else(|| i18n::NO_RESULTS.to_string()),
        );
    }
    let all = interleave(&g.entries);
    let pages = all.len().div_ceil(PAGE).max(1);
    let page = page.min(pages - 1);
    // Every page is a mix of the kinds, shown grouped.
    let mut slice: Vec<Entry> = all[page * PAGE..((page + 1) * PAGE).min(all.len())].to_vec();
    slice.sort_by_key(|e| e.group);
    let slice = &slice[..];
    let mut lines = Vec::new();
    let mut last = None;
    for e in slice {
        if last != Some(e.group) {
            lines.push(
                match e.group {
                    Group::Work => "**🎬 أعمال**",
                    Group::Character => "**👤 شخصيات**",
                    Group::Person => "**🎙️ أشخاص**",
                }
                .to_string(),
            );
            last = Some(e.group);
        }
        lines.push(format!("• {} — {}", e.name, e.tail));
    }
    if let Some(n) = g.partial_note() {
        lines.push(format!("\n{n}"));
    }
    let items: Vec<(String, String)> = slice.iter().map(|e| (e.label(), e.key.clone())).collect();
    let mut rows: Vec<serenity::CreateActionRow> =
        components::any_menu(i18n::PICK_ANY_PLACEHOLDER, &items)
            .into_iter()
            .collect();
    if pages > 1 {
        let btn = |label: &str, to: usize, off: bool| {
            serenity::CreateButton::new(page_id(p.cat, p.ty, p.src, to, q))
                .label(label)
                .style(serenity::ButtonStyle::Secondary)
                .disabled(off)
        };
        rows.push(serenity::CreateActionRow::Buttons(vec![
            btn("◀", page.saturating_sub(1), page == 0),
            serenity::CreateButton::new("nav:indicator")
                .label(format!("{}/{pages}", page + 1))
                .style(serenity::ButtonStyle::Secondary)
                .disabled(true),
            btn("▶", (page + 1).min(pages - 1), page + 1 >= pages),
        ]));
    }
    let e = Emb::new()
        .title(fit(&i18n::search_results_for(q), 256))
        .description(fit(&lines.join("\n"), 4000))
        .footer(format!("{}/{pages}", page + 1));
    Msg::card(e, rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(group: Group, key: &str, name: &str, pop: i64) -> Entry {
        Entry {
            group,
            key: key.into(),
            name: name.into(),
            tail: match group {
                Group::Work => "عمل · أنمي · 2019".into(),
                Group::Character => "شخصية · من Silent Hill".into(),
                Group::Person => "شخص · مؤدي أصوات".into(),
            },
            popularity: pop,
            parent: None,
        }
    }

    fn mixed() -> Vec<Entry> {
        let mut v = Vec::new();
        for i in 0..30 {
            v.push(entry(
                Group::Work,
                &format!("anilist:{i}"),
                &format!("Work {i}"),
                100 - i,
            ));
        }
        for i in 0..4 {
            v.push(entry(
                Group::Character,
                &format!("{}", 500 + i),
                &format!("Char {i}"),
                50 - i,
            ));
        }
        for i in 0..3 {
            v.push(entry(
                Group::Person,
                &format!("tmdb-person:{i}"),
                &format!("Person {i}"),
                10 - i,
            ));
        }
        v
    }

    #[test]
    fn labels_carry_the_icon_and_kind_and_fit_one_hundred_characters() {
        let e = entry(Group::Work, "anilist:1", "Steins;Gate", 5);
        assert_eq!(e.label(), "🎬 Steins;Gate · عمل · أنمي · 2019");
        assert_eq!(
            entry(Group::Character, "1", "Alice", 1).label(),
            "👤 Alice · شخصية · من Silent Hill"
        );
        assert_eq!(
            entry(Group::Person, "tmdb-person:1", "Actor", 1).label(),
            "🎙️ Actor · شخص · مؤدي أصوات"
        );
        let long = entry(Group::Work, "anilist:2", &"x".repeat(300), 1);
        let l = long.label();
        assert!(l.chars().count() <= 100, "{}", l.chars().count());
        assert!(
            l.ends_with("عمل · أنمي · 2019"),
            "the kind is never cut: {l}"
        );
    }

    #[test]
    fn the_mix_is_balanced_capped_at_25_and_grouped_by_kind() {
        let out = balance(&mixed(), 25);
        assert_eq!(out.len(), 25);
        let count = |g| out.iter().filter(|e| e.group == g).count();
        assert!(
            count(Group::Character) == 4 && count(Group::Person) == 3,
            "small groups are fully shown"
        );
        assert_eq!(count(Group::Work), 18, "works fill the rest");
        // grouped: works, then characters, then people
        let order: Vec<Group> = out.iter().map(|e| e.group).collect();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(order, sorted);
        // only one kind in the list: it takes all 25
        let only: Vec<Entry> = mixed()
            .into_iter()
            .filter(|e| e.group == Group::Work)
            .collect();
        assert_eq!(balance(&only, 25).len(), 25);
    }

    #[test]
    fn values_are_unique_provider_prefixed_keys_within_100_characters() {
        let c = choices(&mixed());
        let mut seen = std::collections::HashSet::new();
        for (label, value) in &c {
            assert!(seen.insert(value.clone()));
            assert!(label.chars().count() <= 100 && value.chars().count() <= 100);
            assert!(is_key(value), "{value}");
        }
        assert!(
            !is_key("silent hill")
                && !is_key("")
                && is_key("anilist:5")
                && is_key("tmdb-cast:tv:1:2")
                && is_key("igdb:c7")
        );
    }

    #[test]
    fn ranking_puts_the_exact_name_first_then_popularity_and_drops_duplicate_keys() {
        let r = rank(
            vec![
                entry(Group::Work, "anilist:1", "Other", 900),
                entry(Group::Work, "anilist:2", "silent hill", 1),
                entry(Group::Work, "anilist:1", "Other", 900),
                entry(Group::Person, "tmdb-person:3", "Big", 950),
            ],
            "Silent Hill",
        );
        assert_eq!(
            r.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
            vec!["anilist:2", "tmdb-person:3", "anilist:1"]
        );
    }

    #[test]
    fn the_type_filter_keeps_only_its_kind() {
        assert!(Ty::All.includes(Group::Person) && Ty::All.includes(Group::Work));
        assert!(Ty::Work.includes(Group::Work) && !Ty::Work.includes(Group::Person));
        assert!(Ty::Person.includes(Group::Person) && !Ty::Person.includes(Group::Character));
        assert!(Ty::Character.includes(Group::Character) && !Ty::Character.includes(Group::Work));
    }

    #[test]
    fn page_ids_round_trip_including_colons_and_stay_short() {
        let id = page_id(Cat::Game, Ty::Person, Src::Wiki(12), 3, "re: zero ماذا");
        assert_eq!(
            parse_page_id(&id),
            Some((
                Cat::Game,
                Ty::Person,
                Src::Wiki(12),
                3,
                "re: zero ماذا".into()
            ))
        );
        let long = page_id(Cat::All, Ty::All, Src::AllWikis, 0, &"ا".repeat(200));
        assert!(long.chars().count() <= 100);
        assert_eq!(parse_page_id("searchpage:bad:all:a:0:x"), None);
        assert_eq!(parse_page_id("searchpage:all:all:a:x:q"), None);
    }

    #[test]
    fn the_result_message_has_a_menu_of_25_and_page_buttons() {
        let g = Gathered {
            entries: mixed(),
            failed: vec![],
        };
        let p = Params {
            cat: Cat::All,
            ty: Ty::All,
            mature: true,
            games_on: true,
            live_wikis: true,
            src: Src::Auto,
        };
        let m = results_msg(&g, p, "silent", 0);
        assert_eq!(m.rows.len(), 2);
        let menu = serde_json::to_value(&m.rows[0]).expect("json");
        assert_eq!(menu["components"][0]["custom_id"], components::PICK_ANY);
        assert_eq!(
            menu["components"][0]["options"].as_array().map(Vec::len),
            Some(25)
        );
        let nav = serde_json::to_value(&m.rows[1]).expect("json");
        assert_eq!(
            nav["components"][2]["custom_id"],
            page_id(Cat::All, Ty::All, Src::Auto, 1, "silent")
        );
        let text = serde_json::to_value(m.embed.expect("embed")).expect("json")["description"]
            .as_str()
            .unwrap_or("")
            .to_string();
        assert!(
            text.contains("🎬 أعمال") && text.contains("👤 شخصيات") && text.contains("🎙️ أشخاص"),
            "{text}"
        );
        // a partial answer is said in Arabic
        let g = Gathered {
            entries: mixed()[..3].to_vec(),
            failed: vec!["IGDB".to_string()],
        };
        let m = results_msg(&g, p, "x", 0);
        assert_eq!(m.rows.len(), 1, "no page buttons for one page");
        let text = serde_json::to_value(m.embed.expect("embed")).expect("json")["description"]
            .as_str()
            .unwrap_or("")
            .to_string();
        assert!(text.contains("IGDB"));
        // nothing found
        let none = results_msg(&Gathered::default(), p, "x", 0);
        assert_eq!(none.text.as_deref(), Some(i18n::NO_RESULTS));
    }

    #[test]
    fn categories_and_types_round_trip_their_keys() {
        for c in [
            Cat::All,
            Cat::Anime,
            Cat::Manga,
            Cat::Novel,
            Cat::Drama,
            Cat::Series,
            Cat::Movie,
            Cat::Game,
        ] {
            assert_eq!(Cat::from_key(c.key()), Some(c));
        }
        for t in [Ty::All, Ty::Work, Ty::Character, Ty::Person] {
            assert_eq!(Ty::from_key(t.key()), Some(t));
        }
        assert_eq!(Cat::Drama.people_scope(), Scope::Screen);
        assert_eq!(Cat::All.people_scope(), Scope::All);
    }

    fn character(key: &str, name: &str, parent: Option<&str>, pop: i64) -> Entry {
        Entry {
            group: Group::Character,
            key: key.into(),
            name: name.into(),
            tail: "شخصية".into(),
            popularity: pop,
            parent: parent.map(str::to_string),
        }
    }

    #[test]
    fn the_same_character_from_a_provider_and_two_wikis_is_one_entry() {
        let merged = dedupe_characters(vec![
            character("igdb:c1", "Pyramid Head", Some("Silent Hill 2"), 9),
            character("wiki:1:5", "Pyramid-Head", Some("silent hill 2"), 0),
            character("wiki:2:9", "Pyramid Head (Silent Hill 2)", None, 0),
        ]);
        assert_eq!(
            merged.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
            vec!["igdb:c1"],
            "one entry, the best ranked"
        );
    }

    #[test]
    fn the_same_name_in_another_game_or_with_no_known_game_stays_separate() {
        let kept = dedupe_characters(vec![
            character("igdb:c1", "Pyramid Head", Some("Silent Hill 2"), 9),
            character(
                "wiki:1:6",
                "Pyramid Head",
                Some("Silent Hill: Homecoming"),
                0,
            ),
            character("wiki:1:7", "Pyramid Head", None, 0),
            character("wiki:1:8", "Heather", Some("Silent Hill 3"), 0),
        ]);
        assert_eq!(kept.len(), 4, "a name alone never merges two characters");
        // works are never merged across sources
        let works = vec![
            Entry {
                group: Group::Work,
                key: "igdb:1".into(),
                name: "Silent Hill 2".into(),
                tail: "x".into(),
                popularity: 1,
                parent: None,
            },
            Entry {
                group: Group::Work,
                key: "wiki:1:2".into(),
                name: "Silent Hill 2".into(),
                tail: "x".into(),
                popularity: 0,
                parent: None,
            },
        ];
        assert_eq!(dedupe_characters(works).len(), 2);
    }

    #[test]
    fn source_codes_round_trip() {
        for s in [
            Src::Auto,
            Src::Providers,
            Src::AllWikis,
            Src::Wiki(7),
            Src::Wiki(1234),
        ] {
            assert_eq!(Src::from_code(&s.code()), Some(s));
        }
        assert_eq!(Src::from_code("x"), None);
        assert_eq!(Src::from_code("wx"), None);
        assert!(Src::Auto.providers() && Src::Auto.wikis());
        assert!(Src::Providers.providers() && !Src::Providers.wikis());
        assert!(!Src::AllWikis.providers() && Src::AllWikis.wikis());
        assert_eq!(Src::Wiki(3).only_wiki(), Some(3));
    }
}
