//! The sweeper: every 30 seconds it greys out the components of messages whose time is up, silently (the content stays,
//! nothing new is sent). Public messages come from the `message_sessions` table; ephemeral ones from memory, with the
//! interaction token that is only valid for 15 minutes. It is gentle with Discord: four edits at a time, a pause
//! between edits, a back-off on 429, and a press that happened a moment earlier wins (the expiry is re-read first).

use super::{disable_components, is_v2, store, Class};
use futures::StreamExt;
use poise::serenity_prelude as serenity;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

pub const EVERY: Duration = Duration::from_secs(30);
pub const CONCURRENCY: usize = 4;
const PAUSE: Duration = Duration::from_millis(150);
const BATCH: i64 = 200;
/// An interaction token works for 15 minutes.
pub const TOKEN_LIFETIME_SECS: i64 = 15 * 60;

/// What happened when one message was edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edited {
    Done,
    /// Deleted, or the bot may no longer edit it (404 / 403): forget it.
    Gone,
    /// 429: try again after this many seconds.
    Limited(u64),
    /// Another failure: forget it too (the message is not worth retrying forever).
    Failed,
}

/// Something that can replace the components of a message. The real one talks to Discord; tests use a fake.
pub trait Editor: Send + Sync {
    fn disable(
        &self,
        target: Target,
        components: Vec<Value>,
        v2: bool,
    ) -> impl Future<Output = Edited> + Send;
}

/// Where a message lives.
#[derive(Debug, Clone)]
pub enum Target {
    Public {
        channel: i64,
        message: i64,
    },
    /// An ephemeral message: only its interaction token can edit it.
    Ephemeral {
        token: String,
        message: i64,
    },
}

/// The body of an edit that only changes components (embeds, text and attachments are not mentioned, so Discord keeps
/// them). Components V2 messages need their flag on every edit.
pub fn edit_body(components: Vec<Value>, v2: bool) -> Value {
    let mut body = json!({ "components": components });
    if v2 {
        body["flags"] = json!(crate::ui::v2::IS_COMPONENTS_V2);
    }
    body
}

pub struct DiscordEditor {
    pub http: Arc<serenity::Http>,
}

impl Editor for DiscordEditor {
    async fn disable(&self, target: Target, components: Vec<Value>, v2: bool) -> Edited {
        let body = edit_body(components, v2);
        let result = match target {
            Target::Public { channel, message } => self
                .http
                .edit_message(
                    serenity::ChannelId::new(channel as u64),
                    serenity::MessageId::new(message as u64),
                    &body,
                    Vec::new(),
                )
                .await
                .map(|_| ()),
            Target::Ephemeral { token, message } => self
                .http
                .edit_followup_message(
                    &token,
                    serenity::MessageId::new(message as u64),
                    &body,
                    Vec::new(),
                )
                .await
                .map(|_| ()),
        };
        match result {
            Ok(()) => Edited::Done,
            Err(serenity::Error::Http(e)) => match e.status_code().map(|s| s.as_u16()) {
                Some(404 | 403 | 401 | 10008) => Edited::Gone,
                Some(429) => Edited::Limited(2),
                _ => Edited::Failed,
            },
            Err(_) => Edited::Failed,
        }
    }
}

/// Ephemeral messages that are still active: their token, when they expire, what is on screen.
#[derive(Clone, Default)]
pub struct Ephemerals {
    inner: Arc<Mutex<HashMap<i64, Eph>>>,
}

#[derive(Debug, Clone)]
pub struct Eph {
    pub token: String,
    pub created: i64,
    pub expires_at: i64,
    pub hard_expires_at: i64,
    pub class: Class,
    pub components: Vec<Value>,
    pub v2: bool,
}

impl Ephemerals {
    pub fn insert(&self, message: i64, e: Eph) {
        if let Ok(mut m) = self.inner.lock() {
            m.insert(message, e);
        }
    }

    /// A press on an ephemeral message: slide its expiry and remember what is on screen.
    pub fn touch(
        &self,
        message: i64,
        now: i64,
        l: &super::Lifetimes,
        components: Vec<Value>,
        v2: bool,
    ) {
        if let Ok(mut m) = self.inner.lock() {
            if let Some(e) = m.get_mut(&message) {
                e.expires_at = l.extended(e.class, now, e.expires_at, e.hard_expires_at);
                e.components = components;
                e.v2 = v2;
            }
        }
    }

