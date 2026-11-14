//! Autocomplete plumbing: minimum length, keystroke debounce, a 1 s per-user result cache
//! and a hard timeout that yields an empty list instead of an error.

use moka::future::Cache;
use poise::serenity_prelude as serenity;
use std::{
    collections::HashMap,
    future::Future,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

pub const MIN_CHARS: usize = 2;
pub const MAX_CHOICES: usize = 25;
const DEBOUNCE: Duration = Duration::from_millis(250);
/// Discord allows 3 s; stay below it.
const TIMEOUT: Duration = Duration::from_millis(2500);

type Key = (u64, &'static str);
/// `(label, value)` pairs.
pub type Items = Vec<(String, String)>;

pub struct AutoState {
    latest: Mutex<HashMap<Key, u64>>,
    counter: AtomicU64,
    recent: Cache<Key, (String, Arc<Items>)>,
    debounce: Duration,
    timeout: Duration,
}

impl Default for AutoState {
    fn default() -> Self {
        Self::with_timing(DEBOUNCE, TIMEOUT)
    }
}

impl AutoState {
    pub fn with_timing(debounce: Duration, timeout: Duration) -> Self {
        Self {
            latest: Mutex::new(HashMap::new()),
            counter: AtomicU64::new(0),
            recent: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(1))
                .build(),
            debounce,
            timeout,
        }
    }

    fn mark(&self, key: Key) -> u64 {
        let seq = self.counter.fetch_add(1, Ordering::Relaxed) + 1;
        if let Ok(mut m) = self.latest.lock() {
            m.insert(key, seq);
        }
        seq
    }

    fn is_latest(&self, key: Key, seq: u64) -> bool {
        self.latest
            .lock()
            .map(|m| m.get(&key) == Some(&seq))
            .unwrap_or(true)
    }

    /// Returns `(label, value)` pairs for `partial`, or an empty list when too short,
    /// superseded by a newer keystroke, timed out or failed.
    pub async fn items<F, Fut>(
        &self,
        user: u64,
        kind: &'static str,
        partial: &str,
        fetch: F,
    ) -> Items
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Items>,
    {
        let q = partial.trim();
        if q.chars().count() < MIN_CHARS {
            return Vec::new();
        }
        let key = (user, kind);
        if let Some((last_q, items)) = self.recent.get(&key).await {
            if last_q == q {
                return (*items).clone();
            }
        }
        let seq = self.mark(key);
        tokio::time::sleep(self.debounce).await;
        if !self.is_latest(key, seq) {
            return Vec::new();
        }
        let items = tokio::time::timeout(self.timeout, fetch(q.to_string()))
            .await
            .unwrap_or_default();
        self.recent
            .insert(key, (q.to_string(), Arc::new(items.clone())))
            .await;
        items
    }
}

pub fn to_choices(items: Items) -> Vec<serenity::AutocompleteChoice> {
    items
        .into_iter()
        .take(MAX_CHOICES)
        .map(|(label, value)| {
            serenity::AutocompleteChoice::new(
                super::embeds::fit(&label, 100),
                super::embeds::fit(&value, 100),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn state() -> AutoState {
        AutoState::with_timing(Duration::from_millis(30), Duration::from_millis(200))
    }

    fn one() -> Items {
        vec![("a".into(), "id:1".into())]
    }

    #[tokio::test]
    async fn short_input_is_empty_without_fetching() {
        let calls = AtomicUsize::new(0);
        let out = state()
            .items(1, "t", "a", |_| async {
                calls.fetch_add(1, Ordering::SeqCst);
                one()
            })
            .await;
        assert!(out.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn keystroke_burst_makes_one_upstream_call() {
        let s = state();
        let calls = AtomicUsize::new(0);
        let fetch = |_q: String| async {
            calls.fetch_add(1, Ordering::SeqCst);
            one()
        };
        let (a, b, c) = tokio::join!(
            s.items(1, "t", "na", fetch),
            async {
                tokio::time::sleep(Duration::from_millis(5)).await;
                s.items(1, "t", "nar", fetch).await
            },
            async {
                tokio::time::sleep(Duration::from_millis(10)).await;
                s.items(1, "t", "naru", fetch).await
            },
        );
        assert!(
            a.is_empty() && b.is_empty(),
            "superseded requests answer empty"
        );
        assert_eq!(c, one());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn same_query_is_served_from_the_one_second_cache() {
        let s = state();
        let calls = AtomicUsize::new(0);
        for _ in 0..3 {
            let out = s
                .items(1, "t", "naruto", |_| async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    one()
                })
                .await;
            assert_eq!(out, one());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn timeout_returns_empty_not_an_error() {
        let out = state()
            .items(1, "t", "naruto", |_| async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                one()
            })
            .await;
        assert!(out.is_empty());
    }

    #[test]
    fn choices_are_capped_and_truncated() {
        let items: Items = (0..40)
            .map(|i| ("x".repeat(300), format!("id:{i}")))
            .collect();
        let c = to_choices(items);
        assert_eq!(c.len(), MAX_CHOICES);
    }
}
