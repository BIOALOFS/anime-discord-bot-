//! Watch / read status of a work for a member (`item_status`), for every source.

use sqlx::SqlitePool;

pub type Result<T> = std::result::Result<T, sqlx::Error>;

/// `(key, Arabic label)`; the label of the first two depends on whether the work is read or watched.
pub const STATUSES: &[&str] = &["current", "completed", "planned", "paused", "dropped"];

pub fn label(status: &str, written: bool) -> &'static str {
    match (status, written) {
        ("current", false) => "أشاهده الآن",
        ("current", true) => "أقرؤه الآن",
        ("completed", _) => "أكملته",
        ("planned", false) => "سأشاهده لاحقًا",
        ("planned", true) => "سأقرؤه لاحقًا",
        ("paused", _) => "متوقف",
        ("dropped", _) => "أسقطته",
        _ => "—",
    }
}

pub fn is_valid(status: &str) -> bool {
    STATUSES.contains(&status)
}

pub async fn set(pool: &SqlitePool, user: i64, key: &str, status: &str) -> Result<()> {
    if !is_valid(status) {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO item_status (user_id, work_key, status, updated_at) VALUES (?1, ?2, ?3, unixepoch())
         ON CONFLICT(user_id, work_key) DO UPDATE SET status = excluded.status, updated_at = excluded.updated_at",
    )
    .bind(user)
    .bind(key)
    .bind(status)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear(pool: &SqlitePool, user: i64, key: &str) -> Result<()> {
    sqlx::query("DELETE FROM item_status WHERE user_id = ?1 AND work_key = ?2")
        .bind(user)
        .bind(key)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get(pool: &SqlitePool, user: i64, key: &str) -> Result<Option<String>> {
    sqlx::query_scalar("SELECT status FROM item_status WHERE user_id = ?1 AND work_key = ?2")
        .bind(user)
        .bind(key)
        .fetch_optional(pool)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> SqlitePool {
        let p = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("db");
        sqlx::migrate!("./migrations")
            .run(&p)
            .await
            .expect("migrate");
        p
    }

    #[tokio::test]
    async fn status_is_set_changed_read_and_cleared_per_member_and_work() {
        let p = pool().await;
        assert_eq!(get(&p, 1, "tmdb-tv:5").await.expect("get"), None);
        set(&p, 1, "tmdb-tv:5", "current").await.expect("set");
        set(&p, 2, "tmdb-tv:5", "dropped").await.expect("set");
        set(&p, 1, "tmdb-tv:5", "completed").await.expect("update");
        assert_eq!(
            get(&p, 1, "tmdb-tv:5").await.expect("get").as_deref(),
            Some("completed")
        );
        assert_eq!(
            get(&p, 2, "tmdb-tv:5").await.expect("get").as_deref(),
            Some("dropped")
        );
        set(&p, 1, "anilist:1", "nonsense").await.expect("ignored");
        assert_eq!(
            get(&p, 1, "anilist:1").await.expect("get"),
            None,
            "unknown statuses are refused"
        );
        clear(&p, 1, "tmdb-tv:5").await.expect("clear");
        assert_eq!(get(&p, 1, "tmdb-tv:5").await.expect("get"), None);
    }

    #[test]
    fn labels_follow_watching_or_reading() {
        assert_eq!(label("current", false), "أشاهده الآن");
        assert_eq!(label("current", true), "أقرؤه الآن");
        assert_eq!(label("dropped", true), "أسقطته");
        assert!(STATUSES.iter().all(|s| label(s, false) != "—"));
    }
}
