//! Pure parsers for the MediaWiki API answers (`format=json&formatversion=2`) and the rules that decide which pages
//! count. No network here: the fixtures in the tests are shaped like real answers.

use super::{normalize, Kind, Source};
use serde_json::Value;
use std::collections::HashMap;

/// What a wiki says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub name: String,
    pub lang: String,
    pub license_name: Option<String>,
    pub license_url: Option<String>,
}

/// `meta=siteinfo&siprop=general|rightsinfo`. Anything that is not MediaWiki JSON is refused (`None`).
pub fn parse_siteinfo(doc: &Value) -> Option<Site> {
    let general = doc.pointer("/query/general")?;
    let generator = general.get("generator")?.as_str()?;
    if !generator.to_lowercase().starts_with("mediawiki") {
        return None;
    }
    let name = general.get("sitename")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    let s = |p: &str| {
        doc.pointer(p)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
    };
    Some(Site {
        name: name.to_string(),
        lang: s("/query/general/lang").unwrap_or_else(|| "en".into()),
        license_name: s("/query/rightsinfo/text"),
        license_url: s("/query/rightsinfo/url")
            .filter(|u| u.starts_with("https://") || u.starts_with("http://")),
    })
}

/// A page that came out of a search or a category: enough to ask for its details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub page_id: i64,
    pub title: String,
}

fn members_at(doc: &Value, pointer: &str) -> Vec<Member> {
    doc.pointer(pointer)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|m| m.get("ns").and_then(Value::as_i64).unwrap_or(0) == 0)
                .filter_map(|m| {
                    Some(Member {
                        page_id: m.get("pageid")?.as_i64()?,
                        title: m.get("title")?.as_str()?.to_string(),
                    })
                })
                .filter(|m| title_ok(&m.title))
                .collect()
        })
        .unwrap_or_default()
}

/// `list=search`.
pub fn parse_search(doc: &Value) -> Vec<Member> {
    members_at(doc, "/query/search")
}

/// `list=categorymembers`: the members and the continuation token for the next batch.
pub fn parse_members(doc: &Value) -> (Vec<Member>, Option<String>) {
    let next = doc
        .pointer("/continue/cmcontinue")
        .and_then(Value::as_str)
        .map(str::to_string);
    (members_at(doc, "/query/categorymembers"), next)
}

