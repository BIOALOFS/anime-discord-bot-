pub mod admin;
pub mod chapters;
pub mod custom;
pub mod feed;
pub mod game;
pub mod notify;
pub mod profile;
pub mod reminders;
pub mod status;
pub mod suggest;
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
    if url.contains(":memory:") {
        migrator.run(&pool).await?;
        return Ok(pool);
    }
    // The backup is the safety net of the migration: without a verified one, nothing is migrated.
    let backup = backup_if_pending(&pool, &path, &migrator)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "cannot back up the database before migrating, so nothing was changed \
                 (the old version of the bot keeps working with it): {e:#}"
            )
        })?;
    let Some(backup) = backup else {
        migrator.run(&pool).await?;
        return Ok(pool);
    };
    tracing::info!(backup = %backup.display(), "database backed up and verified before migrating");
    let before = user_data_counts(&pool).await?;
    migrator.run(&pool).await?;
    verify_migrated(&pool, &path, &backup, &before).await?;
    Ok(pool)
}

/// After a migration: no user-data table may have fewer rows than before. If one has, the pool is closed, the
/// backup is put back and an error says so; the bot then refuses to start on a database that lost anything.
pub async fn verify_migrated(
    pool: &SqlitePool,
    db: &std::path::Path,
    backup: &std::path::Path,
    before: &std::collections::BTreeMap<String, i64>,
) -> anyhow::Result<CountReport> {
    let after = user_data_counts(pool).await?;
    let report = compare_counts(before, &after);
    if let Some(lost) = report.dropped() {
        tracing::error!(
            ?lost,
            "row counts dropped during migration, restoring the backup"
        );
        pool.close().await;
        restore_backup(backup, db)?;
        anyhow::bail!(
            "migration lost data in {lost:?}; the backup {} was restored, nothing was changed",
            backup.display()
        );
    }
    tracing::info!(
        tables = report.rows.len(),
        before = report.rows.iter().map(|r| r.1).sum::<i64>(),
        after = report.rows.iter().map(|r| r.2).sum::<i64>(),
        backup = %backup.display(),
        "migration verified: no user-data row count dropped"
    );
    Ok(report)
}

/// Tables that only cache data from elsewhere; they are rebuilt on demand and not user data.
const CACHE_TABLES: &[&str] = &["character_cache", "media_cache", "wiki_cache"];

/// Row counts of every user-data table (every table but the caches and the migration bookkeeping).
pub async fn user_data_counts(
    pool: &SqlitePool,
) -> anyhow::Result<std::collections::BTreeMap<String, i64>> {
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite%' AND substr(name, 1, 5) <> '_sqlx'",
    )
    .fetch_all(pool)
    .await?;
    let mut out = std::collections::BTreeMap::new();
    for name in names
        .into_iter()
        .filter(|n| !CACHE_TABLES.contains(&n.as_str()) && !n.starts_with("wiki_fts"))
    {
        // Table names come from sqlite_master, never from users.
        let n: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM \"{}\"",
            name.replace('"', "\"\"")
        ))
        .fetch_one(pool)
        .await?;
        out.insert(name, n);
    }
    Ok(out)
}

/// `(table, rows before, rows after)` for every table present both before and after (a table an old migration
/// dropped on purpose is not a loss of this one).
#[derive(Debug, PartialEq, Eq)]
pub struct CountReport {
    pub rows: Vec<(String, i64, i64)>,
}

impl CountReport {
    /// The tables whose count went down, if any.
    pub fn dropped(&self) -> Option<Vec<String>> {
        let lost: Vec<String> = self
            .rows
            .iter()
            .filter(|(_, b, a)| a < b)
            .map(|(t, ..)| t.clone())
            .collect();
        (!lost.is_empty()).then_some(lost)
    }
}

pub fn compare_counts(
    before: &std::collections::BTreeMap<String, i64>,
    after: &std::collections::BTreeMap<String, i64>,
) -> CountReport {
    CountReport {
        rows: before
            .iter()
            .filter_map(|(t, b)| after.get(t).map(|a| (t.clone(), *b, *a)))
            .collect(),
    }
}

