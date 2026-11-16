//! Member suggestions: "if you liked A, try B", per server. Ids are provider-prefixed (`anilist:1`, `igdb:5`,
//! `vndb:v17`); the same pair (in either order) can only be added once per server.

use sqlx::{FromRow, SqlitePool};

pub type Result<T> = std::result::Result<T, sqlx::Error>;

pub const MAX_REASON: usize = 300;
pub const DEFAULT_DAILY_LIMIT: i64 = 10;

#[derive(Debug, Clone, FromRow, PartialEq, Eq)]
pub struct Suggestion {
    pub id: i64,
    pub guild_id: i64,
    pub author_id: i64,
    pub first_id: String,
    pub second_id: String,
    pub first_title: String,
    pub second_title: String,
    pub first_url: Option<String>,
    pub second_url: Option<String>,
    pub first_image: Option<String>,
    pub second_image: Option<String>,
    pub reason: String,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewSuggestion {
    pub guild_id: i64,
    pub author_id: i64,
    pub first_id: String,
    pub second_id: String,
    pub first_title: String,
    pub second_title: String,
    pub first_url: Option<String>,
    pub second_url: Option<String>,
    pub first_image: Option<String>,
    pub second_image: Option<String>,
    pub reason: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddResult {
    Added(i64),
    /// The pair exists already; `author` added it.
    Duplicate {
        author: i64,
    },
    /// The author used up today's allowance.
    LimitReached,
    /// Both ids are the same work.
    Same,
}

/// Order-insensitive key of a pair.
pub fn pair_key(a: &str, b: &str) -> String {
    let (x, y) = if a <= b { (a, b) } else { (b, a) };
    format!("{x}|{y}")
}

pub async fn add(pool: &SqlitePool, s: &NewSuggestion, daily_limit: i64) -> Result<AddResult> {
    if s.first_id == s.second_id {
        return Ok(AddResult::Same);
    }
    let key = pair_key(&s.first_id, &s.second_id);
    if let Some(author) = sqlx::query_scalar::<_, i64>(
        "SELECT author_id FROM suggestions WHERE guild_id = ?1 AND pair_key = ?2",
    )
    .bind(s.guild_id)
    .bind(&key)
    .fetch_optional(pool)
    .await?
    {
        return Ok(AddResult::Duplicate { author });
    }
    if daily_limit > 0 && count_today(pool, s.guild_id, s.author_id).await? >= daily_limit {
        return Ok(AddResult::LimitReached);
    }
    let r = sqlx::query(
        "INSERT INTO suggestions (guild_id, author_id, first_id, second_id, first_title, second_title, first_url, \
         second_url, first_image, second_image, pair_key, reason) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
    )
    .bind(s.guild_id)
    .bind(s.author_id)
    .bind(&s.first_id)
    .bind(&s.second_id)
    .bind(&s.first_title)
    .bind(&s.second_title)
    .bind(&s.first_url)
    .bind(&s.second_url)
    .bind(&s.first_image)
    .bind(&s.second_image)
    .bind(&key)
    .bind(&s.reason)
    .execute(pool)
    .await?;
    Ok(AddResult::Added(r.last_insert_rowid()))
}

/// Suggestions the author added in the last 24 hours in this server.
pub async fn count_today(pool: &SqlitePool, guild: i64, author: i64) -> Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM suggestions WHERE guild_id = ?1 AND author_id = ?2 AND created_at > unixepoch() - 86400")
        .bind(guild)
        .bind(author)
        .fetch_one(pool)
        .await
}

const COLS: &str = "id, guild_id, author_id, first_id, second_id, first_title, second_title, first_url, second_url, first_image, second_image, reason, created_at";

/// A random suggestion of this server, optionally only those that involve `work`.
pub async fn random(
    pool: &SqlitePool,
    guild: i64,
    work: Option<&str>,
) -> Result<Option<Suggestion>> {
    let sql = format!(
        "SELECT {COLS} FROM suggestions WHERE guild_id = ?1 AND (?2 IS NULL OR first_id = ?2 OR second_id = ?2) ORDER BY RANDOM() LIMIT 1"
    );
    sqlx::query_as(&sql)
        .bind(guild)
        .bind(work)
        .fetch_optional(pool)
        .await
}

pub async fn by_author(
    pool: &SqlitePool,
    guild: i64,
    author: i64,
    offset: i64,
    limit: i64,
) -> Result<Vec<Suggestion>> {
    let sql = format!("SELECT {COLS} FROM suggestions WHERE guild_id = ?1 AND author_id = ?2 ORDER BY id DESC LIMIT ?3 OFFSET ?4");
    sqlx::query_as(&sql)
        .bind(guild)
        .bind(author)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
}

pub async fn count_by_author(pool: &SqlitePool, guild: i64, author: i64) -> Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM suggestions WHERE guild_id = ?1 AND author_id = ?2")
        .bind(guild)
        .bind(author)
        .fetch_one(pool)
        .await
}

/// All suggestions a member made, in every server (for the profile).
pub async fn count_everywhere(pool: &SqlitePool, author: i64) -> Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM suggestions WHERE author_id = ?1")
        .bind(author)
        .fetch_one(pool)
        .await
}

