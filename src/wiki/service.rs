//! The wiki service: what the rest of the bot calls. It reads the owner's wikis (cached for a minute, dropped whenever
//! they change, so a new wiki is live in every server at once), searches them in parallel with a short timeout so one
//! slow wiki never fails a search, keeps the page cache and answers autocomplete from it alone.

use super::{
    client::{
        Api, Detection, HttpTransport, Transport, WikiError, BACKGROUND_TIMEOUT,
        INTERACTIVE_TIMEOUT,
    },
    normalize, parse, safety, store, AddressError, Kind, Page, Source,
};
use moka::future::Cache;
use sqlx::SqlitePool;
use std::{
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

/// What the owner sees before confirming a new wiki.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub host: String,
    pub api_url: String,
    pub name: String,
    pub lang: String,
    pub license_name: Option<String>,
    pub license_url: Option<String>,
    pub detection: Detection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddError {
    Address(AddressError),
    /// None of the addresses answered like a MediaWiki.
    Unreachable(WikiError),
}

/// A search over the wikis: the pages, and the wikis that did not answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    pub pages: Vec<Page>,
    pub failed: Vec<String>,
}

/// Pages from a batch of raw answers: classified, the game inferred, aliases from redirects, and judged by the content
/// filter (a doubtful page is dropped without a word).
pub fn to_pages(w: &Source, raws: Vec<parse::RawPage>) -> Vec<Page> {
    raws.into_iter()
        .filter_map(|r| {
            let mut aliases = r.redirects_from.clone();
            let base = super::base_title(&r.title).to_string();
            if base != r.title {
                aliases.push(base);
            }
            aliases.dedup();
            let kind = parse::classify(w, &r.categories);
            // The infobox "Series" is the parent as the wiki writes it; otherwise the categories or the title say.
            let parent = r
                .series
                .clone()
                .or_else(|| parse::infer_parent(&r.title, &r.categories));
            safety::clean_page(Page {
                wiki_id: w.id,
                page_id: r.page_id,
                title: r.title,
                kind,
                // Sentences about a wiki bot are dropped; nothing left means no summary (nothing is invented).
                extract: r.extract.as_deref().and_then(super::clean::clean_extract),
                image_url: r.image,
                aliases,
                parent_title: parent,
                categories: r.categories,
            })
        })
        .collect()
}

pub struct Wikis<T: Transport = HttpTransport> {
    pub pool: SqlitePool,
    pub api: Api<T>,
    sources: Cache<(), Arc<Vec<Source>>>,
    searches: Cache<String, Arc<Vec<Page>>>,
    /// Fills for cards (a matching page's picture and text), kept for a week; a "no match" is kept too.
    pub(super) fills: Cache<String, Arc<Option<super::fill::Fill>>>,
    requests: AtomicU64,
    day: AtomicI64,
}

fn today() -> i64 {
    chrono::Utc::now().timestamp() / 86_400
}

