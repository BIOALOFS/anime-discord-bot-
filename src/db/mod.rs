pub mod profile;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

pub async fn connect(url: &str) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);
    if let Some(dir) = opts
        .get_filename()
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        std::fs::create_dir_all(dir)?;
    }
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// One shared in-memory database (a single connection) for tests.
#[cfg(test)]
pub async fn connect_memory() -> SqlitePool {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")
        .expect("url")
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("connect");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrate");
    pool
}

#[cfg(test)]
mod tests {
    /// The default URL is relative to the working directory and the file is created on demand.
    #[tokio::test]
    async fn relative_default_url_creates_the_database() {
        let dir = std::env::temp_dir().join(format!("anpot-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let url = format!(
            "sqlite://{}?mode=rwc",
            dir.join("nested").join("bot.db").display()
        );
        let pool = super::connect(&url)
            .await
            .expect("connect creates parent dirs");
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM profiles")
            .fetch_one(&pool)
            .await
            .expect("migrated");
        assert_eq!(n, 0);
        assert!(dir.join("nested").join("bot.db").exists());
        drop(pool);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            crate::config::Config::for_tests().database_url,
            "sqlite::memory:"
        );
    }

    #[tokio::test]
    async fn migrations_apply() {
        let pool = super::connect_memory().await;
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM guild_settings")
            .fetch_one(&pool)
            .await
            .expect("query");
        assert_eq!(n, 0);
    }
}
