//! "Remind me" on work cards. A reminder fires once per released episode (`last_episode` is the dedupe).

use sqlx::{FromRow, SqlitePool};
use std::collections::HashMap;

pub type Result<T> = std::result::Result<T, sqlx::Error>;

pub const MAX_PER_USER: i64 = 50;

#[derive(Debug, Clone, FromRow, PartialEq, Eq)]
pub struct Reminder {
    pub user_id: i64,
    pub work_id: i64,
    pub title: String,
    pub last_episode: i64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddResult {
    Added,
    Already,
    Full,
}

pub async fn add(pool: &SqlitePool, user: i64, work: i64, title: &str) -> Result<AddResult> {
    if has(pool, user, work).await? {
        return Ok(AddResult::Already);
    }
    if count(pool, user).await? >= MAX_PER_USER {
        return Ok(AddResult::Full);
    }
    sqlx::query("INSERT OR IGNORE INTO reminders (user_id, work_id, title) VALUES (?1, ?2, ?3)")
        .bind(user)
        .bind(work)
        .bind(title.chars().take(100).collect::<String>())
        .execute(pool)
        .await?;
    Ok(AddResult::Added)
}

pub async fn remove(pool: &SqlitePool, user: i64, work: i64) -> Result<bool> {
    Ok(
        sqlx::query("DELETE FROM reminders WHERE user_id = ?1 AND work_id = ?2")
            .bind(user)
            .bind(work)
            .execute(pool)
            .await?
            .rows_affected()
            > 0,
    )
}

pub async fn has(pool: &SqlitePool, user: i64, work: i64) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM reminders WHERE user_id = ?1 AND work_id = ?2",
    )
    .bind(user)
    .bind(work)
    .fetch_one(pool)
    .await?
        > 0)
}

pub async fn count(pool: &SqlitePool, user: i64) -> Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM reminders WHERE user_id = ?1")
        .bind(user)
        .fetch_one(pool)
        .await
}

pub async fn list(pool: &SqlitePool, user: i64) -> Result<Vec<Reminder>> {
    sqlx::query_as("SELECT user_id, work_id, title, last_episode FROM reminders WHERE user_id = ?1 ORDER BY created_at DESC").bind(user).fetch_all(pool).await
}

/// Is any reminder set at all? (The poller skips the AniList call when nothing needs it.)
pub async fn any(pool: &SqlitePool) -> Result<bool> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM (SELECT 1 FROM reminders LIMIT 1)")
            .fetch_one(pool)
            .await?
            > 0,
    )
}

/// Reminders of the given works, grouped by work.
pub async fn for_works(pool: &SqlitePool, works: &[i64]) -> Result<HashMap<i64, Vec<Reminder>>> {
    let mut out: HashMap<i64, Vec<Reminder>> = HashMap::new();
    // 400 ids per query stays far below SQLite's variable limit.
    for chunk in works.chunks(400) {
        let marks = vec!["?"; chunk.len()].join(",");
        let sql = format!("SELECT user_id, work_id, title, last_episode FROM reminders WHERE work_id IN ({marks})");
        let mut q = sqlx::query_as::<_, Reminder>(&sql);
        for id in chunk {
            q = q.bind(id);
        }
        for r in q.fetch_all(pool).await? {
            out.entry(r.work_id).or_default().push(r);
        }
    }
    Ok(out)
}

pub async fn mark(pool: &SqlitePool, user: i64, work: i64, episode: i64) -> Result<()> {
    sqlx::query("UPDATE reminders SET last_episode = MAX(last_episode, ?3) WHERE user_id = ?1 AND work_id = ?2")
        .bind(user)
        .bind(work)
        .bind(episode)
        .execute(pool)
        .await?;
    Ok(())
}

/// Should this release be announced to a reminder? Each episode notifies once.
pub fn is_due(released_episode: i64, last_episode: i64) -> bool {
    released_episode > last_episode
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_memory;

    #[test]
    fn only_newer_episodes_are_due() {
        assert!(is_due(1, 0));
        assert!(is_due(6, 5));
        assert!(!is_due(5, 5), "the same episode never notifies twice");
        assert!(!is_due(4, 5), "an old episode never notifies");
    }

    #[tokio::test]
    async fn add_remove_and_cap() {
        let pool = connect_memory().await;
        assert!(!any(&pool).await.expect("any"));
        assert_eq!(
            add(&pool, 1, 10, "Show").await.expect("a"),
            AddResult::Added
        );
        assert_eq!(
            add(&pool, 1, 10, "Show").await.expect("a"),
            AddResult::Already
        );
        assert!(has(&pool, 1, 10).await.expect("h") && any(&pool).await.expect("any"));
        for w in 100..(100 + MAX_PER_USER - 1) {
            assert_eq!(add(&pool, 1, w, "x").await.expect("a"), AddResult::Added);
        }
        assert_eq!(
            add(&pool, 1, 9999, "x").await.expect("a"),
            AddResult::Full,
            "capped at {MAX_PER_USER}"
        );
        assert_eq!(
            add(&pool, 2, 9999, "x").await.expect("a"),
            AddResult::Added,
            "the cap is per user"
        );
        assert!(remove(&pool, 1, 10).await.expect("r") && !remove(&pool, 1, 10).await.expect("r"));
        assert_eq!(count(&pool, 1).await.expect("n"), MAX_PER_USER - 1);
    }

    #[tokio::test]
    async fn marking_dedupes_each_release() {
        let pool = connect_memory().await;
        add(&pool, 1, 10, "Show").await.expect("a");
        add(&pool, 2, 10, "Show").await.expect("a");
        add(&pool, 2, 11, "Other").await.expect("a");
        let by_work = for_works(&pool, &[10]).await.expect("w");
        assert_eq!(by_work[&10].len(), 2);
        assert!(!by_work.contains_key(&11));
        mark(&pool, 1, 10, 5).await.expect("m");
        mark(&pool, 1, 10, 3).await.expect("m");
        let r = &for_works(&pool, &[10]).await.expect("w")[&10];
        let mine = r.iter().find(|r| r.user_id == 1).expect("mine");
        assert_eq!(mine.last_episode, 5, "never goes backwards");
        assert!(!is_due(5, mine.last_episode) && is_due(6, mine.last_episode));
        assert_eq!(list(&pool, 2).await.expect("l").len(), 2);
    }
}
