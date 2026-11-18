//! The wiki tables: the sources the owner defined, the page cache and its full-text index, and the global switch.

use super::{normalize, Kind, Page, Source};
use sqlx::{Row, SqlitePool};

type Result<T> = std::result::Result<T, sqlx::Error>;

/// Cached pages older than this are not used and are purged (a week, as for page details).
pub const PAGE_TTL_SECS: i64 = 7 * 24 * 3600;

const SOURCE_COLUMNS: &str =
    "id, name, host, api_url, lang, license_name, license_url, character_category, \
    game_category, enabled, added_by, created_at, last_checked_at, last_error";

// ---- sources ---------------------------------------------------------------------------------------

pub async fn list(pool: &SqlitePool) -> Result<Vec<Source>> {
    sqlx::query_as(&format!(
        "SELECT {SOURCE_COLUMNS} FROM wiki_sources ORDER BY id"
    ))
    .fetch_all(pool)
    .await
}

#[cfg(test)]
pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<Source>> {
    sqlx::query_as(&format!(
        "SELECT {SOURCE_COLUMNS} FROM wiki_sources WHERE id = ?1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// What the owner confirmed when adding a wiki.
#[derive(Debug, Clone)]
pub struct NewSource {
    pub name: String,
    pub host: String,
    pub api_url: String,
    pub lang: String,
    pub license_name: Option<String>,
    pub license_url: Option<String>,
    pub character_category: Option<String>,
    pub game_category: Option<String>,
    pub added_by: Option<i64>,
}

/// The outcome of adding a wiki.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Added {
    New(i64),
    /// The host is already there.
    Duplicate(i64),
}

/// A new wiki is enabled at once, for every server. The same host twice is refused (the first one is returned).
pub async fn insert(pool: &SqlitePool, n: &NewSource) -> Result<Added> {
    if let Some(id) = sqlx::query_scalar::<_, i64>("SELECT id FROM wiki_sources WHERE host = ?1")
        .bind(&n.host)
        .fetch_optional(pool)
        .await?
    {
        return Ok(Added::Duplicate(id));
    }
    let r = sqlx::query(
        "INSERT INTO wiki_sources (name, host, api_url, lang, license_name, license_url, character_category, game_category, enabled, added_by) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9)",
    )
    .bind(&n.name)
    .bind(&n.host)
    .bind(&n.api_url)
    .bind(&n.lang)
    .bind(&n.license_name)
    .bind(&n.license_url)
    .bind(&n.character_category)
    .bind(&n.game_category)
    .bind(n.added_by)
    .execute(pool)
    .await?;
    Ok(Added::New(r.last_insert_rowid()))
}

pub async fn update(
    pool: &SqlitePool,
    id: i64,
    name: &str,
    character_category: Option<&str>,
    game_category: Option<&str>,
) -> Result<bool> {
    let r = sqlx::query("UPDATE wiki_sources SET name = ?2, character_category = ?3, game_category = ?4 WHERE id = ?1")
        .bind(id)
        .bind(name)
        .bind(character_category)
        .bind(game_category)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn set_enabled(pool: &SqlitePool, id: i64, enabled: bool) -> Result<bool> {
    let r = sqlx::query("UPDATE wiki_sources SET enabled = ?2 WHERE id = ?1")
        .bind(id)
        .bind(enabled)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// Removes only the source definition. Its cached pages are never shown again (reads join the enabled sources) and
/// are purged when they are old; member data is not involved at all.
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<bool> {
    let r = sqlx::query("DELETE FROM wiki_sources WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// Record the outcome of a call to a wiki (the owner sees the last error).
pub async fn mark_checked(pool: &SqlitePool, id: i64, now: i64, error: Option<&str>) -> Result<()> {
    sqlx::query("UPDATE wiki_sources SET last_checked_at = ?2, last_error = ?3 WHERE id = ?1")
        .bind(id)
        .bind(now)
        .bind(error.map(|e| e.chars().take(300).collect::<String>()))
        .execute(pool)
        .await?;
    Ok(())
}

// ---- the global switch -----------------------------------------------------------------------------

const SWITCH_KEY: &str = "wiki_global_enabled";

/// The emergency switch: on unless the owner turned it off (also on when unreadable).
pub async fn global_enabled(pool: &SqlitePool) -> bool {
    sqlx::query_scalar::<_, String>("SELECT value FROM bot_state WHERE key = ?1")
        .bind(SWITCH_KEY)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .is_none_or(|v| v != "0")
}

pub async fn set_global_enabled(pool: &SqlitePool, on: bool) -> Result<()> {
    sqlx::query(
        "INSERT INTO bot_state (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(SWITCH_KEY)
    .bind(if on { "1" } else { "0" })
    .execute(pool)
    .await?;
    Ok(())
}

// ---- the page cache --------------------------------------------------------------------------------

const PAGE_COLUMNS: &str = "c.wiki_id, c.page_id, c.title, c.kind, c.extract, c.image_url, c.aliases, c.parent_title, c.categories, c.fetched_at";

fn page_of(r: &sqlx::sqlite::SqliteRow) -> (Page, i64) {
    let list = |col: &str| -> Vec<String> {
        r.get::<Option<String>, _>(col)
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default()
    };
    (
        Page {
            wiki_id: r.get("wiki_id"),
            page_id: r.get("page_id"),
            title: r.get("title"),
            kind: Kind::from_key(&r.get::<String, _>("kind")),
            extract: r.get("extract"),
            image_url: r.get("image_url"),
            aliases: list("aliases"),
            parent_title: r.get("parent_title"),
            categories: list("categories"),
        },
        r.get("fetched_at"),
    )
}

pub async fn upsert(pool: &SqlitePool, p: &Page, now: i64) -> Result<()> {
    sqlx::query(
        "INSERT INTO wiki_cache (wiki_id, page_id, title, normalized_key, kind, extract, image_url, aliases, parent_title, categories, fetched_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
         ON CONFLICT(wiki_id, page_id) DO UPDATE SET title = excluded.title, normalized_key = excluded.normalized_key, kind = excluded.kind, \
         extract = excluded.extract, image_url = excluded.image_url, aliases = excluded.aliases, parent_title = excluded.parent_title, \
         categories = excluded.categories, fetched_at = excluded.fetched_at",
    )
    .bind(p.wiki_id)
    .bind(p.page_id)
    .bind(&p.title)
    .bind(p.normalized())
    .bind(p.kind.key())
    .bind(&p.extract)
    .bind(&p.image_url)
    .bind(serde_json::to_string(&p.aliases).unwrap_or_else(|_| "[]".into()))
    .bind(&p.parent_title)
    .bind(serde_json::to_string(&p.categories).unwrap_or_else(|_| "[]".into()))
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// One cached page of an enabled wiki, if it is fresh.
pub async fn get_page(
    pool: &SqlitePool,
    wiki_id: i64,
    page_id: i64,
    now: i64,
) -> Result<Option<Page>> {
    let r = sqlx::query(&format!(
        "SELECT {PAGE_COLUMNS} FROM wiki_cache c JOIN wiki_sources s ON s.id = c.wiki_id AND s.enabled = 1 \
         WHERE c.wiki_id = ?1 AND c.page_id = ?2 AND c.fetched_at > ?3"
    ))
    .bind(wiki_id)
    .bind(page_id)
    .bind(now - PAGE_TTL_SECS)
    .fetch_optional(pool)
    .await?;
    Ok(r.as_ref().map(|r| page_of(r).0))
}

/// Local suggestions for what the member has typed so far: the words of the text as prefixes over titles and aliases
/// (full-text index), best matches first (exact title, then prefix, then shorter titles). Only enabled wikis,
/// only fresh pages. `wiki` narrows to one wiki, `kind` to characters or games.
pub async fn suggest(
    pool: &SqlitePool,
    text: &str,
    kind: Option<Kind>,
    wiki: Option<i64>,
    limit: i64,
    now: i64,
) -> Result<Vec<Page>> {
    let norm = normalize(text);
    if norm.is_empty() {
        return Ok(Vec::new());
    }
    // Every word is a prefix term; quotes make a term safe whatever the member typed.
    let query = norm
        .split(' ')
        .map(|w| format!("\"{}\"*", w.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" ");
    let rows = sqlx::query(&format!(
        "SELECT {PAGE_COLUMNS} FROM wiki_fts f JOIN wiki_cache c ON c.rowid = f.rowid \
         JOIN wiki_sources s ON s.id = c.wiki_id AND s.enabled = 1 \
         WHERE wiki_fts MATCH ?1 AND c.fetched_at > ?2 AND (?3 IS NULL OR c.kind = ?3) AND (?4 IS NULL OR c.wiki_id = ?4) \
         ORDER BY (c.normalized_key = ?5) DESC, (c.normalized_key LIKE ?6) DESC, length(c.title) LIMIT ?7"
    ))
    .bind(query)
    .bind(now - PAGE_TTL_SECS)
    .bind(kind.map(Kind::key))
    .bind(wiki)
    .bind(&norm)
    .bind(format!("{}%", norm.replace(['%', '_'], "")))
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|r| page_of(r).0).collect())
}

/// Fresh pages of enabled wikis with exactly this normalised title (any wiki), for matching and filling.
pub async fn by_title(
    pool: &SqlitePool,
    normalized_title: &str,
    kind: Option<Kind>,
    now: i64,
) -> Result<Vec<Page>> {
    let rows = sqlx::query(&format!(
        "SELECT {PAGE_COLUMNS} FROM wiki_cache c JOIN wiki_sources s ON s.id = c.wiki_id AND s.enabled = 1 \
         WHERE c.normalized_key = ?1 AND c.fetched_at > ?2 AND (?3 IS NULL OR c.kind = ?3) ORDER BY c.wiki_id, c.page_id"
    ))
    .bind(normalized_title)
    .bind(now - PAGE_TTL_SECS)
    .bind(kind.map(Kind::key))
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|r| page_of(r).0).collect())
}

/// The cached game pages of one wiki that carry a given normalised title (for "أظهر العمل").
pub async fn characters_of(
    pool: &SqlitePool,
    wiki_id: i64,
    parent_normalized: &str,
    now: i64,
) -> Result<Vec<Page>> {
    let rows = sqlx::query(&format!(
        "SELECT {PAGE_COLUMNS} FROM wiki_cache c JOIN wiki_sources s ON s.id = c.wiki_id AND s.enabled = 1 \
         WHERE c.wiki_id = ?1 AND c.kind = 'character' AND c.fetched_at > ?2 AND lower(c.parent_title) IS NOT NULL ORDER BY c.title"
    ))
    .bind(wiki_id)
    .bind(now - PAGE_TTL_SECS)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| page_of(r).0)
        .filter(|p| p.parent_title.as_deref().map(normalize).as_deref() == Some(parent_normalized))
        .collect())
}

/// The cached characters of enabled wikis whose series matches this work's title strongly (any wiki). Cache only: it
/// answers at once, and the background warmer and earlier searches are what fill it.
pub async fn characters_for_work(
    pool: &SqlitePool,
    work_title: &str,
    now: i64,
    limit: usize,
) -> Result<Vec<Page>> {
    let rows = sqlx::query(&format!(
        "SELECT {PAGE_COLUMNS} FROM wiki_cache c JOIN wiki_sources s ON s.id = c.wiki_id AND s.enabled = 1 \
         WHERE c.kind = 'character' AND c.parent_title IS NOT NULL AND c.fetched_at > ?1 ORDER BY c.title LIMIT 5000"
    ))
    .bind(now - PAGE_TTL_SECS)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| page_of(r).0)
        .filter(|p| {
            p.parent_title
                .as_deref()
                .is_some_and(|g| super::clean::series_matches(g, work_title))
        })
        .take(limit)
        .collect())
}

/// Pages older than their lifetime go (this is how the pages of a removed wiki disappear). Returns how many.
pub async fn purge_stale(pool: &SqlitePool, now: i64) -> Result<u64> {
    let r = sqlx::query("DELETE FROM wiki_cache WHERE fetched_at <= ?1")
        .bind(now - PAGE_TTL_SECS)
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}

/// The owner's "clear the wiki cache": the cached pages only, never a source and never member data.
pub async fn clear_cache(pool: &SqlitePool) -> Result<u64> {
    let r = sqlx::query("DELETE FROM wiki_cache").execute(pool).await?;
    Ok(r.rows_affected())
}

pub async fn page_count(pool: &SqlitePool) -> Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM wiki_cache")
        .fetch_one(pool)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(host: &str) -> NewSource {
        NewSource {
            name: format!("{host} wiki"),
            host: host.into(),
            api_url: format!("https://{host}/api.php"),
            lang: "en".into(),
            license_name: Some("CC BY-SA".into()),
            license_url: Some("https://creativecommons.org/licenses/by-sa/3.0/".into()),
            character_category: Some("Category:Characters".into()),
            game_category: None,
            added_by: Some(1),
        }
    }

    fn page(wiki: i64, id: i64, title: &str, kind: Kind, parent: Option<&str>) -> Page {
        Page {
            wiki_id: wiki,
            page_id: id,
            title: title.into(),
            kind,
            extract: Some(format!("About {title}.")),
            image_url: None,
            aliases: vec![format!("{title} alias")],
            parent_title: parent.map(str::to_string),
            categories: vec!["Characters".into()],
        }
    }

    #[tokio::test]
    async fn a_new_wiki_is_enabled_at_once_for_everyone_and_the_same_host_is_not_added_twice() {
        let pool = crate::db::connect_memory().await;
        let Added::New(id) = insert(&pool, &new("silenthill.fandom.com"))
            .await
            .expect("insert")
        else {
            panic!("new");
        };
        let w = get(&pool, id).await.expect("get").expect("row");
        assert!(
            w.enabled,
            "enabled on creation, in every server (there is no per-server row)"
        );
        assert_eq!(w.host, "silenthill.fandom.com");
        assert_eq!(
            insert(&pool, &new("silenthill.fandom.com"))
                .await
                .expect("again"),
            Added::Duplicate(id)
        );
        assert_eq!(list(&pool).await.expect("list").len(), 1);
    }

    #[tokio::test]
    async fn toggling_editing_and_deleting_are_global() {
        let pool = crate::db::connect_memory().await;
        let Added::New(id) = insert(&pool, &new("a.fandom.com")).await.expect("insert") else {
            panic!("new");
        };
        assert!(set_enabled(&pool, id, false).await.expect("off"));
        assert!(!get(&pool, id).await.expect("get").expect("row").enabled);
        assert!(update(
            &pool,
            id,
            "Renamed",
            Some("Category:Cast"),
            Some("Category:Games")
        )
        .await
        .expect("update"));
        let w = get(&pool, id).await.expect("get").expect("row");
        assert_eq!(
            (
                w.name.as_str(),
                w.character_category.as_deref(),
                w.game_category.as_deref()
            ),
            ("Renamed", Some("Category:Cast"), Some("Category:Games"))
        );
        mark_checked(&pool, id, 100, Some("timeout"))
            .await
            .expect("mark");
        assert_eq!(
            get(&pool, id)
                .await
                .expect("get")
                .expect("row")
                .last_error
                .as_deref(),
            Some("timeout")
        );
        assert!(delete(&pool, id).await.expect("delete"));
        assert!(!delete(&pool, id).await.expect("again"));
        assert!(list(&pool).await.expect("list").is_empty());
    }

    #[tokio::test]
    async fn the_kill_switch_is_on_by_default_and_remembered() {
        let pool = crate::db::connect_memory().await;
        assert!(global_enabled(&pool).await);
        set_global_enabled(&pool, false).await.expect("off");
        assert!(!global_enabled(&pool).await);
        set_global_enabled(&pool, true).await.expect("on");
        assert!(global_enabled(&pool).await);
    }

    #[tokio::test]
    async fn the_full_text_index_finds_prefixes_by_title_and_alias_and_ranks_exact_first() {
        let pool = crate::db::connect_memory().await;
        let Added::New(w) = insert(&pool, &new("sh.fandom.com")).await.expect("insert") else {
            panic!("new");
        };
        for p in [
            page(w, 1, "Pyramid Head", Kind::Character, Some("Silent Hill 2")),
            page(
                w,
                2,
                "Pyramid Head (Homecoming)",
                Kind::Character,
                Some("Silent Hill: Homecoming"),
            ),
            page(
                w,
                3,
                "Heather Mason",
                Kind::Character,
                Some("Silent Hill 3"),
            ),
            page(w, 4, "Silent Hill 2", Kind::Game, None),
        ] {
            upsert(&pool, &p, 1_000).await.expect("upsert");
        }
        let hits = suggest(&pool, "pyram", None, None, 10, 1_100)
            .await
            .expect("suggest");
        assert_eq!(
            hits.iter().map(|p| p.page_id).collect::<Vec<_>>(),
            vec![1, 2],
            "the shorter title first"
        );
        let exact = suggest(&pool, "Pyramid-Head", None, None, 10, 1_100)
            .await
            .expect("suggest");
        assert_eq!(exact[0].page_id, 1, "an exact normalised title leads");
        assert_eq!(
            suggest(&pool, "heath mas", None, None, 10, 1_100)
                .await
                .expect("suggest")[0]
                .page_id,
            3
        );
        assert_eq!(
            suggest(&pool, "silent", Some(Kind::Game), None, 10, 1_100)
                .await
                .expect("suggest")
                .len(),
            1,
            "the kind filter"
        );
        assert_eq!(
            suggest(&pool, "alias", None, None, 10, 1_100)
                .await
                .expect("suggest")
                .len(),
            4,
            "aliases are indexed"
        );
        assert!(
            suggest(
                &pool,
                "\"; DROP TABLE wiki_cache; --",
                None,
                None,
                10,
                1_100
            )
            .await
            .is_ok(),
            "any text is safe"
        );
        assert!(suggest(&pool, "   ", None, None, 10, 1_100)
            .await
            .expect("blank")
            .is_empty());
        // an update keeps the index in step
        let mut renamed = page(w, 3, "Heather Mason Renamed", Kind::Character, None);
        renamed.aliases = vec![];
        upsert(&pool, &renamed, 1_200).await.expect("update");
        assert!(suggest(&pool, "heather mason", None, None, 10, 1_300)
            .await
            .expect("suggest")
            .iter()
            .any(|p| p.title.ends_with("Renamed")));
        assert!(
            suggest(&pool, "heather", None, None, 10, 1_300)
                .await
                .expect("suggest")
                .len()
                == 1
        );
    }

    #[tokio::test]
    async fn disabled_or_deleted_wikis_and_old_pages_are_never_suggested() {
        let pool = crate::db::connect_memory().await;
        let Added::New(w) = insert(&pool, &new("sh.fandom.com")).await.expect("insert") else {
            panic!("new");
        };
        upsert(
            &pool,
            &page(w, 1, "Pyramid Head", Kind::Character, None),
            1_000,
        )
        .await
        .expect("upsert");
        assert_eq!(
            suggest(&pool, "pyramid", None, None, 5, 1_100)
                .await
                .expect("suggest")
                .len(),
            1
        );
        set_enabled(&pool, w, false).await.expect("off");
        assert!(
            suggest(&pool, "pyramid", None, None, 5, 1_100)
                .await
                .expect("suggest")
                .is_empty(),
            "disabled everywhere at once"
        );
        assert!(get_page(&pool, w, 1, 1_100).await.expect("get").is_none());
        set_enabled(&pool, w, true).await.expect("on");
        assert!(
            suggest(&pool, "pyramid", None, None, 5, 1_000 + PAGE_TTL_SECS + 1)
                .await
                .expect("suggest")
                .is_empty(),
            "a stale page is not used"
        );
        // deleting the wiki leaves the rows to expire; they are not shown meanwhile
        delete(&pool, w).await.expect("delete");
        assert_eq!(page_count(&pool).await.expect("count"), 1);
        assert!(suggest(&pool, "pyramid", None, None, 5, 1_100)
            .await
            .expect("suggest")
            .is_empty());
        assert_eq!(
            purge_stale(&pool, 1_000 + PAGE_TTL_SECS + 1)
                .await
                .expect("purge"),
            1
        );
        assert_eq!(page_count(&pool).await.expect("count"), 0);
    }

    #[tokio::test]
    async fn deleting_a_wiki_or_clearing_the_cache_never_touches_member_data() {
        let pool = crate::db::connect_memory().await;
        crate::db::profile::create_profile(&pool, 1)
            .await
            .expect("profile");
        crate::db::profile::add_character(&pool, 1, 5)
            .await
            .expect("character");
        let Added::New(w) = insert(&pool, &new("sh.fandom.com")).await.expect("insert") else {
            panic!("new");
        };
        upsert(&pool, &page(w, 1, "A", Kind::Character, None), 1)
            .await
            .expect("upsert");
        let before = crate::db::user_data_counts(&pool).await.expect("counts");
        delete(&pool, w).await.expect("delete");
        clear_cache(&pool).await.expect("clear");
        let after = crate::db::user_data_counts(&pool).await.expect("counts");
        for (table, n) in &before {
            if table != "wiki_sources" {
                assert_eq!(after.get(table), Some(n), "{table}");
            }
        }
        assert_eq!(page_count(&pool).await.expect("count"), 0);
    }

    #[tokio::test]
    async fn characters_of_a_work_are_found_by_a_strong_series_match_in_any_wiki() {
        let pool = crate::db::connect_memory().await;
        for host in ["a.example.com", "b.example.com"] {
            insert(&pool, &new(host)).await.expect("insert");
        }
        for p in [
            page(1, 1, "Acerola", Kind::Character, Some("Pokémon Sun/Moon")),
            page(2, 2, "Lillie", Kind::Character, Some("Pokemon Sun & Moon")),
            page(
                1,
                3,
                "Cynthia",
                Kind::Character,
                Some("Pokémon Diamond/Pearl"),
            ),
            page(1, 4, "Silent Hill 2", Kind::Game, None),
        ] {
            upsert(&pool, &p, 10).await.expect("upsert");
        }
        let found = characters_for_work(&pool, "Pokémon Sun and Moon", 20, 25)
            .await
            .expect("found");
        assert_eq!(
            found.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(),
            vec!["Acerola", "Lillie"]
        );
        assert!(
            characters_for_work(&pool, "Pokémon", 20, 25)
                .await
                .expect("none")
                .is_empty(),
            "a vague name matches nothing"
        );
        set_enabled(&pool, 2, false).await.expect("off");
        assert_eq!(
            characters_for_work(&pool, "Pokémon Sun/Moon", 20, 25)
                .await
                .expect("found")
                .len(),
            1,
            "a disabled wiki is gone"
        );
    }

    #[tokio::test]
    async fn characters_of_a_game_are_found_by_its_normalised_name() {
        let pool = crate::db::connect_memory().await;
        let Added::New(w) = insert(&pool, &new("sh.fandom.com")).await.expect("insert") else {
            panic!("new");
        };
        for p in [
            page(w, 1, "Pyramid Head", Kind::Character, Some("Silent Hill 2")),
            page(
                w,
                2,
                "James Sunderland",
                Kind::Character,
                Some("Silent-Hill 2"),
            ),
            page(w, 3, "Heather", Kind::Character, Some("Silent Hill 3")),
        ] {
            upsert(&pool, &p, 10).await.expect("upsert");
        }
        let list = characters_of(&pool, w, &normalize("Silent Hill 2"), 20)
            .await
            .expect("list");
        assert_eq!(
            list.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(),
            vec!["James Sunderland", "Pyramid Head"]
        );
    }
}
