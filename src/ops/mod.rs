//! Operations of the bot itself: who the owner is, maintenance mode, provider health, usage counters and backups.

pub mod backup;
pub mod health;
pub mod stats;

use sqlx::SqlitePool;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};

/// Owner and maintenance state, shared by every command.
pub struct Ops {
    pool: SqlitePool,
    /// The Discord application owner (or `OWNER_ID`); `0` while unknown.
    owner: AtomicU64,
    maintenance: AtomicBool,
    /// The SQLite file, to find the backups folder next to it.
    pub db_path: PathBuf,
}

/// May this person run commands? Everyone does unless maintenance is on; the owner always does.
pub fn maintenance_allows(maintenance: bool, is_owner: bool) -> bool {
    !maintenance || is_owner
}

impl Ops {
    /// Load the persisted maintenance flag. `owner_env` is `OWNER_ID` when set.
    pub async fn load(pool: SqlitePool, owner_env: Option<u64>, db_path: PathBuf) -> Arc<Self> {
        let on = sqlx::query_scalar::<_, String>(
            "SELECT value FROM bot_state WHERE key = 'maintenance'",
        )
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .is_some_and(|v| v == "1");
        Arc::new(Self {
            pool,
            owner: AtomicU64::new(owner_env.unwrap_or(0)),
            maintenance: AtomicBool::new(on),
            db_path,
        })
    }

    /// Set the owner found from the application info (an explicit `OWNER_ID` is never overridden).
    pub fn set_owner_if_unknown(&self, id: u64) {
        let _ = self
            .owner
            .compare_exchange(0, id, Ordering::Relaxed, Ordering::Relaxed);
    }

    pub fn owner(&self) -> Option<u64> {
        Some(self.owner.load(Ordering::Relaxed)).filter(|o| *o != 0)
    }

    pub fn is_owner(&self, user: u64) -> bool {
        self.owner() == Some(user)
    }

    pub fn maintenance(&self) -> bool {
        self.maintenance.load(Ordering::Relaxed)
    }

    /// Switch maintenance mode on or off; it survives restarts.
    pub async fn set_maintenance(&self, on: bool) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO bot_state (key, value) VALUES ('maintenance', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
            .bind(if on { "1" } else { "0" })
            .execute(&self.pool)
            .await?;
        self.maintenance.store(on, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn ops() -> Arc<Ops> {
        let pool = crate::db::connect_memory().await;
        Ops::load(pool, None, PathBuf::from("bot.db")).await
    }

    #[test]
    fn the_gate_lets_everyone_in_unless_maintenance_is_on_and_always_the_owner() {
        assert!(maintenance_allows(false, false) && maintenance_allows(false, true));
        assert!(!maintenance_allows(true, false));
        assert!(
            maintenance_allows(true, true),
            "the owner bypasses maintenance"
        );
    }

    #[tokio::test]
    async fn maintenance_is_persisted_across_restarts() {
        let o = ops().await;
        assert!(!o.maintenance());
        o.set_maintenance(true).await.expect("on");
        assert!(o.maintenance());
        // A "restart": a new Ops over the same database reads the stored flag.
        let again = Ops::load(o.pool.clone(), None, PathBuf::from("bot.db")).await;
        assert!(again.maintenance());
        again.set_maintenance(false).await.expect("off");
        assert!(!Ops::load(o.pool.clone(), None, PathBuf::from("bot.db"))
            .await
            .maintenance());
    }

    #[tokio::test]
    async fn the_owner_comes_from_the_environment_or_the_application_and_the_environment_wins() {
        let o = ops().await;
        assert_eq!(o.owner(), None);
        assert!(!o.is_owner(5));
        o.set_owner_if_unknown(5);
        assert!(o.is_owner(5) && !o.is_owner(6));
        o.set_owner_if_unknown(6);
        assert_eq!(o.owner(), Some(5), "the first known owner stays");
        let pool = crate::db::connect_memory().await;
        let fixed = Ops::load(pool, Some(9), PathBuf::from("x.db")).await;
        fixed.set_owner_if_unknown(5);
        assert_eq!(
            fixed.owner(),
            Some(9),
            "OWNER_ID is never overridden by the application info"
        );
    }
}
