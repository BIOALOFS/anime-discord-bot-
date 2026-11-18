//! Filling the gaps of a card from the wikis. A card from IGDB, VNDB, AniList or TMDB that lacks a picture, a summary
//! or its game gets them from a MATCHING wiki page, only into empty fields, never over what the main source gave, and
//! only for a strong match (same name AND same game for a character). Results are cached for a week and the card never
//! waits longer than its budget for them.

use super::client::Transport;
use super::{normalize, service::Wikis, Kind, Page, Source};
use crate::ui::embeds::Emb;
use std::{sync::Arc, time::Duration};

/// What a card already has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    pub image: Option<String>,
    pub summary: Option<String>,
    pub parent: Option<String>,
}

/// What a matching wiki page offers, with where it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fill {
    pub image: Option<String>,
    pub summary: Option<String>,
    pub parent: Option<String>,
    pub aliases: Vec<String>,
    pub wiki: String,
    pub license: Option<String>,
    pub page_url: String,
}

/// Put the fill into the empty fields of `f`; a field that has a value is never touched. Returns whether anything
/// changed.
pub fn apply(f: &mut Fields, fill: &Fill) -> bool {
    let mut changed = false;
    for (field, from) in [
        (&mut f.image, &fill.image),
        (&mut f.summary, &fill.summary),
        (&mut f.parent, &fill.parent),
    ] {
        if field.as_deref().is_none_or(|v| v.trim().is_empty()) {
            if let Some(v) = from.as_ref().filter(|v| !v.trim().is_empty()) {
                *field = Some(v.clone());
                changed = true;
            }
        }
    }
    changed
}

/// The page that is the SAME thing as a card with this title and game, or none. A character matches only when the
/// name and the game both agree; a game when the name agrees (the wiki's qualifiers like "(game)" are ignored).
/// A weak or name-only similarity is no match.
pub fn pick_match<'a>(
    title: &str,
    parent: Option<&str>,
    kind: Kind,
    pages: &'a [Page],
) -> Option<&'a Page> {
    let strong: Vec<&Page> = pages
        .iter()
        // `Other` is "any work": a page that is not a character (an anime, a series, a film…).
        .filter(|p| match kind {
            Kind::Character => p.kind == Kind::Character,
            Kind::Other => p.kind != Kind::Character,
            _ => p.kind.is_work(),
        })
        .filter(|p| match kind {
            Kind::Character => {
                super::same_identity(title, parent, &p.title, p.parent_title.as_deref())
            }
            _ => {
                let t = normalize(super::base_title(title));
                !t.is_empty() && t == normalize(super::base_title(&p.title))
            }
        })
        .collect();
    // The page that gives the most (a picture and a summary) wins; ties keep the wiki order.
    strong
        .into_iter()
        .max_by_key(|p| usize::from(p.image_url.is_some()) * 2 + usize::from(p.extract.is_some()))
}

fn fill_from(p: &Page, w: &Source) -> Fill {
    Fill {
        image: p.image_url.clone(),
        summary: p.extract.clone(),
        parent: p.parent_title.clone(),
        aliases: p.aliases.clone(),
        wiki: w.name.clone(),
        license: w.license_name.clone(),
        page_url: super::msgs::page_url(&w.host, &p.title),
    }
}