    pub fn forget(&self, message: i64) {
        if let Ok(mut m) = self.inner.lock() {
            m.remove(&message);
        }
    }

    pub fn contains(&self, message: i64) -> bool {
        self.inner.lock().is_ok_and(|m| m.contains_key(&message))
    }

    pub fn expiry(&self, message: i64) -> Option<i64> {
        self.inner.lock().ok()?.get(&message).map(|e| e.expires_at)
    }

    /// Ephemeral messages whose time is up, removed from memory. Those whose token is already dead are dropped
    /// without an edit (nothing can be done for them).
    pub fn take_due(&self, now: i64) -> (Vec<(i64, Eph)>, usize) {
        let Ok(mut m) = self.inner.lock() else {
            return (Vec::new(), 0);
        };
        let due: Vec<i64> = m
            .iter()
            .filter(|(_, e)| e.expires_at <= now)
            .map(|(k, _)| *k)
            .collect();
        let mut live = Vec::new();
        let mut dead = 0;
        for id in due {
            if let Some(e) = m.remove(&id) {
                if now - e.created < TOKEN_LIFETIME_SECS {
                    live.push((id, e));
                } else {
                    dead += 1;
                }
            }
        }
        live.sort_by_key(|(_, e)| e.expires_at);
        (live, dead)
    }
}

/// One pass over the public sessions that are due. Returns how many messages were edited.
pub async fn sweep_public<E: Editor>(
    pool: &SqlitePool,
    editor: &E,
    now: i64,
    pause: Duration,
) -> usize {
    let Ok(rows) = store::due(pool, now, BATCH).await else {
        return 0;
    };
    let edited = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let backoff = Arc::new(Mutex::new(0u64));
    futures::stream::iter(rows)
        .for_each_concurrent(CONCURRENCY, |row| {
            let (edited, backoff) = (edited.clone(), backoff.clone());
            async move {
                // A press may have extended the session since the batch was read: it wins.
                match store::get(pool, row.message_id).await {
                    Ok(Some(fresh)) if fresh.expires_at > now => return,
                    Ok(Some(_)) => {}
                    _ => return,
                }
                // After a 429 every worker waits before its next edit.
                let wait = backoff.lock().map(|b| *b).unwrap_or(0);
                if wait > 0 {
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                }
                let mut components: Vec<Value> =
                    serde_json::from_str(&row.components_json).unwrap_or_default();
                let v2 = row.v2 || is_v2(&components);
                components.iter_mut().for_each(disable_components);
                let target = Target::Public {
                    channel: row.channel_id,
                    message: row.message_id,
                };
                match editor.disable(target, components, v2).await {
                    Edited::Done => {
                        edited.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let _ = store::remove(pool, row.message_id).await;
                    }
                    Edited::Gone | Edited::Failed => {
                        let _ = store::remove(pool, row.message_id).await;
                    }
                    // Keep the row: the next pass tries again.
                    Edited::Limited(secs) => {
                        if let Ok(mut b) = backoff.lock() {
                            *b = (*b).max(secs.min(30));
                        }
                    }
                }
                tokio::time::sleep(pause).await;
            }
        })
        .await;
    edited.load(std::sync::atomic::Ordering::Relaxed)
}

