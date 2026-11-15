pub mod custom;
pub mod feed;
pub mod game;
pub mod profile;
pub mod works;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

pub async fn connect(url: &str) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);
    let path = opts.get_filename().to_path_buf();
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| {
            anyhow::anyhow!("cannot create database directory {}: {e}", dir.display())
        })?;
    }
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "cannot open database at {}: {e} (is the folder writable?)",
                path.display()
            )
        })?;
    let migrator = sqlx::migrate!("./migrations");
    if !url.contains(":memory:") {
        match backup_if_pending(&pool, &path, &migrator).await {
            Ok(Some(b)) => {
                tracing::info!(backup = %b.display(), "database backed up before migrating")
            }
            Ok(None) => {}
            // A failed backup must not stop the bot, but it must be visible.
            Err(e) => tracing::warn!("could not back up the database before migrating: {e:#}"),
        }
    }
    migrator.run(&pool).await?;
    Ok(pool)
}

pub const BACKUPS_KEPT: usize = 7;

/// If the database already has applied migrations and new ones are pending, copy it (consistently, via
/// `VACUUM INTO`) to `backups/<name>-YYYYmmdd-HHMMSS.db` next to it and keep the newest `BACKUPS_KEPT`.
pub async fn backup_if_pending(
    pool: &SqlitePool,
    db_path: &std::path::Path,
    migrator: &sqlx::migrate::Migrator,
) -> anyhow::Result<Option<std::path::PathBuf>> {
    // No migrations table yet: a brand-new database has nothing worth saving.
    let Ok(applied) =
        sqlx::query_scalar::<_, i64>("SELECT version FROM _sqlx_migrations WHERE success = 1")
            .fetch_all(pool)
            .await
    else {
        return Ok(None);
    };
    let pending = migrator
        .iter()
        .any(|m| !m.migration_type.is_down_migration() && !applied.contains(&m.version));
    if applied.is_empty() || !pending {
        return Ok(None);
    }
    let dir = db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."))
        .join("backups");
    std::fs::create_dir_all(&dir)?;
    let stem = db_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("bot");
    let target = dir.join(format!(
        "{stem}-{}.db",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    ));
    let literal = target.to_string_lossy().replace('\'', "''");
    sqlx::query(&format!("VACUUM INTO '{literal}'"))
        .execute(pool)
        .await?;
    prune_backups(&dir, stem, BACKUPS_KEPT)?;
    Ok(Some(target))
}

/// Keep only the `keep` newest `<stem>-*.db` files (names sort chronologically).
pub fn prune_backups(dir: &std::path::Path, stem: &str, keep: usize) -> std::io::Result<()> {
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&format!("{stem}-")) && n.ends_with(".db"))
        })
        .collect();
    files.sort();
    let excess = files.len().saturating_sub(keep);
    for old in files.into_iter().take(excess) {
        std::fs::remove_file(old)?;
    }
    Ok(())
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

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anpot-{tag}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[tokio::test]
    async fn no_backup_for_new_or_fully_migrated_databases() {
        let dir = temp_dir("nobackup");
        let url = format!("sqlite://{}?mode=rwc", dir.join("bot.db").display());
        let pool = super::connect(&url).await.expect("fresh connect");
        assert!(
            !dir.join("backups").exists(),
            "a brand-new database is not backed up"
        );
        drop(pool);
        let pool = super::connect(&url).await.expect("reconnect");
        assert!(
            !dir.join("backups").exists(),
            "nothing pending, nothing to back up"
        );
        drop(pool);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn existing_database_is_backed_up_before_pending_migrations() {
        let dir = temp_dir("backup");
        let db = dir.join("bot.db");
        let url = format!("sqlite://{}?mode=rwc", db.display());
        let pool = super::connect(&url).await.expect("connect");
        sqlx::query("INSERT INTO users (discord_id) VALUES (42)")
            .execute(&pool)
            .await
            .expect("data");
        // Pretend the newest migration is still pending.
        let migrator = sqlx::migrate!("./migrations");
        let newest = migrator
            .iter()
            .map(|m| m.version)
            .max()
            .expect("migrations");
        sqlx::query("DELETE FROM _sqlx_migrations WHERE version = ?1")
            .bind(newest)
            .execute(&pool)
            .await
            .expect("unapply");

        let backup = super::backup_if_pending(&pool, &db, &migrator)
            .await
            .expect("backup")
            .expect("a backup was made");
        assert!(backup.starts_with(dir.join("backups")) && backup.exists());
        let copy = sqlx::SqlitePool::connect(&format!("sqlite://{}", backup.display()))
            .await
            .expect("open backup");
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE discord_id = 42")
            .fetch_one(&copy)
            .await
            .expect("row");
        assert_eq!(n, 1, "the backup holds the existing data");
        drop(copy);
        drop(pool);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_newest_seven_backups_are_kept() {
        let dir = temp_dir("prune");
        for i in 0..10 {
            std::fs::write(dir.join(format!("bot-20260101-0000{i:02}.db")), b"x").expect("file");
        }
        std::fs::write(dir.join("other.txt"), b"keep me").expect("file");
        super::prune_backups(&dir, "bot", super::BACKUPS_KEPT).expect("prune");
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .expect("dir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names.iter().filter(|n| n.starts_with("bot-")).count(), 7);
        assert!(
            names.contains(&"bot-20260101-000009.db".to_string()),
            "newest survives"
        );
        assert!(
            !names.contains(&"bot-20260101-000000.db".to_string()),
            "oldest is gone"
        );
        assert!(names.contains(&"other.txt".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