pub async fn get(pool: &SqlitePool, guild: i64, id: i64) -> Result<Option<Suggestion>> {
    let sql = format!("SELECT {COLS} FROM suggestions WHERE guild_id = ?1 AND id = ?2");
    sqlx::query_as(&sql)
        .bind(guild)
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Authors delete their own suggestions; `admin` may delete any. `true` when a row was removed.
pub async fn delete(
    pool: &SqlitePool,
    guild: i64,
    id: i64,
    user: i64,
    admin: bool,
) -> Result<bool> {
    let r = sqlx::query(
        "DELETE FROM suggestions WHERE guild_id = ?1 AND id = ?2 AND (author_id = ?3 OR ?4)",
    )
    .bind(guild)
    .bind(id)
    .bind(user)
    .bind(admin)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_memory;

    fn new(author: i64, a: &str, b: &str) -> NewSuggestion {
        NewSuggestion {
            guild_id: 1,
            author_id: author,
            first_id: a.into(),
            second_id: b.into(),
            first_title: format!("T {a}"),
            second_title: format!("T {b}"),
            first_url: Some(format!("https://anilist.co/anime/{a}")),
            second_url: None,
            first_image: Some("https://s4.anilist.co/a.png".into()),
            second_image: None,
            reason: "same mood".into(),
        }
    }

    #[test]
    fn pair_keys_ignore_order() {
        assert_eq!(pair_key("a", "b"), pair_key("b", "a"));
        assert_ne!(pair_key("a", "b"), pair_key("a", "c"));
    }

    #[tokio::test]
    async fn duplicates_are_refused_in_either_order_and_name_the_first_author() {
        let pool = connect_memory().await;
        assert!(matches!(
            add(&pool, &new(7, "anilist:1", "igdb:2"), 10)
                .await
                .expect("add"),
            AddResult::Added(_)
        ));
        assert_eq!(
            add(&pool, &new(8, "igdb:2", "anilist:1"), 10)
                .await
                .expect("dup"),
            AddResult::Duplicate { author: 7 }
        );
        assert_eq!(
            add(&pool, &new(8, "anilist:1", "anilist:1"), 10)
                .await
                .expect("same"),
            AddResult::Same
        );
        // Another server may add the same pair.
        let mut other = new(8, "anilist:1", "igdb:2");
        other.guild_id = 2;
        assert!(matches!(
            add(&pool, &other, 10).await.expect("other"),
            AddResult::Added(_)
        ));
    }

    #[tokio::test]
    async fn the_daily_limit_applies_per_author_and_zero_means_unlimited() {
        let pool = connect_memory().await;
        for i in 0..3 {
            assert!(matches!(
                add(
                    &pool,
                    &new(7, "anilist:1", &format!("anilist:{}", i + 10)),
                    3
                )
                .await
                .expect("add"),
                AddResult::Added(_)
            ));
        }
        assert_eq!(
            add(&pool, &new(7, "anilist:1", "anilist:99"), 3)
                .await
                .expect("limit"),
            AddResult::LimitReached
        );
        assert!(matches!(
            add(&pool, &new(8, "anilist:1", "anilist:99"), 3)
                .await
                .expect("other author"),
            AddResult::Added(_)
        ));
        assert!(matches!(
            add(&pool, &new(7, "anilist:1", "anilist:98"), 0)
                .await
                .expect("unlimited"),
            AddResult::Added(_)
        ));
        assert_eq!(count_today(&pool, 1, 7).await.expect("n"), 4);
    }

    #[tokio::test]
    async fn random_mine_and_delete_rules() {
        let pool = connect_memory().await;
        assert!(random(&pool, 1, None).await.expect("r").is_none());
        let AddResult::Added(id) = add(&pool, &new(7, "anilist:1", "anilist:2"), 10)
            .await
            .expect("a")
        else {
            panic!("added")
        };
        add(&pool, &new(8, "anilist:3", "anilist:4"), 10)
            .await
            .expect("b");
        assert!(random(&pool, 1, None).await.expect("r").is_some());
        assert_eq!(
            random(&pool, 1, Some("anilist:3"))
                .await
                .expect("r")
                .map(|s| s.author_id),
            Some(8)
        );
        assert!(random(&pool, 1, Some("anilist:9"))
            .await
            .expect("r")
            .is_none());
        assert_eq!(by_author(&pool, 1, 7, 0, 10).await.expect("mine").len(), 1);
        assert_eq!(count_by_author(&pool, 1, 8).await.expect("n"), 1);
        assert_eq!(count_everywhere(&pool, 7).await.expect("n"), 1);
        assert!(
            !delete(&pool, 1, id, 99, false).await.expect("d"),
            "strangers cannot delete"
        );
        assert!(
            delete(&pool, 1, id, 99, true).await.expect("d"),
            "admins can delete any"
        );
        assert!(get(&pool, 1, id).await.expect("g").is_none());
        let AddResult::Added(own) = add(&pool, &new(7, "anilist:1", "anilist:2"), 10)
            .await
            .expect("a")
        else {
            panic!("added")
        };
        assert!(
            delete(&pool, 1, own, 7, false).await.expect("d"),
            "authors delete their own"
        );
    }
}
