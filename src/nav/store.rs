//! The `message_sessions` table: one row per public interactive message, so components can be disabled on time even
//! after a restart. Interaction tokens are never stored; ephemeral messages live in memory only (`sweeper::Ephemerals`).

use super::{Class, Lifetimes};
use sqlx::SqlitePool;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct SessionRow {
    pub message_id: i64,
    pub channel_id: i64,
    pub guild_id: Option<i64>,
    pub owner_id: Option<i64>,
    pub kind: String,
    pub timeout_class: String,
    pub started_at: i64,
    pub expires_at: i64,
    pub hard_expires_at: i64,
    pub state_json: String,
    pub components_json: String,
    pub v2: bool,
    pub ephemeral: bool,
}

/// What a new session needs.
pub struct NewSession<'a> {
    pub message_id: i64,
    pub channel_id: i64,
    pub guild_id: Option<i64>,
    pub owner_id: Option<i64>,
    pub kind: &'a str,
    pub class: Class,
    pub components_json: &'a str,
    pub v2: bool,
}

const COLUMNS: &str =
    "message_id, channel_id, guild_id, owner_id, kind, timeout_class, started_at, expires_at, \
    hard_expires_at, state_json, components_json, v2, ephemeral";

/// Register (or re-register) a message's session; its lifetime starts at `now`.
pub async fn register(
    pool: &SqlitePool,
    s: &NewSession<'_>,
    l: &Lifetimes,
    now: i64,
) -> Result<(), sqlx::Error> {
    let (expires, hard) = l.first_expiry(s.class, now);
    sqlx::query(
        "INSERT INTO message_sessions (message_id, channel_id, guild_id, owner_id, kind, timeout_class, started_at, \
         expires_at, hard_expires_at, components_json, v2) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
         ON CONFLICT(message_id) DO UPDATE SET expires_at = excluded.expires_at, hard_expires_at = excluded.hard_expires_at, \
         components_json = excluded.components_json, v2 = excluded.v2",
    )
    .bind(s.message_id)
    .bind(s.channel_id)
    .bind(s.guild_id)
    .bind(s.owner_id)
    .bind(s.kind)
    .bind(s.class.key())
    .bind(now)
    .bind(expires)
    .bind(hard)
    .bind(s.components_json)
    .bind(s.v2)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get(pool: &SqlitePool, message_id: i64) -> Result<Option<SessionRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM message_sessions WHERE message_id = ?1"
    ))
    .bind(message_id)
    .fetch_optional(pool)
    .await
}