impl<T: Transport + 'static> Wikis<T> {
    /// A matching wiki page for a card, looked up in the local cache first and then in the wikis; `None` when nothing
    /// matches strongly. The picture, if any, passes the real-image check.
    pub async fn fill_for(&self, title: &str, parent: Option<&str>, kind: Kind) -> Option<Fill> {
        let key = format!(
            "{}|{}|{}",
            normalize(title),
            parent.map(normalize).unwrap_or_default(),
            kind.key()
        );
        self.fills
            .try_get_with(key, async {
                let now = chrono::Utc::now().timestamp();
                let only = (kind != Kind::Other).then_some(kind);
                let mut pages = super::store::by_title(
                    &self.pool,
                    &normalize(super::base_title(title)),
                    only,
                    now,
                )
                .await
                .unwrap_or_default();
                if pick_match(title, parent, kind, &pages).is_none() {
                    pages.extend(
                        self.search(super::base_title(title), only, None)
                            .await
                            .pages,
                    );
                }
                let sources = self.sources().await;
                let found = pick_match(title, parent, kind, &pages).and_then(|p| {
                    let w = sources.iter().find(|w| w.id == p.wiki_id)?;
                    Some(fill_from(p, w))
                });
                let found = match found {
                    Some(mut f) => {
                        if let Some(url) = f.image.clone() {
                            if !self.api.t.probe_image(&url).await {
                                f.image = None;
                            }
                        }
                        (f.image.is_some() || f.summary.is_some() || f.parent.is_some())
                            .then_some(f)
                    }
                    None => None,
                };
                Ok::<_, std::convert::Infallible>(Arc::new(found))
            })
            .await
            .ok()
            .and_then(|a| (*a).clone())
    }

    /// `fill_for` within a budget: the lookup keeps running in the background after the budget, so the next view of
    /// the card finds it cached; the card that asked renders without waiting longer.
    pub async fn fill_within(
        self: &Arc<Self>,
        title: &str,
        parent: Option<&str>,
        kind: Kind,
        budget: Duration,
    ) -> Option<Fill> {
        let me = self.clone();
        let (t, p) = (title.to_string(), parent.map(str::to_string));
        let mut task = tokio::spawn(async move { me.fill_for(&t, p.as_deref(), kind).await });
        match tokio::time::timeout(budget, &mut task).await {
            Ok(Ok(r)) => r,
            _ => None,
        }
    }
}

/// A fill that is looked up while a live card is already on screen: the card renders at once, and when the lookup
/// finishes the view asks for an update, so the message is edited in place. It never extends the message's lifetime
/// (only presses do) and gives up after a few seconds.
pub struct LiveFill {
    task: Option<tokio::task::JoinHandle<Option<Fill>>>,
    started: Option<std::time::Instant>,
    key: Option<String>,
    pub fill: Option<Fill>,
}

/// How long a live card waits for its fill before giving up.
const LIVE_LIMIT: Duration = Duration::from_secs(4);
/// How often a live card looks whether its fill has arrived.
pub const LIVE_POLL: Duration = Duration::from_millis(400);

impl Default for LiveFill {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveFill {
    pub fn new() -> Self {
        Self {
            task: None,
            started: None,
            key: None,
            fill: None,
        }
    }

    /// Start looking up a fill for the item `key` (once per item; a new item resets the previous fill).
    pub fn start<T: Transport + 'static>(
        &mut self,
        wikis: &Arc<Wikis<T>>,
        key: &str,
        title: &str,
        parent: Option<&str>,
        kind: Kind,
    ) {
        if self.key.as_deref() == Some(key) {
            return;
        }
        if let Some(t) = self.task.take() {
            t.abort();
        }
        self.fill = None;
        self.key = Some(key.to_string());
        self.started = Some(std::time::Instant::now());
        let (me, t, p) = (wikis.clone(), title.to_string(), parent.map(str::to_string));
        self.task = Some(tokio::spawn(async move {
            me.fill_for(&t, p.as_deref(), kind).await
        }));
    }

    pub fn pending(&self) -> bool {
        self.task.is_some()
    }

    /// The fill, only if it is for this item.
    pub fn for_key(&self, key: &str) -> Option<&Fill> {
        self.fill
            .as_ref()
            .filter(|_| self.key.as_deref() == Some(key))
    }

    /// Has the fill arrived (or the time run out)? `true` when the card should be drawn again.
    pub async fn poll(&mut self) -> bool {
        let Some(task) = self.task.as_mut() else {
            return false;
        };
        let timed_out = self.started.is_some_and(|s| s.elapsed() > LIVE_LIMIT);
        if !task.is_finished() && !timed_out {
            return false;
        }
        let Some(task) = self.task.take() else {
            return false;
        };
        if !task.is_finished() {
            task.abort();
            return false;
        }
        self.fill = task.await.ok().flatten();
        self.fill.is_some()
    }
}