impl<T: Transport + 'static> Wikis<T> {
    pub fn new(pool: SqlitePool, transport: Arc<T>) -> Self {
        Self {
            pool,
            api: Api::new(transport),
            // A short cache: the wikis are read from the database at most once a minute, and dropped at once when the
            // owner adds, edits or deletes one.
            sources: Cache::builder()
                .time_to_live(Duration::from_secs(60))
                .max_capacity(1)
                .build(),
            searches: Cache::builder()
                .time_to_live(Duration::from_secs(3600))
                .max_capacity(5_000)
                .build(),
            fills: Cache::builder()
                .time_to_live(Duration::from_secs(7 * 24 * 3600))
                .max_capacity(10_000)
                .build(),
            requests: AtomicU64::new(0),
            day: AtomicI64::new(today()),
        }
    }

    fn count_request(&self) {
        let d = today();
        if self.day.swap(d, Ordering::Relaxed) != d {
            self.requests.store(0, Ordering::Relaxed);
        }
        self.requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn requests_today(&self) -> u64 {
        if self.day.load(Ordering::Relaxed) == today() {
            self.requests.load(Ordering::Relaxed)
        } else {
            0
        }
    }

    /// Forget what was read: called after every change the owner makes.
    pub async fn invalidate(&self) {
        self.sources.invalidate_all();
        self.searches.invalidate_all();
        self.fills.invalidate_all();
        self.sources.run_pending_tasks().await;
    }

    /// Every wiki that takes part in searches right now: none when the kill switch is off.
    pub async fn sources(&self) -> Arc<Vec<Source>> {
        if !store::global_enabled(&self.pool).await {
            return Arc::new(Vec::new());
        }
        if let Some(hit) = self.sources.get(&()).await {
            return hit;
        }
        let list: Vec<Source> = store::list(&self.pool)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.enabled)
            .collect();
        let list = Arc::new(list);
        self.sources.insert((), list.clone()).await;
        list
    }

    /// The wiki with this id, if it takes part in searches.
    pub async fn source(&self, id: i64) -> Option<Source> {
        self.sources().await.iter().find(|s| s.id == id).cloned()
    }

    // ---- adding a wiki ----------------------------------------------------------------------------

    /// Check an address and discover what the wiki offers, for the owner to confirm.
    pub async fn preview(&self, input: &str) -> Result<Preview, AddError> {
        let (host, candidates) = super::normalize_input(input).map_err(AddError::Address)?;
        self.count_request();
        let (api_url, site) = self
            .api
            .discover(&candidates)
            .await
            .map_err(AddError::Unreachable)?;
        let detection = self.api.detect(&api_url).await;
        Ok(Preview {
            host,
            api_url,
            name: site.name,
            lang: site.lang,
            license_name: site.license_name,
            license_url: site.license_url,
            detection,
        })
    }

    /// Save a confirmed wiki (enabled at once for every server). `characters` and `games` are the categories the owner
    /// accepted or typed; blank means search-only.
    pub async fn save(
        &self,
        p: &Preview,
        name: Option<&str>,
        characters: Option<&str>,
        games: Option<&str>,
        added_by: i64,
    ) -> Result<store::Added, sqlx::Error> {
        let clean = |c: Option<&str>| {
            c.map(str::trim)
                .filter(|c| !c.is_empty())
                .map(parse::category_title)
        };
        let added = store::insert(
            &self.pool,
            &store::NewSource {
                name: name
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .unwrap_or(&p.name)
                    .chars()
                    .take(80)
                    .collect(),
                host: p.host.clone(),
                api_url: p.api_url.clone(),
                lang: p.lang.clone(),
                license_name: p.license_name.clone(),
                license_url: p.license_url.clone(),
                character_category: clean(characters),
                work_category: clean(games),
                kind_hint: None,
                added_by: Some(added_by),
            },
        )
        .await?;
        self.invalidate().await;
        Ok(added)
    }

    /// "اختبار الاتصال": one siteinfo call, recorded on the wiki.
    pub async fn test(&self, w: &Source) -> Result<(), WikiError> {
        self.count_request();
        let r = self.api.siteinfo(&w.api_url).await.map(|_| ());
        let _ = store::mark_checked(
            &self.pool,
            w.id,
            chrono::Utc::now().timestamp(),
            r.as_ref().err().map(|e| e.to_string()).as_deref(),
        )
        .await;
        r
    }

    // ---- searching --------------------------------------------------------------------------------

    async fn search_one(
        &self,
        w: &Source,
        query: &str,
        kind: Option<Kind>,
    ) -> Result<Arc<Vec<Page>>, Arc<WikiError>> {
        let key = format!(
            "{}|{}|{}",
            w.id,
            normalize(query),
            kind.map_or("any", Kind::key)
        );
        self.searches
            .try_get_with(key, async {
                self.count_request();
                let members = self
                    .api
                    .search(&w.api_url, query, 10, INTERACTIVE_TIMEOUT)
                    .await?;
                let now = chrono::Utc::now().timestamp();
                let mut pages: Vec<Page> = Vec::new();
                let mut missing: Vec<String> = Vec::new();
                for m in &members {
                    match store::get_page(&self.pool, w.id, m.page_id, now).await {
                        Ok(Some(p)) => pages.push(p),
                        _ => missing.push(m.title.clone()),
                    }
                }
                if !missing.is_empty() {
                    self.count_request();
                    let fetched = to_pages(
                        w,
                        self.api
                            .details(&w.api_url, &missing, INTERACTIVE_TIMEOUT)
                            .await?,
                    );
                    for p in &fetched {
                        let _ = store::upsert(&self.pool, p, now).await;
                    }
                    pages.extend(fetched);
                }
                // keep the wiki's own order
                let order = |p: &Page| {
                    members
                        .iter()
                        .position(|m| m.page_id == p.page_id)
                        .unwrap_or(usize::MAX)
                };
                pages.sort_by_key(order);
                pages.retain(|p| kind.is_none_or(|k| k.matches(p.kind)));
                Ok::<_, WikiError>(Arc::new(pages))
            })
            .await
    }

    /// Search the enabled wikis (or only `only`) in parallel. A wiki that fails or times out is named in `failed` and
    /// its pages already in the local cache stand in for it; the others are not affected.
    pub async fn search(&self, query: &str, kind: Option<Kind>, only: Option<i64>) -> Found {
        let q = query.trim();
        if q.chars().count() < 2 {
            return Found::default();
        }
        let sources: Vec<Source> = self
            .sources()
            .await
            .iter()
            .filter(|s| only.is_none_or(|id| id == s.id))
            .cloned()
            .collect();
        let results = futures::future::join_all(
            sources
                .iter()
                .map(|w| async move { (w, self.search_one(w, q, kind).await) }),
        )
        .await;
        let now = chrono::Utc::now().timestamp();
        let mut out = Found::default();
        for (w, r) in results {
            match r {
                Ok(pages) => {
                    let _ = store::mark_checked(&self.pool, w.id, now, None).await;
                    out.pages.extend(pages.iter().cloned());
                }
                Err(e) => {
                    tracing::debug!(wiki = %w.host, "wiki search failed: {e}");
                    let _ = store::mark_checked(&self.pool, w.id, now, Some(&e.to_string())).await;
                    out.failed.push(w.name.clone());
                    out.pages.extend(
                        store::suggest(&self.pool, q, kind, Some(w.id), 5, now)
                            .await
                            .unwrap_or_default(),
                    );
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        out.pages.retain(|p| seen.insert((p.wiki_id, p.page_id)));
        out
    }

    /// Suggestions for autocomplete: the local index only, never a wiki.
    pub async fn suggest(
        &self,
        text: &str,
        kind: Option<Kind>,
        only: Option<i64>,
        limit: i64,
    ) -> Vec<Page> {
        if !store::global_enabled(&self.pool).await {
            return Vec::new();
        }
        store::suggest(
            &self.pool,
            text,
            kind,
            only,
            limit,
            chrono::Utc::now().timestamp(),
        )
        .await
        .unwrap_or_default()
    }

    /// One page by its key parts: from the cache, else fetched by id from its wiki.
    pub async fn page(&self, wiki_id: i64, page_id: i64) -> Option<(Page, Source)> {
        let w = self.source(wiki_id).await?;
        let now = chrono::Utc::now().timestamp();
        if let Ok(Some(p)) = store::get_page(&self.pool, wiki_id, page_id, now).await {
            return Some((p, w));
        }
        self.count_request();
        let doc = self
            .api
            .t
            .get(
                &w.api_url,
                vec![
                    ("action".into(), "query".into()),
                    ("pageids".into(), page_id.to_string()),
                    (
                        "prop".into(),
                        "extracts|pageimages|categories|pageprops".into(),
                    ),
                    ("exintro".into(), "1".into()),
                    ("explaintext".into(), "1".into()),
                    ("exsentences".into(), "4".into()),
                    ("piprop".into(), "original|thumbnail".into()),
                    ("pithumbsize".into(), "600".into()),
                    ("cllimit".into(), "max".into()),
                    ("clshow".into(), "!hidden".into()),
                ],
                INTERACTIVE_TIMEOUT,
            )
            .await
            .ok()?;
        let page = to_pages(&w, parse::parse_details(&doc))
            .into_iter()
            .find(|p| p.page_id == page_id)?;
        let _ = store::upsert(&self.pool, &page, now).await;
        Some((page, w))
    }

    /// The characters of a game: the cached pages of that wiki that belong to it; when there are none the character
    /// category is fetched first (a few batches, within a bounded time).
    pub async fn characters_of(&self, wiki_id: i64, game_title: &str) -> Vec<Page> {
        let Some(w) = self.source(wiki_id).await else {
            return Vec::new();
        };
        let parent = normalize(super::base_title(game_title));
        let now = chrono::Utc::now().timestamp();
        let cached = store::characters_of(&self.pool, wiki_id, &parent, now)
            .await
            .unwrap_or_default();
        if !cached.is_empty() || w.character_category.is_none() {
            return cached;
        }
        self.fetch_category(&w, 150).await;
        store::characters_of(&self.pool, wiki_id, &parent, chrono::Utc::now().timestamp())
            .await
            .unwrap_or_default()
    }

    /// Fetch the members of the wiki's character category and the details of up to `limit` of them into the cache.
    /// Returns how many pages were stored.
    pub async fn fetch_category(&self, w: &Source, limit: usize) -> usize {
        let Some(cat) = w.character_category.as_deref() else {
            return 0;
        };
        self.count_request();
        let Ok(members) = self
            .api
            .category_members(&w.api_url, cat, 500, BACKGROUND_TIMEOUT)
            .await
        else {
            return 0;
        };
        let now = chrono::Utc::now().timestamp();
        let mut todo: Vec<String> = Vec::new();
        for m in members.iter().take(limit.max(1) * 3) {
            if store::get_page(&self.pool, w.id, m.page_id, now)
                .await
                .ok()
                .flatten()
                .is_none()
            {
                todo.push(m.title.clone());
            }
            if todo.len() >= limit {
                break;
            }
        }
        let mut stored = 0;
        for batch in todo.chunks(50) {
            self.count_request();
            match self
                .api
                .details(&w.api_url, batch, BACKGROUND_TIMEOUT)
                .await
            {
                Ok(raws) => {
                    for p in to_pages(w, raws) {
                        if store::upsert(&self.pool, &p, now).await.is_ok() {
                            stored += 1;
                        }
                    }
                }
                Err(e) => {
                    let _ = store::mark_checked(&self.pool, w.id, now, Some(&e.to_string())).await;
                    break;
                }
            }
        }
        stored
    }

    /// The background warmer: fills the cache of wikis that have few cached pages, a little at a time, so autocomplete
    /// has something to suggest before anyone searched. Low priority: it runs rarely and stops at the first failure.
    pub async fn warm_once(&self) -> usize {
        let mut total = 0;
        for w in self.sources().await.iter() {
            if w.character_category.is_none() {
                continue;
            }
            let have: i64 =
                sqlx::query_scalar("SELECT count(*) FROM wiki_cache WHERE wiki_id = ?1")
                    .bind(w.id)
                    .fetch_one(&self.pool)
                    .await
                    .unwrap_or(0);
            if have < 200 {
                total += self.fetch_category(w, 100).await;
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
        let _ = store::purge_stale(&self.pool, chrono::Utc::now().timestamp()).await;
        total
    }

    pub async fn stats(&self) -> Stats {
        let list = store::list(&self.pool).await.unwrap_or_default();
        Stats {
            wikis: list.len(),
            enabled: list.iter().filter(|w| w.enabled).count(),
            pages: store::page_count(&self.pool).await.unwrap_or(0),
            requests_today: self.requests_today(),
            last_errors: list
                .iter()
                .filter_map(|w| w.last_error.as_ref().map(|e| format!("{}: {e}", w.name)))
                .take(3)
                .collect(),
            global: store::global_enabled(&self.pool).await,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stats {
    pub wikis: usize,
    pub enabled: usize,
    pub pages: i64,
    pub requests_today: u64,
    pub last_errors: Vec<String>,
    pub global: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki::client::fake::FakeWiki;
    use serde_json::json;
    use std::collections::HashMap;

    fn fake() -> FakeWiki {
        let mut members = HashMap::new();
        members.insert(
            "Category:Characters".to_string(),
            vec![
                (1, "Pyramid Head".to_string()),
                (2, "Heather Mason".to_string()),
                (3, "Pyramid Head (Homecoming)".to_string()),
            ],
        );
        FakeWiki {
            categories: vec![
                ("Category:Characters".into(), 3),
                ("Category:Games".into(), 2),
            ],
            members,
            pages: vec![
                (
                    1,
                    "Pyramid Head".into(),
                    vec!["Silent Hill 2 characters".into(), "Characters".into()],
                    json!({"thumbnail": {"source": "https://static.wikia.nocookie.net/x/a.png"}}),
                ),
                (
                    2,
                    "Heather Mason".into(),
                    vec!["Silent Hill 3 characters".into(), "Characters".into()],
                    json!({}),
                ),
                (
                    3,
                    "Pyramid Head (Homecoming)".into(),
                    vec!["Characters".into()],
                    json!({}),
                ),
                (
                    9,
                    "Hentai Page".into(),
                    vec!["Characters".into()],
                    json!({}),
                ),
                (
                    10,
                    "Rusty Locker".into(),
                    vec!["Locations".into()],
                    json!({}),
                ),
            ],
            search_titles: vec![
                (1, "Pyramid Head".into()),
                (3, "Pyramid Head (Homecoming)".into()),
                (9, "Hentai Page".into()),
                (10, "Rusty Locker".into()),
            ],
            ..FakeWiki::default()
        }
    }

    async fn service(f: FakeWiki) -> (Wikis<FakeWiki>, Arc<FakeWiki>) {
        let pool = crate::db::connect_memory().await;
        let t = Arc::new(f);
        (Wikis::new(pool, t.clone()), t)
    }

    async fn add(w: &Wikis<FakeWiki>, host: &str) -> i64 {
        let p = w
            .preview(&format!("https://{host}/wiki/Main_Page"))
            .await
            .expect("preview");
        match w
            .save(
                &p,
                None,
                Some("Category:Characters"),
                Some("Category:Games"),
                1,
            )
            .await
            .expect("save")
        {
            store::Added::New(id) => id,
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_preview_validates_discovers_and_a_saved_wiki_is_live_at_once_everywhere() {
        let (w, t) = service(fake()).await;
        assert!(w.sources().await.is_empty());
        let p = w.preview("silenthill.fandom.com").await.expect("preview");
        assert_eq!(p.name, "Silent Hill Wiki");
        assert_eq!(p.detection.character.as_ref().map(|c| c.1), Some(3));
        assert_eq!(p.detection.samples.len(), 3);
        assert!(p.license_name.as_deref() == Some("CC BY-SA 3.0"));
        // nothing is saved until the owner confirms
        assert!(store::list(&w.pool).await.expect("list").is_empty());
        let id = match w
            .save(&p, None, Some("Characters"), None, 7)
            .await
            .expect("save")
        {
            store::Added::New(id) => id,
            other => panic!("{other:?}"),
        };
        // read at once, without a restart, by whoever asks next (there is no per-server state at all)
        let live = w.sources().await;
        assert_eq!(live.len(), 1);
        assert!(live[0].enabled && live[0].added_by == Some(7));
        assert_eq!(
            live[0].character_category.as_deref(),
            Some("Category:Characters"),
            "typed categories are normalised"
        );
        assert_eq!(live[0].work_category, None, "search-only for games");
        assert!(w.source(id).await.is_some());
        assert!(!t.calls.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn bad_addresses_never_reach_the_network_and_non_mediawiki_hosts_are_refused() {
        let (w, t) = service(fake()).await;
        for bad in [
            "http://a.fandom.com",
            "https://127.0.0.1/api.php",
            "https://localhost",
            "https://192.168.0.1",
            "",
        ] {
            assert!(
                matches!(w.preview(bad).await, Err(AddError::Address(_))),
                "{bad}"
            );
        }
        assert!(
            t.calls.lock().expect("lock").is_empty(),
            "refused before any request"
        );
        let (w, _) = service(FakeWiki {
            not_mediawiki: true,
            ..FakeWiki::default()
        })
        .await;
        assert!(matches!(
            w.preview("example.com").await,
            Err(AddError::Unreachable(WikiError::NotMediaWiki))
        ));
    }

    #[tokio::test]
    async fn the_cached_wiki_list_follows_every_change_immediately() {
        let (w, _) = service(fake()).await;
        let a = add(&w, "a.fandom.com").await;
        assert_eq!(w.sources().await.len(), 1);
        let b = add(&w, "b.fandom.com").await;
        assert_eq!(
            w.sources().await.len(),
            2,
            "a second wiki is visible at once"
        );
        store::set_enabled(&w.pool, b, false).await.expect("off");
        w.invalidate().await;
        assert_eq!(
            w.sources().await.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![a],
            "disabled everywhere"
        );
        store::delete(&w.pool, a).await.expect("delete");
        w.invalidate().await;
        assert!(w.sources().await.is_empty());
    }

    #[tokio::test]
    async fn the_kill_switch_silences_every_wiki_at_once() {
        let (w, _) = service(fake()).await;
        add(&w, "a.fandom.com").await;
        assert_eq!(
            w.search("pyramid", Some(Kind::Character), None)
                .await
                .pages
                .len(),
            2,
            "the safe character pages are found"
        );
        store::set_global_enabled(&w.pool, false)
            .await
            .expect("off");
        assert!(w.sources().await.is_empty());
        assert!(w
            .search("pyramid", Some(Kind::Character), None)
            .await
            .pages
            .is_empty());
        assert!(
            w.suggest("pyramid", None, None, 5).await.is_empty(),
            "autocomplete too"
        );
        store::set_global_enabled(&w.pool, true).await.expect("on");
        assert!(!w.suggest("pyramid", None, None, 5).await.is_empty());
    }

    #[tokio::test]
    async fn a_search_drops_doubtful_pages_keeps_the_wikis_order_and_fills_the_cache_for_autocomplete(
    ) {
        let (w, _) = service(fake()).await;
        add(&w, "a.fandom.com").await;
        let found = w.search("pyramid head", Some(Kind::Character), None).await;
        let titles: Vec<&str> = found.pages.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Pyramid Head", "Pyramid Head (Homecoming)"],
            "the explicit page and the location are gone"
        );
        assert!(found.failed.is_empty());
        assert_eq!(
            found.pages[0].parent_title.as_deref(),
            Some("Silent Hill 2")
        );
        assert_eq!(
            found.pages[0].image_url.as_deref(),
            Some("https://static.wikia.nocookie.net/x/a.png")
        );
        // what the search saw is now suggested locally, with no wiki call
        let s = w.suggest("pyram", None, None, 5).await;
        assert_eq!(s.len(), 2);
        assert!(
            w.suggest("hentai", None, None, 5).await.is_empty(),
            "a dropped page is never stored"
        );
    }

    #[tokio::test]
    async fn one_failing_wiki_never_fails_the_search_and_is_named() {
        let pool = crate::db::connect_memory().await;
        struct Mixed(FakeWiki);
        impl Transport for Mixed {
            async fn get(
                &self,
                url: &str,
                p: Vec<(String, String)>,
                t: Duration,
            ) -> Result<serde_json::Value, WikiError> {
                if url.contains("bad.") && !p.iter().any(|(k, _)| k == "meta") {
                    return Err(WikiError::Timeout);
                }
                self.0.get(url, p, t).await
            }
        }
        let w = Wikis::new(pool, Arc::new(Mixed(fake())));
        for host in ["good.fandom.com", "bad.fandom.com"] {
            let p = w.preview(host).await.expect("preview");
            w.save(&p, Some(host), Some("Category:Characters"), None, 1)
                .await
                .expect("save");
        }
        let found = w.search("pyramid head", Some(Kind::Character), None).await;
        assert_eq!(
            found.failed,
            vec!["bad.fandom.com"],
            "the failing wiki is named"
        );
        assert!(
            found.pages.iter().all(|p| p.wiki_id == 1),
            "the good wiki's pages still come"
        );
        assert_eq!(found.pages.len(), 2);
        let bad = store::list(&w.pool)
            .await
            .expect("list")
            .into_iter()
            .find(|s| s.host == "bad.fandom.com")
            .expect("bad");
        assert_eq!(
            bad.last_error.as_deref(),
            Some("timeout"),
            "the error is recorded for the owner"
        );
    }

    #[tokio::test]
    async fn only_one_wiki_can_be_searched_and_a_second_identical_search_is_cached() {
        let (w, t) = service(fake()).await;
        let a = add(&w, "a.fandom.com").await;
        let b = add(&w, "b.fandom.com").await;
        let only_b = w
            .search("pyramid head", Some(Kind::Character), Some(b))
            .await;
        assert!(only_b.pages.iter().all(|p| p.wiki_id == b));
        let before = t.calls.lock().expect("lock").len();
        let again = w
            .search("Pyramid-Head", Some(Kind::Character), Some(b))
            .await;
        assert_eq!(again.pages.len(), only_b.pages.len());
        assert_eq!(
            t.calls.lock().expect("lock").len(),
            before,
            "the normalised search is served from the cache"
        );
        assert!(
            w.search("x", None, None).await.pages.is_empty(),
            "one character is not a search"
        );
        let _ = a;
    }

    #[tokio::test]
    async fn characters_of_a_game_fetch_the_category_once_then_come_from_the_cache() {
        let (w, t) = service(fake()).await;
        let id = add(&w, "a.fandom.com").await;
        let list = w.characters_of(id, "Silent Hill 2").await;
        assert_eq!(
            list.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(),
            vec!["Pyramid Head"]
        );
        let calls = t.calls.lock().expect("lock").len();
        let again = w.characters_of(id, "Silent Hill 2").await;
        assert_eq!(again.len(), 1);
        assert_eq!(
            t.calls.lock().expect("lock").len(),
            calls,
            "now from the cache"
        );
        assert!(w.characters_of(id, "Another Game").await.is_empty());
    }

    #[tokio::test]
    async fn a_page_is_read_from_the_cache_and_a_deleted_wiki_shows_nothing() {
        let (w, _) = service(fake()).await;
        let id = add(&w, "a.fandom.com").await;
        w.search("pyramid head", Some(Kind::Character), None).await;
        let (page, src) = w.page(id, 1).await.expect("cached page");
        assert_eq!((page.title.as_str(), src.id), ("Pyramid Head", id));
        store::delete(&w.pool, id).await.expect("delete");
        w.invalidate().await;
        assert!(w.page(id, 1).await.is_none());
        assert!(w.suggest("pyramid", None, None, 5).await.is_empty());
    }

    #[tokio::test]
    async fn the_stats_line_counts_wikis_pages_requests_and_errors() {
        let (w, _) = service(fake()).await;
        let id = add(&w, "a.fandom.com").await;
        w.search("pyramid head", None, None).await;
        store::mark_checked(&w.pool, id, 1, Some("boom"))
            .await
            .expect("mark");
        let s = w.stats().await;
        assert_eq!((s.wikis, s.enabled), (1, 1));
        assert!(s.pages >= 2 && s.requests_today >= 3 && s.global);
        assert_eq!(s.last_errors, vec!["Silent Hill Wiki: boom"]);
    }

    #[test]
    fn raw_pages_become_clean_classified_pages_with_aliases() {
        let w = Source {
            id: 4,
            name: "W".into(),
            host: "w.example".into(),
            api_url: "https://w.example/api.php".into(),
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
        };
        let raw = |id: i64, title: &str, cats: &[&str], redirects: &[&str]| parse::RawPage {
            page_id: id,
            title: title.into(),
            extract: Some("Text.".into()),
            image: None,
            categories: cats.iter().map(|c| c.to_string()).collect(),
            disambiguation: false,
            redirects_from: redirects.iter().map(|r| r.to_string()).collect(),
            series: None,
        };
        let pages = to_pages(
            &w,
            vec![
                raw(
                    1,
                    "Pyramid Head (Silent Hill 2)",
                    &["Characters"],
                    &["Pyramid-Head"],
                ),
                raw(2, "Silent Hill 2", &["Games"], &[]),
                raw(3, "NSFW Gallery", &["Characters"], &[]),
            ],
        );
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].kind, Kind::Character);
        assert_eq!(pages[0].aliases, vec!["Pyramid-Head", "Pyramid Head"]);
        assert_eq!(pages[0].parent_title.as_deref(), Some("Silent Hill 2"));
        assert_eq!(pages[1].kind, Kind::Game);
    }

    #[test]
    fn a_mudae_style_page_is_cleaned_and_its_series_becomes_the_parent_and_matches_other_sources() {
        let doc = json!({"query": {"pages": [{
            "pageid": 77, "ns": 0, "title": "Acerola",
            "extract": "Acerola is a rollable waifu from Pokémon Sun/Moon. She is the trial captain of Ula'ula Island.",
            "thumbnail": {"source": "https://static.wikia.nocookie.net/mudae/images/a/a1/Acerola.png/revision/latest"},
            "categories": [{"ns": 14, "title": "Category:Female characters"}, {"ns": 14, "title": "Category:Pokémon characters"}],
            "revisions": [{"slots": {"main": {"content": "{{Character\n|series = [[Pokémon Sun and Moon|Pokémon Sun/Moon]]\n|gender = Female\n|roulette = Waifu\n|official site = https://example.org\n}}"}}}]
        }]}});
        let w = Source {
            id: 2,
            name: "Mudae Wiki".into(),
            host: "mudae.fandom.com".into(),
            api_url: "https://mudae.fandom.com/api.php".into(),
            lang: "en".into(),
            license_name: Some("CC BY-SA".into()),
            license_url: None,
            character_category: Some("Category:Characters".into()),
            work_category: None,
            kind_hint: None,
            enabled: true,
            added_by: None,
            created_at: 0,
            last_checked_at: None,
            last_error: None,
        };
        let pages = to_pages(&w, parse::parse_details(&doc));
        assert_eq!(pages.len(), 1);
        let p = &pages[0];
        assert_eq!(
            p.parent_title.as_deref(),
            Some("Pokémon Sun/Moon"),
            "the series is the parent, as the wiki writes it"
        );
        assert_eq!(
            p.extract.as_deref(),
            Some("She is the trial captain of Ula'ula Island."),
            "no rollable-waifu text"
        );
        assert_eq!(
            p.kind,
            Kind::Character,
            "classified from the categories, not assumed to be a game"
        );
        assert!(p.image_url.is_some());
        // the same character from another source, with the series written differently, is the same one
        assert!(crate::wiki::same_identity(
            "Acerola",
            Some("Pokémon Sun and Moon"),
            &p.title,
            p.parent_title.as_deref()
        ));
        assert!(crate::wiki::same_identity(
            "Acerola",
            Some("Pokemon Sun & Moon"),
            &p.title,
            p.parent_title.as_deref()
        ));
        // another game's Acerola, or one with no known game, is not
        assert!(!crate::wiki::same_identity(
            "Acerola",
            Some("Pokémon Sword/Shield"),
            &p.title,
            p.parent_title.as_deref()
        ));
        assert!(!crate::wiki::same_identity(
            "Acerola",
            None,
            &p.title,
            p.parent_title.as_deref()
        ));
        // a page whose whole summary is jargon keeps no summary
        let all_jargon = json!({"query": {"pages": [{"pageid": 78, "ns": 0, "title": "Rem", "extract": "A waifu. Claim her.", "categories": []}]}});
        assert_eq!(
            to_pages(&w, parse::parse_details(&all_jargon))[0].extract,
            None
        );
    }
}