/// `list=allcategories&acprop=size`: `(category title, pages)`.
pub fn parse_allcategories(doc: &Value) -> Vec<(String, u64)> {
    doc.pointer("/query/allcategories")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    let name = c.get("category")?.as_str()?;
                    let pages = c.get("pages").and_then(Value::as_u64).unwrap_or(0);
                    Some((format!("Category:{name}"), pages))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `prop=categoryinfo`: `(category title, pages)` for the categories that exist.
pub fn parse_categoryinfo(doc: &Value) -> Vec<(String, u64)> {
    doc.pointer("/query/pages")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|p| !p.get("missing").and_then(Value::as_bool).unwrap_or(false))
                .filter_map(|p| {
                    let title = p.get("title")?.as_str()?.to_string();
                    let pages = p.pointer("/categoryinfo/pages").and_then(Value::as_u64)?;
                    Some((title, pages))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The category with the most pages, if any has pages at all.
pub fn pick_category(counts: &[(String, u64)]) -> Option<(String, u64)> {
    counts
        .iter()
        .filter(|(_, n)| *n > 0)
        .max_by_key(|(_, n)| *n)
        .cloned()
}

/// The titles tried for the characters of a wiki (before the `allcategories` prefix search).
pub const CHARACTER_CATEGORIES: &[&str] = &[
    "Category:Characters",
    "Category:Main characters",
    "Category:Supporting characters",
    "Category:Antagonists",
    "Category:Playable characters",
    "Category:Non-playable characters",
    "Category:Heroes",
    "Category:Villains",
];

/// The titles tried for the works of a wiki (of any medium). Episodes, chapters, images, templates and hubs are not
/// works (see `plausible_category`).
pub const WORK_CATEGORIES: &[&str] = &[
    "Category:Games",
    "Category:Video games",
    "Category:Anime",
    "Category:Manga",
    "Category:Manhwa",
    "Category:Manhua",
    "Category:Webtoon",
    "Category:Novels",
    "Category:Light novels",
    "Category:Series",
    "Category:TV series",
    "Category:Films",
    "Category:Movies",
    "Category:Dramas",
];

/// Prefixes of `allcategories` scanned for more candidates.
pub const CHARACTER_PREFIXES: &[&str] = &["Char", "Person", "Cast"];
pub const WORK_PREFIXES: &[&str] = &[
    "Game", "Anime", "Manga", "Manhwa", "Novel", "Series", "Film", "Movie", "Drama",
];

/// A category name as the owner may type it ("Characters" or "Category:Characters").
pub fn category_title(input: &str) -> String {
    let t = input.trim();
    let name = t
        .strip_prefix("Category:")
        .or_else(|| t.strip_prefix("category:"))
        .unwrap_or(t)
        .trim()
        .replace('_', " ");
    format!("Category:{name}")
}

/// Is this category (found by prefix) likely a list of characters or games, rather than a bucket of images,
/// templates, stubs or a hub that only holds other categories?
pub fn plausible_category(title: &str) -> bool {
    let t = format!(
        " {} ",
        normalize(title.strip_prefix("Category:").unwrap_or(title))
    );
    const BAD: &[&str] = &[
        " image",
        " images",
        " icon",
        " icons",
        " gallery",
        " galleries",
        " screenshot",
        " screenshots",
        " template",
        " templates",
        " stub",
        " stubs",
        " navigation",
        " by ",
        " files",
        " audio",
        " sounds",
        " music",
        " quotes",
        " trivia",
        " lists ",
        " list of ",
        " to do ",
        " cleanup",
        " pages ",
        " wanted ",
        " episode",
        " chapter",
        " volumes",
        " seasons",
    ];
    !BAD.iter().any(|b| t.contains(b))
}

/// Pages that never count: other namespaces, subpages ("A/Gallery") and drafts.
pub fn title_ok(title: &str) -> bool {
    let t = title.trim();
    if t.is_empty() || t.contains('/') || t.contains('|') {
        return false;
    }
    let lower = t.to_lowercase();
    const SKIP: &[&str] = &[
        "user:",
        "talk:",
        "file:",
        "image:",
        "template:",
        "category:",
        "help:",
        "mediawiki:",
        "module:",
        "special:",
        "draft:",
        "user talk:",
        "forum:",
        "thread:",
        "blog:",
        "message wall:",
        "board:",
    ];
    !SKIP.iter().any(|p| lower.starts_with(p))
        && !lower.contains("(draft)")
        && !lower.starts_with("draft ")
}

/// Titles to fetch together: at most 50 a request (the API's limit), `|` inside a title removed.
pub fn batches(titles: &[String]) -> Vec<String> {
    titles
        .chunks(50)
        .map(|c| {
            c.iter()
                .map(|t| t.replace('|', " "))
                .collect::<Vec<_>>()
                .join("|")
        })
        .collect()
}

/// A page as the API describes it, before it is judged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPage {
    pub page_id: i64,
    pub title: String,
    pub extract: Option<String>,
    pub image: Option<String>,
    pub categories: Vec<String>,
    pub disambiguation: bool,
    pub redirects_from: Vec<String>,
    /// The infobox "Series" (the parent work as the wiki writes it), when the page has one.
    pub series: Option<String>,
}

/// `prop=extracts|pageimages|categories|pageprops` for a batch of titles. Missing pages, other namespaces, subpages
/// and disambiguation pages are left out here. A title that was reached through a redirect keeps the old title as an
/// alias.
pub fn parse_details(doc: &Value) -> Vec<RawPage> {
    let mut from: HashMap<String, Vec<String>> = HashMap::new();
    for r in doc
        .pointer("/query/redirects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let (Some(f), Some(t)) = (
            r.get("from").and_then(Value::as_str),
            r.get("to").and_then(Value::as_str),
        ) {
            from.entry(t.to_string()).or_default().push(f.to_string());
        }
    }
    doc.pointer("/query/pages")
        .and_then(Value::as_array)
        .map(|pages| {
            pages
                .iter()
                .filter(|p| !p.get("missing").and_then(Value::as_bool).unwrap_or(false))
                .filter(|p| p.get("ns").and_then(Value::as_i64).unwrap_or(0) == 0)
                .filter(|p| p.get("redirect").and_then(Value::as_bool) != Some(true))
                .filter_map(|p| {
                    let title = p.get("title")?.as_str()?.to_string();
                    if !title_ok(&title) {
                        return None;
                    }
                    let text = |k: &str| {
                        p.get(k)
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|t| !t.is_empty())
                            .map(str::to_string)
                    };
                    let image = p
                        .pointer("/thumbnail/source")
                        .or_else(|| p.pointer("/original/source"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    Some(RawPage {
                        page_id: p.get("pageid")?.as_i64()?,
                        redirects_from: from.get(&title).cloned().unwrap_or_default(),
                        title,
                        extract: text("extract"),
                        image,
                        categories: p
                            .get("categories")
                            .and_then(Value::as_array)
                            .map(|c| {
                                c.iter()
                                    .filter_map(|c| c.get("title").and_then(Value::as_str))
                                    .map(|t| t.strip_prefix("Category:").unwrap_or(t).to_string())
                                    .collect()
                            })
                            .unwrap_or_default(),
                        disambiguation: p.pointer("/pageprops/disambiguation").is_some(),
                        series: p
                            .pointer("/revisions/0/slots/main/content")
                            .and_then(Value::as_str)
                            .and_then(super::clean::series),
                    })
                })
                .filter(|p| !p.disambiguation)
                .collect()
        })
        .unwrap_or_default()
}

/// The game or series a page belongs to: from a category like "Silent Hill 2 characters", else from the title's
/// qualifier "Pyramid Head (Silent Hill 2)".
pub fn infer_parent(title: &str, categories: &[String]) -> Option<String> {
    const SUFFIXES: &[&str] = &[
        " characters",
        " character",
        " enemies",
        " bosses",
        " monsters",
        " protagonists",
        " antagonists",
        " villains",
    ];
    for c in categories {
        let lower = c.to_lowercase();
        for s in SUFFIXES {
            if let Some(stem) = lower.strip_suffix(s) {
                let cut = c.chars().count() - s.chars().count();
                let name: String = c.chars().take(cut).collect();
                let generic = [
                    "playable",
                    "main",
                    "non-playable",
                    "minor",
                    "recurring",
                    "other",
                    "major",
                    "supporting",
                    "female",
                    "male",
                ];
                if !stem.trim().is_empty() && !generic.contains(&stem.trim()) {
                    return Some(name.trim().to_string());
                }
            }
        }
    }
    super::title_qualifier(title).map(str::to_string)
}

/// The medium a work page belongs to: the wiki's manual hint, then the page's own categories ("Manhwa", "Light novels",
/// "Silent Hill games"), then the name of the wiki's work category; unknown stays `Work` (never assumed a game).
pub fn work_kind(w: &Source, categories: &[String]) -> Kind {
    if let Some(k) = w
        .kind_hint
        .as_deref()
        .map(Kind::from_key)
        .filter(|k| k.is_work())
    {
        return k;
    }
    categories
        .iter()
        .find_map(|c| category_medium(c))
        .or_else(|| {
            w.work_category
                .as_deref()
                .and_then(|c| category_medium(c.strip_prefix("Category:").unwrap_or(c)))
        })
        .unwrap_or(Kind::Work)
}

/// The medium a category name ends with: "Manhwa", "Silent Hill games", "Light novels"; none for "Characters" or "Tropes".
fn category_medium(category: &str) -> Option<Kind> {
    let n = normalize(category);
    let words: Vec<&str> = n.split(' ').collect();
    let last = words.last().copied()?;
    let two = words
        .len()
        .checked_sub(2)
        .map(|i| words[i..].join(" "))
        .unwrap_or_default();
    Kind::medium_of(&two).or_else(|| Kind::medium_of(last))
}

/// The medium of a whole wiki, from its manual hint or the name of its work category (none for a mixed wiki).
pub fn wiki_medium(w: &Source) -> Option<Kind> {
    w.kind_hint
        .as_deref()
        .map(Kind::from_key)
        .filter(|k| k.is_work())
        .or_else(|| {
            w.work_category
                .as_deref()
                .and_then(|c| category_medium(c.strip_prefix("Category:").unwrap_or(c)))
        })
}

/// What a page is: a work when it is in the wiki's work category (its medium from `work_kind`), a character when it is
/// in the character category or any category that names characters, a work of a known medium when one of its
/// categories says so, otherwise something else.
pub fn classify(w: &Source, categories: &[String]) -> Kind {
    let norm: Vec<String> = categories.iter().map(|c| normalize(c)).collect();
    let in_cat = |wanted: &Option<String>| {
        wanted
            .as_deref()
            .map(|c| normalize(c.strip_prefix("Category:").unwrap_or(c)))
            .is_some_and(|c| !c.is_empty() && norm.contains(&c))
    };
    if in_cat(&w.work_category) {
        return work_kind(w, categories);
    }
    if in_cat(&w.character_category)
        || norm.iter().any(|c| {
            c.ends_with(" characters")
                || c == "characters"
                || c.ends_with(" character")
                || c.ends_with(" enemies")
                || c.ends_with(" bosses")
                || c.ends_with(" protagonists")
                || c.ends_with(" antagonists")
        })
    {
        return Kind::Character;
    }
    // A category that names a medium ("Video games", "Manhwa") makes the page a work of that medium.
    if categories
        .iter()
        .any(|c| plausible_category(c) && category_medium(c).is_some())
    {
        return work_kind(w, categories);
    }
    Kind::Other
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn source() -> Source {
        Source {
            id: 1,
            name: "Silent Hill Wiki".into(),
            host: "silenthill.fandom.com".into(),
            api_url: "https://silenthill.fandom.com/api.php".into(),
            lang: "en".into(),
            license_name: None,
            license_url: None,
            character_category: Some("Category:Characters".into()),
            work_category: Some("Category:Games".into()),
            kind_hint: None,
            enabled: true,
            added_by: None,
            created_at: 0,
            last_checked_at: None,
            last_error: None,
        }
    }

    #[test]
    fn siteinfo_gives_name_language_and_license() {
        let doc = json!({"query": {"general": {"sitename": "Silent Hill Wiki", "lang": "en", "generator": "MediaWiki 1.43.0"},
            "rightsinfo": {"url": "https://creativecommons.org/licenses/by-sa/3.0/", "text": "Creative Commons Attribution-Share Alike"}}});
        let s = parse_siteinfo(&doc).expect("a MediaWiki");
        assert_eq!(s.name, "Silent Hill Wiki");
        assert_eq!(s.lang, "en");
        assert_eq!(
            s.license_name.as_deref(),
            Some("Creative Commons Attribution-Share Alike")
        );
        assert_eq!(
            s.license_url.as_deref(),
            Some("https://creativecommons.org/licenses/by-sa/3.0/")
        );
        // no rights info: still valid, no license
        let bare = json!({"query": {"general": {"sitename": "X", "generator": "MediaWiki 1.39"}}});
        let s = parse_siteinfo(&bare).expect("valid");
        assert!(s.license_name.is_none() && s.lang == "en");
    }

    #[test]
    fn anything_that_is_not_mediawiki_json_is_refused() {
        assert!(parse_siteinfo(&json!({"error": "nope"})).is_none());
        assert!(parse_siteinfo(
            &json!({"query": {"general": {"sitename": "X", "generator": "WordPress 6"}}})
        )
        .is_none());
        assert!(
            parse_siteinfo(&json!({"query": {"general": {"generator": "MediaWiki 1.43"}}}))
                .is_none(),
            "no name"
        );
        assert!(parse_siteinfo(&json!("<html>")).is_none());
        assert!(parse_siteinfo(
            &json!({"query": {"general": {"sitename": " ", "generator": "MediaWiki 1.43"}}})
        )
        .is_none());
    }

    #[test]
    fn search_and_category_results_keep_only_real_pages() {
        let doc = json!({"query": {"search": [
            {"ns": 0, "title": "Pyramid Head", "pageid": 10},
            {"ns": 1, "title": "Talk:Pyramid Head", "pageid": 11},
            {"ns": 0, "title": "Pyramid Head/Gallery", "pageid": 12},
            {"ns": 0, "title": "User:Someone", "pageid": 13},
            {"ns": 0, "title": "Heather Mason", "pageid": 14}]}});
        let m = parse_search(&doc);
        assert_eq!(
            m.iter().map(|m| m.title.as_str()).collect::<Vec<_>>(),
            vec!["Pyramid Head", "Heather Mason"]
        );
        let cat = json!({"continue": {"cmcontinue": "page|ABC|1", "continue": "-||"}, "query": {"categorymembers": [{"pageid": 1, "ns": 0, "title": "A"}]}});
        let (members, next) = parse_members(&cat);
        assert_eq!((members.len(), next.as_deref()), (1, Some("page|ABC|1")));
        let last = json!({"query": {"categorymembers": []}});
        assert_eq!(
            parse_members(&last),
            (vec![], None),
            "no continuation on the last batch"
        );
    }

    #[test]
    fn details_skip_missing_redirect_disambiguation_and_other_namespaces_and_keep_aliases() {
        let doc = json!({"query": {
            "redirects": [{"from": "Pyramid-Head", "to": "Pyramid Head"}],
            "pages": [
                {"pageid": 10, "ns": 0, "title": "Pyramid Head", "extract": "A monster.", "thumbnail": {"source": "https://static.wikia.nocookie.net/x/t.png"},
                 "original": {"source": "https://static.wikia.nocookie.net/x/o.png"}, "categories": [{"ns": 14, "title": "Category:Silent Hill 2 characters"}, {"ns": 14, "title": "Category:Characters"}]},
                {"pageid": 20, "ns": 0, "title": "Heather", "pageprops": {"disambiguation": ""}, "extract": "may refer to"},
                {"ns": 0, "title": "Nothing here", "missing": true},
                {"pageid": 30, "ns": 6, "title": "File:Pic.png"},
                {"pageid": 40, "ns": 0, "title": "Pyramid Head/Gallery"},
                {"pageid": 50, "ns": 0, "title": "Some Redirect", "redirect": true}]}});
        let pages = parse_details(&doc);
        assert_eq!(pages.len(), 1);
        let p = &pages[0];
        assert_eq!((p.page_id, p.title.as_str()), (10, "Pyramid Head"));
        assert_eq!(
            p.image.as_deref(),
            Some("https://static.wikia.nocookie.net/x/t.png"),
            "the thumbnail is preferred"
        );
        assert_eq!(p.categories, vec!["Silent Hill 2 characters", "Characters"]);
        assert_eq!(p.redirects_from, vec!["Pyramid-Head"]);
    }

    #[test]
    fn titles_are_batched_at_fifty_and_pipes_never_split_a_title() {
        let titles: Vec<String> = (0..120).map(|i| format!("Page {i}")).collect();
        let b = batches(&titles);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].split('|').count(), 50);
        assert_eq!(b[2].split('|').count(), 20);
        assert_eq!(batches(&["A|B".to_string()]), vec!["A B"]);
        assert!(batches(&[]).is_empty());
    }

    #[test]
    fn the_category_with_the_most_pages_wins() {
        let counts = vec![
            ("Category:Characters".to_string(), 40),
            ("Category:Heroes".to_string(), 3),
            ("Category:Villains".to_string(), 0),
        ];
        assert_eq!(
            pick_category(&counts),
            Some(("Category:Characters".to_string(), 40))
        );
        assert_eq!(pick_category(&[("Category:X".to_string(), 0)]), None);
        assert_eq!(pick_category(&[]), None);
        let info = json!({"query": {"pages": [
            {"title": "Category:Characters", "categoryinfo": {"size": 40, "pages": 38}},
            {"title": "Category:Heroes", "missing": true},
            {"title": "Category:Villains", "categoryinfo": {"size": 5, "pages": 5}}]}});
        assert_eq!(
            parse_categoryinfo(&info),
            vec![
                ("Category:Characters".to_string(), 38),
                ("Category:Villains".to_string(), 5)
            ]
        );
        let all = json!({"query": {"allcategories": [{"category": "Characters by game", "pages": 2}, {"category": "Character images", "pages": 9}]}});
        assert_eq!(
            parse_allcategories(&all)[1],
            ("Category:Character images".to_string(), 9)
        );
        assert_eq!(category_title("Characters"), "Category:Characters");
        assert_eq!(
            category_title(" category:Playable_characters "),
            "Category:Playable characters"
        );
    }

    #[test]
    fn categories_found_by_prefix_must_look_like_lists_of_characters() {
        for ok in [
            "Category:Characters",
            "Category:Playable characters",
            "Category:Games",
            "Category:Silent Hill 2 characters",
        ] {
            assert!(plausible_category(ok), "{ok}");
        }
        for bad in [
            "Category:Character images",
            "Category:Characters by game",
            "Category:Character templates",
            "Category:Character stubs",
            "Category:Game screenshots",
            "Category:Character icons",
        ] {
            assert!(!plausible_category(bad), "{bad}");
        }
    }

    #[test]
    fn the_game_of_a_page_comes_from_its_category_or_its_title() {
        assert_eq!(
            infer_parent(
                "Pyramid Head",
                &["Silent Hill 2 characters".into(), "Characters".into()]
            )
            .as_deref(),
            Some("Silent Hill 2")
        );
        assert_eq!(
            infer_parent("Pyramid Head (Silent Hill 2)", &["Characters".into()]).as_deref(),
            Some("Silent Hill 2")
        );
        assert_eq!(
            infer_parent(
                "Heather",
                &["Playable characters".into(), "Main characters".into()]
            ),
            None,
            "generic categories name no game"
        );
        assert_eq!(infer_parent("Heather", &[]), None);
    }

    #[test]
    fn pages_are_classified_by_the_wikis_categories() {
        let w = source();
        assert_eq!(classify(&w, &["Characters".into()]), Kind::Character);
        assert_eq!(
            classify(&w, &["Silent Hill 2 characters".into()]),
            Kind::Character
        );
        assert_eq!(classify(&w, &["Games".into()]), Kind::Game);
        assert_eq!(classify(&w, &["Locations".into()]), Kind::Other);
        let mut custom = source();
        custom.character_category = Some("Category:Cast".into());
        assert_eq!(classify(&custom, &["Cast".into()]), Kind::Character);
    }
}