/// Put a backup back in place of the database (the pool must be closed first). The copy is written next to the database
/// and renamed over it, so the database file becomes a new file in one step: nothing that still holds the old one open
/// can write into the restored data, and a crash leaves either the old or the complete new file.
pub fn restore_backup(backup: &std::path::Path, db: &std::path::Path) -> anyhow::Result<()> {
    let mut staged = db.as_os_str().to_owned();
    staged.push(".restoring");
    let staged = std::path::PathBuf::from(staged);
    std::fs::copy(backup, &staged).map_err(|e| {
        anyhow::anyhow!(
            "could not copy {} next to {}: {e}",
            backup.display(),
            db.display()
        )
    })?;
    // Leftover write-ahead files belong to the data that is being replaced.
    for suffix in ["-wal", "-shm"] {
        let mut side = db.as_os_str().to_owned();
        side.push(suffix);
        let _ = std::fs::remove_file(std::path::PathBuf::from(side));
    }
    std::fs::rename(&staged, db).map_err(|e| {
        anyhow::anyhow!(
            "could not restore {} over {}: {e}",
            backup.display(),
            db.display()
        )
    })?;
    Ok(())
}

/// Open a backup read-only and prove it is usable: `PRAGMA integrity_check` says ok and the key tables can be read.
pub async fn verify_backup(path: &std::path::Path) -> anyhow::Result<()> {
    let opts = SqliteConnectOptions::new().filename(path).read_only(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await?;
    let verdict: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await?;
    if verdict != "ok" {
        anyhow::bail!("backup failed its integrity check: {verdict}");
    }
    for table in ["users", "profiles", "profile_characters"] {
        sqlx::query_scalar::<_, i64>(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&pool)
            .await
            .map_err(|e| anyhow::anyhow!("backup table {table} is not readable: {e}"))?;
    }
    pool.close().await;
    Ok(())
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
    verify_backup(&target).await.inspect_err(|_| {
        let _ = std::fs::remove_file(&target);
    })?;
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
    use super::*;
    use crate::db::{custom, profile as pdb};
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

    /// The real upgrade path: a database at migration 0015 with member data is migrated to the newest version;
    /// nothing is lost and the new columns get their defaults (old reminders get their provider key).
    #[tokio::test]
    async fn a_database_from_round_six_upgrades_without_losing_data() {
        let dir = temp_dir("upgrade");
        let url = format!("sqlite://{}?mode=rwc", dir.join("bot.db").display());
        let pool = sqlx::SqlitePool::connect(&url).await.expect("open");
        let mut old = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
            .await
            .expect("read migrations");
        let before: Vec<_> = old
            .migrations
            .iter()
            .filter(|m| m.version <= 15)
            .cloned()
            .collect();
        assert!(
            old.migrations.len() > before.len(),
            "there is a newer migration than 0015"
        );
        old.migrations = std::borrow::Cow::Owned(before);
        old.run(&pool).await.expect("migrate to 0015");
        sqlx::query("INSERT INTO profiles (user_id, bio) VALUES (7, 'hello')")
            .execute(&pool)
            .await
            .expect("profile");
        sqlx::query(
            "INSERT INTO profile_characters (user_id, character_id, position) VALUES (7, 1, 1)",
        )
        .execute(&pool)
        .await
        .expect("character");
        sqlx::query("INSERT INTO profile_works (user_id, work_id, kind, position) VALUES (7, 'anilist:5', 'anime', 1)").execute(&pool).await.expect("work");
        sqlx::query(
            "INSERT INTO reminders (user_id, work_id, title) VALUES (7, 77, 'Old reminder')",
        )
        .execute(&pool)
        .await
        .expect("reminder");
        sqlx::query("INSERT INTO guild_settings (guild_id) VALUES (9)")
            .execute(&pool)
            .await
            .expect("guild");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate to the newest");

        let (bio, chars, works): (String, i64, i64) = (
            sqlx::query_scalar("SELECT bio FROM profiles WHERE user_id = 7")
                .fetch_one(&pool)
                .await
                .expect("bio"),
            sqlx::query_scalar("SELECT count(*) FROM profile_characters WHERE user_id = 7")
                .fetch_one(&pool)
                .await
                .expect("c"),
            sqlx::query_scalar("SELECT count(*) FROM profile_works WHERE user_id = 7")
                .fetch_one(&pool)
                .await
                .expect("w"),
        );
        assert_eq!((bio.as_str(), chars, works), ("hello", 1, 1));
        let (key, category, days): (String, String, i64) = sqlx::query_as(
            "SELECT work_key, category, notify_days FROM reminders WHERE work_id = 77",
        )
        .fetch_one(&pool)
        .await
        .expect("reminder");
        assert_eq!(
            (key.as_str(), category.as_str(), days),
            ("anilist:77", "anime", 1),
            "old reminders keep working and gain their key"
        );
        let (tz, spoiler): (String, i64) =
            sqlx::query_as("SELECT timezone, spoiler_guard FROM guild_settings WHERE guild_id = 9")
                .fetch_one(&pool)
                .await
                .expect("guild");
        assert_eq!((tz.as_str(), spoiler), ("Asia/Muscat", 0));
        // The new tables exist and accept TMDB keys next to AniList ones.
        sqlx::query("INSERT INTO profile_works (user_id, work_id, kind, position) VALUES (7, 'tmdb-tv:1396', 'tmdb-tv', 2)").execute(&pool).await.expect("tmdb work");
        sqlx::query("INSERT INTO profile_characters (user_id, character_id, position) VALUES (7, 3000000000001, 2)").execute(&pool).await.expect("tmdb cast id");
        sqlx::query("INSERT INTO item_status (user_id, work_key, status) VALUES (7, 'tmdb-tv:1396', 'current')").execute(&pool).await.expect("status");
        drop(pool);
        let _ = std::fs::remove_dir_all(&dir);
    }

    async fn file_db(tag: &str) -> (std::path::PathBuf, SqlitePool) {
        let dir = temp_dir(tag);
        let path = dir.join("bot.db");
        let pool = super::connect(&format!("sqlite://{}", path.display()))
            .await
            .expect("connect");
        (path, pool)
    }

    async fn put_user_data(pool: &SqlitePool) {
        for sql in [
            "INSERT INTO users (discord_id) VALUES (1), (2)",
            "INSERT INTO profiles (user_id, bio) VALUES (1, 'a'), (2, 'b')",
            "INSERT INTO profile_characters (user_id, character_id, position) VALUES (1, 10, 1), (1, 11, 2), (2, 10, 1)",
            "INSERT INTO reminders (user_id, work_id, title) VALUES (1, 7, 'x')",
        ] {
            sqlx::query(sql).execute(pool).await.expect(sql);
        }
    }

    #[tokio::test]
    async fn the_post_migration_check_restores_the_backup_when_a_count_drops() {
        let (path, pool) = file_db("restore").await;
        put_user_data(&pool).await;
        let backup = path.with_file_name("backup.db");
        let literal = backup.to_string_lossy().replace('\'', "''");
        sqlx::query(&format!("VACUUM INTO '{literal}'"))
            .execute(&pool)
            .await
            .expect("backup");
        super::verify_backup(&backup)
            .await
            .expect("a good backup verifies");
        let before = super::user_data_counts(&pool).await.expect("counts");
        assert_eq!(before["profile_characters"], 3);
        // an honest migration passes
        let ok = super::verify_migrated(&pool, &path, &backup, &before)
            .await
            .expect("nothing lost");
        assert!(ok.dropped().is_none());
        // a migration that lost a row is undone
        sqlx::query("DELETE FROM profile_characters WHERE user_id = 1 AND character_id = 11")
            .execute(&pool)
            .await
            .expect("simulate the loss");
        let err = super::verify_migrated(&pool, &path, &backup, &before)
            .await
            .expect_err("a drop is an error");
        assert!(err.to_string().contains("profile_characters"), "{err}");
        let reopened = super::connect(&format!("sqlite://{}", path.display()))
            .await
            .expect("reopen");
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM profile_characters")
            .fetch_one(&reopened)
            .await
            .expect("count");
        assert_eq!(n, 3, "the backup was put back");
    }

    #[tokio::test]
    async fn a_corrupt_backup_is_not_trusted() {
        let dir = temp_dir("badbackup");
        let bad = dir.join("bad.db");
        std::fs::write(&bad, b"this is not a database at all, just bytes").expect("write");
        assert!(super::verify_backup(&bad).await.is_err());
    }

    #[test]
    fn counts_only_compare_tables_present_on_both_sides() {
        use std::collections::BTreeMap;
        let b: BTreeMap<String, i64> =
            [("a".into(), 3), ("gone".into(), 9), ("c".into(), 1)].into();
        let a: BTreeMap<String, i64> = [("a".into(), 3), ("c".into(), 2), ("new".into(), 0)].into();
        let r = super::compare_counts(&b, &a);
        assert!(
            r.dropped().is_none(),
            "growth and a dropped legacy table are fine"
        );
        let a: BTreeMap<String, i64> = [("a".into(), 2), ("c".into(), 2)].into();
        assert_eq!(
            super::compare_counts(&b, &a).dropped(),
            Some(vec!["a".to_string()])
        );
    }

    #[test]
    fn new_migrations_never_delete_or_drop_user_data() {
        // 0011 and 0012 (earlier rounds, already deployed) dropped retired structures; everything after is additive.
        let mut checked = 0;
        for entry in std::fs::read_dir("migrations").expect("dir") {
            let path = entry.expect("entry").path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let number: u32 = name.get(..4).and_then(|n| n.parse().ok()).unwrap_or(0);
            if number < 13 {
                continue;
            }
            let sql = std::fs::read_to_string(&path).expect("read").to_lowercase();
            for bad in ["drop table", "drop column", "delete from", "truncate"] {
                assert!(
                    !sql.contains(bad),
                    "{name} contains `{bad}`: migrations may only add"
                );
            }
            checked += 1;
        }
        assert!(checked >= 6);
    }

    #[tokio::test]
    async fn editing_and_resetting_one_member_never_touches_another() {
        let pool = connect_memory().await;
        for u in [1i64, 2] {
            pdb::create_profile(&pool, u).await.expect("profile");
            for c in [10i64, 11] {
                pdb::add_character(&pool, u, c).await.expect("add");
            }
        }
        let img = custom::NewImage {
            hash: "h".into(),
            mime: "image/png".into(),
            bytes: vec![1, 2, 3],
            width: 1,
            height: 1,
            animated: false,
        };
        let mut ids = Vec::new();
        for u in [1i64, 2] {
            match custom::store_image(&pool, u, &img, 30)
                .await
                .expect("store")
            {
                custom::StoreOutcome::Stored(id) | custom::StoreOutcome::Existing(id) => {
                    ids.push(id)
                }
                other => panic!("{other:?}"),
            }
            custom::set_character_image(&pool, u, 10, ids[ids.len() - 1])
                .await
                .expect("set");
        }
        let snapshot = async |p: &SqlitePool| {
            let rows: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM profile_characters WHERE user_id = 2 AND custom_image_id IS NOT NULL",
            )
            .fetch_one(p)
            .await
            .expect("count");
            let imgs = custom::image_count(p, 2).await.expect("images");
            let items = pdb::list_page(p, 2, None, 0, 100)
                .await
                .expect("list")
                .len();
            (rows, imgs, items)
        };
        let before = snapshot(&pool).await;
        custom::reset_character(&pool, 1, 10)
            .await
            .expect("reset character");
        pdb::reset_profile(&pool, 1).await.expect("reset profile");
        assert_eq!(
            snapshot(&pool).await,
            before,
            "member 2 is untouched by member 1's edits"
        );
        pdb::delete_user_data(&pool, 1)
            .await
            .expect("delete member 1");
        assert_eq!(
            snapshot(&pool).await,
            before,
            "and by member 1 deleting their data"
        );
        assert_eq!(
            pdb::list_page(&pool, 1, None, 0, 10)
                .await
                .expect("list")
                .len(),
            0
        );
    }

    /// The round 10 upgrade (0018 -> 0019) on a database with realistic member data, through the real startup path:
    /// verified backup first, migration, row-count check. Nothing existing changes and the new table is there.
    #[tokio::test]
    async fn the_round_ten_upgrade_keeps_every_row_and_file_reference() {
        let dir = temp_dir("round10");
        let path = dir.join("bot.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let pool = sqlx::SqlitePool::connect(&url).await.expect("open");
        let mut old = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
            .await
            .expect("read migrations");
        let before: Vec<_> = old
            .migrations
            .iter()
            .filter(|m| m.version <= 18)
            .cloned()
            .collect();
        assert!(
            old.migrations.len() > before.len(),
            "0019 is the migration under test"
        );
        old.migrations = std::borrow::Cow::Owned(before);
        old.run(&pool).await.expect("migrate to 0018");
        let person = crate::people::encode_person_id("tmdb-person:17419").expect("person id");
        for sql in [
            "INSERT INTO users (discord_id) VALUES (7), (8)",
            "INSERT INTO profiles (user_id, bio, banner) VALUES (7, 'hello', 'img:1'), (8, 'other', NULL)",
            "INSERT INTO custom_images (user_id, hash, mime, bytes, width, height, animated) VALUES (7, 'abc', 'image/gif', x'47494638', 10, 10, 1)",
            "INSERT INTO profile_characters (user_id, character_id, position, custom_image_id, custom_title) VALUES (7, 1, 1, 1, 'Nick'), (8, 2, 1, NULL, NULL)",
            "INSERT INTO character_cache (character_id, name_romaji, is_safe, fetched_at) VALUES (1, 'Rem', 1, 0)",
            "INSERT INTO profile_works (user_id, work_id, kind, position) VALUES (7, 'anilist:5', 'anime', 1)",
            "INSERT INTO reminders (user_id, work_id, title) VALUES (7, 77, 'Reminder')",
            "INSERT INTO suggestions (guild_id, author_id, first_id, second_id, first_title, second_title, pair_key, reason) VALUES (9, 7, 'anilist:1', 'anilist:2', 'A', 'B', 'anilist:1|anilist:2', 'because')",
            "INSERT INTO guild_settings (guild_id) VALUES (9)",
        ] {
            sqlx::query(sql).execute(&pool).await.expect(sql);
        }
        sqlx::query(
            "INSERT INTO profile_characters (user_id, character_id, position) VALUES (7, ?1, 2)",
        )
        .bind(person)
        .execute(&pool)
        .await
        .expect("a person item");
        let counts_before = super::user_data_counts(&pool).await.expect("counts");
        pool.close().await;

        // the real startup path: backup, verify, migrate, compare
        let pool = super::connect(&url).await.expect("the upgrade runs");
        let counts_after = super::user_data_counts(&pool).await.expect("counts");
        for (table, n) in &counts_before {
            assert_eq!(counts_after.get(table), Some(n), "{table} keeps its rows");
        }
        assert!(
            counts_after.contains_key("message_sessions"),
            "the new table exists"
        );
        assert_eq!(counts_after["message_sessions"], 0);
        let (bytes, animated): (Vec<u8>, bool) =
            sqlx::query_as("SELECT bytes, animated FROM custom_images WHERE id = 1")
                .fetch_one(&pool)
                .await
                .expect("image");
        assert_eq!(
            (bytes, animated),
            (vec![0x47, 0x49, 0x46, 0x38], true),
            "the custom picture is untouched"
        );
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT character_id FROM profile_characters WHERE user_id = 7 ORDER BY position",
        )
        .fetch_all(&pool)
        .await
        .expect("ids");
        assert_eq!(
            ids,
            vec![1, person],
            "the person item and its position are unchanged"
        );
        let nick: String = sqlx::query_scalar(
            "SELECT custom_title FROM profile_characters WHERE user_id = 7 AND character_id = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("title");
        assert_eq!(nick, "Nick");
        // the backup was written before the migration and verifies
        let backups: Vec<_> = std::fs::read_dir(dir.join("backups"))
            .expect("backups")
            .flatten()
            .collect();
        assert_eq!(backups.len(), 1);
        super::verify_backup(&backups[0].path())
            .await
            .expect("the backup is good");
    }

    /// The round 11 upgrade (0019 -> 0020, the wiki tables) on a database with member data, through the real startup
    /// path: verified backup, migration, row counts compared. Nothing existing changes.
    #[tokio::test]
    async fn the_wiki_migration_is_additive_and_keeps_member_data() {
        let dir = temp_dir("round11");
        let path = dir.join("bot.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let pool = sqlx::SqlitePool::connect(&url).await.expect("open");
        let mut old = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
            .await
            .expect("read migrations");
        let before: Vec<_> = old
            .migrations
            .iter()
            .filter(|m| m.version <= 19)
            .cloned()
            .collect();
        assert!(
            old.migrations.len() > before.len(),
            "0020 is the migration under test"
        );
        old.migrations = std::borrow::Cow::Owned(before);
        old.run(&pool).await.expect("migrate to 0019");
        for sql in [
            "INSERT INTO users (discord_id) VALUES (7)",
            "INSERT INTO profiles (user_id, bio) VALUES (7, 'hello')",
            "INSERT INTO profile_characters (user_id, character_id, position) VALUES (7, 1, 1)",
            "INSERT INTO reminders (user_id, work_id, title) VALUES (7, 77, 'Reminder')",
            "INSERT INTO guild_settings (guild_id) VALUES (9)",
            "INSERT INTO message_sessions (message_id, channel_id, started_at, expires_at, hard_expires_at) VALUES (1, 2, 3, 4, 5)",
        ] {
            sqlx::query(sql).execute(&pool).await.expect(sql);
        }
        let counts_before = super::user_data_counts(&pool).await.expect("counts");
        pool.close().await;
        let pool = super::connect(&url).await.expect("the upgrade runs");
        let counts_after = super::user_data_counts(&pool).await.expect("counts");
        for (table, n) in &counts_before {
            assert_eq!(counts_after.get(table), Some(n), "{table} keeps its rows");
        }
        for table in ["wiki_sources", "wiki_cache"] {
            let n: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .expect(table);
            assert_eq!(n, 0, "{table} starts empty");
        }
        // the full-text index works on the migrated database
        sqlx::query("INSERT INTO wiki_cache (wiki_id, page_id, title, normalized_key, fetched_at) VALUES (1, 1, 'Pyramid Head', 'pyramid head', 1)")
            .execute(&pool)
            .await
            .expect("insert");
        let hits: i64 =
            sqlx::query_scalar("SELECT count(*) FROM wiki_fts WHERE wiki_fts MATCH '\"pyram\"*'")
                .fetch_one(&pool)
                .await
                .expect("fts");
        assert_eq!(hits, 1);
        let wikis_on: bool = crate::wiki::store::global_enabled(&pool).await;
        assert!(wikis_on, "the kill switch is on by default");
        let backups: Vec<_> = std::fs::read_dir(dir.join("backups"))
            .expect("backups")
            .flatten()
            .collect();
        assert_eq!(backups.len(), 1);
        super::verify_backup(&backups[0].path())
            .await
            .expect("the backup is good");
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
