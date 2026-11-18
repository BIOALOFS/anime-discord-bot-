//! The MediaWiki API client. `Transport` is the one place that touches the network (rate limit per wiki, a global cap
//! on parallel requests, `maxlag`, retries); `Api` builds the requests and parses the answers, so everything above
//! it is tested with recorded JSON and a fake transport.

use super::parse::{self, Member, RawPage, Site};
use governor::{clock::DefaultClock, state::InMemoryState, state::NotKeyed, Quota, RateLimiter};
use serde_json::Value;
use std::{
    collections::HashMap,
    future::Future,
    num::NonZeroU32,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Semaphore;

/// The interactive budget of one call to a wiki.
pub const INTERACTIVE_TIMEOUT: Duration = Duration::from_millis(2500);
/// Background work (category fetches, the warmer) may take longer.
pub const BACKGROUND_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WikiError {
    Http(String),
    Timeout,
    /// The answer was not MediaWiki JSON.
    NotMediaWiki,
}

impl std::fmt::Display for WikiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "{e}"),
            Self::Timeout => write!(f, "timeout"),
            Self::NotMediaWiki => write!(f, "not a MediaWiki API"),
        }
    }
}

pub trait Transport: Send + Sync {
    /// One GET of `api_url` with these parameters (the base parameters are added by the transport).
    fn get(
        &self,
        api_url: &str,
        params: Vec<(String, String)>,
        timeout: Duration,
    ) -> impl Future<Output = Result<Value, WikiError>> + Send;

    /// Is this a real, reasonably sized image? (`Content-Type: image/*` and at most 8 MB.) Used before a picture of
    /// a wiki fills a gap in a card. The thumbnails asked from the API are at most 600 px wide, so the pixel count is
    /// bounded by the request itself.
    fn probe_image(&self, _url: &str) -> impl Future<Output = bool> + Send {
        async { true }
    }
}

/// The largest picture a wiki may fill a card with.
pub const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

/// The decision of the image check from what the server said.
pub fn image_ok(content_type: Option<&str>, length: Option<u64>) -> bool {
    content_type.is_some_and(|c| {
        c.trim().to_lowercase().starts_with("image/") && !c.to_lowercase().contains("svg")
    }) && length.is_none_or(|l| l > 0 && l <= MAX_IMAGE_BYTES)
}

type Limiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

/// The real transport: one shared `reqwest` client with a descriptive User-Agent.
pub struct HttpTransport {
    http: reqwest::Client,
    per_second: u32,
    limiters: Mutex<HashMap<String, Arc<Limiter>>>,
    open: Semaphore,
}

/// Requests a wiki may get per second unless configured otherwise.
pub const DEFAULT_PER_SECOND: u32 = 2;
const MAX_PARALLEL: usize = 6;

impl HttpTransport {
    pub fn new(http: reqwest::Client, per_second: u32) -> Self {
        Self {
            http,
            per_second: per_second.max(1),
            limiters: Mutex::new(HashMap::new()),
            open: Semaphore::new(MAX_PARALLEL),
        }
    }

    fn limiter(&self, host: &str) -> Arc<Limiter> {
        let mut m = self.limiters.lock().unwrap_or_else(|e| e.into_inner());
        m.entry(host.to_string())
            .or_insert_with(|| {
                Arc::new(RateLimiter::direct(Quota::per_second(
                    NonZeroU32::new(self.per_second).unwrap_or(NonZeroU32::MIN),
                )))
            })
            .clone()
    }
}

/// How long to wait before a retry, from `Retry-After` (seconds), capped.
pub fn retry_wait(header: Option<&str>) -> Duration {
    Duration::from_secs(
        header
            .and_then(|h| h.trim().parse::<u64>().ok())
            .unwrap_or(1)
            .clamp(1, 5),
    )
}

/// Is this answer MediaWiki telling us to slow down (`maxlag`)?
pub fn is_maxlag(doc: &Value) -> bool {
    doc.pointer("/error/code").and_then(Value::as_str) == Some("maxlag")
}

