//! Backups of the SQLite file: a consistent copy with `VACUUM INTO`, the newest seven kept, and a guard that skips the
//! copy (with a warning) when the volume is nearly full.

use super::Ops;
use std::path::{Path, PathBuf};

pub const KEPT: usize = crate::db::BACKUPS_KEPT;
/// Largest backup sent as a Discord attachment.
pub const MAX_ATTACH_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Made {
    pub path: PathBuf,
    pub size: u64,
}

/// Is there room for a copy? It needs three times the database plus a 50 MB margin.
pub fn space_ok(available: u64, db_size: u64) -> bool {
    available >= db_size.saturating_mul(3).saturating_add(50 * 1024 * 1024)
}

#[derive(Debug, thiserror::Error)]
pub enum Skipped {
    #[error("not enough free space for a backup ({available} bytes free, database {db} bytes)")]
    LowSpace { available: u64, db: u64 },
    #[error("backup failed: {0}")]
    Failed(String),
}

fn dir_of(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .join("backups")
}

fn stem_of(db_path: &Path) -> String {
    db_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("bot")
        .to_string()
}

/// Make a backup now and rotate. Guarded against a full volume.
pub async fn make(pool: &sqlx::SqlitePool, db_path: &Path) -> Result<Made, Skipped> {
    let dir = dir_of(db_path);
    std::fs::create_dir_all(&dir).map_err(|e| Skipped::Failed(e.to_string()))?;
    let db_size = std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);
    let available = fs2::available_space(&dir).unwrap_or(u64::MAX);
    if !space_ok(available, db_size) {
        tracing::warn!(
            available,
            db_size,
            "backup skipped: the volume is nearly full"
        );
        return Err(Skipped::LowSpace {
            available,
            db: db_size,
        });
    }
    let target = dir.join(format!(
        "{}-{}.db",
        stem_of(db_path),
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    ));
    let literal = target.to_string_lossy().replace('\'', "''");
    sqlx::query(&format!("VACUUM INTO '{literal}'"))
        .execute(pool)
        .await
        .map_err(|e| Skipped::Failed(e.to_string()))?;
    crate::db::prune_backups(&dir, &stem_of(db_path), KEPT)
        .map_err(|e| Skipped::Failed(e.to_string()))?;
    let size = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
    Ok(Made { path: target, size })
}

/// Is the newest backup older than a day (or missing)?
pub fn daily_due(db_path: &Path, now: i64) -> bool {
    let dir = dir_of(db_path);
    let stem = stem_of(db_path);
    let newest = std::fs::read_dir(&dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with(&format!("{stem}-")) && n.ends_with(".db"))
        })
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .max();
    newest.is_none_or(|t| now - t >= 23 * 3600)
}

/// The daily job: back up when due. A skipped or failed backup is logged and never stops the bot.
pub async fn daily(ops: &Ops, pool: &sqlx::SqlitePool, now: i64) {
    if !daily_due(&ops.db_path, now) {
        return;
    }
    match make(pool, &ops.db_path).await {
        Ok(m) => tracing::info!(path = %m.path.display(), size = m.size, "daily backup done"),
        Err(e) => tracing::warn!("daily backup: {e}"),
    }
}

/// Human size, e.g. `1.4 MB`.
pub fn size_text(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn file_db(dir: &Path) -> (sqlx::SqlitePool, PathBuf) {
        let path = dir.join("bot.db");
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!("sqlite://{}?mode=rwc", path.display()))
            .await
            .expect("db");
        sqlx::query("CREATE TABLE t (n INTEGER)")
            .execute(&pool)
            .await
            .expect("table");
        sqlx::query("INSERT INTO t VALUES (42)")
            .execute(&pool)
            .await
            .expect("row");
        (pool, path)
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "anpot-bk-{tag}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&d).expect("dir");
        d
    }

    #[test]
    fn the_low_space_guard_needs_three_times_the_database_and_a_margin() {
        let mb = 1024 * 1024;
        assert!(space_ok(500 * mb, 10 * mb));
        assert!(
            !space_ok(60 * mb, 10 * mb),
            "60 MB free is below 3 x 10 MB + 50 MB"
        );
        assert!(space_ok(80 * mb, 10 * mb));
        assert!(!space_ok(0, 0));
        assert!(space_ok(50 * mb, 0));
    }

    #[tokio::test]
    async fn a_backup_is_a_consistent_copy_and_only_the_newest_seven_survive() {
        let dir = temp("rotate");
        let (pool, path) = file_db(&dir).await;
        let backups = dir.join("backups");
        std::fs::create_dir_all(&backups).expect("dir");
        // Nine old backups already exist.
        for i in 0..9 {
            std::fs::write(backups.join(format!("bot-20260101-0000{i:02}.db")), b"old")
                .expect("old");
        }
        let made = make(&pool, &path).await.expect("backup");
        assert!(made.path.exists() && made.size > 0);
        let copy = SqlitePoolOptions::new()
            .connect(&format!("sqlite://{}", made.path.display()))
            .await
            .expect("open copy");
        let n: i64 = sqlx::query_scalar("SELECT n FROM t")
            .fetch_one(&copy)
            .await
            .expect("row");
        assert_eq!(n, 42, "the copy holds the data");
        let count = std::fs::read_dir(&backups).expect("dir").count();
        assert_eq!(count, KEPT, "rotation keeps seven");
        assert!(made.path.exists(), "the newest is among them");
        drop((copy, pool));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_daily_job_runs_only_when_the_newest_backup_is_a_day_old() {
        let dir = temp("daily");
        let (pool, path) = file_db(&dir).await;
        let now = chrono::Utc::now().timestamp();
        assert!(daily_due(&path, now), "no backup yet");
        make(&pool, &path).await.expect("backup");
        assert!(!daily_due(&path, now), "a fresh backup exists");
        assert!(
            daily_due(&path, now + 24 * 3600),
            "a day later it is due again"
        );
        drop(pool);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sizes_print_readably() {
        assert_eq!(size_text(1536 * 1024), "1.5 MB");
        assert_eq!(size_text(20 * 1024), "20 KB");
    }
}