/// A press: slide the expiry (never past the hard cap, never for a notification) and remember what is on screen now.
/// Atomic in one statement, so a press and the sweeper cannot interleave badly.
pub async fn touch(
    pool: &SqlitePool,
    message_id: i64,
    l: &Lifetimes,
    now: i64,
    state_json: &str,
    components_json: &str,
    v2: bool,
) -> Result<(), sqlx::Error> {
    let (default_idle, _) = l.of(Class::Default);
    let (profile_idle, _) = l.of(Class::Profile);
    sqlx::query(
        "UPDATE message_sessions SET state_json = ?2, components_json = ?3, v2 = ?4, \
         expires_at = CASE timeout_class \
            WHEN 'notification' THEN expires_at \
            WHEN 'profile' THEN MAX(expires_at, MIN(hard_expires_at, ?1 + ?5)) \
            ELSE MAX(expires_at, MIN(hard_expires_at, ?1 + ?6)) END \
         WHERE message_id = ?7",
    )
    .bind(now)
    .bind(state_json)
    .bind(components_json)
    .bind(v2)
    .bind(profile_idle as i64)
    .bind(default_idle as i64)
    .bind(message_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Sessions whose time is up, oldest first.
pub async fn due(pool: &SqlitePool, now: i64, limit: i64) -> Result<Vec<SessionRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM message_sessions WHERE expires_at <= ?1 ORDER BY expires_at LIMIT ?2"
    ))
    .bind(now)
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn remove(pool: &SqlitePool, message_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM message_sessions WHERE message_id = ?1")
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(id: i64, class: Class) -> NewSession<'static> {
        NewSession {
            message_id: id,
            channel_id: 9,
            guild_id: Some(1),
            owner_id: Some(5),
            kind: "view",
            class,
            components_json: r#"[{"type":1,"components":[]}]"#,
            v2: false,
        }
    }

    #[tokio::test]
    async fn a_session_is_registered_with_its_class_lifetime() {
        let pool = crate::db::connect_memory().await;
        let l = Lifetimes::default();
        register(&pool, &new(1, Class::Default), &l, 1000)
            .await
            .expect("register");
        register(&pool, &new(2, Class::Profile), &l, 1000)
            .await
            .expect("register");
        register(&pool, &new(3, Class::Notification), &l, 1000)
            .await
            .expect("register");
        let a = get(&pool, 1).await.expect("get").expect("row");
        assert_eq!((a.expires_at, a.hard_expires_at), (1060, 1600));
        assert_eq!(a.timeout_class, "default");
        let b = get(&pool, 2).await.expect("get").expect("row");
        assert_eq!((b.expires_at, b.hard_expires_at), (1300, 1900));
        let c = get(&pool, 3).await.expect("get").expect("row");
        assert_eq!((c.expires_at, c.hard_expires_at), (4600, 4600));
        assert_eq!(a.owner_id, Some(5));
    }

    #[tokio::test]
    async fn a_press_extends_atomically_within_the_cap_and_never_a_notification() {
        let pool = crate::db::connect_memory().await;
        let l = Lifetimes::default();
        register(&pool, &new(1, Class::Default), &l, 1000)
            .await
            .expect("register");
        register(&pool, &new(3, Class::Notification), &l, 1000)
            .await
            .expect("register");
        touch(&pool, 1, &l, 1050, "[]", "[]", false)
            .await
            .expect("touch");
        assert_eq!(
            get(&pool, 1).await.expect("get").expect("row").expires_at,
            1110
        );
        touch(&pool, 1, &l, 1590, "[]", "[]", false)
            .await
            .expect("touch");
        assert_eq!(
            get(&pool, 1).await.expect("get").expect("row").expires_at,
            1600,
            "the hard cap"
        );
        touch(&pool, 3, &l, 2000, "[]", "[]", false)
            .await
            .expect("touch");
        assert_eq!(
            get(&pool, 3).await.expect("get").expect("row").expires_at,
            4600,
            "fixed"
        );
        // a press never shortens a session
        touch(&pool, 1, &l, 10, "[]", "[]", false)
            .await
            .expect("touch");
        assert!(get(&pool, 1).await.expect("get").expect("row").expires_at >= 1600);
    }

    #[tokio::test]
    async fn due_sessions_come_oldest_first_and_removal_works() {
        let pool = crate::db::connect_memory().await;
        let l = Lifetimes::default();
        for (id, at) in [(1, 300), (2, 100), (3, 200)] {
            register(&pool, &new(id, Class::Default), &l, at)
                .await
                .expect("register");
        }
        let due_now = due(&pool, 270, 10).await.expect("due");
        assert_eq!(
            due_now.iter().map(|r| r.message_id).collect::<Vec<_>>(),
            vec![2, 3]
        );
        remove(&pool, 2).await.expect("remove");
        assert!(get(&pool, 2).await.expect("get").is_none());
    }

    #[tokio::test]
    async fn sessions_persist_across_a_simulated_restart() {
        let dir = std::env::temp_dir().join(format!("anpot-nav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let url = format!("sqlite://{}", dir.join("bot.db").display());
        let l = Lifetimes::default();
        {
            let pool = crate::db::connect(&url).await.expect("connect");
            register(&pool, &new(77, Class::Default), &l, 1000)
                .await
                .expect("register");
            pool.close().await;
        }
        let pool = crate::db::connect(&url).await.expect("reconnect");
        let row = get(&pool, 77)
            .await
            .expect("get")
            .expect("the session survived");
        assert_eq!(row.expires_at, 1060);
        assert_eq!(
            due(&pool, 2000, 10).await.expect("due").len(),
            1,
            "and is due after the restart"
        );
    }
}