impl Transport for HttpTransport {
    async fn probe_image(&self, url: &str) -> bool {
        if !super::safety::image_url_ok(url) {
            return false;
        }
        let Ok(resp) = self
            .http
            .head(url)
            .timeout(Duration::from_secs(3))
            .send()
            .await
        else {
            return false;
        };
        let header = |n: &str| {
            resp.headers()
                .get(n)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        resp.status().is_success()
            && image_ok(
                header("content-type").as_deref(),
                header("content-length").and_then(|l| l.parse().ok()),
            )
    }

    async fn get(
        &self,
        api_url: &str,
        params: Vec<(String, String)>,
        timeout: Duration,
    ) -> Result<Value, WikiError> {
        let host = reqwest::Url::parse(api_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .ok_or_else(|| WikiError::Http("bad address".into()))?;
        let limiter = self.limiter(&host);
        let _open = self
            .open
            .acquire()
            .await
            .map_err(|_| WikiError::Http("closed".into()))?;
        let mut query = vec![
            ("format".to_string(), "json".to_string()),
            ("formatversion".to_string(), "2".to_string()),
            ("maxlag".to_string(), "5".to_string()),
            ("redirects".to_string(), "1".to_string()),
        ];
        query.extend(params);
        for attempt in 0..3 {
            limiter.until_ready().await;
            crate::ops::stats::bump("api", "wiki");
            let resp = self
                .http
                .get(api_url)
                .header(
                    reqwest::header::USER_AGENT,
                    concat!(
                        "anpot-discord-bot/",
                        env!("CARGO_PKG_VERSION"),
                        " (+https://github.com/BIOALOFS/anpot; MediaWiki API client)"
                    ),
                )
                .query(&query)
                .timeout(timeout)
                .send()
                .await
                .map_err(|e| {
                    if e.is_timeout() {
                        WikiError::Timeout
                    } else {
                        WikiError::Http(e.without_url().to_string())
                    }
                })?;
            let status = resp.status();
            if (status.as_u16() == 429 || status.as_u16() == 503) && attempt < 2 {
                let wait = retry_wait(
                    resp.headers()
                        .get("retry-after")
                        .and_then(|h| h.to_str().ok()),
                );
                tokio::time::sleep(wait).await;
                continue;
            }
            if !status.is_success() {
                return Err(WikiError::Http(
                    crate::http_log::failed("wiki", &host, resp).await,
                ));
            }
            let doc: Value = resp.json().await.map_err(|_| WikiError::NotMediaWiki)?;
            if is_maxlag(&doc) && attempt < 2 {
                tokio::time::sleep(retry_wait(None)).await;
                continue;
            }
            return Ok(doc);
        }
        Err(WikiError::Http("the wiki is busy".into()))
    }
}

fn p(k: &str, v: impl Into<String>) -> (String, String) {
    (k.to_string(), v.into())
}

/// What the owner is shown before confirming a wiki.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub character: Option<(String, u64)>,
    pub work: Option<(String, u64)>,
    /// A few character names from the chosen category.
    pub samples: Vec<String>,
}

/// The requests of the client, over any transport.
pub struct Api<T: Transport> {
    pub t: Arc<T>,
}

impl<T: Transport> Clone for Api<T> {
    fn clone(&self) -> Self {
        Self { t: self.t.clone() }
    }
}

impl<T: Transport> Api<T> {
    pub fn new(t: Arc<T>) -> Self {
        Self { t }
    }

    /// `meta=siteinfo`: the wiki's name, language and license; refused when the answer is not a MediaWiki.
    pub async fn siteinfo(&self, api_url: &str) -> Result<Site, WikiError> {
        let doc = self
            .t
            .get(
                api_url,
                vec![
                    p("action", "query"),
                    p("meta", "siteinfo"),
                    p("siprop", "general|rightsinfo"),
                ],
                INTERACTIVE_TIMEOUT * 2,
            )
            .await?;
        parse::parse_siteinfo(&doc).ok_or(WikiError::NotMediaWiki)
    }