/// Add a fill to a card's embed: only into what is empty, and always with the wiki named in the footer (and the page
/// linked) when anything of it was used.
pub fn enrich(e: &mut Emb, has_image: bool, fill: &Fill) {
    let mut fields = Fields {
        image: (has_image || e.image.is_some()).then(|| "present".to_string()),
        summary: e.description.clone().filter(|d| !d.trim().is_empty()),
        parent: None,
    };
    let before = fields.clone();
    apply(&mut fields, fill);
    let mut used = false;
    if before.image.is_none() {
        if let Some(img) = fields.image.clone().filter(|i| i != "present") {
            e.image = Some(img);
            used = true;
        }
    }
    if before.summary.is_none() {
        if let Some(s) = fields.summary.clone() {
            e.description = Some(format!(
                "{}\n\n📄 {}",
                crate::ui::embeds::fit(&s, 600),
                fill.page_url
            ));
            used = true;
        }
    }
    if used {
        let credit = match &fill.license {
            Some(l) => format!("{} · {l}", fill.wiki),
            None => fill.wiki.clone(),
        };
        e.footer = Some(match e.footer.take() {
            Some(old) if !old.is_empty() => format!("{old} · {credit}"),
            _ => credit,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki::client::fake::FakeWiki;
    use crate::wiki::store;

    fn page(id: i64, title: &str, kind: Kind, parent: Option<&str>, image: bool) -> Page {
        Page {
            wiki_id: 1,
            page_id: id,
            title: title.into(),
            kind,
            extract: Some(format!("About {title}.")),
            image_url: image.then(|| "https://static.wikia.nocookie.net/x/a.png".to_string()),
            aliases: vec![],
            parent_title: parent.map(str::to_string),
            categories: vec![],
        }
    }

    fn fill() -> Fill {
        Fill {
            image: Some("https://static.wikia.nocookie.net/x/a.png".into()),
            summary: Some("From the wiki.".into()),
            parent: Some("Silent Hill 2".into()),
            aliases: vec![],
            wiki: "Silent Hill Wiki".into(),
            license: Some("CC BY-SA 3.0".into()),
            page_url: "https://silenthill.fandom.com/wiki/Pyramid_Head".into(),
        }
    }

    #[test]
    fn only_empty_fields_are_filled_and_nothing_is_overwritten() {
        let mut f = Fields {
            image: None,
            summary: Some("The provider's text.".into()),
            parent: Some("   ".into()),
        };
        assert!(apply(&mut f, &fill()));
        assert_eq!(
            f.image.as_deref(),
            Some("https://static.wikia.nocookie.net/x/a.png")
        );
        assert_eq!(
            f.summary.as_deref(),
            Some("The provider's text."),
            "an existing summary stays"
        );
        assert_eq!(
            f.parent.as_deref(),
            Some("Silent Hill 2"),
            "a blank field counts as empty"
        );
        let mut full = Fields {
            image: Some("a".into()),
            summary: Some("b".into()),
            parent: Some("c".into()),
        };
        assert!(!apply(&mut full, &fill()));
        assert_eq!(
            full,
            Fields {
                image: Some("a".into()),
                summary: Some("b".into()),
                parent: Some("c".into())
            }
        );
        // nothing to give: nothing changes
        let empty_fill = Fill {
            image: None,
            summary: None,
            parent: None,
            ..fill()
        };
        assert!(!apply(&mut Fields::default(), &empty_fill));
    }

    #[test]
    fn a_character_matches_only_with_the_same_name_and_the_same_game() {
        let pages = vec![
            page(
                1,
                "Pyramid Head",
                Kind::Character,
                Some("Silent Hill: Homecoming"),
                true,
            ),
            page(
                2,
                "Pyramid Head",
                Kind::Character,
                Some("Silent Hill 2"),
                true,
            ),
            page(3, "Pyramid Head", Kind::Character, None, true),
        ];
        let m = pick_match(
            "Pyramid-Head",
            Some("Silent Hill 2"),
            Kind::Character,
            &pages,
        )
        .expect("match");
        assert_eq!(
            m.page_id, 2,
            "the same game, not the other game and not the page without a game"
        );
        assert!(
            pick_match("Pyramid Head", None, Kind::Character, &pages).is_none(),
            "a name alone is not enough"
        );
        assert!(pick_match(
            "Pyramid Head",
            Some("Another Game"),
            Kind::Character,
            &pages
        )
        .is_none());
        assert!(pick_match("Heather", Some("Silent Hill 2"), Kind::Character, &pages).is_none());
    }

    #[test]
    fn a_game_matches_by_name_ignoring_the_wikis_qualifier_and_prefers_the_richer_page() {
        let pages = vec![
            page(1, "Silent Hill 2 (game)", Kind::Game, None, false),
            page(2, "Silent Hill 2", Kind::Game, None, true),
            page(3, "Silent Hill 3", Kind::Game, None, true),
            page(4, "Silent Hill 2", Kind::Character, None, true),
        ];
        assert_eq!(
            pick_match("Silent Hill 2", None, Kind::Game, &pages).map(|p| p.page_id),
            Some(2)
        );
        assert!(
            pick_match("Silent Hill", None, Kind::Game, &pages).is_none(),
            "a prefix is not a match"
        );
    }

    #[test]
    fn any_work_matches_a_page_that_is_not_a_character_by_its_name() {
        let pages = vec![
            page(1, "Frieren", Kind::Character, Some("Frieren"), true),
            page(2, "Frieren", Kind::Other, None, true),
            page(3, "Frieren: Beyond Journey's End", Kind::Game, None, false),
        ];
        assert_eq!(
            pick_match("Frieren", None, Kind::Other, &pages).map(|p| p.page_id),
            Some(2),
            "never the character page"
        );
        assert!(
            pick_match("Sousou no Frieren", None, Kind::Other, &pages).is_none(),
            "a different name is no match"
        );
    }

    #[test]
    fn the_embed_gets_the_picture_and_summary_it_lacks_and_the_wiki_is_named() {
        let mut e = Emb::new().title("Pyramid Head");
        e.footer = Some("Data from IGDB.com".into());
        enrich(&mut e, false, &fill());
        assert_eq!(
            e.image.as_deref(),
            Some("https://static.wikia.nocookie.net/x/a.png")
        );
        assert!(
            e.description
                .as_deref()
                .unwrap_or("")
                .contains("From the wiki.")
                && e.description
                    .as_deref()
                    .unwrap_or("")
                    .contains("fandom.com/wiki/Pyramid_Head")
        );
        assert_eq!(
            e.footer.as_deref(),
            Some("Data from IGDB.com · Silent Hill Wiki · CC BY-SA 3.0")
        );
        // a card that has its own picture and text is left exactly as it is, and the wiki is not credited
        let mut own = Emb::new().title("X").description("Own text.");
        own.footer = Some("Data from IGDB.com".into());
        enrich(&mut own, true, &fill());
        assert_eq!(own.description.as_deref(), Some("Own text."));
        assert!(
            own.image.is_none(),
            "the attachment of the card stays the picture"
        );
        assert_eq!(own.footer.as_deref(), Some("Data from IGDB.com"));
    }

    async fn service(f: FakeWiki) -> Arc<Wikis<FakeWiki>> {
        let pool = crate::db::connect_memory().await;
        Arc::new(Wikis::new(pool, Arc::new(f)))
    }

    #[tokio::test]
    async fn a_card_is_filled_from_the_cached_wiki_and_the_result_is_cached() {
        let w = service(FakeWiki::default()).await;
        store::insert(
            &w.pool,
            &store::NewSource {
                name: "SH".into(),
                host: "sh.example".into(),
                api_url: "https://sh.example/api.php".into(),
                lang: "en".into(),
                license_name: Some("CC BY-SA".into()),
                license_url: None,
                character_category: None,
                work_category: None,
                kind_hint: None,
                added_by: None,
            },
        )
        .await
        .expect("source");
        w.invalidate().await;
        store::upsert(
            &w.pool,
            &page(
                5,
                "Pyramid Head",
                Kind::Character,
                Some("Silent Hill 2"),
                true,
            ),
            chrono::Utc::now().timestamp(),
        )
        .await
        .expect("page");
        let got = w
            .fill_for("Pyramid Head", Some("Silent Hill 2"), Kind::Character)
            .await
            .expect("a fill");
        assert_eq!(got.wiki, "SH");
        assert_eq!(got.summary.as_deref(), Some("About Pyramid Head."));
        assert!(got.image.is_some() && got.page_url.ends_with("/wiki/Pyramid_Head"));
        // no match for another game: nothing is filled
        assert!(w
            .fill_for(
                "Pyramid Head",
                Some("Silent Hill: Homecoming"),
                Kind::Character
            )
            .await
            .is_none());
    }

    #[tokio::test]
    async fn a_picture_that_fails_the_image_check_is_not_used_but_the_text_still_is() {
        struct Blind(FakeWiki);
        impl Transport for Blind {
            async fn get(
                &self,
                u: &str,
                p: Vec<(String, String)>,
                t: Duration,
            ) -> Result<serde_json::Value, crate::wiki::client::WikiError> {
                self.0.get(u, p, t).await
            }
            async fn probe_image(&self, _u: &str) -> bool {
                false
            }
        }
        let pool = crate::db::connect_memory().await;
        let w = Arc::new(Wikis::new(pool, Arc::new(Blind(FakeWiki::default()))));
        store::insert(
            &w.pool,
            &store::NewSource {
                name: "SH".into(),
                host: "sh.example".into(),
                api_url: "https://sh.example/api.php".into(),
                lang: "en".into(),
                license_name: None,
                license_url: None,
                character_category: None,
                work_category: None,
                kind_hint: None,
                added_by: None,
            },
        )
        .await
        .expect("source");
        w.invalidate().await;
        store::upsert(
            &w.pool,
            &page(5, "Silent Hill 2", Kind::Game, None, true),
            chrono::Utc::now().timestamp(),
        )
        .await
        .expect("page");
        let got = w
            .fill_for("Silent Hill 2", None, Kind::Game)
            .await
            .expect("a fill");
        assert!(got.image.is_none(), "the picture failed the check");
        assert!(got.summary.is_some());
    }

    #[tokio::test]
    async fn a_slow_lookup_does_not_hold_the_card_and_finishes_for_the_next_view() {
        struct Slow(FakeWiki);
        impl Transport for Slow {
            async fn get(
                &self,
                u: &str,
                p: Vec<(String, String)>,
                t: Duration,
            ) -> Result<serde_json::Value, crate::wiki::client::WikiError> {
                tokio::time::sleep(Duration::from_millis(300)).await;
                self.0.get(u, p, t).await
            }
        }
        let pool = crate::db::connect_memory().await;
        let w = Arc::new(Wikis::new(pool, Arc::new(Slow(FakeWiki::default()))));
        store::insert(
            &w.pool,
            &store::NewSource {
                name: "SH".into(),
                host: "sh.example".into(),
                api_url: "https://sh.example/api.php".into(),
                lang: "en".into(),
                license_name: None,
                license_url: None,
                character_category: None,
                work_category: None,
                kind_hint: None,
                added_by: None,
            },
        )
        .await
        .expect("source");
        w.invalidate().await;
        let started = std::time::Instant::now();
        let first = w
            .fill_within("Nothing Known", None, Kind::Game, Duration::from_millis(50))
            .await;
        assert!(
            first.is_none() && started.elapsed() < Duration::from_millis(250),
            "the card did not wait"
        );
        // the background lookup completes and the (negative) answer is cached
        tokio::time::sleep(Duration::from_millis(700)).await;
        let before = started.elapsed();
        let _ = w.fill_for("Nothing Known", None, Kind::Game).await;
        assert!(
            started.elapsed() - before < Duration::from_millis(100),
            "the second look is served from the cache"
        );
    }

    #[tokio::test]
    async fn a_live_card_gets_its_fill_later_and_asks_for_an_update() {
        let w = service(FakeWiki::default()).await;
        store::insert(
            &w.pool,
            &store::NewSource {
                name: "SH".into(),
                host: "sh.example".into(),
                api_url: "https://sh.example/api.php".into(),
                lang: "en".into(),
                license_name: None,
                license_url: None,
                character_category: None,
                work_category: None,
                kind_hint: None,
                added_by: None,
            },
        )
        .await
        .expect("source");
        w.invalidate().await;
        store::upsert(
            &w.pool,
            &page(5, "Silent Hill 2", Kind::Game, None, true),
            chrono::Utc::now().timestamp(),
        )
        .await
        .expect("page");
        let mut live = LiveFill::new();
        assert!(!live.pending() && !live.poll().await);
        live.start(&w, "igdb:1", "Silent Hill 2", None, Kind::Game);
        assert!(live.pending());
        let mut updated = false;
        for _ in 0..50 {
            if live.poll().await {
                updated = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(updated, "the view is asked to draw the card again");
        assert!(live.fill.is_some() && !live.pending());
        // the same item again does nothing; a new item starts over
        live.start(&w, "igdb:1", "Silent Hill 2", None, Kind::Game);
        assert!(!live.pending());
        live.start(&w, "igdb:2", "Nothing Known", None, Kind::Game);
        assert!(live.pending() && live.fill.is_none());
    }

    #[tokio::test]
    async fn a_live_card_without_a_match_never_asks_for_an_update() {
        let w = service(FakeWiki::default()).await;
        let mut live = LiveFill::new();
        live.start(&w, "igdb:9", "No Such Game", None, Kind::Game);
        let mut asked = false;
        for _ in 0..20 {
            asked |= live.poll().await;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!asked, "nothing to show, so the message is left alone");
        assert!(!live.pending());
    }
}