/// One pass over the ephemeral messages that are due.
pub async fn sweep_ephemeral<E: Editor>(
    eph: &Ephemerals,
    editor: &E,
    now: i64,
    pause: Duration,
) -> usize {
    let (due, _dead) = eph.take_due(now);
    let edited = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    futures::stream::iter(due)
        .for_each_concurrent(CONCURRENCY, |(message, e)| {
            let edited = edited.clone();
            async move {
                let mut components = e.components.clone();
                components.iter_mut().for_each(disable_components);
                let target = Target::Ephemeral {
                    token: e.token.clone(),
                    message,
                };
                if editor.disable(target, components, e.v2).await == Edited::Done {
                    edited.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                tokio::time::sleep(pause).await;
            }
        })
        .await;
    edited.load(std::sync::atomic::Ordering::Relaxed)
}

/// Run the sweeper for the life of the bot.
pub fn spawn(http: Arc<serenity::Http>, pool: SqlitePool, eph: Ephemerals) {
    tokio::spawn(async move {
        let editor = DiscordEditor { http };
        loop {
            tokio::time::sleep(EVERY).await;
            let now = super::now();
            let n = sweep_public(&pool, &editor, now, PAUSE).await;
            let m = sweep_ephemeral(&eph, &editor, now, PAUSE).await;
            if n + m > 0 {
                tracing::debug!(public = n, ephemeral = m, "components disabled");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav::{store::NewSession, Lifetimes};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<(Target, Vec<Value>, bool)>>,
        running: AtomicUsize,
        peak: AtomicUsize,
        gone: Mutex<Vec<i64>>,
        limited_once: Mutex<Vec<i64>>,
    }

    impl Editor for Fake {
        async fn disable(&self, target: Target, components: Vec<Value>, v2: bool) -> Edited {
            let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(5)).await;
            self.running.fetch_sub(1, Ordering::SeqCst);
            let id = match &target {
                Target::Public { message, .. } | Target::Ephemeral { message, .. } => *message,
            };
            if self.gone.lock().expect("lock").contains(&id) {
                return Edited::Gone;
            }
            {
                let mut l = self.limited_once.lock().expect("lock");
                if let Some(pos) = l.iter().position(|x| *x == id) {
                    l.remove(pos);
                    return Edited::Limited(0);
                }
            }
            self.calls
                .lock()
                .expect("lock")
                .push((target, components, v2));
            Edited::Done
        }
    }

    async fn add(pool: &SqlitePool, id: i64, at: i64, components: &str, v2: bool) {
        let s = NewSession {
            message_id: id,
            channel_id: 9,
            guild_id: None,
            owner_id: Some(1),
            kind: "view",
            class: Class::Default,
            components_json: components,
            v2,
        };
        store::register(pool, &s, &Lifetimes::default(), at)
            .await
            .expect("register");
    }

    const CLASSIC: &str = r#"[{"type":1,"components":[{"type":2,"style":2,"custom_id":"a","label":"x"}]},{"type":1,"components":[{"type":3,"custom_id":"m","options":[]}]}]"#;
    const V2: &str = r#"[{"type":17,"components":[{"type":10,"content":"hi"},{"type":1,"components":[{"type":2,"style":2,"custom_id":"a"}]}]}]"#;

    #[tokio::test]
    async fn due_messages_are_disabled_with_only_components_in_the_edit_and_the_row_is_removed() {
        let pool = crate::db::connect_memory().await;
        add(&pool, 1, 0, CLASSIC, false).await;
        add(&pool, 2, 0, V2, true).await;
        add(&pool, 3, 10_000, CLASSIC, false).await; // not due
        let fake = Fake::default();
        let n = sweep_public(&pool, &fake, 1000, Duration::ZERO).await;
        assert_eq!(n, 2);
        let calls = fake.calls.lock().expect("lock").clone();
        let classic = calls
            .iter()
            .find(|(t, ..)| matches!(t, Target::Public { message: 1, .. }))
            .expect("classic");
        assert_eq!(classic.1[0]["components"][0]["disabled"], true);
        assert_eq!(classic.1[1]["components"][0]["disabled"], true);
        assert!(!classic.2);
        let v2 = calls
            .iter()
            .find(|(t, ..)| matches!(t, Target::Public { message: 2, .. }))
            .expect("v2");
        assert_eq!(v2.1[0]["components"][1]["components"][0]["disabled"], true);
        assert_eq!(
            v2.1[0]["components"][0]["content"], "hi",
            "the text is untouched"
        );
        assert!(v2.2, "a V2 edit carries the flag");
        assert!(store::get(&pool, 1).await.expect("get").is_none());
        assert!(
            store::get(&pool, 3).await.expect("get").is_some(),
            "a message that is not due is left alone"
        );
        let body = edit_body(vec![json!({"type": 1})], true);
        assert_eq!(body["flags"], crate::ui::v2::IS_COMPONENTS_V2);
        assert!(
            body.get("embeds").is_none() && body.get("content").is_none(),
            "no new text, no new embeds"
        );
    }

    #[tokio::test]
    async fn a_press_that_extended_the_session_wins_over_the_sweeper() {
        let pool = crate::db::connect_memory().await;
        add(&pool, 1, 930, CLASSIC, false).await; // would expire at 990
        let l = Lifetimes::default();
        store::touch(&pool, 1, &l, 985, "[]", CLASSIC, false)
            .await
            .expect("touch"); // a press: now 1045
        let fake = Fake::default();
        assert_eq!(sweep_public(&pool, &fake, 1000, Duration::ZERO).await, 0);
        assert!(fake.calls.lock().expect("lock").is_empty());
        assert!(store::get(&pool, 1).await.expect("get").is_some());
    }

    #[tokio::test]
    async fn a_deleted_message_is_skipped_and_forgotten_silently() {
        let pool = crate::db::connect_memory().await;
        add(&pool, 1, 0, CLASSIC, false).await;
        add(&pool, 2, 0, CLASSIC, false).await;
        let fake = Fake::default();
        fake.gone.lock().expect("lock").push(1);
        assert_eq!(sweep_public(&pool, &fake, 1000, Duration::ZERO).await, 1);
        assert!(
            store::get(&pool, 1).await.expect("get").is_none(),
            "the row of a deleted message is dropped"
        );
    }

    #[tokio::test]
    async fn a_rate_limit_keeps_the_row_for_the_next_pass() {
        let pool = crate::db::connect_memory().await;
        add(&pool, 1, 0, CLASSIC, false).await;
        let fake = Fake::default();
        fake.limited_once.lock().expect("lock").push(1);
        assert_eq!(sweep_public(&pool, &fake, 1000, Duration::ZERO).await, 0);
        assert!(
            store::get(&pool, 1).await.expect("get").is_some(),
            "kept for a retry"
        );
        assert_eq!(
            sweep_public(&pool, &fake, 1001, Duration::ZERO).await,
            1,
            "the retry succeeds"
        );
        assert!(store::get(&pool, 1).await.expect("get").is_none());
    }

    #[tokio::test]
    async fn a_burst_of_expirations_never_exceeds_the_concurrency_bound() {
        let pool = crate::db::connect_memory().await;
        for id in 1..=60 {
            add(&pool, id, 0, CLASSIC, false).await;
        }
        let fake = Fake::default();
        let n = sweep_public(&pool, &fake, 1000, Duration::ZERO).await;
        assert_eq!(n, 60);
        assert!(
            fake.peak.load(Ordering::SeqCst) <= CONCURRENCY,
            "peak {}",
            fake.peak.load(Ordering::SeqCst)
        );
        assert!(store::due(&pool, 1000, 100).await.expect("due").is_empty());
    }

    #[tokio::test]
    async fn expired_sessions_found_after_a_restart_are_swept_like_any_other() {
        let dir = std::env::temp_dir().join(format!("anpot-sweep-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let url = format!("sqlite://{}", dir.join("bot.db").display());
        {
            let pool = crate::db::connect(&url).await.expect("connect");
            add(&pool, 5, 0, CLASSIC, false).await;
            pool.close().await;
        }
        let pool = crate::db::connect(&url).await.expect("reconnect");
        let fake = Fake::default();
        assert_eq!(sweep_public(&pool, &fake, 5000, Duration::ZERO).await, 1);
    }

    fn eph(created: i64, expires: i64) -> Eph {
        Eph {
            token: "tok".into(),
            created,
            expires_at: expires,
            hard_expires_at: expires + 500,
            class: Class::Default,
            components: serde_json::from_str(CLASSIC).expect("json"),
            v2: false,
        }
    }

    #[tokio::test]
    async fn ephemeral_messages_are_disabled_through_their_token_unless_it_has_expired() {
        let e = Ephemerals::default();
        e.insert(1, eph(900, 960)); // token still valid
        e.insert(2, eph(0, 60)); // created over 15 minutes ago: nothing can be edited
        e.insert(3, eph(900, 5000)); // not due
        let fake = Fake::default();
        assert_eq!(sweep_ephemeral(&e, &fake, 1000, Duration::ZERO).await, 1);
        let calls = fake.calls.lock().expect("lock").clone();
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0].0, Target::Ephemeral { token, message: 1 } if token == "tok"));
        assert!(!e.contains(1) && !e.contains(2), "both are forgotten");
        assert!(e.contains(3));
    }

    #[test]
    fn a_press_slides_an_ephemeral_session() {
        let e = Ephemerals::default();
        e.insert(1, eph(900, 960));
        e.touch(1, 950, &Lifetimes::default(), Vec::new(), false);
        assert_eq!(e.expiry(1), Some(1010));
    }
}