    /// The first of the addresses that answers like a MediaWiki: `(api url, site)`.
    pub async fn discover(&self, candidates: &[String]) -> Result<(String, Site), WikiError> {
        let mut last = WikiError::NotMediaWiki;
        for url in candidates {
            match self.siteinfo(url).await {
                Ok(site) => return Ok((url.clone(), site)),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    pub async fn search(
        &self,
        api_url: &str,
        query: &str,
        limit: u32,
        timeout: Duration,
    ) -> Result<Vec<Member>, WikiError> {
        let doc = self
            .t
            .get(
                api_url,
                vec![
                    p("action", "query"),
                    p("list", "search"),
                    p("srsearch", query),
                    p("srlimit", limit.to_string()),
                    p("srnamespace", "0"),
                ],
                timeout,
            )
            .await?;
        Ok(parse::parse_search(&doc))
    }

    /// Details of pages, 50 titles a request.
    pub async fn details(
        &self,
        api_url: &str,
        titles: &[String],
        timeout: Duration,
    ) -> Result<Vec<RawPage>, WikiError> {
        let mut out = Vec::new();
        for batch in parse::batches(titles) {
            let doc = self
                .t
                .get(
                    api_url,
                    vec![
                        p("action", "query"),
                        p("prop", "extracts|pageimages|categories|pageprops|revisions"),
                        p("rvprop", "content"),
                        p("rvslots", "main"),
                        p("exintro", "1"),
                        p("explaintext", "1"),
                        p("exsentences", "4"),
                        p("exlimit", "max"),
                        p("piprop", "original|thumbnail"),
                        p("pithumbsize", "600"),
                        p("cllimit", "max"),
                        p("clshow", "!hidden"),
                        p("ppprop", "disambiguation"),
                        p("titles", batch),
                    ],
                    timeout,
                )
                .await?;
            out.extend(parse::parse_details(&doc));
        }
        Ok(out)
    }

    /// The pages of a category, following the continuation until `max` titles are collected.
    pub async fn category_members(
        &self,
        api_url: &str,
        category: &str,
        max: usize,
        timeout: Duration,
    ) -> Result<Vec<Member>, WikiError> {
        let mut out: Vec<Member> = Vec::new();
        let mut cont: Option<String> = None;
        loop {
            let mut params = vec![
                p("action", "query"),
                p("list", "categorymembers"),
                p("cmtitle", category),
                p("cmlimit", "500"),
                p("cmtype", "page"),
            ];
            if let Some(c) = &cont {
                params.push(p("cmcontinue", c.clone()));
            }
            let doc = self.t.get(api_url, params, timeout).await?;
            let (members, next) = parse::parse_members(&doc);
            out.extend(members);
            match next {
                Some(n) if out.len() < max => cont = Some(n),
                _ => break,
            }
        }
        out.truncate(max);
        Ok(out)
    }

    async fn category_counts(
        &self,
        api_url: &str,
        titles: &[String],
    ) -> Result<Vec<(String, u64)>, WikiError> {
        let mut out = Vec::new();
        for batch in parse::batches(titles) {
            let doc = self
                .t
                .get(
                    api_url,
                    vec![
                        p("action", "query"),
                        p("prop", "categoryinfo"),
                        p("titles", batch),
                    ],
                    INTERACTIVE_TIMEOUT * 2,
                )
                .await?;
            out.extend(parse::parse_categoryinfo(&doc));
        }
        Ok(out)
    }

    async fn categories_with_prefix(&self, api_url: &str, prefix: &str) -> Vec<(String, u64)> {
        match self
            .t
            .get(
                api_url,
                vec![
                    p("action", "query"),
                    p("list", "allcategories"),
                    p("acprefix", prefix),
                    p("aclimit", "50"),
                    p("acprop", "size"),
                ],
                INTERACTIVE_TIMEOUT * 2,
            )
            .await
        {
            Ok(doc) => parse::parse_allcategories(&doc),
            Err(_) => Vec::new(),
        }
    }

    /// Find the character category and the game category (the ones with the most pages) and a few sample names.
    pub async fn detect(&self, api_url: &str) -> Detection {
        let mut chars: Vec<String> = parse::CHARACTER_CATEGORIES
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut games: Vec<String> = parse::WORK_CATEGORIES
            .iter()
            .map(|s| s.to_string())
            .collect();
        let scan = |prefixes: &'static [&'static str]| async move {
            futures::future::join_all(
                prefixes
                    .iter()
                    .map(|p| self.categories_with_prefix(api_url, p)),
            )
            .await
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
        };
        let (extra_c, extra_g) =
            futures::join!(scan(parse::CHARACTER_PREFIXES), scan(parse::WORK_PREFIXES));
        // Categories found by prefix are candidates only when they look like lists of characters or games, not of
        // images, templates or hubs ("Character images", "Characters by game").
        let known_c: Vec<(String, u64)> = extra_c
            .into_iter()
            .filter(|(t, _)| parse::plausible_category(t))
            .collect();
        let known_g: Vec<(String, u64)> = extra_g
            .into_iter()
            .filter(|(t, _)| parse::plausible_category(t))
            .collect();
        chars.extend(known_c.iter().map(|(t, _)| t.clone()));
        games.extend(known_g.iter().map(|(t, _)| t.clone()));
        let (cc, gc) = futures::join!(
            async {
                self.category_counts(api_url, &chars)
                    .await
                    .unwrap_or_default()
            },
            async {
                self.category_counts(api_url, &games)
                    .await
                    .unwrap_or_default()
            }
        );
        let character = parse::pick_category(&cc);
        let work = parse::pick_category(&gc);
        let samples = match &character {
            Some((cat, _)) => self
                .category_members(api_url, cat, 5, INTERACTIVE_TIMEOUT * 2)
                .await
                .map(|m| m.into_iter().map(|m| m.title).collect())
                .unwrap_or_default(),
            None => Vec::new(),
        };
        Detection {
            character,
            work,
            samples,
        }
    }

    /// The pages of a category as a count (for the preview after the owner edits the category).
    pub async fn count_category(&self, api_url: &str, category: &str) -> Option<u64> {
        let t = parse::category_title(category);
        self.category_counts(api_url, std::slice::from_ref(&t))
            .await
            .ok()?
            .into_iter()
            .find(|(c, _)| *c == t)
            .map(|(_, n)| n)
    }
}

#[cfg(test)]
pub mod fake {
    //! A fake wiki for the tests: answers by `action` / `list` / `prop` with recorded-shape JSON and counts calls.

    use super::*;
    use serde_json::json;

    #[derive(Default)]
    pub struct FakeWiki {
        pub calls: Mutex<Vec<Vec<(String, String)>>>,
        /// `(category title, pages)` for `prop=categoryinfo`.
        pub categories: Vec<(String, u64)>,
        /// Category members by title, served in batches of `batch` with continuation.
        pub members: HashMap<String, Vec<(i64, String)>>,
        pub batch: usize,
        /// Pages served by `prop=extracts…`: `(id, title, categories, extra json)`.
        pub pages: Vec<(i64, String, Vec<String>, Value)>,
        pub search_titles: Vec<(i64, String)>,
        pub not_mediawiki: bool,
        pub fail: bool,
    }

    fn arg<'a>(params: &'a [(String, String)], k: &str) -> Option<&'a str> {
        params.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    impl Transport for FakeWiki {
        async fn get(
            &self,
            _api_url: &str,
            params: Vec<(String, String)>,
            _timeout: Duration,
        ) -> Result<Value, WikiError> {
            self.calls.lock().expect("lock").push(params.clone());
            if self.fail {
                return Err(WikiError::Timeout);
            }
            if self.not_mediawiki {
                return Ok(json!({"hello": "world"}));
            }
            if arg(&params, "meta") == Some("siteinfo") {
                return Ok(
                    json!({"query": {"general": {"sitename": "Silent Hill Wiki", "lang": "en", "generator": "MediaWiki 1.43"},
                    "rightsinfo": {"text": "CC BY-SA 3.0", "url": "https://creativecommons.org/licenses/by-sa/3.0/"}}}),
                );
            }
            if arg(&params, "list") == Some("allcategories") {
                let prefix = arg(&params, "acprefix").unwrap_or("");
                let list: Vec<Value> = self
                    .categories
                    .iter()
                    .filter_map(|(t, n)| t.strip_prefix("Category:").map(|c| (c, n)))
                    .filter(|(c, _)| c.starts_with(prefix))
                    .map(|(c, n)| json!({"category": c, "pages": n}))
                    .collect();
                return Ok(json!({"query": {"allcategories": list}}));
            }
            if arg(&params, "prop") == Some("categoryinfo") {
                let titles: Vec<&str> = arg(&params, "titles").unwrap_or("").split('|').collect();
                let pages: Vec<Value> = titles
                    .iter()
                    .map(|t| match self.categories.iter().find(|(c, _)| c == t) {
                        Some((c, n)) => {
                            json!({"title": c, "categoryinfo": {"pages": n, "size": n}})
                        }
                        None => json!({"title": t, "missing": true}),
                    })
                    .collect();
                return Ok(json!({"query": {"pages": pages}}));
            }
            if arg(&params, "list") == Some("categorymembers") {
                let all = self
                    .members
                    .get(arg(&params, "cmtitle").unwrap_or(""))
                    .cloned()
                    .unwrap_or_default();
                let size = if self.batch == 0 { 500 } else { self.batch };
                let start = arg(&params, "cmcontinue")
                    .and_then(|c| c.parse::<usize>().ok())
                    .unwrap_or(0);
                let slice: Vec<Value> = all
                    .iter()
                    .skip(start)
                    .take(size)
                    .map(|(id, t)| json!({"pageid": id, "ns": 0, "title": t}))
                    .collect();
                let mut out = json!({"query": {"categorymembers": slice}});
                if start + size < all.len() {
                    out["continue"] =
                        json!({"cmcontinue": (start + size).to_string(), "continue": "-||"});
                }
                return Ok(out);
            }
            if arg(&params, "list") == Some("search") {
                let list: Vec<Value> = self
                    .search_titles
                    .iter()
                    .map(|(id, t)| json!({"pageid": id, "ns": 0, "title": t}))
                    .collect();
                return Ok(json!({"query": {"search": list}}));
            }
            if arg(&params, "prop").is_some_and(|p| p.contains("extracts")) {
                let wanted: Vec<&str> = arg(&params, "titles").unwrap_or("").split('|').collect();
                let pages: Vec<Value> = self
                    .pages
                    .iter()
                    .filter(|(_, t, ..)| wanted.contains(&t.as_str()))
                    .map(|(id, t, cats, extra)| {
                        let mut v = json!({"pageid": id, "ns": 0, "title": t, "extract": format!("About {t}."),
                            "categories": cats.iter().map(|c| json!({"ns": 14, "title": format!("Category:{c}")})).collect::<Vec<_>>()});
                        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
                            o.extend(e.clone());
                        }
                        v
                    })
                    .collect();
                return Ok(json!({"query": {"pages": pages}}));
            }
            Ok(json!({}))
        }
    }

    pub fn api(f: FakeWiki) -> (Api<FakeWiki>, Arc<FakeWiki>) {
        let t = Arc::new(f);
        (Api::new(t.clone()), t)
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;
    use serde_json::json;

    fn wiki() -> FakeWiki {
        let characters: Vec<(i64, String)> =
            (1..=7).map(|i| (i, format!("Character {i}"))).collect();
        let mut members = HashMap::new();
        members.insert("Category:Characters".to_string(), characters);
        FakeWiki {
            categories: vec![
                ("Category:Characters".into(), 7),
                ("Category:Heroes".into(), 2),
                ("Category:Games".into(), 5),
                ("Category:Character images".into(), 99),
            ],
            members,
            batch: 3,
            ..FakeWiki::default()
        }
    }

    #[tokio::test]
    async fn siteinfo_is_validated_and_a_non_mediawiki_answer_is_refused() {
        let (api, _) = api(wiki());
        let site = api
            .siteinfo("https://x.fandom.com/api.php")
            .await
            .expect("site");
        assert_eq!(
            (site.name.as_str(), site.license_name.as_deref()),
            ("Silent Hill Wiki", Some("CC BY-SA 3.0"))
        );
        let (bad, _) = super::fake::api(FakeWiki {
            not_mediawiki: true,
            ..FakeWiki::default()
        });
        assert_eq!(
            bad.siteinfo("https://x.org/api.php").await,
            Err(WikiError::NotMediaWiki)
        );
        let (down, _) = super::fake::api(FakeWiki {
            fail: true,
            ..FakeWiki::default()
        });
        assert!(down
            .discover(&[
                "https://a.org/api.php".to_string(),
                "https://a.org/w/api.php".to_string()
            ])
            .await
            .is_err());
    }

    #[tokio::test]
    async fn discovery_tries_the_next_address_when_the_first_is_not_a_mediawiki() {
        struct Second;
        impl Transport for Second {
            async fn get(
                &self,
                url: &str,
                _p: Vec<(String, String)>,
                _t: Duration,
            ) -> Result<Value, WikiError> {
                if url.contains("/w/api.php") {
                    Ok(
                        json!({"query": {"general": {"sitename": "W", "generator": "MediaWiki 1.43"}}}),
                    )
                } else {
                    Ok(json!({"nope": 1}))
                }
            }
        }
        let api = Api::new(Arc::new(Second));
        let (url, site) = api
            .discover(&[
                "https://x.wiki.gg/api.php".to_string(),
                "https://x.wiki.gg/w/api.php".to_string(),
            ])
            .await
            .expect("the second address works");
        assert_eq!(
            (url.as_str(), site.name.as_str()),
            ("https://x.wiki.gg/w/api.php", "W")
        );
    }

    #[tokio::test]
    async fn the_character_category_with_the_most_pages_is_detected_with_samples() {
        let (api, _) = api(wiki());
        let d = api.detect("https://x.fandom.com/api.php").await;
        assert_eq!(
            d.character,
            Some(("Category:Characters".to_string(), 7)),
            "7 pages beat the 2 of Heroes; 'Character images' wins by count too"
        );
        assert_eq!(d.work, Some(("Category:Games".to_string(), 5)));
        assert_eq!(d.samples.len(), 5);
        // a wiki with no such categories finds nothing and still allows manual setup
        let (none, _) = super::fake::api(FakeWiki::default());
        let d = none.detect("https://x.fandom.com/api.php").await;
        assert_eq!((d.character, d.work, d.samples.len()), (None, None, 0));
    }

    #[tokio::test]
    async fn category_members_follow_the_continuation_up_to_the_limit() {
        let (api, t) = api(wiki());
        let all = api
            .category_members("u", "Category:Characters", 100, INTERACTIVE_TIMEOUT)
            .await
            .expect("members");
        assert_eq!(all.len(), 7);
        assert_eq!(
            t.calls.lock().expect("lock").len(),
            3,
            "three batches of 3, 3 and 1"
        );
        let some = api
            .category_members("u", "Category:Characters", 4, INTERACTIVE_TIMEOUT)
            .await
            .expect("members");
        assert_eq!(some.len(), 4, "cut at the limit");
    }

    #[tokio::test]
    async fn details_are_requested_fifty_titles_at_a_time_and_redirects_disambiguation_are_dropped()
    {
        let pages: Vec<(i64, String, Vec<String>, Value)> = (1..=120)
            .map(|i| {
                (
                    i,
                    format!("Page {i}"),
                    vec!["Characters".to_string()],
                    json!({}),
                )
            })
            .chain([(
                500,
                "Heather".to_string(),
                vec![],
                json!({"pageprops": {"disambiguation": ""}}),
            )])
            .collect();
        let (api, t) = api(FakeWiki {
            pages,
            ..FakeWiki::default()
        });
        let mut titles: Vec<String> = (1..=120).map(|i| format!("Page {i}")).collect();
        titles.push("Heather".into());
        let out = api
            .details("u", &titles, INTERACTIVE_TIMEOUT)
            .await
            .expect("details");
        assert_eq!(out.len(), 120, "the disambiguation page is skipped");
        let calls = t.calls.lock().expect("lock").clone();
        assert_eq!(calls.len(), 3, "121 titles in three requests");
        let sizes: Vec<usize> = calls
            .iter()
            .map(|c| {
                c.iter()
                    .find(|(k, _)| k == "titles")
                    .map_or(0, |(_, v)| v.split('|').count())
            })
            .collect();
        assert_eq!(sizes, vec![50, 50, 21]);
        let first = &calls[0];
        for (k, v) in [
            ("exintro", "1"),
            ("explaintext", "1"),
            ("exsentences", "4"),
            ("pithumbsize", "600"),
        ] {
            assert!(first.iter().any(|(a, b)| a == k && b == v), "{k}={v}");
        }
    }

    #[tokio::test]
    async fn a_search_and_a_manual_category_count() {
        let (api, _) = api(FakeWiki {
            search_titles: vec![(1, "Pyramid Head".into()), (2, "Talk:Pyramid Head".into())],
            ..wiki()
        });
        let hits = api
            .search("u", "pyramid", 10, INTERACTIVE_TIMEOUT)
            .await
            .expect("search");
        assert_eq!(hits.len(), 1, "the talk page is filtered out");
        assert_eq!(api.count_category("u", "Characters").await, Some(7));
        assert_eq!(api.count_category("u", "Category:Nothing").await, None);
    }

    #[test]
    fn the_image_check_wants_a_real_image_of_a_sane_size() {
        assert!(image_ok(Some("image/png"), Some(1_000)));
        assert!(image_ok(Some("image/webp; charset=binary"), None));
        assert!(!image_ok(Some("text/html"), Some(10)));
        assert!(!image_ok(Some("image/svg+xml"), Some(10)), "no SVG");
        assert!(!image_ok(None, Some(10)));
        assert!(
            !image_ok(Some("image/png"), Some(MAX_IMAGE_BYTES + 1)),
            "over 8 MB"
        );
        assert!(!image_ok(Some("image/png"), Some(0)));
    }

    #[test]
    fn retry_and_lag_helpers() {
        assert_eq!(retry_wait(Some("3")), Duration::from_secs(3));
        assert_eq!(retry_wait(Some("999")), Duration::from_secs(5));
        assert_eq!(retry_wait(Some("soon")), Duration::from_secs(1));
        assert_eq!(retry_wait(None), Duration::from_secs(1));
        assert!(is_maxlag(
            &json!({"error": {"code": "maxlag", "info": "Waiting for a database server"}})
        ));
        assert!(!is_maxlag(&json!({"query": {}})));
    }
}
